use lore_core::*;
use lore_server::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

fn descriptor(id: &str) -> MappingStoreDescriptor {
    MappingStoreDescriptor {
        id: id.parse().unwrap(),
        repositories: BTreeMap::from([
            ("compiler".into(), RepositoryRegistration::default()),
            ("kernels".into(), RepositoryRegistration::default()),
        ]),
        backends: BTreeSet::from(["notion".into(), "private".into()]),
    }
}

fn view(parent: Option<&MappingView>, context: &str) -> MappingView {
    let mapping = if let Some(parent) = parent {
        let mut mapping = parent.record.mapping.clone();
        mapping.context = context.into();
        mapping
    } else {
        Mapping {
            id: MappingId::new(),
            document: DocumentReference {
                backend: "notion".into(),
                id: "page".into(),
            },
            anchors: vec![],
            scopes: vec![PathScope {
                repository: "compiler".into(),
                glob: "src/**".into(),
            }],
            context: context.into(),
            archived: false,
        }
    };
    let record = MappingRevision {
        parent: parent.map(|view| view.revision.clone()),
        actor: Actor::Agent,
        at_ms: 1,
        mapping,
    };
    MappingView {
        revision: record.id().unwrap(),
        record,
    }
}

fn request(
    store: &MappingStoreDescriptor,
    history: Vec<MappingView>,
    expected: Option<RevisionId>,
) -> PublishMapping {
    PublishMapping {
        operation: DocumentId::new().to_string(),
        expected,
        bundle: MappingBundle {
            store: store.id.clone(),
            revisions: history,
        },
    }
}

