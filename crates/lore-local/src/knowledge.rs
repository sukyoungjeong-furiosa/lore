use crate::{ActiveDocument, SharedBaseline, Workspace, WorkspaceState, atomic_write};
use lore_core::*;
use lore_store::GitKnowledgeStore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnowledgeWorktree {
    pub store: KnowledgeStoreId,
    pub branch: String,
    pub path: PathBuf,
}

pub(crate) struct BranchDocumentStore {
    pub history: GitKnowledgeStore,
    pub branch: String,
    pub path: Option<PathBuf>,
}

impl DocumentCommitStore for BranchDocumentStore {
    fn list(&self) -> Result<Vec<DocumentView>> {
        self.history
            .read_version(&self.branch)?
            .snapshot
            .documents
            .into_values()
            .map(|record| Ok(DocumentView::new(record.id()?, record)))
            .collect()
    }

    fn read(&self, id: &DocumentId, revision: Option<&RevisionId>) -> Result<DocumentBundle> {
        self.history.document_bundle(&self.branch, id, revision)
    }

    fn commit(
        &self,
        expected: Option<&RevisionId>,
        bundle: &DocumentBundle,
    ) -> Result<DocumentView> {
        if let Some(path) = &self.path {
            self.history.ensure_clean_worktree(path, &self.branch)?;
        }
        let current = self.history.read_version(&self.branch)?;
        let head = bundle.head()?;
        let previous = current.snapshot.documents.get(&head.document.id);
        if previous.is_some_and(|record| {
            record.document == head.document
                && record.actor == bundle.revisions[0].actor
                && record.at_ms == bundle.revisions[0].at_ms
        }) {
            let record = previous.unwrap().clone();
            return Ok(DocumentView::new(record.id()?, record));
        }
        if previous.map(Revision::id).transpose()?.as_ref() != expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Document branch baseline changed; inspect and resolve before committing",
            ));
        }
        let mut snapshot = current.snapshot;
        snapshot
            .documents
            .insert(head.document.id.clone(), bundle.revisions[0].clone());
        let committed = self.history.commit_branch(
            &self.branch,
            &current.version,
            snapshot,
            &format!("Record {}", head.document.title),
        )?;
        if let Some(path) = &self.path {
            self.history.refresh_worktree(path)?;
        }
        let record = committed.snapshot.documents[&head.document.id].clone();
        Ok(DocumentView::new(record.id()?, record))
    }
}

impl Workspace {
    pub fn with_central_view(mut self) -> Self {
        self.central_view = true;
        self
    }

