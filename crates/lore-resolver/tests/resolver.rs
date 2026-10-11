use lore_core::*;
use lore_resolver::*;
use serde_json::json;
use std::{cell::RefCell, collections::BTreeMap};

struct Code {
    files: RefCell<BTreeMap<String, String>>,
}

impl Code {
    fn new() -> Self {
        Self {
            files: RefCell::new(BTreeMap::from([
                (
                    "planner.rs".into(),
                    "fn plan() {\n    prepare();\n    execute();\n}\n".into(),
                ),
                (
                    "helpers.rs".into(),
                    "fn prepare() {}\nfn execute() {}\n".into(),
                ),
            ])),
        }
    }
}

impl CodeRepository for Code {
    fn identity(&self) -> &str {
        "repo"
    }
    fn head(&self) -> Option<String> {
        Some("target".into())
    }
    fn read(&self, path: &str) -> Result<Option<String>> {
        Ok(self.files.borrow().get(path).cloned())
    }
    fn committed_content(&self, _: &str) -> Result<Option<String>> {
        Ok(None)
    }
    fn paths(&self, _: &Anchor, _: Option<&str>) -> Result<Vec<String>> {
        Ok(self.files.borrow().keys().cloned().collect())
    }
}

struct Proposer<F>(F);

impl<F: Fn(&ProjectionInput) -> Result<ProjectionProposal>> ProjectionProposer for Proposer<F> {
    fn propose(&self, input: &ProjectionInput) -> Result<ProjectionProposal> {
        self.0(input)
    }
}

fn anchor() -> Anchor {
    let snapshot = "fn schedule() { old_algorithm(); }\n".to_string();
    Anchor {
        id: DocumentId::new(),
        path: "scheduler.rs".into(),
        start: 1,
        end: 1,
        context: CodeContext {
            repository: "repo".into(),
            head: Some("source".into()),
            working_tree: false,
            content_digest: hash(&snapshot),
        },
        snapshot,
    }
}

fn proposal(value: serde_json::Value) -> Result<ProjectionProposal> {
    Ok(serde_json::from_value(value).unwrap())
}

#[test]
fn joint_locations_and_competing_proposals_preserve_original_reference() {
    let code = Code::new();
    let source = anchor();
    let before = source.clone();
    let resolver = ModelResolver {
        max_input_bytes: 10000,
        proposer: Proposer(|input: &ProjectionInput| {
            assert_eq!(input.source.context.head.as_deref(), Some("source"));
            assert_eq!(input.target.len(), 2);
            proposal(json!({"candidates":[
                [{"path":"planner.rs","start":2,"end":3,"excerpt":"    prepare();\n    execute();"},
                 {"path":"helpers.rs","start":1,"end":2,"excerpt":"fn prepare() {}\nfn execute() {}"}],
                [{"path":"planner.rs","start":1,"end":4,"excerpt":"fn plan() {\n    prepare();\n    execute();\n}"}]
            ],"explanation":"The original operation is split across a caller and helpers; another scope is possible."}))
        }),
    };
    let result = resolver.resolve(&source, &code, None).unwrap();
    assert_eq!(source, before);
    assert!(result.locations.is_empty());
    assert_eq!(result.alternatives.len(), 2);
    assert_eq!(result.alternatives[0].len(), 2);
    assert_eq!(result.target[0].context.head.as_deref(), Some("target"));
    assert!(result.target.iter().all(|file| file.context.working_tree));
}

#[test]
fn model_proposals_do_not_erase_exact_text_alternatives() {
    let code = Code::new();
    let source = anchor();
    code.files
        .borrow_mut()
        .insert("first.rs".into(), source.snapshot.clone());
    code.files
        .borrow_mut()
        .insert("second.rs".into(), source.snapshot.clone());
    for candidates in [
        json!([]),
        json!([[{"path":"first.rs","start":1,"end":1,"excerpt":source.snapshot.trim_end()}]]),
    ] {
        let resolver = ModelResolver {
            max_input_bytes: 10000,
            proposer: Proposer(|_: &ProjectionInput| {
                proposal(
                    json!({"candidates":candidates,"explanation":"No additional correspondence established"}),
                )
            }),
        };
        let result = resolver.resolve(&source, &code, None).unwrap();
        assert_eq!(result.alternatives.len(), 2);
        assert!(result.locations.is_empty());
    }
}

#[test]
fn fabricated_paths_line_ranges_and_excerpts_are_rejected() {
    for location in [
        json!({"path":"../other.rs","start":1,"end":1,"excerpt":"x"}),
        json!({"path":"planner.rs","start":0,"end":1,"excerpt":"x"}),
        json!({"path":"planner.rs","start":2,"end":200,"excerpt":"x"}),
        json!({"path":"planner.rs","start":1,"end":1,"excerpt":"wrong line"}),
    ] {
        let resolver = ModelResolver {
            max_input_bytes: 10000,
            proposer: Proposer(|_: &ProjectionInput| {
                proposal(json!({"candidates":[[location]],"explanation":"A claim"}))
            }),
        };
        assert_eq!(
            resolver
                .resolve(&anchor(), &Code::new(), None)
                .unwrap_err()
                .kind,
            ErrorKind::Corrupt
        );
    }
}

#[test]
fn changed_target_and_new_candidates_during_a_request_invalidate_the_result() {
    for add_file in [false, true] {
        let code = Code::new();
        let resolver = ModelResolver {
            max_input_bytes: 10000,
            proposer: Proposer(|_: &ProjectionInput| {
                code.files.borrow_mut().insert(
                    if add_file { "new.rs" } else { "planner.rs" }.into(),
                    "changed during inference".into(),
                );
                proposal(
                    json!({"candidates":[],"explanation":"No correspondence in captured input"}),
                )
            }),
        };
        assert_eq!(
            resolver.resolve(&anchor(), &code, None).unwrap_err().kind,
            ErrorKind::Conflict
        );
    }
}

#[test]
fn failure_is_distinct_from_no_correspondence() {
    let failing = ModelResolver {
        max_input_bytes: 10000,
        proposer: Proposer(|_: &ProjectionInput| {
            Err(Error::new(ErrorKind::Unavailable, "Unavailable provider"))
        }),
    };
    assert_eq!(
        failing
            .resolve(&anchor(), &Code::new(), None)
            .unwrap_err()
            .kind,
        ErrorKind::Unavailable
    );
    let empty = ModelResolver {
        max_input_bytes: 10000,
        proposer: Proposer(|_: &ProjectionInput| {
            proposal(json!({"candidates":[],"explanation":"No supported correspondence"}))
        }),
    };
    let result = empty.resolve(&anchor(), &Code::new(), None).unwrap();
    assert!(result.locations.is_empty() && result.alternatives.is_empty());
    assert_eq!(result.target.len(), 2);
}

#[test]
fn exact_correspondence_repository_mismatch_and_limits_avoid_model_calls() {
    let mut source = anchor();
    let code = Code::new();
    let resolver = ModelResolver {
        max_input_bytes: 1,
        proposer: Proposer(|_: &ProjectionInput| panic!("No model call expected")),
    };
    assert_eq!(
        resolver.resolve(&source, &code, None).unwrap_err().kind,
        ErrorKind::InvalidInput
    );
    code.files
        .borrow_mut()
        .insert(source.path.clone(), source.snapshot.clone());
    assert_eq!(
        resolver.resolve(&source, &code, None).unwrap().locations[0].start,
        1
    );
    source.context.repository = "other-repository".into();
    let result = resolver.resolve(&source, &code, None).unwrap();
    assert!(result.locations.is_empty() && result.alternatives.is_empty());
}
