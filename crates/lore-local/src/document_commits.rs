use crate::{
    ActiveDocument, ConflictResolutionChoice, SharedBaseline, Workspace, WorkspaceState,
    atomic_write, now_ms,
};
use lore_core::*;
use std::collections::BTreeMap;

impl Workspace {
    pub(crate) fn document_store(
        &self,
        state: &WorkspaceState,
    ) -> Result<Option<crate::knowledge::BranchDocumentStore>> {
        let Some(history) = self.knowledge_history(state)? else {
            return Ok(None);
        };
        let Some(worktree) = &state.knowledge_worktree else {
            return Ok(None);
        };
        Ok(Some(crate::knowledge::BranchDocumentStore {
            history,
            branch: if self.central_view {
                "main".into()
            } else {
                worktree.branch.clone()
            },
            path: if self.central_view {
                None
            } else {
                Some(worktree.path.clone())
            },
        }))
    }

    pub fn document_store_state_path(&self) -> Result<Option<std::path::PathBuf>> {
        Ok(self.state()?.local_store.map(|connection| {
            connection
                .directory
                .join("knowledge")
                .join(hash(&self.code.repository_id))
                .join("history.git/refs/heads/main")
        }))
    }

    pub(crate) fn available_documents(&self, state: &WorkspaceState) -> Result<Vec<DocumentView>> {
        let mut documents = BTreeMap::new();
        if let Some(store) = self.document_store(state)? {
            for view in store.list()? {
                documents.insert(view.document.id.clone(), view);
            }
        }
        for (id, active) in state.documents.iter().filter(|_| !self.central_view) {
            let revision = self.repository.load(id, &active.revision)?;
            documents.insert(
                id.clone(),
                DocumentView::new(active.revision.clone(), revision),
            );
        }
        Ok(documents.into_values().collect())
    }

    pub(crate) fn cache_document_bundle(&self, bundle: &DocumentBundle) -> Result<DocumentView> {
        bundle.validate()?;
        for record in &bundle.revisions {
            self.repository.record(record)?;
        }
        bundle.head()
    }

    pub fn read_committed_document(
        &self,
        id: &DocumentId,
        revision: Option<&RevisionId>,
    ) -> Result<DocumentView> {
        let store = self.document_store(&self.state()?)?.ok_or_else(|| {
            Error::new(
                ErrorKind::NotInitialized,
                "Run lore init to select a local document store",
            )
        })?;
        self.cache_document_bundle(&store.read(id, revision)?)
    }

