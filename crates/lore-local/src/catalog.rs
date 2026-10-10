use crate::{BackendRegistry, SharedBaseline, Workspace, atomic_write, now_ms};
use lore_core::{
    CatalogEntry, Error, ErrorKind, MappingCatalog, MappingId, MappingSummaryView, Result,
    RevisionId, hash,
};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

/// Connects to read-only mapping discovery metadata, independently of document access or publication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogConnection {
    pub url: String,
    pub token_env: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogObservation {
    pub source: String,
    pub fetched_at_ms: u64,
    pub catalog: MappingCatalog,
}

pub struct CatalogStore {
    directory: PathBuf,
    repository: String,
}

impl CatalogStore {
    pub fn new(home: &Path, repository: &str) -> Self {
        Self {
            directory: home.join("catalogs").join(hash(repository)),
            repository: repository.into(),
        }
    }

    fn lock(&self) -> Result<std::fs::File> {
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("catalog.lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    pub fn connection(&self) -> Result<Option<CatalogConnection>> {
        match std::fs::read(self.directory.join("source.json")) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn configure(&self, source: Option<&CatalogConnection>) -> Result<()> {
        if let Some(source) = source {
            let url = reqwest::Url::parse(&source.url)
                .map_err(|_| Error::invalid("Invalid mapping catalog URL"))?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(Error::invalid(
                    "Mapping catalogs use HTTP(S) without embedded credentials",
                ));
            }
            if let Some(name) = &source.token_env
                && (name.is_empty()
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
            {
                return Err(Error::invalid(
                    "Invalid mapping catalog token environment variable",
                ));
            }
        }
        let _lock = self.lock()?;
        let path = self.directory.join("source.json");
        if let Some(source) = source {
            atomic_write(&path, &serde_json::to_vec_pretty(source)?)?;
        } else if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn read(&self, cached: bool) -> Result<CatalogObservation> {
        match self.connection()? {
            None => self.local(),
            Some(source) => match self.cached(&source.url) {
                Ok(observation) => Ok(observation),
                Err(error) if error.kind == ErrorKind::NotCached && !cached => self.fetch(None),
                Err(error) => Err(error),
            },
        }
    }

    fn local(&self) -> Result<CatalogObservation> {
        let catalog = match std::fs::read(self.directory.join("published.json")) {
            Ok(bytes) => serde_json::from_slice::<MappingCatalog>(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                MappingCatalog::empty(&self.repository)
            }
            Err(error) => return Err(error.into()),
        };
        catalog.validate(&self.repository)?;
        Ok(CatalogObservation {
            source: "local".into(),
            fetched_at_ms: now_ms(),
            catalog,
        })
    }

    fn cached(&self, url: &str) -> Result<CatalogObservation> {
        let bytes = std::fs::read(self.directory.join("remote-cache.json")).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    ErrorKind::NotCached,
                    "Mapping catalog is not cached; run lore map fetch",
                )
            } else {
                error.into()
            }
        })?;
        let observation: CatalogObservation = serde_json::from_slice(&bytes)?;
        if observation.source != url {
            return Err(Error::new(
                ErrorKind::NotCached,
                "Mapping catalog connection changed; fetch its index",
            ));
        }
        observation.catalog.validate(&self.repository)?;
        Ok(observation)
    }

    /// Refreshes discovery metadata without activating revisions or changing workspace baselines.
    pub fn fetch(&self, max_age_seconds: Option<u64>) -> Result<CatalogObservation> {
        let _lock = self.lock()?;
        let Some(source) = self.connection()? else {
            return self.local();
        };
        if let Some(age) = max_age_seconds
            && let Ok(cached) = self.cached(&source.url)
            && now_ms().saturating_sub(cached.fetched_at_ms) < age.saturating_mul(1000)
        {
            return Ok(cached);
        }
        let unavailable = || {
            Error::new(
                ErrorKind::Unavailable,
                "Mapping catalog retrieval failed; previous cache is preserved",
            )
        };
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable())?;
        let mut request = client.get(&source.url);
        if let Some(name) = &source.token_env {
            let token = std::env::var(name).map_err(|_| {
                Error::new(
                    ErrorKind::PermissionDenied,
                    format!("Set {name} for this mapping catalog"),
                )
            })?;
            request = request.bearer_auth(token);
        }
        let response = request.send().map_err(|_| unavailable())?;
        if !response.status().is_success() {
            return Err(unavailable());
        }
        let mut bytes = vec![];
        response.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::invalid("Mapping catalog exceeds 8 MB"));
        }
        let catalog: MappingCatalog = serde_json::from_slice(&bytes)?;
        catalog.validate(&self.repository)?;
        let observation = CatalogObservation {
            source: source.url,
            fetched_at_ms: now_ms(),
            catalog,
        };
        atomic_write(
            &self.directory.join("remote-cache.json"),
            &serde_json::to_vec(&observation)?,
        )?;
        Ok(observation)
    }

    fn publish(&self, entry: CatalogEntry, expected: Option<&RevisionId>) -> Result<()> {
        let _lock = self.lock()?;
        if self.connection()?.is_some() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "HTTP mapping catalogs are read-only; publication is supported only by the local catalog",
            ));
        }
        let mut catalog = self.local()?.catalog;
        let id = &entry.view.record.mapping.id;
        let current = catalog.mappings.get(id).map(|entry| &entry.view.revision);
        if current == Some(&entry.view.revision) {
            return Ok(());
        }
        if current != expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Published mapping changed; provide its observed revision with --if-shared",
            ));
        }
        catalog.mappings.insert(id.clone(), entry);
        catalog.validate(&self.repository)?;
        atomic_write(
            &self.directory.join("published.json"),
            &serde_json::to_vec(&catalog)?,
        )
    }
}

impl Workspace {
    /// Publishes a local mapping to the local shared discovery catalog; does not publish its document.
    pub fn publish_mapping(
        &self,
        home: &Path,
        id: &MappingId,
        expected_local: &RevisionId,
        expected_shared: Option<&RevisionId>,
    ) -> Result<MappingSummaryView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let active = state.mappings.get_mut(id).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Mapping is not active in this workspace",
            )
        })?;
        if active.revision != *expected_local {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Local mapping changed before publication",
            ));
        }
        let view = MappingSummaryView::from(self.read_mapping(id, None)?);
        let title = match BackendRegistry::new(home)
            .cache(&view.record.mapping.document.backend)
            .cached_title(&view.record.mapping.document)
        {
            Ok(title) => Some(title),
            Err(error) if error.kind == ErrorKind::NotCached => None,
            Err(error) => return Err(error),
        };
        CatalogStore::new(home, &self.code.repository_id).publish(
            CatalogEntry {
                view: view.clone(),
                title,
            },
            expected_shared,
        )?;
        active.shared_baseline = Some(SharedBaseline {
            source: "local-catalog".into(),
            revision: view.revision.to_string(),
        });
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(view)
    }
}
