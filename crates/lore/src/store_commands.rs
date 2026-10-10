use clap::Subcommand;
use lore_core::Result;
use lore_local::LocalStoreRegistry;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum StoreOperation {
    /// Create an independent local mapping store outside code repositories.
    Create {
        name: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Connect an existing store, including one moved to a new directory.
    Add {
        name: String,
        path: PathBuf,
    },
    Ls,
    Show {
        name: String,
    },
    Register {
        name: String,
        #[arg(long)]
        repository: Vec<String>,
        #[arg(long)]
        backend: Vec<String>,
    },
}

pub fn execute(home: &Path, command: &StoreOperation) -> Result<Value> {
    let registry = LocalStoreRegistry::new(home);
    match command {
        StoreOperation::Create { name, path } => Ok(serde_json::to_value(
            registry.create(name, path.as_deref())?,
        )?),
        StoreOperation::Add { name, path } => Ok(serde_json::to_value(registry.add(name, path)?)?),
        StoreOperation::Ls => Ok(json!({ "stores": registry.list()? })),
        StoreOperation::Show { name } => {
            let connection = registry.get(name)?;
            Ok(json!({"connection": connection, "descriptor": connection.descriptor()?}))
        }
        StoreOperation::Register {
            name,
            repository,
            backend,
        } => Ok(serde_json::to_value(
            registry.get(name)?.register(repository, backend)?,
        )?),
    }
}
