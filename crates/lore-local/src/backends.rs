use crate::{atomic_write, now_ms};
use lore_core::{
    BackendDocument, DocumentContent, DocumentListing, DocumentReference, Error, ErrorKind,
    KnowledgeBackend, Result, hash,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendConfig {
    Notion {
        roots: Vec<String>,
        token_env: String,
        api_url: String,
    },
}
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
    pub roots: Vec<DocumentReference>,
    pub documents: Vec<BackendDocument>,
    pub complete: bool,
    pub warnings: Vec<String>,
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
        let key = format!("children:{}", serde_json::to_string(reference)?);
        match backend {
            Some(backend) => {
                let listing = backend.children(reference)?;
                for document in &listing.documents {
                    self.store(
                        &format!("metadata:{}", serde_json::to_string(&document.reference)?),
                        document,
                    )?;
                }
                self.store(&key, listing)
            }
            None => self.cached(&key),
        }
    }
    pub fn metadata(
        &self,
        backend: Option<&dyn KnowledgeBackend>,
        reference: &DocumentReference,
    ) -> Result<Retrieved<BackendDocument>> {
        let key = format!("metadata:{}", serde_json::to_string(reference)?);
        match backend {
            Some(backend) => self.store(&key, backend.metadata(reference)?),
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
        roots: &[DocumentReference],
    ) -> Result<Retrieved<DocumentIndex>> {
        let listing = self.roots(Some(backend), roots)?.value;
        let mut warnings = listing.warnings;
        let mut documents: BTreeMap<_, _> = listing
            .documents
            .into_iter()
            .map(|d| (d.reference.clone(), d))
            .collect();
        let mut pending: VecDeque<_> = documents.keys().cloned().collect();
        let mut visited = BTreeSet::new();
        while let Some(reference) = pending.pop_front() {
            if !visited.insert(reference.clone()) {
                continue;
            }
            if visited.len() > 5000 {
                warnings.push("Document indexing limit reached".into());
                break;
            }
            match self.children(Some(backend), &reference) {
                Ok(listing) => {
                    warnings.extend(listing.value.warnings);
                    for document in listing.value.documents {
                        if !visited.contains(&document.reference) {
                            pending.push_back(document.reference.clone());
                        }
                        documents.insert(document.reference.clone(), document);
                    }
                }
                Err(e) => warnings.push(format!("Document {}: {e}", reference.id)),
            }
        }
        self.store(
            "index",
            DocumentIndex {
                roots: roots.to_vec(),
                documents: documents.into_values().collect(),
                complete: warnings.is_empty(),
                warnings,
            },
        )
    }
    pub fn search(
        &self,
        query: &str,
        roots: &[DocumentReference],
    ) -> Result<Retrieved<DocumentIndex>> {
        let mut index: Retrieved<DocumentIndex> = self.cached("index")?;
        if index.value.roots != roots {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "Configured roots differ from the cached index; run docs index again",
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
