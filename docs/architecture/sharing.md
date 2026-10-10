# Sharing

A knowledge store owns Lore-managed documents and code mappings. It registers code repositories and document backends while preserving their separate ownership.

## Relationships

| Relationship | Cardinality | Meaning |
| --- | --- | --- |
| Lore server → Knowledge store | 1:N | A service may host independent shared spaces. |
| Lore remote → Knowledge store | N:1 | A connection selects one stable store identity. |
| Knowledge store ↔ Code repository | N:M | Different teams may maintain independent knowledge for the same code. |
| Knowledge store ↔ Document backend | N:M | Backend registrations identify document spaces, not adapter kinds or personal credentials. |
| Knowledge store → Repository knowledge history | 1:N | Each repository scope has accepted central knowledge and working branches. |
| Shared central knowledge → Personal central knowledge | 1:N | People incorporate shared knowledge while maintaining their local accepted state. |
| Personal central knowledge → Working branch/worktree | 1:N | Agents or people work independently and submit changes to their parent central store. |
| Mapping → Document / Code repository | N:1 each | A mapping connects one document to one repository through anchors or path scopes. |

A person's store may connect to multiple remotes. A selected connection never implies that all local knowledge should be sent to that destination.

## Identity and Access

- Store, repository, backend, and document identities survive renaming and endpoint changes. Matching titles or references do not merge independent stores.
- Code repository and backend registrations belong to the shared space. Credentials and local display names belong to connections.
- Authorization retains repository–backend pairing. Mapping access does not grant document access; inaccessible knowledge must not be overwritten or removed by a client's filtered view.
- Local access uses filesystem permissions. Network access authenticates the submitting person separately from human or agent attribution.
- Lore-owned documents are a distinct backend capability. Sharing their content requires that capability and its permission; referencing a Notion document does not transfer its ownership.

## Central Knowledge

Each central store accepts proposals from its working children. A local central store is accepted knowledge for its agents and a source of proposals for a team store. Acceptance, conflict handling, and retry guarantees follow [synchronization](synchronization.md).

Storage, network request handling, and optional reconciliation are separate responsibilities. Git is an initial history implementation, not the definition of a Lore remote. GitHub and hosted PRs are optional integrations.
