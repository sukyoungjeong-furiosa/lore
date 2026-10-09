use crate::{Anchor, CodeContext, DocumentId, Result, hash, validate_relative_path};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetFile {
    pub path: String,
    pub context: CodeContext,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Projection {
    pub anchor: DocumentId,
    pub target: Vec<TargetFile>,
    pub locations: Vec<Location>,
    pub alternatives: Vec<Vec<Location>>,
    pub explanation: String,
}
impl Projection {
    pub fn possible_locations(&self) -> impl Iterator<Item = &Location> {
        self.locations
            .iter()
            .chain(self.alternatives.iter().flatten())
    }
}

pub trait CodeRepository {
    fn identity(&self) -> &str;
    fn head(&self) -> Option<String>;
    fn read(&self, path: &str) -> Result<Option<String>>;
    fn paths(&self, anchor: &Anchor, requested: Option<&str>) -> Result<Vec<String>>;
    fn committed_content(&self, path: &str) -> Result<Option<String>>;
}

/// Resolution is read-only. A resolver may discover code through the repository port.
pub trait AnchorResolver {
    fn resolve(
        &self,
        anchor: &Anchor,
        code: &dyn CodeRepository,
        requested: Option<&str>,
    ) -> Result<Projection>;
}

pub struct TextResolver;
impl AnchorResolver for TextResolver {
    fn resolve(
        &self,
        anchor: &Anchor,
        code: &dyn CodeRepository,
        requested: Option<&str>,
    ) -> Result<Projection> {
        let mut result = Projection {
            anchor: anchor.id.clone(),
            target: vec![],
            locations: vec![],
            alternatives: vec![],
            explanation: String::new(),
        };
        if anchor.context.repository != code.identity() {
            result.explanation = "Target repository differs from the recorded repository".into();
            return Ok(result);
        }
        validate_relative_path(&anchor.path)?;
        if hash(&anchor.snapshot) != anchor.context.content_digest {
            return Err(crate::Error::invalid(
                "Anchor snapshot does not match its code context",
            ));
        }
        let source: Vec<_> = anchor.snapshot.lines().collect();
        if anchor.start == 0 || anchor.end < anchor.start || anchor.end > source.len() {
            return Err(crate::Error::invalid("Anchor exceeds the captured code"));
        }
        let excerpt = &source[anchor.start - 1..anchor.end];
        let mut matches = vec![];
        if let Some(text) = code.read(&anchor.path)?
            && hash(&text) == anchor.context.content_digest
        {
            result.target.push(TargetFile {
                path: anchor.path.clone(),
                context: CodeContext {
                    repository: code.identity().into(),
                    head: code.head(),
                    working_tree: code.committed_content(&anchor.path)?.as_deref() != Some(&text),
                    content_digest: hash(&text),
                },
            });
            result.locations.push(Location {
                path: anchor.path.clone(),
                start: anchor.start,
                end: anchor.end,
            });
            result.explanation =
                "Recorded file content is unchanged; explanation validity was not assessed".into();
            return Ok(result);
        }
        for path in code.paths(anchor, requested)? {
            validate_relative_path(&path)?;
            let Some(text) = code.read(&path)? else {
                continue;
            };
            result.target.push(TargetFile {
                path: path.clone(),
                context: CodeContext {
                    repository: code.identity().into(),
                    head: code.head(),
                    working_tree: code.committed_content(&path)?.as_deref() != Some(&text),
                    content_digest: hash(&text),
                },
            });
            let lines: Vec<_> = text.lines().collect();
            if lines.len() < excerpt.len() {
                continue;
            }
            for (index, window) in lines.windows(excerpt.len()).enumerate() {
                if window == excerpt {
                    matches.push(Location {
                        path: path.clone(),
                        start: index + 1,
                        end: index + excerpt.len(),
                    });
                }
            }
        }
        match matches.len() {
            0 => result.explanation = "No exact excerpt correspondence established; changed code may require another resolver".into(),
            1 => {
                result.locations = matches;
                result.explanation = "Unique exact excerpt correspondence; explanation validity was not assessed".into();
            }
            _ => {
                result.alternatives = matches.into_iter().map(|location| vec![location]).collect();
                result.explanation = "Multiple exact excerpts are alternatives, not a joint correspondence".into();
            }
        }
        Ok(result)
    }
}
