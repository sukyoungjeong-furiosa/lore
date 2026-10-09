import * as vscode from 'vscode';
import { Anchor, call, LocatedMapping, Location, LoreError, Projection, View } from './client';
import { SelectedDocument } from './document-surface';

export interface MappingEntry {
  label: string;
  description?: string;
  detail?: string;
  selection?: SelectedDocument;
  anchor?: string;
  mapping?: string;
  location?: Location;
}
export class Mappings implements vscode.TreeDataProvider<MappingEntry>, vscode.Disposable {
  private event = new vscode.EventEmitter<MappingEntry | undefined>();
  readonly onDidChangeTreeData = this.event.event;
  selected?: SelectedDocument;
  select(document?: SelectedDocument): void { this.selected = document; this.refresh(); }
  refresh(): void { this.event.fire(undefined); }
  getTreeItem(entry: MappingEntry): vscode.TreeItem {
    const tree = new vscode.TreeItem(entry.label);
    tree.description = entry.description;
    tree.tooltip = entry.detail;
    if (entry.location) {
      tree.iconPath = new vscode.ThemeIcon('file-code');
      tree.command = { command: 'lore.openCodeLocation', title: 'Open connected code', arguments: [entry] };
    } else if (entry.anchor) tree.iconPath = new vscode.ThemeIcon('warning');
    return tree;
  }
  async getChildren(entry?: MappingEntry): Promise<MappingEntry[]> {
    if (entry) return [];
    const selected = this.selected;
    if (!selected) return [{ label: 'Select a document to see connected code' }];
    try {
      let result: MappingEntry[];
      if (selected.source.kind === 'backend') {
        const ref = selected.source.reference;
        const listed = await call<{ mappings: LocatedMapping[] }>(selected.root, ['map', 'ls']);
        const related = listed.mappings.filter(({ view }) => view.record.mapping.document.backend === ref.backend && view.record.mapping.document.id === ref.id);
        const located = await Promise.all(related.map(({ view }) => call<LocatedMapping>(selected.root, ['map', 'resolve', view.record.mapping.id])));
        result = located.flatMap(({ view, projections }) => [
          ...this.entries(selected, view.record.mapping.anchors, projections, view.record.mapping.context, view.record.mapping.id),
          ...(view.record.mapping.scopes || []).map(scope => ({ label: scope.glob, description: 'Code scope', detail: [scope.repository, view.record.mapping.context].filter(Boolean).join('\n') }))
        ]);
      } else {
        const args = ['show', selected.source.id]; if (selected.source.revision) args.push('--revision', selected.source.revision);
        const view = await call<View>(selected.root, args);
        const projections = selected.source.revision ? [] : (await call<{ projections: Projection[] }>(selected.root, ['resolve', selected.source.id])).projections;
        result = this.entries(selected, view.document.anchors, projections, '', undefined, !!selected.source.revision);
      }
      if (this.selected !== selected) return this.getChildren();
      return result.length ? result : [{ label: 'No connected code' }];
    } catch (error) {
      if (this.selected !== selected) return this.getChildren();
      return [{ label: error instanceof LoreError && error.kind === 'not_initialized' ? 'Initialize Lore workspace to see connected code' : 'Code connections unavailable', detail: (error as Error).message }];
    }
  }
  private entries(selection: SelectedDocument, anchors: Anchor[], projections: Projection[], context: string, mapping?: string, historical = false): MappingEntry[] {
    return anchors.flatMap<MappingEntry>(anchor => {
      const projection = projections.find(p => p.anchor === anchor.id);
      const original = `${anchor.path}:${anchor.start}-${anchor.end}`;
      const detail = [context, `Recorded: ${original}`, projection?.explanation].filter(Boolean).join('\n');
      if (!projection?.locations.length) return [{ label: original, description: historical ? 'Recorded location' : 'Location unresolved', detail, selection, anchor: anchor.id, mapping }];
      return projection.locations.map(location => ({ label: `${location.path}:${location.start}-${location.end}`, detail, selection, anchor: anchor.id, mapping, location }));
    });
  }
  async openLocation(entry: MappingEntry): Promise<void> {
    const selected = entry.selection;
    if (!selected || !entry.location || !entry.anchor) return;
    if (this.selected !== selected) throw new Error('The selected document changed. Choose its current code connection.');
    if (!entry.mapping && selected.source.kind !== 'local') return;
    const result = entry.mapping
      ? await call<LocatedMapping>(selected.root, ['map', 'resolve', entry.mapping])
      : await call<{ projections: Projection[] }>(selected.root, ['resolve', selected.source.kind === 'local' ? selected.source.id : '']);
    const location = result.projections.find(p => p.anchor === entry.anchor)?.locations.find(l => l.path === entry.location!.path && l.start === entry.location!.start && l.end === entry.location!.end);
    if (this.selected !== selected) return;
    if (!location) { this.refresh(); throw new Error('Code correspondence changed. Choose a refreshed code connection.'); }
    const document = await vscode.workspace.openTextDocument(vscode.Uri.joinPath(vscode.Uri.file(selected.root), location.path));
    if (document.isDirty) throw new Error('Save this code file before navigating to a recorded line range.');
    await vscode.window.showTextDocument(document, { viewColumn: vscode.ViewColumn.One, selection: new vscode.Range(location.start - 1, 0, location.end - 1, 0) });
  }
  dispose(): void { this.event.dispose(); }
}
