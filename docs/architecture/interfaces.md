# Interfaces

Lore provides CLI, editor, and agent-facing interfaces for knowledge operations and code-aware discovery.

## Common Semantics

- Interfaces may expose different subsets of Lore operations, but shared operations follow the same semantics.
- Interfaces addressing the same workspace share its recorded knowledge state and respect its isolation and synchronization contracts.
- Document access and editing follow backend capabilities and permissions. Unsupported operations are explicit.
- Unavailable knowledge, unresolved references, and failures are distinguishable from successful empty results. Discovery identifies its scope and does not imply complete backend coverage.

## CLI

- Provides code-aware discovery, document and mapping operations, workspace management, and knowledge synchronization.
- Supports non-interactive use and machine-readable results for automation.
- Provides document access and management where supported by the owning backend.

## Editor Integration

- Connects code locations to related documents and supports navigation through Lore or the owning backend.
- Supports inspecting and editing mappings within the editor. Document reading and editing depend on backend support.
- Uses the active workspace's code context for anchor projection.

## Agent Integration

- Exposes structured operations through MCP or equivalent interfaces.
- Supports discovery, document access, mapping creation and maintenance, and code-reference navigation.
- Agents may use backend-native tools for document operations while using Lore to preserve code relationships.
- Does not prescribe agent workflows, prompts, or strategies for organizing knowledge.
