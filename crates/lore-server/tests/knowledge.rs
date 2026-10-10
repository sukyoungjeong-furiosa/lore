use lore_core::*;
use lore_server::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Memory {
    main: String,
    versions: BTreeMap<String, KnowledgeSnapshot>,
    proposals: BTreeMap<String, KnowledgeProposal>,
}
struct MemoryFactory(Arc<Mutex<Memory>>);
struct MemoryStore(Arc<Mutex<Memory>>);
impl KnowledgeStorage for MemoryFactory {
    fn open(&self, _: &KnowledgeStoreId, _: &str) -> Result<Box<dyn KnowledgeStore>> {
        Ok(Box::new(MemoryStore(self.0.clone())))
    }
}
impl KnowledgeStore for MemoryStore {
    fn read_version(&self, version: &str) -> Result<KnowledgeVersion> {
        let state = self.0.lock().unwrap();
        let version = if version == "main" {
            &state.main
        } else {
            version
        };
        Ok(KnowledgeVersion {
            version: version.into(),
            snapshot: state
                .versions
                .get(version)
                .ok_or_else(|| Error::new(ErrorKind::NotFound, "Unknown version"))?
                .clone(),
        })
    }
    fn submit(
        &self,
        operation: &str,
        base: &str,
        snapshot: &KnowledgeSnapshot,
    ) -> Result<KnowledgeProposal> {
        let mut state = self.0.lock().unwrap();
        if let Some(existing) = state.proposals.get(operation) {
            return Ok(existing.clone());
        }
        let base = base.to_owned();
        let head = format!("proposed-{}", state.versions.len());
        state.versions.insert(head.clone(), snapshot.clone());
        let proposal = KnowledgeProposal {
            id: operation.into(),
            base,
            head,
            source: "http".into(),
            integrated: None,
        };
        state.proposals.insert(operation.into(), proposal.clone());
        Ok(proposal)
    }
    fn proposals(&self) -> Result<Vec<KnowledgeProposal>> {
        Ok(self.0.lock().unwrap().proposals.values().cloned().collect())
    }
    fn integrate(&self, id: &str, expected: &str) -> Result<KnowledgeVersion> {
        let mut state = self.0.lock().unwrap();
        let proposal = state.proposals[id].clone();
        if let Some(version) = proposal.integrated {
            return Ok(KnowledgeVersion {
                snapshot: state.versions[&version].clone(),
                version,
            });
        }
        if state.main != expected {
            return Err(Error::new(ErrorKind::Conflict, "Central changed"));
        }
        let snapshot = merge_knowledge(
            &state.versions[&proposal.base],
            &state.versions[&state.main],
            &state.versions[&proposal.head],
        )?;
        let version = format!("accepted-{}", state.versions.len());
        state.versions.insert(version.clone(), snapshot.clone());
        state.main = version.clone();
        state.proposals.get_mut(id).unwrap().integrated = Some(version.clone());
        Ok(KnowledgeVersion { snapshot, version })
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Server,
    Arc<Mutex<Memory>>,
    MappingId,
    MappingId,
) {
    let temp = tempfile::tempdir().unwrap();
    let mut snapshot = KnowledgeSnapshot::default();
    let document = Document::new(
        "Local knowledge".into(),
        "Owned by Lore".into(),
        Actor::Human,
    )
    .unwrap();
    snapshot.documents.insert(
        document.id.clone(),
        Revision {
            parent: None,
            actor: Actor::Human,
            at_ms: 1,
            document,
        },
    );
    let mut mapping = |backend: &str| {
        let mapping = Mapping {
            id: MappingId::new(),
            document: DocumentReference {
                backend: backend.into(),
                id: "page".into(),
            },
            anchors: vec![],
            scopes: vec![PathScope {
                repository: "compiler".into(),
                glob: "src/**".into(),
            }],
            context: backend.into(),
            archived: false,
        };
        let id = mapping.id.clone();
        snapshot.mappings.insert(
            id.clone(),
            MappingRevision {
                parent: None,
                actor: Actor::Agent,
                at_ms: 1,
                mapping,
            },
        );
        id
    };
    let notion = mapping("notion");
    let private = mapping("private");
    let state = Arc::new(Mutex::new(Memory {
        main: "opaque-version-one".into(),
        versions: BTreeMap::from([("opaque-version-one".into(), snapshot)]),
        proposals: BTreeMap::new(),
    }));
    let grant = |backends: &[&str], write| StoreGrant {
        store: "team".parse().unwrap(),
        repositories: BTreeSet::from(["compiler".into()]),
        backends: backends.iter().map(|s| s.to_string()).collect(),
        write,
    };
    let config = ServerConfig {
        stores: vec![KnowledgeStoreDescriptor {
            id: "team".parse().unwrap(),
            repositories: BTreeMap::from([("compiler".into(), RepositoryRegistration::default())]),
            backends: BTreeSet::from(["local".into(), "notion".into(), "private".into()]),
        }],
        principals: vec![
            PrincipalConfig {
                name: "owner".into(),
                token_env: "owner".into(),
                grants: vec![grant(&["local", "notion", "private"], true)],
            },
            PrincipalConfig {
                name: "partial".into(),
                token_env: "partial".into(),
                grants: vec![grant(&["notion"], true)],
            },
            PrincipalConfig {
                name: "reader".into(),
                token_env: "reader".into(),
                grants: vec![grant(&["notion"], false)],
            },
        ],
    };
    let server = Server::new(
        config,
        Arc::new(FileSharedStore {
            directory: temp.path().into(),
        }),
        |name| Some(name.into()),
    )
    .unwrap()
    .with_knowledge_storage(Arc::new(MemoryFactory(state.clone())));
    (temp, server, state, notion, private)
}
fn read(server: &Server, token: &str) -> KnowledgeVersion {
    match server
        .execute_knowledge(
            token,
            KnowledgeRequest::Read {
                store: "team".parse().unwrap(),
                repository: "compiler".into(),
                version: None,
            },
        )
        .unwrap()
    {
        KnowledgeResponse::Version { version } => version,
        _ => panic!(),
    }
}

#[test]
fn http_knowledge_contract_does_not_require_git_storage_or_git_version_tokens() {
    let (_temp, server, state, notion, private) = fixture();
    let observed = read(&server, "partial");
    assert_eq!(observed.version, "opaque-version-one");
    assert!(observed.snapshot.documents.is_empty());
    assert_eq!(observed.snapshot.mappings.len(), 1);
    let mut proposed = observed.snapshot.clone();
    proposed.mappings.get_mut(&notion).unwrap().mapping.context = "Updated public mapping".into();
    let proposal = match server
        .execute_knowledge(
            "partial",
            KnowledgeRequest::Submit {
                store: "team".parse().unwrap(),
                repository: "compiler".into(),
                operation_id: "operation-one".into(),
                base: observed.version.clone(),
                snapshot: proposed,
            },
        )
        .unwrap()
    {
        KnowledgeResponse::Proposal { proposal } => proposal,
        _ => panic!(),
    };
    assert_eq!(
        read(&server, "owner").snapshot.mappings[&notion]
            .mapping
            .context,
        "notion",
        "Submission does not publish"
    );
    assert_eq!(
        server
            .execute_knowledge(
                "reader",
                KnowledgeRequest::Integrate {
                    store: "team".parse().unwrap(),
                    repository: "compiler".into(),
                    proposal: proposal.id.clone(),
                    if_match: observed.version.clone()
                }
            )
            .unwrap_err()
            .kind,
        ErrorKind::PermissionDenied
    );
    server
        .execute_knowledge(
            "partial",
            KnowledgeRequest::Integrate {
                store: "team".parse().unwrap(),
                repository: "compiler".into(),
                proposal: proposal.id,
                if_match: observed.version,
            },
        )
        .unwrap();
    let owner = read(&server, "owner");
    assert_eq!(
        owner.snapshot.mappings[&notion].mapping.context,
        "Updated public mapping"
    );
    assert_eq!(owner.snapshot.mappings[&private].mapping.context, "private");
    assert_eq!(
        owner.snapshot.documents.len(),
        1,
        "Invisible documents are not deleted by a scoped client"
    );
    assert!(state.lock().unwrap().main.starts_with("accepted-"));
}

#[test]
fn a_client_cannot_replace_an_invisible_mapping_by_reusing_its_identity() {
    let (_temp, server, _, notion, private) = fixture();
    let observed = read(&server, "partial");
    let mut proposed = observed.snapshot.clone();
    let mut record = proposed.mappings[&notion].clone();
    record.mapping.id = private.clone();
    proposed.mappings.insert(private, record);
    let error = server
        .execute_knowledge(
            "partial",
            KnowledgeRequest::Submit {
                store: "team".parse().unwrap(),
                repository: "compiler".into(),
                operation_id: "collision".into(),
                base: observed.version,
                snapshot: proposed,
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::PermissionDenied);
}

#[test]
fn integration_requires_permission_for_both_sides_of_a_backend_change() {
    let (_temp, server, _, _, private) = fixture();
    let observed = read(&server, "owner");
    let mut proposed = observed.snapshot.clone();
    proposed
        .mappings
        .get_mut(&private)
        .unwrap()
        .mapping
        .document
        .backend = "notion".into();
    let proposal = match server
        .execute_knowledge(
            "owner",
            KnowledgeRequest::Submit {
                store: "team".parse().unwrap(),
                repository: "compiler".into(),
                operation_id: "backend-change".into(),
                base: observed.version.clone(),
                snapshot: proposed,
            },
        )
        .unwrap()
    {
        KnowledgeResponse::Proposal { proposal } => proposal,
        _ => panic!(),
    };
    let denied = server
        .execute_knowledge(
            "partial",
            KnowledgeRequest::Integrate {
                store: "team".parse().unwrap(),
                repository: "compiler".into(),
                proposal: proposal.id,
                if_match: observed.version,
            },
        )
        .unwrap_err();
    assert_eq!(denied.kind, ErrorKind::PermissionDenied);
    assert_eq!(
        read(&server, "owner").snapshot.mappings[&private]
            .mapping
            .document
            .backend,
        "private"
    );
}
