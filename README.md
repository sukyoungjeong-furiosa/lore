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

For Remote SSH, install the VSIX from the connected window and install the CLI on the remote host. See the [extension guide](editors/vscode/README.md) for backend setup. Updates currently require reinstalling the VSIX; a private Marketplace is planned.

### Codex

Optional hooks and a replaceable example skill are available through the [agent setup guide](docs/development/agent-context.md#optional-codex-hook). They currently require manual setup.

## Documentation

- [Architecture](docs/architecture/)
- [Design decisions](docs/decisions.md)
- [Notion MVP](docs/development/notion-mvp.md)
