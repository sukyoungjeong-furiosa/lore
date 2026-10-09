use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

const PAGE: &str = "00000000-0000-4000-8000-000000000001";

struct Setup {
    temp: tempfile::TempDir,
    repo: std::path::PathBuf,
}

impl Setup {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("code");
        std::fs::create_dir_all(repo.join("src/scheduler")).unwrap();
        std::fs::write(
            repo.join("src/scheduler/plan.rs"),
            "// private source snapshot\nfn plan() {}\n",
        )
        .unwrap();
        let setup = Self { temp, repo };
        setup.git(&["init", "-q"]);
        setup.git(&["config", "user.email", "test@example.invalid"]);
        setup.git(&["config", "user.name", "test"]);
        setup.git(&["add", "."]);
        setup.git(&["commit", "-qm", "initial"]);
        setup.ok(&["init"]);
        setup.ok(&["backend", "add-notion", "team", "--root", PAGE]);
        setup
    }

    fn git(&self, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    fn command(&self, repo: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lore"));
        command
            .args(["--json", "--repo"])
            .arg(repo)
            .args(args)
            .env("LORE_HOME", self.temp.path().join("knowledge"))
            .env_remove("NOTION_TOKEN")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn ok(&self, args: &[&str]) -> Value {
        parse(self.command(&self.repo, args).output().unwrap())
    }

    fn scoped(&self, page: &str) -> Value {
        self.ok(&["map", "create", "team", page, "--glob", "src/scheduler/**"])
    }

    fn event(&self, session: &str) -> Value {
        json!({
            "hook_event_name": "PostToolUse", "session_id": session, "cwd": self.repo,
            "transcript_path": self.temp.path().join(format!("{session}.jsonl")),
            "tool_name": "Bash", "tool_input": {"command": "any-tool src/scheduler/plan.rs"},
            "tool_response": {"output": "fn plan() {}"},
        })
    }

    fn adapter(&self, args: &[&str]) -> Command {
        let mut command = Command::new("python3");
        command
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../integrations/codex/lore_context.py"
            ))
            .args(args)
            .env("LORE_EXECUTABLE", env!("CARGO_BIN_EXE_lore"))
            .env("LORE_CODEX_REFRESH", "0")
            .env("LORE_HOME", self.temp.path().join("knowledge"))
            .env_remove("NOTION_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn start_hook(&self, event: &Value, args: &[&str]) -> std::process::Child {
        let mut child = self.adapter(args).spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        child
    }

    fn hook(&self, event: &Value) -> Output {
        self.start_hook(event, &[]).wait_with_output().unwrap()
    }

    fn introduce(&self, repo: &Path, session: &str, limit: &str, path: &str) -> Vec<Value> {
        let mut event = self.event(session);
        event["cwd"] = json!(repo);
        event["tool_input"] = json!({"path": path});
        hook_documents(
            self.start_hook(&event, &["--session", session, "--limit", limit])
                .wait_with_output()
                .unwrap(),
        )
    }
}

