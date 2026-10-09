use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DocumentId(String);
impl DocumentId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Default for DocumentId {
    fn default() -> Self {
        Self::new()
    }
}
impl TryFrom<String> for DocumentId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        uuid::Uuid::parse_str(&value)
            .map(|v| Self(v.to_string()))
            .map_err(|_| Error::invalid("Invalid document ID"))
    }
}
impl std::str::FromStr for DocumentId {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        value.to_owned().try_into()
    }
}
impl From<DocumentId> for String {
    fn from(value: DocumentId) -> Self {
        value.0
    }
}
impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RevisionId(String);
impl RevisionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn of(bytes: impl AsRef<[u8]>) -> Self {
        Self(hash(bytes))
    }
}
impl TryFrom<String> for RevisionId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::invalid("Invalid revision ID"));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
}
impl std::str::FromStr for RevisionId {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        value.to_owned().try_into()
    }
}
impl From<RevisionId> for String {
    fn from(value: RevisionId) -> Self {
        value.0
    }
}
impl std::fmt::Display for RevisionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Human,
    Agent,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Curation {
    pub by: String,
    pub content_digest: String,
    pub at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeContext {
    pub repository: String,
    pub head: Option<String>,
    pub working_tree: bool,
    pub content_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub id: DocumentId,
    pub path: String,
    pub start: usize,
    pub end: usize,
    pub context: CodeContext,
    pub snapshot: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub title: String,
    pub content_format: String,
    pub content: String,
    pub tags: Vec<String>,
    pub authorship: Vec<Actor>,
    pub curation: Option<Curation>,
    pub archived: bool,
    pub anchors: Vec<Anchor>,
}
impl Document {
    pub fn new(title: String, content: String, actor: Actor) -> Result<Self> {
        let value = Self {
            id: DocumentId::new(),
            title,
            content_format: "markdown".into(),
            content,
            tags: vec![],
            authorship: vec![actor],
            curation: None,
            archived: false,
            anchors: vec![],
        };
        value.validate()?;
        Ok(value)
    }
    pub fn content_digest(&self) -> String {
        hash(
            serde_json::to_vec(&(&self.content_format, &self.content))
                .expect("String serialization"),
        )
    }
    pub fn curated(&self) -> bool {
        self.curation
            .as_ref()
            .is_some_and(|c| c.content_digest == self.content_digest())
    }
    pub fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(Error::invalid("Title must not be empty"));
        }
        if self.content_format.trim().is_empty() {
            return Err(Error::invalid("Content format must not be empty"));
        }
        if self.tags.iter().any(|t| t.trim().is_empty()) {
            return Err(Error::invalid("Tags must not be empty"));
        }
        for anchor in &self.anchors {
            validate_relative_path(&anchor.path)?;
            let count = anchor.snapshot.lines().count();
            if anchor.start == 0 || anchor.end < anchor.start || anchor.end > count {
                return Err(Error::invalid("Anchor exceeds the captured code"));
            }
            if hash(&anchor.snapshot) != anchor.context.content_digest {
                return Err(Error::invalid(
                    "Anchor snapshot does not match its code context",
                ));
            }
        }
        Ok(())
    }
    pub fn record_content_author(&mut self, actor: Actor) {
        if !self.authorship.contains(&actor) {
            self.authorship.push(actor);
        }
    }
}

pub fn validate_relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(Error::invalid(
            "Code path must be relative to the checkout without traversal",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub parent: Option<RevisionId>,
    pub actor: Actor,
    pub at_ms: u64,
    pub document: Document,
}
impl Revision {
    pub fn id(&self) -> Result<RevisionId> {
        Ok(RevisionId::of(serde_json::to_vec(self)?))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentView {
    pub revision: RevisionId,
    pub document: Document,
    pub curated: bool,
}
impl DocumentView {
    pub fn new(revision: RevisionId, document: Document) -> Self {
        let curated = document.curated();
        Self {
            revision,
            document,
            curated,
        }
    }
}
