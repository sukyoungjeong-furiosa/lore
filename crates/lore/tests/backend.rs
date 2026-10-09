use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

const ROOT: &str = "b81457a4-a41d-45b4-9ae4-82d0c0e9e00c";
const CHILD: &str = "00000000-0000-4000-8000-000000000001";
const NESTED: &str = "00000000-0000-4000-8000-000000000002";
const TOGGLE: &str = "00000000-0000-4000-8000-000000000003";
const DATABASE: &str = "00000000-0000-4000-8000-000000000004";
const SOURCE: &str = "00000000-0000-4000-8000-000000000005";
const ROW: &str = "00000000-0000-4000-8000-000000000006";
const SECRET: &str = "test-token-do-not-print";
struct Mock {
    url: String,
    requests: Arc<Mutex<Vec<(String, String, Value)>>>,
    responses: Arc<Mutex<BTreeMap<String, (u16, Value)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Mock {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(vec![]));
        let responses: Arc<Mutex<BTreeMap<String, (u16, Value)>>> =
            Arc::new(Mutex::new(BTreeMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (received, values, stopping) = (requests.clone(), responses.clone(), stop.clone());
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &received, &values),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            url,
            requests,
            responses,
            stop,
            thread: Some(thread),
        }
    }

    fn set(&self, path: &str, status: u16, body: Value) {
        self.responses
            .lock()
            .unwrap()
            .insert(format!("/v1{path}"), (status, body));
    }

    fn fixture(&self) {
        for (id, title) in [
            (ROOT, "PR"),
            (CHILD, "Scheduler Design"),
            (NESTED, "Nested Design"),
            (ROW, "Database Design"),
        ] {
            self.set(&format!("/pages/{id}"), 200, page(id, title));
            self.set(&format!("/blocks/{id}/children"), 200, listing(vec![]));
        }
        self.set(
            &format!("/blocks/{ROOT}/children"),
            200,
            json!({
                "results": [
                    child(CHILD, "Scheduler Design"),
                    {"id": TOGGLE, "type": "toggle", "has_children": true},
                ],
                "has_more": true,
                "next_cursor": "second cursor",
            }),
        );
        self.set(
            &format!("/blocks/{ROOT}/children?second"),
            200,
            listing(vec![
                json!({"id":DATABASE,"type":"child_database","has_children":true}),
            ]),
        );
        self.set(
            &format!("/blocks/{TOGGLE}/children"),
            200,
            listing(vec![child(NESTED, "Nested Design")]),
        );
        self.set(
            &format!("/databases/{DATABASE}"),
            200,
            json!({"data_sources":[{"id":SOURCE}]}),
        );
        self.set(
            &format!("/data_sources/{SOURCE}/query"),
            200,
            listing(vec![page(ROW, "Database Design")]),
        );
        self.set(
            &format!("/pages/{CHILD}/markdown"),
            200,
            json!({
                "markdown": "# Design\nBody fetched on demand.",
                "truncated": false,
                "unknown_block_ids": [],
            }),
        );
    }
}

fn serve(
    mut stream: TcpStream,
    requests: &Mutex<Vec<(String, String, Value)>>,
    responses: &Mutex<BTreeMap<String, (u16, Value)>>,
) {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let parts: Vec<_> = line.split_whitespace().collect();
    let (method, uri) = (parts[0].to_owned(), parts[1].to_owned());
    let mut size = 0;
    let mut authorization = false;
    let mut version = false;
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            size = value.trim().parse().unwrap();
        }
        authorization |= line.trim() == format!("authorization: Bearer {SECRET}");
        version |= line.trim() == "notion-version: 2026-03-11";
    }
    assert!(
        authorization && (uri.ends_with("/mapping-index") || version),
        "Authentication and version headers must be supplied"
    );
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).unwrap();
    let body = if size == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    let bare = uri.split('?').next().unwrap();
    let key = if uri.contains("start_cursor=") {
        format!("{bare}?second")
    } else {
        bare.to_string()
    };
    requests.lock().unwrap().push((method, uri, body));
    let (status, value) = responses
        .lock()
        .unwrap()
        .get(&key)
        .cloned()
        .unwrap_or((404, json!({"message":"not available"})));
    let body = value.to_string();
    write!(
        stream,
        "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn page(id: &str, title: &str) -> Value {
    json!({
        "id": id,
        "url": format!("https://www.notion.so/{}", id.replace('-', "")),
        "last_edited_time": "2026-10-09T00:00:00Z",
        "properties": {
            "Name": {"type": "title", "title": [{"plain_text": title}]},
        },
    })
}

