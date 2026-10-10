# Lore

Lore links code to documentation for humans and AI agents.

Browse documents, map them to code, project anchors across revisions, and share mappings locally or through a Lore server. [Architecture](docs/architecture/overview.md) defines the contracts.

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

## Local Worktrees

`lore init` creates or reuses a personal mapping store outside Git, without a server or agent process. One store can register several repositories and document backends. Run it in each worktree; published mappings are discoverable across worktrees while local edits, active revisions, baselines, and projections stay separate.

```bash
lore init
lore map create "<backend-id>" "<document-id>" src/scheduler.rs:100-150
lore push "<mapping-id>"
```

In another worktree, run `lore init`, then `lore pull --path src/` to activate the shared mappings. `lore fetch` caches discovery metadata; `lore fetch --id <mapping-id>` retrieves history without changing active revisions or baselines. Push and Pull synchronize mappings, not document bodies.

Use `lore store ls/show` to inspect local stores. To select another, run `lore store create <name> [--path <directory>]`, then `lore init --local-store <name>`. Operations can also select `--local-store <name>` explicitly. Creating a store does not require a Git repository. A moved store can be connected with `lore store add <name> <directory>` and selected again without changing its identity or baselines.

Stores live under Lore's data home, normally `~/.local/share/lore/stores/`. `LORE_HOME` overrides the data home, including connections and caches; set it to `~/.lore` if preferred. Existing data locations are retained. Local access uses filesystem permissions and requires no API token.

## Lore Remotes

A Lore remote connects to one shared mapping store. A server can host several stores, each registering multiple repositories and document backends. Credentials and working changes remain local.

Generate a server configuration once, using the repository ID from `lore status` and the backend ID from `lore backend ls`. Set `LORE_SERVER_TOKEN` in the server environment before starting:

```bash
lore --json server config --repository "<repository-id>" --backend "<backend-id>" > server.json
lore server serve --config server.json --data-dir "<server-data-directory>"
```

The configuration assigns a stable store ID in `stores[0].id`; keep it when moving the server. Edit the configuration to register more stores, repositories, backends, or users with separate token environment variables and read/write grants. Keep configuration and server data outside the code repository.

On each client, add the remote and supply its assigned token through `LORE_REMOTE_TOKEN` or, after adding it, `lore remote auth set compiler-team --stdin`:

```bash
lore remote add compiler-team --url http://localhost:7766 --store "<store-id>"
lore remote inspect compiler-team
lore push "<mapping-id>" --remote compiler-team
lore pull --remote compiler-team --path src/
```

- `fetch --remote <name>` caches discovery metadata; adding `--id <mapping-id>` fetches mapping history. Neither changes active revisions or baselines.
- `pull --remote <name>` activates selected shared mappings while preserving local edits. Use `--id` or `--path` to limit incorporation; conflicts remain explicit.
- `push <mapping-id> --remote <name>` publishes one mapping snapshot. Interrupted publications can be retried without duplicating them or losing later edits.
- `remote connect <name>` enables shared discovery without Pull. Push and Pull also connect the remote. Backend credentials remain separate under `lore backend`.

When an ID exists in several stores, use `map --local-store <name>`, `map --remote <name>`, or `map --store <store-id>` to select it. Resolve conflicts explicitly with `map resolve-conflict --help` before publishing again.

The initial server uses file storage and configured bearer-token permissions. Transfers include complete mapping history, limited to 16 MiB; document bodies are not synchronized. Local projection supports exact-text and model resolvers. Server canonical reconciliation is deferred: a publication needing it reports `pending`, with no background worker yet.

## Documentation

- [Architecture](docs/architecture/)
- [Experience](docs/experience.md)
