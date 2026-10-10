use lore_core::{Error, MappingId, MappingRemote, Result};
use lore_local::{HttpMappingRemote, RemoteCatalogCache, RemoteRegistry, Workspace};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn client(home: &Path, name: &str) -> Result<HttpMappingRemote> {
    HttpMappingRemote::new(home, name, RemoteRegistry::new(home).get(name)?)
}

pub fn push(
    workspace: &Workspace,
    home: &Path,
    name: &str,
    id: &MappingId,
    from: Option<&str>,
) -> Result<Value> {
    let remote = client(home, name)?;
    let from = from
        .map(|name| {
            RemoteRegistry::new(home)
                .get(name)
                .map(|remote| remote.store)
        })
        .transpose()?;
    workspace.connect_remote(&remote.connection.store, name)?;
    Ok(serde_json::to_value(workspace.push_mapping(
        &remote,
        &remote.connection.store,
        id,
        from.as_ref(),
    )?)?)
}

pub fn pull(
    workspace: &Workspace,
    home: &Path,
    name: &str,
    id: Option<&MappingId>,
    path: Option<&PathBuf>,
    backend: Option<&str>,
) -> Result<Value> {
    let remote = client(home, name)?;
    let store = &remote.connection.store;
    workspace.connect_remote(store, name)?;
    let observation =
        RemoteCatalogCache::new(home).fetch(&remote, &workspace.code.repository_id)?;
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
            let bundle = workspace.fetch_mapping(&remote, store, &id)?;
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
    name: &str,
    id: Option<&MappingId>,
) -> Result<Value> {
    let remote = client(home, name)?;
    if let Some(id) = id {
        return Ok(serde_json::to_value(workspace.fetch_mapping(
            &remote,
            &remote.connection.store,
            id,
        )?)?);
    }
    Ok(serde_json::to_value(
        RemoteCatalogCache::new(home).fetch(&remote, &workspace.code.repository_id)?,
    )?)
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
