use lore_core::*;

fn mapping() -> Mapping {
    let snapshot = "fn schedule() {}\n".to_string();
    Mapping {
        id: MappingId::new(),
        document: DocumentReference {
            backend: "team-docs".into(),
            id: "page".into(),
        },
        anchors: vec![Anchor {
            id: DocumentId::new(),
            path: "scheduler.rs".into(),
            start: 1,
            end: 1,
            context: CodeContext {
                repository: "compiler".into(),
                head: Some("original".into()),
                working_tree: false,
                content_digest: hash(&snapshot),
            },
            snapshot,
        }],
        scopes: vec![PathScope {
            repository: "compiler".into(),
            glob: "src/**".into(),
        }],
        context: String::new(),
        archived: false,
    }
}

#[test]
fn a_mapping_can_span_files_and_revisions_but_not_code_repositories() {
    let mut value = mapping();
    let mut another = value.anchors[0].clone();
    another.id = DocumentId::new();
    another.path = "planner.rs".into();
    another.context.head = Some("later".into());
    value.anchors.push(another);
    value.validate().unwrap();
    assert_eq!(value.code_repository().unwrap(), "compiler");
    value.anchors[1].context.repository = "runtime".into();
    assert_eq!(value.validate().unwrap_err().kind, ErrorKind::InvalidInput);
    value.anchors.pop();
    value.scopes[0].repository = "runtime".into();
    assert_eq!(value.validate().unwrap_err().kind, ErrorKind::InvalidInput);
    value.scopes.clear();
    value.anchors[0].context.repository.clear();
    assert_eq!(value.validate().unwrap_err().kind, ErrorKind::InvalidInput);
    value.anchors.clear();
    assert_eq!(value.validate().unwrap_err().kind, ErrorKind::InvalidInput);
}

#[test]
fn repository_catalogs_reject_foreign_anchors_and_scopes() {
    let value = mapping();
    let id = value.id.clone();
    let view = MappingView {
        revision: RevisionId::of("revision"),
        record: MappingRevision {
            parent: None,
            actor: Actor::Human,
            at_ms: 1,
            mapping: value,
        },
    };
    let mut catalog = MappingCatalog::empty("compiler");
    catalog.mappings.insert(
        id.clone(),
        CatalogEntry {
            view: view.into(),
            title: None,
        },
    );
    catalog.validate("compiler").unwrap();
    catalog
        .mappings
        .get_mut(&id)
        .unwrap()
        .view
        .record
        .mapping
        .anchors[0]
        .context
        .repository = "runtime".into();
    assert_eq!(
        catalog.validate("compiler").unwrap_err().kind,
        ErrorKind::InvalidInput
    );
    let summary = &mut catalog.mappings.get_mut(&id).unwrap().view.record.mapping;
    summary.anchors[0].context.repository = "compiler".into();
    summary.scopes[0].repository = "runtime".into();
    assert_eq!(
        catalog.validate("compiler").unwrap_err().kind,
        ErrorKind::InvalidInput
    );
}
