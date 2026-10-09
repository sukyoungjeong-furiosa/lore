import * as vscode from 'vscode';
import * as fs from 'node:fs';
import { BackendConfiguration, BackendDocument, call, DocumentListing, LocatedMapping, LoreError, Retrieved } from './client';

export interface BackendEntry {
  root: vscode.WorkspaceFolder;
  backend: string;
  document?: BackendDocument;
  ancestorIds?: string[];
  label?: string;
  count?: number;
  cached?: boolean;
  warning?: boolean;
  related?: boolean;
}
export class Backends implements vscode.TreeDataProvider<BackendEntry>, vscode.Disposable {
  private event = new vscode.EventEmitter<BackendEntry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  currentFile = false;
  cached = false;
  private watchers = new Map<string, () => void>();
  editor: () => vscode.TextEditor | undefined = () => vscode.window.activeTextEditor;
  onExternalChange: () => void = () => this.refresh();
  refresh(): void { this.event.fire(undefined); }
  getTreeItem(entry: BackendEntry): vscode.TreeItem {
    const expandable = !entry.warning && !entry.label && !entry.related;
    const item = new vscode.TreeItem(entry.document?.title || entry.label || entry.backend, expandable ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
    item.id = `${entry.root.uri.toString()}:${entry.backend}:${(entry.ancestorIds || []).join('/')}:${entry.document?.reference.id || entry.label || 'root'}`;
    item.description = entry.document ? `${entry.count || 0} mappings${entry.cached ? ' · cached' : ''}` : undefined;
    item.iconPath = new vscode.ThemeIcon(entry.warning ? 'warning' : entry.document ? 'book' : 'cloud');
    if (entry.document) {
      item.tooltip = `${entry.document.title}\n${entry.backend}:${entry.document.reference.id}\n${entry.document.url}`;
      item.command = { command: 'lore.openBackend', title: 'Read backend document', arguments: [entry.document.reference, entry.root.uri.fsPath] };
    }
    return item;
  }
  async getChildren(entry?: BackendEntry): Promise<BackendEntry[]> {
    if (!entry) {
      const result: BackendEntry[] = [];
      for (const root of vscode.workspace.workspaceFolders || []) {
        try {
          try {
            const status = await call<{ state_path: string }>(root.uri.fsPath, ['status']);
            if (!this.watchers.has(status.state_path)) {
              const changed = () => this.onExternalChange();
              fs.watchFile(status.state_path, { interval: 800, persistent: false }, changed);
              this.watchers.set(status.state_path, () => fs.unwatchFile(status.state_path, changed));
            }
          } catch (error) { if (!(error instanceof LoreError && error.kind === 'not_initialized')) throw error; }
          const backends = await call<Record<string, BackendConfiguration>>(root.uri.fsPath, ['backend', 'list']);
          for (const backend of Object.keys(backends)) result.push({ root, backend });
        } catch (error) { result.push({ root, backend: '', label: (error as Error).message, warning: true }); }
      }
      if (!result.length && vscode.workspace.workspaceFolders?.[0]) result.push({ root: vscode.workspace.workspaceFolders[0], backend: '', label: 'Run Lore: Connect Notion to start', warning: true });
      return result;
    }
    if (entry.warning || entry.label || entry.related) return [];
    try {
      const path = entry.root.uri.fsPath;
      let mappings: LocatedMapping[] = [];
      const active = this.editor()?.document.uri;
      if (this.currentFile) {
        if (!active || active.scheme !== 'file' || vscode.workspace.getWorkspaceFolder(active)?.uri.toString() !== entry.root.uri.toString()) return [];
        mappings = (await call<{ mappings: LocatedMapping[] }>(path, ['map', 'ls', active.fsPath])).mappings;
        if (entry.document) return [];
        const refs = new Map<string, BackendEntry>();
        for (const { view } of mappings) {
          const ref = view.record.mapping.document;
          if (ref.backend !== entry.backend) continue;
          const previous = refs.get(ref.id);
          if (previous) { previous.count = (previous.count || 0) + 1; continue; }
          let document: BackendDocument = { reference: ref, title: ref.id, url: '', observed_revision: undefined };
          try { document = (await call<Retrieved<BackendDocument>>(path, ['docs', 'describe', ref.backend, ref.id, '--cached'])).value; } catch { /* A preserved mapping can outlive backend availability. */ }
          refs.set(ref.id, { ...entry, related: true, count: 1, document, cached: true });
        }
        return refs.size ? [...refs.values()] : [{ ...entry, label: 'No related workspace mappings (saved code)' }];
      }
      const args = ['docs', 'browse', entry.backend];
      if (entry.document) args.push(entry.document.reference.id);
      if (this.cached) args.push('--cached');
      const response = await call<Retrieved<DocumentListing>>(path, args);
      let mappingWarning: string | undefined;
      try { mappings = (await call<{ mappings: LocatedMapping[] }>(path, ['map', 'ls'])).mappings; }
      catch (error) { if (!(error instanceof LoreError && error.kind === 'not_initialized')) mappingWarning = `Mapping state unavailable: ${(error as Error).message}`; }
      const ancestors = [...(entry.ancestorIds || []), ...(entry.document ? [entry.document.reference.id] : [])];
      return [
        ...response.value.documents.filter(document => !ancestors.includes(document.reference.id)).map(document => ({
          root: entry.root, backend: entry.backend, document, ancestorIds: ancestors, cached: response.cached,
          count: mappings.filter(m => m.view.record.mapping.document.backend === entry.backend && m.view.record.mapping.document.id === document.reference.id).length,
        })),
        ...response.value.warnings.map(label => ({ root: entry.root, backend: entry.backend, label, warning: true })),
        ...(mappingWarning ? [{ root: entry.root, backend: entry.backend, label: mappingWarning, warning: true }] : []),
      ];
    } catch (error) { return [{ ...entry, document: undefined, label: (error as Error).message, warning: true }]; }
  }
  dispose(): void { for (const unwatch of this.watchers.values()) unwatch(); this.event.dispose(); }
}
