use lore_core::{Error, ErrorKind, MappingStoreClient, Result};
use lore_local::{HttpMappingRemote, RemoteRegistry};
use serde_json::Value;
use std::path::Path;

pub fn retired() -> Error {
    Error::new(
        ErrorKind::Unsupported,
        "Mapping-only synchronization is retired. Use commit <id> --mapping --if-match <revision>, then push for the local store or push --remote <name> --mapping-id <id>. Use fetch/pull [--remote <name>] for accepted knowledge; select local stores with init --local-store <name>.",
    )
}

pub fn inspect(home: &Path, name: &str) -> Result<Value> {
    let client = HttpMappingRemote::new(home, name, RemoteRegistry::new(home).get(name)?)?;
    Ok(serde_json::to_value(client.descriptor()?)?)
}
