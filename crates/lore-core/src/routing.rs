use crate::{AnchorSummary, DocumentId, DocumentReference, MappingId, PathScope, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum LoreDocument {
    Backend { reference: DocumentReference },
    Local { id: DocumentId },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoutingDocument {
    #[serde(default)]
    pub mapping: Option<MappingId>,
    pub document: LoreDocument,
    pub title: Option<String>,
    pub scopes: Vec<PathScope>,
    pub anchors: Vec<AnchorSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoutingMatch {
    Scope {
        path: String,
        glob: String,
    },
    RecordedAnchor {
        path: String,
        start: usize,
        end: usize,
        head: Option<String>,
        working_tree: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelatedLore {
    pub document: LoreDocument,
    pub title: Option<String>,
    pub matches: Vec<RoutingMatch>,
    pub mappings: Vec<MappingId>,
}

/// Discovers recorded relationships without reading code or establishing current correspondence.
pub fn discover_lore(
    documents: &[RoutingDocument],
    repository: &str,
    paths: &[String],
) -> Result<Vec<RelatedLore>> {
    let mut results: BTreeMap<LoreDocument, RelatedLore> = BTreeMap::new();
    for document in documents {
        let mut matches = vec![];
        for scope in &document.scopes {
            if scope.repository != repository {
                continue;
            }
            let matcher = scope.matcher()?;
            for path in paths {
                // A mentioned directory can identify a pattern rooted beneath that directory.
                if matcher.is_match(path) || scope.glob.starts_with(&format!("{path}/")) {
                    matches.push(RoutingMatch::Scope {
                        path: path.clone(),
                        glob: scope.glob.clone(),
                    });
                }
            }
        }
        for anchor in &document.anchors {
            if anchor.context.repository == repository
                && paths.iter().any(|path| {
                    anchor.path == *path || anchor.path.starts_with(&format!("{path}/"))
                })
            {
                matches.push(RoutingMatch::RecordedAnchor {
                    path: anchor.path.clone(),
                    start: anchor.start,
                    end: anchor.end,
                    head: anchor.context.head.clone(),
                    working_tree: anchor.context.working_tree,
                });
            }
        }
        if matches.is_empty() {
            continue;
        }
        let related = results
            .entry(document.document.clone())
            .or_insert_with(|| RelatedLore {
                document: document.document.clone(),
                title: document.title.clone(),
                matches: vec![],
                mappings: vec![],
            });
        if let Some(id) = &document.mapping
            && !related.mappings.contains(id)
        {
            related.mappings.push(id.clone());
        }
        for location in matches {
            if !related.matches.contains(&location) {
                related.matches.push(location);
            }
        }
    }
    Ok(results.into_values().collect())
}
