use lore_core::*;
struct Code {
    content: Option<String>,
    unavailable: bool,
}
impl CodeRepository for Code {
    fn identity(&self) -> &str {
        "example"
    }
    fn head(&self) -> Option<String> {
        Some("target-commit".into())
    }
    fn read(&self, _: &str) -> Result<Option<String>> {
        if self.unavailable {
            Err(Error::new(
                ErrorKind::Unavailable,
                "Code service is unavailable",
            ))
        } else {
            Ok(self.content.clone())
        }
    }
    fn paths(&self, _: &Anchor, _: Option<&str>) -> Result<Vec<String>> {
        Ok(vec!["code.rs".into()])
    }
    fn committed_content(&self, _: &str) -> Result<Option<String>> {
        Ok(None)
    }
}
fn anchor() -> Anchor {
    let snapshot = "same\nsame\n".to_string();
    Anchor {
        id: DocumentId::new(),
        path: "code.rs".into(),
        start: 2,
        end: 2,
        context: CodeContext {
            repository: "example".into(),
            head: Some("source-commit".into()),
            working_tree: false,
            content_digest: hash(&snapshot),
        },
        snapshot,
    }
}
#[test]
fn unchanged_file_retains_selected_occurrence_even_with_duplicate_excerpts() {
    let anchor = anchor();
    let code = Code {
        content: Some(anchor.snapshot.clone()),
        unavailable: false,
    };
    let result = TextResolver.resolve(&anchor, &code, None).unwrap();
    assert_eq!(result.locations[0].start, 2);
    assert!(result.alternatives.is_empty());
    assert_eq!(result.target[0].path, "code.rs");
    assert_eq!(
        result.target[0].context.head.as_deref(),
        Some("target-commit")
    );
    assert_eq!(anchor.context.head.as_deref(), Some("source-commit"));
}
#[test]
fn unsupported_correspondence_is_distinct_from_resolver_failure() {
    let code = Code {
        content: None,
        unavailable: false,
    };
    let result = TextResolver.resolve(&anchor(), &code, None).unwrap();
    assert!(result.locations.is_empty());
    assert!(!result.explanation.is_empty());
    let failing = Code {
        content: None,
        unavailable: true,
    };
    assert_eq!(
        TextResolver
            .resolve(&anchor(), &failing, None)
            .unwrap_err()
            .kind,
        ErrorKind::Unavailable
    );
}
#[test]
fn malformed_anchor_is_rejected_without_panicking() {
    let mut anchor = anchor();
    anchor.start = 0;
    assert_eq!(
        TextResolver
            .resolve(
                &anchor,
                &Code {
                    content: None,
                    unavailable: false
                },
                None
            )
            .unwrap_err()
            .kind,
        ErrorKind::InvalidInput
    );
}

#[test]
fn inconsistent_snapshot_cannot_establish_unchanged_correspondence() {
    let mut anchor = anchor();
    let target = "different\ncode\n".to_string();
    anchor.context.content_digest = hash(&target);
    let code = Code {
        content: Some(target),
        unavailable: false,
    };
    assert_eq!(
        TextResolver.resolve(&anchor, &code, None).unwrap_err().kind,
        ErrorKind::InvalidInput
    );
}
