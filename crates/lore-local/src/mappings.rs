use crate::{ActiveDocument, Workspace, WorkspaceState, atomic_write, now_ms};
use lore_core::{
    Actor, Anchor, AnchorResolver, DocumentReference, Error, ErrorKind, KnowledgeStoreId, Mapping,
    MappingId, MappingRepository, MappingRevision, MappingView, PathScope, Projection, Result,
    RevisionId,
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
    pub store: Option<KnowledgeStoreId>,
    pub view: lore_core::MappingSummaryView,
    pub projections: Vec<Projection>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScopedMappingView {
    pub store: Option<KnowledgeStoreId>,
    pub view: MappingView,
}

impl WorkspaceState {
    pub(crate) fn mapping_scope(
        &self,
        id: &MappingId,
        selected: Option<&KnowledgeStoreId>,
    ) -> Result<Option<KnowledgeStoreId>> {
        if let Some(store) = selected {
            if self
                .store_mappings
                .get(store)
                .is_some_and(|mappings| mappings.contains_key(id))
            {
                return Ok(Some(store.clone()));
            }
            return Err(Error::new(
                ErrorKind::NotFound,
                "Mapping is not active in the selected store",
            ));
        }
        let mut candidates = vec![];
        if self.mappings.contains_key(id) {
            candidates.push(None);
        }
        candidates.extend(
            self.store_mappings
                .iter()
                .filter(|(_, mappings)| mappings.contains_key(id))
                .map(|(store, _)| Some(store.clone())),
        );
        match candidates.len() {
            0 => Err(Error::new(
                ErrorKind::NotFound,
                "Mapping is not active in this workspace",
            )),
            1 => Ok(candidates.pop().unwrap()),
            _ => Err(Error::new(
                ErrorKind::Conflict,
                "Mapping ID exists in several stores; select --store explicitly",
            )),
        }
    }

    pub(crate) fn mapping_states(
        &self,
        store: Option<&KnowledgeStoreId>,
    ) -> Option<&std::collections::BTreeMap<MappingId, ActiveDocument>> {
        match store {
            Some(store) => self.store_mappings.get(store),
            None => Some(&self.mappings),
        }
    }

    pub(crate) fn mapping_states_mut(
        &mut self,
        store: Option<&KnowledgeStoreId>,
    ) -> &mut std::collections::BTreeMap<MappingId, ActiveDocument> {
        match store {
            Some(store) => self.store_mappings.entry(store.clone()).or_default(),
            None => &mut self.mappings,
        }
    }
}

impl Workspace {
    pub fn mapping_store_for(&self, id: &MappingId) -> Result<Option<KnowledgeStoreId>> {
        let state = self.state()?;
        if self.mapping_store.is_none()
            && self.committed_knowledge_mappings(&state)?.contains_key(id)
            && !state.mappings.contains_key(id)
        {
            return Ok(None);
        }
        state.mapping_scope(id, self.mapping_store.as_ref())
    }

    fn origin_path(&self, store: &KnowledgeStoreId, id: &MappingId) -> Result<PathBuf> {
        Ok(self
            .state()?
            .store
            .join("mapping-origins")
            .join(lore_core::hash(store.as_str()))
            .join(format!("{id}.json")))
    }

    pub(crate) fn retain_store_revisions<'a>(
        &self,
        store: &KnowledgeStoreId,
        id: &MappingId,
        revisions: impl IntoIterator<Item = &'a RevisionId>,
    ) -> Result<()> {
        let path = self.origin_path(store, id)?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock()?;
        let mut known: std::collections::BTreeSet<String> = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
            Err(error) => return Err(error.into()),
        };
        let before = known.len();
        known.extend(revisions.into_iter().map(|revision| revision.to_string()));
        if known.len() != before {
            atomic_write(&path, &serde_json::to_vec(&known)?)?;
        }
        Ok(())
    }

    fn is_store_revision(
        &self,
        store: &KnowledgeStoreId,
        id: &MappingId,
        revision: &RevisionId,
    ) -> Result<bool> {
        match std::fs::read(self.origin_path(store, id)?) {
            Ok(bytes) => Ok(
                serde_json::from_slice::<std::collections::BTreeSet<String>>(&bytes)?
                    .contains(revision.as_str()),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub fn with_mapping_store(mut self, store: KnowledgeStoreId) -> Self {
        self.mapping_store = Some(store);
        self
    }

    pub fn read_mapping(
        &self,
        id: &MappingId,
        revision: Option<&RevisionId>,
    ) -> Result<MappingView> {
        let state = self.state()?;
        if self.mapping_store.is_none()
            && (self.central_view || !state.mappings.contains_key(id))
            && let Some(record) = self.committed_knowledge_mappings(&state)?.remove(id)
        {
            let current = self.mapping_repository.record(&record)?;
            let selected = revision.unwrap_or(&current);
            let record = if *selected == current {
                record
            } else if self.central_view {
                self.knowledge_history(&state)?
                    .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?
                    .mapping_records("main", id)?
                    .into_iter()
                    .find(|record| record.id().ok().as_ref() == Some(selected))
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::NotFound,
                            "Mapping revision is not in central history",
                        )
                    })?
            } else {
                self.mapping_repository.load(id, selected)?
            };
            return Ok(MappingView {
                revision: selected.clone(),
                record,
            });
        }
        let store = state.mapping_scope(id, self.mapping_store.as_ref())?;
        if self.central_view && store.is_none() {
            return Err(Error::new(
                ErrorKind::NotFound,
                "Mapping is not in central knowledge",
            ));
        }
        let active = &state.mapping_states(store.as_ref()).unwrap()[id];
        let selected = revision.unwrap_or(&active.revision);
        if let Some(store) = &store
            && revision.is_some()
            && !self.is_store_revision(store, id, selected)?
        {
            return Err(Error::new(
                ErrorKind::NotFound,
                "Revision has not been recorded or fetched for this mapping store",
            ));
        }
        Ok(MappingView {
            revision: selected.clone(),
            record: self.mapping_repository.load(id, selected)?,
        })
    }

    pub fn list_mappings(&self, include_archived: bool) -> Result<Vec<MappingView>> {
        Ok(self
            .list_scoped_mappings(include_archived)?
            .into_iter()
            .map(|item| item.view)
            .collect())
    }

    pub fn list_scoped_mappings(&self, include_archived: bool) -> Result<Vec<ScopedMappingView>> {
        let state = self.state()?;
        let mut scopes = vec![];
        if self.mapping_store.is_none() && !self.central_view {
            scopes.push((None, &state.mappings));
        }
        if !self.central_view || self.mapping_store.is_some() {
            scopes.extend(
                state
                    .store_mappings
                    .iter()
                    .filter(|(store, _)| {
                        self.mapping_store
                            .as_ref()
                            .is_none_or(|selected| *store == selected)
                    })
                    .map(|(store, mappings)| (Some(store.clone()), mappings)),
            );
        }
        let mut views = vec![];
        if self.mapping_store.is_none() {
            for (id, record) in self.committed_knowledge_mappings(&state)? {
                if (self.central_view || !state.mappings.contains_key(&id))
                    && (include_archived || !record.mapping.archived)
                {
                    let revision = self.mapping_repository.record(&record)?;
                    views.push(ScopedMappingView {
                        store: None,
                        view: MappingView { revision, record },
                    });
                }
            }
        }
        for (store, mappings) in scopes {
            for (id, active) in mappings {
                let record = self.mapping_repository.load(id, &active.revision)?;
                if include_archived || !record.mapping.archived {
                    views.push(ScopedMappingView {
                        store: store.clone(),
                        view: MappingView {
                            revision: active.revision.clone(),
                            record,
                        },
                    });
                }
            }
        }
        Ok(views)
    }

    pub fn create_mapping(
        &self,
        document: DocumentReference,
        anchors: Vec<Anchor>,
        scopes: Vec<PathScope>,
        context: String,
        actor: Actor,
    ) -> Result<MappingView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let mapping = Mapping {
            id: MappingId::new(),
            document,
            anchors,
            scopes,
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
        if let Some(store) = &self.mapping_store {
            self.retain_store_revisions(store, &record.mapping.id, [&revision])?;
        }
        state
            .mapping_states_mut(self.mapping_store.as_ref())
            .insert(
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
        if self.mapping_store.is_none() && state.mapping_scope(id, None).is_err() {
            let record = self
                .committed_knowledge_mappings(&state)?
                .remove(id)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::NotFound,
                        "Mapping is not in this knowledge branch",
                    )
                })?;
            let revision = self.mapping_repository.record(&record)?;
            if *expected != revision {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Mapping changed since it was read",
                ));
            }
            state.mappings.insert(
                id.clone(),
                ActiveDocument {
                    revision: revision.clone(),
                    shared_baseline: Some(crate::SharedBaseline {
                        source: self.document_source(&state)?,
                        revision: revision.to_string(),
                    }),
                },
            );
        }
        let store = state.mapping_scope(id, self.mapping_store.as_ref())?;
        let active = state
            .mapping_states_mut(store.as_ref())
            .get_mut(id)
            .unwrap();
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
        if let Some(store) = &store {
            self.retain_store_revisions(store, id, [&revision])?;
        }
        active.revision = revision.clone();
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(MappingView { revision, record })
    }

    pub fn mapping_history(&self, id: &MappingId) -> Result<Vec<MappingView>> {
        let state = self.state()?;
        if self.mapping_store.is_none()
            && self.committed_knowledge_mappings(&state)?.contains_key(id)
        {
            let history = self
                .knowledge_history(&state)?
                .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
            let worktree = state
                .knowledge_worktree
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
            for record in history.mapping_records(
                if self.central_view {
                    "main"
                } else {
                    &worktree.branch
                },
                id,
            )? {
                self.mapping_repository.record(&record)?;
            }
        }
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
        for item in self.list_scoped_mappings(include_archived)? {
            let view = item.view;
            let mut relevant = scope.is_none();
            if let Some(path) = scope {
                relevant |= self.mapping_scope_matches(&view.record.mapping, path, recursive)?;
            }
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
                    store: item.store,
                    view: view.into(),
                    projections,
                });
            }
        }
        Ok(result)
    }

    pub fn mapping_scope_matches(
        &self,
        mapping: &Mapping,
        path: &str,
        recursive: bool,
    ) -> Result<bool> {
        for scope in &mapping.scopes {
            if scope.repository != self.code.repository_id {
                continue;
            }
            let matcher = scope.matcher()?;
            if path.is_empty()
                || matcher.is_match(path)
                || recursive && scope.glob.starts_with(&format!("{path}/"))
            {
                return Ok(true);
            }
            if recursive
                && self
                    .code
                    .files(Some(path))?
                    .iter()
                    .any(|file| matcher.is_match(file))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn install_mapping(&self, source: &Workspace, id: &MappingId) -> Result<MappingView> {
        let view = source.read_mapping(id, None)?;
        let history = source.mapping_history(id)?;
        for item in &history {
            self.mapping_repository.record(&item.record)?;
        }
        if let Some(store) = &self.mapping_store {
            self.retain_store_revisions(store, id, history.iter().map(|view| &view.revision))?;
        }
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let target = state.mapping_states_mut(self.mapping_store.as_ref());
        if let Some(active) = target.get(id) {
            if active.revision != view.revision {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Local mapping differs; installation cannot overwrite it",
                ));
            }
        } else {
            target.insert(
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
