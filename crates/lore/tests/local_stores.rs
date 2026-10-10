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
fn worktrees_share_published_mappings_without_a_server_and_preserve_private_edits() {
    let setup = Setup::new();
    let initial = setup.ok(&setup.repo, &["status"]);
    let other = setup.ok(&setup.worktree, &["status"]);
    let store = initial["workspace"]["local_store"]["store"]
        .as_str()
        .unwrap();
    assert_eq!(
        initial["workspace"]["local_store"],
        other["workspace"]["local_store"]
    );
    assert!(
        !initial["workspace"]["local_store"]["directory"]
            .as_str()
            .unwrap()
            .starts_with(setup.repo.to_str().unwrap())
    );
    assert!(
        setup.ok(&setup.repo, &["remote", "ls"])["remotes"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let created = setup.create(&setup.repo, "notion");
    let id = created["record"]["mapping"]["id"].as_str().unwrap();
    let first = created["revision"].as_str().unwrap();
    assert!(
        setup.ok(
            &setup.worktree,
            &["context", "discover", "src/scheduler.rs", "--cached"]
        )["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let pushed = setup.ok(&setup.repo, &["push", id]);
    assert_eq!(pushed["receipt"]["store"], store);
    assert_eq!(pushed["receipt"]["reconciliation"], "not_required");
    let discovered = setup.ok(
        &setup.worktree,
        &["context", "discover", "src/scheduler.rs", "--cached"],
    );
    assert_eq!(
        discovered["documents"][0]["shared_mappings"][0]["store"],
        store
    );
    assert!(
        setup.ok(&setup.worktree, &["map", "ls"])["mappings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let before_fetch = setup.ok(&setup.worktree, &["status"]);
    let metadata = setup.ok(&setup.worktree, &["fetch"]);
    assert!(!metadata.to_string().contains("snapshot"));
    setup.ok(&setup.worktree, &["fetch", "--id", id]);
    assert_eq!(before_fetch, setup.ok(&setup.worktree, &["status"]));
    git(
        &setup.worktree,
        &["mv", "src/scheduler.rs", "src/planner.rs"],
    );
    git(&setup.worktree, &["commit", "-qm", "rename"]);
    setup.ok(&setup.worktree, &["pull", "--path", "src/planner.rs"]);
    assert_eq!(
        setup.ok(&setup.worktree, &["map", "resolve", id])["projections"][0]["locations"][0]["path"],
        "src/planner.rs"
    );
    let a = setup.ok(
        &setup.repo,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            first,
            "--context",
            "main draft",
        ],
    );
    let b = setup.ok(
        &setup.worktree,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            first,
            "--context",
            "feature draft",
        ],
    );
    assert_eq!(
        setup.ok(&setup.repo, &["store", "show", "personal"])["descriptor"]["repositories"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    let visible = setup.ok(&setup.worktree, &["fetch", "--id", id]);
    assert_eq!(visible["revisions"][0]["revision"], first);
    setup.ok(&setup.repo, &["push", id]);
    let rejected = setup.run(&setup.worktree, &["push", id]);
    assert_eq!(
        serde_json::from_slice::<Value>(&rejected.stderr).unwrap()["error"]["kind"],
        "conflict"
    );
    let conflicted = setup.run(&setup.worktree, &["pull", "--id", id]);
    assert_eq!(conflicted.status.code(), Some(3));
    assert_eq!(
        setup.ok(&setup.worktree, &["map", "show", id])["revision"],
        b["revision"]
    );
    setup.ok(
        &setup.worktree,
        &[
            "map",
            "resolve-conflict",
            id,
            "--if-match",
            b["revision"].as_str().unwrap(),
            "--if-shared",
            a["revision"].as_str().unwrap(),
            "--take",
            "local",
        ],
    );
    setup.ok(&setup.worktree, &["push", id]);
    setup.ok(&setup.repo, &["pull", "--id", id]);
    let shared = setup.ok(&setup.repo, &["map", "show", id]);
    assert_eq!(shared["record"]["mapping"]["context"], "feature draft");
    assert_eq!(
        shared["record"]["mapping"]["anchors"][0]["context"]["head"],
        created["record"]["mapping"]["anchors"][0]["context"]["head"]
    );
    assert_eq!(
        shared["record"]["mapping"]["anchors"][0]["path"],
        "src/scheduler.rs"
    );
}

#[test]
fn one_personal_store_registers_multiple_repositories_and_backends_without_mixing_catalogs() {
    let setup = Setup::new();
    let first = setup.create(&setup.repo, "notion");
    setup.ok(
        &setup.repo,
        &["push", first["record"]["mapping"]["id"].as_str().unwrap()],
    );
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
    setup.ok(
        &kernels,
        &["push", second["record"]["mapping"]["id"].as_str().unwrap()],
    );
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
        setup.ok(&setup.repo, &["fetch"])["catalog"]["catalog"]["mappings"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        setup.ok(&kernels, &["fetch"])["catalog"]["catalog"]["mappings"]
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
    setup.ok(&setup.repo, &["push", id]);
    setup.ok(
        &setup.repo,
        &[
            "push",
            id,
            "--local-store",
            "personal",
            "--from-local-store",
            "custom",
        ],
    );
    assert!(
        !setup
            .run(&setup.repo, &["map", "show", id])
            .status
            .success()
    );
    let private = setup.ok(
        &setup.repo,
        &[
            "map",
            "edit",
            id,
            "--local-store",
            "custom",
            "--if-match",
            created["revision"].as_str().unwrap(),
            "--context",
            "custom draft",
        ],
    );
    assert_eq!(
        setup.ok(
            &setup.repo,
            &["map", "show", id, "--local-store", "personal"]
        )["record"]["mapping"]["context"],
        "initial"
    );
    setup.ok(&setup.repo, &["push", id]);
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
        setup.ok(&setup.repo, &["map", "show", id, "--local-store", "moved"])["revision"],
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
        setup.ok(&setup.repo, &["fetch"])["catalog"]["catalog"]["mappings"]
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
