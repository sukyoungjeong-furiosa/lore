use crate::{Error, Mapping, MappingCatalog, MappingId, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KnowledgeStoreId(String);

impl KnowledgeStoreId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for KnowledgeStoreId {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<String> for KnowledgeStoreId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        validate_identity(&value)?;
        Ok(Self(value))
    }
}

impl From<KnowledgeStoreId> for String {
    fn from(value: KnowledgeStoreId) -> Self {
        value.0
    }
}

impl std::str::FromStr for KnowledgeStoreId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        value.to_owned().try_into()
    }
}

impl std::fmt::Display for KnowledgeStoreId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryRegistration {
    pub canonical_ref: Option<String>,
}

/// Shared registrations contain resource identities and policy, not personal credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeStoreDescriptor {
    pub id: KnowledgeStoreId,
    pub repositories: BTreeMap<String, RepositoryRegistration>,
    pub backends: BTreeSet<String>,
}

/// The same mapping ID in two stores identifies two independent relationships.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SharedMappingKey {
    pub store: KnowledgeStoreId,
    pub mapping: MappingId,
}

fn validate_identity(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(Error::invalid(
            "Shared resource identities must be nonempty and contain no control characters",
        ));
    }
    Ok(())
}

impl KnowledgeStoreDescriptor {
    pub fn validate(&self) -> Result<()> {
        for (repository, registration) in &self.repositories {
            validate_identity(repository)?;
            if let Some(reference) = &registration.canonical_ref {
                validate_identity(reference)?;
            }
        }
        for backend in &self.backends {
            validate_identity(backend)?;
        }
        Ok(())
    }

    fn validate_membership(&self, repository: &str, backend: &str) -> Result<()> {
        if !self.repositories.contains_key(repository) {
            return Err(Error::invalid(
                "Code repository is not registered in this mapping store",
            ));
        }
        if !self.backends.contains(backend) {
            return Err(Error::invalid(
                "Document backend is not registered in this mapping store",
            ));
        }
        Ok(())
    }

    /// Checks mapping integrity and registration membership; membership does not grant access.
    pub fn validate_mapping(&self, mapping: &Mapping) -> Result<()> {
        self.validate()?;
        mapping.validate()?;
        self.validate_membership(mapping.code_repository()?, &mapping.document.backend)
    }

    /// A catalog covers one registered repository, not the entire store.
    pub fn validate_catalog(&self, catalog: &MappingCatalog) -> Result<()> {
        self.validate()?;
        if !self.repositories.contains_key(&catalog.repository) {
            return Err(Error::invalid(
                "Catalog code repository is not registered in this mapping store",
            ));
        }
        catalog.validate(&catalog.repository)?;
        for entry in catalog.mappings.values() {
            self.validate_membership(
                &catalog.repository,
                &entry.view.record.mapping.document.backend,
            )?;
        }
        Ok(())
    }
}

/// Compatibility names for the earlier mapping-only store API.
pub type MappingStoreId = KnowledgeStoreId;
pub type MappingStoreDescriptor = KnowledgeStoreDescriptor;
