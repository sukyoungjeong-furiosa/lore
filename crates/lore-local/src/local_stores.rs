use crate::{GitRepository, absolute, atomic_write};
use lore_core::*;
use lore_store::{FileSharedStore, SharedStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// A filesystem location and the persistent identity expected at that location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalStoreConnection {
    pub store: KnowledgeStoreId,
    pub directory: PathBuf,
}

fn validate_directory(directory: &Path) -> Result<()> {
    for ancestor in directory.ancestors().filter(|path| path.is_dir()) {
        if let Ok(code) = GitRepository::discover(ancestor, None) {
            if directory.starts_with(&code.root) || directory.starts_with(&code.common_directory) {
                return Err(Error::invalid(
                    "Local stores must live outside code repositories and Git directories",
                ));
            }
            break;
        }
    }
    Ok(())
}

impl LocalStoreConnection {
    fn create(directory: &Path, id: KnowledgeStoreId) -> Result<Self> {
        let directory = absolute(directory)?;
        validate_directory(&directory)?;
        std::fs::create_dir_all(&directory)?;
        let lock = descriptor_lock(&directory)?;
        lock.lock()?;
        let path = directory.join("store.json");
        if path.exists() {
            return Err(Error::new(
                ErrorKind::Conflict,
                "A store already exists here; use lore store add to connect it",
            ));
        }
        let descriptor = KnowledgeStoreDescriptor {
            id,
            integration_policy: Default::default(),
            repositories: BTreeMap::new(),
            backends: Default::default(),
        };
        atomic_write(&path, &serde_json::to_vec_pretty(&descriptor)?)?;
        Ok(Self {
            store: descriptor.id,
            directory,
        })
    }

    pub fn open(directory: &Path) -> Result<Self> {
        let directory = absolute(directory)?;
        validate_directory(&directory)?;
        let descriptor = read_descriptor(&directory)?;
        Ok(Self {
            store: descriptor.id,
            directory,
        })
    }

    pub fn descriptor(&self) -> Result<KnowledgeStoreDescriptor> {
        let descriptor = read_descriptor(&self.directory)?;
        if descriptor.id != self.store {
            return Err(Error::new(
                ErrorKind::Conflict,
                "The directory now contains a different mapping store",
            ));
        }
        Ok(descriptor)
    }

    /// Registers resources explicitly selected by the local owner. Existing canonical refs are retained.
    pub fn register(
        &self,
        repositories: &[String],
        backends: &[String],
    ) -> Result<KnowledgeStoreDescriptor> {
        let lock = descriptor_lock(&self.directory)?;
        lock.lock()?;
        let mut descriptor = self.descriptor()?;
        let before = (descriptor.repositories.len(), descriptor.backends.len());
        for repository in repositories {
            descriptor
                .repositories
                .entry(repository.clone())
                .or_default();
        }
        descriptor.backends.extend(backends.iter().cloned());
        descriptor.validate()?;
        if before != (descriptor.repositories.len(), descriptor.backends.len()) {
            atomic_write(
                &self.directory.join("store.json"),
                &serde_json::to_vec_pretty(&descriptor)?,
            )?;
        }
        Ok(descriptor)
    }
}

fn descriptor_lock(directory: &Path) -> Result<std::fs::File> {
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("descriptor.lock"))?)
}

fn read_descriptor(directory: &Path) -> Result<KnowledgeStoreDescriptor> {
    let bytes = std::fs::read(directory.join("store.json")).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::new(ErrorKind::NotFound, "Local mapping store is unavailable")
        } else {
            error.into()
        }
    })?;
    let descriptor: KnowledgeStoreDescriptor = serde_json::from_slice(&bytes)?;
    descriptor.validate()?;
    Ok(descriptor)
}

pub struct LocalStoreRegistry {
    home: PathBuf,
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 100
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::invalid(
            "Store names require ASCII letters, digits, hyphens, or underscores",
        ));
    }
    Ok(())
}

