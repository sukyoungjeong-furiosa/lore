use crate::{Error, MappingBundle, MappingId, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Selected external-document mappings and their complete revision histories.
/// Anchors include captured code. Document bodies and credentials are excluded.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingArtifact {
    pub schema_version: u32,
    pub repository: String,
    pub mappings: BTreeMap<MappingId, MappingBundle>,
}

impl MappingArtifact {
    pub fn validate(&self, repository: &str) -> Result<()> {
        if self.schema_version != 1 || self.repository != repository || repository.trim().is_empty()
        {
            return Err(Error::invalid(
                "Mapping artifact schema or code repository does not match",
            ));
        }
        if self.mappings.len() > 10000 {
            return Err(Error::invalid("Mapping artifact exceeds 10000 mappings"));
        }
        for (id, bundle) in &self.mappings {
            bundle.validate()?;
            if bundle.head()?.record.mapping.id != *id {
                return Err(Error::invalid(
                    "Artifact mapping key does not match its identity",
                ));
            }
            for view in &bundle.revisions {
                if view.record.mapping.code_repository()? != repository {
                    return Err(Error::invalid(
                        "Artifact mapping belongs to a different code repository",
                    ));
                }
                if view.record.mapping.document.backend == "local" {
                    return Err(Error::invalid(
                        "Mapping artifacts support external documents only; Lore-owned documents require knowledge sharing",
                    ));
                }
            }
        }
        Ok(())
    }
}
