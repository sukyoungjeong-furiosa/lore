# Architecture Overview

Lore connects code to documents and preserves those relationships for reuse.

## Principles

- **Code-centric:** Navigate between code and its context.
- **Decoupled:** Documents, code relationships, and code have separate ownership and histories.
- **Agent-native:** Humans and agents discover, edit, and submit knowledge through common operations.
- **Human-readable:** Content and explanations remain inspectable.

## Components

| Component | Responsibility |
| --- | --- |
| Document backend | Owns document content and access. Notion and Lore-owned documents are separate capabilities. |
| Knowledge store | Maintains Lore-owned documents, mappings, accepted central knowledge, working history, and proposals. |
| Workspace | Associates a code checkout with its knowledge working branch/worktree, pending changes, and projections. |
| Lore server | Provides authenticated discovery, submission, and integration. Reconciliation agents are optional. |
| Resolver | Relates recorded code references to a target code state. Model connections and profiles are replaceable. |
| Interfaces | CLI, editor integrations, and agent interfaces expose these operations. |

A personal central store serves independent working branches and may submit accepted knowledge to a team store. Git can implement history without determining the sharing protocol or hosting service.

## Connections

- **Lore remote:** A named connection to a shared knowledge store through a Lore server. A store can register multiple code repositories and document backends.
- **Local store connection:** Selects a knowledge store by persistent identity and filesystem location.
- **Backend connection:** Configures personal document access and discovery.
- **Model connection:** Configures resolver API access independently of knowledge access.

Within Lore, remote means the knowledge-sharing connection. A document backend connection and a Git remote are separate concepts.

[Sharing](sharing.md), [workspace](workspace.md), and [synchronization](synchronization.md) define the behavioral contracts.
