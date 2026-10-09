# Architecture Overview

Lore connects code to documents across knowledge backends and preserves those connections for reuse.

## Principles

- **Code-centric:** Knowledge is linked to code, enabling navigation between code and its context.
- **Decoupled:** Documents and code mappings have separate ownership and evolve independently.
- **Agent-native:** Agents can discover knowledge and maintain its connections to code.
- **Human-readable:** Documents and mapping explanations remain accessible to humans.

## Architecture

- **Knowledge Backends:** Own document content, editing, permissions, and available history. External systems such as Notion and Lore's own document storage can coexist.
- **Mapping Store:** Owns persistent document-to-code mappings and their revisions, independently of document storage.
- **Workspace:** Provides code-aware discovery, local mapping changes, code projections, and caches for a checkout or worktree.
- **Interfaces:** CLI, editor integrations, and MCP provide access to Lore and supported backend operations.

An optional server shares Lore-managed mappings. Document access and management follow the owning backend's capabilities and permissions.