fn parse(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn hook_documents(output: Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if output.stdout.is_empty() {
        return vec![];
    }
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(envelope.get("decision").is_none());
    let context = envelope["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(!context.contains("private source snapshot"));
    serde_json::from_str(context.split_once('\n').unwrap().1).unwrap()
}

#[test]
fn automatic_introduction_is_once_per_document_and_explicit_access_remains_available() {
    let s = Setup::new();
    let mapping = s.scoped(PAGE);
    s.ok(&["map", "create", "team", PAGE, "src/scheduler/plan.rs:2"]);
    let local = s.ok(&["create", "Local design"]);
    let local_id = local["document"]["id"].as_str().unwrap();
    s.ok(&["attach", local_id, "src/scheduler/plan.rs:2"]);
    let first = hook_documents(s.hook(&s.event("session-a")));
    assert_eq!(first.len(), 2);
    let backend = first
        .iter()
        .find(|document| document["document"]["source"] == "backend")
        .unwrap();
    assert_eq!(backend["matches"].as_array().unwrap().len(), 2);
    assert!(
        backend["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["kind"] == "recorded_anchor" && m["start"] == 2)
    );
    assert!(hook_documents(s.hook(&s.event("session-a"))).is_empty());
    let mut resumed = s.event("session-a");
    resumed["transcript_path"] = json!("different-transcript-after-compaction.jsonl");
    assert!(hook_documents(s.hook(&resumed)).is_empty());
    s.ok(&[
        "map",
        "edit",
        mapping["record"]["mapping"]["id"].as_str().unwrap(),
        "--if-match",
        mapping["revision"].as_str().unwrap(),
        "--context",
        "New explanation",
    ]);
    assert!(hook_documents(s.hook(&s.event("session-a"))).is_empty());
    assert_eq!(hook_documents(s.hook(&s.event("session-b"))).len(), 2);
    assert_eq!(
        s.ok(&["context", "discover", "src/scheduler/plan.rs"])["documents"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        s.ok(&["show", local_id])["document"]["title"],
        "Local design"
    );
    let state = s.ok(&["status"]);
    let index =
        Path::new(state["state_path"].as_str().unwrap()).with_file_name("routing-index.json");
    assert!(
        !std::fs::read_to_string(index)
            .unwrap()
            .contains("private source snapshot")
    );
}

#[test]
fn explicit_session_identity_supports_hosts_without_a_session_id() {
    let s = Setup::new();
    s.scoped(PAGE);
    let mut event = s.event("unused");
    event.as_object_mut().unwrap().remove("session_id");
    assert!(!s.hook(&event).status.success());
    let introduce = || {
        hook_documents(
            s.start_hook(&event, &["--session", "host-session"])
                .wait_with_output()
                .unwrap(),
        )
    };
    assert_eq!(introduce().len(), 1);
    assert!(introduce().is_empty());
}

#[test]
fn concurrent_introductions_and_limits_do_not_claim_unsent_documents() {
    let s = Setup::new();
    s.scoped(PAGE);
    let children: Vec<_> = (0..4)
        .map(|_| s.start_hook(&s.event("race"), &[]))
        .collect();
    let total: usize = children
        .into_iter()
        .map(|child| hook_documents(child.wait_with_output().unwrap()).len())
        .sum();
    assert_eq!(total, 1);
    for n in 2..=5 {
        s.scoped(&format!("00000000-0000-4000-8000-{n:012}"));
    }
    for expected in [2, 2, 1, 0] {
        assert_eq!(
            s.introduce(&s.repo, "bounded", "2", "src/scheduler/plan.rs")
                .len(),
            expected
        );
    }
    assert_eq!(
        s.ok(&["context", "discover", "src/scheduler/plan.rs"])["documents"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn byte_budget_does_not_mark_omitted_documents_as_introduced() {
    let s = Setup::new();
    let directory = format!("src/scheduler/{}", "a".repeat(850));
    let glob = format!("{directory}/**");
    let path = format!("{directory}/plan.rs");
    for index in 1..=8 {
        s.ok(&[
            "map",
            "create",
            "team",
            &format!("00000000-0000-4000-8000-{index:012}"),
            "--glob",
            &glob,
        ]);
    }
    let mut introduced = std::collections::BTreeSet::new();
    for _ in 0..8 {
        let documents = s.introduce(&s.repo, "bytes", "20", &path);
        assert!(serde_json::to_vec(&documents).unwrap().len() < 6100);
        for document in documents {
            assert!(
                introduced.insert(
                    document["document"]["reference"]["id"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                )
            );
        }
    }
    assert_eq!(introduced.len(), 8);
}

#[test]
fn scope_installation_is_worktree_specific_but_session_deduplication_is_shared() {
    let s = Setup::new();
    s.scoped(PAGE);
    let peer = s.temp.path().join("peer");
    s.git(&["worktree", "add", "-qb", "peer", peer.to_str().unwrap()]);
    parse(s.command(&peer, &["init"]).output().unwrap());
    assert!(
        parse(
            s.command(&peer, &["context", "discover", "src/scheduler/plan.rs"])
                .output()
                .unwrap()
        )["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let result = parse(
        s.command(
            &peer,
            &[
                "map",
                "install",
                "--from",
                s.repo.to_str().unwrap(),
                "--path",
                "src/scheduler",
            ],
        )
        .output()
        .unwrap(),
    );
    assert_eq!(result["results"].as_array().unwrap().len(), 1);
    assert_eq!(
        s.introduce(&s.repo, "shared", "8", "src/scheduler/plan.rs")
            .len(),
        1
    );
    assert!(
        s.introduce(&peer, "shared", "8", "src/scheduler/plan.rs")
            .is_empty()
    );
    assert_eq!(
        parse(
            s.command(&peer, &["context", "discover", "src/scheduler/plan.rs"])
                .output()
                .unwrap()
        )["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn hook_uses_path_mentions_and_separates_agent_threads_without_parsing_commands() {
    let s = Setup::new();
    s.scoped(PAGE);
    let mut event = s.event("paths");
    event["tool_input"] = json!({"file_path": "src/scheduler/a file.rs"});
    assert_eq!(hook_documents(s.hook(&event)).len(), 1);
    event["agent_id"] = json!("child-agent");
    assert_eq!(hook_documents(s.hook(&event)).len(), 1);
    event["agent_id"] = Value::Null;
    event["tool_input"] =
        json!({"paths": ["../../src/scheduler/plan.rs", "/other-checkout/src/scheduler/plan.rs"]});
    assert!(hook_documents(s.hook(&event)).is_empty());
    event["session_id"] = json!("outputs");
    event["tool_input"] = json!({"command": "a tool with no paths"});
    event["tool_response"] = json!({"content": [{"type": "text", "text": "src/scheduler/plan.rs:2: discovered reference"}]});
    assert_eq!(hook_documents(s.hook(&event)).len(), 1);
    event["session_id"] = json!("irrelevant");
    event["tool_response"] = json!({"output": "no relevant path"});
    assert!(hook_documents(s.hook(&event)).is_empty());
    event["session_id"] = json!("nested-cwd");
    event["cwd"] = json!(s.repo.join("src"));
    event["tool_input"] = json!({"file_path": "scheduler/plan.rs"});
    assert_eq!(hook_documents(s.hook(&event)).len(), 1);
    event["session_id"] = json!("tool-workdir");
    event["cwd"] = json!(s.repo);
    event["tool_input"] = json!({"workdir": "src/scheduler", "command": "anything plan.rs"});
    assert_eq!(hook_documents(s.hook(&event)).len(), 1);
}

#[test]
fn routing_refreshes_on_archive_and_invalid_globs_do_not_change_workspace_state() {
    let s = Setup::new();
    let mapping = s.scoped(PAGE);
    assert_eq!(
        s.ok(&["map", "ls", "src/scheduler/plan.rs"])["mappings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.ok(&["context", "discover", "src/scheduler"])["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let state = s.ok(&["status"]);
    for glob in ["/absolute/**", "../outside/**", "src/[broken"] {
        let result = s
            .command(&s.repo, &["map", "create", "team", PAGE, "--glob", glob])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(s.ok(&["status"]), state);
    }
    let archived = s.ok(&[
        "map",
        "archive",
        mapping["record"]["mapping"]["id"].as_str().unwrap(),
        "--if-match",
        mapping["revision"].as_str().unwrap(),
    ]);
    assert!(
        s.ok(&["context", "discover", "src/scheduler/plan.rs"])["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    s.ok(&[
        "map",
        "restore",
        mapping["record"]["mapping"]["id"].as_str().unwrap(),
        "--if-match",
        archived["revision"].as_str().unwrap(),
    ]);
    assert_eq!(
        s.ok(&["context", "discover", "src/scheduler/plan.rs"])["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn session_start_prepares_worktrees_and_shared_discovery_does_not_expose_drafts() {
    let s = Setup::new();
    let shared = s.scoped(PAGE);
    let id = shared["record"]["mapping"]["id"].as_str().unwrap();
    let revision = shared["revision"].as_str().unwrap();
    s.scoped("00000000-0000-4000-8000-000000000002");
    let peer = s.temp.path().join("fresh-worktree");
    s.git(&["worktree", "add", "-qb", "fresh", peer.to_str().unwrap()]);
    let mut start = s.event("fresh");
    start["hook_event_name"] = json!("SessionStart");
    start["cwd"] = json!(peer);
    assert!(hook_documents(s.hook(&start)).is_empty());
    let status = parse(s.command(&peer, &["status"]).output().unwrap());
    assert_eq!(status["workspace"]["mappings"], json!({}));
    assert!(
        s.introduce(&peer, "fresh", "8", "src/scheduler/plan.rs")
            .is_empty()
    );
    s.ok(&["map", "publish", id, "--if-match", revision]);
    let published_state = s.ok(&["status"]);
    s.ok(&["map", "publish", id, "--if-match", revision]);
    assert_eq!(
        s.ok(&["status"]),
        published_state,
        "Retrying the same publication is idempotent"
    );
    assert_eq!(
        s.introduce(&peer, "fresh", "8", "src/scheduler/plan.rs")
            .len(),
        1
    );
    assert_eq!(
        parse(s.command(&peer, &["status"]).output().unwrap()),
        status,
        "Shared discovery does not activate mappings or change workspace baselines"
    );
    let draft = s.ok(&["map", "archive", id, "--if-match", revision]);
    assert!(
        s.ok(&["context", "discover", "src/scheduler/plan.rs"])["documents"]
            .as_array()
            .unwrap()
            .iter()
            .all(|doc| doc["document"]["reference"]["id"] != PAGE)
    );
    assert_eq!(
        parse(
            s.command(&peer, &["context", "discover", "src/scheduler/plan.rs"])
                .output()
                .unwrap()
        )["documents"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "Local archive does not change shared discovery"
    );
    let conflict = s
        .command(
            &s.repo,
            &[
                "map",
                "publish",
                id,
                "--if-match",
                draft["revision"].as_str().unwrap(),
            ],
        )
        .output()
        .unwrap();
    assert_eq!(conflict.status.code(), Some(3));
    s.ok(&[
        "map",
        "publish",
        id,
        "--if-match",
        draft["revision"].as_str().unwrap(),
        "--if-shared",
        revision,
    ]);
    assert!(
        parse(
            s.command(&peer, &["context", "discover", "src/scheduler/plan.rs"])
                .output()
                .unwrap()
        )["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_worktree_created_mid_session_is_prepared_from_structured_tool_workdir() {
    let s = Setup::new();
    let mapping = s.scoped(PAGE);
    s.ok(&[
        "map",
        "publish",
        mapping["record"]["mapping"]["id"].as_str().unwrap(),
        "--if-match",
        mapping["revision"].as_str().unwrap(),
    ]);
    let peer = s.temp.path().join("mid-session");
    s.git(&["worktree", "add", "-qb", "mid", peer.to_str().unwrap()]);
    let mut event = s.event("worktree-session");
    event["tool_input"] = json!({"workdir": peer, "command": "anything src/scheduler/plan.rs"});
    assert_eq!(hook_documents(s.hook(&event)).len(), 1);
    assert!(
        parse(s.command(&peer, &["status"]).output().unwrap())["workspace"]["mappings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    event["tool_input"] = json!({"file_path": peer.join("src/scheduler/plan.rs")});
    assert!(
        hook_documents(s.hook(&event)).is_empty(),
        "Changing worktree or tool does not reset session deduplication"
    );
}
