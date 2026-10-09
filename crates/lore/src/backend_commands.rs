use clap::Subcommand;
use lore_core::{DocumentReference, Error, ErrorKind, Result};
use lore_local::{BackendConfig, BackendRegistry};
use lore_notion::{NotionBackend, page_id};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Subcommand)]
pub enum BackendOperation {
    AddNotion {
        name: String,
        #[arg(long, required = true)]
        root: Vec<String>,
        #[arg(long, default_value = "NOTION_TOKEN")]
        token_env: String,
        #[arg(long, default_value = "https://api.notion.com/v1")]
        api_url: String,
    },
    List,
    Remove {
        name: String,
    },
}
#[derive(Subcommand)]
pub enum DocsOperation {
    /// Read document metadata independently of its body.
    Describe {
        backend: String,
        page: String,
        #[arg(long)]
        cached: bool,
        #[arg(long, conflicts_with = "cached")]
        refresh: bool,
    },
    /// List configured roots, or documents directly contained in a page.
    Browse {
        backend: String,
        page: Option<String>,
        #[arg(long)]
        cached: bool,
        #[arg(long, conflicts_with = "cached")]
        refresh: bool,
    },
    /// Read a cached observation, retrieving it on a cache miss. Refresh is explicit.
    Show {
        backend: String,
        page: String,
        #[arg(long)]
        cached: bool,
        #[arg(long, conflicts_with = "cached")]
        refresh: bool,
        #[arg(long, conflicts_with = "json")]
        raw: bool,
    },
    /// Traverse configured roots to build a title index; does not retrieve document bodies.
    Index { backend: String },
    /// Refresh a document, or the tree and title index under configured roots.
    Fetch {
        backend: String,
        page: Option<String>,
        #[arg(long, conflicts_with = "page")]
        with_content: bool,
    },
    /// Search cached titles within the configured roots. Run docs index first.
    Search {
        backend: String,
        #[arg(default_value = "")]
        query: String,
    },
}
pub fn configure(home: &Path, command: &BackendOperation) -> Result<Value> {
    let registry = BackendRegistry::new(home);
    match command {
        BackendOperation::List => Ok(serde_json::to_value(registry.list()?)?),
        BackendOperation::Remove { name } => {
            registry.remove(name)?;
            Ok(json!({"removed":name,"mappings_preserved":true}))
        }
        BackendOperation::AddNotion {
            name,
            root,
            token_env,
            api_url,
        } => {
            if token_env.is_empty()
                || !token_env
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(Error::invalid("Token environment variable name is invalid"));
            }
            let _ = NotionBackend::new(name.clone(), "configuration-validation".into(), api_url)?;
            let mut roots = root
                .iter()
                .map(|id| page_id(id))
                .collect::<Result<Vec<_>>>()?;
            roots.sort();
            roots.dedup();
            registry.add(
                name,
                BackendConfig::Notion {
                    roots,
                    token_env: token_env.clone(),
                    api_url: api_url.clone(),
                },
            )?;
            Ok(json!({"backend":name,"configured":true,"credentials_stored":false}))
        }
    }
}
fn configured(
    registry: &BackendRegistry,
    name: &str,
    cached: bool,
) -> Result<(Vec<DocumentReference>, Option<NotionBackend>)> {
    let BackendConfig::Notion {
        roots,
        token_env,
        api_url,
    } = registry.get(name)?;
    let roots = roots
        .into_iter()
        .map(|id| DocumentReference {
            backend: name.into(),
            id,
        })
        .collect();
    let backend = if cached {
        None
    } else {
        let token = std::env::var(&token_env).map_err(|_| {
            Error::new(
                ErrorKind::PermissionDenied,
                format!("Set {token_env} for this Notion connection"),
            )
        })?;
        Some(NotionBackend::new(name.into(), token, &api_url)?)
    };
    Ok((roots, backend))
}
pub fn reference(home: &Path, backend: &str, page: &str) -> Result<DocumentReference> {
    BackendRegistry::new(home).get(backend)?;
    Ok(DocumentReference {
        backend: backend.into(),
        id: page_id(page)?,
    })
}
fn prefer_cache<T>(
    cached: bool,
    refresh: bool,
    read_cached: impl FnOnce() -> Result<T>,
    retrieve: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if !refresh {
        match read_cached() {
            Ok(value) => return Ok(value),
            Err(error) if cached || error.kind != ErrorKind::NotCached => return Err(error),
            Err(_) => {}
        }
    }
    retrieve()
}
pub fn documents(home: &Path, command: &DocsOperation) -> Result<Value> {
    let registry = BackendRegistry::new(home);
    let name = match command {
        DocsOperation::Describe { backend, .. }
        | DocsOperation::Browse { backend, .. }
        | DocsOperation::Show { backend, .. }
        | DocsOperation::Index { backend }
        | DocsOperation::Fetch { backend, .. }
        | DocsOperation::Search { backend, .. } => backend,
    };
    let (roots, _) = configured(&registry, name, true)?;
    let cache = registry.cache(name);
    let backend = || configured(&registry, name, false).map(|(_, backend)| backend.unwrap());
    match command {
        DocsOperation::Describe {
            page,
            cached,
            refresh,
            ..
        } => {
            let reference = reference(home, name, page)?;
            Ok(serde_json::to_value(prefer_cache(
                *cached,
                *refresh,
                || cache.metadata(None, &reference),
                || cache.metadata(Some(&backend()?), &reference),
            )?)?)
        }
        DocsOperation::Browse {
            page,
            cached,
            refresh,
            ..
        } => match page {
            Some(page) => {
                let reference = reference(home, name, page)?;
                Ok(serde_json::to_value(prefer_cache(
                    *cached,
                    *refresh,
                    || cache.children(None, &reference),
                    || cache.children(Some(&backend()?), &reference),
                )?)?)
            }
            None => Ok(serde_json::to_value(prefer_cache(
                *cached,
                *refresh,
                || cache.roots(None, &roots),
                || cache.roots(Some(&backend()?), &roots),
            )?)?),
        },
        DocsOperation::Show {
            page,
            cached,
            refresh,
            ..
        } => {
            let reference = reference(home, name, page)?;
            Ok(serde_json::to_value(prefer_cache(
                *cached,
                *refresh,
                || cache.read(None, &reference),
                || cache.read(Some(&backend()?), &reference),
            )?)?)
        }
        DocsOperation::Index { .. } => Ok(serde_json::to_value(cache.index(&backend()?, &roots)?)?),
        DocsOperation::Search { query, .. } => {
            Ok(serde_json::to_value(cache.search(query, &roots)?)?)
        }
        DocsOperation::Fetch {
            page, with_content, ..
        } => {
            let backend = backend()?;
            if let Some(page) = page {
                let reference = reference(home, name, page)?;
                let content = cache.read(Some(&backend), &reference)?;
                Ok(serde_json::to_value(content)?)
            } else {
                let index = cache.index(&backend, &roots)?;
                let mut results = vec![];
                if *with_content {
                    for document in &index.value.documents {
                        results.push(match cache.read(Some(&backend), &document.reference) {
                            Ok(content) => json!({"status":"fetched","document":document.reference,"partial":content.value.partial,"warnings":content.value.warnings}),
                            Err(error) => json!({"status":"failed","document":document.reference,"error":{"kind":error.kind,"message":error.message}}),
                        });
                    }
                }
                let mut value = serde_json::to_value(index)?;
                value["results"] = json!(results);
                Ok(value)
            }
        }
    }
}

