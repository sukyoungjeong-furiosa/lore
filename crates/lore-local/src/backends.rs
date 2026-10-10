use crate::{atomic_write, now_ms};
use lore_core::{
    BackendDocument, DocumentContent, DocumentListing, DocumentReference, Error, ErrorKind,
    KnowledgeBackend, Result, hash,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendConfig {
    Notion {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        roots: Vec<String>,
        #[serde(default)]
        discovery_pages: Vec<String>,
        token_env: String,
        api_url: String,
    },
}

/// Personal document access settings, independent of a store's shared backend registrations.
pub struct BackendRegistry {
    directory: PathBuf,
}

impl BackendRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            directory: home.join("backends"),
        }
    }

    pub fn list(&self) -> Result<BTreeMap<String, BackendConfig>> {
        match std::fs::read(self.directory.join("config.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get(&self, name: &str) -> Result<BackendConfig> {
        self.list()?
            .remove(name)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Backend is not configured"))
    }

    pub fn add(&self, name: &str, config: BackendConfig) -> Result<()> {
        if name.is_empty()
            || name.len() > 100
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Error::invalid(
                "Backend identity requires ASCII letters, digits, hyphens, or underscores",
            ));
        }
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.directory.join("config.lock"))?;
        lock.lock()?;
        let mut configs = self.list()?;
        if configs.contains_key(name) {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Backend identity already configured; remove it explicitly before replacing configuration",
            ));
        }
        configs.insert(name.into(), config);
        atomic_write(
            &self.directory.join("config.json"),
            &serde_json::to_vec_pretty(&configs)?,
        )
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.directory.join("config.lock"))?;
        lock.lock()?;
        let mut configs = self.list()?;
        if configs.remove(name).is_none() {
            return Err(Error::new(ErrorKind::NotFound, "Backend is not configured"));
        }
        atomic_write(
            &self.directory.join("config.json"),
            &serde_json::to_vec_pretty(&configs)?,
        )
    }

    /// Changes the connection's display name without changing document references.
    pub fn set_display_name(&self, backend: &str, name: String) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err(Error::invalid(
                "Display name must be nonempty text without control characters",
            ));
        }
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("config.lock"))?;
        lock.lock()?;
        let mut configs = self.list()?;
        let config = configs
            .get_mut(backend)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Backend is not configured"))?;
        let BackendConfig::Notion { display_name, .. } = config;
        *display_name = Some(name.to_owned());
        atomic_write(
            &self.directory.join("config.json"),
            &serde_json::to_vec_pretty(&configs)?,
        )
    }

    pub fn set_child_discovery(&self, name: &str, page: String, enabled: bool) -> Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("config.lock"))?;
        lock.lock()?;
        let mut configs = self.list()?;
        let config = configs
            .get_mut(name)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Backend is not configured"))?;
        let BackendConfig::Notion {
            discovery_pages, ..
        } = config;
        discovery_pages.retain(|id| id != &page);
        if enabled {
            discovery_pages.push(page);
            discovery_pages.sort();
        }
        atomic_write(
            &self.directory.join("config.json"),
            &serde_json::to_vec_pretty(&configs)?,
        )
    }

    pub fn cache(&self, name: &str) -> BackendCache {
        BackendCache {
            directory: self.directory.join("cache").join(hash(name)),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Retrieved<T> {
    pub cached: bool,
    pub fetched_at_ms: u64,
    pub value: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentIndex {
    pub scope: DiscoveryScope,
    pub documents: Vec<BackendDocument>,
    pub complete: bool,
    pub warnings: Vec<String>,
}

/// Entry pages and pages explicitly selected for direct-child discovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryScope {
    pub roots: Vec<DocumentReference>,
    pub discovery_pages: Vec<DocumentReference>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum IndexProgress {
    Waiting,
    Starting,
    Traversing {
        processed: usize,
        documents: usize,
        remaining: usize,
        title: String,
    },
    Request {
        requests: u64,
    },
    Cached {
        documents: usize,
    },
    Finished {
        documents: usize,
        complete: bool,
    },
}

pub struct BackendCache {
    directory: PathBuf,
}

impl BackendCache {
    fn file(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{}.json", hash(key)))
    }

    fn store<T: Serialize>(&self, key: &str, value: T) -> Result<Retrieved<T>> {
        let retrieved = Retrieved {
            cached: false,
            fetched_at_ms: now_ms(),
            value,
        };
        atomic_write(&self.file(key), &serde_json::to_vec_pretty(&retrieved)?)?;
        Ok(retrieved)
    }

    fn cached<T: DeserializeOwned>(&self, key: &str) -> Result<Retrieved<T>> {
        let bytes = std::fs::read(self.file(key)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    ErrorKind::NotCached,
                    "Not available in cache; fetch while connected first",
                )
            } else {
                e.into()
            }
        })?;
        let mut result: Retrieved<T> = serde_json::from_slice(&bytes)?;
        result.cached = true;
        Ok(result)
    }

    pub fn roots(
        &self,
        backend: Option<&dyn KnowledgeBackend>,
        references: &[DocumentReference],
    ) -> Result<Retrieved<DocumentListing>> {
        let key = format!("roots:{}", serde_json::to_string(references)?);
        let Some(backend) = backend else {
            return self.cached(&key);
        };
        let mut documents = vec![];
        let mut warnings = vec![];
        for reference in references {
            match backend.metadata(reference) {
                Ok(document) => {
                    self.cache_title(&document)?;
                    self.store(
                        &format!("metadata:{}", serde_json::to_string(&document.reference)?),
                        &document,
                    )?;
                    documents.push(document);
                }
                Err(e) => warnings.push(format!("Root {}: {e}", reference.id)),
            }
        }
        if documents.is_empty() && !warnings.is_empty() {
            return Err(Error::new(ErrorKind::Unavailable, warnings.join("; ")));
        }
        self.store(
            &key,
            DocumentListing {
                documents,
                warnings,
            },
        )
    }

    pub fn children(
        &self,
        backend: Option<&dyn KnowledgeBackend>,
        reference: &DocumentReference,
    ) -> Result<Retrieved<DocumentListing>> {
        let key = format!(
            "children:discovery-v1:{}",
            serde_json::to_string(reference)?
        );
        match backend {
            Some(backend) => {
                let listing = backend.children(reference)?;
                for document in &listing.documents {
                    self.cache_title(document)?;
                }
                self.store(&key, listing)
            }
            None => self.cached(&key),
        }
    }

    fn cache_title(&self, document: &BackendDocument) -> Result<()> {
        self.store(
            &format!("title:{}", serde_json::to_string(&document.reference)?),
            &document.title,
        )?;
        Ok(())
    }

    /// Returns an observed title without retrieving document metadata or content.
    pub fn cached_title(&self, reference: &DocumentReference) -> Result<String> {
        match self.cached::<String>(&format!("title:{}", serde_json::to_string(reference)?)) {
            Ok(title) => Ok(title.value),
            Err(error) if error.kind == ErrorKind::NotCached => {
                Ok(self.metadata(None, reference)?.value.title)
            }
            Err(error) => Err(error),
        }
    }

    pub fn metadata(
        &self,
        backend: Option<&dyn KnowledgeBackend>,
        reference: &DocumentReference,
    ) -> Result<Retrieved<BackendDocument>> {
        let key = format!("metadata:{}", serde_json::to_string(reference)?);
        match backend {
            Some(backend) => {
                let document = backend.metadata(reference)?;
                self.cache_title(&document)?;
                self.store(&key, document)
            }
            None => self.cached(&key),
        }
    }

    pub fn read(
        &self,
        backend: Option<&dyn KnowledgeBackend>,
        reference: &DocumentReference,
    ) -> Result<Retrieved<DocumentContent>> {
        let key = format!("content:{}", serde_json::to_string(reference)?);
        match backend {
            Some(backend) => {
                let content = backend.read(reference)?;
                self.cache_title(&content.document)?;
                self.store(
                    &format!(
                        "metadata:{}",
                        serde_json::to_string(&content.document.reference)?
                    ),
                    &content.document,
                )?;
                self.store(&key, content)
            }
            None => self.cached(&key),
        }
    }

    pub fn index(
        &self,
        backend: &dyn KnowledgeBackend,
        scope: &DiscoveryScope,
    ) -> Result<Retrieved<DocumentIndex>> {
        self.index_with_progress(backend, scope, |_| {})
    }

    fn index_with_progress(
        &self,
        backend: &dyn KnowledgeBackend,
        scope: &DiscoveryScope,
        mut progress: impl FnMut(IndexProgress),
    ) -> Result<Retrieved<DocumentIndex>> {
        progress(IndexProgress::Starting);
        let listing = self.roots(Some(backend), &scope.roots)?.value;
        let mut warnings = listing.warnings;
        let mut documents: BTreeMap<_, _> = listing
            .documents
            .into_iter()
            .map(|d| (d.reference.clone(), d))
            .collect();
        let pages: BTreeSet<_> = scope.discovery_pages.iter().cloned().collect();
        for reference in &pages {
            if scope.roots.contains(reference) {
                continue;
            }
            match self.metadata(Some(backend), reference) {
                Ok(document) => {
                    documents.insert(reference.clone(), document.value);
                }
                Err(error) => warnings.push(format!("Document {}: {error}", reference.id)),
            }
        }
        for (processed, reference) in pages.iter().enumerate() {
            progress(IndexProgress::Traversing {
                processed,
                documents: documents.len(),
                remaining: pages.len() - processed - 1,
                title: documents
                    .get(reference)
                    .map(|d| d.title.clone())
                    .unwrap_or_default(),
            });
            match self.children(Some(backend), reference) {
                Ok(listing) => {
                    warnings.extend(listing.value.warnings);
                    for document in listing.value.documents {
                        documents
                            .entry(document.reference.clone())
                            .or_insert(document);
                    }
                }
                Err(e) => warnings.push(format!("Document {}: {e}", reference.id)),
            }
        }
        let index = self.store(
            "index:discovery-v1",
            DocumentIndex {
                scope: scope.clone(),
                documents: documents.into_values().collect(),
                complete: warnings.is_empty(),
                warnings,
            },
        )?;
        progress(IndexProgress::Finished {
            documents: index.value.documents.len(),
            complete: index.value.complete,
        });
        Ok(index)
    }

    /// Refreshes shared title metadata, rechecking freshness under a backend-wide lock.
    pub fn fetch_index<B: KnowledgeBackend>(
        &self,
        scope: &DiscoveryScope,
        max_age_seconds: Option<u64>,
        backend: impl FnOnce() -> Result<B>,
    ) -> Result<Retrieved<DocumentIndex>> {
        self.fetch_index_with_progress(scope, max_age_seconds, backend, |_| {})
    }

    /// Reuses recent partial observations and coalesces refreshes waiting on the same backend.
    /// Warnings and completeness remain available in the returned index.
    pub fn fetch_index_with_progress<B: KnowledgeBackend>(
        &self,
        scope: &DiscoveryScope,
        max_age_seconds: Option<u64>,
        backend: impl FnOnce() -> Result<B>,
        mut progress: impl FnMut(IndexProgress),
    ) -> Result<Retrieved<DocumentIndex>> {
        let previous = self.search("", scope).ok();
        if let Some(index) = previous.as_ref()
            && max_age_seconds.is_some_and(|age| {
                now_ms().saturating_sub(index.fetched_at_ms) < age.saturating_mul(1000)
            })
        {
            progress(IndexProgress::Cached {
                documents: index.value.documents.len(),
            });
            return Ok(index.clone());
        }
        let observed = previous.map(|index| index.fetched_at_ms);
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("index-discovery-v1.lock"))?;
        progress(IndexProgress::Waiting);
        lock.lock()?;
        if let Ok(index) = self.search("", scope)
            && (Some(index.fetched_at_ms) != observed
                || max_age_seconds.is_some_and(|age| {
                    now_ms().saturating_sub(index.fetched_at_ms) < age.saturating_mul(1000)
                }))
        {
            progress(IndexProgress::Cached {
                documents: index.value.documents.len(),
            });
            return Ok(index);
        }
        self.index_with_progress(&backend()?, scope, progress)
    }

    pub fn search(&self, query: &str, scope: &DiscoveryScope) -> Result<Retrieved<DocumentIndex>> {
        let mut index: Retrieved<DocumentIndex> = self.cached("index:discovery-v1")?;
        if index.value.scope != *scope {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "Discovery settings differ from the cached index; refresh the document list",
            ));
        }
        let terms: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        index.value.documents.retain(|document| {
            terms
                .iter()
                .all(|term| document.title.to_lowercase().contains(term))
        });
        Ok(index)
    }
}
