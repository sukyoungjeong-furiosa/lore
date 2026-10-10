use lore_core::ErrorKind;
use lore_local::{LoreRemote, RemoteRegistry};

fn remote(store: &str, url: &str) -> LoreRemote {
    LoreRemote {
        store: store.parse().unwrap(),
        server_url: url.into(),
    }
}

#[test]
fn aliases_and_endpoints_are_independent_of_store_identity_and_do_not_retarget_implicitly() {
    let temp = tempfile::tempdir().unwrap();
    let registry = RemoteRegistry::new(temp.path());
    registry
        .add(
            "personal",
            remote("personal-store", "http://localhost:7777"),
        )
        .unwrap();
    registry
        .add("team", remote("team-store", "https://example.invalid/lore"))
        .unwrap();
    registry
        .add(
            "team-replica",
            remote("team-store", "https://replica.invalid"),
        )
        .unwrap();
    assert_eq!(
        registry.get("team").unwrap().store,
        registry.get("team-replica").unwrap().store
    );
    assert_eq!(
        registry
            .add("team", remote("other-store", "https://example.invalid"))
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    registry
        .set_url("team", "https://new-host.invalid/lore".into())
        .unwrap();
    assert_eq!(registry.get("team").unwrap().store.as_str(), "team-store");
    let before = serde_json::to_value(registry.list().unwrap()).unwrap();
    for url in [
        "file:///tmp/store",
        "https://user:secret@example.invalid",
        "https://example.invalid?token=secret",
        "https://example.invalid#fragment",
    ] {
        assert!(registry.set_url("team", url.into()).is_err());
    }
    assert!(
        registry
            .add("../bad", remote("store", "https://example.invalid"))
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(registry.list().unwrap()).unwrap(),
        before
    );
    registry.remove("team").unwrap();
    assert!(registry.get("team-replica").is_ok());
}

#[test]
fn concurrent_connection_changes_do_not_lose_other_remotes() {
    let temp = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        for n in 0..8 {
            let home = temp.path();
            scope.spawn(move || {
                RemoteRegistry::new(home)
                    .add(
                        &format!("remote-{n}"),
                        remote("same-store", "http://localhost:7777"),
                    )
                    .unwrap();
            });
        }
    });
    assert_eq!(RemoteRegistry::new(temp.path()).list().unwrap().len(), 8);
}