#[test]
fn publication_is_durable_idempotent_and_store_scoped_even_after_a_later_update() {
    let temp = tempfile::tempdir().unwrap();
    let storage = FileSharedStore {
        directory: temp.path().into(),
    };
    let team = descriptor("team");
    let personal = descriptor("personal");
    let first = view(None, "first");
    let first_request = request(&team, vec![first.clone()], None);
    let receipt = storage.publish(&team, "alice", &first_request).unwrap();
    let second = view(Some(&first), "second");
    storage
        .publish(
            &team,
            "bob",
            &request(
                &team,
                vec![second.clone(), first.clone()],
                Some(first.revision.clone()),
            ),
        )
        .unwrap();
    let restarted = FileSharedStore {
        directory: temp.path().into(),
    };
    assert_eq!(
        serde_json::to_value(restarted.publish(&team, "alice", &first_request).unwrap()).unwrap(),
        serde_json::to_value(receipt).unwrap()
    );
    assert_eq!(
        restarted
            .read(&team.id, &first.record.mapping.id, None)
            .unwrap()
            .head()
            .unwrap()
            .revision,
        second.revision
    );
    assert_eq!(
        restarted
            .publish(&team, "bob", &first_request)
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    let mut altered = first_request.clone();
    altered.expected = Some(second.revision.clone());
    assert_eq!(
        restarted
            .publish(&team, "alice", &altered)
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    restarted
        .publish(
            &personal,
            "alice",
            &request(&personal, vec![first.clone()], None),
        )
        .unwrap();
    assert_eq!(
        restarted
            .read(&personal.id, &first.record.mapping.id, None)
            .unwrap()
            .head()
            .unwrap()
            .revision,
        first.revision
    );
    assert_eq!(
        restarted
            .read(&team.id, &first.record.mapping.id, Some(&first.revision))
            .unwrap()
            .head()
            .unwrap()
            .revision,
        first.revision
    );
}

#[test]
fn concurrent_updates_have_one_winner_and_do_not_publish_the_losing_revision() {
    let temp = tempfile::tempdir().unwrap();
    let storage = FileSharedStore {
        directory: temp.path().into(),
    };
    let team = descriptor("team");
    let first = view(None, "initial");
    storage
        .publish(&team, "alice", &request(&team, vec![first.clone()], None))
        .unwrap();
    let a = view(Some(&first), "a");
    let b = view(Some(&first), "b");
    let results = std::thread::scope(|scope| {
        let a = request(
            &team,
            vec![a.clone(), first.clone()],
            Some(first.revision.clone()),
        );
        let b = request(
            &team,
            vec![b.clone(), first.clone()],
            Some(first.revision.clone()),
        );
        let storage = &storage;
        let team = &team;
        let one = scope.spawn(move || storage.publish(team, "alice", &a));
        let two = scope.spawn(move || storage.publish(team, "bob", &b));
        [one.join().unwrap(), two.join().unwrap()]
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| r.as_ref().is_err_and(|e| e.kind == ErrorKind::Conflict))
            .count(),
        1
    );
    let current = storage
        .read(&team.id, &first.record.mapping.id, None)
        .unwrap();
    let losing = if current.head().unwrap().revision == a.revision {
        &b
    } else {
        &a
    };
    assert_eq!(
        storage
            .read(&team.id, &first.record.mapping.id, Some(&losing.revision))
            .unwrap_err()
            .kind,
        ErrorKind::NotFound
    );
}

#[test]
fn grants_do_not_combine_into_unauthorized_repository_backend_pairs() {
    let temp = tempfile::tempdir().unwrap();
    let team = descriptor("team");
    let config = ServerConfig {
        stores: vec![team.clone(), descriptor("other")],
        principals: vec![PrincipalConfig {
            name: "alice".into(),
            token_env: "ALICE_TOKEN".into(),
            grants: vec![
                StoreGrant {
                    store: team.id.clone(),
                    repositories: BTreeSet::from(["compiler".into()]),
                    backends: BTreeSet::from(["notion".into()]),
                    write: true,
                },
                StoreGrant {
                    store: team.id.clone(),
                    repositories: BTreeSet::from(["kernels".into()]),
                    backends: BTreeSet::from(["private".into()]),
                    write: false,
                },
            ],
        }],
    };
    let server = Server::new(
        config,
        Arc::new(FileSharedStore {
            directory: temp.path().into(),
        }),
        |_| Some("test-token".into()),
    )
    .unwrap();
    let first = view(None, "first");
    let good = request(&team, vec![first.clone()], None);
    server
        .execute("test-token", StoreRequest::Publish { request: good })
        .unwrap();
    let mut forbidden = view(None, "private");
    forbidden.record.mapping.document.backend = "private".into();
    forbidden.revision = forbidden.record.id().unwrap();
    assert_eq!(
        server
            .execute(
                "test-token",
                StoreRequest::Publish {
                    request: request(&team, vec![forbidden.clone()], None)
                }
            )
            .unwrap_err()
            .kind,
        ErrorKind::PermissionDenied
    );
    forbidden.record.mapping.scopes[0].repository = "kernels".into();
    forbidden.revision = forbidden.record.id().unwrap();
    assert_eq!(
        server
            .execute(
                "test-token",
                StoreRequest::Publish {
                    request: request(&team, vec![forbidden], None)
                }
            )
            .unwrap_err()
            .kind,
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        server
            .execute(
                "wrong-token",
                StoreRequest::Describe {
                    store: team.id.clone()
                }
            )
            .unwrap_err()
            .kind,
        ErrorKind::PermissionDenied
    );
    assert_eq!(
        server
            .execute(
                "test-token",
                StoreRequest::Describe {
                    store: "other".parse().unwrap()
                }
            )
            .unwrap_err()
            .kind,
        ErrorKind::PermissionDenied
    );
    match server
        .execute(
            "test-token",
            StoreRequest::Catalog {
                store: team.id,
                repository: "compiler".into(),
            },
        )
        .unwrap()
    {
        StoreResponse::Catalog { catalog } => assert_eq!(catalog.catalog.mappings.len(), 1),
        _ => panic!("wrong response"),
    }
}
