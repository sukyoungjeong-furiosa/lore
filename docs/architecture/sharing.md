# Sharing

Lore shares code relationships through mapping stores. A [Lore remote](overview.md#connections) connects a client to one such shared space, containing registrations for multiple code repositories and document backends. Documents remain with their owning backends.

The shared space defines which code and knowledge can be connected. A workspace determines what is relevant to its current checkout without redefining that shared space.

## Relationships

| Relationship | Cardinality | Contract |
| --- | --- | --- |
| Lore Server → Mapping Store | 1:N | A logical service can host several stores. Store identity survives endpoint and deployment changes. |
| Lore Remote → Mapping Store | N:1 | Each remote selects one store. Its local alias and endpoint do not define store identity. |
| Mapping Store → Repository / Backend Registration | 1:N each | A store defines its shared resource set. Registrations contain no personal credentials. |
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

Documents may have no mappings; stores may have no registrations and workspaces may have no remotes. Connecting a remote does not require fetching every repository's mappings or every backend's documents.

## Identity and Scope

- Stores, repositories, and document backends have stable identities independent of URLs, Git remote names, local aliases, and credentials. Backend identities identify document spaces, not adapter kinds or selected root pages.
- A remote selects a store, not a single repository–backend pair. The store's repository and backend registrations are shared configuration; local filters do not change that configuration.
- A mapping references one registered repository and one document in a registered backend. The same repository or backend may be registered in several stores, each with independent mappings and repository-specific canonical refs.
- Workspace discovery uses its current code repository and any selected backend scope within each connected store. It includes accessible mappings in that scope; backend page discovery remains separate. Document-level selection is not required initially.
- Mapping identity, baselines, and publication targets retain their store identity. Document identity is its backend identity and backend-native document ID. Matching titles or references do not merge independent mappings.
- Connection settings select knowledge; they do not grant access. The server enforces permitted store, repository, and backend scope. Mapping access does not confer document access.
- Backend connections bind shared backend identities to personal credentials and local display names. Connecting a remote does not create credentials or replace an existing personal connection. Cached content and authentication remain local to the user's environment.

## Working Experience

- Discovery combines selected Lore remotes while retaining provenance. Users need not choose a store for every read.
- Creating a shared mapping uses a configured writable destination. An ambiguous destination is selected explicitly rather than guessed.
- Fetch and Pull may select multiple Lore remotes. Push targets one mapping in one store. A failed connection does not hide successful results or imply complete discovery.
- Local changes and projections remain workspace-specific. Connecting another store does not redirect existing changes, baselines, or caches.
- Lore remote settings may be reused across checkouts without sharing working changes or projections. Backend and model connections can be reused independently.

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
