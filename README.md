# Lore

Lore links code to documentation for humans and AI agents.

## Install

Development installation requires Rust 1.92+ and Node.js/npm for the VS Code extension. GitHub Release packages and the Git-backed Codex plugin marketplace are planned.

### CLI

```bash
git clone https://github.com/sukyoungjeong-furiosa/lore.git
cd lore
cargo install --path crates/lore --locked
```

Run `lore init` in the code repository you want to work with.

### VS Code

From the Lore checkout:

```bash
cd editors/vscode
npm ci
npm run package -- --out lore.vsix
code --install-extension lore.vsix --force
```

For Remote SSH, install the VSIX from the connected window and install the CLI on the remote host. Updates currently require reinstalling the VSIX; a private Marketplace is planned.

### Codex

Optional hooks and a replaceable example skill require [manual setup](integrations/codex/README.md).

## Connect Notion

In VS Code, run **Lore: Initialize Workspace**, then **Lore: Connect Notion**. Supply a page URL and a token authorized to read it. CLI and hooks share the saved connection on the same machine.

For CLI setup and operations, use `lore --help` and `lore backend --help`.

## Documentation

- [Architecture](docs/architecture/)
- [Experience](docs/experience.md)