fn child(id: &str, title: &str) -> Value {
    json!({"id":id,"type":"child_page","has_children":true,"child_page":{"title":title}})
}

fn listing(rows: Vec<Value>) -> Value {
    json!({"results":rows,"has_more":false,"next_cursor":null})
}

struct Setup {
    temp: tempfile::TempDir,
    repo: PathBuf,
}

impl Setup {
    fn new(mock: &Mock) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("code");
        std::fs::create_dir(&repo).unwrap();
        let s = Self { temp, repo };
        s.git(&["init", "-q"]);
        s.git(&["config", "user.email", "test@example.invalid"]);
        s.git(&["config", "user.name", "test"]);
        std::fs::write(
            s.repo.join("scheduler.py"),
            "# scheduler\ndef schedule():\n    return 42\n",
        )
        .unwrap();
        s.git(&["add", "."]);
        s.git(&["commit", "-qm", "initial"]);
        s.ok(&["init"]);
        s.ok(&[
            "backend",
            "add-notion",
            "team",
            "--root",
            "https://app.notion.com/p/furiosa/PR-b81457a4a41d45b49ae482d0c0e9e00c?source=copy_link",
            "--token-env",
            "LORE_TEST_NOTION_TOKEN",
            "--api-url",
            &mock.url,
        ]);
        s
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_at(&self, repo: &std::path::Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lore"))
            .arg("--repo")
            .arg(repo)
            .arg("--json")
            .args(args)
            .env("LORE_HOME", self.temp.path().join("knowledge"))
            .env("LORE_TEST_NOTION_TOKEN", SECRET)
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> Value {
        parse(self.run_at(&self.repo, args))
    }
}

fn parse(o: Output) -> Value {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

#[test]
fn shared_credentials_allow_uncached_reads_across_worktrees_without_token_environment() {
    use std::process::Stdio;
    let mock = Mock::new();
    mock.fixture();
    let s = Setup::new(&mock);
    let mut set = Command::new(env!("CARGO_BIN_EXE_lore"))
        .args(["--json", "backend", "auth", "set", "team", "--stdin"])
        .env("LORE_HOME", s.temp.path().join("knowledge"))
        .env_remove("LORE_TEST_NOTION_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    set.stdin
        .take()
        .unwrap()
        .write_all(SECRET.as_bytes())
        .unwrap();
    let output = set.wait_with_output().unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SECRET));
    assert_eq!(parse(output)["stored"], true);
    let peer = s.temp.path().join("other-worktree");
    s.git(&[
        "worktree",
        "add",
        "-qb",
        "auth-peer",
        peer.to_str().unwrap(),
    ]);
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_lore"))
            .arg("--repo")
            .arg(&peer)
            .arg("--json")
            .args(args)
            .env("LORE_HOME", s.temp.path().join("knowledge"))
            .env_remove("LORE_TEST_NOTION_TOKEN")
            .output()
            .unwrap()
    };
    assert_eq!(
        parse(run(&["backend", "auth", "status", "team"]))["source"],
        "stored"
    );
    assert_eq!(
        parse(run(&["docs", "show", "team", CHILD]))["cached"],
        false
    );
    assert_eq!(
        parse(run(&["docs", "browse", "team"]))["value"]["documents"][0]["title"],
        "PR"
    );
    let registry =
        std::fs::read_to_string(s.temp.path().join("knowledge/backends/config.json")).unwrap();
    assert!(!registry.contains(SECRET));
    let environment = s.ok(&["backend", "auth", "status", "team"]);
    assert_eq!(environment["source"], "environment");
    parse(run(&["backend", "auth", "remove", "team"]));
    let failed = run(&["docs", "show", "team", CHILD, "--refresh"]);
    assert!(!failed.status.success());
    let error: Value = serde_json::from_slice(&failed.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "permission_denied");
    assert_eq!(parse(run(&["docs", "show", "team", CHILD]))["cached"], true);
}

