use lore_core::{Error, ErrorKind, KnowledgeStoreId, MappingId, MappingStoreClient, Result};
use lore_local::{
    HttpMappingRemote, LocalMappingClient, LocalStoreConnection, LocalStoreRegistry, LoreRemote,
    RemoteCatalogCache, RemoteRegistry, Workspace,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn client(home: &Path, name: &str) -> Result<HttpMappingRemote> {
    HttpMappingRemote::new(home, name, RemoteRegistry::new(home).get(name)?)
}

enum Connection {
    Local(LocalStoreConnection),
    Http {
        name: String,
        connection: LoreRemote,
    },
}

pub struct SelectedStore {
    pub store: KnowledgeStoreId,
    pub client: Box<dyn MappingStoreClient>,
    connection: Connection,
}

impl SelectedStore {
    fn catalog_cache(&self, home: &Path) -> Result<RemoteCatalogCache> {
        match &self.connection {
            Connection::Local(_) => Ok(RemoteCatalogCache::new(home)),
            Connection::Http { name, connection } => {
                RemoteCatalogCache::for_remote(home, name, connection)
            }
        }
    }

    fn connect(&self, workspace: &Workspace) -> Result<()> {
        match &self.connection {
            Connection::Local(connection) => workspace.connect_local_store(connection),
            Connection::Http { name, connection } => {
                workspace.connect_remote(&connection.store, name)
            }
        }
    }
}

pub fn select(
    workspace: &Workspace,
    home: &Path,
    remote: Option<&str>,
    local: Option<&str>,
) -> Result<SelectedStore> {
    if remote.is_some() && local.is_some() {
        return Err(Error::invalid("Select either --remote or --local-store"));
    }
    if let Some(name) = remote {
        let connection = RemoteRegistry::new(home).get(name)?;
        return Ok(SelectedStore {
            store: connection.store.clone(),
            client: Box::new(HttpMappingRemote::new(home, name, connection.clone())?),
            connection: Connection::Http {
                name: name.into(),
                connection,
            },
        });
    }
    let connection = if let Some(name) = local {
        LocalStoreRegistry::new(home).get(name)?
    } else {
        workspace.state()?.local_store.ok_or_else(|| {
            Error::new(
                ErrorKind::NotInitialized,
                "Run lore init to select a local mapping store",
            )
        })?
    };
    Ok(SelectedStore {
        store: connection.store.clone(),
        client: Box::new(LocalMappingClient::new(connection.clone())?),
        connection: Connection::Local(connection),
    })
}

pub fn push(
    workspace: &Workspace,
    home: &Path,
    id: &MappingId,
    remote: Option<&str>,
    local: Option<&str>,
    from_remote: Option<&str>,
    from_local: Option<&str>,
) -> Result<Value> {
    let target = select(workspace, home, remote, local)?;
    let from = match (from_remote, from_local) {
        (Some(name), None) => Some(RemoteRegistry::new(home).get(name)?.store),
        (None, Some(name)) => Some(LocalStoreRegistry::new(home).get(name)?.store),
        (None, None) => None,
        _ => return Err(Error::invalid("Select one source store")),
    };
    target.connect(workspace)?;
    Ok(serde_json::to_value(workspace.push_mapping(
        target.client.as_ref(),
        &target.store,
        id,
        from.as_ref(),
    )?)?)
}

pub fn pull(
    workspace: &Workspace,
    home: &Path,
    remote: Option<&str>,
    local: Option<&str>,
    id: Option<&MappingId>,
    path: Option<&PathBuf>,
    backend: Option<&str>,
) -> Result<Value> {
    let target = select(workspace, home, remote, local)?;
    let store = &target.store;
    target.connect(workspace)?;
    let observation = target
        .catalog_cache(home)?
        .fetch(target.client.as_ref(), &workspace.code.repository_id)?;
    let requested = path.map(|path| super::scope(workspace, path)).transpose()?;
    let recursive = path.is_some_and(|path| workspace.code.root.join(path).is_dir());
    let ids: Vec<_> = match id {
        Some(id) => vec![id.clone()],
        None => observation
            .catalog
            .catalog
            .mappings
            .iter()
            .filter(|(_, entry)| {
                backend.is_none_or(|backend| entry.view.record.mapping.document.backend == backend)
            })
            .map(|(id, _)| id.clone())
            .collect(),
    };
    let mut results = vec![];
    for id in ids {
        let result: Result<Option<Value>> = (|| {
            let before = workspace.observed_store_mapping(store, &id)?;
            let bundle = workspace.fetch_mapping(target.client.as_ref(), store, &id)?;
            if backend.is_some_and(|backend| {
                bundle
                    .head()
                    .is_ok_and(|head| head.record.mapping.document.backend != backend)
            }) {
                return Err(Error::invalid(
                    "Requested mapping belongs to another backend",
                ));
            }
            if let Some(requested) = &requested
                && !workspace.mapping_matches_scope(
                    &bundle.head()?.record.mapping,
                    requested,
                    recursive,
                )?
            {
                return Ok(None);
            }
            let result = workspace.incorporate_bundle(&bundle, before.as_ref())?;
            Ok(Some(serde_json::to_value(result)?))
        })();
        match result {
            Ok(Some(result)) => results.push(result),
            Ok(None) => {},
            Err(error) => results.push(json!({"mapping":id,"status":"failed","error":{"kind":error.kind,"message":error.message}})),
        }
    }
    Ok(json!({"store": store, "results": results}))
}

pub fn fetch(
    workspace: &Workspace,
    home: &Path,
    remote: Option<&str>,
    local: Option<&str>,
    id: Option<&MappingId>,
) -> Result<Value> {
    let target = select(workspace, home, remote, local)?;
    if let Some(id) = id {
        return Ok(serde_json::to_value(workspace.fetch_mapping(
            target.client.as_ref(),
            &target.store,
            id,
        )?)?);
    }
    Ok(serde_json::to_value(target.catalog_cache(home)?.fetch(
        target.client.as_ref(),
        &workspace.code.repository_id,
    )?)?)
}

pub fn inspect(home: &Path, name: &str) -> Result<Value> {
    Ok(serde_json::to_value(client(home, name)?.descriptor()?)?)
}

pub fn scoped_workspace(workspace: Workspace, home: &Path, name: &str) -> Result<Workspace> {
    let remote = RemoteRegistry::new(home).get(name)?;
    if remote.store.as_str().is_empty() {
        return Err(Error::invalid("Remote store identity is empty"));
    }
    Ok(workspace.with_mapping_store(remote.store))
}