impl LocalStoreRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            home: home.to_owned(),
        }
    }

    pub fn list(&self) -> Result<BTreeMap<String, LocalStoreConnection>> {
        let connections: BTreeMap<String, LocalStoreConnection> =
            match std::fs::read(self.home.join("local-stores/config.json")) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(BTreeMap::new());
                }
                Err(error) => return Err(error.into()),
            };
        for (name, connection) in &connections {
            validate_name(name)?;
            if !connection.directory.is_absolute() {
                return Err(Error::invalid("Local store locations must be absolute"));
            }
        }
        Ok(connections)
    }

    pub fn get(&self, name: &str) -> Result<LocalStoreConnection> {
        self.list()?
            .remove(name)
            .ok_or_else(|| Error::new(ErrorKind::NotFound, "Local store is not configured"))
    }

    fn update<T>(
        &self,
        change: impl FnOnce(&mut BTreeMap<String, LocalStoreConnection>) -> Result<T>,
    ) -> Result<T> {
        let directory = self.home.join("local-stores");
        std::fs::create_dir_all(&directory)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("config.lock"))?;
        lock.lock()?;
        let mut connections = self.list()?;
        let before = connections.clone();
        let result = change(&mut connections)?;
        if connections != before {
            atomic_write(
                &directory.join("config.json"),
                &serde_json::to_vec_pretty(&connections)?,
            )?;
        }
        Ok(result)
    }

    fn create_connection(&self, path: Option<&Path>) -> Result<LocalStoreConnection> {
        let id = KnowledgeStoreId::new();
        let generated = self.home.join("stores").join(id.as_str());
        LocalStoreConnection::create(path.unwrap_or(&generated), id)
    }

    pub fn create(&self, name: &str, path: Option<&Path>) -> Result<LocalStoreConnection> {
        validate_name(name)?;
        self.update(|connections| {
            if connections.contains_key(name) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Local store name is already configured",
                ));
            }
            let connection = self.create_connection(path)?;
            connections.insert(name.into(), connection.clone());
            Ok(connection)
        })
    }

    /// A missing existing store is an error, rather than a reason to replace its identity.
    pub fn ensure_personal(&self) -> Result<LocalStoreConnection> {
        self.update(|connections| {
            if let Some(connection) = connections.get("personal") {
                connection.descriptor()?;
                return Ok(connection.clone());
            }
            let connection = self.create_connection(None)?;
            connections.insert("personal".into(), connection.clone());
            Ok(connection)
        })
    }

    pub fn add(&self, name: &str, directory: &Path) -> Result<LocalStoreConnection> {
        validate_name(name)?;
        let connection = LocalStoreConnection::open(directory)?;
        self.update(|connections| {
            if connections.contains_key(name) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Local store name is already configured",
                ));
            }
            connections.insert(name.into(), connection.clone());
            Ok(connection.clone())
        })
    }
}

/// Direct store access uses filesystem permissions, without HTTP or model inference.
pub struct LocalMappingClient {
    pub connection: LocalStoreConnection,
    storage: FileSharedStore,
}

impl LocalMappingClient {
    pub fn new(connection: LocalStoreConnection) -> Result<Self> {
        connection.descriptor()?;
        Ok(Self {
            storage: FileSharedStore {
                directory: connection.directory.join("data"),
            },
            connection,
        })
    }
}

impl MappingStoreClient for LocalMappingClient {
    fn descriptor(&self) -> Result<KnowledgeStoreDescriptor> {
        self.connection.descriptor()
    }

    fn catalog(&self, repository: &str) -> Result<StoreCatalog> {
        let descriptor = self.descriptor()?;
        let catalog = StoreCatalog {
            catalog: self.storage.catalog(&descriptor, repository)?,
            store: descriptor,
        };
        catalog.validate(&self.connection.store, repository)?;
        Ok(catalog)
    }

    fn read(&self, mapping: &MappingId, revision: Option<&RevisionId>) -> Result<MappingBundle> {
        let descriptor = self.descriptor()?;
        let bundle = self.storage.read(&descriptor.id, mapping, revision)?;
        for view in &bundle.revisions {
            descriptor.validate_mapping(&view.record.mapping)?;
        }
        Ok(bundle)
    }

    fn publish(&self, request: &PublishMapping) -> Result<PublicationReceipt> {
        request.validate()?;
        if request.bundle.store != self.connection.store {
            return Err(Error::invalid(
                "Publication targets a different local store",
            ));
        }
        let mut repositories = vec![];
        let mut backends = vec![];
        for view in &request.bundle.revisions {
            repositories.push(view.record.mapping.code_repository()?.to_owned());
            backends.push(view.record.mapping.document.backend.clone());
        }
        // Publishing locally registers the resources in the owner's explicit proposal.
        let descriptor = self.connection.register(&repositories, &backends)?;
        self.storage.publish(&descriptor, "local", request)
    }
}
