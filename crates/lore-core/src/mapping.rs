use crate::{
    Actor, Anchor, DocumentReference, Error, Result, RevisionId, hash, validate_relative_path,
};
use serde::{Deserialize, Serialize};

/// A checkout-relative pattern evaluated against current paths without code projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathScope {
    pub repository: String,
    pub glob: String,
}

impl PathScope {
    pub fn matcher(&self) -> Result<globset::GlobMatcher> {
        if self.repository.trim().is_empty() || self.glob.len() > 1024 {
            return Err(Error::invalid(
                "Code scopes require a repository and a glob of at most 1024 bytes",
            ));
        }
        validate_relative_path(&self.glob)?;
        globset::GlobBuilder::new(&self.glob)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map(|glob| glob.compile_matcher())
            .map_err(|error| Error::invalid(format!("Invalid code scope glob: {error}")))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MappingId(String);

impl MappingId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for MappingId {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<String> for MappingId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        uuid::Uuid::parse_str(&value)
            .map(|v| Self(v.to_string()))
            .map_err(|_| Error::invalid("Invalid mapping ID"))
    }
}

impl std::str::FromStr for MappingId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        value.to_owned().try_into()
    }
}

impl From<MappingId> for String {
    fn from(value: MappingId) -> Self {
        value.0
    }
}

impl std::fmt::Display for MappingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub id: MappingId,
    pub document: DocumentReference,
    pub anchors: Vec<Anchor>,
    pub context: String,
    pub archived: bool,
    // Empty scopes are omitted so existing immutable revisions retain their hashes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<PathScope>,
}

impl Mapping {
    pub fn validate(&self) -> Result<()> {
        self.document.validate()?;
        if self.anchors.is_empty() && self.scopes.is_empty() {
            return Err(Error::invalid("Mappings require an anchor or a code scope"));
        }
        for scope in &self.scopes {
            scope.matcher()?;
        }
        for anchor in &self.anchors {
            validate_relative_path(&anchor.path)?;
            if anchor.start == 0
                || anchor.end < anchor.start
                || anchor.end > anchor.snapshot.lines().count()
                || hash(&anchor.snapshot) != anchor.context.content_digest
            {
                return Err(Error::invalid("Anchor does not match its captured code"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingRevision {
    pub parent: Option<RevisionId>,
    pub actor: Actor,
    pub at_ms: u64,
    pub mapping: Mapping,
}

impl MappingRevision {
    pub fn id(&self) -> Result<RevisionId> {
        Ok(RevisionId::of(serde_json::to_vec(self)?))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingView {
    pub revision: RevisionId,
    pub record: MappingRevision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnchorSummary {
    pub id: crate::DocumentId,
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub context: crate::CodeContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingSummary {
    pub id: MappingId,
    pub document: DocumentReference,
    pub anchors: Vec<AnchorSummary>,
    pub context: String,
    pub archived: bool,
    #[serde(default)]
    pub scopes: Vec<PathScope>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingRevisionSummary {
    pub parent: Option<RevisionId>,
    pub actor: Actor,
    pub at_ms: u64,
    pub mapping: MappingSummary,
}

/// Discovery omits captured source bodies; full revisions remain available through explicit reads.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingSummaryView {
    pub revision: RevisionId,
    pub record: MappingRevisionSummary,
}

impl From<MappingView> for MappingSummaryView {
    fn from(view: MappingView) -> Self {
        let record = view.record;
        let mapping = record.mapping;
        Self {
            revision: view.revision,
            record: MappingRevisionSummary {
                parent: record.parent,
                actor: record.actor,
                at_ms: record.at_ms,
                mapping: MappingSummary {
                    id: mapping.id,
                    document: mapping.document,
                    context: mapping.context,
                    archived: mapping.archived,
                    scopes: mapping.scopes,
                    anchors: mapping
                        .anchors
                        .into_iter()
                        .map(|anchor| AnchorSummary {
                            id: anchor.id,
                            path: anchor.path,
                            start: anchor.start,
                            end: anchor.end,
                            context: anchor.context,
                        })
                        .collect(),
                },
            },
        }
    }
}

/// Stores immutable mapping revisions independently of document access and workspace activation.
pub trait MappingRepository {
    fn load(&self, mapping: &MappingId, revision: &RevisionId) -> Result<MappingRevision>;
    fn record(&self, revision: &MappingRevision) -> Result<RevisionId>;
}