#[test]
fn notion_browse_is_not_import_and_body_fetch_is_lazy() {
    let mock = Mock::new();
    mock.fixture();
    let s = Setup::new(&mock);
    let roots = s.ok(&["docs", "browse", "team"]);
    assert_eq!(roots["value"]["documents"][0]["title"], "PR");
    let children = s.ok(&["docs", "browse", "team", ROOT]);
    assert_eq!(children["value"]["documents"].as_array().unwrap().len(), 3);
    let log = mock.requests.lock().unwrap();
    assert!(
        log.iter().all(
            |(method, path, _)| method == "GET" || method == "POST" && path.ends_with("/query")
        )
    );
    assert!(log.iter().all(|(_, path, _)| !path.ends_with("/markdown")));
    assert!(
        log.iter()
            .any(|(_, path, _)| path.contains("start_cursor=second+cursor"))
    );
    drop(log);
    assert_eq!(s.ok(&["status"])["workspace"]["documents"], json!({}));
    let before = mock.requests.lock().unwrap().len();
    let mapping = s.ok(&[
        "--author",
        "agent",
        "map",
        "create",
        "team",
        CHILD,
        "scheduler.py:2-3",
        "--context",
        "Explains scheduler constraints",
    ]);
    assert_eq!(
        before,
        mock.requests.lock().unwrap().len(),
        "Mapping does not require fetching the document"
    );
    assert_eq!(mapping["record"]["actor"], "agent");
    let listing = s.ok(&["map", "ls", "scheduler.py"]);
    assert!(
        listing["mappings"][0]["view"]["record"]["mapping"]["anchors"][0]
            .get("snapshot")
            .is_none(),
        "Discovery must not dump captured source bodies"
    );
    let doc = s.ok(&["docs", "show", "team", CHILD]);
    assert_eq!(doc["cached"], false);
    assert!(
        doc["value"]["content"]
            .as_str()
            .unwrap()
            .contains("on demand")
    );
    let calls = mock
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, path, _)| path.ends_with("/markdown"))
        .count();
    let index = s.ok(&["docs", "index", "team"]);
    assert_eq!(index["value"]["documents"].as_array().unwrap().len(), 4);
    assert_eq!(index["value"]["complete"], true);
    assert_eq!(
        calls,
        mock.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, path, _)| path.ends_with("/markdown"))
            .count(),
        "Index does not fetch bodies"
    );
    assert_eq!(
        s.ok(&["docs", "search", "team", "scheduler"])["value"]["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let config =
        std::fs::read_to_string(s.temp.path().join("knowledge/backends/config.json")).unwrap();
    assert!(!config.contains(SECRET));
    mock.set(&format!("/pages/{CHILD}"), 404, json!({"message":SECRET}));
    let failed = s.run_at(&s.repo, &["docs", "show", "team", CHILD, "--refresh"]);
    assert!(!failed.status.success());
    assert!(!String::from_utf8_lossy(&failed.stderr).contains(SECRET));
    assert_eq!(
        s.ok(&["map", "ls", "scheduler.py"])["mappings"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "Unavailable backend does not remove mappings"
    );
    assert_eq!(
        s.ok(&["docs", "show", "team", CHILD, "--cached"])["cached"],
        true
    );
}

#[test]
fn incomplete_content_and_traversal_remain_explicit() {
    let mock = Mock::new();
    mock.fixture();
    let s = Setup::new(&mock);
    mock.set(
        &format!("/pages/{CHILD}/markdown"),
        200,
        json!({"markdown":"Some content","truncated":true,"unknown_block_ids":[NESTED]}),
    );
    assert_eq!(
        s.ok(&["docs", "show", "team", CHILD])["value"]["partial"],
        true
    );
    mock.set(&format!("/blocks/{TOGGLE}/children"), 403, json!({}));
    let children = s.ok(&["docs", "browse", "team", ROOT]);
    assert!(!children["value"]["warnings"].as_array().unwrap().is_empty());
    assert_eq!(s.ok(&["docs", "index", "team"])["value"]["complete"], false);
    mock.set(
        &format!("/blocks/{ROOT}/children"),
        200,
        json!({"wrong":[]}),
    );
    let failed = s.run_at(&s.repo, &["docs", "browse", "team", ROOT, "--refresh"]);
    assert!(!failed.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&failed.stderr).unwrap()["error"]["kind"],
        "corrupt"
    );
}

#[test]
fn mapping_history_worktree_projection_and_cas_do_not_change_documents() {
    let mock = Mock::new();
    let s = Setup::new(&mock);
    let old = s.ok(&["create", "Local prototype"]);
    let mapping = s.ok(&["map", "create", "team", CHILD, "scheduler.py:2-3"]);
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    let first = mapping["revision"].as_str().unwrap();
    let edited = s.ok(&[
        "--author",
        "human",
        "map",
        "edit",
        id,
        "--if-match",
        first,
        "--context",
        "Changed explanation",
    ]);
    let failed = s.run_at(
        &s.repo,
        &[
            "map",
            "edit",
            id,
            "--if-match",
            first,
            "--context",
            "Stale edit",
        ],
    );
    assert_eq!(failed.status.code(), Some(3));
    assert_eq!(s.ok(&["map", "history", id]).as_array().unwrap().len(), 2);
    assert_eq!(
        s.ok(&["show", old["document"]["id"].as_str().unwrap()])["revision"],
        old["revision"]
    );
    let other = s.temp.path().join("feature");
    s.git(&["worktree", "add", "-qb", "feature", other.to_str().unwrap()]);
    parse(s.run_at(&other, &["init"]));
    assert!(
        parse(s.run_at(&other, &["map", "ls"]))["mappings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    Command::new("git")
        .arg("-C")
        .arg(&other)
        .args(["mv", "scheduler.py", "planner.py"])
        .output()
        .unwrap();
    let installed = parse(s.run_at(
        &other,
        &[
            "map",
            "install",
            "--from",
            s.repo.to_str().unwrap(),
            "--path",
            "planner.py",
        ],
    ));
    assert_eq!(installed["results"].as_array().unwrap().len(), 1);
    let located = parse(s.run_at(&other, &["map", "ls", "planner.py"]));
    assert_eq!(
        located["mappings"][0]["projections"][0]["locations"][0]["path"],
        "planner.py"
    );
    assert_eq!(
        located["mappings"][0]["view"]["record"]["mapping"]["anchors"][0]["path"],
        "scheduler.py"
    );
    parse(s.run_at(
        &other,
        &[
            "map",
            "archive",
            id,
            "--if-match",
            edited["revision"].as_str().unwrap(),
        ],
    ));
    assert!(
        parse(s.run_at(&other, &["map", "ls"]))["mappings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        s.ok(&["map", "ls"])["mappings"].as_array().unwrap().len(),
        1
    );
    assert!(mock.requests.lock().unwrap().is_empty());
}

#[test]
fn root_scope_discovers_connections_after_code_changes() {
    let mock = Mock::new();
    let s = Setup::new(&mock);
    s.ok(&["map", "create", "team", CHILD, "scheduler.py:2-3"]);
    let document = s.ok(&["create", "Local scheduler design"]);
    s.ok(&[
        "attach",
        document["document"]["id"].as_str().unwrap(),
        "scheduler.py:2-3",
    ]);
    std::fs::write(
        s.repo.join("scheduler.py"),
        "# inserted context\n# scheduler\ndef schedule():\n    return 42\n",
    )
    .unwrap();

    let mappings = s.ok(&["map", "ls", "."]);
    assert_eq!(mappings["mappings"].as_array().unwrap().len(), 1);
    assert_eq!(
        mappings["mappings"][0]["projections"][0]["locations"][0]["start"],
        3
    );
    let documents = s.ok(&["ls", "."]);
    assert_eq!(documents["documents"].as_array().unwrap().len(), 1);
    assert_eq!(
        documents["documents"][0]["projections"][0]["locations"][0]["start"],
        3
    );
    assert!(mock.requests.lock().unwrap().is_empty());
}

#[test]
fn cache_first_reads_and_explicit_fetch_preserve_observations_and_mapping_state() {
    let mock = Mock::new();
    mock.fixture();
    let s = Setup::new(&mock);
    let state = s.ok(&["status"]);
    let missing = s.run_at(&s.repo, &["docs", "show", "team", CHILD, "--cached"]);
    assert!(!missing.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&missing.stderr).unwrap()["error"]["kind"],
        "not_cached"
    );
    assert!(mock.requests.lock().unwrap().is_empty());
    s.ok(&["docs", "browse", "team"]);
    s.ok(&["docs", "browse", "team", ROOT]);
    let first = s.ok(&["docs", "show", "team", CHILD]);
    let count = mock.requests.lock().unwrap().len();
    for args in [
        vec!["docs", "browse", "team"],
        vec!["docs", "browse", "team", ROOT],
        vec!["docs", "show", "team", CHILD],
    ] {
        assert_eq!(s.ok(&args)["cached"], true);
    }
    assert_eq!(mock.requests.lock().unwrap().len(), count);
    let without_credentials = Command::new(env!("CARGO_BIN_EXE_lore"))
        .args([
            "--repo",
            s.repo.to_str().unwrap(),
            "--json",
            "docs",
            "show",
            "team",
            CHILD,
        ])
        .env("LORE_HOME", s.temp.path().join("knowledge"))
        .env_remove("LORE_TEST_NOTION_TOKEN")
        .output()
        .unwrap();
    assert_eq!(parse(without_credentials)["cached"], true);
    mock.set(
        &format!("/pages/{CHILD}/markdown"),
        200,
        json!({"markdown":"Updated backend body","truncated":false,"unknown_block_ids":[]}),
    );
    assert_eq!(
        s.ok(&["docs", "show", "team", CHILD])["value"]["content"],
        first["value"]["content"]
    );
    let fetched = s.ok(&["docs", "fetch", "team", CHILD]);
    assert_eq!(fetched["cached"], false);
    assert_eq!(
        s.ok(&["docs", "show", "team", CHILD])["value"]["content"],
        "Updated backend body"
    );
    mock.set(&format!("/pages/{CHILD}/markdown"), 403, json!({}));
    assert!(
        !s.run_at(&s.repo, &["docs", "fetch", "team", CHILD])
            .status
            .success()
    );
    assert_eq!(
        s.ok(&["docs", "show", "team", CHILD])["value"]["content"],
        "Updated backend body"
    );
    let bodies = mock
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, p, _)| p.ends_with("/markdown"))
        .count();
    let fetched = s.ok(&["docs", "fetch", "team"]);
    assert_eq!(fetched["value"]["complete"], true);
    assert_eq!(
        mock.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, p, _)| p.ends_with("/markdown"))
            .count(),
        bodies
    );
    let batch = s.run_at(&s.repo, &["docs", "fetch", "team", "--with-content"]);
    assert!(!batch.status.success());
    let result: Value = serde_json::from_slice(&batch.stdout).unwrap();
    assert!(
        result["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["status"] == "failed")
    );
    let human_batch = Command::new(env!("CARGO_BIN_EXE_lore"))
        .args(["docs", "fetch", "team", "--with-content"])
        .env("LORE_HOME", s.temp.path().join("knowledge"))
        .env("LORE_TEST_NOTION_TOKEN", SECRET)
        .output()
        .unwrap();
    assert!(!human_batch.status.success());
    let errors = String::from_utf8_lossy(&human_batch.stderr);
    assert!(
        errors.contains(CHILD),
        "Failed documents must be identifiable: {errors}"
    );
    assert!(
        errors.contains("HTTP 403"),
        "Failure reasons must be visible: {errors}"
    );
    assert!(!errors.contains(SECRET));
    assert_eq!(
        s.ok(&["status"]),
        state,
        "Fetch never changes workspace document or mapping state"
    );
}

