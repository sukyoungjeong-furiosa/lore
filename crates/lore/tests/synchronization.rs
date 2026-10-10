use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    time::Duration,
};

struct Daemon {
    child: Child,
    url: String,
}

impl Daemon {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lore"))
            .args([
                "server",
                "serve",
                "--config",
                root.join("server.json").to_str().unwrap(),
                "--data-dir",
                root.join("server-data").to_str().unwrap(),
                "--listen",
                "127.0.0.1:0",
            ])
            .env("ALICE_TOKEN", "test-only-alice")
            .env("BOB_TOKEN", "test-only-bob")
            .env("READER_TOKEN", "test-only-reader")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(stderr).read_line(&mut line).unwrap();
            let _ = sender.send(line);
        });
        let line = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        let address = line
            .trim()
            .strip_prefix("Lore server listening on ")
            .unwrap_or_else(|| panic!("{line}"));
        Self {
            child,
            url: format!("http://{address}"),
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Client {
    home: PathBuf,
    repo: PathBuf,
}

impl Client {
    fn run(&self, args: &[&str], input: Option<&str>) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lore"))
            .args(["--json", "--repo", self.repo.to_str().unwrap()])
            .args(args)
            .env("LORE_HOME", &self.home)
            .env_remove("LORE_REMOTE_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = input {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        drop(child.stdin.take());
        child.wait_with_output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        self.input(args, None)
    }
    fn input(&self, args: &[&str], input: Option<&str>) -> Value {
        let output = self.run(args, input);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn connect(&self, name: &str, store: &str, url: &str, token: &str) {
        self.ok(&["remote", "add", name, "--url", url, "--store", store]);
        self.input(&["remote", "auth", "set", name, "--stdin"], Some(token));
    }
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

struct Setup {
    temp: tempfile::TempDir,
    daemon: Daemon,
    alice: Client,
    bob: Client,
}

impl Setup {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let stores = ["team", "personal"].map(|id| json!({"id":id,"repositories":{"compiler":{"canonical_ref":null},"kernels":{"canonical_ref":null}},"backends":["notion","local"]}));
        let grant = |store: &str, repos: &[&str], write: bool| json!({"store":store,"repositories":repos,"backends":["notion","local"],"write":write});
        let config = json!({"stores":stores,"principals":[
            {"name":"alice","token_env":"ALICE_TOKEN","grants":[grant("team", &["compiler","kernels"], true),grant("personal", &["compiler"],true)]},
            {"name":"bob","token_env":"BOB_TOKEN","grants":[grant("team", &["compiler"],true)]},
            {"name":"reader","token_env":"READER_TOKEN","grants":[grant("team", &["compiler"],false)]}
        ]});
        std::fs::write(
            root.join("server.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let repo = root.join("alice-code");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/scheduler.rs"), "fn schedule() {}\n").unwrap();
        std::fs::write(repo.join("src/other.rs"), "fn unrelated() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "test"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["add", "."],
            vec!["commit", "-qm", "initial"],
        ] {
            git(&repo, &args);
        }
        let other = root.join("bob-code");
        assert!(
            Command::new("git")
                .args([
                    "clone",
                    "-q",
                    repo.to_str().unwrap(),
                    other.to_str().unwrap()
                ])
                .output()
                .unwrap()
                .status
                .success()
        );
        git(&other, &["config", "user.name", "test"]);
        git(&other, &["config", "user.email", "test@example.invalid"]);
        let alice = Client {
            home: root.join("alice-home"),
            repo,
        };
        let bob = Client {
            home: root.join("bob-home"),
            repo: other,
        };
        alice.ok(&["init", "--repository-id", "compiler"]);
        bob.ok(&["init", "--repository-id", "compiler"]);
        alice.ok(&[
            "backend",
            "add-notion",
            "notion",
            "--root",
            "00000000-0000-4000-8000-000000000001",
        ]);
        let daemon = Daemon::start(root);
        alice.connect("team", "team", &daemon.url, "test-only-alice");
        bob.connect("team", "team", &daemon.url, "test-only-bob");
        Self {
            temp,
            daemon,
            alice,
            bob,
        }
    }
    fn create(&self, path: &str, document: &str) -> Value {
        self.alice.ok(&["map", "create", "notion", document, path])
    }
}

#[test]
fn two_users_share_mapping_history_discovery_projection_and_explicit_conflict_resolution() {
    let setup = Setup::new();
    let first = setup.create("src/scheduler.rs:1", "00000000-0000-4000-8000-000000000001");
    let other = setup.create("src/other.rs:1", "00000000-0000-4000-8000-000000000002");
    let id = first["record"]["mapping"]["id"].as_str().unwrap();
    let original = first["revision"].as_str().unwrap();
    setup.alice.ok(&["push", "--remote", "team", id]);
    setup.alice.ok(&[
        "push",
        "--remote",
        "team",
        other["record"]["mapping"]["id"].as_str().unwrap(),
    ]);
    let before = setup.bob.ok(&["status"]);
    let fetched = setup.bob.ok(&["map", "fetch", "--remote", "team"]);
    assert_eq!(
        fetched["catalog"]["catalog"]["mappings"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert!(!fetched.to_string().contains("snapshot"));
    assert_eq!(before, setup.bob.ok(&["status"]));
    setup.bob.ok(&["remote", "connect", "team"]);
    let discovered = setup
        .bob
        .ok(&["context", "discover", "src/scheduler.rs", "--refresh"]);
    assert_eq!(
        discovered["documents"][0]["shared_mappings"][0]["store"],
        "team"
    );
    assert!(
        setup.bob.ok(&["map", "ls"])["mappings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    git(
        &setup.bob.repo,
        &["mv", "src/scheduler.rs", "src/planner.rs"],
    );
    git(&setup.bob.repo, &["commit", "-qm", "rename"]);
    let pulled = setup
        .bob
        .ok(&["pull", "--remote", "team", "--path", "src/planner.rs"]);
    assert_eq!(pulled["results"].as_array().unwrap().len(), 1);
    assert_eq!(
        setup.bob.ok(&["map", "ls", "--remote", "team"])["mappings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        setup.bob.ok(&["map", "resolve", id, "--remote", "team"])["projections"][0]["locations"][0]
            ["path"],
        "src/planner.rs"
    );
    let b = setup.bob.ok(&[
        "map",
        "edit",
        id,
        "--remote",
        "team",
        "--if-match",
        original,
        "--context",
        "bob change",
    ]);
    let a = setup.alice.ok(&[
        "map",
        "edit",
        id,
        "--remote",
        "team",
        "--if-match",
        original,
        "--context",
        "alice change",
    ]);
    setup.alice.ok(&["push", "--remote", "team", id]);
    let rejected = setup.bob.run(&["push", "--remote", "team", id], None);
    assert_eq!(
        serde_json::from_slice::<Value>(&rejected.stderr).unwrap()["error"]["kind"],
        "conflict"
    );
    let conflict = setup
        .bob
        .run(&["pull", "--remote", "team", "--id", id], None);
    assert_eq!(conflict.status.code(), Some(3));
    assert_eq!(
        serde_json::from_slice::<Value>(&conflict.stdout).unwrap()["results"][0]["status"],
        "conflict"
    );
    assert_eq!(
        setup.bob.ok(&["map", "show", id, "--remote", "team"])["revision"],
        b["revision"]
    );
    setup.bob.ok(&[
        "map",
        "resolve-conflict",
        id,
        "--remote",
        "team",
        "--if-match",
        b["revision"].as_str().unwrap(),
        "--if-shared",
        a["revision"].as_str().unwrap(),
        "--take",
        "local",
    ]);
    setup.bob.ok(&["push", "--remote", "team", id]);
    setup.alice.ok(&["pull", "--remote", "team", "--id", id]);
    assert_eq!(
        setup.alice.ok(&["map", "show", id, "--remote", "team"])["record"]["mapping"]["context"],
        "bob change"
    );
    setup
        .alice
        .connect("personal", "personal", &setup.daemon.url, "test-only-alice");
    setup
        .alice
        .ok(&["push", "--remote", "personal", id, "--from-remote", "team"]);
    assert!(!setup.alice.run(&["map", "show", id], None).status.success());
    let personal = setup.alice.ok(&["map", "show", id, "--remote", "personal"]);
    let private_edit = setup.alice.ok(&[
        "map",
        "edit",
        id,
        "--remote",
        "personal",
        "--if-match",
        personal["revision"].as_str().unwrap(),
        "--context",
        "personal context",
    ]);
    assert!(
        !setup
            .alice
            .run(
                &[
                    "map",
                    "show",
                    id,
                    "--remote",
                    "team",
                    "--revision",
                    private_edit["revision"].as_str().unwrap(),
                ],
                None,
            )
            .status
            .success()
    );
    assert_eq!(
        setup.alice.ok(&["map", "show", id, "--remote", "team"])["record"]["mapping"]["context"],
        "bob change"
    );
    let related = setup
        .alice
        .ok(&["context", "discover", "src/scheduler.rs", "--cached"]);
    assert_eq!(
        related["documents"][0]["shared_mappings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        !setup
            .bob
            .run(&["remote", "inspect", "personal"], None)
            .status
            .success()
    );
}

#[test]
fn server_restart_preserves_heads_and_read_only_credentials_cannot_publish() {
    let mut setup = Setup::new();
    let first = setup.create("src/scheduler.rs:1", "00000000-0000-4000-8000-000000000001");
    let id = first["record"]["mapping"]["id"].as_str().unwrap();
    setup.alice.ok(&["push", "--remote", "team", id]);
    setup.bob.input(
        &["remote", "auth", "set", "team", "--stdin"],
        Some("test-only-reader"),
    );
    setup.bob.ok(&["pull", "--remote", "team", "--id", id]);
    let output = setup.bob.run(&["push", "--remote", "team", id], None);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["kind"],
        "permission_denied"
    );
    let descriptor = setup.bob.ok(&["remote", "inspect", "team"]);
    assert_eq!(descriptor["repositories"].as_object().unwrap().len(), 1);
    setup.daemon.child.kill().unwrap();
    setup.daemon.child.wait().unwrap();
    let restarted = Daemon::start(setup.temp.path());
    setup.daemon = restarted;
    setup
        .alice
        .ok(&["remote", "set-url", "team", &setup.daemon.url]);
    assert!(
        !setup
            .alice
            .run(&["remote", "inspect", "team"], None)
            .status
            .success()
    );
    setup.alice.input(
        &["remote", "auth", "set", "team", "--stdin"],
        Some("test-only-alice"),
    );
    assert_eq!(
        setup.alice.ok(&["map", "fetch", "--remote", "team"])["catalog"]["catalog"]["mappings"][id]
            ["view"]["revision"],
        first["revision"]
    );
    setup.alice.ok(&["pull", "--remote", "team", "--id", id]);
}

#[test]
fn knowledge_proposals_share_between_people_over_http_without_git_remotes() {
    let setup = Setup::new();
    let a = &setup.alice;
    let b = &setup.bob;
    a.ok(&["pull", "--remote", "team"]);
    b.ok(&["pull", "--remote", "team"]);
    let document = a.input(
        &["create", "Scheduler decision", "--stdin"],
        Some("Accepted locally"),
    );
    let id = document["document"]["id"].as_str().unwrap();
    let committed = a.ok(&[
        "commit",
        id,
        "--if-match",
        document["revision"].as_str().unwrap(),
    ]);
    a.ok(&["push"]);
    assert_eq!(b.ok(&["--central", "search"]), json!([]));
    let unrelated = a.input(
        &["create", "Private local context", "--stdin"],
        Some("Unrelated to this team"),
    );
    a.ok(&[
        "commit",
        unrelated["document"]["id"].as_str().unwrap(),
        "--if-match",
        unrelated["revision"].as_str().unwrap(),
    ]);
    a.ok(&["push"]);
    assert!(!a.run(&["push", "--remote", "team"], None).status.success());
    let submitted = a.ok(&["push", "--remote", "team", "--document", id]);
    let proposal = submitted["proposal"]["id"].as_str().unwrap();
    assert_eq!(
        a.ok(&["push", "--remote", "team", "--document", id])["proposal"]["id"],
        proposal
    );
    assert!(
        b.ok(&["fetch", "--remote", "team"])["knowledge"]["snapshot"]["documents"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    let review = a.ok(&["proposal", "--remote", "team", "show", proposal]);
    let expected = review["central"]["version"].as_str().unwrap();
    assert_eq!(
        review["proposed"]["snapshot"]["documents"]
            .as_object()
            .unwrap()
            .len(),
        1,
        "Selected remote submission excludes unrelated local knowledge"
    );
    assert_eq!(
        review["proposed"]["snapshot"]["documents"][id]["document"]["content"],
        "Accepted locally"
    );
    a.connect("readonly", "team", &setup.daemon.url, "test-only-reader");
    let denied = a.run(
        &[
            "proposal",
            "--remote",
            "readonly",
            "integrate",
            proposal,
            "--if-match",
            expected,
        ],
        None,
    );
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("permission_denied"));
    let accepted = a.ok(&[
        "proposal",
        "--remote",
        "team",
        "integrate",
        proposal,
        "--if-match",
        expected,
    ]);
    assert_eq!(
        a.ok(&[
            "proposal",
            "--remote",
            "team",
            "integrate",
            proposal,
            "--if-match",
            expected
        ]),
        accepted,
        "Integration retry returns its original durable result"
    );
    assert_eq!(
        a.ok(&["push", "--remote", "team", "--document", id])["proposal"]["integrated"],
        accepted["version"]["version"]
    );
    assert_eq!(b.ok(&["--central", "search"]), json!([]));
    b.ok(&["fetch", "--remote", "team"]);
    assert_eq!(
        b.ok(&["show", id, "--remote", "team"])["document"]["content"],
        "Accepted locally"
    );
    assert_eq!(
        b.ok(&["--central", "search"]),
        json!([]),
        "Fetch does not incorporate knowledge"
    );
    b.ok(&["pull", "--remote", "team"]);
    assert_eq!(
        b.ok(&["--central", "show", id])["document"]["content"],
        "Accepted locally"
    );
    assert_eq!(
        b.ok(&["search"]),
        json!([]),
        "Remote Pull advances local central, not every agent branch"
    );
    b.ok(&["pull"]);
    assert_eq!(
        b.ok(&["show", id])["document"]["content"],
        "Accepted locally"
    );
    a.ok(&["pull", "--remote", "team"]);
    let bob = b.ok(&["show", id]);
    let bob = b.input(
        &[
            "edit",
            id,
            "--if-match",
            bob["revision"].as_str().unwrap(),
            "--stdin",
        ],
        Some("Bob's conclusion"),
    );
    b.ok(&[
        "commit",
        id,
        "--if-match",
        bob["revision"].as_str().unwrap(),
    ]);
    b.ok(&["push"]);
    let alice = a.input(
        &[
            "edit",
            id,
            "--if-match",
            committed["revision"].as_str().unwrap(),
            "--stdin",
        ],
        Some("Alice's conclusion"),
    );
    a.ok(&[
        "commit",
        id,
        "--if-match",
        alice["revision"].as_str().unwrap(),
    ]);
    a.ok(&["push"]);
    let p1 = a.ok(&["push", "--remote", "team", "--document", id]);
    let p2 = b.ok(&["push", "--remote", "team", "--document", id]);
    let review = a.ok(&[
        "proposal",
        "--remote",
        "team",
        "show",
        p1["proposal"]["id"].as_str().unwrap(),
    ]);
    a.ok(&[
        "proposal",
        "--remote",
        "team",
        "integrate",
        p1["proposal"]["id"].as_str().unwrap(),
        "--if-match",
        review["central"]["version"].as_str().unwrap(),
    ]);
    let review = b.ok(&[
        "proposal",
        "--remote",
        "team",
        "show",
        p2["proposal"]["id"].as_str().unwrap(),
    ]);
    assert_eq!(
        b.run(
            &[
                "proposal",
                "--remote",
                "team",
                "integrate",
                p2["proposal"]["id"].as_str().unwrap(),
                "--if-match",
                review["central"]["version"].as_str().unwrap()
            ],
            None
        )
        .status
        .code(),
        Some(3)
    );
    assert_eq!(
        review["central"]["snapshot"]["documents"][id]["document"]["content"],
        "Alice's conclusion"
    );
    assert_eq!(
        review["proposed"]["snapshot"]["documents"]
            .as_object()
            .unwrap()
            .len(),
        1,
        "Selected remote submission excludes unrelated local knowledge"
    );
    assert_eq!(
        review["proposed"]["snapshot"]["documents"][id]["document"]["content"],
        "Bob's conclusion"
    );
    assert_eq!(
        b.ok(&["--central", "show", id])["document"]["content"],
        "Bob's conclusion"
    );
}
