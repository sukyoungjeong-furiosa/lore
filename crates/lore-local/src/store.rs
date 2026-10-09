use lore_core::{DocumentId, Error, ErrorKind, KnowledgeRepository, Result, Revision, RevisionId};
use std::path::PathBuf;

pub struct FileRepository {
    pub directory: PathBuf,
}

impl FileRepository {
    fn path(&self, document: &DocumentId, revision: &RevisionId) -> PathBuf {
        self.directory
            .join("documents")
            .join(document.as_str())
            .join("revisions")
            .join(format!("{revision}.json"))
    }
}

impl KnowledgeRepository for FileRepository {
    fn load(&self, document: &DocumentId, revision: &RevisionId) -> Result<Revision> {
        let path = self.path(document, revision);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    format!("Document revision unavailable: {document}@{revision}"),
                ));
            }
            Err(e) => return Err(e.into()),
        };
        let value: Revision = serde_json::from_slice(&bytes)?;
        value.document.validate()?;
        if value.document.id != *document || value.id()? != *revision {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Stored revision failed its identity check",
            ));
        }
        Ok(value)
    }

    fn record(&self, value: &Revision) -> Result<RevisionId> {
        use std::io::Write;
        value.document.validate()?;
        let id = value.id()?;
        let path = self.path(&value.document.id, &id);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(&path) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.load(&value.document.id, &id)?;
            }
            Err(e) => return Err(e.error.into()),
        }
        Ok(id)
    }
}
