# Workspace

A workspace associates a code checkout with a knowledge working branch and worktree. Code and knowledge histories remain separate.

## Ownership

- Each code worktree has independent recorded changes, a knowledge working branch, editing baselines, and projections.
- Workspaces using the same local knowledge store and repository share its central branch. Commit alone does not change that branch.
- Remote incorporation baselines belong to the local store, repository, and remote connection; its worktrees share them, independent stores do not.
- Immutable revisions and document caches may be shared. Credentials, projections, and pending changes are not committed knowledge.
- A workspace's selected knowledge view and target code context are separate. Reading central knowledge may project anchors against any associated code worktree.

## Working Knowledge

- Creating and editing Lore-owned documents or mappings records workspace changes. Commit records selected changes on the working branch; unrelated recorded changes remain pending.
- Reads on the working branch include its commits and recorded changes. Central reads include accepted knowledge only and do not activate working revisions.
- Checkout changes the knowledge working branch without discarding pending changes. Reading central knowledge does not permit committing directly to it.
- Pull incorporates accepted central changes into the working branch. Pending edits remain available; their editing baseline does not silently advance.
- Applying edits, commits, or explicit conflict resolutions checks the versions observed by the caller. Conflicts preserve the working content and accepted history.

## Availability and Projection

- A workspace need not hold all knowledge from a source. Related external documents remain backend-owned and can be retrieved on demand.
- Code-scoped discovery accounts for recorded relationships and known projections. Discovery does not establish complete coverage or document validity.
- [Code projections](code-projection.md) remain separate from document and mapping state and must not overstate their applicability.

## Offline Operation

Available knowledge can be read and edited, committed, and submitted to a local central branch without a server or model. Unavailable sources do not prevent work on available knowledge. Network publication remains separate.
