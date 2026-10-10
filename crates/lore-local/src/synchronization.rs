use crate::{ActiveDocument, SharedBaseline, Workspace, atomic_write};
use lore_core::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct PendingPush {
    request: PublishMapping,
    source_store: Option<KnowledgeStoreId>,
    target_before: Option<ActiveDocument>,
}

#[derive(Serialize)]
pub struct PushResult {
    pub receipt: PublicationReceipt,
    pub integration: String,
    pub replayed: bool,
    pub local_changes_remaining: bool,
}

#[derive(Serialize)]
pub struct PullResult {
    pub store: KnowledgeStoreId,
    pub mapping: MappingId,
    pub status: String,
    pub local_revision: Option<RevisionId>,
    pub shared_revision: RevisionId,
    pub baseline: Option<RevisionId>,
}

pub enum ConflictResolutionChoice {
    Local,
    Shared,
}

fn baseline(active: &ActiveDocument, store: &KnowledgeStoreId) -> Result<Option<RevisionId>> {
    match &active.shared_baseline {
        Some(baseline) if baseline.source == store.as_str() => Ok(Some(baseline.revision.parse()?)),
        Some(_) => Err(Error::new(
            ErrorKind::Corrupt,
            "Mapping baseline belongs to another store",
        )),
        None => Ok(None),
    }
}

fn shared_active(
    revision: RevisionId,
    store: &KnowledgeStoreId,
    shared: Option<&RevisionId>,
) -> ActiveDocument {
    ActiveDocument {
        revision,
        shared_baseline: shared.map(|revision| SharedBaseline {
            source: store.to_string(),
            revision: revision.to_string(),
        }),
    }
}

