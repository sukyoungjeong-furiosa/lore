use lore_core::*;
use lore_local::*;
use lore_server::*;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

struct LocalRemote {
    store: MappingStoreId,
    server: Arc<Server>,
    lose_response: Cell<bool>,
    after_publish: RefCell<Option<Box<dyn FnOnce()>>>,
    after_read: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl LocalRemote {
    fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
        self.server.execute("test-token", request)
    }
}

impl MappingStoreClient for LocalRemote {
    fn descriptor(&self) -> Result<MappingStoreDescriptor> {
        match self.execute(StoreRequest::Describe {
            store: self.store.clone(),
        })? {
            StoreResponse::Descriptor { descriptor } => Ok(descriptor),
            _ => unreachable!(),
        }
    }
    fn catalog(&self, repository: &str) -> Result<StoreCatalog> {
        match self.execute(StoreRequest::Catalog {
            store: self.store.clone(),
            repository: repository.into(),
        })? {
            StoreResponse::Catalog { catalog } => Ok(catalog),
            _ => unreachable!(),
        }
    }
    fn read(&self, mapping: &MappingId, revision: Option<&RevisionId>) -> Result<MappingBundle> {
        let response = self.execute(StoreRequest::Read {
            store: self.store.clone(),
            mapping: mapping.clone(),
            revision: revision.cloned(),
        })?;
        if let Some(action) = self.after_read.borrow_mut().take() {
            action();
        }
        match response {
            StoreResponse::Mapping { bundle } => Ok(bundle),
            _ => unreachable!(),
        }
    }
    fn publish(&self, request: &PublishMapping) -> Result<PublicationReceipt> {
        let response = self.execute(StoreRequest::Publish {
            request: request.clone(),
        })?;
        if let Some(action) = self.after_publish.borrow_mut().take() {
            action();
        }
        if self.lose_response.replace(false) {
            return Err(Error::new(
                ErrorKind::Unavailable,
                "Response lost after publication",
            ));
        }
        match response {
            StoreResponse::Published { receipt } => Ok(receipt),
            _ => unreachable!(),
        }
    }
}

struct Setup {
    temp: tempfile::TempDir,
    repo: PathBuf,
    other: PathBuf,
    remote: LocalRemote,
}

impl Setup {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("code");
        let other = temp.path().join("feature");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("scheduler.rs"), "fn schedule() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "test"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["add", "."],
            vec!["commit", "-qm", "initial"],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["worktree", "add", "-qb", "feature", other.to_str().unwrap()])
                .status()
                .unwrap()
                .success()
        );
        let descriptor = |id: &str| MappingStoreDescriptor {
            id: id.parse().unwrap(),
            repositories: BTreeMap::from([("compiler".into(), RepositoryRegistration::default())]),
            backends: BTreeSet::from(["notion".into()]),
        };
        let config = ServerConfig {
            stores: vec![descriptor("team"), descriptor("personal")],
            principals: vec![PrincipalConfig {
                name: "alice".into(),
                token_env: "TOKEN".into(),
                grants: ["team", "personal"]
                    .into_iter()
                    .map(|id| StoreGrant {
                        store: id.parse().unwrap(),
                        repositories: BTreeSet::from(["compiler".into()]),
                        backends: BTreeSet::from(["notion".into()]),
                        write: true,
                    })
                    .collect(),
            }],
        };
        let server = Arc::new(
            Server::new(
                config,
                Arc::new(FileSharedStore {
                    directory: temp.path().join("server"),
                }),
                |_| Some("test-token".into()),
            )
            .unwrap(),
        );
        let remote = LocalRemote {
            store: "team".parse().unwrap(),
            server,
            lose_response: Cell::new(false),
            after_publish: RefCell::new(None),
            after_read: RefCell::new(None),
        };
        Self {
            temp,
            repo,
            other,
            remote,
        }
    }
    fn home(&self) -> PathBuf {
        self.temp.path().join("home")
    }
    fn workspace(&self, path: &Path) -> Workspace {
        Workspace::init(&self.home(), path, None, Some("compiler".into())).unwrap()
    }
    fn create(&self, workspace: &Workspace) -> MappingView {
        workspace
            .create_mapping(
                DocumentReference {
                    backend: "notion".into(),
                    id: "page".into(),
                },
                vec![workspace.code.capture("scheduler.rs", 1, 1).unwrap()],
                vec![],
                "initial".into(),
                Actor::Human,
            )
            .unwrap()
    }
}

#[test]
fn lost_acknowledgement_retries_the_same_snapshot_and_preserves_later_local_edits() {
    let setup = Setup::new();
    let workspace = setup.workspace(&setup.repo);
    let first = setup.create(&workspace);
    let id = first.record.mapping.id.clone();
    let home = setup.home();
    let repo = setup.repo.clone();
    let edit_id = id.clone();
    let expected = first.revision.clone();
    setup.remote.after_publish.replace(Some(Box::new(move || {
        Workspace::open(&home, &repo)
            .unwrap()
            .update_mapping(&edit_id, &expected, Actor::Human, |mapping| {
                mapping.context = "later local edit".into();
                Ok(())
            })
            .unwrap();
    })));
    setup.remote.lose_response.set(true);
    assert_eq!(
        workspace
            .push_mapping(&setup.remote, &setup.remote.store, &id, None)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Unavailable
    );
    let later = workspace.read_mapping(&id, None).unwrap();
    assert_eq!(
        workspace
            .pull_mapping(&setup.remote, &setup.remote.store, &id)
            .unwrap()
            .status,
        "conflict"
    );
    assert_eq!(workspace.list_mappings(true).unwrap().len(), 1);
    let retry = workspace
        .push_mapping(&setup.remote, &setup.remote.store, &id, None)
        .unwrap();
    assert!(retry.replayed && retry.local_changes_remaining);
    assert_eq!(retry.receipt.revision, first.revision);
    assert!(
        setup
            .workspace(&setup.repo)
            .with_mapping_store(setup.remote.store.clone())
            .read_mapping(&id, Some(&later.revision))
            .is_ok()
    );
    assert_eq!(
        workspace.read_mapping(&id, None).unwrap().revision,
        later.revision
    );
    assert_eq!(setup.remote.read(&id, None).unwrap().revisions.len(), 1);
    let next = workspace
        .push_mapping(&setup.remote, &setup.remote.store, &id, None)
        .unwrap();
    assert!(!next.replayed && !next.local_changes_remaining);
    assert_eq!(next.receipt.revision, later.revision);
}

