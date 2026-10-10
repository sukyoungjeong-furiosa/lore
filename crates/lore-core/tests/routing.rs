use lore_core::*;
use serde::Serialize;

#[test]
fn adding_optional_scopes_preserves_preexisting_mapping_revision_identity() {
    #[derive(Serialize)]
    struct PreviousMapping {
        id: MappingId,
        document: DocumentReference,
        anchors: Vec<Anchor>,
        context: String,
        archived: bool,
    }
    #[derive(Serialize)]
    struct PreviousRevision {
        parent: Option<RevisionId>,
        actor: Actor,
        at_ms: u64,
        mapping: PreviousMapping,
    }
    let snapshot = "fn original() {}\n".to_string();
    let previous = PreviousRevision {
        parent: None,
        actor: Actor::Human,
        at_ms: 123,
        mapping: PreviousMapping {
            id: MappingId::new(),
            document: DocumentReference {
                backend: "team".into(),
                id: "page".into(),
            },
            anchors: vec![Anchor {
                id: DocumentId::new(),
                path: "code.rs".into(),
                start: 1,
                end: 1,
                context: CodeContext {
                    repository: "repo".into(),
                    head: None,
                    working_tree: true,
                    content_digest: hash(&snapshot),
                },
                snapshot,
            }],
            context: "Original context".into(),
            archived: false,
        },
    };
    let bytes = serde_json::to_vec(&previous).unwrap();
    let loaded: MappingRevision = serde_json::from_slice(&bytes).unwrap();
    loaded.mapping.validate().unwrap();
    assert!(loaded.mapping.scopes.is_empty());
    assert_eq!(loaded.id().unwrap(), RevisionId::of(bytes));
}

#[test]
fn glob_boundaries_repository_identity_and_duplicate_mappings_remain_explicit() {
    let reference = LoreDocument::Backend {
        reference: DocumentReference {
            backend: "team".into(),
            id: "page".into(),
        },
    };
    let route = RoutingDocument {
        store: None,
        mapping: None,
        document: reference,
        title: None,
        scopes: vec![PathScope {
            repository: "repo".into(),
            glob: "src/scheduler/*.rs".into(),
        }],
        anchors: vec![],
    };
    assert!(
        discover_lore(
            std::slice::from_ref(&route),
            "repo",
            &["src/scheduler/nested/file.rs".into()]
        )
        .unwrap()
        .is_empty()
    );
    assert!(
        discover_lore(
            std::slice::from_ref(&route),
            "different-repo",
            &["src/scheduler/file.rs".into()]
        )
        .unwrap()
        .is_empty()
    );
    let result = discover_lore(
        &[route.clone(), route],
        "repo",
        &["src/scheduler/file.rs".into()],
    )
    .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].matches.len(), 1);
}
