pub use lore_store::{FileSharedStore, SharedStore};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use lore_core::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreGrant {
    pub store: KnowledgeStoreId,
    pub repositories: BTreeSet<String>,
    pub backends: BTreeSet<String>,
    #[serde(default)]
    pub write: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalConfig {
    pub name: String,
    pub token_env: String,
    pub grants: Vec<StoreGrant>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub stores: Vec<KnowledgeStoreDescriptor>,
    pub principals: Vec<PrincipalConfig>,
}

struct Principal {
    config: PrincipalConfig,
    token: String,
}

pub struct Server {
    knowledge_storage: Option<Arc<dyn KnowledgeStorage>>,
    stores: BTreeMap<KnowledgeStoreId, KnowledgeStoreDescriptor>,
    principals: Vec<Principal>,
    storage: Arc<dyn SharedStore>,
}

fn denied() -> Error {
    Error::new(
        ErrorKind::PermissionDenied,
        "Access to the requested mapping scope is denied",
    )
}

impl Server {
    pub fn new(
        config: ServerConfig,
        storage: Arc<dyn SharedStore>,
        token: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let mut stores = BTreeMap::new();
        for descriptor in config.stores {
            descriptor.validate()?;
            if stores.insert(descriptor.id.clone(), descriptor).is_some() {
                return Err(Error::invalid("Duplicate mapping store identity"));
            }
        }
        let mut principals = vec![];
        let mut names = BTreeSet::new();
        let mut tokens = BTreeSet::new();
        for principal in config.principals {
            if principal.name.trim().is_empty()
                || principal.name.chars().any(char::is_control)
                || !names.insert(principal.name.clone())
                || principal.token_env.is_empty()
            {
                return Err(Error::invalid("Invalid or duplicate server principal"));
            }
            for grant in &principal.grants {
                let descriptor = stores
                    .get(&grant.store)
                    .ok_or_else(|| Error::invalid("Grant references an unknown store"))?;
                if grant
                    .repositories
                    .iter()
                    .any(|repo| !descriptor.repositories.contains_key(repo))
                    || !grant.backends.is_subset(&descriptor.backends)
                {
                    return Err(Error::invalid("Grant references unregistered resources"));
                }
            }
            let token = token(&principal.token_env)
                .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::PermissionDenied,
                        format!("Set {} for the server principal", principal.token_env),
                    )
                })?;
            if !tokens.insert(token.clone()) {
                return Err(Error::invalid("Server principal tokens must be distinct"));
            }
            principals.push(Principal {
                config: principal,
                token,
            });
        }
        if principals.is_empty() {
            return Err(Error::invalid(
                "The server requires an authenticated principal",
            ));
        }
        Ok(Self {
            knowledge_storage: None,
            stores,
            principals,
            storage,
        })
    }

    pub fn with_knowledge_storage(mut self, storage: Arc<dyn KnowledgeStorage>) -> Self {
        self.knowledge_storage = Some(storage);
        self
    }

    pub fn execute_knowledge(
        &self,
        token: &str,
        request: KnowledgeRequest,
    ) -> Result<KnowledgeResponse> {
        let principal = &self
            .principals
            .iter()
            .find(|p| p.token == token)
            .ok_or_else(denied)?
            .config;
        let (store, repository) = match &request {
            KnowledgeRequest::Read {
                store, repository, ..
            }
            | KnowledgeRequest::Submit {
                store, repository, ..
            }
            | KnowledgeRequest::Proposals { store, repository }
            | KnowledgeRequest::Integrate {
                store, repository, ..
            } => (store.clone(), repository.clone()),
        };
        let descriptor = self.descriptor(principal, &store)?;
        if !descriptor.repositories.contains_key(&repository) {
            return Err(denied());
        }
        // Local Documents are a separate backend permission; mapping permissions retain backend pairing.
        let filter = |snapshot: &KnowledgeSnapshot| {
            let mut selected = snapshot.clone();
            if !Self::allows(principal, &store, &repository, "local", false) {
                selected.documents.clear();
            }
            selected.mappings.retain(|_, r| {
                Self::allows(
                    principal,
                    &store,
                    &repository,
                    &r.mapping.document.backend,
                    false,
                )
            });
            selected
        };
        let storage = self
            .knowledge_storage
            .as_ref()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Unsupported,
                    "Versioned knowledge storage is not configured",
                )
            })?
            .open(&store, &repository)?;
        match request {
            KnowledgeRequest::Read { version, .. } => {
                let mut version = storage.read_version(version.as_deref().unwrap_or("main"))?;
                version.snapshot = filter(&version.snapshot);
                Ok(KnowledgeResponse::Version { version })
            }
            KnowledgeRequest::Proposals { .. } => Ok(KnowledgeResponse::Proposals {
                proposals: storage.proposals()?,
            }),
            KnowledgeRequest::Submit {
                operation_id,
                base,
                snapshot,
                ..
            } => {
                snapshot.validate(&repository)?;
                let base_version = storage.read_version(&base)?;
                let mut combined = base_version.snapshot.clone();
                let visible_base = filter(&base_version.snapshot);
                let changed = merge_knowledge(&visible_base, &visible_base, &snapshot)?;
                if changed.documents != visible_base.documents
                    && !Self::allows(principal, &store, &repository, "local", true)
                {
                    return Err(denied());
                }
                for (id, record) in &changed.mappings {
                    if base_version.snapshot.mappings.get(id) != Some(record) {
                        Self::mapping_access(principal, &store, &record.mapping, true)?;
                        if let Some(old) = base_version.snapshot.mappings.get(id) {
                            Self::mapping_access(principal, &store, &old.mapping, true)?;
                        }
                    }
                }
                for id in visible_base
                    .mappings
                    .keys()
                    .filter(|id| !changed.mappings.contains_key(*id))
                {
                    Self::mapping_access(
                        principal,
                        &store,
                        &visible_base.mappings[id].mapping,
                        true,
                    )?;
                }
                if Self::allows(principal, &store, &repository, "local", false) {
                    combined.documents = changed.documents;
                } else if !changed.documents.is_empty() {
                    return Err(denied());
                }
                combined
                    .mappings
                    .retain(|id, _| !visible_base.mappings.contains_key(id));
                combined.mappings.extend(changed.mappings);
                combined.validate_document_references()?;
                Ok(KnowledgeResponse::Proposal {
                    proposal: storage.submit(
                        &format!("{}:{operation_id}", principal.name),
                        &base,
                        &combined,
                    )?,
                })
            }
            KnowledgeRequest::Integrate {
                proposal, if_match, ..
            } => {
                let proposed = storage
                    .proposals()?
                    .into_iter()
                    .find(|p| p.id == proposal)
                    .ok_or_else(|| Error::new(ErrorKind::NotFound, "Proposal does not exist"))?;
                let base = storage.read_version(&proposed.base)?.snapshot;
                let next = storage.read_version(&proposed.head)?.snapshot;
                if base.documents != next.documents
                    && !Self::allows(principal, &store, &repository, "local", true)
                {
                    return Err(denied());
                }
                for (id, record) in &next.mappings {
                    if base.mappings.get(id) != Some(record) {
                        Self::mapping_access(principal, &store, &record.mapping, true)?;
                        if let Some(previous) = base.mappings.get(id) {
                            Self::mapping_access(principal, &store, &previous.mapping, true)?;
                        }
                    }
                }
                for (id, record) in &base.mappings {
                    if !next.mappings.contains_key(id) {
                        Self::mapping_access(principal, &store, &record.mapping, true)?;
                    }
                }
                proposed.check_integration(descriptor.integration_policy, &if_match)?;
                let mut version = storage.integrate(&proposal, &if_match)?;
                version.snapshot = filter(&version.snapshot);
                Ok(KnowledgeResponse::Version { version })
            }
        }
    }

    fn descriptor(
        &self,
        principal: &PrincipalConfig,
        store: &KnowledgeStoreId,
    ) -> Result<KnowledgeStoreDescriptor> {
        let descriptor = self.stores.get(store).ok_or_else(denied)?;
        let grants: Vec<_> = principal
            .grants
            .iter()
            .filter(|grant| grant.store == *store)
            .collect();
        if grants.is_empty() {
            return Err(denied());
        }
        Ok(KnowledgeStoreDescriptor {
            id: store.clone(),
            integration_policy: descriptor.integration_policy,
            repositories: descriptor
                .repositories
                .iter()
                .filter(|(id, _)| grants.iter().any(|g| g.repositories.contains(*id)))
                .map(|(id, registration)| (id.clone(), registration.clone()))
                .collect(),
            backends: descriptor
                .backends
                .iter()
                .filter(|id| grants.iter().any(|g| g.backends.contains(*id)))
                .cloned()
                .collect(),
        })
    }

    fn allows(
        principal: &PrincipalConfig,
        store: &KnowledgeStoreId,
        repository: &str,
        backend: &str,
        write: bool,
    ) -> bool {
        principal.grants.iter().any(|grant| {
            grant.store == *store
                && grant.repositories.contains(repository)
                && grant.backends.contains(backend)
                && (!write || grant.write)
        })
    }

    fn mapping_access(
        principal: &PrincipalConfig,
        store: &KnowledgeStoreId,
        mapping: &Mapping,
        write: bool,
    ) -> Result<()> {
        if !Self::allows(
            principal,
            store,
            mapping.code_repository()?,
            &mapping.document.backend,
            write,
        ) {
            return Err(denied());
        }
        Ok(())
    }

    pub fn execute(&self, token: &str, request: StoreRequest) -> Result<StoreResponse> {
        let principal = &self
            .principals
            .iter()
            .find(|principal| principal.token == token)
            .ok_or_else(denied)?
            .config;
        match request {
            StoreRequest::Describe { store } => Ok(StoreResponse::Descriptor {
                descriptor: self.descriptor(principal, &store)?,
            }),
            StoreRequest::Catalog { store, repository } => {
                let descriptor = self.descriptor(principal, &store)?;
                if !descriptor.repositories.contains_key(&repository) {
                    return Err(denied());
                }
                let mut catalog = self.storage.catalog(&self.stores[&store], &repository)?;
                catalog.mappings.retain(|_, entry| {
                    Self::allows(
                        principal,
                        &store,
                        &repository,
                        &entry.view.record.mapping.document.backend,
                        false,
                    )
                });
                descriptor.validate_catalog(&catalog)?;
                Ok(StoreResponse::Catalog {
                    catalog: StoreCatalog {
                        store: descriptor,
                        catalog,
                    },
                })
            }
            StoreRequest::Read {
                store,
                mapping,
                revision,
            } => {
                self.descriptor(principal, &store)?;
                let bundle = self.storage.read(&store, &mapping, revision.as_ref())?;
                for view in &bundle.revisions {
                    Self::mapping_access(principal, &store, &view.record.mapping, false)?;
                }
                Ok(StoreResponse::Mapping { bundle })
            }
            StoreRequest::Publish { .. } => Err(Error::new(
                ErrorKind::Unsupported,
                "Mapping-only publication is retired; submit and integrate a knowledge proposal through /v1/knowledge",
            )),
        }
    }
}

