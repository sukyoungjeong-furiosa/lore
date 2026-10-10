use lore_core::ErrorKind;
use lore_local::{ModelConnection, ResolverProfile, ResolverRegistry};

fn profile(connection: &str, model: &str) -> ResolverProfile {
    ResolverProfile {
        connection: connection.into(),
        model: model.into(),
        system_prompt: None,
        timeout_seconds: None,
        max_input_bytes: None,
        max_output_tokens: None,
    }
}

#[test]
fn profiles_share_connections_preserve_options_and_protect_references() {
    let temp = tempfile::tempdir().unwrap();
    let registry = ResolverRegistry::new(temp.path());
    assert!(registry.read().unwrap().connections.is_empty());
    registry
        .set_connection(
            "access",
            ModelConnection {
                base_url: "https://example.invalid/v1".into(),
                token_env: "MODEL_TOKEN".into(),
            },
        )
        .unwrap();
    let mut first = profile("access", "first");
    first.system_prompt = Some("Custom projection prompt".into());
    first.max_output_tokens = Some(128);
    registry.set_profile("first", first).unwrap();
    registry
        .set_profile("second", profile("access", "second"))
        .unwrap();
    registry.use_profile("first").unwrap();
    registry
        .set_profile("first", profile("access", "changed"))
        .unwrap();
    let settings = registry.read().unwrap();
    assert_eq!(
        settings.profile("first").unwrap().max_output_tokens,
        Some(128)
    );
    assert_eq!(
        settings.profile("first").unwrap().system_prompt.as_deref(),
        Some("Custom projection prompt")
    );
    assert_eq!(settings.profile("first").unwrap().model, "changed");
    assert_eq!(
        registry.remove_connection("access").err().unwrap().kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        registry.remove_profile("first").err().unwrap().kind,
        ErrorKind::Conflict
    );
    registry.use_profile("second").unwrap();
    registry.remove_profile("first").unwrap();
    assert_eq!(
        registry.read().unwrap().default_profile.as_deref(),
        Some("second")
    );
    registry.clear_default().unwrap();
    registry.remove_profile("second").unwrap();
    registry.remove_connection("access").unwrap();
    assert!(registry.read().unwrap().connections.is_empty());
}

#[test]
fn invalid_configuration_does_not_replace_valid_settings() {
    let temp = tempfile::tempdir().unwrap();
    let registry = ResolverRegistry::new(temp.path());
    registry
        .set_connection(
            "access",
            ModelConnection {
                base_url: "https://example.invalid/v1".into(),
                token_env: "MODEL_TOKEN".into(),
            },
        )
        .unwrap();
    let before = serde_json::to_value(registry.read().unwrap()).unwrap();
    for url in [
        "file:///tmp/api",
        "https://user:secret@example.invalid/v1",
        "https://example.invalid/v1?token=secret",
        "https://example.invalid/v1#fragment",
    ] {
        assert!(
            registry
                .set_connection(
                    "access",
                    ModelConnection {
                        base_url: url.into(),
                        token_env: "MODEL_TOKEN".into()
                    }
                )
                .is_err()
        );
    }
    assert!(
        registry
            .set_profile("missing", profile("unknown", "model"))
            .is_err()
    );
    assert!(registry.use_profile("missing").is_err());
    let mut invalid = profile("access", "model");
    invalid.timeout_seconds = Some(0);
    assert!(registry.set_profile("invalid", invalid).is_err());
    assert_eq!(
        serde_json::to_value(registry.read().unwrap()).unwrap(),
        before
    );
}

#[test]
fn concurrent_profile_updates_do_not_lose_other_profiles() {
    let temp = tempfile::tempdir().unwrap();
    ResolverRegistry::new(temp.path())
        .set_connection(
            "access",
            ModelConnection {
                base_url: "https://example.invalid/v1".into(),
                token_env: "MODEL_TOKEN".into(),
            },
        )
        .unwrap();
    std::thread::scope(|scope| {
        for n in 0..8 {
            let home = temp.path();
            scope.spawn(move || {
                ResolverRegistry::new(home)
                    .set_profile(&format!("profile-{n}"), profile("access", "model"))
                    .unwrap();
            });
        }
    });
    assert_eq!(
        ResolverRegistry::new(temp.path())
            .read()
            .unwrap()
            .profiles
            .len(),
        8
    );
}