    /// Editing a discovered document captures its observed committed revision as the baseline.
    pub(crate) fn prepare_document_edit(
        &self,
        state: &mut WorkspaceState,
        id: &DocumentId,
        expected: &RevisionId,
    ) -> Result<()> {
        if state.documents.contains_key(id) {
            return Ok(());
        }
        let store = self.document_store(state)?.ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "Document is not available in this workspace",
            )
        })?;
        let view = self.cache_document_bundle(&store.read(id, None)?)?;
        if view.revision != *expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Committed document changed since it was read; reread before editing",
            ));
        }
        let store_id = self.document_source(state)?;
        state
            .documents
            .insert(id.clone(), document_active(view.revision, &store_id));
        Ok(())
    }

    /// Records the selected document on this workspace's knowledge branch.
    /// The workspace lock preserves edits; the store compares the baseline under its own lock.
    pub fn commit_document(&self, id: &DocumentId, expected: &RevisionId) -> Result<DocumentView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        self.prepare_document_edit(&mut state, id, expected)?;
        let active = state.documents.get(id).unwrap();
        if active.revision != *expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Workspace document changed since it was read",
            ));
        }
        let store_id = self.document_source(&state)?;
        let baseline = match &active.shared_baseline {
            Some(baseline) if baseline.source == store_id.as_str() => {
                Some(baseline.revision.parse()?)
            }
            Some(_) => {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Document baseline belongs to a different store; inspect before committing",
                ));
            }
            None => None,
        };
        let mut revisions = vec![];
        let mut next = Some(expected.clone());
        while let Some(revision) = next {
            if revisions.len() >= 10000 {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Workspace document ancestry exceeds its limit",
                ));
            }
            let record = self.repository.load(id, &revision)?;
            next = record.parent.clone();
            revisions.push(record);
        }
        let store = self.document_store(&state)?.unwrap();
        let committed = store.commit(baseline.as_ref(), &DocumentBundle { revisions })?;
        state
            .local_store
            .as_ref()
            .unwrap()
            .register(&[], &["local".into()])?;
        self.cache_document_bundle(&store.read(id, None)?)?;
        state.documents.insert(
            id.clone(),
            document_active(committed.revision.clone(), &store_id),
        );
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(committed)
    }

    /// Selects the latest committed revision without discarding uncommitted workspace edits.
    pub fn checkout_document(&self, id: &DocumentId) -> Result<DocumentView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let store = self.document_store(&state)?.ok_or_else(|| {
            Error::new(
                ErrorKind::NotInitialized,
                "Run lore init to select a local document store",
            )
        })?;
        let committed = self.cache_document_bundle(&store.read(id, None)?)?;
        let store_id = self.document_source(&state)?;
        if let Some(active) = state.documents.get(id) {
            let clean = active.shared_baseline.as_ref().is_some_and(|baseline| {
                baseline.source == store_id.as_str()
                    && baseline.revision == active.revision.as_str()
            });
            if !clean && active.revision != committed.revision {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Checkout would discard local changes; inspect and resolve-conflict explicitly",
                ));
            }
        }
        state.documents.insert(
            id.clone(),
            document_active(committed.revision.clone(), &store_id),
        );
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(committed)
    }

    /// Applies an explicit whole-document choice while both observed versions still match.
    /// Central knowledge is incorporated into the branch; choosing local leaves an uncommitted document change.
    pub fn resolve_document_conflict(
        &self,
        id: &DocumentId,
        expected_local: &RevisionId,
        expected_shared: &RevisionId,
        choice: ConflictResolutionChoice,
        actor: Actor,
    ) -> Result<DocumentView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let active = state
            .documents
            .get(id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Document has no workspace version"))?;
        if active.revision != *expected_local {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Local document changed during conflict resolution",
            ));
        }
        let local = self.repository.load(id, expected_local)?;
        let history = self
            .knowledge_history(&state)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        let worktree = state
            .knowledge_worktree
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?
            .clone();
        history.ensure_clean_worktree(&worktree.path, &worktree.branch)?;
        let central = history.read_version("main")?;
        let shared_record = central
            .snapshot
            .documents
            .get(id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Document is not in central knowledge"))?
            .clone();
        if shared_record.id()? != *expected_shared {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Central document changed during resolution",
            ));
        }
        let branch = history.resolve(&worktree.branch)?;
        let incorporated =
            history.pull_resolving_document(&worktree.branch, &branch, &central.version, id)?;
        history.refresh_worktree(&worktree.path)?;
        self.advance_clean_documents(&mut state, &incorporated.snapshot)?;
        let shared_record = incorporated.snapshot.documents[id].clone();
        let shared = DocumentView::new(self.repository.record(&shared_record)?, shared_record);
        let store_id = self.document_source(&state)?;
        let selected = match choice {
            ConflictResolutionChoice::Shared => shared.clone(),
            ConflictResolutionChoice::Local => {
                let record = Revision {
                    parent: Some(shared.revision.clone()),
                    actor,
                    at_ms: now_ms(),
                    document: local.document,
                };
                let revision = self.repository.record(&record)?;
                DocumentView::new(revision, record)
            }
        };
        let mut active = document_active(shared.revision, &store_id);
        active.revision = selected.revision.clone();
        state.documents.insert(id.clone(), active);
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(selected)
    }
}

fn document_active(revision: RevisionId, store: &str) -> ActiveDocument {
    ActiveDocument {
        shared_baseline: Some(SharedBaseline {
            source: store.to_string(),
            revision: revision.to_string(),
        }),
        revision,
    }
}
