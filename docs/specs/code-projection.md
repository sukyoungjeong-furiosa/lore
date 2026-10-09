# Code Projection

Code projection describes correspondence between a recorded anchor and a target
code state, independently of the document backend. Server reconciliation is defined in [Synchronization](synchronization.md#server-reconciliation).

## Reference

- A result identifies the original anchor and the target code repository and state.
- Projection preserves the original reference and does not modify documents, recorded mappings, or code.

## Correspondence

- A result can identify one or more corresponding locations, competing alternatives, or no established correspondence.
- Multiple locations that jointly correspond to an anchor are distinct from alternatives whose correspondence is uncertain.
- No established correspondence is a normal outcome; it does not prove that the code was deleted. Correspondence does not establish the validity of the document's explanation.

## Code Context

- Targets can be committed revisions or working code states with uncommitted changes.
- A branch names a moving target. Results identify the code state used, not the branch name alone.

## Reuse

- Results may be computed on demand and reused across code changes.
- Reused results retain their code context and uncertainty. Reuse alone does not establish applicability to a different code state.

## Resolvers

- Resolvers interpret an anchor against a requested target code context and may discover the needed source and target information themselves.
- Results report corresponding locations and uncertainty, with explanations when available.
- Resolver failures must not be reported as established absence of corresponding code.
- Agent choice, reasoning strategy, and prompts are not prescribed.