#[test]
fn remote_mapping_discovery_fetches_metadata_without_installing_or_reading_bodies() {
    let mock = Mock::new();
    mock.fixture();
    let s = Setup::new(&mock);
    let mapping = s.ok(&["map", "create", "team", CHILD, "--glob", "compiler/**"]);
    let id = mapping["record"]["mapping"]["id"].as_str().unwrap();
    s.ok(&[
        "map",
        "publish",
        id,
        "--if-match",
        mapping["revision"].as_str().unwrap(),
    ]);
    let mut catalog = s.ok(&["map", "fetch"])["catalog"].clone();
    catalog["mappings"][id]["title"] = json!("Remote Scheduler Design");
    mock.set("/mapping-index", 200, catalog.clone());
    s.ok(&[
        "map",
        "source",
        "set",
        &format!("{}/mapping-index", mock.url),
        "--token-env",
        "LORE_TEST_NOTION_TOKEN",
    ]);
    let peer = s.temp.path().join("peer");
    s.git(&["worktree", "add", "-qb", "peer", peer.to_str().unwrap()]);
    parse(s.run_at(&peer, &["init"]));
    let before = parse(s.run_at(&peer, &["status"]));
    let unavailable = parse(s.run_at(
        &peer,
        &["context", "discover", "--cached", "compiler/scheduler.py"],
    ));
    assert_eq!(unavailable["shared_available"], false);
    assert!(!unavailable["warnings"].as_array().unwrap().is_empty());
    assert!(mock.requests.lock().unwrap().is_empty());
    let result = parse(s.run_at(&peer, &["context", "discover", "compiler/scheduler.py"]));
    assert_eq!(result["documents"][0]["title"], "Remote Scheduler Design");
    assert_eq!(result["documents"][0]["mappings"][0], id);
    assert_eq!(mock.requests.lock().unwrap().len(), 1);
    parse(s.run_at(
        &peer,
        &["context", "discover", "--cached", "compiler/scheduler.py"],
    ));
    parse(s.run_at(&peer, &["map", "fetch", "--max-age-seconds", "300"]));
    assert_eq!(
        mock.requests.lock().unwrap().len(),
        1,
        "Cache freshness is shared across worktrees"
    );
    assert_eq!(parse(s.run_at(&peer, &["status"])), before);
    mock.set("/mapping-index", 503, json!({}));
    assert!(!s.run_at(&peer, &["map", "fetch"]).status.success());
    assert_eq!(
        parse(s.run_at(
            &peer,
            &["context", "discover", "--cached", "compiler/scheduler.py"]
        ))["documents"],
        result["documents"]
    );
    catalog["repository"] = json!("different-repository");
    mock.set("/mapping-index", 200, catalog);
    assert!(!s.run_at(&peer, &["map", "fetch"]).status.success());
    assert_eq!(
        parse(s.run_at(
            &peer,
            &["context", "discover", "--cached", "compiler/scheduler.py"]
        ))["documents"],
        result["documents"]
    );
    assert!(
        mock.requests
            .lock()
            .unwrap()
            .iter()
            .all(|(_, path, _)| path.ends_with("/mapping-index")),
        "Discovery never reads Notion bodies or traverses its tree"
    );
}

