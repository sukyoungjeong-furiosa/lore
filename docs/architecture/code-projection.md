# Code Projection

Code projection describes correspondence between a recorded anchor and a target
code state, independently of the document backend. Canonical reconciliation is defined in [Synchronization](synchronization.md#code-reconciliation).

## Reference

- A result identifies the original anchor and the target code repository and state.
- Projection preserves the original reference and does not modify documents, recorded mappings, or code.

## Correspondence

- A result distinguishes established correspondence, unconfirmed candidates, and no established correspondence. A correspondence can span multiple locations.
- Multiple locations that jointly correspond to an anchor are distinct from alternatives whose correspondence is uncertain.
- No established correspondence is a normal outcome; it does not prove that the code was deleted. Correspondence does not establish the validity of the document's explanation.

## Code Context

- Targets can be committed revisions or working code states with uncommitted changes.
- A branch names a moving target. Results identify the code state used, not the branch name alone.

## Reuse

- Results may be computed on demand and reused across code changes.
- Reused results retain their anchor, target code context, and uncertainty. A matching branch or commit alone does not establish applicability to changed working files.

## Resolvers

- Resolvers interpret an anchor against a requested target code context and may discover the needed source and target information themselves.
- Results report corresponding locations and uncertainty, with explanations when available.
- Resolver failures must not be reported as established absence of corresponding code.
- Agent choice, reasoning strategy, and prompts are not prescribed.
- Model-proposed correspondence remains distinguishable from established correspondence. Valid paths and line ranges alone do not establish a semantic relationship.
- Results based on captured code retain that context. Changes during resolution must not cause the result to be presented against a different code state.
