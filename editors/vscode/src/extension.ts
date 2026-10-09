import * as vscode from 'vscode';
import * as fs from 'node:fs';
import { call, configureSecrets, BackendConfiguration, DocumentReference, Located, LocatedMapping, LoreError, Retrieved, View } from './client';
import { KnowledgePanel } from './panel';
import { Backends } from './backends';
import { BackendPanel, selectionLocation } from './backend-panel';
import { DocumentSurface } from './document-surface';
import { Knowledge, KnowledgeEntry } from './knowledge';
import { Mappings, MappingEntry } from './mappings';
type Root = vscode.WorkspaceFolder;
interface Entry { label: string; root: Root; view?: View; children?: Entry[]; error?: string }
class Documents implements vscode.TreeDataProvider<Entry>, vscode.Disposable {
  private event = new vscode.EventEmitter<Entry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  currentFile = false;
  archived = false;
  private watchers = new Map<string, { dispose(): void }>();
  editor: () => vscode.TextEditor | undefined = () => vscode.window.activeTextEditor;
  onExternalChange: () => void = () => this.refresh();
  refresh(): void { this.event.fire(undefined); }
  getTreeItem(entry: Entry): vscode.TreeItem {
    const item = new vscode.TreeItem(entry.label, entry.children ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.None);
    if (entry.view) {
      const view = entry.view;
      item.description = `${view.curated ? 'curated' : 'uncurated'}${view.document.archived ? ' · archived' : ''}`;
      item.tooltip = `${view.document.title}\n${view.document.id}\n${view.document.authorship.join(', ')}`;
      item.command = { command: 'lore.open', title: 'Read document', arguments: [view.document.id, entry.root.uri.fsPath] };
      item.iconPath = new vscode.ThemeIcon(view.document.archived ? 'archive' : 'book');
    }
    if (entry.error === 'not_initialized') item.command = { command: 'lore.init', title: 'Initialize', arguments: [entry.root.uri.fsPath] };
    return item;
  }
  async getChildren(entry?: Entry): Promise<Entry[]> {
    if (entry) return entry.children || [];
    const result: Entry[] = [];
    for (const root of vscode.workspace.workspaceFolders || []) {
      try {
        const status = await call<{ state_path: string }>(root.uri.fsPath, ['status']);
        if (!this.watchers.has(status.state_path)) {
          const changed = () => this.onExternalChange(); fs.watchFile(status.state_path, { interval: 800, persistent: false }, changed);
          this.watchers.set(status.state_path, { dispose: () => fs.unwatchFile(status.state_path, changed) });
        }
        const active = this.editor()?.document.uri;
        const args = ['ls'];
        if (this.currentFile) {
          if (!active || vscode.workspace.getWorkspaceFolder(active)?.uri.toString() !== root.uri.toString() || active.scheme !== 'file') continue;
          args.push(active.fsPath);
        }
        if (this.archived) args.push('--include-archived');
        const response = await call<{ documents: Located[] }>(root.uri.fsPath, args);
        const groups = new Map<string, Entry[]>();
        for (const { view } of response.documents) {
          for (const tag of view.document.tags.length ? view.document.tags : ['']) {
            const docs = groups.get(tag) || []; docs.push({ label: view.document.title, root, view }); groups.set(tag, docs);
          }
        }
        const children = [...groups].sort(([a], [b]) => a.localeCompare(b)).map(([tag, docs]) => ({ label: tag === '' ? 'Untagged' : '#' + tag, root, children: docs }));
        if (!children.length) children.push({ label: this.currentFile ? 'No related local documents (saved code)' : 'No local documents', root, children: [] });
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
  const backends = new Backends();
  const panels = new Map<string, KnowledgePanel>();
  const backendPanels = new Map<string, BackendPanel>();
  let lastEditor = vscode.window.activeTextEditor;
  documents.editor = backends.editor = () => lastEditor;
  const lenses = new vscode.EventEmitter<void>();
  const surface = new DocumentSurface(context.extensionUri);
  const knowledge = new Knowledge(backends, documents);
  const mappings = new Mappings();
  const tree = vscode.window.createTreeView('lore.knowledge', { treeDataProvider: knowledge });
  const mappingTree = vscode.window.createTreeView('lore.mappings', { treeDataProvider: mappings });
  const refresh = () => {
    documents.refresh(); backends.refresh(); knowledge.refresh(); mappings.refresh(); lenses.fire();
    tree.description = `${documents.currentFile ? 'Current file · saved code' : 'Cache first'}${documents.archived ? ' · archived included' : ''}`;
    mappingTree.description = mappings.selected?.title;
  };
  context.subscriptions.push(documents, backends, knowledge, mappings, tree, mappingTree, lenses, surface,
    surface.onDidChangeDocument(document => { mappings.select(document); mappingTree.description = document?.title; }),
    vscode.window.registerWebviewViewProvider('lore.reader', surface, { webviewOptions: { retainContextWhenHidden: true } }));
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
  async function open(id: string, root: string) {
    let panel = panels.get(root);
    if (!panel || panel.isDisposed()) { panel = new KnowledgePanel(root, surface, refresh); panels.set(root, panel); context.subscriptions.push(panel); }
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
    return names.length === 1 ? names[0] : vscode.window.showQuickPick(names, { placeHolder: 'Select a knowledge backend' });
  }
  function command(name: string, handler: (...args: any[]) => Promise<unknown> | unknown) {
    context.subscriptions.push(vscode.commands.registerCommand(name, async (...args: unknown[]) => {
      try { return await handler(...args); } catch (error) { void vscode.window.showErrorMessage(`Lore: ${(error as Error).message}`); throw error; }
    }));
  }
  command('lore.init', async (path?: string) => { const root = await rootFor(path); await call(root, ['init']); refresh(); });
  command('lore.connectNotion', async () => {
    const root = await rootFor();
    const name = await vscode.window.showInputBox({ prompt: 'Stable backend identity (e.g. furiosa). Keep it when credentials change.' }); if (!name) return;
    const page = await vscode.window.showInputBox({ prompt: 'Root Notion page URL or ID' }); if (!page) return;
    await call(root, ['backend', 'add-notion', name, '--root', page]);
    refresh(); await vscode.commands.executeCommand('lore.setBackendToken', name, root);
  });
  command('lore.setBackendToken', async (name?: string, path?: string) => {
    const root = await rootFor(path); const backend = name || await chooseBackend(root); if (!backend) return;
    const token = await vscode.window.showInputBox({ prompt: 'Notion connection token or personal access token. Stored in VS Code SecretStorage.', password: true, ignoreFocusOut: true });
    if (!token) return;
    await context.secrets.store(`lore.backend.${backend}.token`, token.trim()); refresh();
  });
  command('lore.openBackend', async (reference: DocumentReference, path?: string) => {
    if (!reference || typeof reference.backend !== 'string' || typeof reference.id !== 'string') return;
    await openBackend(reference, await rootFor(path));
  });
  command('lore.reloadDocument', () => surface.invokeBackendAction('fetch'));
  command('lore.openDocumentInNotion', () => surface.invokeBackendAction('original'));
  command('lore.openCodeLocation', (entry: MappingEntry) => mappings.openLocation(entry));
  async function fetchBackend(entry?: KnowledgeEntry, withContent = false) {
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
  command('lore.fetchBackend', (entry?: KnowledgeEntry) => fetchBackend(entry));
  command('lore.fetchBackendContent', (entry?: KnowledgeEntry) => fetchBackend(entry, true));
  command('lore.indexBackend', async () => {
    const root = await rootFor(); const backend = await chooseBackend(root); if (!backend) return;
    const result = await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Lore: Indexing Notion titles' }, () => call<Retrieved<{ documents: unknown[]; complete: boolean; warnings: string[] }>>(root, ['docs', 'index', backend]));
    void vscode.window.showInformationMessage(`Lore: Indexed ${result.value.documents.length} titles${result.value.complete ? '' : ' · incomplete; some pages could not be traversed'}`); refresh();
  });
  command('lore.searchBackend', async () => {
    const root = await rootFor(); const backend = await chooseBackend(root); if (!backend) return;
    const query = await vscode.window.showInputBox({ prompt: 'Search cached Notion titles. Run Lore: Index Notion Titles first.' }); if (query === undefined) return;
    const results = await call<Retrieved<{ documents: { reference: DocumentReference; title: string }[]; complete: boolean }>>(root, ['docs', 'search', backend, query]);
    const chosen = await vscode.window.showQuickPick(results.value.documents.map(document => ({ label: document.title, description: document.reference.id, reference: document.reference })),
      { placeHolder: `Cached title index${results.value.complete ? '' : ' · incomplete'} · ${new Date(results.fetched_at_ms).toLocaleString()}` });
    if (chosen) await openBackend(chosen.reference, root);
  });
  command('lore.attachNotion', async () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.uri.scheme !== 'file' || editor.document.isDirty) throw new Error('Select code in a saved file first.');
    const root = await rootFor(); const backend = await chooseBackend(root); if (!backend) return;
    const location = selectionLocation(editor);
    const page = await vscode.window.showInputBox({ prompt: 'Notion page URL or ID to link to this code' }); if (!page) return;
    const context = await vscode.window.showInputBox({ prompt: 'Why is this document related to the selected code? (optional)' }); if (context === undefined) return;
    await call(root, ['map', 'create', backend, page, location, '--context', context]);
    refresh(); await openBackend({ backend, id: page }, root);
  });
  command('lore.create', async (path?: string) => {
    const root = await rootFor(path);
    const title = await vscode.window.showInputBox({ prompt: 'Document title' }); if (!title) return;
    const view = await call<View>(root, ['create', title]); refresh(); await open(view.document.id, root);
  });
  command('lore.open', async (id: string, path?: string) => { if (typeof id !== 'string') return; await open(id, await rootFor(path)); });
  command('lore.search', async (path?: string) => {
    const root = await rootFor(path);
    const query = await vscode.window.showInputBox({ prompt: 'Search local Lore documents' }); if (query === undefined) return;
    const results = await call<{ id: string; title: string; curated: boolean; snippet: string }[]>(root, ['search', query]);
    if (!results.length) { void vscode.window.showInformationMessage('Lore: No matching local documents'); return; }
    const chosen = await vscode.window.showQuickPick(results.map(d => ({ label: d.title, description: d.curated ? 'curated' : 'uncurated', detail: d.snippet, id: d.id })), { placeHolder: 'Read knowledge' });
    if (chosen) await open(chosen.id, root);
  });
  command('lore.attach', async () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.uri.scheme !== 'file') throw new Error('Select code in a saved file first');
    if (editor.document.isDirty) throw new Error('Save the code file before attaching. This milestone captures code on disk.');
    const root = await rootFor();
    const docs = await call<{ documents: Located[] }>(root, ['ls']);
    const chosen = await vscode.window.showQuickPick(docs.documents.map(d => ({ label: d.view.document.title, id: d.view.document.id, revision: d.view.revision })), { placeHolder: 'Attach a document to this code selection' });
    if (!chosen) return;
    const selection = editor.selection;
    const end = !selection.isEmpty && selection.end.character === 0 ? selection.end.line : selection.end.line + 1;
    await call(root, ['attach', chosen.id, `${editor.document.uri.fsPath}:${selection.start.line + 1}-${end}`, '--if-match', chosen.revision]);
    refresh(); await open(chosen.id, root);
  });
  command('lore.refresh', refresh);
  command('lore.toggleCurrentFile', () => { documents.currentFile = !documents.currentFile; backends.currentFile = documents.currentFile; refresh(); });
  command('lore.toggleCached', () => { backends.cached = !backends.cached; refresh(); });
  command('lore.toggleArchived', () => { documents.archived = !documents.archived; refresh(); });
  context.subscriptions.push(vscode.window.onDidChangeActiveTextEditor(editor => { if (editor) lastEditor = editor; refresh(); }), vscode.workspace.onDidSaveTextDocument(refresh), vscode.workspace.onDidChangeTextDocument(() => lenses.fire()), vscode.workspace.onDidChangeWorkspaceFolders(refresh), vscode.workspace.onDidChangeConfiguration(e => { if (e.affectsConfiguration('lore')) refresh(); }));
  context.subscriptions.push(vscode.languages.registerCodeLensProvider({ scheme: 'file' }, {
    onDidChangeCodeLenses: lenses.event,
    async provideCodeLenses(document) {
      if (document.isDirty || !vscode.workspace.getConfiguration('lore').get('codeLens')) return [];
      const root = vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath; if (!root) return [];
      try {
        const [result, mapped] = await Promise.all([
          call<{ documents: Located[] }>(root, ['ls', document.uri.fsPath]),
          call<{ mappings: LocatedMapping[] }>(root, ['map', 'ls', document.uri.fsPath]),
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
  refresh();
  return { refresh, open, openBackend, tree: documents, backends, panels, backendPanels, knowledge, mappings, surface };
}