#[test]
fn fresh_title_discovery_is_shared_and_does_not_require_credentials_again() {
    let mock = Mock::new();
    mock.fixture();
    let s = Setup::new(&mock);
    s.ok(&["docs", "fetch", "team", "--max-age-seconds", "900"]);
    let requests = mock.requests.lock().unwrap().len();
    let result = Command::new(env!("CARGO_BIN_EXE_lore"))
        .args([
            "--json",
            "docs",
            "fetch",
            "team",
            "--max-age-seconds",
            "900",
        ])
        .env("LORE_HOME", s.temp.path().join("knowledge"))
        .env_remove("LORE_TEST_NOTION_TOKEN")
        .output()
        .unwrap();
    assert_eq!(parse(result)["cached"], true);
    assert_eq!(mock.requests.lock().unwrap().len(), requests);
    assert!(
        mock.requests
            .lock()
            .unwrap()
            .iter()
            .all(|(_, path, _)| !path.ends_with("/markdown"))
    );
}

#[test]
fn executable_session_hook_refreshes_remote_discovery_in_the_background() {
    use std::{
        io::Write,
        process::Stdio,
        time::{Duration, Instant},
    };
    let mock = Mock::new();
    mock.fixture();
    let s = Setup::new(&mock);
    let mapping = s.ok(&["map", "create", "team", CHILD, "--glob", "*.py"]);
    s.ok(&[
        "map",
        "publish",
        mapping["record"]["mapping"]["id"].as_str().unwrap(),
        "--if-match",
        mapping["revision"].as_str().unwrap(),
    ]);
    let observation = s.ok(&["map", "fetch"]);
    mock.set("/mapping-index", 200, observation["catalog"].clone());
    s.ok(&[
        "map",
        "source",
        "set",
        &format!("{}/mapping-index", mock.url),
        "--token-env",
        "LORE_TEST_NOTION_TOKEN",
    ]);
    let peer = s.temp.path().join("agent-worktree");
    s.git(&["worktree", "add", "-qb", "agent", peer.to_str().unwrap()]);
    let invoke = |event: Value| {
        let mut child = Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../integrations/codex/lore_context.py"
            ))
            .env("LORE_EXECUTABLE", env!("CARGO_BIN_EXE_lore"))
            .env("LORE_HOME", s.temp.path().join("knowledge"))
            .env("LORE_TEST_NOTION_TOKEN", SECRET)
            .env("LORE_CODEX_REFRESH", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    let event =
        json!({"hook_event_name":"SessionStart", "session_id":"remote-session", "cwd":peer});
    assert!(invoke(event).stdout.is_empty());
    let path = s
        .temp
        .path()
        .join("knowledge/integrations/codex/refresh")
        .join(lore_core::hash(
            observation["catalog"]["repository"].as_str().unwrap(),
        ))
        .join("result.json");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(bytes) = std::fs::read(&path) {
            let result: Value = serde_json::from_slice(&bytes).unwrap();
            if result["results"].as_array().unwrap().len() == 2 {
                assert!(
                    result["results"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|r| r["status"] == "refreshed")
                );
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "Background discovery refresh did not complete"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let requests = mock.requests.lock().unwrap().len();
    let result = parse(invoke(
        json!({"hook_event_name":"PostToolUse", "session_id":"remote-session", "cwd":peer, "tool_input":{"path":"scheduler.py"}}),
    ));
    let context = result["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    let documents: Value = serde_json::from_str(context.split_once('\n').unwrap().1).unwrap();
    assert_eq!(documents[0]["title"], "Scheduler Design");
    assert_eq!(
        mock.requests.lock().unwrap().len(),
        requests,
        "Tool hook reads shared cache without remote requests"
    );
    assert!(
        mock.requests
            .lock()
            .unwrap()
            .iter()
            .all(|(_, path, _)| !path.ends_with("/markdown"))
    );
    assert!(
        parse(s.run_at(&peer, &["status"]))["workspace"]["mappings"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}
