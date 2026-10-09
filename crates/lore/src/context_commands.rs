use clap::Subcommand;
use lore_core::{Error, LoreDocument, Result, validate_relative_path};
use lore_local::Workspace;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Subcommand)]
pub enum ContextOperation {
    /// Discover shared and local relationships without reading bodies or projecting code.
    Discover {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Prohibits remote retrieval; unavailable shared coverage remains explicit.
        #[arg(long, conflicts_with = "refresh")]
        cached: bool,
        /// Refresh the shared mapping index before discovery.
        #[arg(long)]
        refresh: bool,
        /// JSON array of document references to omit from discovery.
        #[arg(long)]
        exclude_file: Option<PathBuf>,
    },
}

pub fn execute(workspace: &Workspace, home: &Path, command: &ContextOperation) -> Result<Value> {
    let ContextOperation::Discover {
        paths,
        cached,
        refresh,
        exclude_file,
    } = command;
    let excluded: BTreeSet<LoreDocument> = if let Some(path) = exclude_file {
        let mut bytes = vec![];
        std::fs::File::open(path)?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::invalid("Discovery exclusion file exceeds 8 MB"));
        }
        serde_json::from_slice(&bytes)?
    } else {
        BTreeSet::new()
    };
    let paths = paths
        .iter()
        .map(|path| {
            let relative = if path.is_absolute() {
                path.strip_prefix(&workspace.code.root)
                    .map_err(|_| Error::invalid("Context path is outside this checkout"))?
            } else {
                path.as_path()
            };
            let path = relative
                .to_str()
                .ok_or_else(|| Error::invalid("Context path is not UTF-8"))?
                .trim_start_matches("./");
            validate_relative_path(path)?;
            Ok(path.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::to_value(workspace.related_lore(
        &paths, home, *cached, *refresh, &excluded,
    )?)?)
}

pub fn print(value: &Value) {
    for document in value["documents"].as_array().unwrap() {
        println!("{}", serde_json::to_string_pretty(document).unwrap());
    }
    if let Some(warnings) = value["warnings"].as_array() {
        for warning in warnings {
            eprintln!("{}", warning.as_str().unwrap_or_default());
        }
    }
}
