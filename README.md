# Lore

Lore links code to documentation for humans and AI agents.

Browse documents, map them to code, project anchors across revisions, and share knowledge locally or through a Lore server. [Technical Overview (한국어)](docs/design.md) describes the models, components, and sharing semantics; [Architecture](docs/architecture/overview.md) defines the contracts.

## Install

Development installation requires Git, Rust 1.92+, and Node.js/npm for the VS Code extension. GitHub Release packages and the Git-backed Codex plugin marketplace are planned.

### CLI

```bash
git clone https://github.com/sukyoungjeong-furiosa/lore.git
cd lore
cargo install --path crates/lore --locked
```

Run `lore init` in the code repository you want to work with.

Each code worktree gets its own Lore working branch and worktree. `lore commit` records local work; `lore push` submits it to local central knowledge. The editor browses central knowledge. Git stores history internally; no GitHub knowledge repository is required.

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

Optional hooks and replaceable reading and recording skills require [manual setup](integrations/codex/README.md).

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

Use `--request-options-file <json>` on profile creation or update to configure sampling and reasoning parameters. These options belong to the selected profile; credentials remain separate. No sampling or reasoning preset is imposed by Lore.

## Local Knowledge

```bash
lore init
lore --json create "Scheduler design" --stdin
lore commit <document-id> --if-match <revision>
lore push                         # Submit and integrate locally when non-conflicting
lore --central search scheduler
```

Other working branches receive accepted knowledge with `lore pull`. `lore push --proposal` leaves a proposal for inspection; `lore proposal show <id>` and `lore proposal integrate <id> --if-match <central-version>` review and accept it. `lore worktree` inspects branch state; `lore checkout <branch> --create` creates another working branch. Pending changes are preserved on conflicts.

Mappings follow the same workflow: `lore map create ...`, `lore commit <mapping-id> --mapping --if-match <revision>`, then Push. External document bodies remain in their backend.

Stores and caches live under `~/.local/share/lore/`; `LORE_HOME` overrides that location. `lore store create <name>` and `lore init --local-store <name>` select another store. Lore data stays outside the code checkout.

## Shared Knowledge

The initial Lore server uses authenticated HTTP for proposals and Git internally for history. Neither Git transport nor GitHub is required.

```bash
lore --json server config --repository "<repository-id>" --backend local --backend "<backend-id>" > server.json
lore server serve --config server.json --data-dir "<server-data-directory>"
```

Set the token environment variable specified in the configuration. Register each user and their repository/backend grants separately. The `local` backend grant permits Lore-owned document access. A store's `integration_policy` defaults to `conflict_free`; use `fast_forward_only` to accept new proposals only against the current central version. Neither requires an agent.

```bash
lore remote add team --url http://localhost:7766 --store "<store-id>"
lore remote auth set team --stdin
lore pull --remote team
lore push --remote team --document <document-id>
```

Remote Push submits a proposal; `lore proposal --remote team show <id>` inspects it and `integrate <id> --if-match <central-version>` accepts it. Remote Pull updates local central knowledge; working branches then Pull from it. Fetch observes remote knowledge without incorporating it.

Remote submission requires `--document`, `--mapping-id`, or explicit `--all`. Tokens and backend credentials stay outside history. Snapshot transfers are limited to 16 MiB. Server model reconciliation remains deferred; integration works without it.

Earlier mapping-only catalogs and `push <mapping-id>` remain compatibility operations. Use the commit/proposal workflow for new knowledge sharing.

## Mapping Artifacts

External-document mappings can be shared as JSON files through a separate Git repository without a Lore server. Configure the same backend identity on each machine and authenticate to the document source independently.

```bash
lore --central map export --all --output mappings.json
# Or select mappings with repeated --mapping-id <id> instead of --all.
lore --json map import --input mappings.json
lore commit <mapping-id> --mapping --if-match <returned-revision>
lore push
```

Export follows the selected knowledge view: `--central` exports accepted mappings; omitting it includes working changes. Artifacts preserve source store identities, mapping IDs, original anchors, scopes, and complete revision histories, including archived mappings. They contain captured code files and historical mapping context, but no document bodies or credentials. Use a private repository when these are confidential. Lore-owned document references are rejected; use knowledge sharing for those documents. Files are limited to 64 MiB.

Import validates the artifact against the checkout's code repository identity and records working changes. It does not authenticate to Notion, commit, or publish. Repeating an import is idempotent. Updates require the same source store, descendant source history, and unchanged local content; a conflict rejects the entire batch. A mapping omitted from a later artifact remains installed; archive it explicitly to distribute retirement. Import baselines belong to the destination store and working branch; local commit normalization does not change them.

Keep the target code repository identity, backend roots, and discovery settings alongside the mapping file in the sharing repository. Installation instructions should pin a Lore commit and provision credentials on each machine. Review import results, then commit and Push to make mappings available to central discovery in the editor and hooks.

## Documentation

- [Technical Overview (한국어)](docs/design.md)
- [Architecture](docs/architecture/)
- [Experience](docs/experience.md)
