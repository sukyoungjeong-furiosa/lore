# Workspace

A workspace provides a working context for mappings and knowledge discovery. It can be associated with a code checkout or worktree.

## State Ownership

- Mapping availability, pending mapping changes, synchronization baselines, and code projections are workspace-specific.
- Separate worktrees have independent working state. Changes are not propagated between workspaces implicitly. Initialization may reuse repository and storage defaults without copying local changes.
- Workspaces may share immutable mapping revisions and document caches without sharing local changes or projections.
- A cached external document remains backend-owned; caching does not grant access or editing permissions or create a Lore-owned copy.

## Knowledge Availability

- A workspace need not hold every mapping from a selected store or every document from a connected backend. Shared mappings can be discovered without activating them locally.
- A connected Lore remote may register several repositories and backends. Workspace discovery uses its current repository without narrowing the remote's shared registrations.
- File or directory installation selects mappings associated with the requested code scope, accounting for known code projections.
- Installation makes complete mapping revisions available, including their document references and anchors. It does not require fetching document content.
- Documents can be accessed on demand through their owning backend. Discovery distinguishes recorded mappings from unrecorded suggestions.
- Discovery results do not imply complete coverage of a code repository or document backend.

## Local Changes

- Recorded mapping changes remain local until explicitly pushed and are preserved until explicitly discarded or successfully synchronized.
- Each synchronized mapping retains the shared revision on which its local state is based. Fetching another revision does not overwrite local changes or advance that baseline.
- Document editing follows the owning backend's contract. External edits are not subject to Lore's mapping Push/Pull lifecycle.

## Code Projection

A [code projection](code-projection.md) describes how a recorded anchor corresponds to the workspace's current code.

- Workspace projections account for uncommitted edits and remain separate from recorded mapping state.
- Projection results must not overstate their certainty or applicability to current code.

## Offline Operation

- Available mappings and cached documents can be read and navigated without server or backend access. Cached content retains its observed document state.
- Local mapping changes remain available for later synchronization. Offline document editing depends on the owning backend's capabilities.
- Unavailable documents remain identifiable; failed retrieval does not prevent work on available mappings and knowledge.
