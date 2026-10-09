use clap::Subcommand;
use lore_core::{Actor, AnchorResolver, Error, MappingId, Result, RevisionId, TextResolver};
use lore_local::{LocatedMapping, Workspace};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum MapOperation {
    Create {
        backend: String,
        document: String,
        location: String,
        #[arg(long, default_value = "")]
        context: String,
    },
    Ls {
        path: Option<PathBuf>,
        #[arg(long)]
        recursive: bool,
        #[arg(long)]
        include_archived: bool,
        #[arg(long, requires = "document")]
        backend: Option<String>,
        #[arg(long, requires = "backend")]
        document: Option<String>,
    },
    Show {
        id: MappingId,
        #[arg(long)]
        revision: Option<RevisionId>,
    },
    Edit {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
        #[arg(long)]
        context: String,
    },
    Attach {
        id: MappingId,
        location: String,
        #[arg(long)]
        if_match: RevisionId,
    },
    Archive {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
    },
    Restore {
        id: MappingId,
        #[arg(long)]
        if_match: RevisionId,
    },
    History {
        id: MappingId,
    },
    Resolve {
        id: MappingId,
    },
    Install {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        path: PathBuf,
    },
}
fn capture(workspace: &Workspace, location: &str) -> Result<lore_core::Anchor> {
    let (path, range) = location
        .rsplit_once(':')
        .ok_or_else(|| Error::invalid("Use path:line or path:start-end"))?;
    let (start, end) = range.split_once('-').unwrap_or((range, range));
    let start = start
        .parse()
        .map_err(|_| Error::invalid("Invalid first line"))?;
    let end = end
        .parse()
        .map_err(|_| Error::invalid("Invalid last line"))?;
    let path = workspace.code.relative(Path::new(path))?;
    workspace.code.capture(&path, start, end)
}
pub fn print(value: &Value, command: &MapOperation) {
    match command {
        MapOperation::Ls { .. } => {
            for item in value["mappings"].as_array().unwrap() {
                print_view(&item["view"]);
            }
        }
        MapOperation::History { .. } => {
            for item in value.as_array().unwrap() {
                print_view(item);
            }
        }
        MapOperation::Resolve { .. } => {
            print_view(&value["view"]);
            for projection in value["projections"].as_array().unwrap() {
                println!("{}", projection["explanation"].as_str().unwrap());
                for location in projection["locations"].as_array().unwrap() {
                    println!(
                        "  {}:{}-{}",
                        location["path"].as_str().unwrap(),
                        location["start"],
                        location["end"]
                    );
                }
            }
        }
        _ if value.get("record").is_some() => print_view(value),
        _ => println!("{}", serde_json::to_string_pretty(value).unwrap()),
    }
}
fn print_view(view: &Value) {
    let mapping = &view["record"]["mapping"];
    println!(
        "{}  {}:{}{}\nRevision: {}\n{}",
        mapping["id"].as_str().unwrap(),
        mapping["document"]["backend"].as_str().unwrap(),
        mapping["document"]["id"].as_str().unwrap(),
        if mapping["archived"] == true {
            " [archived]"
        } else {
            ""
        },
        view["revision"].as_str().unwrap(),
        mapping["context"].as_str().unwrap()
    );
    for anchor in mapping["anchors"].as_array().unwrap() {
        println!(
            "  Recorded: {}:{}-{}",
            anchor["path"].as_str().unwrap(),
            anchor["start"],
            anchor["end"]
        );
    }
}
pub fn execute(
    workspace: &Workspace,
    home: &Path,
    actor: Actor,
    command: &MapOperation,
) -> Result<Value> {
    match command {
        MapOperation::Create {
            backend,
            document,
            location,
            context,
        } => {
            let reference = super::backend_commands::reference(home, backend, document)?;
            Ok(serde_json::to_value(workspace.create_mapping(
                reference,
                capture(workspace, location)?,
                context.clone(),
                actor,
            )?)?)
        }
        MapOperation::Ls {
            path,
            recursive,
            include_archived,
            backend,
            document,
        } => {
            let mut mappings = if let Some(path) = path {
                workspace.locate_mappings(
                    Some(&super::scope(workspace, path)?),
                    *recursive || workspace.code.root.join(path).is_dir(),
                    *include_archived,
                    &TextResolver,
                )?
            } else {
                workspace
                    .list_mappings(*include_archived)?
                    .into_iter()
                    .map(|view| LocatedMapping {
                        view: view.into(),
                        projections: vec![],
                    })
                    .collect()
            };
            if let (Some(backend), Some(document)) = (backend, document) {
                let reference = super::backend_commands::reference(home, backend, document)?;
                mappings.retain(|item| item.view.record.mapping.document == reference);
            }
            Ok(
                json!({"mappings":mappings,"coverage":"active workspace mappings; original references and exact-text projections"}),
            )
        }
        MapOperation::Show { id, revision } => Ok(serde_json::to_value(
            workspace.read_mapping(id, revision.as_ref())?,
        )?),
        MapOperation::Edit {
            id,
            if_match,
            context,
        } => Ok(serde_json::to_value(workspace.update_mapping(
            id,
            if_match,
            actor,
            |mapping| {
                mapping.context = context.clone();
                Ok(())
            },
        )?)?),
        MapOperation::Attach {
            id,
            location,
            if_match,
        } => {
            let anchor = capture(workspace, location)?;
            Ok(serde_json::to_value(workspace.update_mapping(
                id,
                if_match,
                actor,
                |mapping| {
                    mapping.anchors.push(anchor);
                    Ok(())
                },
            )?)?)
        }
        MapOperation::Archive { id, if_match } | MapOperation::Restore { id, if_match } => {
            let archived = matches!(command, MapOperation::Archive { .. });
            Ok(serde_json::to_value(workspace.update_mapping(
                id,
                if_match,
                actor,
                |mapping| {
                    mapping.archived = archived;
                    Ok(())
                },
            )?)?)
        }
        MapOperation::History { id } => Ok(serde_json::to_value(workspace.mapping_history(id)?)?),
        MapOperation::Resolve { id } => {
            let view = workspace.read_mapping(id, None)?;
            let projections = view
                .record
                .mapping
                .anchors
                .iter()
                .map(|anchor| TextResolver.resolve(anchor, &workspace.code, None))
                .collect::<Result<Vec<_>>>()?;
            Ok(json!({"view":lore_core::MappingSummaryView::from(view),"projections":projections}))
        }
        MapOperation::Install { from, path } => {
            let source = Workspace::open(home, from)?;
            let selected = source.list_mappings(false)?;
            let requested = super::scope(workspace, path)?;
            let recursive = workspace.code.root.join(path).is_dir();
            let in_scope = |p: &str| {
                requested.is_empty()
                    || p == requested
                    || recursive && p.starts_with(&format!("{requested}/"))
            };
            let mut results = vec![];
            for view in selected {
                let mut relevant = false;
                for anchor in &view.record.mapping.anchors {
                    let projection =
                        TextResolver.resolve(anchor, &workspace.code, Some(&requested))?;
                    relevant |= anchor.context.repository == workspace.code.repository_id
                        && in_scope(&anchor.path)
                        || projection.possible_locations().any(|l| in_scope(&l.path));
                }
                if relevant {
                    results.push(match workspace.install_mapping(&source, &view.record.mapping.id) {
                        Ok(view) => json!({"status":"installed","view":view}),
                        Err(error) => json!({"status":"failed","id":view.record.mapping.id,"error":{"kind":error.kind,"message":error.message}}),
                    });
                }
            }
            Ok(json!({"results":results}))
        }
    }
}
