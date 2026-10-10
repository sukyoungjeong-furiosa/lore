use crate::{
    Error, MappingCatalog, MappingId, MappingStoreDescriptor, MappingStoreId, MappingView, Result,
    RevisionId,
};
use serde::{Deserialize, Serialize};

pub const MAX_SYNC_BYTES: usize = 16 * 1024 * 1024;

/// A repository view of the caller's accessible shared registrations and mappings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreCatalog {
    pub store: MappingStoreDescriptor,
    pub catalog: MappingCatalog,
}

impl StoreCatalog {
    pub fn validate(&self, store: &MappingStoreId, repository: &str) -> Result<()> {
        if self.store.id != *store || self.catalog.repository != repository {
            return Err(Error::invalid(
                "Shared catalog identity does not match the requested scope",
            ));
        }
        self.store.validate_catalog(&self.catalog)
    }
}

/// Complete ancestry, newest first. Captured code is included; document bodies are not.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingBundle {
    pub store: MappingStoreId,
    pub revisions: Vec<MappingView>,
}

impl MappingBundle {
    pub fn head(&self) -> Result<&MappingView> {
        self.revisions
            .first()
            .ok_or_else(|| Error::invalid("Mapping history is empty"))
    }

    pub fn validate(&self) -> Result<()> {
        let head = self.head()?;
        let repository = head.record.mapping.code_repository()?;
        if self.revisions.len() > 10000 {
            return Err(Error::invalid("Mapping history exceeds 10000 revisions"));
        }
        for (index, view) in self.revisions.iter().enumerate() {
            view.record.mapping.validate()?;
            if view.record.id()? != view.revision
                || view.record.mapping.id != head.record.mapping.id
                || view.record.mapping.code_repository()? != repository
                || view.record.parent.as_ref()
                    != self.revisions.get(index + 1).map(|next| &next.revision)
            {
                return Err(Error::invalid(
                    "Mapping history has inconsistent identity or ancestry",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishMapping {
    pub operation: String,
    pub expected: Option<RevisionId>,
    pub bundle: MappingBundle,
}

impl PublishMapping {
    pub fn validate(&self) -> Result<()> {
        uuid::Uuid::parse_str(&self.operation)
            .map_err(|_| Error::invalid("Invalid publication operation ID"))?;
        self.bundle.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationState {
    NotRequired,
    Pending,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicationReceipt {
    pub store: MappingStoreId,
    pub operation: String,
    pub mapping: MappingId,
    pub revision: RevisionId,
    pub reconciliation: ReconciliationState,
}

/// Shared mapping access is independent of document retrieval and local workspace activation.
pub trait MappingStoreClient {
    fn descriptor(&self) -> Result<MappingStoreDescriptor>;
    fn catalog(&self, repository: &str) -> Result<StoreCatalog>;
    fn read(&self, mapping: &MappingId, revision: Option<&RevisionId>) -> Result<MappingBundle>;
    fn publish(&self, request: &PublishMapping) -> Result<PublicationReceipt>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum StoreRequest {
    Describe {
        store: MappingStoreId,
    },
    Catalog {
        store: MappingStoreId,
        repository: String,
    },
    Read {
        store: MappingStoreId,
        mapping: MappingId,
        revision: Option<RevisionId>,
    },
    Publish {
        request: PublishMapping,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum StoreResponse {
    Descriptor { descriptor: MappingStoreDescriptor },
    Catalog { catalog: StoreCatalog },
    Mapping { bundle: MappingBundle },
    Published { receipt: PublicationReceipt },
}
