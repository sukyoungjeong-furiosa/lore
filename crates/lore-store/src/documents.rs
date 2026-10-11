use crate::atomic;
use lore_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub struct FileDocumentStore {
    pub directory: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
struct DocumentState {
    heads: BTreeMap<DocumentId, RevisionId>,
    revisions: BTreeMap<DocumentId, BTreeSet<String>>,
}

impl FileDocumentStore {
    pub fn state_path(&self) -> PathBuf {
        self.directory.join("state.json")
    }

    fn state(&self) -> Result<DocumentState> {
        match std::fs::read(self.state_path()) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(DocumentState::default())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn revision_path(&self, id: &DocumentId, revision: &RevisionId) -> PathBuf {
        self.directory
            .join("revisions")
            .join(id.as_str())
            .join(format!("{revision}.json"))
    }

    fn revision(&self, id: &DocumentId, revision: &RevisionId) -> Result<Revision> {
        let bytes = std::fs::read(self.revision_path(id, revision)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(ErrorKind::Corrupt, "Committed document revision is missing")
            } else {
                error.into()
            }
        })?;
        let record: Revision = serde_json::from_slice(&bytes)?;
        record.document.validate()?;
        if record.document.id != *id || record.id()? != *revision {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Committed document revision failed its identity check",
            ));
        }
        Ok(record)
    }
}

impl DocumentCommitStore for FileDocumentStore {
    fn list(&self) -> Result<Vec<DocumentView>> {
        self.state()?
            .heads
            .into_iter()
            .map(|(id, revision)| {
                Ok(DocumentView::new(
                    revision.clone(),
                    self.revision(&id, &revision)?,
                ))
            })
            .collect()
    }

    fn read(&self, id: &DocumentId, revision: Option<&RevisionId>) -> Result<DocumentBundle> {
        let state = self.state()?;
        let head = state
            .heads
            .get(id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Document has not been committed"))?;
        let selected = revision.unwrap_or(head);
        if !state
            .revisions
            .get(id)
            .is_some_and(|revisions| revisions.contains(selected.as_str()))
        {
            return Err(Error::new(
                ErrorKind::NotFound,
                "Document revision has not been committed",
            ));
        }
        let mut revisions = vec![];
        let mut next = Some(selected.clone());
        while let Some(revision) = next {
            if revisions.len() >= 10000 {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Committed document ancestry exceeds its limit",
                ));
            }
            let record = self.revision(id, &revision)?;
            next = record.parent.clone();
            revisions.push(record);
        }
        let bundle = DocumentBundle { revisions };
        bundle.validate()?;
        Ok(bundle)
    }

    fn commit(
        &self,
        expected: Option<&RevisionId>,
        bundle: &DocumentBundle,
    ) -> Result<DocumentView> {
        bundle.validate()?;
        let head = bundle.head()?;
        let ancestors = bundle
            .revisions
            .iter()
            .map(Revision::id)
            .collect::<Result<Vec<_>>>()?;
        if let Some(expected) = expected
            && !ancestors.contains(expected)
        {
            return Err(Error::invalid(
                "Committed document must retain its observed shared ancestor",
            ));
        }
        std::fs::create_dir_all(&self.directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join("store.lock"))?;
        lock.lock()?;
        let mut state = self.state()?;
        let current = state.heads.get(&head.document.id);
        if current == Some(&head.revision) {
            // The head and ancestry are durable even if the workspace baseline write was interrupted.
            self.revision(&head.document.id, &head.revision)?;
            return Ok(head);
        }
        if current != expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Committed document changed; inspect show --committed and resolve-conflict explicitly",
            ));
        }
        let published = state.revisions.entry(head.document.id.clone()).or_default();
        for record in &bundle.revisions {
            let revision = record.id()?;
            let path = self.revision_path(&head.document.id, &revision);
            if path.exists() {
                self.revision(&head.document.id, &revision)?;
            } else {
                atomic(&path, &serde_json::to_vec(record)?)?;
            }
            published.insert(revision.to_string());
        }
        state
            .heads
            .insert(head.document.id.clone(), head.revision.clone());
        atomic(&self.state_path(), &serde_json::to_vec(&state)?)?;
        Ok(head)
    }
}
