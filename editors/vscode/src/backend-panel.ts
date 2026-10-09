import * as vscode from 'vscode';
import { DocumentSurface } from './document-surface';
import MarkdownIt from 'markdown-it';
import { call, BackendConfiguration, DocumentContent, DocumentReference, Retrieved } from './client';
import { escape } from './markdown';

const reader = new MarkdownIt({ html: false, linkify: true });
reader.validateLink = url => !/^[a-z][a-z0-9+.-]*:/i.test(url) || /^(https?:|mailto:)/i.test(url);
reader.renderer.rules.image = (tokens, index) => `<span>[Image: ${escape(tokens[index].content || 'open in Notion')}]</span>`;
export class BackendPanel implements vscode.Disposable {
  private disposed = false;
  private sequence = 0;
  private current?: DocumentReference;
  private childDiscovery = false;
  private sourceName = '';
  private content?: Retrieved<DocumentContent>;
  constructor(private readonly root: string, private readonly surface: DocumentSurface, private readonly cached: () => boolean,
    private readonly changed: () => void) {}
  isDisposed(): boolean { return this.disposed; }
  async open(reference: DocumentReference): Promise<void> {
    await this.surface.select(this, message => this.handle(message));
    const sequence = ++this.sequence;
    try {
      const args = ['docs', 'show', reference.backend, reference.id]; if (this.cached()) args.push('--cached');
      const [content, configs] = await Promise.all([call<Retrieved<DocumentContent>>(this.root, args),
        call<Record<string, BackendConfiguration>>(this.root, ['backend', 'list'])]);
      if (sequence !== this.sequence || this.disposed || !this.surface.active(this)) return;
      reference = content.value.document.reference;
      this.current = content.value.document.reference; this.content = content;
      this.sourceName = configs[reference.backend]?.display_name || reference.backend;
      this.childDiscovery = !!configs[reference.backend]?.discovery_pages?.includes(reference.id);
      this.render();
    } catch (error) { if (sequence === this.sequence) this.surface.fail(this, (error as Error).message); throw error; }
  }
  private render(): void {
    if (!this.content) return;
    const { value } = this.content;
    const document = value.document;
    const modified = document.last_modified ? new Date(document.last_modified).toLocaleString() : 'Unknown';
    const user = (value: typeof document.created_by) => value ? value.name || value.id : 'Unknown';
    this.surface.post(this, { type: 'backendDocument', childDiscovery: this.childDiscovery, root: this.root, title: value.document.title, reference: value.document.reference,
      status: `Last modified: ${modified} · Created by: ${user(document.created_by)} · Last edited by: ${user(document.last_edited_by)} · ${this.sourceName}`,
      warnings: [...(value.partial ? ['Partial content. Open the original in Notion for the full document.'] : []), ...value.warnings],
      html: reader.render(value.content) }, `Document · ${value.document.title}`);
  }

  private async handle(message: Record<string, unknown>): Promise<void> {
    if (!this.current) return;
    const reference = this.current;
    const stillSelected = () => this.surface.active(this) && this.current === reference;
    if (message.action === 'fetch') {
      await vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: 'Lore: Fetching document' },
        () => call(this.root, ['docs', 'fetch', reference.backend, reference.id]));
      this.changed(); if (stillSelected()) await this.open(reference); return;
    }
    if (message.action === 'original') {
      const uri = vscode.Uri.parse(this.content!.value.document.url);
      if (uri.scheme === 'https') await vscode.env.openExternal(uri); return;
    }
    if (message.action === 'link' && typeof message.url === 'string') {
      const url = vscode.Uri.parse(new URL(message.url, this.content!.value.document.url).toString());
      if (url.scheme === 'https' && ['notion.so', 'www.notion.so', 'app.notion.com', 'www.notion.com'].includes(url.authority)) {
        await this.open({ backend: this.current.backend, id: url.toString() });
      } else if (['http', 'https', 'mailto'].includes(url.scheme)) await vscode.env.openExternal(url);
      return;
    }
  }

  dispose(): void { this.disposed = true; }
}
export function selectionLocation(editor: vscode.TextEditor): string {
  const selection = editor.selection;
  const end = !selection.isEmpty && selection.end.character === 0 ? selection.end.line : selection.end.line + 1;
  return `${editor.document.uri.fsPath}:${selection.start.line + 1}-${end}`;
}
