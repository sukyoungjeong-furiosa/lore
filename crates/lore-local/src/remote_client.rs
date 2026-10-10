use crate::{CredentialStore, LoreRemote, atomic_write, now_ms};
use lore_core::*;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct HttpMappingRemote {
    home: PathBuf,
    name: String,
    pub connection: LoreRemote,
}

impl HttpMappingRemote {
    pub fn new(home: &Path, name: &str, connection: LoreRemote) -> Result<Self> {
        connection.validate()?;
        Ok(Self {
            home: home.to_owned(),
            name: name.into(),
            connection,
        })
    }

    pub fn token(&self) -> Result<String> {
        let name = self
            .connection
            .token_env
            .as_deref()
            .unwrap_or("LORE_REMOTE_TOKEN");
        if let Ok(token) = std::env::var(name) {
            if token.trim().is_empty() {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    "Lore remote token is empty",
                ));
            }
            return Ok(token);
        }
        CredentialStore::new(&self.home)
            .get_connection("remote", &self.name, &self.connection.server_url)?
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::PermissionDenied,
                    format!(
                        "Set {name} or run lore remote auth set {} --stdin",
                        self.name
                    ),
                )
            })
    }

    fn request(&self, request: StoreRequest) -> Result<StoreResponse> {
        self.send_request("sync", &request)
    }

    pub(crate) fn request_knowledge(
        &self,
        request: &KnowledgeRequest,
    ) -> Result<KnowledgeResponse> {
        self.send_request("knowledge", request)
    }

    fn send_request<T: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        operation: &str,
        request: &T,
    ) -> Result<R> {
        let bytes = serde_json::to_vec(request)?;
        if bytes.len() > MAX_SYNC_BYTES {
            return Err(Error::invalid("Synchronization request exceeds 16 MiB"));
        }
        let mut url = reqwest::Url::parse(&self.connection.server_url)
            .map_err(|_| Error::invalid("Invalid Lore server URL"))?;
        url.path_segments_mut()
            .map_err(|_| Error::invalid("Invalid Lore server URL"))?
            .pop_if_empty()
            .push("v1")
            .push(operation);
        let unavailable = || {
            Error::new(
                ErrorKind::Unavailable,
                "Lore server request failed; publication may be uncertain and can be retried",
            )
        };
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable())?
            .post(url)
            .bearer_auth(self.token()?)
            .header("content-type", "application/json")
            .body(bytes)
            .send()
            .map_err(|_| unavailable())?;
        let status = response.status();
        let mut bytes = vec![];
        response
            .take(MAX_SYNC_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() > MAX_SYNC_BYTES {
            return Err(Error::invalid("Synchronization response exceeds 16 MiB"));
        }
        if !status.is_success() {
            #[derive(Deserialize)]
            struct Failure {
                error: Error,
            }
            let kind = match status.as_u16() {
                401 | 403 => ErrorKind::PermissionDenied,
                404 => ErrorKind::NotFound,
                409 => ErrorKind::Conflict,
                400 | 413 | 422 => ErrorKind::InvalidInput,
                _ => ErrorKind::Unavailable,
            };
            if let Ok(failure) = serde_json::from_slice::<Failure>(&bytes) {
                // Status controls the error category; untrusted services cannot claim a definite conflict after a transport failure.
                return Err(Error::new(kind, failure.error.message));
            }
            return Err(Error::new(
                kind,
                "Lore server rejected the synchronization request",
            ));
        }
        serde_json::from_slice(&bytes).map_err(|_| unavailable())
    }
}

impl MappingStoreClient for HttpMappingRemote {
    fn descriptor(&self) -> Result<KnowledgeStoreDescriptor> {
        match self.request(StoreRequest::Describe {
            store: self.connection.store.clone(),
        })? {
            StoreResponse::Descriptor { descriptor } if descriptor.id == self.connection.store => {
                descriptor.validate()?;
                Ok(descriptor)
            }
            _ => Err(Error::invalid(
                "Lore server returned a different store or operation result",
            )),
        }
    }

    fn catalog(&self, repository: &str) -> Result<StoreCatalog> {
        match self.request(StoreRequest::Catalog {
            store: self.connection.store.clone(),
            repository: repository.into(),
        })? {
            StoreResponse::Catalog { catalog } => {
                catalog.validate(&self.connection.store, repository)?;
                Ok(catalog)
            }
            _ => Err(Error::invalid(
                "Lore server returned a different operation result",
            )),
        }
    }

    fn read(&self, mapping: &MappingId, revision: Option<&RevisionId>) -> Result<MappingBundle> {
        match self.request(StoreRequest::Read {
            store: self.connection.store.clone(),
            mapping: mapping.clone(),
            revision: revision.cloned(),
        })? {
            StoreResponse::Mapping { bundle } if bundle.store == self.connection.store => {
                bundle.validate()?;
                let head = bundle.head()?;
                if head.record.mapping.id != *mapping
                    || revision.is_some_and(|revision| *revision != head.revision)
                {
                    return Err(Error::invalid(
                        "Shared mapping identity does not match the request",
                    ));
                }
                Ok(bundle)
            }
            _ => Err(Error::invalid(
                "Lore server returned a different store or operation result",
            )),
        }
    }

    fn publish(&self, request: &PublishMapping) -> Result<PublicationReceipt> {
        request.validate()?;
        if request.bundle.store != self.connection.store {
            return Err(Error::invalid("Publication targets a different remote"));
        }
        match self.request(StoreRequest::Publish {
            request: request.clone(),
        })? {
            StoreResponse::Published { receipt }
                if receipt.store == self.connection.store
                    && receipt.operation == request.operation
                    && receipt.mapping == request.bundle.head()?.record.mapping.id
                    && receipt.revision == request.bundle.head()?.revision =>
            {
                Ok(receipt)
            }
            _ => Err(Error::invalid(
                "Lore server publication receipt does not match the request",
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteCatalogObservation {
    pub fetched_at_ms: u64,
    pub catalog: StoreCatalog,
}

pub struct RemoteCatalogCache {
    directory: PathBuf,
}

impl RemoteCatalogCache {
    pub fn new(home: &Path) -> Self {
        Self {
            directory: home.join("remote-catalogs"),
        }
    }

    fn path(&self, store: &KnowledgeStoreId, repository: &str) -> PathBuf {
        self.directory
            .join(hash(store.as_str()))
            .join(format!("{}.json", hash(repository)))
    }

    pub fn read(
        &self,
        store: &KnowledgeStoreId,
        repository: &str,
    ) -> Result<RemoteCatalogObservation> {
        let bytes = std::fs::read(self.path(store, repository)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    ErrorKind::NotCached,
                    "Lore remote catalog is not cached; fetch or pull it first",
                )
            } else {
                error.into()
            }
        })?;
        let observation: RemoteCatalogObservation = serde_json::from_slice(&bytes)?;
        observation.catalog.validate(store, repository)?;
        Ok(observation)
    }

    pub fn fetch(
        &self,
        remote: &dyn MappingStoreClient,
        repository: &str,
    ) -> Result<RemoteCatalogObservation> {
        let catalog = remote.catalog(repository)?;
        let observation = RemoteCatalogObservation {
            fetched_at_ms: now_ms(),
            catalog,
        };
        atomic_write(
            &self.path(&observation.catalog.store.id, repository),
            &serde_json::to_vec(&observation)?,
        )?;
        Ok(observation)
    }
}
