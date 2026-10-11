use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Setup {
    _temp: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    peer: PathBuf,
}

impl Setup {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("code");
        let peer = temp.path().join("feature");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("scheduler.rs"), "fn schedule() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "test"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["add", "."],
            vec!["commit", "-qm", "initial"],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["worktree", "add", "-qb", "feature", peer.to_str().unwrap()])
                .output()
                .unwrap()
                .status
                .success()
        );
        let setup = Self {
            home: temp.path().join("knowledge"),
            _temp: temp,
            repo,
            peer,
        };
        setup.ok(&setup.repo, &["init"], None);
        setup.ok(&setup.peer, &["init"], None);
        setup
    }

    fn run(&self, repo: &Path, args: &[&str], body: Option<&str>) -> Output {
        use std::io::Write;
        let mut child = Command::new(env!("CARGO_BIN_EXE_lore"))
            .args([
                "--json",
                "--author",
                "agent",
                "--repo",
                repo.to_str().unwrap(),
            ])
            .args(args)
            .env("LORE_HOME", &self.home)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(body) = body {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(body.as_bytes())
                .unwrap();
        } else {
            drop(child.stdin.take());
        }
        child.wait_with_output().unwrap()
    }

    fn ok(&self, repo: &Path, args: &[&str], body: Option<&str>) -> Value {
        let output = self.run(repo, args, body);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn edit(&self, repo: &Path, id: &str, revision: &str, body: &str) -> Value {
        self.ok(
            repo,
            &["edit", id, "--if-match", revision, "--stdin"],
            Some(body),
        )
    }
}

fn commit(s: &Setup, repo: &Path, view: &Value) -> Value {
    s.ok(
        repo,
        &[
            "commit",
            view["document"]["id"].as_str().unwrap(),
            "--if-match",
            view["revision"].as_str().unwrap(),
        ],
        None,
    )
}
fn read(s: &Setup, repo: &Path, id: &str) -> Value {
    s.ok(repo, &["show", id], None)
}
fn central(s: &Setup, id: &str) -> Value {
    s.ok(&s.repo, &["--central", "show", id], None)
}

#[test]
fn commit_is_private_until_submission_and_other_branches_pull_explicitly() {
    let s = Setup::new();
    let created = s.ok(
        &s.peer,
        &["create", "Scheduler design", "--stdin"],
        Some("Resource allocation constraint"),
    );
    let id = created["document"]["id"].as_str().unwrap();
    let attached = s.ok(
        &s.peer,
        &[
            "attach",
            id,
            "scheduler.rs:1",
            "--if-match",
            created["revision"].as_str().unwrap(),
        ],
        None,
    );
    let committed = commit(&s, &s.peer, &attached);
    assert_eq!(committed["document"], attached["document"]);
    assert!(
        s.ok(&s.repo, &["--central", "search"], None)
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.ok(&s.repo, &["search"], None)
            .as_array()
            .unwrap()
            .is_empty()
    );
    let before = s.ok(&s.repo, &["status"], None)["workspace"].clone();
    assert_eq!(s.ok(&s.peer, &["push"], None)["status"], "integrated");
    assert_eq!(
        central(&s, id)["document"]["content"],
        "Resource allocation constraint"
    );
    assert!(
        s.ok(&s.repo, &["search"], None)
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.ok(&s.repo, &["status"], None)["workspace"], before);
    assert_eq!(
        s.ok(
            &s.repo,
            &[
                "--central",
                "context",
                "discover",
                "scheduler.rs",
                "--cached"
            ],
            None
        )["documents"][0]["document"]["id"],
        id
    );
    s.ok(&s.repo, &["pull"], None);
    assert_eq!(
        read(&s, &s.repo, id)["document"]["content"],
        "Resource allocation constraint"
    );
    assert!(
        s.ok(&s.repo, &["status"], None)["workspace"]["documents"]
            .get(id)
            .is_none()
    );
    assert_eq!(
        s.ok(&s.repo, &["history", id], None)
            .as_array()
            .unwrap()
            .len(),
        1,
        "Private save revisions are not published as committed history"
    );
    let modified = s.edit(
        &s.peer,
        id,
        committed["revision"].as_str().unwrap(),
        "Private follow-up",
    );
    commit(&s, &s.peer, &modified);
    assert_eq!(
        central(&s, id)["document"]["content"],
        "Resource allocation constraint"
    );
    s.ok(&s.peer, &["push"], None);
    assert_eq!(central(&s, id)["document"]["content"], "Private follow-up");
    assert_eq!(
        read(&s, &s.repo, id)["document"]["content"],
        "Resource allocation constraint"
    );
}