#[test]
fn divergent_changes_are_preserved_and_resolution_requires_both_observed_revisions() {
    let setup = Setup::new();
    let alice = setup.workspace(&setup.repo);
    let first = setup.create(&alice);
    let id = &first.record.mapping.id;
    alice
        .push_mapping(&setup.remote, &setup.remote.store, id, None)
        .unwrap();
    let bob = setup.workspace(&setup.other);
    bob.pull_mapping(&setup.remote, &setup.remote.store, id)
        .unwrap();
    let a = alice
        .update_mapping(id, &first.revision, Actor::Human, |mapping| {
            mapping.context = "alice".into();
            Ok(())
        })
        .unwrap();
    let b = bob
        .update_mapping(id, &first.revision, Actor::Agent, |mapping| {
            mapping.context = "bob".into();
            Ok(())
        })
        .unwrap();
    alice
        .push_mapping(&setup.remote, &setup.remote.store, id, None)
        .unwrap();
    assert_eq!(
        bob.push_mapping(&setup.remote, &setup.remote.store, id, None)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    let pull = bob
        .pull_mapping(&setup.remote, &setup.remote.store, id)
        .unwrap();
    assert_eq!(pull.status, "conflict");
    assert_eq!(bob.read_mapping(id, None).unwrap().revision, b.revision);
    assert!(bob.read_mapping(id, Some(&a.revision)).is_ok());
    assert_eq!(
        bob.resolve_mapping_conflict(
            &setup.remote,
            &SharedMappingKey {
                store: setup.remote.store.clone(),
                mapping: id.clone()
            },
            &first.revision,
            &a.revision,
            ConflictResolutionChoice::Local,
            Actor::Human
        )
        .unwrap_err()
        .kind,
        ErrorKind::Conflict
    );
    let resolved = bob
        .resolve_mapping_conflict(
            &setup.remote,
            &SharedMappingKey {
                store: setup.remote.store.clone(),
                mapping: id.clone(),
            },
            &b.revision,
            &a.revision,
            ConflictResolutionChoice::Local,
            Actor::Human,
        )
        .unwrap();
    assert_eq!(resolved.record.mapping.context, "bob");
    assert_eq!(resolved.record.parent.as_ref(), Some(&a.revision));
    bob.push_mapping(&setup.remote, &setup.remote.store, id, None)
        .unwrap();
    assert_eq!(
        setup
            .remote
            .read(id, None)
            .unwrap()
            .head()
            .unwrap()
            .revision,
        resolved.revision
    );
    assert!(bob.read_mapping(id, Some(&b.revision)).is_ok());
    let shared = alice
        .resolve_mapping_conflict(
            &setup.remote,
            &SharedMappingKey {
                store: setup.remote.store.clone(),
                mapping: id.clone(),
            },
            &a.revision,
            &resolved.revision,
            ConflictResolutionChoice::Shared,
            Actor::Human,
        )
        .unwrap();
    assert_eq!(shared.revision, resolved.revision);
    assert_eq!(shared.record.mapping.context, "bob");
}

#[test]
fn fetch_does_not_activate_and_pull_does_not_overwrite_edits_made_during_retrieval() {
    let setup = Setup::new();
    let alice = setup.workspace(&setup.repo);
    let first = setup.create(&alice);
    let id = first.record.mapping.id.clone();
    alice
        .push_mapping(&setup.remote, &setup.remote.store, &id, None)
        .unwrap();
    let bob = setup.workspace(&setup.other);
    bob.fetch_mapping(&setup.remote, &setup.remote.store, &id)
        .unwrap();
    assert!(bob.list_mappings(true).unwrap().is_empty());
    bob.pull_mapping(&setup.remote, &setup.remote.store, &id)
        .unwrap();
    let a = alice
        .update_mapping(&id, &first.revision, Actor::Human, |mapping| {
            mapping.context = "shared update".into();
            Ok(())
        })
        .unwrap();
    alice
        .push_mapping(&setup.remote, &setup.remote.store, &id, None)
        .unwrap();
    let home = setup.home();
    let repo = setup.other.clone();
    let edit_id = id.clone();
    let expected = first.revision.clone();
    setup.remote.after_read.replace(Some(Box::new(move || {
        Workspace::open(&home, &repo)
            .unwrap()
            .update_mapping(&edit_id, &expected, Actor::Agent, |mapping| {
                mapping.context = "editing during pull".into();
                Ok(())
            })
            .unwrap();
    })));
    assert_eq!(
        bob.pull_mapping(&setup.remote, &setup.remote.store, &id)
            .unwrap()
            .status,
        "local_changed"
    );
    assert_eq!(
        bob.read_mapping(&id, None).unwrap().record.mapping.context,
        "editing during pull"
    );
    assert!(bob.read_mapping(&id, Some(&a.revision)).is_ok());
}
