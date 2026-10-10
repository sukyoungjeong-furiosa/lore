import * as vscode from 'vscode';
import * as fs from 'node:fs';
import { call, configureSecrets, migrateBackendTokens, BackendConfiguration, BackendDocument, DocumentReference, Located, LocatedMapping, LoreError, Retrieved, View, IndexProgress } from './client';
import { KnowledgePanel } from './panel';
import { Backends } from './backends';
import { BackendPanel, selectionLocation } from './backend-panel';
import { DocumentSurface, SelectedDocument } from './document-surface';
import { Knowledge, KnowledgeEntry } from './knowledge';
import { KnowledgeView } from './knowledge-view';
import { Mappings, MappingEntry } from './mappings';
import { registerResolverCommands } from './resolver';
type Root = vscode.WorkspaceFolder;
interface Entry { label: string; root: Root; view?: View; children?: Entry[]; error?: string }
class Documents implements vscode.TreeDataProvider<Entry>, vscode.Disposable {
  private event = new vscode.EventEmitter<Entry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  archived = false;
  private watchers = new Map<string, { dispose(): void }>();
  onExternalChange: () => void = () => this.refresh();
  refresh(): void { this.event.fire(undefined); }
  getTreeItem(entry: Entry): vscode.TreeItem {
    const item = new vscode.TreeItem(entry.label, entry.children ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
    if (entry.view) {
      const view = entry.view;
      item.description = view.document.archived ? 'archived' : undefined;
      item.tooltip = `${view.document.title}\n${view.document.id}\n${view.document.authorship.join(', ')}`;
      item.command = { command: 'lore.open', title: 'Read document', arguments: [view.document.id, entry.root.uri.fsPath] };
      item.iconPath = new vscode.ThemeIcon(view.document.archived ? 'archive' : 'book');
    } else if (entry.children) item.iconPath = new vscode.ThemeIcon('folder');
    if (entry.error === 'not_initialized') item.command = { command: 'lore.init', title: 'Initialize', arguments: [entry.root.uri.fsPath] };
    return item;
  }
  async getChildren(entry?: Entry): Promise<Entry[]> {
    if (entry) return entry.children || [];
    const result: Entry[] = [];
    for (const root of vscode.workspace.workspaceFolders || []) {
      try {
        const status = await call<{ state_path: string; document_store_state_path?: string | null }>(root.uri.fsPath, ['status']);
        for (const path of [status.state_path, status.document_store_state_path].filter((path): path is string => !!path)) {
          if (!this.watchers.has(path)) {
            const changed = () => this.onExternalChange(); fs.watchFile(path, { interval: 800, persistent: false }, changed);
            this.watchers.set(path, { dispose: () => fs.unwatchFile(path, changed) });
          }
        }
        const args = ['--central', 'ls'];
        if (this.archived) args.push('--include-archived');
        const response = await call<{ documents: Located[] }>(root.uri.fsPath, args);
        const children: Entry[] = [];
        const folders = new Map<string, Entry>();
        for (const { view } of response.documents) {
          let entries = children; let directory = '';
          for (const name of view.document.directory.split('/').filter(Boolean)) {
            directory = directory ? `${directory}/${name}` : name;
            let folder = folders.get(directory);
            if (!folder) { folder = { label: name, root, children: [] }; folders.set(directory, folder); entries.push(folder); }
            entries = folder.children!;
          }
          entries.push({ label: view.document.title, root, view });
        }
        const sort = (entries: Entry[]) => {
          entries.sort((a, b) => Number(!!b.children) - Number(!!a.children) || a.label.localeCompare(b.label));
          for (const entry of entries) if (entry.children) sort(entry.children);
        };
        sort(children);
        if (!children.length) children.push({ label: 'No local documents', root, children: [] });
        if ((vscode.workspace.workspaceFolders?.length || 0) > 1) result.push({ label: root.name, root, children });
        else result.push(...children);
      } catch (error) {
        result.push({ label: error instanceof LoreError && error.kind === 'not_initialized' ? 'Initialize Lore workspace' : `Lore unavailable: ${(error as Error).message}`, root, error: error instanceof LoreError ? error.kind : 'unavailable' });
      }
    }
    return result;
  }
  dispose(): void { for (const watcher of this.watchers.values()) watcher.dispose(); this.event.dispose(); }
}
export function activate(context: vscode.ExtensionContext) {
  configureSecrets(context.secrets);
  const documents = new Documents();
  documents.archived = context.workspaceState.get<boolean>('includeArchived', false);
  const backends = new Backends();
  const panels = new Map<string, KnowledgePanel>();
  const backendPanels = new Map<string, BackendPanel>();
  let lastEditor = vscode.window.activeTextEditor;
  const lenses = new vscode.EventEmitter<void>();
  const surface = new DocumentSurface(context.extensionUri);
  const knowledge = new Knowledge(backends, documents);
  const mappings = new Mappings();
  const tree = new KnowledgeView(context, knowledge);
  const mappingTree = vscode.window.createTreeView('lore.mappings', { treeDataProvider: mappings });
  const updateFilterDescription = () => {
    tree.description = [knowledge.filter ? `${knowledge.regex ? 'Regex' : 'Filter'}: ${knowledge.filter}` : '', documents.archived ? 'Archived included' : ''].filter(Boolean).join(' · ');
    void vscode.commands.executeCommand('setContext', 'lore.includeArchived', documents.archived);
  };
  const refresh = () => {
    documents.refresh(); backends.refresh(); knowledge.refresh(); mappings.refresh(); lenses.fire();
    updateFilterDescription();
  };
  context.subscriptions.push(vscode.window.registerWebviewViewProvider('lore.knowledge', tree, { webviewOptions: { retainContextWhenHidden: true } }));
  context.subscriptions.push(documents, backends, knowledge, mappings, tree, mappingTree, lenses, surface,
    surface.onDidChangeDocument(document => mappings.select(document)),
    mappings.onDidChangeSummary(() => { mappingTree.description = mappings.description; }),
    vscode.window.registerWebviewViewProvider('lore.reader', surface, { webviewOptions: { retainContextWhenHidden: true } }));
  context.subscriptions.push(knowledge.onDidChangeTreeData(updateFilterDescription));
  documents.onExternalChange = refresh;
  backends.onExternalChange = refresh;
  async function rootFor(path?: string): Promise<string> {
    if (path) return path;
    const active = lastEditor?.document.uri;
    const associated = active && vscode.workspace.getWorkspaceFolder(active);
    if (associated) return associated.uri.fsPath;
    const roots = vscode.workspace.workspaceFolders || [];
    if (roots.length === 1) return roots[0].uri.fsPath;
    const root = await vscode.window.showQuickPick(roots.map(r => ({ label: r.name, root: r.uri.fsPath })), { placeHolder: 'Select a code workspace' });
    if (!root) throw new Error('Open or select a code workspace first');
    return root.root;
  }
  async function open(id: string, root: string, working = false) {
    let panel = panels.get(root);
    if (!panel || panel.isDisposed()) { panel = new KnowledgePanel(root, surface, refresh); panels.set(root, panel); context.subscriptions.push(panel); }
    if (working) panel.markWorking(id);
    await panel.open(id);
  }
  async function openBackend(reference: DocumentReference, root: string) {
    let panel = backendPanels.get(root);
    if (!panel || panel.isDisposed()) {
      panel = new BackendPanel(root, surface, () => backends.cached, refresh);
      backendPanels.set(root, panel); context.subscriptions.push(panel);
    }
    await panel.open(reference);
  }
  async function chooseBackend(root: string): Promise<string | undefined> {
    const configs = await call<Record<string, BackendConfiguration>>(root, ['backend', 'list']);
    const names = Object.keys(configs);
    if (!names.length) throw new Error('Run Lore: Connect Notion first.');
    return names.length === 1 ? names[0] : vscode.window.showQuickPick(names, { placeHolder: 'Select a document backend' });
  }
  function command(name: string, handler: (...args: any[]) => Promise<unknown> | unknown) {
    context.subscriptions.push(vscode.commands.registerCommand(name, async (...args: unknown[]) => {
      try { return await handler(...args); } catch (error) { void vscode.window.showErrorMessage(`Lore: ${(error as Error).message}`); throw error; }
    }));
  }
  command('lore.init', async (path?: string) => { const root = await rootFor(path); await call(root, ['init']); refresh(); });
  registerResolverCommands(command, rootFor);
  command('lore.connectNotion', async () => {
    const root = await rootFor();
    const name = await vscode.window.showInputBox({ prompt: 'Document backend ID (e.g. notion). Document references use this ID.' }); if (!name) return;
    const page = await vscode.window.showInputBox({ prompt: 'Root Notion page URL or ID' }); if (!page) return;
    await call(root, ['backend', 'add-notion', name, '--root', page]);
    refresh(); await vscode.commands.executeCommand('lore.setBackendToken', name, root);
  });
  command('lore.setBackendToken', async (name?: string, path?: string) => {
    const root = await rootFor(path); const backend = name || await chooseBackend(root); if (!backend) return;
    const token = await vscode.window.showInputBox({ prompt: 'Notion token. Shared with Lore CLI and agent hooks on this machine.', password: true, ignoreFocusOut: true });
    if (!token) return;
    await call(root, ['backend', 'auth', 'set', backend, '--stdin'], token);
    await context.secrets.delete(`lore.backend.${backend}.token`); refresh();
  });
  command('lore.openBackend', async (reference: DocumentReference, path?: string) => {
    if (!reference || typeof reference.backend !== 'string' || typeof reference.id !== 'string') return;
    await openBackend(reference, await rootFor(path));
  });
  command('lore.reloadDocument', () => surface.reload());
  command('lore.publishDocument', () => surface.invokeLocalAction('publish'));
  command('lore.editDocument', () => surface.invokeLocalAction('edit'));
  command('lore.archiveDocument', () => surface.invokeLocalAction('archive'));
  command('lore.restoreDocument', () => surface.invokeLocalAction('archive'));
  command('lore.documentHistory', () => surface.invokeLocalAction('history'));
  command('lore.openDocumentInNotion', () => surface.invokeBackendAction('original'));
  command('lore.openCodeLocation', (entry: MappingEntry) => mappings.openLocation(entry));
  async function childDiscovery(enabled?: boolean, entry?: KnowledgeEntry | { loreEntryId: string }) {
    if (entry && 'loreEntryId' in entry) {
      const current = tree.entry(entry.loreEntryId);
      if (!current) throw new Error('Select a current knowledge entry first.');
      entry = current;
    }
    const source = entry?.kind === 'backend' ? entry.entry : undefined;
    const selected = mappings.selected;
    const reference = source?.document?.reference || (selected?.source.kind === 'backend' ? selected.source.reference : undefined);
    const root = source?.root.uri.fsPath || selected?.root;
    if (!reference || !root) throw new Error('Select a backend document first.');
    const configs = await call<Record<string, BackendConfiguration>>(root, ['backend', 'list']);
    enabled ??= !configs[reference.backend]?.discovery_pages?.includes(reference.id);
    await call(root, ['docs', 'discovery', reference.backend, reference.id, enabled ? '--enable' : '--disable']);
    const current = mappings.selected;
    if (current?.root === root && current.source.kind === 'backend' && current.source.reference.backend === reference.backend && current.source.reference.id === reference.id)
      surface.setChildDiscovery(enabled);
    knowledge.clearBrowsedTitles(); refresh();
  }
  command('lore.enableChildDiscovery', (entry?: KnowledgeEntry | { loreEntryId: string }) => childDiscovery(true, entry));
  command('lore.disableChildDiscovery', (entry?: KnowledgeEntry | { loreEntryId: string }) => childDiscovery(false, entry));
  command('lore.toggleChildDiscovery', () => childDiscovery());

  async function fetchBackend(entry?: KnowledgeEntry | { loreEntryId: string }, withContent = false) {
    if (entry && 'loreEntryId' in entry) {
      const selected = tree.entry(entry.loreEntryId);
      if (!selected) throw new Error('Select a current knowledge entry first.');
      entry = selected;
    }
    const source = entry?.kind === 'backend' ? entry.entry : undefined;
    const root = await rootFor(source?.root.uri.fsPath);
    const backend = source?.backend || await chooseBackend(root); if (!backend) return;
    const args = ['docs', 'fetch', backend];
    if (source?.document) args.push(source.document.reference.id); else if (withContent) args.push('--with-content');
    try {
      const result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: `Lore: Fetching ${withContent ? 'documents' : source?.document ? 'document' : 'tree and titles'}` }, () => call<Retrieved<{ complete?: boolean; warnings: string[] }> & { results?: { status: string }[] }>(root, args));
      if (result.value.complete === false || result.value.warnings?.length) void vscode.window.showWarningMessage(`Lore: Fetch completed with warnings. ${result.value.warnings.join(' ')}`);
    } catch (error) {
      if (error instanceof LoreError && error.result) void vscode.window.showWarningMessage(`Lore: ${error.message}`); else throw error;
    } finally { refresh(); }
  }
  command('lore.fetchBackend', (entry?: KnowledgeEntry | { loreEntryId: string }) => fetchBackend(entry));
  command('lore.fetchBackendContent', (entry?: KnowledgeEntry | { loreEntryId: string }) => fetchBackend(entry, true));
  command('lore.indexBackend', async () => {
    const root = await rootFor(); const backend = await chooseBackend(root); if (!backend) return;
    const result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Lore: Indexing Notion titles' }, () => call<Retrieved<{ documents: unknown[]; complete: boolean; warnings: string[] }>>(root, ['docs', 'index', backend]));
    void vscode.window.showInformationMessage(`Lore: Indexed ${result.value.documents.length} titles${result.value.complete ? '' : ' · incomplete; some pages could not be traversed'}`); refresh();
  });
  command('lore.create', async (path?: string) => {
    const root = await rootFor(path);
    const title = await vscode.window.showInputBox({ prompt: 'Document title' }); if (!title) return;
    const view = await call<View>(root, ['create', title]); refresh(); await open(view.document.id, root, true);
  });
  command('lore.open', async (id: string, path?: string) => { if (typeof id !== 'string') return; await open(id, await rootFor(path)); });
  command('lore.search', () => tree.toggleFilter());
  command('lore.attach', async () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.uri.scheme !== 'file' || editor.document.isDirty) throw new Error('Select code in a saved file first.');
    const root = vscode.workspace.getWorkspaceFolder(editor.document.uri)?.uri.fsPath;
    if (!root) throw new Error('Select code in an open workspace first.');
    const location = selectionLocation(editor);
    let selected = mappings.selected?.root === root ? mappings.selected : undefined;
    if (!selected) {
      const choices: (vscode.QuickPickItem & { source?: SelectedDocument['source']; manual?: boolean })[] = [];
      try {
        const local = await call<{ documents: Located[] }>(root, ['--central', 'ls']);
        choices.push(...local.documents.map(({ view }) => ({ label: view.document.title, description: 'Local Documents', source: { kind: 'local' as const, id: view.document.id } })));
      } catch (error) { if (!(error instanceof LoreError && error.kind === 'not_initialized')) throw error; }
      const configs = await call<Record<string, BackendConfiguration>>(root, ['backend', 'list']);
      for (const backend of Object.keys(configs)) {
        try {
          const index = await call<Retrieved<{ documents: BackendDocument[] }>>(root, ['docs', 'search', backend, '']);
          choices.push(...index.value.documents.map(document => ({ label: document.title, description: backend, source: { kind: 'backend' as const, reference: document.reference } })));
        } catch (error) { if (!(error instanceof LoreError && error.kind === 'not_cached')) throw error; }
      }
      if (Object.keys(configs).length) choices.push({ label: 'Enter a Notion page URL or ID…', manual: true });
      if (!choices.length) throw new Error('Create or select a document in Lore first.');
      const chosen = await vscode.window.showQuickPick(choices, { placeHolder: 'Link a document to the selected code' });
      if (!chosen) return;
      let source = chosen.source;
      if (chosen.manual) {
        const backend = await chooseBackend(root); if (!backend) return;
        const page = await vscode.window.showInputBox({ prompt: 'Notion page URL or ID' }); if (!page) return;
        source = { kind: 'backend', reference: { backend, id: page } };
      }
      if (!source) return;
      selected = { root, title: chosen.label, source };
    }
    if (selected.source.kind === 'local') {
      if (selected.source.revision) throw new Error('Open the current document revision before linking code.');
      if (!selected.source.working) { await call(root,['pull']); await call(root,['checkout',selected.source.id]); }
      const view = await call<View>(root, ['show', selected.source.id]);
      await call(root, ['attach', view.document.id, location, '--if-match', view.revision]);
      refresh(); await open(view.document.id, root, true);
    } else {
      const reference = selected.source.reference;
      const pending = await call<{ unsubmitted_documents: string[]; unsubmitted_mappings: string[] }>(root,['worktree']);
      if (pending.unsubmitted_documents.length || pending.unsubmitted_mappings.length) throw new Error('This branch has committed changes awaiting submission. Inspect and submit them with the Lore CLI before creating a shared code link.');
      const created=await call<import('./client').MappingView>(root, ['map', 'create', reference.backend, reference.id, location]);
      await call(root,['commit',created.record.mapping.id,'--mapping','--if-match',created.revision]);
      await call(root,['push']);
      refresh(); await openBackend(reference, root);
    }
  });
  command('lore.attachNotion', () => vscode.commands.executeCommand('lore.attach'));
  let refreshing: Promise<void> | undefined;
  async function refreshLists(): Promise<void> {
    const failures: string[] = [];
    const seen = new Set<string>();
    try {
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Lore: Refreshing configured discovery pages', cancellable: true }, async (progress, cancellation) => {
        for (const folder of vscode.workspace.workspaceFolders || []) {
          if (cancellation.isCancellationRequested) return;
          const root = folder.uri.fsPath;
          const home = vscode.workspace.getConfiguration('lore', folder.uri).get<string>('dataHome') || process.env.LORE_HOME || '';
          if (seen.has(home)) continue;
          seen.add(home);
          try {
            progress.report({ message: 'Checking connected backends' });
            const configs = await call<Record<string, BackendConfiguration>>(root, ['backend', 'list'], undefined, { cancellation });
            for (const [backend, config] of Object.entries(configs)) {
              if (cancellation.isCancellationRequested) return;
              const started = Date.now();
              let phase = 'Connecting'; let requests = 0; let processed: number | undefined; let documents: number | undefined;
              const report = () => progress.report({ message: [config.display_name || backend, `${(config.discovery_pages || []).length} discovery page${config.discovery_pages?.length === 1 ? '' : 's'}`, phase, requests ? `${requests} API requests` : '', processed !== undefined ? `${processed} pages checked` : '', documents !== undefined ? `${documents} discovered` : '', `${Math.floor((Date.now() - started) / 1000)}s`].filter(Boolean).join(' · ') });
              const onProgress = (event: IndexProgress) => {
                if (event.phase !== 'request') phase = { waiting: 'Waiting for shared refresh', starting: 'Reading configured pages', traversing: event.title ? `Checking ${event.title}` : 'Checking pages', cached: 'Using shared result', finished: event.complete ? 'Finished' : 'Finished with warnings' }[event.phase];
                if (event.requests !== undefined) requests = event.requests;
                if (event.processed !== undefined) processed = event.processed;
                if (event.documents !== undefined) documents = event.documents;
                report();
              };
              report(); const timer = setInterval(report, 1000);
              try {
                const result = await call<Retrieved<{ warnings: string[] }>>(root, ['docs', 'fetch', backend, '--progress'], undefined, { cancellation, onProgress });
                failures.push(...result.value.warnings.map(warning => `${backend}: ${warning}`));
              } catch (error) {
                if (error instanceof LoreError && error.kind === 'cancelled') return;
                failures.push(`${backend}: ${(error as Error).message}`);
              } finally { clearInterval(timer); }
            }
          } catch (error) {
            if (error instanceof LoreError && error.kind === 'cancelled') return;
            failures.push(`${folder.name}: ${(error as Error).message}`);
          }
        }
      });
    } finally { refresh(); }
    if (failures.length) void vscode.window.showWarningMessage(`Lore: ${failures.join(' ')}`);
  }
  command('lore.refresh', () => {
    if (!refreshing) refreshing = refreshLists().finally(() => { refreshing = undefined; });
    return refreshing;
  });
  command('lore.refreshMappings', () => mappings.refresh());
  command('lore.toggleCached', () => { backends.cached = !backends.cached; refresh(); });
  command('lore.toggleArchived', async () => { documents.archived = !documents.archived; await context.workspaceState.update('includeArchived', documents.archived); refresh(); });
  context.subscriptions.push(vscode.window.onDidChangeActiveTextEditor(editor => { if (editor) lastEditor = editor; refresh(); }), vscode.workspace.onDidSaveTextDocument(refresh), vscode.workspace.onDidChangeTextDocument(() => lenses.fire()), vscode.workspace.onDidChangeWorkspaceFolders(refresh), vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('lore')) refresh(); }));
  context.subscriptions.push(vscode.languages.registerCodeLensProvider({ scheme: 'file' }, {
    onDidChangeCodeLenses: lenses.event,
    async provideCodeLenses(document) {
      if (document.isDirty || !vscode.workspace.getConfiguration('lore').get('codeLens')) return [];
      const root = vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath; if (!root) return [];
      try {
        const [result, mapped] = await Promise.all([
          call<{ documents: Located[] }>(root, ['--central', 'ls', document.uri.fsPath]),
          call<{ mappings: LocatedMapping[] }>(root, ['--central', 'map', 'ls', document.uri.fsPath]),
        ]);
        const localLenses = result.documents.flatMap(d => d.projections.flatMap(p => p.locations.filter(l => vscode.Uri.joinPath(vscode.Uri.file(root), l.path).fsPath === document.uri.fsPath && l.start <= document.lineCount).map(l => new vscode.CodeLens(new vscode.Range(l.start - 1, 0, l.start - 1, 0), { command: 'lore.open', title: `Lore: ${d.view.document.title}`, arguments: [d.view.document.id, root] }))));
        const backendLenses = mapped.mappings.flatMap(({ view, projections }) => projections.flatMap(p => p.locations.filter(l => vscode.Uri.joinPath(vscode.Uri.file(root), l.path).fsPath === document.uri.fsPath && l.start <= document.lineCount).map(l => new vscode.CodeLens(new vscode.Range(l.start - 1, 0, l.start - 1, 0), {
          command: 'lore.openBackend', title: `Lore: ${view.record.mapping.context || view.record.mapping.document.backend}`, arguments: [view.record.mapping.document, root],
        }))));
        return [...localLenses, ...backendLenses];
      } catch (error) {
        if (!(error instanceof LoreError && error.kind === 'not_initialized')) output.appendLine((error as Error).message);
        return [];
      }
    },
  }));
  const output = vscode.window.createOutputChannel('Lore'); context.subscriptions.push(output);
  for (const root of vscode.workspace.workspaceFolders || []) {
    void migrateBackendTokens(root.uri.fsPath).then(refresh, () => output.appendLine('Stored backend token migration failed. Use Lore: Set Notion Connection Token to retry.'));
  }
  refresh();
  return { refresh, open, openBackend, tree: documents, backends, panels, backendPanels, knowledge, mappings, surface };
}
