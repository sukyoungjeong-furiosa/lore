use lore_core::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

struct Remote {
    url: String,
    version: Arc<Mutex<KnowledgeVersion>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Remote {
    fn new(version: KnowledgeVersion) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let version = Arc::new(Mutex::new(version));
        let stop = Arc::new(AtomicBool::new(false));
        let current = version.clone();
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let (stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with("POST /v1/knowledge "));
                let mut length = 0;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: KnowledgeRequest = serde_json::from_slice(&body).unwrap();
                assert!(matches!(request, KnowledgeRequest::Read { .. }));
                let response = KnowledgeResponse::Version {
                    version: current.lock().unwrap().clone(),
                };
                let bytes = serde_json::to_vec(&response).unwrap();
                let mut stream = reader.into_inner();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
                stream.write_all(&bytes).unwrap();
            }
        });
        Self {
            url,
            version,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

struct Setup {
    temp: tempfile::TempDir,
    home: PathBuf,
    repo: PathBuf,
    remote: Remote,
    document: DocumentId,
}

impl Setup {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let repo = temp.path().join("code");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(
            &repo,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "initial",
            ],
        );
        let record = Revision {
            parent: None,
            actor: Actor::Human,
            at_ms: 1,
            document: Document::new("Shared design".into(), "First".into(), Actor::Human).unwrap(),
        };
        let document = record.document.id.clone();
        let mut snapshot = KnowledgeSnapshot::default();
        snapshot.documents.insert(document.clone(), record);
        let remote = Remote::new(KnowledgeVersion {
            version: "remote-v1".into(),
            snapshot,
        });
        let s = Self {
            temp,
            home,
            repo,
            remote,
            document,
        };
        s.ok(&s.repo, &["init", "--repository-id", "compiler"]);
        s.ok(
            &s.repo,
            &[
                "remote",
                "add",
                "team",
                "--url",
                &s.remote.url,
                "--store",
                "team-store",
            ],
        );
        s
    }

    fn run(&self, repo: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lore"))
            .args(["--repo", repo.to_str().unwrap(), "--json"])
            .args(args)
            .env("LORE_HOME", &self.home)
            .env("LORE_REMOTE_TOKEN", "mock-token")
            .output()
            .unwrap()
    }

    fn ok(&self, repo: &Path, args: &[&str]) -> Value {
        let output = self.run(repo, args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn sync_path(&self) -> PathBuf {
        let paths: Vec<_> = std::fs::read_dir(self.home.join("knowledge-remotes/v2"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(paths.len(), 1);
        paths[0].clone()
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

#[test]
fn independent_local_stores_pull_their_own_baselines_while_worktrees_share_their_stores_baseline() {
    let s = Setup::new();
    s.ok(&s.repo, &["pull", "--remote", "team"]);
    let peer = s.temp.path().join("peer");
    git(
        &s.repo,
        &["worktree", "add", "-qb", "feature", peer.to_str().unwrap()],
    );
    s.ok(&peer, &["init"]);
    s.ok(&peer, &["pull", "--remote", "team"]);
    assert_eq!(
        s.ok(&peer, &["--central", "show", s.document.as_str()])["document"]["content"],
        "First"
    );
    assert!(s.sync_path().join("baseline.json").is_file());

    let independent = s.temp.path().join("independent");
    std::fs::create_dir_all(&independent).unwrap();
    git(&independent, &["init", "-q"]);
    s.ok(&independent, &["store", "create", "separate"]);
    s.ok(
        &independent,
        &[
            "init",
            "--repository-id",
            "compiler",
            "--local-store",
            "separate",
        ],
    );
    let pulled = s.ok(&independent, &["pull", "--remote", "team"]);
    assert_eq!(
        pulled["central"]["snapshot"]["documents"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        std::fs::read_dir(s.home.join("knowledge-remotes/v2"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn interrupted_baseline_publication_is_recovered_before_pulling_new_remote_changes() {
    let s = Setup::new();
    s.ok(&s.repo, &["pull", "--remote", "team"]);
    let sync = s.sync_path();
    let proposals = s.ok(&s.repo, &["proposal", "list"]);
    let source = s.remote.version.lock().unwrap().clone();
    // Recreate the durable state after central integration and before source-baseline publication.
    std::fs::write(
        sync.join("pending-pull.json"),
        serde_json::to_vec(&json!({
            "proposal": proposals["proposals"][0]["id"], "source": source,
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::remove_file(sync.join("baseline.json")).unwrap();
    std::fs::create_dir(sync.join("baseline.json")).unwrap();
    let failed = s.run(&s.repo, &["pull", "--remote", "team"]);
    assert!(!failed.status.success());
    assert!(sync.join("pending-pull.json").is_file());
    assert_eq!(
        s.ok(&s.repo, &["--central", "show", s.document.as_str()])["document"]["content"],
        "First"
    );
    {
        let mut remote = s.remote.version.lock().unwrap();
        remote.version = "remote-v2".into();
        let record = remote.snapshot.documents.get_mut(&s.document).unwrap();
        record.document.content = "Updated remotely".into();
        record.at_ms = 2;
    }
    std::fs::remove_dir(sync.join("baseline.json")).unwrap();
    let pulled = s.ok(&s.repo, &["pull", "--remote", "team"]);
    assert_eq!(pulled["source_version"], "remote-v2");
    assert_eq!(
        s.ok(&s.repo, &["--central", "show", s.document.as_str()])["document"]["content"],
        "Updated remotely"
    );
    assert!(!sync.join("pending-pull.json").exists());
    let baseline: KnowledgeVersion =
        serde_json::from_slice(&std::fs::read(sync.join("baseline.json")).unwrap()).unwrap();
    assert_eq!(baseline.version, "remote-v2");
}

#[test]
fn remote_mappings_cannot_publish_missing_owned_documents() {
    let s = Setup::new();
    let before = s.ok(&s.repo, &["worktree"])["central_version"].clone();
    let mapping = Mapping {
        id: MappingId::new(),
        document: DocumentReference {
            backend: "local".into(),
            id: DocumentId::new().to_string(),
        },
        anchors: vec![],
        scopes: vec![PathScope {
            repository: "compiler".into(),
            glob: "src/**".into(),
        }],
        context: String::new(),
        archived: false,
    };
    s.remote.version.lock().unwrap().snapshot.mappings.insert(
        mapping.id.clone(),
        MappingRevision {
            parent: None,
            actor: Actor::Agent,
            at_ms: 1,
            mapping,
        },
    );
    let failed = s.run(&s.repo, &["pull", "--remote", "team"]);
    let error: Value = serde_json::from_slice(&failed.stderr).unwrap();
    assert_eq!(error["error"]["kind"], json!("invalid_input"));
    assert_eq!(s.ok(&s.repo, &["worktree"])["central_version"], before);
}

#[test]
fn selective_mapping_push_reports_its_missing_document_before_contacting_the_remote() {
    let s = Setup::new();
    s.ok(&s.repo, &["pull", "--remote", "team"]);
    let document = s.ok(&s.repo, &["create", "New local knowledge"]);
    let did = document["document"]["id"].as_str().unwrap();
    s.ok(
        &s.repo,
        &[
            "commit",
            did,
            "--if-match",
            document["revision"].as_str().unwrap(),
        ],
    );
    let workspace = lore_local::Workspace::open(&s.home, &s.repo).unwrap();
    let mapping = workspace
        .create_mapping(
            DocumentReference {
                backend: "local".into(),
                id: did.into(),
            },
            vec![],
            vec![PathScope {
                repository: "compiler".into(),
                glob: "src/**".into(),
            }],
            String::new(),
            Actor::Agent,
        )
        .unwrap();
    let mapping = serde_json::to_value(mapping).unwrap();
    let mid = mapping["record"]["mapping"]["id"].as_str().unwrap();
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
    s.ok(&s.repo, &["push"]);
    let failed = s.run(&s.repo, &["push", "--remote", "team", "--mapping-id", mid]);
    let error: Value = serde_json::from_slice(&failed.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "invalid_input");
    assert!(error["error"]["message"].as_str().unwrap().contains(did));
    assert_eq!(
        s.ok(&s.repo, &["--central", "show", did])["document"]["title"],
        "New local knowledge"
    );
}
