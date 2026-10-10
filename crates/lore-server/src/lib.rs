mod storage;
pub use storage::{FileSharedStore, SharedStore};

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
    pub store: MappingStoreId,
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
    pub stores: Vec<MappingStoreDescriptor>,
    pub principals: Vec<PrincipalConfig>,
}

struct Principal {
    config: PrincipalConfig,
    token: String,
}

pub struct Server {
    stores: BTreeMap<MappingStoreId, MappingStoreDescriptor>,
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
            stores,
            principals,
            storage,
        })
    }

    fn descriptor(
        &self,
        principal: &PrincipalConfig,
        store: &MappingStoreId,
    ) -> Result<MappingStoreDescriptor> {
        let descriptor = self.stores.get(store).ok_or_else(denied)?;
        let grants: Vec<_> = principal
            .grants
            .iter()
            .filter(|grant| grant.store == *store)
            .collect();
        if grants.is_empty() {
            return Err(denied());
        }
        Ok(MappingStoreDescriptor {
            id: store.clone(),
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
        store: &MappingStoreId,
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
        store: &MappingStoreId,
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
            StoreRequest::Publish { request } => {
                request.validate()?;
                let descriptor = self.descriptor(principal, &request.bundle.store)?;
                for view in &request.bundle.revisions {
                    descriptor.validate_mapping(&view.record.mapping)?;
                    Self::mapping_access(principal, &descriptor.id, &view.record.mapping, true)?;
                }
                match self.storage.read(
                    &descriptor.id,
                    &request.bundle.head()?.record.mapping.id,
                    None,
                ) {
                    Ok(current) => Self::mapping_access(
                        principal,
                        &descriptor.id,
                        &current.head()?.record.mapping,
                        true,
                    )?,
                    Err(error) if error.kind == ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                Ok(StoreResponse::Published {
                    receipt: self.storage.publish(
                        &self.stores[&descriptor.id],
                        &principal.name,
                        &request,
                    )?,
                })
            }
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

pub fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/v1/sync", post(handle))
        .layer(DefaultBodyLimit::max(MAX_SYNC_BYTES))
        .with_state(server)
}

pub fn serve(config: &Path, directory: &Path, address: SocketAddr) -> Result<()> {
    let config: ServerConfig = serde_json::from_slice(&std::fs::read(config)?)?;
    let server = Arc::new(Server::new(
        config,
        Arc::new(FileSharedStore {
            directory: directory.to_owned(),
        }),
        |name| std::env::var(name).ok(),
    )?);
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
