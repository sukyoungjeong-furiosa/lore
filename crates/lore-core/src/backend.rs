use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DocumentReference {
    pub backend: String,
    pub id: String,
}
impl DocumentReference {
    pub fn validate(&self) -> Result<()> {
        if self.backend.trim().is_empty() || self.id.trim().is_empty() {
            return Err(Error::invalid(
                "Document references require a backend and document identity",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackendDocument {
    pub reference: DocumentReference,
    pub title: String,
    pub url: String,
    pub observed_revision: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentListing {
    pub documents: Vec<BackendDocument>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentContent {
    pub document: BackendDocument,
    pub content: String,
    pub content_format: String,
    pub partial: bool,
    pub warnings: Vec<String>,
}

/// Accesses backend-owned documents without creating Lore-owned copies or mappings.
pub trait KnowledgeBackend {
    fn metadata(&self, reference: &DocumentReference) -> Result<BackendDocument>;
    /// Lists contained documents; warnings describe incomplete traversal.
    fn children(&self, reference: &DocumentReference) -> Result<DocumentListing>;
    fn read(&self, reference: &DocumentReference) -> Result<DocumentContent>;
}
