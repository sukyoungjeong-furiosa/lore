use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

const TOKEN: &str = "test-only-model-token";
const TOKEN_ENV: &str = "LORE_TEST_MODEL_TOKEN";

struct Setup {
    temp: tempfile::TempDir,
    repo: PathBuf,
    id: String,
    revision: String,
}

impl Setup {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("code");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            repo.join("scheduler.rs"),
            "fn schedule() { old_algorithm(); }\n",
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "test@example.invalid"],
            vec!["config", "user.name", "test"],
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
        let mut s = Self {
            temp,
            repo,
            id: String::new(),
            revision: String::new(),
        };
        s.ok(&["init"]);
        s.ok(&[
            "backend",
            "add-notion",
            "notion",
            "--root",
            "00000000-0000-4000-8000-000000000001",
        ]);
        let mapping = s.ok(&[
            "map",
            "create",
            "notion",
            "00000000-0000-4000-8000-000000000001",
            "scheduler.rs:1",
        ]);
        s.id = mapping["record"]["mapping"]["id"].as_str().unwrap().into();
        s.revision = mapping["revision"].as_str().unwrap().into();
        s
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_input(args, None)
    }

    fn run_input(&self, args: &[&str], input: Option<&str>) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lore"))
            .args(["--repo", self.repo.to_str().unwrap(), "--json"])
            .args(args)
            .env("LORE_HOME", self.temp.path().join("knowledge"))
            .env(TOKEN_ENV, TOKEN)
            .env_remove("LORE_TEST_SAVED_MODEL_TOKEN")
            .env_remove("FURIOSA_ACCESS_API_KEY")
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
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn changed(&self) {
        std::fs::write(
            self.repo.join("scheduler.rs"),
            "fn plan() {\n    new_algorithm();\n}\n",
        )
        .unwrap();
    }

    fn config(&self, url: &str) -> PathBuf {
        let path = self.temp.path().join("resolver.json");
        std::fs::write(
            &path,
            json!({"base_url":url,"model":"configured-model","api_key_env":TOKEN_ENV}).to_string(),
        )
        .unwrap();
        path
    }
}

fn serve(status: u16, response: Value) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    serve_many(status, response, 1)
}

fn serve_many(
    status: u16,
    response: Value,
    count: usize,
) -> (String, std::thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let mut requests = vec![];
        for _ in 0..count {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("POST /v1/chat/completions "));
            let mut length = 0;
            let mut authorized = false;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                authorized |= line
                    .trim()
                    .eq_ignore_ascii_case(&format!("authorization: Bearer {TOKEN}"));
            }
            assert!(authorized);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            let body = response.to_string();
            write!(stream, "HTTP/1.1 {status} Result\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            requests.push(request);
        }
        requests
    });
    (url, thread)
}