    pub(crate) fn knowledge_history(
        &self,
        state: &WorkspaceState,
    ) -> Result<Option<GitKnowledgeStore>> {
        let Some(connection) = &state.local_store else {
            return Ok(None);
        };
        let descriptor = connection.descriptor()?;
        if !descriptor
            .repositories
            .contains_key(&self.code.repository_id)
        {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "Repository is not registered in the knowledge store",
            ));
        }
        Ok(Some(GitKnowledgeStore {
            directory: connection
                .directory
                .join("knowledge")
                .join(hash(&self.code.repository_id)),
            repository: self.code.repository_id.clone(),
        }))
    }

    fn migrate_legacy_mappings(
        &self,
        state: &WorkspaceState,
        history: &GitKnowledgeStore,
    ) -> Result<()> {
        let shared = state.mappings.iter().filter_map(|(id, active)| {
            active
                .shared_baseline
                .as_ref()
                .filter(|baseline| baseline.source == "local-catalog")
                .map(|baseline| (id, baseline))
        });
        let selected = shared.collect::<Vec<_>>();
        if selected.is_empty() {
            return Ok(());
        }
        history.initialize()?;
        let central = history.read_version("main")?;
        let mut snapshot = central.snapshot.clone();
        for (id, baseline) in selected {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                snapshot.mappings.entry(id.clone())
            {
                entry.insert(
                    self.mapping_repository
                        .load(id, &baseline.revision.parse()?)?,
                );
            }
        }
        if snapshot != central.snapshot {
            let operation = format!(
                "legacy-mapping-import:{}",
                hash(serde_json::to_vec(&snapshot)?)
            );
            let proposal = history.submit(&operation, &central.version, &snapshot)?;
            history.integrate(&proposal.id, &central.version)?;
        }
        Ok(())
    }

    pub(crate) fn ensure_knowledge_worktree(&self) -> Result<KnowledgeWorktree> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let history = self.knowledge_history(&state)?.ok_or_else(|| {
            Error::new(
                ErrorKind::NotInitialized,
                "Run lore init to select a knowledge store",
            )
        })?;
        let store = state.local_store.as_ref().unwrap().store.clone();
        self.migrate_legacy_mappings(&state, &history)?;
        if let Some(worktree) = &state.knowledge_worktree
            && worktree.store == store
        {
            history.create_worktree(&worktree.path, &worktree.branch)?;
            return Ok(worktree.clone());
        }
        let worktree = KnowledgeWorktree {
            store: store.clone(),
            branch: format!(
                "workspaces/{}",
                hash(self.code.root.to_string_lossy().as_bytes())
            ),
            path: self
                .state_path
                .parent()
                .unwrap()
                .join("knowledge-worktrees")
                .join(hash(store.as_str())),
        };
        history.initialize()?;
        let legacy_directory = state
            .local_store
            .as_ref()
            .unwrap()
            .directory
            .join("documents")
            .join(hash(&self.code.repository_id));
        if legacy_directory.join("state.json").exists()
            && !history.directory.join("legacy-imported.json").exists()
        {
            let legacy = lore_store::FileDocumentStore {
                directory: legacy_directory,
            };
            let previous = history.read_version("main")?;
            let mut snapshot = previous.snapshot.clone();
            for view in legacy.list()? {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    snapshot.documents.entry(view.document.id.clone())
                {
                    let bundle = legacy.read(&view.document.id, None)?;
                    entry.insert(bundle.revisions[0].clone());
                }
            }
            if snapshot != previous.snapshot {
                let proposal =
                    history.submit("legacy-document-import", &previous.version, &snapshot)?;
                history.integrate(&proposal.id, &previous.version)?;
            }
            atomic_write(&history.directory.join("legacy-imported.json"), b"true")?;
        }
        history.create_worktree(&worktree.path, &worktree.branch)?;
        let snapshot = history.read_version(&worktree.branch)?.snapshot;
        for (id, record) in snapshot.documents {
            if let Some(active) = state.documents.get_mut(&id) {
                let old = self.repository.load(&id, &active.revision)?;
                if old.document == record.document
                    && old.actor == record.actor
                    && old.at_ms == record.at_ms
                {
                    let revision = self.repository.record(&record)?;
                    *active = ActiveDocument {
                        revision: revision.clone(),
                        shared_baseline: Some(SharedBaseline {
                            source: format!("{}:branch:{}", store, worktree.branch),
                            revision: revision.to_string(),
                        }),
                    };
                }
            }
        }
        state.knowledge_worktree = Some(worktree.clone());
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(worktree)
    }

    pub(crate) fn document_source(&self, state: &WorkspaceState) -> Result<String> {
        let worktree = state.knowledge_worktree.as_ref().ok_or_else(|| {
            Error::new(
                ErrorKind::NotInitialized,
                "Run lore init to create the knowledge worktree",
            )
        })?;
        Ok(format!("{}:branch:{}", worktree.store, worktree.branch))
    }

    pub fn knowledge_status(&self) -> Result<serde_json::Value> {
        let state = self.state()?;
        Ok(
            serde_json::json!({"initialized":state.knowledge_worktree.is_some(),"worktree":state.knowledge_worktree,"view":if self.central_view {"central"} else {"working"}}),
        )
    }

    pub fn knowledge_worktree_status(&self) -> Result<serde_json::Value> {
        let mut result = self.knowledge_status()?;
        let state = self.state()?;
        if let Some(worktree) = &state.knowledge_worktree {
            let history = self.knowledge_history(&state)?.unwrap();
            result["working_version"] = serde_json::json!(history.resolve(&worktree.branch)?);
            result["central_version"] = serde_json::json!(history.resolve("main")?);
            let working = history.read_version(&worktree.branch)?.snapshot;
            let central = history.read_version("main")?.snapshot;
            result["unsubmitted_documents"] = serde_json::json!(
                working
                    .documents
                    .iter()
                    .filter(|(id, r)| central
                        .documents
                        .get(*id)
                        .is_none_or(|c| c.document != r.document
                            || c.actor != r.actor
                            || c.at_ms != r.at_ms))
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>()
            );
            result["unsubmitted_mappings"] = serde_json::json!(
                working
                    .mappings
                    .iter()
                    .filter(|(id, r)| central
                        .mappings
                        .get(*id)
                        .is_none_or(|c| c.mapping != r.mapping
                            || c.actor != r.actor
                            || c.at_ms != r.at_ms))
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>()
            );
        }
        Ok(result)
    }

    pub fn push_knowledge(&self, proposal_only: bool) -> Result<serde_json::Value> {
        let _lock = self.lock()?;
        let state = self.state()?;
        let worktree = state
            .knowledge_worktree
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        let history = self.knowledge_history(&state)?.unwrap();
        history.ensure_clean_worktree(&worktree.path, &worktree.branch)?;
        let proposal = history.submit_branch(&worktree.branch)?;
        if proposal_only {
            return Ok(serde_json::json!({"status":"submitted","proposal":proposal}));
        }
        let central = history.resolve("main")?;
        match history.integrate(&proposal.id, &central) {
            Ok(version) => {
                Ok(serde_json::json!({"status":"integrated","proposal":proposal,"central":version}))
            }
            Err(error) if error.kind == ErrorKind::Conflict => {
                Ok(serde_json::json!({"status":"conflict","proposal":proposal,"error":error}))
            }
            Err(error) => Err(error),
        }
    }

    pub fn knowledge_proposals(&self) -> Result<Vec<KnowledgeProposal>> {
        self.knowledge_history(&self.state()?)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?
            .proposals()
    }

    pub fn knowledge_proposal(&self, id: &str) -> Result<serde_json::Value> {
        let history = self
            .knowledge_history(&self.state()?)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        let proposal = history
            .proposals()?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Proposal does not exist"))?;
        Ok(
            serde_json::json!({"base":history.read_version(&proposal.base)?,"proposed":history.read_version(&proposal.head)?,"central":history.read_version("main")?,"proposal":proposal}),
        )
    }

    pub fn integrate_knowledge(&self, id: &str, expected: &str) -> Result<KnowledgeVersion> {
        self.knowledge_history(&self.state()?)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?
            .integrate(id, expected)
    }

    pub fn pull_knowledge(&self) -> Result<KnowledgeVersion> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let worktree = state
            .knowledge_worktree
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?
            .clone();
        let history = self.knowledge_history(&state)?.unwrap();
        history.ensure_clean_worktree(&worktree.path, &worktree.branch)?;
        let version = history.pull_branch(&worktree.branch)?;
        history.refresh_worktree(&worktree.path)?;
        self.advance_clean_documents(&mut state, &version.snapshot)?;
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(version)
    }

    pub(crate) fn advance_clean_documents(
        &self,
        state: &mut WorkspaceState,
        snapshot: &KnowledgeSnapshot,
    ) -> Result<()> {
        let source = self.document_source(state)?;
        let store = self
            .document_store(state)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        for (id, record) in &snapshot.documents {
            self.cache_document_bundle(&store.read(id, None)?)?;
            let revision = self.repository.record(record)?;
            if let Some(active) = state.documents.get_mut(id)
                && active
                    .shared_baseline
                    .as_ref()
                    .is_some_and(|b| b.source == source && b.revision == active.revision.as_str())
            {
                *active = ActiveDocument {
                    revision: revision.clone(),
                    shared_baseline: Some(SharedBaseline {
                        source: source.clone(),
                        revision: revision.to_string(),
                    }),
                };
            }
        }
        for (id, record) in &snapshot.mappings {
            for ancestor in store.history.mapping_records(&store.branch, id)? {
                self.mapping_repository.record(&ancestor)?;
            }
            let revision = self.mapping_repository.record(record)?;
            if let Some(active) = state.mappings.get_mut(id)
                && active.shared_baseline.as_ref().is_some_and(|baseline| {
                    baseline.revision == active.revision.as_str()
                        && (baseline.source == source
                            || baseline.source == "local-catalog"
                                && self
                                    .mapping_repository
                                    .load(id, &active.revision)
                                    .is_ok_and(|previous| {
                                        previous.mapping == record.mapping
                                            && previous.actor == record.actor
                                            && previous.at_ms == record.at_ms
                                    }))
                })
            {
                *active = ActiveDocument {
                    revision: revision.clone(),
                    shared_baseline: Some(SharedBaseline {
                        source: source.clone(),
                        revision: revision.to_string(),
                    }),
                };
            }
        }
        Ok(())
    }

    pub fn checkout_knowledge_branch(
        &self,
        branch: &str,
        create: bool,
    ) -> Result<KnowledgeWorktree> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let source = self.document_source(&state)?;
        if state
            .documents
            .values()
            .chain(state.mappings.values())
            .any(|a| {
                a.shared_baseline
                    .as_ref()
                    .is_none_or(|b| b.source != source || b.revision != a.revision.as_str())
            })
        {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Commit or explicitly discard recorded changes before changing the knowledge branch",
            ));
        }
        let mut worktree = state
            .knowledge_worktree
            .clone()
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        let history = self.knowledge_history(&state)?.unwrap();
        history.ensure_clean_worktree(&worktree.path, &worktree.branch)?;
        history.checkout_worktree(&worktree.path, branch, create)?;
        worktree.branch = branch.into();
        state.documents.clear();
        state.mappings.clear();
        state.knowledge_worktree = Some(worktree.clone());
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(worktree)
    }

    pub fn commit_mapping_knowledge(
        &self,
        id: &MappingId,
        expected: &RevisionId,
    ) -> Result<MappingView> {
        let _lock = self.lock()?;
        let state = self.state()?;
        let worktree = state
            .knowledge_worktree
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        let active = state
            .mappings
            .get(id)
            .ok_or_else(|| Error::invalid("Commit requires an unscoped workspace mapping"))?;
        if active.revision != *expected {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Mapping changed since it was read",
            ));
        }
        let record = self.mapping_repository.load(id, expected)?;
        let history = self.knowledge_history(&state)?.unwrap();
        history.ensure_clean_worktree(&worktree.path, &worktree.branch)?;
        let current = history.read_version(&worktree.branch)?;
        let source = self.document_source(&state)?;
        let previous = current.snapshot.mappings.get(id);
        let unchanged = previous.is_some_and(|old| {
            old.mapping == record.mapping && old.actor == record.actor && old.at_ms == record.at_ms
        });
        let baseline = match &active.shared_baseline {
            Some(baseline) if baseline.source == source => {
                Some(baseline.revision.parse::<RevisionId>()?)
            }
            Some(_) => {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Mapping baseline belongs to a different store; inspect before committing",
                ));
            }
            None => None,
        };
        let version = if unchanged {
            current
        } else {
            if previous.map(MappingRevision::id).transpose()? != baseline {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Mapping branch baseline changed; inspect and resolve before committing",
                ));
            }
            let mut snapshot = current.snapshot;
            snapshot.mappings.insert(id.clone(), record);
            history.commit_branch(
                &worktree.branch,
                &current.version,
                snapshot,
                "Record code mapping",
            )?
        };
        history.refresh_worktree(&worktree.path)?;
        let record = version.snapshot.mappings[id].clone();
        let revision = self.mapping_repository.record(&record)?;
        let mut state = state;
        state.mappings.insert(
            id.clone(),
            ActiveDocument {
                revision: revision.clone(),
                shared_baseline: Some(SharedBaseline {
                    source: self.document_source(&state)?,
                    revision: revision.to_string(),
                }),
            },
        );
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(MappingView { revision, record })
    }

    pub fn resolve_knowledge_mapping_conflict(
        &self,
        id: &MappingId,
        expected_local: &RevisionId,
        expected_shared: &RevisionId,
        choice: crate::ConflictResolutionChoice,
        actor: Actor,
    ) -> Result<MappingView> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let active = state
            .mappings
            .get(id)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Mapping has no working changes"))?;
        if active.revision != *expected_local {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Local mapping changed during resolution",
            ));
        }
        let local = self.mapping_repository.load(id, expected_local)?;
        let worktree = state
            .knowledge_worktree
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?
            .clone();
        let history = self
            .knowledge_history(&state)?
            .ok_or_else(|| Error::new(ErrorKind::NotInitialized, "Run lore init"))?;
        history.ensure_clean_worktree(&worktree.path, &worktree.branch)?;
        let central = history.read_version("main")?;
        let shared = central.snapshot.mappings.get(id).ok_or_else(|| {
            Error::new(ErrorKind::NotFound, "Mapping is not in central knowledge")
        })?;
        if shared.id()? != *expected_shared {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Central mapping changed during resolution",
            ));
        }
        let branch = history.resolve(&worktree.branch)?;
        let incorporated =
            history.pull_resolving_mapping(&worktree.branch, &branch, &central.version, id)?;
        history.refresh_worktree(&worktree.path)?;
        self.advance_clean_documents(&mut state, &incorporated.snapshot)?;
        let shared = incorporated.snapshot.mappings[id].clone();
        let baseline = self.mapping_repository.record(&shared)?;
        let record = match choice {
            crate::ConflictResolutionChoice::Shared => shared,
            crate::ConflictResolutionChoice::Local => MappingRevision {
                parent: Some(baseline.clone()),
                actor,
                at_ms: crate::now_ms(),
                mapping: local.mapping,
            },
        };
        let revision = self.mapping_repository.record(&record)?;
        state.mappings.insert(
            id.clone(),
            ActiveDocument {
                revision: revision.clone(),
                shared_baseline: Some(SharedBaseline {
                    source: self.document_source(&state)?,
                    revision: baseline.to_string(),
                }),
            },
        );
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(MappingView { revision, record })
    }

    pub(crate) fn committed_knowledge_mappings(
        &self,
        state: &WorkspaceState,
    ) -> Result<BTreeMap<MappingId, MappingRevision>> {
        let Some(history) = self.knowledge_history(state)? else {
            return Ok(BTreeMap::new());
        };
        if state.knowledge_worktree.is_none() {
            return Ok(BTreeMap::new());
        }
        let branch = if self.central_view {
            "main"
        } else {
            &state.knowledge_worktree.as_ref().unwrap().branch
        };
        Ok(history.read_version(branch)?.snapshot.mappings)
    }
}
