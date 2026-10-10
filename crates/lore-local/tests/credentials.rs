use lore_core::ErrorKind;
use lore_local::{BackendConfig, CredentialStore};

fn config(api_url: &str) -> BackendConfig {
    BackendConfig::Notion {
        display_name: None,
        discovery_pages: vec![],
        roots: vec!["page".into()],
        token_env: "TEST_NOTION_TOKEN".into(),
        api_url: api_url.into(),
    }
}

#[test]
fn credentials_are_connection_bound_private_and_removable() {
    let temp = tempfile::tempdir().unwrap();
    let store = CredentialStore::new(temp.path());
    let original = config("https://api.notion.com/v1");
    assert!(store.get("team", &original).unwrap().is_none());
    store.set("team", &original, " token-one\n").unwrap();
    assert_eq!(
        store.get("team", &original).unwrap().as_deref(),
        Some("token-one")
    );
    assert!(store.get("other", &original).unwrap().is_none());
    let mut same_endpoint = config("https://api.notion.com/v1");
    let BackendConfig::Notion {
        display_name,
        roots,
        discovery_pages,
        token_env,
        ..
    } = &mut same_endpoint;
    *display_name = Some("notion".into());
    roots.push("another-page".into());
    discovery_pages.push("discover-this-page".into());
    *token_env = "OTHER_TOKEN_ENV".into();
    assert_eq!(
        store.get("team", &same_endpoint).unwrap().as_deref(),
        Some("token-one")
    );
    let changed = config("https://other.example/v1");
    assert_eq!(
        store.get("team", &changed).unwrap_err().kind,
        ErrorKind::PermissionDenied
    );
    assert!(store.set("team", &original, "a\nb").is_err());
    assert_eq!(
        store.get("team", &original).unwrap().as_deref(),
        Some("token-one")
    );
    store.set("team", &original, "token-two").unwrap();
    assert_eq!(
        store.get("team", &original).unwrap().as_deref(),
        Some("token-two")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let directory = temp.path().join("credentials");
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = std::fs::read_dir(directory)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            store.get("team", &original).unwrap_err().kind,
            ErrorKind::PermissionDenied
        );
    }
    store.remove("team").unwrap();
    store.remove("team").unwrap();
    assert!(store.get("team", &original).unwrap().is_none());
}

#[test]
fn api_credentials_are_isolated_by_namespace_name_and_endpoint() {
    let temp = tempfile::tempdir().unwrap();
    let store = CredentialStore::new(temp.path());
    let endpoint = "https://example.invalid/v1";
    store
        .set("team", &config(endpoint), "notion-token")
        .unwrap();
    store
        .set_connection("resolver", "team", endpoint, "model-token")
        .unwrap();
    assert_eq!(
        store.get("team", &config(endpoint)).unwrap().as_deref(),
        Some("notion-token")
    );
    assert_eq!(
        store
            .get_connection("resolver", "team", endpoint)
            .unwrap()
            .as_deref(),
        Some("model-token")
    );
    assert!(
        store
            .get_connection("other", "team", endpoint)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .get_connection("resolver", "other", endpoint)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_connection("resolver", "team", "https://other.invalid/v1")
            .unwrap_err()
            .kind,
        ErrorKind::PermissionDenied
    );
    store.remove_connection("resolver", "team").unwrap();
    assert!(
        store
            .get_connection("resolver", "team", endpoint)
            .unwrap()
            .is_none()
    );
    assert!(store.get("team", &config(endpoint)).unwrap().is_some());
}
