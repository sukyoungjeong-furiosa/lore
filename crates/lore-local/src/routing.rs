use crate::{BackendRegistry, CatalogStore, Workspace, atomic_write};
use lore_core::{
    AnchorSummary, ErrorKind, LoreDocument, RelatedLore, Result, RoutingDocument, discover_lore,
    hash,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

#[derive(Serialize, Deserialize)]
struct RoutingIndex {
    schema_version: u32,
    fingerprint: String,
    documents: Vec<RoutingDocument>,
}

impl Workspace {
    fn routing_documents(&self) -> Result<Vec<RoutingDocument>> {
        let state = self.state()?;
        let fingerprint = hash(serde_json::to_vec(&(
            &state.repository_id,
            &state.documents,
            &state.mappings,
        ))?);
        let path = self.state_path.with_file_name("routing-index.json");
        match std::fs::read(&path) {
            Ok(bytes) => {
                let index: RoutingIndex = serde_json::from_slice(&bytes)?;
                if index.schema_version == 2 && index.fingerprint == fingerprint {
                    return Ok(index.documents);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut documents = vec![];
        for (id, active) in state.mappings {
            let mapping = self.mapping_repository.load(&id, &active.revision)?.mapping;
            if !mapping.archived {
                documents.push(RoutingDocument {
                    mapping: Some(id),
                    document: LoreDocument::Backend {
                        reference: mapping.document,
                    },
                    title: None,
                    scopes: mapping.scopes,
                    anchors: mapping
                        .anchors
                        .into_iter()
                        .map(|anchor| AnchorSummary {
                            id: anchor.id,
                            path: anchor.path,
                            start: anchor.start,
                            end: anchor.end,
                            context: anchor.context,
                        })
                        .collect(),
                });
            }
        }
        for (id, active) in state.documents {
            let document = self.repository.load(&id, &active.revision)?.document;
            if !document.archived {
                documents.push(RoutingDocument {
                    mapping: None,
                    document: LoreDocument::Local { id },
                    title: Some(document.title),
                    scopes: vec![],
                    anchors: document
                        .anchors
                        .into_iter()
                        .map(|anchor| AnchorSummary {
                            id: anchor.id,
                            path: anchor.path,
                            start: anchor.start,
                            end: anchor.end,
                            context: anchor.context,
                        })
                        .collect(),
                });
            }
        }
        atomic_write(
            &path,
            &serde_json::to_vec(&RoutingIndex {
                schema_version: 2,
                fingerprint,
                documents: documents.clone(),
            })?,
        )?;
        Ok(documents)
    }

    pub fn related_lore(
        &self,
        paths: &[String],
        home: &Path,
        cached: bool,
        refresh: bool,
        excluded: &BTreeSet<LoreDocument>,
    ) -> Result<DiscoveryResult> {
        let store = CatalogStore::new(home, &self.code.repository_id);
        let shared = if refresh {
            store.fetch(None)
        } else {
            store.read(cached)
        };
        let mut warnings = vec![];
        let mut shared_fetched_at_ms = None;
        let mut shared_source = None;
        let mut documents: Vec<_> = self
            .routing_documents()?
            .into_iter()
            .filter(|entry| !excluded.contains(&entry.document))
            .collect();
        let state = self.state()?;
        match shared {
            Ok(observation) => {
                shared_fetched_at_ms = Some(observation.fetched_at_ms);
                shared_source = Some(observation.source);
                for (id, entry) in observation.catalog.mappings {
                    let mapping = entry.view.record.mapping;
                    let document = LoreDocument::Backend {
                        reference: mapping.document,
                    };
                    // Local activation overrides the shared relationship, including local archive.
                    if !mapping.archived
                        && !state.mappings.contains_key(&id)
                        && !excluded.contains(&document)
                    {
                        documents.push(RoutingDocument {
                            mapping: Some(id),
                            document,
                            title: entry.title,
                            scopes: mapping.scopes,
                            anchors: mapping.anchors,
                        });
                    }
                }
            }
            Err(error) if cached && error.kind == ErrorKind::NotCached => {
                warnings.push(error.message)
            }
            Err(error) => return Err(error),
        }
        let mut related = discover_lore(&documents, &self.code.repository_id, paths)?;
        for item in &mut related {
            if let LoreDocument::Backend { reference } = &item.document {
                match BackendRegistry::new(home)
                    .cache(&reference.backend)
                    .cached_title(reference)
                {
                    Ok(title) => item.title = Some(title),
                    Err(error) if error.kind == ErrorKind::NotCached => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(DiscoveryResult {
            documents: related,
            shared_source,
            shared_fetched_at_ms,
            shared_available: warnings.is_empty(),
            warnings,
        })
    }
}

#[derive(Serialize)]
pub struct DiscoveryResult {
    pub documents: Vec<RelatedLore>,
    pub shared_source: Option<String>,
    pub shared_fetched_at_ms: Option<u64>,
    pub shared_available: bool,
    pub warnings: Vec<String>,
}
