use crate::{
    BackendRegistry, CatalogStore, HttpMappingRemote, LocalMappingClient, LocalStoreConnection,
    RemoteCatalogCache, RemoteRegistry, Workspace, atomic_write,
};
use lore_core::{
    AnchorSummary, ErrorKind, LoreDocument, MappingStoreClient, RelatedLore, Result,
    RoutingDocument, discover_lore, hash,
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
                if index.schema_version == 2 && index.fingerprint == fingerprint {
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
                            store: None,
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
        for (store, name) in &state.connected_remotes {
            let observation = (|| {
                let connection = RemoteRegistry::new(home).get(name)?;
                if connection.store != *store {
                    return Err(lore_core::Error::new(
                        ErrorKind::Conflict,
                        "Configured remote now selects a different store; reconnect explicitly",
                    ));
                }
                let cache = RemoteCatalogCache::new(home);
                if !refresh {
                    match cache.read(store, &self.code.repository_id) {
                        Ok(observation) => return Ok(observation),
                        Err(error) if error.kind == ErrorKind::NotCached && !cached => {}
                        Err(error) => return Err(error),
                    }
                }
                cache.fetch(
                    &HttpMappingRemote::new(home, name, connection)?,
                    &self.code.repository_id,
                )
            })();
            match observation {
                Ok(observation) => {
                    let mut shared = vec![];
                    for (id, entry) in observation.catalog.catalog.mappings {
                        let mapping = entry.view.record.mapping;
                        let document = LoreDocument::Backend {
                            reference: mapping.document,
                        };
                        if !mapping.archived
                            && !state
                                .store_mappings
                                .get(store)
                                .is_some_and(|mappings| mappings.contains_key(&id))
                            && !excluded.contains(&document)
                        {
                            shared.push(RoutingDocument {
                                store: Some(store.clone()),
                                mapping: Some(id),
                                document,
                                title: entry.title,
                                scopes: mapping.scopes,
                                anchors: mapping.anchors,
                            });
                        }
                    }
                    documents.extend(shared);
                }
                Err(error) => warnings.push(format!("Lore remote {name}: {}", error.message)),
            }
        }
        for (store, directory) in &state.connected_local_stores {
            let catalog = (|| {
                let client = LocalMappingClient::new(LocalStoreConnection {
                    store: store.clone(),
                    directory: directory.clone(),
                })?;
                client.catalog(&self.code.repository_id)
            })();
            match catalog {
                Ok(catalog) => {
                    for (id, entry) in catalog.catalog.mappings {
                        let mapping = entry.view.record.mapping;
                        let document = LoreDocument::Backend {
                            reference: mapping.document,
                        };
                        if !mapping.archived
                            && !state
                                .store_mappings
                                .get(store)
                                .is_some_and(|mappings| mappings.contains_key(&id))
                            && !excluded.contains(&document)
                        {
                            documents.push(RoutingDocument {
                                store: Some(store.clone()),
                                mapping: Some(id),
                                document,
                                title: entry.title,
                                scopes: mapping.scopes,
                                anchors: mapping.anchors,
                            });
                        }
                    }
                }
                Err(error) => warnings.push(format!("Local store {store}: {}", error.message)),
            }
        }
        for name in state.connected_remotes.values() {
            if let Some(version) = self.cached_remote_knowledge(home, name)? {
                let source = RemoteRegistry::new(home).get(name)?.store;
                for record in version.snapshot.documents.into_values() {
                    let document = LoreDocument::Local {
                        id: record.document.id,
                    };
                    if !record.document.archived && !excluded.contains(&document) {
                        documents.push(RoutingDocument {
                            store: Some(source.clone()),
                            mapping: None,
                            document,
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
                }
                for record in version.snapshot.mappings.into_values() {
                    let mapping = record.mapping;
                    let document = LoreDocument::Backend {
                        reference: mapping.document,
                    };
                    if !mapping.archived && !excluded.contains(&document) {
                        documents.push(RoutingDocument {
                            store: Some(RemoteRegistry::new(home).get(name)?.store),
                            mapping: Some(mapping.id),
                            document,
                            title: None,
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
                }
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
