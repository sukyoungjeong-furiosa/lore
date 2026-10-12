use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

const PAGE: &str = "00000000-0000-4000-8000-000000000001";

struct Peer {
    home: PathBuf,
    repo: PathBuf,
}

impl Peer {
    fn new(root: &Path, name: &str) -> Self {
        let peer = Self {
            home: root.join(format!("{name}-home")),
            repo: root.join(name),
        };
        std::fs::create_dir_all(peer.repo.join("src")).unwrap();
        std::fs::write(peer.repo.join("src/scheduler.rs"), "fn schedule() {}\n").unwrap();
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
                    .arg(&peer.repo)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        peer.ok(&["init", "--repository-id", "compiler"]);
        peer.ok(&["backend", "add-notion", "notion", "--root", PAGE]);
        peer
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lore"))
            .args(["--json", "--repo", self.repo.to_str().unwrap()])
            .args(args)
            .env("LORE_HOME", &self.home)
            .env_remove("NOTION_TOKEN")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn create(&self, context: &str) -> Value {
        self.ok(&[
            "map",
            "create",
            "notion",
            PAGE,
            "src/scheduler.rs:1",
            "--glob",
            "src/**",
            "--context",
            context,
        ])
    }

    fn edit(&self, view: &Value, context: &str) -> Value {
        self.ok(&[
            "map",
            "edit",
            id(view),
            "--if-match",
            revision(view),
            "--context",
            context,
        ])
    }

    fn publish(&self, view: &Value) -> Value {
        let committed = self.ok(&[
            "commit",
            id(view),
            "--mapping",
            "--if-match",
            revision(view),
        ]);
        assert_eq!(self.ok(&["push"])["status"], "integrated");
        committed
    }

    fn export(&self, path: &Path, ids: &[&str]) -> Value {
        let mut args = vec!["map", "export", "--output", path.to_str().unwrap()];
        for id in ids {
            args.extend(["--mapping-id", id]);
        }
        self.ok(&args);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn import(&self, path: &Path) -> Value {
        self.ok(&["map", "import", "--input", path.to_str().unwrap()])
    }

    fn reject(&self, path: &Path) {
        let before = self.ok(&["status"]);
        let output = self.run(&["map", "import", "--input", path.to_str().unwrap()]);
        assert!(!output.status.success());
        assert_eq!(self.ok(&["status"]), before);
    }
}

fn id(view: &Value) -> &str {
    view["record"]["mapping"]["id"].as_str().unwrap()
}
fn revision(view: &Value) -> &str {
    view["revision"].as_str().unwrap()
}
fn write(path: &Path, value: &Value) {
    std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn independent_homes_preserve_selected_history_and_discovery_without_document_access() {
    let temp = tempfile::tempdir().unwrap();
    let source = Peer::new(temp.path(), "source");
    let target = Peer::new(temp.path(), "target");
    let first = source.create("initial");
    let second = source.edit(&first, "second");
    let selected = source.edit(&second, "selected");
    let unrelated = source.create("private unrelated mapping");
    let path = temp.path().join("mapping.json");
    let before = source.ok(&["status"]);
    let artifact = source.export(&path, &[id(&selected)]);
    assert_eq!(source.ok(&["status"]), before);
    assert_eq!(artifact["mappings"].as_object().unwrap().len(), 1);
    assert!(artifact["mappings"].get(id(&unrelated)).is_none());
    assert_eq!(
        artifact["mappings"][id(&selected)]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(target.import(&path)["results"][0]["status"], "imported");
    assert_eq!(target.ok(&["map", "show", id(&selected)]), selected);
    assert_eq!(
        target.ok(&["map", "history", id(&selected)]),
        json!([selected, second, first])
    );
    let repeated = target.import(&path);
    assert_eq!(repeated["results"][0]["status"], "unchanged");
    assert!(
        target.ok(&["fetch"])["knowledge"]["snapshot"]["mappings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let committed = target.publish(&selected);
    assert_ne!(
        revision(&committed),
        revision(&selected),
        "Local history normalizes imported ancestry"
    );
    assert_eq!(target.import(&path)["results"][0]["status"], "unchanged");
    let found = target.ok(&[
        "--central",
        "context",
        "discover",
        "src/scheduler.rs",
        "--cached",
    ]);
    assert_eq!(found["documents"][0]["document"]["reference"]["id"], PAGE);
    assert_eq!(found["documents"][0]["mappings"][0], id(&selected));
    let updated = source.edit(&selected, "updated");
    source.export(&path, &[id(&updated)]);
    assert_eq!(target.import(&path)["results"][0]["status"], "updated");
    target.publish(&updated);
    assert_eq!(
        target.ok(&["--central", "map", "show", id(&updated)])["record"]["mapping"],
        updated["record"]["mapping"]
    );
}

#[test]
fn conflicting_local_edits_reject_the_entire_artifact_and_preserve_unrelated_state() {
    let temp = tempfile::tempdir().unwrap();
    let source = Peer::new(temp.path(), "source");
    let target = Peer::new(temp.path(), "target");
    let first = source.create("first");
    let second = source.create("second");
    let path = temp.path().join("mapping.json");
    source.export(&path, &[id(&first), id(&second)]);
    target.import(&path);
    let local = target.edit(&first, "local edit");
    let unrelated = target.create("unrelated local mapping");
    let updated = source.edit(&second, "source edit");
    source.export(&path, &[id(&first), id(&updated)]);
    target.reject(&path);
    assert_eq!(target.ok(&["map", "show", id(&local)]), local);
    assert_eq!(target.ok(&["map", "show", id(&second)]), second);
    assert_eq!(target.ok(&["map", "show", id(&unrelated)]), unrelated);
}

#[test]
fn source_identity_ancestry_archives_and_omission_have_explicit_import_semantics() {
    let temp = tempfile::tempdir().unwrap();
    let source = Peer::new(temp.path(), "source");
    let target = Peer::new(temp.path(), "target");
    let first = source.create("first");
    let path = temp.path().join("mapping.json");
    let initial = source.export(&path, &[id(&first)]);
    target.import(&path);
    let updated = source.edit(&first, "updated");
    let mut artifact = source.export(&path, &[id(&updated)]);
    artifact["mappings"][id(&updated)]["store"] = json!("another-store");
    write(&path, &artifact);
    target.reject(&path);
    source.export(&path, &[id(&updated)]);
    target.import(&path);
    write(&path, &initial);
    target.reject(&path);
    let archived = source.ok(&[
        "map",
        "archive",
        id(&updated),
        "--if-match",
        revision(&updated),
    ]);
    source.ok(&["map", "export", "--all", "--output", path.to_str().unwrap()]);
    target.import(&path);
    assert_eq!(target.ok(&["map", "show", id(&archived)]), archived);
    assert!(
        target.ok(&["context", "discover", "src/scheduler.rs", "--cached"])["documents"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut empty = initial;
    empty["mappings"] = json!({});
    write(&path, &empty);
    target.import(&path);
    assert_eq!(target.ok(&["map", "show", id(&archived)]), archived);
}

#[test]
fn malformed_or_foreign_artifacts_are_rejected_before_any_mapping_is_applied() {
    let temp = tempfile::tempdir().unwrap();
    let source = Peer::new(temp.path(), "source");
    let target = Peer::new(temp.path(), "target");
    let first = source.create("first");
    let path = temp.path().join("mapping.json");
    let artifact = source.export(&path, &[id(&first)]);
    let mut invalid = vec![];
    let mut value = artifact.clone();
    value["schema_version"] = json!(2);
    invalid.push(value);
    let mut value = artifact.clone();
    value["repository"] = json!("another-repository");
    invalid.push(value);
    let mut value = artifact.clone();
    value["mappings"][id(&first)]["revisions"][0]["record"]["mapping"]["anchors"][0]["snapshot"] =
        json!("tampered\n");
    invalid.push(value);
    let mut value = artifact.clone();
    value["mappings"][id(&first)]["revisions"][0]["record"]["parent"] = json!("a".repeat(64));
    invalid.push(value);
    let mut value = artifact.clone();
    value["mappings"][id(&first)]["revisions"] = json!([]);
    invalid.push(value);
    let mut value = artifact.clone();
    value["credentials"] = json!("never accepted");
    invalid.push(value);
    let mut value = artifact.clone();
    let view = &mut value["mappings"][id(&first)]["revisions"][0];
    view["record"]["mapping"]["document"]["backend"] = json!("local");
    let record: lore_core::MappingRevision =
        serde_json::from_value(view["record"].clone()).unwrap();
    view["revision"] = json!(record.id().unwrap());
    invalid.push(value);
    for value in invalid {
        write(&path, &value);
        target.reject(&path);
    }
    write(&path, &artifact);
    let central = target.run(&[
        "--central",
        "map",
        "import",
        "--input",
        path.to_str().unwrap(),
    ]);
    assert!(!central.status.success());
    assert!(
        target.ok(&["map", "ls"])["mappings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn knowledge_branches_retain_independent_artifact_baselines() {
    let temp = tempfile::tempdir().unwrap();
    let source = Peer::new(temp.path(), "source");
    let target = Peer::new(temp.path(), "target");
    let first = source.create("first");
    let path = temp.path().join("mapping.json");
    source.export(&path, &[id(&first)]);
    target.import(&path);
    target.publish(&first);
    let branch = target.ok(&["worktree"])["worktree"]["branch"]
        .as_str()
        .unwrap()
        .to_owned();
    target.ok(&["checkout", "other", "--create"]);
    target.import(&path);
    let second = source.edit(&first, "second");
    source.export(&path, &[id(&second)]);
    target.import(&path);
    target.publish(&second);
    target.ok(&["checkout", &branch]);
    assert_eq!(target.import(&path)["results"][0]["status"], "updated");
    assert_eq!(target.ok(&["map", "show", id(&second)]), second);
}

#[test]
fn central_export_includes_only_accepted_mappings_and_requires_explicit_selection() {
    let temp = tempfile::tempdir().unwrap();
    let source = Peer::new(temp.path(), "source");
    let accepted = source.create("accepted");
    source.publish(&accepted);
    source.create("pending");
    let path = temp.path().join("mapping.json");
    assert!(
        !source
            .run(&["map", "export", "--output", path.to_str().unwrap()])
            .status
            .success()
    );
    source.ok(&[
        "--central",
        "map",
        "export",
        "--all",
        "--output",
        path.to_str().unwrap(),
    ]);
    let artifact: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(artifact["mappings"].as_object().unwrap().len(), 1);
    assert!(artifact["mappings"].get(id(&accepted)).is_some());
}
