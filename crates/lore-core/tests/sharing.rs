use lore_core::*;
use std::collections::{BTreeMap, BTreeSet};

fn store(id: &str) -> KnowledgeStoreDescriptor {
    KnowledgeStoreDescriptor {
        id: id.parse().unwrap(),
        integration_policy: Default::default(),
        repositories: BTreeMap::from([
            (
                "compiler".into(),
                RepositoryRegistration {
                    canonical_ref: Some("upstream/master".into()),
                },
            ),
            (
                "kernels".into(),
                RepositoryRegistration {
                    canonical_ref: Some("origin/main".into()),
                },
            ),
        ]),
        backends: BTreeSet::from(["design-docs".into(), "local-docs".into()]),
    }
}

fn mapping(repository: &str, backend: &str) -> Mapping {
    Mapping {
        id: MappingId::new(),
        document: DocumentReference {
            backend: backend.into(),
            id: "page".into(),
        },
        anchors: vec![],
        scopes: vec![PathScope {
            repository: repository.into(),
            glob: "src/**".into(),
        }],
        context: String::new(),
        archived: false,
    }
}

#[test]
fn store_membership_allows_multiple_repositories_and_backends_without_merging_stores() {
    let team = store("team-store");
    let mut personal = store("personal-store");
    personal
        .repositories
        .get_mut("compiler")
        .unwrap()
        .canonical_ref = Some("origin/experiment".into());
    let mut value = mapping("compiler", "design-docs");
    team.validate_mapping(&value).unwrap();
    personal.validate_mapping(&value).unwrap();
    assert_ne!(
        SharedMappingKey {
            store: team.id.clone(),
            mapping: value.id.clone()
        },
        SharedMappingKey {
            store: personal.id.clone(),
            mapping: value.id.clone()
        },
    );
    assert_ne!(
        team.repositories["compiler"].canonical_ref,
        personal.repositories["compiler"].canonical_ref
    );
    team.validate_mapping(&mapping("kernels", "local-docs"))
        .unwrap();
    value.document.backend = "unregistered".into();
    assert_eq!(
        team.validate_mapping(&value).unwrap_err().kind,
        ErrorKind::InvalidInput
    );
    value.document.backend = "design-docs".into();
    value.scopes[0].repository = "unregistered".into();
    assert_eq!(
        team.validate_mapping(&value).unwrap_err().kind,
        ErrorKind::InvalidInput
    );
}

#[test]
fn catalogs_are_repository_views_of_a_store_and_reject_unregistered_backends() {
    let team = store("team-store");
    for repository in ["compiler", "kernels"] {
        let value = mapping(repository, "design-docs");
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
        let mut catalog = MappingCatalog::empty(repository);
        catalog.mappings.insert(
            id.clone(),
            CatalogEntry {
                view: view.into(),
                title: None,
            },
        );
        team.validate_catalog(&catalog).unwrap();
        catalog
            .mappings
            .get_mut(&id)
            .unwrap()
            .view
            .record
            .mapping
            .document
            .backend = "other-store-only".into();
        assert_eq!(
            team.validate_catalog(&catalog).unwrap_err().kind,
            ErrorKind::InvalidInput
        );
    }
    assert!(
        team.validate_catalog(&MappingCatalog::empty("unregistered"))
            .is_err()
    );
}

#[test]
fn shared_configuration_allows_empty_registrations_and_rejects_credential_fields() {
    let empty = KnowledgeStoreDescriptor {
        id: "empty".parse().unwrap(),
        integration_policy: Default::default(),
        repositories: BTreeMap::new(),
        backends: BTreeSet::new(),
    };
    empty.validate().unwrap();
    assert!(" \n".parse::<KnowledgeStoreId>().is_err());
    let mut value = serde_json::to_value(store("team-store")).unwrap();
    value["token"] = serde_json::json!("should-not-be-shared");
    assert!(serde_json::from_value::<KnowledgeStoreDescriptor>(value).is_err());
    let mut invalid = store("team-store");
    invalid
        .repositories
        .get_mut("compiler")
        .unwrap()
        .canonical_ref = Some("".into());
    assert!(invalid.validate().is_err());
}
