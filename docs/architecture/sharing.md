# Sharing

Lore shares code relationships through mapping stores. Documents remain with their owning [document backends](overview.md#components-and-terms).

## Relationships

| Relationship | Cardinality | Contract |
| --- | --- | --- |
| Lore Server → Mapping Store | 1:N | A logical service can host several stores. Store identity survives endpoint and deployment changes. |
| Mapping Store ↔ Code Repository | N:M | Teams can maintain independent mappings for the same repository. |
| Mapping Store ↔ Document Backend | N:M | Stores reference selected document spaces; neither owns the other. |
| Document Backend → Document | 1:N | A document has one owning backend. Caches do not become owners. |
| Mapping Store → Mapping | 1:N | A shared mapping belongs to one store. Copies in different stores are independent relationships. |
| Mapping → Document / Code Repository | N:1 each | Each mapping references one document and one repository. |
| Mapping → Anchor / Path Scope | 1:N combined | At least one code reference is required. |
| Code Repository → Workspace | 1:N | Each workspace uses one checkout or worktree with independent working state. |
| Workspace ↔ Mapping Store / Document Backend | N:M | Workspaces select stores and access backends independently. |
| Anchor → Projection Result | 1:N | Different target states and resolvers can produce different results. |
| Model Connection → Resolver Profile | 1:N | Profiles can reuse authentication while selecting different models or prompts. |

Documents may have no mappings; stores and workspaces may have no connections. Selecting a store does not select all its repositories or backends.

## Identity and Scope

- Stores, repositories, and document backends have stable identities independent of URLs, Git remote names, local aliases, and credentials. Backend identities identify document spaces, not adapter kinds or selected root pages.
- A store connection selects one store, one code repository, and one or more document backends. Discovery includes all accessible mappings in that scope. Document-level selection is not required initially; backend page discovery remains separate.
- Mapping identity, baselines, and publication targets retain their store identity. Document identity is its backend identity and backend-native document ID. Matching titles or references do not merge independent mappings.
- Connection settings select knowledge; they do not grant access. The server enforces permitted store, repository, and backend scope. Mapping access does not confer document access.
- Backend connections use shared identities with personal credentials and local display names. Cached content and authentication remain local to the user's environment.

## Working Experience

- Discovery combines selected store connections while retaining provenance. Users need not choose a store for every read.
- Creating a shared mapping uses a configured writable destination. An ambiguous destination is selected explicitly rather than guessed.
- Fetch and Pull may select multiple store connections. Push targets one mapping in one store. A failed connection does not hide successful results or imply complete discovery.
- Local changes and projections remain workspace-specific. Connecting another store does not redirect existing changes, baselines, or caches.
- Store connection choices may be reused across checkouts without sharing working changes or projections. Backend and model connections can be reused independently.

## Server

Request handling, durable storage, and background projection are separate responsibilities.

- A Push atomically checks authorization and the expected shared revision, records an immutable mapping revision, and advances its shared head. Conflicts preserve the proposal and current shared revision.
- Publication is independent of document retrieval and model availability. The client learns the published revision even when reconciliation is pending.
- Retrying a publication has one durable outcome. Reusing its operation identity with different content is rejected; retries do not duplicate revisions or jobs.
- Authentication identifies the submitting user separately from human or agent attribution. Secrets are not shared store configuration or revision data.
- Canonical reconciliation uses the ref configured for each store–repository pair and available read-only code context. It records the target commit and preserves the submitted anchor.
- Reconciliation results belong to a mapping revision and target code state; they do not rewrite published revisions. Missing context or uncertain correspondence remains explicit.
- Background work survives restart and reuses applicable results. A ref advance does not make an earlier projection current because it finished later.
- Client working trees are not presumed to exist on the server. Server model connections and credentials are configured independently from clients.

Lore-owned document storage and sharing are separate backend capabilities from mapping synchronization.
