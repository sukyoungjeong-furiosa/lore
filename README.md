# Lore

Lore links code to documentation for humans and AI agents.

Local document browsing, code mappings, and projection are available. Shared mapping servers and multi-store synchronization are planned; [architecture](docs/architecture/overview.md) defines their contracts.

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

## Model-Assisted Projection

Run **Lore: Configure Resolver** in VS Code to set a model connection, model, and optional token. CLI and agents share the saved credentials on the host running Lore. For CLI setup, supply `FURIOSA_ACCESS_API_KEY` or register a token separately with `lore resolver auth set furiosa --stdin`:

```bash
lore resolver connection set furiosa
lore resolver profile set rngd --connection furiosa --model furiosa-ai/K-EXAONE-236B-A23B-NVFP4A16
lore resolver use rngd
lore --repo <checkout> --json map resolve <mapping-id> --resolver model --path <file-or-directory>
```

Exact-text correspondence is checked first; otherwise the selected model proposes unconfirmed candidates. Use **Lore: Select Resolver**, `lore resolver use <profile>`, or `--profile <name>` to switch models. Environment variables override saved tokens. Without a selected profile, model mode uses Furiosa Access's K-EXAONE defaults and `FURIOSA_ACCESS_API_KEY`. `--resolver-config <file.json>` bypasses profiles for an explicit configuration. Regular discovery and editor navigation do not call the model.

## Mapping Catalogs

`lore map catalog set <url>` connects a read-only HTTP mapping catalog; `lore map fetch` retrieves discovery metadata without document bodies. `lore map publish` publishes to the local catalog only. The earlier `map source` command remains an alias.

## Documentation

- [Architecture](docs/architecture/)
- [Experience](docs/experience.md)