struct HttpError(Error);

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = match self.0.kind {
            ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
            ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::Conflict => StatusCode::CONFLICT,
            ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let error = if status == StatusCode::INTERNAL_SERVER_ERROR {
            Error::new(ErrorKind::Unavailable, "Shared storage operation failed")
        } else {
            self.0
        };
        (status, Json(serde_json::json!({"error": error}))).into_response()
    }
}

async fn handle(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    request: std::result::Result<Json<StoreRequest>, JsonRejection>,
) -> std::result::Result<Json<StoreResponse>, HttpError> {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| HttpError(denied()))?
        .to_string();
    let request = request
        .map_err(|_| {
            HttpError(Error::invalid(
                "Invalid or oversized synchronization request",
            ))
        })?
        .0;
    tokio::task::spawn_blocking(move || server.execute(&token, request))
        .await
        .map_err(|_| {
            HttpError(Error::new(
                ErrorKind::Unavailable,
                "Shared storage operation failed",
            ))
        })?
        .map(Json)
        .map_err(HttpError)
}

async fn handle_knowledge(
    State(server): State<Arc<Server>>,
    headers: HeaderMap,
    request: std::result::Result<Json<KnowledgeRequest>, JsonRejection>,
) -> std::result::Result<Json<KnowledgeResponse>, HttpError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| HttpError(denied()))?
        .to_owned();
    let request = request
        .map_err(|_| HttpError(Error::invalid("Invalid or oversized knowledge request")))?
        .0;
    tokio::task::spawn_blocking(move || server.execute_knowledge(&token, request))
        .await
        .map_err(|_| {
            HttpError(Error::new(
                ErrorKind::Unavailable,
                "Knowledge operation failed",
            ))
        })?
        .map(Json)
        .map_err(HttpError)
}

pub fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/v1/sync", post(handle))
        .route("/v1/knowledge", post(handle_knowledge))
        .layer(DefaultBodyLimit::max(MAX_SYNC_BYTES))
        .with_state(server)
}

pub fn serve(config: &Path, directory: &Path, address: SocketAddr) -> Result<()> {
    let config: ServerConfig = serde_json::from_slice(&std::fs::read(config)?)?;
    let server = Arc::new(
        Server::new(
            config,
            Arc::new(FileSharedStore {
                directory: directory.to_owned(),
            }),
            |name| std::env::var(name).ok(),
        )?
        .with_knowledge_storage(Arc::new(lore_store::GitKnowledgeStorage {
            directory: directory.to_owned(),
        })),
    );
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let listener = tokio::net::TcpListener::bind(address).await?;
            eprintln!("Lore server listening on {}", listener.local_addr()?);
            axum::serve(listener, router(server))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await
                .map_err(Error::from)
        })
}