pub fn print(value: &Value, command: &DocsOperation) {
    match command {
        DocsOperation::Describe { .. } => println!(
            "{}  {}",
            value["value"]["reference"]["id"].as_str().unwrap(),
            value["value"]["title"].as_str().unwrap()
        ),
        DocsOperation::Show { raw, .. } => {
            if !raw {
                println!(
                    "{}\n{}\n",
                    value["value"]["document"]["title"]
                        .as_str()
                        .unwrap_or_default(),
                    value["value"]["document"]["url"]
                        .as_str()
                        .unwrap_or_default()
                );
            }
            println!("{}", value["value"]["content"].as_str().unwrap_or_default());
        }
        DocsOperation::Fetch { page: Some(_), .. } => println!(
            "Fetched {}",
            value["value"]["document"]["title"].as_str().unwrap()
        ),
        _ => {
            for doc in value["value"]["documents"].as_array().unwrap() {
                println!(
                    "{}  {}",
                    doc["reference"]["id"].as_str().unwrap(),
                    doc["title"].as_str().unwrap()
                );
            }
        }
    }
    if value["cached"] == true {
        eprintln!(
            "Cached observation from {} ms since Unix epoch",
            value["fetched_at_ms"]
        );
    }
    if value["value"]["partial"] == true {
        eprintln!("Partial content; open the original in Notion");
    }
    if let Some(warnings) = value["value"]["warnings"].as_array() {
        for warning in warnings {
            eprintln!("Warning: {}", warning.as_str().unwrap_or_default());
        }
    }
}
