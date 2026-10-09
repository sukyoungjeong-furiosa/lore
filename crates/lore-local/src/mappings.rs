use crate::{ActiveDocument, Workspace, atomic_write, now_ms};
use lore_core::{
    Actor, Anchor, AnchorResolver, DocumentReference, Error, ErrorKind, Mapping, MappingId,
    MappingRepository, MappingRevision, MappingView, Projection, Result, RevisionId,
};
use serde::Serialize;
use std::path::PathBuf;

pub struct FileMappingRepository {
    pub directory: PathBuf,
}

impl FileMappingRepository {
    fn path(&self, id: &MappingId, revision: &RevisionId) -> PathBuf {
        self.directory
            .join("mappings")
            .join(id.as_str())
            .join("revisions")
            .join(format!("{revision}.json"))
    }
}

impl MappingRepository for FileMappingRepository {
    fn load(&self, id: &MappingId, revision: &RevisionId) -> Result<MappingRevision> {
        let bytes = std::fs::read(self.path(id, revision)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(ErrorKind::NotFound, "Mapping revision is unavailable")
            } else {
                e.into()
            }
        })?;
        let value: MappingRevision = serde_json::from_slice(&bytes)?;
        value.mapping.validate()?;
        if value.mapping.id != *id || value.id()? != *revision {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Mapping revision failed its identity check",
            ));
        }
        Ok(value)
    }

    fn record(&self, value: &MappingRevision) -> Result<RevisionId> {
        use std::io::Write;
        value.mapping.validate()?;
        let id = value.id()?;
        let path = self.path(&value.mapping.id, &id);
        std::fs::create_dir_all(path.parent().unwrap())?;
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(path) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.load(&value.mapping.id, &id)?;
            }
            Err(e) => return Err(e.error.into()),
        }
        Ok(id)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct LocatedMapping {
    pub view: lore_core::MappingSummaryView,
    pub projections: Vec<Projection>,
}

impl Workspace {
    pub fn read_mapping(
        &self,
        id: &MappingId,
        revision: Option<&RevisionId>,
    ) -> Result<MappingView> {
        let state = self.state()?;
        let active = state.mappings.get(id).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Mapping is not active in this workspace",
            )
        })?;
        let selected = revision.unwrap_or(&active.revision);
        Ok(MappingView {
            revision: selected.clone(),
            record: self.mapping_repository.load(id, selected)?,
        })
    }

    pub fn list_mappings(&self, include_archived: bool) -> Result<Vec<MappingView>> {
        let mut views = vec![];
        for (id, active) in self.state()?.mappings {
            let record = self.mapping_repository.load(&id, &active.revision)?;
            if include_archived || !record.mapping.archived {
                views.push(MappingView {
                    revision: active.revision,
                    record,
                });
            }
        }
        Ok(views)
    }

    pub fn create_mapping(
        &self,
        document: DocumentReference,
        anchor: Anchor,
        context: String,
        actor: Actor,
    ) -> Result<MappingView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let mapping = Mapping {
            id: MappingId::new(),
            document,
            anchors: vec![anchor],
            context,
            archived: false,
        };
        let record = MappingRevision {
            parent: None,
            actor,
            at_ms: now_ms(),
            mapping,
        };
        let revision = self.mapping_repository.record(&record)?;
        state.mappings.insert(
            record.mapping.id.clone(),
            ActiveDocument {
                revision: revision.clone(),
                shared_baseline: None,
            },
        );
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(MappingView { revision, record })
    }

    pub fn update_mapping(
        &self,
        id: &MappingId,
        expected: &RevisionId,
        actor: Actor,
        edit: impl FnOnce(&mut Mapping) -> Result<()>,
    ) -> Result<MappingView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let active = state.mappings.get_mut(id).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Mapping is not active in this workspace",
            )
        })?;
        if active.revision != *expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Mapping changed; inspect the current revision before editing",
            ));
        }
        let current = self.mapping_repository.load(id, &active.revision)?;
        let mut mapping = current.mapping.clone();
        edit(&mut mapping)?;
        mapping.validate()?;
        if mapping.id != *id {
            return Err(Error::invalid("Mapping identity cannot change"));
        }
        if mapping == current.mapping {
            return Ok(MappingView {
                revision: active.revision.clone(),
                record: current,
            });
        }
        let record = MappingRevision {
            parent: Some(active.revision.clone()),
            actor,
            at_ms: now_ms(),
            mapping,
        };
        let revision = self.mapping_repository.record(&record)?;
        active.revision = revision.clone();
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(MappingView { revision, record })
    }

    pub fn mapping_history(&self, id: &MappingId) -> Result<Vec<MappingView>> {
        let mut result = vec![];
        let mut next = Some(self.read_mapping(id, None)?.revision);
        while let Some(revision) = next {
            let record = self.mapping_repository.load(id, &revision)?;
            next = record.parent.clone();
            result.push(MappingView { revision, record });
        }
        Ok(result)
    }

    pub fn locate_mappings(
        &self,
        scope: Option<&str>,
        recursive: bool,
        include_archived: bool,
        resolver: &dyn AnchorResolver,
    ) -> Result<Vec<LocatedMapping>> {
        let in_scope = |path: &str| {
            scope.is_none_or(|scope| {
                scope.is_empty()
                    || path == scope
                    || recursive && path.starts_with(&format!("{scope}/"))
            })
        };
        let mut result = vec![];
        for view in self.list_mappings(include_archived)? {
            let mut relevant = scope.is_none();
            let mut projections = vec![];
            for anchor in &view.record.mapping.anchors {
                let projection = resolver.resolve(anchor, &self.code, scope)?;
                relevant |= anchor.context.repository == self.code.repository_id
                    && in_scope(&anchor.path)
                    || projection
                        .possible_locations()
                        .any(|location| in_scope(&location.path));
                projections.push(projection);
            }
            if relevant {
                result.push(LocatedMapping {
                    view: view.into(),
                    projections,
                });
            }
        }
        Ok(result)
    }

    pub fn install_mapping(&self, source: &Workspace, id: &MappingId) -> Result<MappingView> {
        let view = source.read_mapping(id, None)?;
        for item in source.mapping_history(id)? {
            self.mapping_repository.record(&item.record)?;
        }
        let _lock = self.lock()?;
        let mut state = self.state()?;
        if let Some(active) = state.mappings.get(id) {
            if active.revision != view.revision {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Local mapping differs; installation cannot overwrite it",
                ));
            }
        } else {
            state.mappings.insert(
                id.clone(),
                ActiveDocument {
                    revision: view.revision.clone(),
                    shared_baseline: None,
                },
            );
            atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        }
        Ok(view)
    }
}
