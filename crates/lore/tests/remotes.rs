use serde_json::Value;
use std::{path::Path, process::Command};

fn run(home: &Path, repo: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_lore"))
        .args(["--repo", repo.to_str().unwrap(), "--json"])
        .args(args)
        .env("LORE_HOME", home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
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

#[test]
fn remotes_can_be_configured_before_initialization_and_reused_without_sharing_working_state() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("knowledge");
    let repo = temp.path().join("code");
    let worktree = temp.path().join("feature");
    std::fs::create_dir_all(&repo).unwrap();
    run(
        &home,
        &repo,
        &[
            "remote",
            "add",
            "team",
            "--url",
            "https://example.invalid",
            "--store",
            "team-store",
        ],
    );
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "test"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["commit", "--allow-empty", "-qm", "initial"]);
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
    run(&home, &repo, &["init"]);
    run(&home, &worktree, &["init"]);
    run(&home, &repo, &["create", "Private draft"]);
    let before = run(&home, &repo, &["status"]);
    assert_eq!(
        before["workspace"]["documents"].as_object().unwrap().len(),
        1
    );
    assert!(
        run(&home, &worktree, &["status"])["workspace"]["documents"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        run(&home, &repo, &["remote", "ls"]),
        run(&home, &worktree, &["remote", "ls"])
    );
    run(
        &home,
        &worktree,
        &["remote", "set-url", "team", "http://localhost:7777"],
    );
    let connection = run(&home, &repo, &["remote", "show", "team"]);
    assert_eq!(connection["remote"]["store"], "team-store");
    assert_eq!(connection["remote"]["server_url"], "http://localhost:7777");
    assert_eq!(run(&home, &repo, &["status"]), before);
    run(&home, &worktree, &["remote", "remove", "team"]);
    assert_eq!(run(&home, &repo, &["status"]), before);
    assert!(
        run(&home, &repo, &["remote", "ls"])["remotes"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}