impl Workspace {
    pub fn connect_local_store(&self, connection: &crate::LocalStoreConnection) -> Result<()> {
        let observed = crate::LocalStoreConnection::open(&connection.directory)?;
        if observed.store != connection.store {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Local store identity changed",
            ));
        }
        let _lock = self.lock()?;
        let mut state = self.state()?;
        state
            .connected_local_stores
            .insert(connection.store.clone(), observed.directory);
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)
    }

    pub fn connect_remote(&self, store: &KnowledgeStoreId, name: &str) -> Result<()> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        state.connected_remotes.insert(store.clone(), name.into());
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)
    }

    pub fn disconnect_remote(&self, store: &KnowledgeStoreId) -> Result<()> {
        let _lock = self.lock()?;
        let mut state = self.state()?;
        state.connected_remotes.remove(store);
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)
    }

    fn history_bundle(
        &self,
        store: &KnowledgeStoreId,
        id: &MappingId,
        active: &ActiveDocument,
    ) -> Result<MappingBundle> {
        let mut next = Some(active.revision.clone());
        let mut revisions = vec![];
        while let Some(revision) = next {
            if revisions.len() >= 10000 {
                return Err(Error::new(
                    ErrorKind::Corrupt,
                    "Local mapping ancestry exceeds its limit",
                ));
            }
            let record = self.mapping_repository.load(id, &revision)?;
            next = record.parent.clone();
            revisions.push(MappingView { revision, record });
        }
        let bundle = MappingBundle {
            store: store.clone(),
            revisions,
        };
        bundle.validate()?;
        if bundle.head()?.record.mapping.code_repository()? != self.code.repository_id {
            return Err(Error::invalid(
                "Mapping does not belong to this workspace's code repository",
            ));
        }
        Ok(bundle)
    }

    /// Persists the exact request before publication. Retrying a pending request preserves its identity and payload.
    pub fn push_mapping(
        &self,
        remote: &dyn MappingStoreClient,
        store: &KnowledgeStoreId,
        id: &MappingId,
        from_store: Option<&KnowledgeStoreId>,
    ) -> Result<PushResult> {
        let directory = self
            .state_path
            .parent()
            .unwrap()
            .join("pushes")
            .join(hash(store.as_str()));
        std::fs::create_dir_all(&directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(format!("{id}.lock")))?;
        lock.lock()?;
        let path = directory.join(format!("{id}.json"));
        let replayed = path.exists();
        let pending: PendingPush = if path.exists() {
            serde_json::from_slice(&std::fs::read(&path)?)?
        } else {
            let _lock = self.lock()?;
            let state = self.state()?;
            let target = state
                .store_mappings
                .get(store)
                .and_then(|mappings| mappings.get(id))
                .cloned();
            if target.is_some() && from_store.is_some_and(|source| source != store) {
                return Err(Error::invalid(
                    "--from-remote only selects the source of a new mapping in the destination store",
                ));
            }
            let source_store = if target.is_some() {
                Some(store.clone())
            } else {
                state.mapping_scope(id, from_store)?
            };
            let active = &state.mapping_states(source_store.as_ref()).unwrap()[id];
            let expected = target
                .as_ref()
                .map(|active| baseline(active, store))
                .transpose()?
                .flatten();
            let pending = PendingPush {
                request: PublishMapping {
                    operation: DocumentId::new().to_string(),
                    expected,
                    bundle: self.history_bundle(store, id, active)?,
                },
                source_store,
                target_before: target,
            };
            if serde_json::to_vec(&StoreRequest::Publish {
                request: pending.request.clone(),
            })?
            .len()
                > MAX_SYNC_BYTES
            {
                return Err(Error::invalid(
                    "Publication history exceeds the 16 MiB transfer limit",
                ));
            }
            atomic_write(&path, &serde_json::to_vec(&pending)?)?;
            pending
        };
        pending.request.validate()?;
        if pending.request.bundle.store != *store
            || pending.request.bundle.head()?.record.mapping.id != *id
        {
            return Err(Error::new(
                ErrorKind::Corrupt,
                "Pending publication belongs to another mapping scope",
            ));
        }
        let receipt = match remote.publish(&pending.request) {
            Ok(receipt) => receipt,
            Err(error) if error.kind == ErrorKind::Conflict => {
                std::fs::remove_file(&path)?;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        if receipt.store != *store
            || receipt.operation != pending.request.operation
            || receipt.mapping != *id
            || receipt.revision != pending.request.bundle.head()?.revision
        {
            return Err(Error::invalid(
                "Publication receipt does not match the pending request",
            ));
        }
        self.retain_store_revisions(
            store,
            id,
            pending
                .request
                .bundle
                .revisions
                .iter()
                .map(|view| &view.revision),
        )?;
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let current = state
            .store_mappings
            .get(store)
            .and_then(|mappings| mappings.get(id))
            .cloned();
        let expected_baseline = pending
            .target_before
            .as_ref()
            .map(|active| baseline(active, store))
            .transpose()?
            .flatten();
        let current_baseline = current
            .as_ref()
            .map(|active| baseline(active, store))
            .transpose()?
            .flatten();
        let mut integration = "applied";
        if current_baseline != expected_baseline
            && current_baseline.as_ref() != Some(&receipt.revision)
        {
            integration = "conflict";
        } else if pending.source_store.as_ref() == Some(store) {
            if let Some(active) = state.mapping_states_mut(Some(store)).get_mut(id) {
                active.shared_baseline = Some(SharedBaseline {
                    source: store.to_string(),
                    revision: receipt.revision.to_string(),
                });
            } else {
                integration = "conflict";
            }
        } else if current != pending.target_before {
            integration = "conflict";
        } else if pending.source_store.is_none() {
            if let Some(active) = state.mappings.remove(id) {
                let history = self.history_bundle(store, id, &active)?;
                self.retain_store_revisions(
                    store,
                    id,
                    history.revisions.iter().map(|view| &view.revision),
                )?;
                state.mapping_states_mut(Some(store)).insert(
                    id.clone(),
                    shared_active(active.revision, store, Some(&receipt.revision)),
                );
            } else {
                integration = "conflict";
            }
        } else {
            state.mapping_states_mut(Some(store)).insert(
                id.clone(),
                shared_active(receipt.revision.clone(), store, Some(&receipt.revision)),
            );
        }
        if integration == "applied" {
            atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        }
        std::fs::remove_file(&path)?;
        let local_changes_remaining = state
            .store_mappings
            .get(store)
            .and_then(|mappings| mappings.get(id))
            .is_some_and(|active| active.revision != receipt.revision);
        Ok(PushResult {
            receipt,
            integration: integration.into(),
            replayed,
            local_changes_remaining,
        })
    }

    /// Fetches complete mapping ancestry without activating it or changing baselines.
    pub fn fetch_mapping(
        &self,
        remote: &dyn MappingStoreClient,
        store: &KnowledgeStoreId,
        id: &MappingId,
    ) -> Result<MappingBundle> {
        let bundle = remote.read(id, None)?;
        bundle.validate()?;
        if bundle.store != *store
            || bundle.head()?.record.mapping.id != *id
            || bundle.head()?.record.mapping.code_repository()? != self.code.repository_id
        {
            return Err(Error::invalid(
                "Fetched mapping does not match the requested store and code repository",
            ));
        }
        for view in bundle.revisions.iter().rev() {
            self.mapping_repository.record(&view.record)?;
        }
        self.retain_store_revisions(
            store,
            id,
            bundle.revisions.iter().map(|view| &view.revision),
        )?;
        Ok(bundle)
    }

    pub fn pull_mapping(
        &self,
        remote: &dyn MappingStoreClient,
        store: &KnowledgeStoreId,
        id: &MappingId,
    ) -> Result<PullResult> {
        let before = self
            .state()?
            .store_mappings
            .get(store)
            .and_then(|mappings| mappings.get(id))
            .cloned();
        let bundle = self.fetch_mapping(remote, store, id)?;
        self.incorporate_bundle(&bundle, before.as_ref())
    }

    pub fn incorporate_bundle(
        &self,
        bundle: &MappingBundle,
        before: Option<&ActiveDocument>,
    ) -> Result<PullResult> {
        bundle.validate()?;
        let view = bundle.head()?;
        let id = &view.record.mapping.id;
        let store = &bundle.store;
        if view.record.mapping.code_repository()? != self.code.repository_id {
            return Err(Error::invalid(
                "Pulled mapping refers to another code repository",
            ));
        }
        for revision in &bundle.revisions {
            self.mapping_repository.load(id, &revision.revision)?;
        }
        self.retain_store_revisions(
            store,
            id,
            bundle.revisions.iter().map(|view| &view.revision),
        )?;
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let current = state
            .store_mappings
            .get(store)
            .and_then(|mappings| mappings.get(id))
            .cloned();
        let unowned = if current.is_none() {
            state.mappings.get(id).cloned()
        } else {
            None
        };
        let base = current
            .as_ref()
            .map(|active| baseline(active, store))
            .transpose()?
            .flatten();
        let mut status = "incorporated";
        if current.as_ref() != before {
            status = "local_changed";
        } else if unowned.is_some() {
            status = "conflict";
        } else if let Some(active) = &current {
            if active.revision == view.revision {
            } else if base.as_ref() == Some(&view.revision) {
                status = "local_changes_preserved";
            } else if base.as_ref() != Some(&active.revision) {
                status = "conflict";
            }
        }
        if status == "incorporated" {
            state.mapping_states_mut(Some(store)).insert(
                id.clone(),
                shared_active(view.revision.clone(), store, Some(&view.revision)),
            );
            atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        }
        Ok(PullResult {
            store: store.clone(),
            mapping: id.clone(),
            status: status.into(),
            local_revision: if status == "incorporated" {
                Some(view.revision.clone())
            } else {
                current.or(unowned).map(|active| active.revision)
            },
            shared_revision: view.revision.clone(),
            baseline: if status == "incorporated" {
                Some(view.revision.clone())
            } else {
                base
            },
        })
    }

    pub fn mapping_matches_scope(
        &self,
        mapping: &Mapping,
        requested: &str,
        recursive: bool,
    ) -> Result<bool> {
        if self.mapping_scope_matches(mapping, requested, recursive)? {
            return Ok(true);
        }
        let in_scope = |path: &str| {
            requested.is_empty()
                || path == requested
                || recursive && path.starts_with(&format!("{requested}/"))
        };
        for anchor in &mapping.anchors {
            if anchor.context.repository == self.code.repository_id && in_scope(&anchor.path)
                || TextResolver
                    .resolve(anchor, &self.code, Some(requested))?
                    .possible_locations()
                    .any(|location| in_scope(&location.path))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Applies an explicit resolution against observed revisions. Publication still checks the shared head.
    pub fn resolve_mapping_conflict(
        &self,
        remote: &dyn MappingStoreClient,
        key: &SharedMappingKey,
        expected_local: &RevisionId,
        expected_shared: &RevisionId,
        choice: ConflictResolutionChoice,
        actor: Actor,
    ) -> Result<MappingView> {
        let store = &key.store;
        let id = &key.mapping;
        let pending = self
            .state_path
            .parent()
            .unwrap()
            .join("pushes")
            .join(hash(store.as_str()))
            .join(format!("{id}.json"));
        std::fs::create_dir_all(pending.parent().unwrap())?;
        let operation_lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(pending.with_extension("lock"))?;
        operation_lock.lock()?;
        if pending.exists() {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Retry the uncertain push before resolving this mapping",
            ));
        }
        let bundle = self.fetch_mapping(remote, store, id)?;
        if bundle.head()?.revision != *expected_shared {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Shared mapping changed since the resolution was prepared",
            ));
        }
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let source = if state
            .store_mappings
            .get(store)
            .is_some_and(|mappings| mappings.contains_key(id))
        {
            Some(store.clone())
        } else {
            None
        };
        let active = state
            .mapping_states(source.as_ref())
            .and_then(|mappings| mappings.get(id))
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    "Mapping is not active in the selected store",
                )
            })?;
        if active.revision != *expected_local {
            return Err(Error::new(
                ErrorKind::Conflict,
                "Local mapping changed since the resolution was prepared",
            ));
        }
        let view = if matches!(choice, ConflictResolutionChoice::Local) {
            let mapping = self.mapping_repository.load(id, &active.revision)?.mapping;
            let record = MappingRevision {
                parent: Some(expected_shared.clone()),
                actor,
                at_ms: crate::now_ms(),
                mapping,
            };
            let revision = self.mapping_repository.record(&record)?;
            self.retain_store_revisions(store, id, [&revision])?;
            MappingView { revision, record }
        } else {
            bundle.head()?.clone()
        };
        if source.is_none() {
            let local_history = self.history_bundle(store, id, &active)?;
            self.retain_store_revisions(
                store,
                id,
                local_history.revisions.iter().map(|view| &view.revision),
            )?;
            state.mappings.remove(id);
        }
        state.mapping_states_mut(Some(store)).insert(
            id.clone(),
            shared_active(view.revision.clone(), store, Some(expected_shared)),
        );
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(view)
    }

    pub fn observed_store_mapping(
        &self,
        store: &KnowledgeStoreId,
        id: &MappingId,
    ) -> Result<Option<ActiveDocument>> {
        Ok(self
            .state()?
            .store_mappings
            .get(store)
            .and_then(|mappings| mappings.get(id))
            .cloned())
    }
}
