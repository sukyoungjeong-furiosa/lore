use crate::{ActiveDocument, SharedBaseline, Workspace, atomic_write};
use lore_core::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The source revision last applied from an artifact, independent of local commit normalization.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingArtifactBaseline {
    pub store: KnowledgeStoreId,
    pub revision: RevisionId,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingImportStatus {
    Imported,
    Updated,
    Unchanged,
}

#[derive(Clone, Debug, Serialize)]
pub struct MappingImportResult {
    pub id: MappingId,
    pub revision: RevisionId,
    pub status: MappingImportStatus,
}

fn same_record(a: &MappingRevision, b: &MappingRevision) -> bool {
    a.mapping == b.mapping && a.actor == b.actor && a.at_ms == b.at_ms
}

impl Workspace {
    /// Exports selected observed mappings without publishing changes or retrieving documents.
    pub fn export_mapping_artifact(&self, ids: &[MappingId]) -> Result<MappingArtifact> {
        let state = self.state()?;
        let local = state.local_store.as_ref().ok_or_else(|| {
            Error::new(
                ErrorKind::NotInitialized,
                "Run lore init before exporting mappings",
            )
        })?;
        let mut sources = BTreeMap::<_, Vec<_>>::new();
        for item in self.list_scoped_mappings(true)? {
            sources
                .entry(item.view.record.mapping.id)
                .or_default()
                .push(item.store);
        }
        let mut mappings = BTreeMap::new();
        for id in ids {
            let candidates = sources.get(id).ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    "Mapping is not in the selected knowledge view",
                )
            })?;
            if candidates.len() > 1 {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Mapping ID exists in several stores; select --store explicitly",
                ));
            }
            let view = self.read_mapping(id, None)?;
            let store = candidates[0].clone().unwrap_or_else(|| local.store.clone());
            let revisions = self.mapping_history(id)?;
            if revisions.first().map(|r| &r.revision) != Some(&view.revision) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    "Mapping changed during export; retry the export",
                ));
            }
            mappings.insert(id.clone(), MappingBundle { store, revisions });
        }
        let artifact = MappingArtifact {
            schema_version: 1,
            repository: self.code.repository_id.clone(),
            mappings,
        };
        artifact.validate(&self.code.repository_id)?;
        Ok(artifact)
    }

    /// Validates every mapping before atomically updating working state.
    /// Updates require source ancestry and unchanged local content. Omission does not delete mappings.
    /// Import does not commit knowledge, publish it, or access document backends.
    pub fn import_mapping_artifact(
        &self,
        artifact: &MappingArtifact,
    ) -> Result<Vec<MappingImportResult>> {
        if self.central_view || self.mapping_store.is_some() {
            return Err(Error::invalid(
                "Import mappings on an unscoped working branch",
            ));
        }
        artifact.validate(&self.code.repository_id)?;
        let _lock = self.lock()?;
        let mut state = self.state()?;
        let committed = self.committed_knowledge_mappings(&state)?;
        let source = self.document_source(&state)?;
        let mut results = vec![];
        for (id, bundle) in &artifact.mappings {
            if state.store_mappings.values().any(|m| m.contains_key(id)) {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    format!("Mapping {id} already exists in another connected store"),
                ));
            }
            let incoming = bundle.head()?;
            let active = state.mappings.get(id);
            let current = match active {
                Some(active) => Some(self.mapping_repository.load(id, &active.revision)?),
                None => committed.get(id).cloned(),
            };
            let baseline = state
                .mapping_artifact_baselines
                .get(&source)
                .and_then(|m| m.get(id));
            if let Some(baseline) = baseline
                && (baseline.store != bundle.store
                    || !bundle
                        .revisions
                        .iter()
                        .any(|r| r.revision == baseline.revision))
            {
                return Err(Error::new(
                    ErrorKind::Conflict,
                    format!(
                        "Mapping {id} has a different artifact source or divergent source history"
                    ),
                ));
            }
            let unchanged = current
                .as_ref()
                .is_some_and(|r| same_record(r, &incoming.record));
            if !unchanged && let Some(current) = &current {
                let previous = baseline.ok_or_else(|| {
                    Error::new(
                        ErrorKind::Conflict,
                        format!(
                            "Mapping {id} already has local content; import cannot overwrite it"
                        ),
                    )
                })?;
                let previous = self.mapping_repository.load(id, &previous.revision)?;
                if !same_record(current, &previous) {
                    return Err(Error::new(
                        ErrorKind::Conflict,
                        format!("Mapping {id} has local changes since its last artifact import"),
                    ));
                }
            }
            let revision = if unchanged {
                current.as_ref().unwrap().id()?
            } else {
                let shared_baseline = match active {
                    Some(active) => active.shared_baseline.clone(),
                    None => committed
                        .get(id)
                        .map(|record| {
                            Ok::<_, Error>(SharedBaseline {
                                source: source.clone(),
                                revision: record.id()?.to_string(),
                            })
                        })
                        .transpose()?,
                };
                state.mappings.insert(
                    id.clone(),
                    ActiveDocument {
                        revision: incoming.revision.clone(),
                        shared_baseline,
                    },
                );
                incoming.revision.clone()
            };
            state
                .mapping_artifact_baselines
                .entry(source.clone())
                .or_default()
                .insert(
                    id.clone(),
                    MappingArtifactBaseline {
                        store: bundle.store.clone(),
                        revision: incoming.revision.clone(),
                    },
                );
            results.push(MappingImportResult {
                id: id.clone(),
                revision,
                status: if unchanged {
                    MappingImportStatus::Unchanged
                } else if current.is_some() {
                    MappingImportStatus::Updated
                } else {
                    MappingImportStatus::Imported
                },
            });
        }
        for bundle in artifact.mappings.values() {
            for view in &bundle.revisions {
                self.mapping_repository.record(&view.record)?;
            }
        }
        atomic_write(&self.state_path, &serde_json::to_vec_pretty(&state)?)?;
        Ok(results)
    }
}
