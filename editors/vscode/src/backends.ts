import * as vscode from 'vscode';
import * as fs from 'node:fs';
import { BackendConfiguration, BackendDocument, call, DocumentListing, LoreError, Retrieved } from './client';

export interface BackendEntry {
  root: vscode.WorkspaceFolder;
  backend: string;
  displayName?: string;
  document?: BackendDocument;
  ancestorIds?: string[];
  discoveryPages?: string[];
  label?: string;
  warning?: boolean;
}
export class Backends implements vscode.TreeDataProvider<BackendEntry>, vscode.Disposable {
  private event = new vscode.EventEmitter<BackendEntry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  cached = false;
  private watchers = new Map<string, () => void>();
  onExternalChange: () => void = () => this.refresh();
  refresh(): void { this.event.fire(undefined); }
  getTreeItem(entry: BackendEntry): vscode.TreeItem {
    const expandable = !entry.warning && !entry.label && (!entry.document || this.childDiscovery(entry));
    const item = new vscode.TreeItem(entry.document?.title || entry.label || entry.displayName || entry.backend, expandable ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
    item.id = `${entry.root.uri.toString()}:${entry.backend}:${(entry.ancestorIds || []).join('/')}:${entry.document?.reference.id || entry.label || 'root'}`;
    item.iconPath = new vscode.ThemeIcon(entry.warning ? 'warning' : entry.document ? 'book' : 'cloud');
    if (entry.document) {
      item.tooltip = `${entry.document.title}\n${entry.backend}:${entry.document.reference.id}\n${entry.document.url}`;
      item.command = { command: 'lore.openBackend', title: 'Read backend document', arguments: [entry.document.reference, entry.root.uri.fsPath] };
    }
    return item;
  }
  childDiscovery(entry: BackendEntry): boolean {
    return !!entry.document && !!entry.discoveryPages?.includes(entry.document.reference.id);
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
          for (const [backend, config] of Object.entries(backends)) result.push({ root, backend, displayName: config.display_name, discoveryPages: config.discovery_pages || [] });
        } catch (error) { result.push({ root, backend: '', label: (error as Error).message, warning: true }); }
      }
      if (!result.length && vscode.workspace.workspaceFolders?.[0]) result.push({ root: vscode.workspace.workspaceFolders[0], backend: '', label: 'Run Lore: Connect Notion to start', warning: true });
      return result;
    }
    if (entry.warning || entry.label || (entry.document && !this.childDiscovery(entry))) return [];
    try {
      const path = entry.root.uri.fsPath;
      const args = ['docs', 'browse', entry.backend];
      if (entry.document) args.push(entry.document.reference.id);
      if (this.cached) args.push('--cached');
      const response = await call<Retrieved<DocumentListing>>(path, args);
      const ancestors = [...(entry.ancestorIds || []), ...(entry.document ? [entry.document.reference.id] : [])];
      return [
        ...response.value.documents.filter(document => !ancestors.includes(document.reference.id)).map(document => ({
          root: entry.root, backend: entry.backend, document, ancestorIds: ancestors, discoveryPages: entry.discoveryPages,
        })),
        ...response.value.warnings.map(label => ({ root: entry.root, backend: entry.backend, label, warning: true })),
      ];
    } catch (error) { return [{ ...entry, document: undefined, label: (error as Error).message, warning: true }]; }
  }
  dispose(): void { for (const unwatch of this.watchers.values()) unwatch(); this.event.dispose(); }
}
