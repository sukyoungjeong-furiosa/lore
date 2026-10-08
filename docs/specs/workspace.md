# Workspace

A workspace provides a working context for knowledge. It can be
associated with a code checkout or worktree.

## State Ownership

- Document availability, pending changes, synchronization baselines, and code projections are workspace-specific.
- Separate worktrees have independent working state. Changes are not propagated between workspaces implicitly.
- Workspaces may share immutable document revisions without sharing local changes or projections.

## Knowledge Availability

- A workspace need not hold every document or every revision from a source.
- File or directory installation selects documents associated with the requested code scope, accounting for known code projections.
- Installation makes complete document revisions available, including all their anchors.
- Linked documents and previous revisions may be fetched separately.

## Local Changes

- Recorded local changes are preserved until explicitly discarded or incorporated through successful synchronization.
- New documents and edits remain local until explicitly pushed.
- Each synchronized document retains the shared revision on which its local state is based.
- Fetching another revision does not overwrite local changes or advance that baseline.

## Code Projection

A [code projection](code-projection.md) describes how a recorded anchor corresponds
to the workspace's current code.

- Workspace projections account for uncommitted edits and remain separate from document state.
- Projection results must not overstate their certainty or applicability to current code.

## Offline Operation

- Available documents can be read, edited, and navigated without a server.
- Local changes remain available for later synchronization.
- Unavailable documents remain identifiable; failed retrieval does not prevent work on available knowledge.
