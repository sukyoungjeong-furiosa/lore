use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

struct Setup {
    temp: TempDir,
}

impl Setup {
    fn new() -> Self {
        let this = Self {
            temp: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir(this.repo()).unwrap();
        this.git(&["init", "-q"]);
        this.git(&["config", "user.email", "test@example.invalid"]);
        this.git(&["config", "user.name", "test"]);
        std::fs::create_dir(this.repo().join("compiler")).unwrap();
        std::fs::write(
            this.repo().join("compiler/scheduler.py"),
            "# scheduler\ndef schedule():\n    return 42\n",
        )
        .unwrap();
        this.git(&["add", "."]);
        this.git(&["commit", "-qm", "initial"]);
        this.ok(&["init"]);
        this
    }

    fn repo(&self) -> std::path::PathBuf {
        self.temp.path().join("code")
    }

    fn git(&self, args: &[&str]) {
        let o = Command::new("git")
            .arg("-C")
            .arg(self.repo())
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{:?}", o);
    }

    fn run_at(&self, repo: &Path, args: &[&str], input: Option<&str>) -> Output {
        use std::io::Write;
        let mut child = Command::new(env!("CARGO_BIN_EXE_lore"))
            .arg("--json")
            .arg("--repo")
            .arg(repo)
            .args(args)
            .env("LORE_HOME", self.temp.path().join("knowledge"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = input {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        } else {
            drop(child.stdin.take());
        }
        child.wait_with_output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_at(&self.repo(), args, None)
    }

    fn ok(&self, args: &[&str]) -> Value {
        parse(self.run(args))
    }

    fn edit(&self, id: &str, revision: &str, body: &str) -> Value {
        parse(self.run_at(
            &self.repo(),
            &[
                "--author",
                "agent",
                "edit",
                id,
                "--if-match",
                revision,
                "--stdin",
            ],
            Some(body),
        ))
    }
}

fn parse(o: Output) -> Value {
    assert!(
        o.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap()
}

#[test]
fn document_history_cas_curation_and_archive() {
    let s = Setup::new();
    let created = s.ok(&[
        "--author",
        "agent",
        "create",
        "Scheduler Design",
        "--tag",
        "design",
    ]);
    let id = text(&created["document"], "id");
    let first = text(&created, "revision");
    let attached = s.ok(&[
        "attach",
        id,
        "compiler/scheduler.py:2-3",
        "--if-match",
        first,
    ]);
    let reviewed = s.ok(&[
        "curate",
        id,
        "--if-match",
        text(&attached, "revision"),
        "--human",
        "--by",
        "Reviewer",
    ]);
    assert_eq!(reviewed["curated"], true);
    assert!(
        !s.run(&[
            "--author",
            "agent",
            "curate",
            id,
            "--if-match",
            text(&reviewed, "revision"),
            "--human",
            "--by",
            "Fake"
        ])
        .status
        .success()
    );
    let title = s.ok(&[
        "edit",
        id,
        "--if-match",
        text(&reviewed, "revision"),
        "--title",
        "Planner Design",
    ]);
    assert_eq!(title["curated"], true);
    let stale = s.run(&["edit", id, "--if-match", first, "--title", "lost update"]);
    assert_eq!(stale.status.code(), Some(3));
    assert_eq!(
        serde_json::from_slice::<Value>(&stale.stderr).unwrap()["error"]["kind"],
        "conflict"
    );
    let changed = s.edit(
        id,
        text(&title, "revision"),
        "# Planner\nKeep scheduling deterministic.\n",
    );
    assert_eq!(changed["curated"], false);
    assert_eq!(
        s.ok(&["search", "deterministic"]).as_array().unwrap().len(),
        1
    );
    assert_eq!(
        s.ok(&["search", "--exclude-agent"])
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        s.ok(&["search", "--curated-only"])
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let archived = s.ok(&["archive", id, "--if-match", text(&changed, "revision")]);
    assert!(s.ok(&["search"]).as_array().unwrap().is_empty());
    assert_eq!(
        s.ok(&["search", "--include-archived"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(s.ok(&["show", id])["document"]["archived"], true);
    s.ok(&["restore", id, "--if-match", text(&archived, "revision")]);
    assert_eq!(s.ok(&["history", id]).as_array().unwrap().len(), 7);
    assert_eq!(
        s.ok(&["show", id, "--revision", first])["document"]["title"],
        "Scheduler Design"
    );
    assert_eq!(
        std::fs::read_to_string(s.repo().join("compiler/scheduler.py")).unwrap(),
        "# scheduler\ndef schedule():\n    return 42\n"
    );
    assert!(!s.repo().join(".lore").exists());
}

#[test]
fn worktree_install_uses_target_projection_and_isolates_changes() {
    let s = Setup::new();
    let created = s.ok(&["create", "Design"]);
    let id = text(&created["document"], "id");
    let original = s.ok(&["attach", id, "compiler/scheduler.py:2-3"]);
    let peer = s.temp.path().join("peer");
    s.git(&["worktree", "add", "-qb", "feature", peer.to_str().unwrap()]);
    parse(s.run_at(&peer, &["init"], None));
    assert!(
        parse(s.run_at(&peer, &["ls"], None))["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    std::fs::rename(
        peer.join("compiler/scheduler.py"),
        peer.join("compiler/planner.py"),
    )
    .unwrap();
    let installed = parse(s.run_at(
        &peer,
        &[
            "install",
            "--from",
            s.repo().to_str().unwrap(),
            "--path",
            "compiler/planner.py",
        ],
        None,
    ));
    assert_eq!(installed["results"][0]["status"], "installed");
    let found = parse(s.run_at(&peer, &["ls", "compiler/planner.py"], None));
    assert_eq!(
        found["documents"][0]["projections"][0]["locations"][0]["path"],
        "compiler/planner.py"
    );
    assert_eq!(
        found["documents"][0]["view"]["document"]["anchors"][0]["path"],
        "compiler/scheduler.py"
    );
    let changed = s.edit(id, text(&original, "revision"), "original checkout changed");
    assert_ne!(
        changed["revision"],
        parse(s.run_at(&peer, &["show", id], None))["revision"]
    );
    let output = s.run_at(
        &peer,
        &["install", id, "--from", s.repo().to_str().unwrap()],
        None,
    );
    assert!(!output.status.success());
    let failed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failed["results"][0]["error"]["kind"], "conflict");
}

#[test]
fn ambiguous_locations_and_links_remain_explicit() {
    let s = Setup::new();
    let target = s.ok(&["create", "Target"]);
    let id = text(&target["document"], "id");
    s.ok(&["attach", id, "compiler/scheduler.py:2-3"]);
    std::fs::write(
        s.repo().join("compiler/scheduler.py"),
        "def schedule():\n    return 42\n\ndef schedule():\n    return 42\n",
    )
    .unwrap();
    let resolved = s.ok(&["resolve", id]);
    assert!(
        resolved["projections"][0]["locations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        resolved["projections"][0]["alternatives"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let other = s.ok(&["create", "Target"]);
    let page = s.ok(&["create", "Wiki"]);
    let pid = text(&page["document"], "id");
    s.edit(
        pid,
        text(&page, "revision"),
        &format!(
            "[[Target]] [[id:{id}|stable]]\n\n``[[Target]]``\n\n    [[Target]]\n\n```\n[[Target]]\n```\n\\[[Target]]\n"
        ),
    );
    let graph = s.ok(&["links", pid]);
    assert_eq!(graph["outgoing"].as_array().unwrap().len(), 2);
    assert_eq!(graph["outgoing"][0]["matches"].as_array().unwrap().len(), 2);
    let changed = s.ok(&[
        "edit",
        id,
        "--if-match",
        text(&s.ok(&["show", id]), "revision"),
        "--title",
        "Renamed",
    ]);
    assert_eq!(changed["document"]["id"], target["document"]["id"]);
    assert_eq!(s.ok(&["links", id])["backlinks"][0]["id"], json!(pid));
    assert_eq!(
        s.ok(&["links", pid])["outgoing"][1]["matches"][0]["title"],
        "Renamed"
    );
    assert_ne!(other["document"]["id"], target["document"]["id"]);
}

#[test]
fn unavailable_and_corrupt_are_not_empty_search() {
    let s = Setup::new();
    let d = s.ok(&["create", "Persisted"]);
    let state = s.ok(&["status"]);
    let store = Path::new(state["workspace"]["store"].as_str().unwrap());
    let revision = store
        .join("documents")
        .join(text(&d["document"], "id"))
        .join("revisions")
        .join(format!("{}.json", text(&d, "revision")));
    std::fs::write(revision, "{}").unwrap();
    let o = s.run(&["search"]);
    assert!(!o.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&o.stderr).unwrap()["error"]["kind"],
        "corrupt"
    );
    let other = s.temp.path().join("uninitialized");
    std::fs::create_dir(&other).unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&other)
            .status()
            .unwrap()
            .success()
    );
    let o = s.run_at(&other, &["ls"], None);
    assert_eq!(
        serde_json::from_slice::<Value>(&o.stderr).unwrap()["error"]["kind"],
        "not_initialized"
    );
}

#[test]
fn custom_settings_are_idempotent_and_data_cannot_enter_code() {
    let s = Setup::new();
    let other = s.temp.path().join("custom");
    std::fs::create_dir(&other).unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&other)
            .status()
            .unwrap()
            .success()
    );
    let store = s.temp.path().join("custom-store");
    let original = parse(s.run_at(
        &other,
        &[
            "init",
            "--repository-id",
            "project:custom",
            "--store",
            store.to_str().unwrap(),
        ],
        None,
    ));
    assert_eq!(original, parse(s.run_at(&other, &["init"], None)));
    assert!(
        !s.run(&["init", "--store", s.repo().join("data").to_str().unwrap()])
            .status
            .success()
    );
}

#[test]
fn concurrent_edits_preserve_a_single_winner() {
    let s = Setup::new();
    let d = s.ok(&["create", "Concurrent"]);
    let id = text(&d["document"], "id");
    let rev = text(&d, "revision");
    let commands = (0..2)
        .map(|n| {
            Command::new(env!("CARGO_BIN_EXE_lore"))
                .arg("--json")
                .arg("--repo")
                .arg(s.repo())
                .args([
                    "edit",
                    id,
                    "--if-match",
                    rev,
                    "--title",
                    &format!("writer {n}"),
                ])
                .env("LORE_HOME", s.temp.path().join("knowledge"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let outputs = commands
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outputs.iter().filter(|o| o.status.success()).count(), 1);
    assert_eq!(s.ok(&["history", id]).as_array().unwrap().len(), 2);
}

#[test]
fn malformed_cli_input_is_machine_readable_when_requested() {
    let s = Setup::new();
    let output = s.run(&["edit", "not-a-document-id"]);
    assert_eq!(output.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "invalid_input");
}
