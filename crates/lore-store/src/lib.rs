use lore_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};

mod documents;
mod knowledge;
pub use documents::FileDocumentStore;
pub use knowledge::{GitKnowledgeStorage, GitKnowledgeStore};

/// Storage implementations atomically commit a head and its idempotency receipt.
pub trait SharedStore: Send + Sync {
    fn catalog(
        &self,
        descriptor: &KnowledgeStoreDescriptor,
        repository: &str,
    ) -> Result<MappingCatalog>;
    fn read(
        &self,
        store: &KnowledgeStoreId,
        mapping: &MappingId,
        revision: Option<&RevisionId>,
    ) -> Result<MappingBundle>;
    fn publish(
        &self,
        descriptor: &KnowledgeStoreDescriptor,
        subject: &str,
        request: &PublishMapping,
    ) -> Result<PublicationReceipt>;
}

pub struct FileSharedStore {
    pub directory: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
struct StoreState {
    heads: BTreeMap<MappingId, RevisionId>,
    revisions: BTreeMap<MappingId, BTreeSet<String>>,
    operations: BTreeMap<String, RecordedPublication>,
}

#[derive(Serialize, Deserialize)]
struct RecordedPublication {
    subject: String,
    digest: String,
    receipt: PublicationReceipt,
}

fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Missing storage directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|error| Error::from(error.error))?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

impl FileSharedStore {
    fn directory(&self, store: &KnowledgeStoreId) -> PathBuf {
        self.directory.join(hash(store.as_str()))
    }

    fn state(&self, store: &KnowledgeStoreId) -> Result<StoreState> {
        match std::fs::read(self.directory(store).join("state.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(StoreState::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn revision_path(
        &self,
        store: &KnowledgeStoreId,
        mapping: &MappingId,
        revision: &RevisionId,
    ) -> PathBuf {
        self.directory(store)
            .join("revisions")
            .join(mapping.as_str())
            .join(format!("{revision}.json"))
    }

    fn revision(
        &self,
        store: &KnowledgeStoreId,
        mapping: &MappingId,
        revision: &RevisionId,
    ) -> Result<MappingView> {
        let bytes =
            std::fs::read(self.revision_path(store, mapping, revision)).map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    Error::new(
                        ErrorKind::NotFound,
                        "Shared mapping revision is unavailable",
                    )
                } else {
                    error.into()
                }
            })?;
        let record: MappingRevision = serde_json::from_slice(&bytes)?;
        record.mapping.validate()?;
        if record.mapping.id != *mapping || record.id()? != *revision {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Shared mapping revision failed its identity check",
            ));
        }
        Ok(MappingView {
            revision: revision.clone(),
            record,
        })
    }
}

impl SharedStore for FileSharedStore {
    fn catalog(
        &self,
        descriptor: &KnowledgeStoreDescriptor,
        repository: &str,
    ) -> Result<MappingCatalog> {
        let mut catalog = MappingCatalog::empty(repository);
        for (id, revision) in self.state(&descriptor.id)?.heads {
            let view = self.revision(&descriptor.id, &id, &revision)?;
            if view.record.mapping.code_repository()? == repository {
                catalog.mappings.insert(
                    id,
                    CatalogEntry {
                        view: view.into(),
                        title: None,
                    },
                );
            }
        }
        Ok(catalog)
    }

    fn read(
        &self,
        store: &KnowledgeStoreId,
        mapping: &MappingId,
        revision: Option<&RevisionId>,
    ) -> Result<MappingBundle> {
        let state = self.state(store)?;
        let head = state
            .heads
            .get(mapping)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Shared mapping does not exist"))?;
        if revision.is_some_and(|revision| {
            !state
                .revisions
                .get(mapping)
                .is_some_and(|revisions| revisions.contains(revision.as_str()))
        }) {
            return Err(Error::new(
                ErrorKind::NotFound,
                "Shared revision was not published",
            ));
        }
        let mut next = Some(revision.unwrap_or(head).clone());
        let mut revisions = vec![];
        while let Some(revision) = next {
            if revisions.len() >= 10000 {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Shared mapping ancestry exceeds its limit",
                ));
            }
            let view = self.revision(store, mapping, &revision)?;
            next = view.record.parent.clone();
            revisions.push(view);
        }
        let bundle = MappingBundle {
            store: store.clone(),
            revisions,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    fn publish(
        &self,
        descriptor: &KnowledgeStoreDescriptor,
        subject: &str,
        request: &PublishMapping,
    ) -> Result<PublicationReceipt> {
        request.validate()?;
        if request.bundle.store != descriptor.id {
            return Err(Error::invalid(
                "Publication targets a different mapping store",
            ));
        }
        for view in &request.bundle.revisions {
            descriptor.validate_mapping(&view.record.mapping)?;
        }
        let directory = self.directory(&descriptor.id);
        std::fs::create_dir_all(&directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("store.lock"))?;
        lock.lock()?;
        let mut state = self.state(&descriptor.id)?;
        let digest = hash(serde_json::to_vec(request)?);
        if let Some(previous) = state.operations.get(&request.operation) {
            if previous.subject != subject || previous.digest != digest {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Publication operation was reused with a different subject or payload",
                ));
            }
            return Ok(previous.receipt.clone());
        }
        let head = request.bundle.head()?;
        let mapping = &head.record.mapping;
        if state.heads.get(&mapping.id) != request.expected.as_ref() {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Shared mapping changed; pull and inspect the conflicting revisions",
            ));
        }
        if let Some(expected) = &request.expected
            && !request
                .bundle
                .revisions
                .iter()
                .any(|view| view.revision == *expected)
        {
            return Err(Error::invalid(
                "Publication history does not include the shared baseline",
            ));
        }
        for view in &request.bundle.revisions {
            let path = self.revision_path(&descriptor.id, &mapping.id, &view.revision);
            if path.exists() {
                self.revision(&descriptor.id, &mapping.id, &view.revision)?;
            } else {
                atomic(&path, &serde_json::to_vec(&view.record)?)?;
            }
        }
        let receipt = PublicationReceipt {
            store: descriptor.id.clone(),
            operation: request.operation.clone(),
            mapping: mapping.id.clone(),
            revision: head.revision.clone(),
            reconciliation: if mapping.anchors.is_empty()
                || descriptor.repositories[mapping.code_repository()?]
                    .canonical_ref
                    .is_none()
            {
                ReconciliationState::NotRequired
            } else {
                ReconciliationState::Pending
            },
        };
        state
            .heads
            .insert(mapping.id.clone(), head.revision.clone());
        state
            .revisions
            .entry(mapping.id.clone())
            .or_default()
            .extend(
                request
                    .bundle
                    .revisions
                    .iter()
                    .map(|view| view.revision.to_string()),
            );
        state.operations.insert(
            request.operation.clone(),
            RecordedPublication {
                subject: subject.into(),
                digest,
                receipt: receipt.clone(),
            },
        );
        atomic(&directory.join("state.json"), &serde_json::to_vec(&state)?)?;
        Ok(receipt)
    }
}
