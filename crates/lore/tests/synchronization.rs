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
    _temp: tempfile::TempDir,
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
            _temp: temp,
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
fn discovery_does_not_reuse_another_connections_knowledge_for_the_same_store() {
    let setup = Setup::new();
    setup.alice.ok(&["pull", "--remote", "team"]);
    let mapping = setup.create("src/scheduler.rs:1", "00000000-0000-4000-8000-000000000001");
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    setup.alice.ok(&[
        "commit",
        id,
        "--mapping",
        "--if-match",
        mapping["revision"].as_str().unwrap(),
    ]);
    setup.alice.ok(&["push"]);
    let proposal = setup
        .alice
        .ok(&["push", "--remote", "team", "--mapping-id", id]);
    let proposal = proposal["proposal"]["id"].as_str().unwrap();
    let review = setup
        .alice
        .ok(&["proposal", "--remote", "team", "show", proposal]);
    setup.alice.ok(&[
        "proposal",
        "--remote",
        "team",
        "integrate",
        proposal,
        "--if-match",
        review["central"]["version"].as_str().unwrap(),
    ]);
    setup.bob.ok(&["fetch", "--remote", "team"]);
    setup.bob.ok(&["remote", "connect", "team"]);
    let known = setup
        .bob
        .ok(&["context", "discover", "src/scheduler.rs", "--cached"]);
    assert_eq!(known["documents"].as_array().unwrap().len(), 1);

    setup.bob.connect(
        "restricted",
        "team",
        &setup.daemon.url,
        "invalid-test-token",
    );
    setup.bob.ok(&["remote", "connect", "restricted"]);
    let isolated = setup
        .bob
        .ok(&["context", "discover", "src/scheduler.rs", "--cached"]);
    assert!(isolated["documents"].as_array().unwrap().is_empty());
    assert!(!isolated["warnings"].as_array().unwrap().is_empty());
    let denied = setup
        .bob
        .ok(&["context", "discover", "src/scheduler.rs", "--refresh"]);
    assert!(denied["documents"].as_array().unwrap().is_empty());

    setup.bob.ok(&["remote", "connect", "team"]);
    setup
        .bob
        .ok(&["remote", "set-url", "team", "http://127.0.0.1:1"]);
    let retargeted = setup
        .bob
        .ok(&["context", "discover", "src/scheduler.rs", "--cached"]);
    assert!(retargeted["documents"].as_array().unwrap().is_empty());
    assert!(!retargeted["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn mapping_only_publication_is_rejected_without_changing_shared_knowledge() {
    let setup = Setup::new();
    let mapping = setup.create("src/scheduler.rs:1", "00000000-0000-4000-8000-000000000001");
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    for args in [
        vec!["push", "--remote", "team", id],
        vec![
            "map",
            "publish",
            id,
            "--if-match",
            mapping["revision"].as_str().unwrap(),
        ],
        vec!["pull", "--remote", "team", "--id", id],
        vec!["map", "fetch", "--remote", "team"],
    ] {
        let output = setup.alice.run(&args, None);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported"));
    }
    assert!(
        setup.bob.ok(&["fetch", "--remote", "team"])["knowledge"]["snapshot"]["mappings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
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

#[test]
fn restarting_the_server_preserves_integrated_mapping_proposals() {
    let mut s = Setup::new();
    let a = &s.alice;
    a.ok(&["pull", "--remote", "team"]);
    let mapping = s.create("src/scheduler.rs:1", "00000000-0000-4000-8000-000000000001");
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    a.ok(&[
        "commit",
        id,
        "--mapping",
        "--if-match",
        mapping["revision"].as_str().unwrap(),
    ]);
    a.ok(&["push"]);
    let submitted = a.ok(&["push", "--remote", "team", "--mapping-id", id]);
    let proposal = submitted["proposal"]["id"].as_str().unwrap();
    let review = a.ok(&["proposal", "--remote", "team", "show", proposal]);
    let accepted = a.ok(&[
        "proposal",
        "--remote",
        "team",
        "integrate",
        proposal,
        "--if-match",
        review["central"]["version"].as_str().unwrap(),
    ]);
    s.daemon.child.kill().unwrap();
    s.daemon.child.wait().unwrap();
    s.daemon = Daemon::start(s._temp.path());
    a.ok(&["remote", "set-url", "team", &s.daemon.url]);
    a.input(
        &["remote", "auth", "set", "team", "--stdin"],
        Some("test-only-alice"),
    );
    let fetched = a.ok(&["fetch", "--remote", "team"]);
    assert_eq!(
        fetched["knowledge"]["snapshot"]["mappings"][id],
        mapping["record"]
    );
    assert_eq!(
        a.ok(&["proposal", "--remote", "team", "show", proposal])["proposal"]["integrated"],
        accepted["version"]["version"]
    );
}