#[test]
fn proposals_are_inspectable_and_integration_is_conditional_and_idempotent() {
    let s = Setup::new();
    let document = s.ok(&s.peer, &["create", "Proposed design"], None);
    commit(&s, &s.peer, &document);
    let submitted = s.ok(&s.peer, &["push", "--proposal"], None);
    let id = submitted["proposal"]["id"].as_str().unwrap();
    assert_eq!(
        s.ok(&s.repo, &["--central", "search"], None),
        serde_json::json!([])
    );
    let inspected = s.ok(&s.repo, &["proposal", "show", id], None);
    assert_eq!(
        inspected["proposed"]["snapshot"]["documents"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    let expected = inspected["central"]["version"].as_str().unwrap();
    let local = s.ok(&s.repo, &["create", "Independent design"], None);
    commit(&s, &s.repo, &local);
    s.ok(&s.repo, &["push"], None);
    assert_eq!(
        s.run(
            &s.repo,
            &["proposal", "integrate", id, "--if-match", expected],
            None
        )
        .status
        .code(),
        Some(3)
    );
    let current = s.ok(&s.repo, &["worktree"], None)["central_version"]
        .as_str()
        .unwrap()
        .to_owned();
    let incorporated = s.ok(
        &s.repo,
        &["proposal", "integrate", id, "--if-match", &current],
        None,
    );
    assert_eq!(
        incorporated["snapshot"]["documents"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    let repeated = s.ok(
        &s.repo,
        &["proposal", "integrate", id, "--if-match", expected],
        None,
    );
    assert_eq!(repeated["version"], incorporated["version"]);
}

#[test]
fn local_fast_forward_policy_requires_pull_but_does_not_reapply_accepted_commits() {
    let s = Setup::new();
    let status = s.ok(&s.repo, &["status"], None);
    let path = PathBuf::from(
        status["workspace"]["local_store"]["directory"]
            .as_str()
            .unwrap(),
    )
    .join("store.json");
    let mut descriptor: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    descriptor["integration_policy"] = "fast_forward_only".into();
    std::fs::write(path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
    let first = s.ok(&s.peer, &["create", "Agent A"], None);
    commit(&s, &s.peer, &first);
    let accepted = s.ok(&s.peer, &["push"], None);
    assert_eq!(accepted["status"], "integrated");
    let second = s.ok(&s.repo, &["create", "Agent B"], None);
    commit(&s, &s.repo, &second);
    let rejected = s.run(&s.repo, &["push"], None);
    assert_eq!(rejected.status.code(), Some(3));
    let rejected: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(rejected["status"], "conflict");
    assert_eq!(
        s.ok(&s.repo, &["worktree"], None)["central_version"],
        accepted["central"]["version"]
    );
    s.ok(&s.repo, &["pull"], None);
    let advanced = s.ok(&s.repo, &["push"], None);
    assert_eq!(advanced["status"], "integrated");
    assert_ne!(
        advanced["central"]["version"],
        accepted["central"]["version"]
    );
    let repeated = s.ok(&s.peer, &["push"], None);
    assert_eq!(
        repeated["central"]["version"],
        accepted["central"]["version"]
    );
    assert_eq!(
        s.ok(&s.repo, &["worktree"], None)["central_version"],
        advanced["central"]["version"]
    );
}

#[test]
fn concurrent_document_changes_preserve_the_proposal_until_explicit_resolution() {
    let s = Setup::new();
    let initial = s.ok(&s.repo, &["create", "Design", "--stdin"], Some("Base"));
    let initial = commit(&s, &s.repo, &initial);
    s.ok(&s.repo, &["push"], None);
    s.ok(&s.peer, &["pull"], None);
    let id = initial["document"]["id"].as_str().unwrap();
    let peer = read(&s, &s.peer, id);
    let local = s.edit(
        &s.repo,
        id,
        initial["revision"].as_str().unwrap(),
        "Owner conclusion",
    );
    commit(&s, &s.repo, &local);
    s.ok(&s.repo, &["push"], None);
    let proposal = s.edit(
        &s.peer,
        id,
        peer["revision"].as_str().unwrap(),
        "Agent conclusion",
    );
    let proposal = commit(&s, &s.peer, &proposal);
    let failed = s.run(&s.peer, &["push"], None);
    assert_eq!(failed.status.code(), Some(3));
    let response: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(response["status"], "conflict");
    assert_eq!(central(&s, id)["document"]["content"], "Owner conclusion");
    assert_eq!(
        read(&s, &s.peer, id)["document"]["content"],
        "Agent conclusion"
    );
    assert_eq!(s.run(&s.peer, &["pull"], None).status.code(), Some(3));
    let shared = central(&s, id);
    assert_eq!(
        s.run(
            &s.peer,
            &[
                "resolve-conflict",
                id,
                "--if-match",
                initial["revision"].as_str().unwrap(),
                "--if-shared",
                shared["revision"].as_str().unwrap(),
                "--take",
                "local"
            ],
            None
        )
        .status
        .code(),
        Some(3)
    );
    let resolved = s.ok(
        &s.peer,
        &[
            "resolve-conflict",
            id,
            "--if-match",
            proposal["revision"].as_str().unwrap(),
            "--if-shared",
            shared["revision"].as_str().unwrap(),
            "--take",
            "local",
        ],
        None,
    );
    assert_eq!(resolved["document"]["content"], "Agent conclusion");
    assert_eq!(central(&s, id)["document"]["content"], "Owner conclusion");
    commit(&s, &s.peer, &resolved);
    s.ok(&s.peer, &["push"], None);
    assert_eq!(central(&s, id)["document"]["content"], "Agent conclusion");
}

#[test]
fn pulling_commits_preserves_pending_editor_changes_and_requires_baseline_resolution() {
    let s = Setup::new();
    let original = s.ok(&s.repo, &["create", "Design", "--stdin"], Some("Base"));
    let original = commit(&s, &s.repo, &original);
    s.ok(&s.repo, &["push"], None);
    s.ok(&s.peer, &["pull"], None);
    let id = original["document"]["id"].as_str().unwrap();
    let selected = read(&s, &s.peer, id);
    let pending = s.edit(
        &s.peer,
        id,
        selected["revision"].as_str().unwrap(),
        "Uncommitted draft",
    );
    let edited = s.edit(
        &s.repo,
        id,
        original["revision"].as_str().unwrap(),
        "Accepted update",
    );
    commit(&s, &s.repo, &edited);
    s.ok(&s.repo, &["push"], None);
    s.ok(&s.peer, &["pull"], None);
    assert_eq!(read(&s, &s.peer, id)["revision"], pending["revision"]);
    assert_eq!(
        s.run(
            &s.peer,
            &[
                "commit",
                id,
                "--if-match",
                pending["revision"].as_str().unwrap()
            ],
            None
        )
        .status
        .code(),
        Some(3)
    );
    assert_eq!(
        read(&s, &s.peer, id)["document"]["content"],
        "Uncommitted draft"
    );
    let accepted = central(&s, id);
    let resolved = s.ok(
        &s.peer,
        &[
            "resolve-conflict",
            id,
            "--if-match",
            pending["revision"].as_str().unwrap(),
            "--if-shared",
            accepted["revision"].as_str().unwrap(),
            "--take",
            "shared",
        ],
        None,
    );
    assert_eq!(resolved["document"]["content"], "Accepted update");
}

#[test]
fn worktrees_are_real_isolated_checkouts_and_direct_file_changes_are_preserved() {
    let s = Setup::new();
    let a = s.ok(&s.repo, &["worktree"], None);
    let b = s.ok(&s.peer, &["worktree"], None);
    assert_ne!(a["worktree"]["branch"], b["worktree"]["branch"]);
    assert_ne!(a["worktree"]["path"], b["worktree"]["path"]);
    let created = s.ok(
        &s.peer,
        &["create", "Document", "--stdin"],
        Some("# Markdown\n"),
    );
    let committed = commit(&s, &s.peer, &created);
    let id = created["document"]["id"].as_str().unwrap();
    let path = PathBuf::from(b["worktree"]["path"].as_str().unwrap());
    let body = path.join("documents").join(id).join("content.md");
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "# Markdown\n");
    assert!(path.join(".git").is_file());
    assert!(!path.join("workspace.json").exists());
    std::fs::write(&body, "Direct user edit").unwrap();
    assert_eq!(s.run(&s.peer, &["push"], None).status.code(), Some(3));
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "Direct user edit");
    std::fs::write(&body, "# Markdown\n").unwrap();
    let checkout = s.ok(&s.peer, &["checkout", "experiment", "--create"], None);
    assert_eq!(checkout["branch"], "experiment");
    assert_eq!(read(&s, &s.peer, id)["document"], committed["document"]);
    let changed = s.edit(
        &s.peer,
        id,
        committed["revision"].as_str().unwrap(),
        "Pending change",
    );
    assert_eq!(
        s.run(
            &s.peer,
            &["checkout", b["worktree"]["branch"].as_str().unwrap()],
            None
        )
        .status
        .code(),
        Some(3)
    );
    assert_eq!(read(&s, &s.peer, id)["revision"], changed["revision"]);
    assert_eq!(
        s.run(&s.peer, &["--central", "create", "Forbidden"], None)
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn document_commit_retries_heal_the_workspace_without_duplicating_history() {
    let s = Setup::new();
    let created = s.ok(&s.peer, &["create", "Retry"], None);
    let committed = commit(&s, &s.peer, &created);
    let id = created["document"]["id"].as_str().unwrap();
    let status = s.ok(&s.peer, &["status"], None);
    let state_path = status["state_path"].as_str().unwrap();
    let mut state = status["workspace"].clone();
    state["documents"][id]["shared_baseline"] = Value::Null;
    std::fs::write(state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    let repeated = commit(&s, &s.peer, &committed);
    assert_eq!(repeated["revision"], committed["revision"]);
    assert_eq!(
        s.ok(&s.peer, &["history", id], None)
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let edited = s.edit(
        &s.peer,
        id,
        committed["revision"].as_str().unwrap(),
        "Later edit",
    );
    assert_eq!(
        s.run(
            &s.peer,
            &[
                "commit",
                id,
                "--if-match",
                committed["revision"].as_str().unwrap()
            ],
            None
        )
        .status
        .code(),
        Some(3)
    );
    assert_eq!(read(&s, &s.peer, id)["revision"], edited["revision"]);
}

#[test]
fn committed_mappings_follow_the_same_branch_submission_contract() {
    let s = Setup::new();
    s.ok(
        &s.peer,
        &[
            "backend",
            "add-notion",
            "notion",
            "--root",
            "00000000-0000-4000-8000-000000000001",
        ],
        None,
    );
    let mapping = s.ok(
        &s.peer,
        &[
            "map",
            "create",
            "notion",
            "00000000-0000-4000-8000-000000000001",
            "scheduler.rs:1",
            "--context",
            "Scheduler background",
        ],
        None,
    );
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    s.ok(
        &s.peer,
        &[
            "commit",
            id,
            "--mapping",
            "--if-match",
            mapping["revision"].as_str().unwrap(),
        ],
        None,
    );
    assert!(
        s.ok(&s.repo, &["--central", "map", "ls"], None)["mappings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !s.run(&s.peer, &["--central", "map", "show", id], None)
            .status
            .success()
    );
    s.ok(&s.peer, &["push"], None);
    assert_eq!(
        s.ok(&s.repo, &["--central", "map", "ls"], None)["mappings"][0]["view"]["record"]["mapping"]
            ["id"],
        id
    );
    s.ok(&s.repo, &["pull"], None);
    let selected = s.ok(&s.repo, &["map", "show", id], None);
    let edit = s.ok(
        &s.repo,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            selected["revision"].as_str().unwrap(),
            "--context",
            "New explanation",
        ],
        None,
    );
    s.ok(
        &s.repo,
        &[
            "commit",
            id,
            "--mapping",
            "--if-match",
            edit["revision"].as_str().unwrap(),
        ],
        None,
    );
    s.ok(&s.repo, &["push"], None);
    assert_eq!(
        s.ok(&s.repo, &["--central", "map", "show", id], None)["record"]["mapping"]["context"],
        "New explanation"
    );
}

#[test]
fn pending_mapping_cannot_overwrite_a_pulled_branch_revision() {
    let s = Setup::new();
    s.ok(
        &s.repo,
        &[
            "backend",
            "add-notion",
            "notion",
            "--root",
            "00000000-0000-4000-8000-000000000001",
        ],
        None,
    );
    let mapping = s.ok(
        &s.repo,
        &[
            "map",
            "create",
            "notion",
            "00000000-0000-4000-8000-000000000001",
            "scheduler.rs:1",
        ],
        None,
    );
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    let record = |repo: &Path, view: &Value| {
        s.ok(
            repo,
            &[
                "commit",
                id,
                "--mapping",
                "--if-match",
                view["revision"].as_str().unwrap(),
            ],
            None,
        )
    };
    let committed = record(&s.repo, &mapping);
    s.ok(&s.repo, &["push"], None);
    s.ok(&s.peer, &["pull"], None);
    let pending = s.ok(
        &s.repo,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            committed["revision"].as_str().unwrap(),
            "--context",
            "Pending explanation",
        ],
        None,
    );
    let peer = s.ok(&s.peer, &["map", "show", id], None);
    let changed = s.ok(
        &s.peer,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            peer["revision"].as_str().unwrap(),
            "--context",
            "Accepted explanation",
        ],
        None,
    );
    record(&s.peer, &changed);
    s.ok(&s.peer, &["push"], None);
    s.ok(&s.repo, &["pull"], None);
    assert_eq!(
        s.ok(&s.repo, &["map", "show", id], None)["revision"],
        pending["revision"]
    );
    assert_eq!(
        s.run(
            &s.repo,
            &[
                "commit",
                id,
                "--mapping",
                "--if-match",
                pending["revision"].as_str().unwrap()
            ],
            None
        )
        .status
        .code(),
        Some(3)
    );
    assert_eq!(
        s.ok(&s.repo, &["--central", "map", "show", id], None)["record"]["mapping"]["context"],
        "Accepted explanation"
    );
    assert!(
        !s.run(
            &s.peer,
            &[
                "--central",
                "map",
                "archive",
                id,
                "--if-match",
                changed["revision"].as_str().unwrap()
            ],
            None
        )
        .status
        .success()
    );
    let accepted = s.ok(&s.repo, &["--central", "map", "show", id], None);
    let resolved = s.ok(
        &s.repo,
        &[
            "map",
            "resolve-conflict",
            id,
            "--if-match",
            pending["revision"].as_str().unwrap(),
            "--if-shared",
            accepted["revision"].as_str().unwrap(),
            "--take",
            "local",
        ],
        None,
    );
    record(&s.repo, &resolved);
    s.ok(&s.repo, &["push"], None);
    assert_eq!(
        s.ok(&s.repo, &["--central", "map", "show", id], None)["record"]["mapping"]["context"],
        "Pending explanation"
    );
    assert!(
        s.ok(&s.repo, &["--central", "map", "history", id], None)
            .as_array()
            .unwrap()
            .len()
            >= 3
    );
    s.ok(
        &s.peer,
        &["checkout", "another-working-branch", "--create"],
        None,
    );
}

#[test]
fn initialization_migrates_published_legacy_mappings_without_publishing_drafts() {
    let s = Setup::new();
    s.ok(
        &s.repo,
        &[
            "backend",
            "add-notion",
            "notion",
            "--root",
            "00000000-0000-4000-8000-000000000001",
        ],
        None,
    );
    let shared = s.ok(
        &s.repo,
        &[
            "map",
            "create",
            "notion",
            "00000000-0000-4000-8000-000000000001",
            "scheduler.rs:1",
        ],
        None,
    );
    let id = shared["record"]["mapping"]["id"].as_str().unwrap();
    let workspace = lore_local::Workspace::open(&s.home, &s.repo).unwrap();
    workspace
        .publish_mapping(
            &s.home,
            &id.parse().unwrap(),
            &shared["revision"].as_str().unwrap().parse().unwrap(),
            None,
        )
        .unwrap();
    let draft = s.ok(
        &s.repo,
        &[
            "map",
            "create",
            "notion",
            "00000000-0000-4000-8000-000000000002",
            "scheduler.rs:1",
        ],
        None,
    );
    s.ok(&s.repo, &["init"], None);
    assert_eq!(
        s.ok(&s.repo, &["--central", "map", "ls"], None)["mappings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        !s.run(
            &s.repo,
            &[
                "--central",
                "map",
                "show",
                draft["record"]["mapping"]["id"].as_str().unwrap()
            ],
            None
        )
        .status
        .success()
    );
    s.ok(&s.repo, &["pull"], None);
    let accepted = s.ok(&s.repo, &["map", "show", id], None);
    let edit = s.ok(
        &s.repo,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            accepted["revision"].as_str().unwrap(),
            "--context",
            "Editable after migration",
        ],
        None,
    );
    s.ok(
        &s.repo,
        &[
            "commit",
            id,
            "--mapping",
            "--if-match",
            edit["revision"].as_str().unwrap(),
        ],
        None,
    );
    s.ok(&s.repo, &["push"], None);
    assert_eq!(
        s.ok(&s.repo, &["--central", "map", "show", id], None)["record"]["mapping"]["context"],
        "Editable after migration"
    );
}
