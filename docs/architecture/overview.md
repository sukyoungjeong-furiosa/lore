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
| Mapping Store | A shared space that registers code repositories and document backends and owns their mappings, revision history, and shared heads. Independent of its database or server URL. |
| Lore Server | Provides authenticated access to mapping stores, synchronization, and background reconciliation. |
| Mapping Catalog | A discovery index of mapping references and metadata for a code repository. It does not contain document bodies or replace a mapping store. |
| Code Repository | The identity and revision history of code, independent of checkout paths and Git remote names. |
| Workspace | A checkout or worktree's local mapping changes, active revisions, synchronization baselines, and projections. |
| Model Connection | An API endpoint and authentication configuration used by model resolvers. It grants neither document nor mapping-store access. |
| Resolver Profile | Selects a model connection, model, prompt, and request limits. |

A document backend is a document space, not merely an adapter kind: several backends may use Notion. A mapping store references documents without owning their backends. A server may host several stores; document storage is a separate capability.

## Connections

- **Lore remote:** A named connection to one mapping store through a Lore server. That store may register multiple code repositories and document backends. The remote name is a local alias; the store has its own stable identity.
- **Backend connection:** Configures personal access to a registered document backend, including authentication and page discovery settings. It may also support standalone document browsing without a Lore remote.
- **Model connection:** Configures model API access independently of store and backend connections.

Within Lore, **remote** means the mapping-sharing connection; document and model connections are not called remotes. A **Git remote** refers to Git. Several Lore remotes may use the same server, and server replicas or URL changes do not create new store identities.

For example, `compiler-team` may connect to a store containing compiler and kernel repositories alongside Notion and Lore-owned document backends. A workspace selects mappings for its current repository; it does not define the remote's full scope. A `personal` remote may connect to a local server and register some of the same resources with independent mappings.

Repository and backend registrations belong to the shared space; the actual code and documents retain their external ownership. Personal credentials remain client settings. Sharing a mapping does not grant document access. A mapping catalog covers one repository's discovery metadata; a read-only catalog connection is not a synchronization remote.

CLI, editor integrations, and MCP are interfaces to these operations. [Sharing](sharing.md) defines the relationships and access contracts.
