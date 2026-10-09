use crate::{DocumentId, Result, Revision, RevisionId};

/// Stores immutable revisions independently of workspace activation and synchronization.
pub trait KnowledgeRepository {
    fn load(&self, document: &DocumentId, revision: &RevisionId) -> Result<Revision>;
    fn record(&self, revision: &Revision) -> Result<RevisionId>;
}