#[test]
fn configured_model_returns_proposals_without_changing_mapping_or_document_state() {
    let s = Setup::new();
    s.changed();
    let proposal = json!({"candidates":[[{"path":"scheduler.rs","start":1,"end":3,"excerpt":"fn plan() {\n    new_algorithm();\n}"}]],"explanation":"The renamed function preserves the operation, but its implementation changed."});
    let (url, server) = serve(
        200,
        json!({"choices":[{"finish_reason":"stop","message":{"reasoning":"ignored reasoning","content":proposal.to_string()}}]}),
    );
    let config = s.config(&url);
    let before = s.ok(&["status"]);
    let result = s.ok(&[
        "map",
        "resolve",
        &s.id,
        "--resolver",
        "model",
        "--resolver-config",
        config.to_str().unwrap(),
        "--path",
        "scheduler.rs",
    ]);
    assert!(
        result["projections"][0]["locations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(result["projections"][0]["alternatives"][0][0]["start"], 1);
    assert_eq!(result["resolver"]["model"], "configured-model");
    assert_eq!(s.ok(&["map", "show", &s.id])["revision"], s.revision);
    assert_eq!(s.ok(&["status"]), before);
    let request = server.join().unwrap().remove(0);
    assert_eq!(request["model"], "configured-model");
    let input: Value =
        serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert!(
        input["source"]["snapshot"]
            .as_str()
            .unwrap()
            .contains("old_algorithm")
    );
    assert!(
        input["target"][0]["content"]
            .as_str()
            .unwrap()
            .contains("new_algorithm")
    );
    assert!(
        input["target"][0]["reference"]["context"]["working_tree"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn model_errors_and_incomplete_answers_are_not_empty_success_and_do_not_leak_response_bodies() {
    for (status, response, kind) in [
        (503, json!({"message":TOKEN}), "unavailable"),
        (
            200,
            json!({"choices":[{"finish_reason":"length","message":{"content":TOKEN}}]}),
            "corrupt",
        ),
        (
            200,
            json!({"choices":[{"finish_reason":"stop","message":{"content":TOKEN}}]}),
            "corrupt",
        ),
    ] {
        let s = Setup::new();
        s.changed();
        let (url, server) = serve(status, response);
        let config = s.config(&url);
        let output = s.run(&[
            "map",
            "resolve",
            &s.id,
            "--resolver",
            "model",
            "--resolver-config",
            config.to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains(TOKEN));
        assert_eq!(
            serde_json::from_str::<Value>(&stderr).unwrap()["error"]["kind"],
            kind
        );
        server.join().unwrap();
    }
}

#[test]
fn profiles_reuse_saved_credentials_and_environment_overrides_them() {
    let s = Setup::new();
    s.changed();
    let (url, server) = serve_many(
        200,
        json!({"choices":[{"finish_reason":"stop","message":{"content":json!({"candidates":[],"explanation":"No correspondence established."}).to_string()}}]}),
        3,
    );
    s.ok(&[
        "resolver",
        "connection",
        "set",
        "access",
        "--url",
        &url,
        "--token-env",
        "LORE_TEST_SAVED_MODEL_TOKEN",
    ]);
    let saved = s.run_input(
        &["resolver", "auth", "set", "access", "--stdin"],
        Some(TOKEN),
    );
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    assert!(!String::from_utf8_lossy(&saved.stdout).contains(TOKEN));
    assert_eq!(
        s.ok(&["resolver", "auth", "status", "access"])["source"],
        "stored"
    );
    let options = s.temp.path().join("sampling.json");
    let sampling = json!({"temperature":1.0,"top_p":0.95,"presence_penalty":0.0,"chat_template_kwargs":{"enable_thinking":false}});
    std::fs::write(&options, sampling.to_string()).unwrap();
    for name in ["first", "second"] {
        let mut args = vec![
            "resolver",
            "profile",
            "set",
            name,
            "--connection",
            "access",
            "--model",
            name,
        ];
        if name == "first" {
            args.extend(["--request-options-file", options.to_str().unwrap()]);
        }
        s.ok(&args);
        s.ok(&["resolver", "use", name]);
        let result = s.ok(&[
            "map",
            "resolve",
            &s.id,
            "--resolver",
            "model",
            "--path",
            "scheduler.rs",
        ]);
        assert_eq!(result["resolver"]["profile"], name);
        assert_eq!(result["resolver"]["model_connection"], "access");
        assert!(
            result["resolver"]["config_digest"]
                .as_str()
                .is_some_and(|s| s.len() == 64)
        );
        assert_eq!(result["resolver"]["model"], name);
    }
    assert!(
        s.run_input(
            &["resolver", "auth", "set", "access", "--stdin"],
            Some("superseded-token")
        )
        .status
        .success()
    );
    s.ok(&[
        "resolver",
        "connection",
        "set",
        "access",
        "--url",
        &url,
        "--token-env",
        TOKEN_ENV,
    ]);
    assert_eq!(
        s.ok(&["resolver", "auth", "status", "access"])["source"],
        "environment"
    );
    let explicit = s.ok(&[
        "map",
        "resolve",
        &s.id,
        "--resolver",
        "model",
        "--profile",
        "first",
    ]);
    assert_eq!(explicit["resolver"]["profile"], "first");
    assert_eq!(s.ok(&["resolver", "list"])["default_profile"], "second");
    let requests = server.join().unwrap();
    for request in [&requests[0], &requests[2]] {
        for (key, value) in sampling.as_object().unwrap() {
            assert_eq!(request[key], *value);
        }
    }
    assert!(requests[1].get("chat_template_kwargs").is_none());
    assert_eq!(
        requests
            .iter()
            .map(|r| r["model"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second", "first"]
    );
    assert!(!s.ok(&["resolver", "list"]).to_string().contains(TOKEN));
    assert_eq!(s.ok(&["map", "show", &s.id])["revision"], s.revision);
    s.ok(&[
        "resolver",
        "connection",
        "set",
        "access",
        "--url",
        "http://127.0.0.1:1/v1",
        "--token-env",
        "LORE_TEST_SAVED_MODEL_TOKEN",
    ]);
    let denied = s.run(&["map", "resolve", &s.id, "--resolver", "model"]);
    assert!(!denied.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&denied.stderr).unwrap()["error"]["kind"],
        "permission_denied"
    );
    s.ok(&["resolver", "auth", "remove", "access"]);
    assert_eq!(
        s.ok(&["resolver", "auth", "status", "access"])["source"],
        "missing"
    );
    s.ok(&["resolver", "use", "--clear"]);
    assert!(s.ok(&["resolver", "list"])["default_profile"].is_null());
    for name in ["first", "second"] {
        s.ok(&["resolver", "profile", "remove", name]);
    }
    s.ok(&["resolver", "connection", "remove", "access"]);
    assert!(
        s.ok(&["resolver", "list"])["connections"]
            .as_object()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn api_request_options_cannot_override_projection_protocol_fields() {
    let s = Setup::new();
    s.changed();
    for field in ["model", "messages", "max_tokens", "stream"] {
        let config = s.config("http://127.0.0.1:1/v1");
        let mut value: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        value["request_options"] = json!({field: true});
        std::fs::write(&config, value.to_string()).unwrap();
        let output = s.run(&[
            "map",
            "resolve",
            &s.id,
            "--resolver",
            "model",
            "--resolver-config",
            config.to_str().unwrap(),
        ]);
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["kind"], "invalid_input");
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("cannot override")
        );
    }
}

#[test]
fn resolver_configuration_does_not_require_a_workspace() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lore"))
        .args([
            "--repo",
            temp.path().join("not-a-checkout").to_str().unwrap(),
            "--json",
            "resolver",
            "connection",
            "set",
            "access",
        ])
        .env("LORE_HOME", temp.path().join("knowledge"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let settings: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(settings["connections"]["access"].is_object());
    assert!(settings["default_profile"].is_null());
}

#[test]
fn model_mode_needs_no_token_for_exact_text_and_reports_missing_credentials_for_changed_code() {
    let s = Setup::new();
    let result = s.ok(&["map", "resolve", &s.id, "--resolver", "model"]);
    assert_eq!(result["projections"][0]["locations"][0]["start"], 1);
    s.changed();
    let output = s.run(&["map", "resolve", &s.id, "--resolver", "model"]);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["kind"], "permission_denied");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("FURIOSA_ACCESS_API_KEY")
    );
    let result = s.ok(&["map", "resolve", &s.id]);
    assert!(
        result["projections"][0]["locations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
