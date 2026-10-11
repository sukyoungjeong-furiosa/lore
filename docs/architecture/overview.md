# Architecture Overview

Lore connects code to documents and preserves those relationships for reuse.
[Technical Overview](../design.md) explains the system; these documents retain its core contracts.

## Components

| Component | Responsibility |
| --- | --- |
| Document backend | Owns document content and access. Notion and Lore-owned documents are separate capabilities. |
| Knowledge store | Maintains Lore-owned documents, mappings, working history, proposals, and integrated central knowledge. |
| Workspace | Associates a code checkout with its knowledge working branch/worktree, pending changes, and projections. |
| Lore server | Provides authenticated discovery, submission, and integration without requiring an agent. |
| Resolver | Relates recorded anchors to a target code state without validating document content. |
| Interfaces | CLI, editor integrations, and agent interfaces use the same operation semantics. |

A personal store serves independent working branches and may submit integrated knowledge to a team store. Both relationships use proposals and integration; their submission scope and acceptance policy may differ. Git implements history and worktrees by default without requiring Git transport, GitHub, or hosted PRs.

## Connections

- **Lore remote:** A named connection to a shared knowledge store through a Lore server. A store can register multiple code repositories and document backends.
- **Local store connection:** Selects a knowledge store by persistent identity and filesystem location.
- **Backend connection:** Configures personal document access and discovery.
- **Model connection:** Configures resolver API access independently of knowledge access.

Within Lore, remote means the knowledge-sharing connection. A document backend connection and a Git remote are separate concepts.

## Sharing and Access

- A server may host multiple stores. A Lore remote selects one store; a person may connect to several remotes.
- Stores may register multiple code repositories and document backends. The same repository or backend may participate in independent stores.
- Store and resource identities are distinct from display names, endpoints, and credentials. Copying object IDs does not establish shared history.
- Authorization preserves repository–backend pairing. Mapping metadata access does not grant access to backend document content, and filtered views must not remove inaccessible knowledge.
- Credentials belong to the execution environment. Local access uses filesystem permissions; server authentication is separate from human or agent authorship.
- Interfaces distinguish unavailable knowledge, unresolved references, and failures from successful empty results. Discovery identifies its scope without promising complete coverage.

[Knowledge Model](knowledge-model.md), [Workspace](workspace.md), [Synchronization](synchronization.md), and [Code Projection](code-projection.md) define the remaining contracts.
