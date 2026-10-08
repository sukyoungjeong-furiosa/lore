# Interfaces

Lore provides CLI, editor, and agent-facing interfaces to its knowledge and workspace operations.

## Common Semantics

- Interfaces may expose different subsets of Lore operations, but shared operations follow the same semantics.
- Interfaces addressing the same workspace share its recorded state and respect its isolation and synchronization contracts.
- Unavailable knowledge, unresolved references, and failures are distinguishable from successful empty results.

## CLI

- Provides document discovery, editing, workspace management, and synchronization.
- Supports non-interactive use and machine-readable results for automation.

## Editor Integration

- Connects code locations to related documents and supports navigation between code and knowledge.
- Supports reading, editing, and navigating documents within the editor.
- Uses the active workspace's code context for anchor projection.

## Agent Integration

- Exposes structured operations through MCP or equivalent interfaces.
- Supports knowledge discovery, reading, editing, and code-reference navigation.
- Does not prescribe agent workflows, prompts, or strategies for organizing knowledge.
