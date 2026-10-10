# Architecture Overview

Lore connects code to documents and preserves those relationships for reuse.

## Principles

- **Code-centric:** Knowledge is linked to code, enabling navigation between code and its context.
- **Decoupled:** Documents and code mappings have separate ownership and evolve independently.
- **Agent-native:** Agents can discover knowledge and maintain its connections to code.
- **Human-readable:** Documents and mapping explanations remain accessible to humans.

## Components and Terms

| Term | Meaning |
| --- | --- |
| Document Backend | Owns document content, editing, permissions, and available history. Notion and Lore-owned document storage are examples. `backend` in the CLI means a document backend. |
| Mapping Store | Owns mappings, their revision history, and shared heads. A logical sharing space, independent of its database or server URL. |
| Lore Server | Provides authenticated access to mapping stores, synchronization, and background reconciliation. |
| Mapping Catalog | A discovery index of mapping references and metadata for a code repository. It does not contain document bodies or replace a mapping store. |
| Code Repository | The identity and revision history of code, independent of checkout paths and Git remote names. |
| Workspace | A checkout or worktree's local mapping changes, active revisions, synchronization baselines, and projections. |
| Model Connection | An API endpoint and authentication configuration used by model resolvers. It grants neither document nor mapping-store access. |
| Resolver Profile | Selects a model connection, model, prompt, and request limits. |

A document backend is a document space, not merely an adapter kind: several backends may use Notion. A mapping store references documents without owning their backends. A server may host several stores; document storage is a separate capability.

## Connections

- **Store connection:** Selects a mapping store, code repository, and document backends for discovery and synchronization.
- **Backend connection:** Configures access to a document backend, including personal authentication and page discovery settings.
- **Model connection:** Configures model API access independently of store and backend connections.

Use these qualified names rather than an unqualified “remote” or “source.” A Git remote refers only to Git. Server replicas and URL changes do not create new store identities.

CLI, editor integrations, and MCP are interfaces to these operations. [Sharing](sharing.md) defines the relationships and access contracts.
