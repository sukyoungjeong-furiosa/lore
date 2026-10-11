use crate::{BackendRegistry, RemoteRegistry, Workspace, atomic_write};
use lore_core::{
    AnchorSummary, ErrorKind, KnowledgeSnapshot, KnowledgeStoreId, LoreDocument, RelatedLore,
    Result, RoutingDocument, discover_lore, hash,
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
        let available = self.list(false)?;
        let committed_mappings = self.list_scoped_mappings(true)?;
        let fingerprint = hash(serde_json::to_vec(&(
            &state.repository_id,
            &state.documents,
            &state.mappings,
            &state.store_mappings,
            committed_mappings
                .iter()
                .map(|v| (&v.view.record.mapping.id, &v.view.revision))
                .collect::<Vec<_>>(),
            available
                .iter()
                .map(|view| (&view.document.id, &view.revision))
                .collect::<Vec<_>>(),
        ))?);
        let path = self.state_path.with_file_name("routing-index.json");
        match std::fs::read(&path) {
            Ok(bytes) => {
                let index: RoutingIndex = serde_json::from_slice(&bytes)?;
                if index.schema_version == 3 && index.fingerprint == fingerprint {
                    return Ok(index.documents);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut documents = vec![];
        for item in self.list_scoped_mappings(true)? {
            let mapping = item.view.record.mapping;
            if !mapping.archived {
                documents.push(RoutingDocument {
                    store: item.store,
                    mapping: Some(mapping.id.clone()),
                    document: LoreDocument::from_reference(mapping.document)?,
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
        for view in available {
            let document = view.document;
            if !document.archived {
                documents.push(RoutingDocument {
                    store: None,
                    mapping: None,
                    document: LoreDocument::Local { id: document.id },
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
                schema_version: 3,
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
        let mut warnings = vec![];
        let mut documents = self.routing_documents()?;
        if !self.central_view {
            let central = routing_snapshot(self.fetch_knowledge()?.snapshot, None)?;
            let working_mappings: BTreeSet<_> = self
                .list_scoped_mappings(true)?
                .into_iter()
                .map(|item| item.view.record.mapping.id)
                .collect();
            let working_documents: BTreeSet<_> = self
                .list(true)?
                .into_iter()
                .map(|d| LoreDocument::Local { id: d.document.id })
                .collect();
            documents.extend(central.into_iter().filter(|entry| match &entry.mapping {
                Some(id) => !working_mappings.contains(id),
                None => !working_documents.contains(&entry.document),
            }));
        }
        let state = self.state()?;
        for (store, name) in &state.connected_remotes {
            let fetched = (|| {
                let connection = RemoteRegistry::new(home).get(name)?;
                if connection.store != *store {
                    return Err(lore_core::Error::new(
                        ErrorKind::Conflict,
                        "Configured remote now selects a different store; reconnect explicitly",
                    ));
                }
                if refresh || (!cached && self.cached_remote_knowledge(home, name)?.is_none()) {
                    self.fetch_remote_knowledge(home, name)?;
                }
                let version = self.cached_remote_knowledge(home, name)?.ok_or_else(|| {
                    lore_core::Error::new(
                        ErrorKind::NotCached,
                        "Run lore fetch --remote to retrieve knowledge",
                    )
                })?;
                version.snapshot.validate(&self.code.repository_id)?;
                version.snapshot.validate_document_references()?;
                routing_snapshot(version.snapshot, Some(store.clone()))
            })();
            match fetched {
                Ok(shared) => documents.extend(shared),
                Err(error) => warnings.push(format!("Lore remote {name}: {}", error.message)),
            }
        }
        documents.retain(|entry| !excluded.contains(&entry.document));
        let local_views = self.list(true)?;
        let archived: BTreeSet<_> = local_views
            .iter()
            .filter(|d| d.document.archived)
            .map(|d| d.document.id.clone())
            .collect();
        documents.retain(|entry| !matches!(&entry.document, LoreDocument::Local { id } if entry.store.is_none() && archived.contains(id)));
        for entry in &mut documents {
            if let LoreDocument::Local { id } = &entry.document
                && entry.title.is_none()
                && let Some(view) = local_views.iter().find(|view| view.document.id == *id)
            {
                entry.title = Some(view.document.title.clone());
            }
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

            shared_available: warnings.is_empty(),
            warnings,
        })
    }
}

#[derive(Serialize)]
pub struct DiscoveryResult {
    pub documents: Vec<RelatedLore>,
    pub shared_available: bool,
    pub warnings: Vec<String>,
}

fn routing_snapshot(
    snapshot: KnowledgeSnapshot,
    store: Option<KnowledgeStoreId>,
) -> Result<Vec<RoutingDocument>> {
    let mut documents = vec![];
    for record in snapshot.mappings.into_values() {
        let mapping = record.mapping;
        let document = LoreDocument::from_reference(mapping.document)?;
        let local = if let LoreDocument::Local { id } = &document {
            snapshot.documents.get(id)
        } else {
            None
        };
        if mapping.archived || local.is_some_and(|r| r.document.archived) {
            continue;
        }
        documents.push(RoutingDocument {
            store: store.clone(),
            mapping: Some(mapping.id),
            document,
            title: local.map(|r| r.document.title.clone()),
            scopes: mapping.scopes,
            anchors: mapping
                .anchors
                .into_iter()
                .map(|a| AnchorSummary {
                    id: a.id,
                    path: a.path,
                    start: a.start,
                    end: a.end,
                    context: a.context,
                })
                .collect(),
        });
    }
    for record in snapshot
        .documents
        .into_values()
        .filter(|r| !r.document.archived)
    {
        documents.push(RoutingDocument {
            store: store.clone(),
            mapping: None,
            document: LoreDocument::Local {
                id: record.document.id,
            },
            title: Some(record.document.title),
            scopes: vec![],
            anchors: record
                .document
                .anchors
                .into_iter()
                .map(|a| AnchorSummary {
                    id: a.id,
                    path: a.path,
                    start: a.start,
                    end: a.end,
                    context: a.context,
                })
                .collect(),
        });
    }
    Ok(documents)
}
