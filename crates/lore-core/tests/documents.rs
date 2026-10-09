use lore_core::{DocumentView, Revision, RevisionId};

#[test]
fn legacy_tagged_revisions_keep_identity_and_expose_root_directory() {
    for tags in ["[]", "[\"ai generated\"]"] {
        let bytes = format!(
            r#"{{"parent":null,"actor":"agent","at_ms":123,"document":{{"id":"0402c6e3-0394-4b44-bb97-74fa29e3f3d1","title":"Original","content_format":"markdown","content":"Original body","tags":{tags},"authorship":["agent"],"curation":null,"archived":false,"anchors":[]}}}}"#
        );
        let previous_id = RevisionId::of(&bytes);
        let mut revision: Revision = serde_json::from_str(&bytes).unwrap();
        revision.document.validate().unwrap();
        assert_eq!(revision.id().unwrap(), previous_id);
        assert_eq!(revision.document.directory, "");
        let view = DocumentView::new(previous_id.clone(), revision.clone());
        let public = serde_json::to_value(view).unwrap();
        assert_eq!(public["document"]["directory"], "");
        assert!(public["document"].get("tags").is_none());
        assert!(public["document"].get("curation").is_none());
        assert!(public.get("curated").is_none());
        revision.document.directory = "Compiler/Scheduler".into();
        revision.document.retire_legacy_metadata();
        assert_ne!(revision.id().unwrap(), previous_id);
        assert!(
            serde_json::to_value(revision).unwrap()["document"]
                .get("tags")
                .is_none()
        );
    }
}

#[test]
fn historical_curation_records_keep_revision_identity_without_exposing_status() {
    let bytes = r#"{"parent":null,"actor":"human","at_ms":123,"document":{"id":"0402c6e3-0394-4b44-bb97-74fa29e3f3d1","title":"Original","content_format":"markdown","content":"Original body","authorship":["human"],"curation":{"by":"Reviewer","content_digest":"obsolete-digest","at_ms":100},"archived":false,"anchors":[]}}"#;
    let original_id = RevisionId::of(bytes);
    let mut revision: Revision = serde_json::from_str(bytes).unwrap();
    assert_eq!(revision.id().unwrap(), original_id);
    let public =
        serde_json::to_value(DocumentView::new(original_id.clone(), revision.clone())).unwrap();
    assert!(public["document"].get("curation").is_none());
    assert!(public.get("curated").is_none());
    revision.document.retire_legacy_metadata();
    assert!(
        serde_json::to_value(&revision).unwrap()["document"]
            .get("curation")
            .is_none()
    );
    assert_ne!(revision.id().unwrap(), original_id);
}
