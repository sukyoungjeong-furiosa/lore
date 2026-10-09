use lore_core::ErrorKind;
use lore_local::{BackendConfig, CredentialStore};

fn config(api_url: &str) -> BackendConfig {
    BackendConfig::Notion {
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
        roots, token_env, ..
    } = &mut same_endpoint;
    roots.push("another-page".into());
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
