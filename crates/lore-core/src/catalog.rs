use crate::{Error, MappingId, MappingSummaryView, Result, validate_relative_path};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub view: MappingSummaryView,
    pub title: Option<String>,
}

/// A discovery index, not complete mapping revisions or backend document bodies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingCatalog {
    pub schema_version: u32,
    pub repository: String,
    pub mappings: BTreeMap<MappingId, CatalogEntry>,
}

impl MappingCatalog {
    pub fn empty(repository: &str) -> Self {
        Self {
            schema_version: 1,
            repository: repository.into(),
            mappings: BTreeMap::new(),
        }
    }

    pub fn validate(&self, repository: &str) -> Result<()> {
        if self.schema_version != 1 || self.repository != repository {
            return Err(Error::invalid(
                "Mapping catalog schema or repository does not match",
            ));
        }
        if self.mappings.len() > 10000 {
            return Err(Error::invalid("Mapping catalog exceeds 10000 entries"));
        }
        for (id, entry) in &self.mappings {
            let mapping = &entry.view.record.mapping;
            if *id != mapping.id || mapping.anchors.is_empty() && mapping.scopes.is_empty() {
                return Err(Error::invalid(
                    "Catalog mapping identity or code references are invalid",
                ));
            }
            mapping.document.validate()?;
            for scope in &mapping.scopes {
                scope.matcher()?;
            }
            for anchor in &mapping.anchors {
                validate_relative_path(&anchor.path)?;
                if anchor.start == 0
                    || anchor.end < anchor.start
                    || anchor.context.repository.is_empty()
                {
                    return Err(Error::invalid("Catalog anchor location is invalid"));
                }
            }
        }
        Ok(())
    }
}
