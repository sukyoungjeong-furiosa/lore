use lore_core::*;
use lore_store::GitKnowledgeStore;
use std::process::Command;

fn store() -> (tempfile::TempDir, GitKnowledgeStore) {
    let temp = tempfile::tempdir().unwrap();
    let store = GitKnowledgeStore {
        directory: temp.path().into(),
        repository: "compiler".into(),
    };
    store.initialize().unwrap();
    (temp, store)
}

fn document() -> Revision {
    Revision {
        parent: None,
        actor: Actor::Agent,
        at_ms: 1,
        document: Document::new("Design".into(), "Original".into(), Actor::Agent).unwrap(),
    }
}

fn mapping(backend: &str, document: &str) -> MappingRevision {
    MappingRevision {
        parent: None,
        actor: Actor::Agent,
        at_ms: 1,
        mapping: Mapping {
            id: MappingId::new(),
            document: DocumentReference {
                backend: backend.into(),
                id: document.into(),
            },
            anchors: vec![],
            scopes: vec![PathScope {
                repository: "compiler".into(),
                glob: "src/**".into(),
            }],
            context: "Original mapping".into(),
            archived: false,
        },
    }
}

#[test]
fn publication_requires_owned_documents_but_allows_external_and_archived_references() {
    let (_temp, store) = store();
    let base = store.read_version("main").unwrap();
    let mut snapshot = KnowledgeSnapshot::default();
    let mut record = document();
    let connection = mapping("local", record.document.id.as_str());
    snapshot
        .mappings
        .insert(connection.mapping.id.clone(), connection);
    let error = store
        .submit("missing-document", &base.version, &snapshot)
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidInput);
    assert!(error.message.contains(record.document.id.as_str()));
    assert!(store.proposals().unwrap().is_empty());

    record.document.archived = true;
    snapshot
        .documents
        .insert(record.document.id.clone(), record);
    let external = mapping("notion", "page-without-a-cached-body");
    snapshot
        .mappings
        .insert(external.mapping.id.clone(), external);
    let proposal = store.submit("complete", &base.version, &snapshot).unwrap();
    let accepted = store.integrate(&proposal.id, &base.version).unwrap();
    assert_eq!(accepted.snapshot.mappings.len(), 2);
}

#[test]
fn a_conflict_rejects_the_whole_proposal_without_publishing_its_independent_mapping_change() {
    let (_temp, store) = store();
    let base = store.read_version("main").unwrap();
    let record = document();
    let did = record.document.id.clone();
    let mapping = mapping("notion", "page");
    let mid = mapping.mapping.id.clone();
    let mut initial = KnowledgeSnapshot::default();
    initial.documents.insert(did.clone(), record);
    initial.mappings.insert(mid.clone(), mapping);
    let seed = store.submit("seed", &base.version, &initial).unwrap();
    let base = store.integrate(&seed.id, &base.version).unwrap();
    let mut a = base.snapshot.clone();
    a.documents.get_mut(&did).unwrap().document.content = "Agent A".into();
    let mut b = base.snapshot.clone();
    b.documents.get_mut(&did).unwrap().document.content = "Agent B".into();
    b.mappings.get_mut(&mid).unwrap().mapping.context = "Independent change".into();
    let p1 = store.submit("a", &base.version, &a).unwrap();
    let p2 = store.submit("b", &base.version, &b).unwrap();
    let central = store.integrate(&p1.id, &base.version).unwrap();
    assert_eq!(
        store.integrate(&p2.id, &central.version).unwrap_err().kind,
        ErrorKind::Conflict
    );
    let after = store.read_version("main").unwrap();
    assert_eq!(after.version, central.version);
    assert_eq!(
        after.snapshot.mappings[&mid].mapping.context,
        "Original mapping"
    );
    assert!(
        store
            .proposals()
            .unwrap()
            .iter()
            .find(|p| p.id == p2.id)
            .unwrap()
            .integrated
            .is_none()
    );
}

#[test]
fn integration_checks_references_after_combining_independent_entity_changes() {
    let (_temp, store) = store();
    let base = store.read_version("main").unwrap();
    let record = document();
    let did = record.document.id.clone();
    let mapping = mapping("notion", "page");
    let mid = mapping.mapping.id.clone();
    let mut initial = KnowledgeSnapshot::default();
    initial.documents.insert(did.clone(), record);
    initial.mappings.insert(mid.clone(), mapping);
    let seed = store.submit("seed", &base.version, &initial).unwrap();
    let base = store.integrate(&seed.id, &base.version).unwrap();
    let mut deletion = base.snapshot.clone();
    deletion.documents.remove(&did);
    let mut connection = base.snapshot.clone();
    connection.mappings.get_mut(&mid).unwrap().mapping.document = DocumentReference {
        backend: "local".into(),
        id: did.to_string(),
    };
    let p1 = store
        .submit("remove-document", &base.version, &deletion)
        .unwrap();
    let p2 = store
        .submit("connect-document", &base.version, &connection)
        .unwrap();
    let central = store.integrate(&p1.id, &base.version).unwrap();
    assert_eq!(
        store.integrate(&p2.id, &central.version).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(store.read_version("main").unwrap().version, central.version);
}

#[test]
fn unintegrated_proposals_remain_reachable_after_git_gc() {
    let (_temp, store) = store();
    let base = store.read_version("main").unwrap();
    let record = document();
    let mut snapshot = KnowledgeSnapshot::default();
    snapshot
        .documents
        .insert(record.document.id.clone(), record);
    let proposal = store.submit("pending", &base.version, &snapshot).unwrap();
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(store.git_directory())
        .args(["gc", "--prune=now"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        store.read_version(&proposal.head).unwrap().snapshot,
        snapshot
    );
    assert_eq!(
        store.read_version(&proposal.base).unwrap().version,
        base.version
    );
    assert!(store.proposals().unwrap()[0].integrated.is_none());
}
