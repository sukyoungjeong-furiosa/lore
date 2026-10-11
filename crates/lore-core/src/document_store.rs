use crate::{DocumentId, DocumentView, Error, Result, Revision, RevisionId};

/// Complete document ancestry, newest first.
#[derive(Clone, Debug)]
pub struct DocumentBundle {
    pub revisions: Vec<Revision>,
}

impl DocumentBundle {
    pub fn head(&self) -> Result<DocumentView> {
        let record = self
            .revisions
            .first()
            .ok_or_else(|| Error::invalid("Document history is empty"))?;
        Ok(DocumentView::new(record.id()?, record.clone()))
    }

    pub fn validate(&self) -> Result<()> {
        let head = self.head()?;
        if self.revisions.len() > 10000 {
            return Err(Error::invalid("Document history exceeds 10000 revisions"));
        }
        for (index, record) in self.revisions.iter().enumerate() {
            record.document.validate()?;
            let parent = self
                .revisions
                .get(index + 1)
                .map(Revision::id)
                .transpose()?;
            if record.document.id != head.document.id || record.parent != parent {
                return Err(Error::invalid(
                    "Document history has inconsistent identity or ancestry",
                ));
            }
        }
        Ok(())
    }
}

/// Committed document heads in one selected store and repository scope.
pub trait DocumentCommitStore {
    fn list(&self) -> Result<Vec<DocumentView>>;
    fn read(&self, id: &DocumentId, revision: Option<&RevisionId>) -> Result<DocumentBundle>;
    /// Commits only against the observed committed head, or an unused identity when absent.
    /// Repeating an already-current revision succeeds without advancing the head.
    fn commit(
        &self,
        expected: Option<&RevisionId>,
        bundle: &DocumentBundle,
    ) -> Result<DocumentView>;
}
