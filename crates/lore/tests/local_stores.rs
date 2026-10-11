use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Setup {
    temp: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    worktree: PathBuf,
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_code(repo: &Path) {
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/scheduler.rs"), "fn schedule() {}\n").unwrap();
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.name", "test"]);
    git(repo, &["config", "user.email", "test@example.invalid"]);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-qm", "initial"]);
}

impl Setup {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("knowledge");
        let repo = temp.path().join("code");
        let worktree = temp.path().join("feature");
        create_code(&repo);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-qb",
                "feature",
                worktree.to_str().unwrap(),
            ],
        );
        let setup = Self {
            temp,
            home,
            repo,
            worktree,
        };
        setup.ok(&setup.repo, &["init", "--repository-id", "compiler"]);
        setup.ok(&setup.worktree, &["init"]);
        setup.ok(
            &setup.repo,
            &[
                "backend",
                "add-notion",
                "notion",
                "--root",
                "00000000-0000-4000-8000-000000000001",
            ],
        );
        setup
    }

    fn run(&self, repo: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lore"))
            .args(["--json", "--repo", repo.to_str().unwrap()])
            .args(args)
            .env("LORE_HOME", &self.home)
            .env_remove("LORE_REMOTE_TOKEN")
            .output()
            .unwrap()
    }

    fn ok(&self, repo: &Path, args: &[&str]) -> Value {
        let output = self.run(repo, args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn publish(&self, repo: &Path, view: &Value) {
        self.ok(
            repo,
            &[
                "commit",
                view["record"]["mapping"]["id"].as_str().unwrap(),
                "--mapping",
                "--if-match",
                view["revision"].as_str().unwrap(),
            ],
        );
        self.ok(repo, &["push"]);
    }

    fn create(&self, repo: &Path, backend: &str) -> Value {
        self.ok(
            repo,
            &[
                "map",
                "create",
                backend,
                "00000000-0000-4000-8000-000000000001",
                "src/scheduler.rs:1",
                "--context",
                "initial",
            ],
        )
    }
}

#[test]
fn retired_mapping_commands_do_not_publish_uncommitted_changes() {
    let s = Setup::new();
    let mapping = s.create(&s.repo, "notion");
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    let before = s.ok(&s.repo, &["status"]);
    for args in [
        vec!["push", id],
        vec!["fetch", "--id", id],
        vec!["pull", "--id", id],
        vec![
            "map",
            "publish",
            id,
            "--if-match",
            mapping["revision"].as_str().unwrap(),
        ],
    ] {
        let output = s.run(&s.repo, &args);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("commit"));
    }
    assert_eq!(s.ok(&s.repo, &["status"]), before);
    assert!(
        s.ok(&s.repo, &["fetch"])["knowledge"]["snapshot"]["mappings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn one_personal_store_registers_multiple_repositories_and_backends_without_mixing_catalogs() {
    let setup = Setup::new();
    let first = setup.create(&setup.repo, "notion");
    setup.publish(&setup.repo, &first);
    let kernels = setup.temp.path().join("kernels");
    create_code(&kernels);
    setup.ok(&kernels, &["init", "--repository-id", "kernels"]);
    setup.ok(
        &kernels,
        &[
            "backend",
            "add-notion",
            "other",
            "--root",
            "00000000-0000-4000-8000-000000000002",
        ],
    );
    let second = setup.create(&kernels, "other");
    setup.publish(&kernels, &second);
    let descriptor = setup.ok(&kernels, &["store", "show", "personal"]);
    assert_eq!(
        descriptor["descriptor"]["repositories"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        descriptor["descriptor"]["backends"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        setup.ok(&setup.repo, &["fetch"])["knowledge"]["snapshot"]["mappings"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        setup.ok(&kernels, &["fetch"])["knowledge"]["snapshot"]["mappings"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    setup.ok(&setup.worktree, &["pull"]);
    assert_eq!(
        setup.ok(&setup.worktree, &["map", "ls"])["mappings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        setup.ok(&setup.repo, &["status"])["workspace"]["local_store"],
        setup.ok(&kernels, &["status"])["workspace"]["local_store"]
    );
}

#[test]
fn named_stores_are_independent_and_moving_a_store_preserves_its_identity_and_baselines() {
    let setup = Setup::new();
    let directory = setup.temp.path().join("custom-store");
    let custom = setup.ok(
        &setup.repo,
        &[
            "store",
            "create",
            "custom",
            "--path",
            directory.to_str().unwrap(),
        ],
    );
    setup.ok(&setup.repo, &["init", "--local-store", "custom"]);
    let created = setup.create(&setup.repo, "notion");
    let id = created["record"]["mapping"]["id"].as_str().unwrap();
    setup.publish(&setup.repo, &created);
    let personal = setup.ok(&setup.repo, &["store", "show", "personal"]);
    assert_ne!(personal["connection"]["store"], custom["store"]);
    let private = setup.ok(
        &setup.repo,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            created["revision"].as_str().unwrap(),
            "--context",
            "custom draft",
        ],
    );
    let before = setup.ok(&setup.repo, &["status"]);
    let moved = setup.temp.path().join("moved-store");
    std::fs::rename(&directory, &moved).unwrap();
    assert!(!setup.run(&setup.repo, &["pull"]).status.success());
    assert_eq!(before, setup.ok(&setup.repo, &["status"]));
    let reconnected = setup.ok(
        &setup.repo,
        &["store", "add", "moved", moved.to_str().unwrap()],
    );
    assert_eq!(reconnected["store"], custom["store"]);
    setup.ok(&setup.repo, &["init", "--local-store", "moved"]);
    setup.ok(&setup.repo, &["pull"]);
    assert_eq!(
        setup.ok(&setup.repo, &["map", "show", id])["revision"],
        private["revision"]
    );
    assert_eq!(
        setup.ok(&setup.repo, &["status"])["workspace"]["local_store"]["store"],
        custom["store"]
    );
    assert!(
        !setup
            .run(
                &setup.repo,
                &[
                    "store",
                    "create",
                    "inside",
                    "--path",
                    setup.repo.join("notes").to_str().unwrap()
                ]
            )
            .status
            .success()
    );
    assert!(!setup.repo.join("notes").exists());
}

#[test]
fn initializing_an_existing_workspace_preserves_legacy_drafts_and_is_idempotent() {
    let setup = Setup::new();
    let mapping = setup.create(&setup.repo, "notion");
    setup.ok(&setup.repo, &["create", "Private document"]);
    let status = setup.ok(&setup.repo, &["status"]);
    let path = PathBuf::from(status["state_path"].as_str().unwrap());
    let mut legacy = status["workspace"].clone();
    legacy.as_object_mut().unwrap().remove("local_store");
    legacy
        .as_object_mut()
        .unwrap()
        .remove("connected_local_stores");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    for directory in std::fs::read_dir(setup.home.join("checkout-defaults")).unwrap() {
        let path = directory.unwrap().path().join("settings.json");
        let mut defaults: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        defaults.as_object_mut().unwrap().remove("local_store");
        std::fs::write(path, serde_json::to_vec(&defaults).unwrap()).unwrap();
    }
    setup.ok(&setup.repo, &["init"]);
    let upgraded = setup.ok(&setup.repo, &["status"]);
    assert_eq!(upgraded["workspace"]["mappings"], legacy["mappings"]);
    assert_eq!(upgraded["workspace"]["documents"], legacy["documents"]);
    assert_eq!(upgraded["workspace"]["store"], legacy["store"]);
    assert_eq!(
        setup.ok(
            &setup.repo,
            &[
                "map",
                "show",
                mapping["record"]["mapping"]["id"].as_str().unwrap()
            ]
        )["revision"],
        mapping["revision"]
    );
    assert!(
        setup.ok(&setup.repo, &["fetch"])["knowledge"]["snapshot"]["mappings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    setup.ok(&setup.repo, &["init"]);
    assert_eq!(
        std::fs::metadata(path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(upgraded, setup.ok(&setup.repo, &["status"]));
}

#[test]
fn owned_document_mappings_are_independent_and_discoverable_across_worktrees() {
    let s = Setup::new();
    let document = s.ok(&s.repo, &["create", "Scheduler design"]);
    let id = document["document"]["id"].as_str().unwrap();
    let mapping = s.ok(
        &s.repo,
        &[
            "map",
            "create",
            "local",
            id,
            "src/scheduler.rs:1",
            "--glob",
            "src/**",
        ],
    );
    let mid = mapping["record"]["mapping"]["id"].as_str().unwrap();
    assert_eq!(
        s.ok(&s.repo, &["show", id])["revision"],
        document["revision"]
    );
    let found = s.ok(
        &s.repo,
        &["context", "discover", "src/scheduler.rs", "--cached"],
    );
    assert_eq!(found["documents"][0]["document"]["source"], "local");
    assert_eq!(found["documents"][0]["document"]["id"], id);
    assert_eq!(found["documents"][0]["title"], "Scheduler design");
    s.ok(
        &s.repo,
        &[
            "commit",
            mid,
            "--mapping",
            "--if-match",
            mapping["revision"].as_str().unwrap(),
        ],
    );
    let rejected = s.run(&s.repo, &["push"]);
    assert!(
        !rejected.status.success(),
        "A mapping cannot publish a missing owned document"
    );
    assert!(
        s.ok(&s.repo, &["fetch"])["knowledge"]["snapshot"]["mappings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    s.ok(
        &s.repo,
        &[
            "commit",
            id,
            "--if-match",
            document["revision"].as_str().unwrap(),
        ],
    );
    s.ok(&s.repo, &["push"]);
    let peer_before = s.ok(&s.worktree, &["status"]);
    let discovered = s.ok(
        &s.worktree,
        &[
            "--central",
            "context",
            "discover",
            "src/scheduler.rs",
            "--cached",
        ],
    );
    assert_eq!(discovered["documents"][0]["document"]["source"], "local");
    assert_eq!(discovered["documents"][0]["title"], "Scheduler design");
    assert_eq!(discovered["documents"][0]["mappings"][0], mid);
    assert_eq!(s.ok(&s.worktree, &["status"]), peer_before);
    s.ok(&s.worktree, &["pull"]);
    let list = s.ok(
        &s.worktree,
        &["map", "ls", "--backend", "local", "--document", id],
    );
    assert_eq!(list["mappings"].as_array().unwrap().len(), 1);
    assert_eq!(
        s.ok(&s.worktree, &["map", "resolve", mid])["projections"][0]["locations"][0]["path"],
        "src/scheduler.rs"
    );
    let accepted = s.ok(&s.worktree, &["show", id]);
    let view = s.ok(&s.worktree, &["map", "show", mid]);
    let archived = s.ok(
        &s.worktree,
        &[
            "map",
            "archive",
            mid,
            "--if-match",
            view["revision"].as_str().unwrap(),
        ],
    );
    s.ok(
        &s.worktree,
        &[
            "commit",
            mid,
            "--mapping",
            "--if-match",
            archived["revision"].as_str().unwrap(),
        ],
    );
    s.ok(&s.worktree, &["push"]);
    assert_eq!(
        s.ok(&s.worktree, &["--central", "show", id])["revision"],
        accepted["revision"]
    );
    assert!(
        s.ok(
            &s.worktree,
            &[
                "--central",
                "context",
                "discover",
                "src/scheduler.rs",
                "--cached"
            ]
        )["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !s.run(
            &s.repo,
            &[
                "map",
                "create",
                "local",
                "00000000-0000-4000-8000-000000000099",
                "src/scheduler.rs:1"
            ]
        )
        .status
        .success()
    );
}

#[test]
fn initialization_imports_accepted_mapping_store_data_without_exposing_drafts() {
    use lore_core::MappingStoreClient;
    let s = Setup::new();
    let accepted = s.create(&s.repo, "notion");
    let id = accepted["record"]["mapping"]["id"].as_str().unwrap();
    let revision = accepted["revision"].as_str().unwrap();
    let workspace = lore_local::Workspace::open(&s.home, &s.repo).unwrap();
    let connection = workspace.state().unwrap().local_store.unwrap();
    let client = lore_local::LocalMappingClient::new(connection.clone()).unwrap();
    let record = workspace.read_mapping(&id.parse().unwrap(), None).unwrap();
    client
        .publish(&lore_core::PublishMapping {
            operation: "00000000-0000-4000-8000-000000000123".into(),
            expected: None,
            bundle: lore_core::MappingBundle {
                store: connection.store,
                revisions: vec![record],
            },
        })
        .unwrap();
    let draft = s.create(&s.repo, "notion");
    let mut state = workspace.state().unwrap();
    state
        .store_mappings
        .entry(client.descriptor().unwrap().id)
        .or_default()
        .insert(
            draft["record"]["mapping"]["id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            lore_local::ActiveDocument {
                revision: draft["revision"].as_str().unwrap().parse().unwrap(),
                shared_baseline: None,
            },
        );
    std::fs::write(&workspace.state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    s.ok(&s.repo, &["init"]);
    let imported = s.ok(&s.repo, &["--central", "map", "ls"]);
    assert_eq!(imported["mappings"].as_array().unwrap().len(), 1);
    assert_eq!(imported["mappings"][0]["view"]["revision"], revision);
    assert_eq!(
        s.ok(&s.repo, &["status"])["workspace"]["mappings"]
            [draft["record"]["mapping"]["id"].as_str().unwrap()]["revision"],
        draft["revision"]
    );
    let central = s.ok(&s.repo, &["fetch"]);
    s.ok(&s.repo, &["init"]);
    assert_eq!(
        s.ok(&s.repo, &["fetch"]),
        central,
        "Migration does not create repeated integrations"
    );
    assert!(
        !s.run(
            &s.repo,
            &[
                "backend",
                "add-notion",
                "local",
                "--root",
                "00000000-0000-4000-8000-000000000001"
            ]
        )
        .status
        .success()
    );
}
