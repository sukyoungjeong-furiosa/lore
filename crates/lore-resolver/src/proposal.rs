use lore_core::{Anchor, Location, Result, TargetFile};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ProjectionInput {
    pub source: Anchor,
    pub target: Vec<CodeFile>,
}

#[derive(Serialize)]
pub struct CodeFile {
    pub reference: TargetFile,
    pub content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionProposal {
    pub candidates: Vec<Vec<ProposedLocation>>,
    pub explanation: String,
}

#[derive(Deserialize)]
pub struct ProposedLocation {
    #[serde(flatten)]
    pub location: Location,
    pub excerpt: String,
}

/// Proposes correspondence against captured input; it cannot establish correspondence or mutate code.
pub trait ProjectionProposer {
    fn propose(&self, input: &ProjectionInput) -> Result<ProjectionProposal>;
}
