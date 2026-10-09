import { spawn } from 'node:child_process';
import * as vscode from 'vscode';
export interface Anchor { id: string; path: string; start: number; end: number; context: { head?: string; working_tree: boolean } }
export interface Document { id: string; title: string; content: string; content_format: string; directory: string; authorship: string[]; archived: boolean; anchors: Anchor[] }
export interface View { revision: string; document: Document; last_modified_ms: number }
export interface Location { path: string; start: number; end: number }
export interface Projection { anchor: string; locations: Location[]; alternatives: Location[][]; explanation: string }
export interface Located { view: View; projections: Projection[] }
export interface Connections { outgoing: { target: string; label: string; matches: { id: string; title: string; archived: boolean }[] }[]; backlinks: { id: string; title: string; archived: boolean }[] }
export interface DocumentReference { backend: string; id: string }
export interface BackendUser { id: string; name?: string }
export interface BackendDocument { reference: DocumentReference; title: string; url: string; observed_revision?: string; last_modified?: string; created_by?: BackendUser; last_edited_by?: BackendUser }
export interface Retrieved<T> { cached: boolean; fetched_at_ms: number; value: T }
export interface DocumentListing { documents: BackendDocument[]; warnings: string[] }
export interface DocumentContent { document: BackendDocument; content: string; content_format: string; partial: boolean; warnings: string[] }
export interface MappingView { revision: string; record: { actor: string; at_ms: number; mapping: { id: string; document: DocumentReference; context: string; archived: boolean; anchors: Anchor[]; scopes?: { repository: string; glob: string }[] } } }
export interface LocatedMapping { view: MappingView; projections: Projection[] }
export interface BackendConfiguration { kind: string; display_name?: string; roots: string[]; discovery_pages: string[]; token_env: string; api_url: string }
let secretStorage: vscode.SecretStorage | undefined;
export function configureSecrets(storage: vscode.SecretStorage): void { secretStorage = storage; }
const migrations = new Map<string, Promise<void>>();
export async function migrateBackendTokens(root: string): Promise<void> {
  if (!secretStorage) return;
  const configs = await call<Record<string, BackendConfiguration>>(root, ['backend', 'list']);
  const home = vscode.workspace.getConfiguration('lore', vscode.Uri.file(root)).get<string>('dataHome') || process.env.LORE_HOME || '';
  for (const backend of Object.keys(configs)) {
    const key = `${home}:${backend}`;
    let migration = migrations.get(key);
    if (!migration) {
      migration = (async () => {
        const legacyKey = `lore.backend.${backend}.token`;
        const token = await secretStorage!.get(legacyKey);
        if (!token) return;
        const status = await call<{ stored: boolean }>(root, ['backend', 'auth', 'status', backend]);
        if (!status.stored) await call(root, ['backend', 'auth', 'set', backend, '--stdin'], token);
        await secretStorage!.delete(legacyKey);
      })();
      migrations.set(key, migration);
      migration.catch(() => migrations.delete(key));
    }
    await migration;
  }
}
export class LoreError extends Error {
  constructor(public readonly kind: string, message: string, public readonly result?: unknown) { super(message); }
}
export interface IndexProgress {
  phase: 'waiting' | 'starting' | 'traversing' | 'request' | 'cached' | 'finished';
  documents?: number; processed?: number; remaining?: number; title?: string; requests?: number; complete?: boolean;
}
export interface CallOptions { cancellation?: vscode.CancellationToken; onProgress?: (progress: IndexProgress) => void }
function cancelled(): LoreError { return new LoreError('cancelled', 'Lore operation cancelled'); }
async function cancellable<T>(operation: Promise<T>, token?: vscode.CancellationToken): Promise<T> {
  if (!token) return operation;
  if (token.isCancellationRequested) throw cancelled();
  let listener: vscode.Disposable | undefined;
  try {
    return await Promise.race([operation, new Promise<never>((_, reject) => { listener = token.onCancellationRequested(() => reject(cancelled())); })]);
  } finally { listener?.dispose(); }
}
export async function call<T>(root: string, args: string[], input?: string, options: CallOptions = {}): Promise<T> {
  const config = vscode.workspace.getConfiguration('lore', vscode.Uri.file(root));
  const env = { ...process.env };
  if (config.get<string>('dataHome')) env.LORE_HOME = config.get<string>('dataHome');
  if (options.cancellation?.isCancellationRequested) throw cancelled();
  if (args[0] === 'docs') await cancellable(migrateBackendTokens(root), options.cancellation);
  if (options.cancellation?.isCancellationRequested) throw cancelled();
  return new Promise((resolve, reject) => {
    const child = spawn(config.get<string>('executable') || 'lore', ['--repo', root, '--json', '--author', 'human', ...args], { env, windowsHide: true });
    let out = '', error = '', pendingError = ''; let size = 0; let settled = false;
    let stopped: LoreError | undefined;
    let killer: NodeJS.Timeout | undefined;
    let cancellation: vscode.Disposable | undefined;
    const finish = (failure?: Error, result?: T) => {
      if (settled) return;
      settled = true; clearTimeout(timer); if (killer) clearTimeout(killer); cancellation?.dispose();
      if (failure) reject(failure); else resolve(result as T);
    };
    const stop = (reason: LoreError) => {
      if (settled || stopped) return;
      stopped = reason; child.kill();
      killer = setTimeout(() => child.kill('SIGKILL'), 1000);
    };
    const consumeError = (line: string) => {
      try {
        const event = JSON.parse(line).progress as IndexProgress | undefined;
        if (event && ['waiting', 'starting', 'traversing', 'request', 'cached', 'finished'].includes(event.phase)) { options.onProgress?.(event); return; }
      } catch { /* Ordinary CLI errors remain available below. */ }
      error += line + '\n';
    };
    const timeout = args[0] === 'docs' ? (['index', 'fetch'].includes(args[1]) ? 15 * 60 * 1000 : 120000) : 30000;
    const timer = setTimeout(() => stop(new LoreError('unavailable', 'Lore CLI timed out')), timeout);
    cancellation = options.cancellation?.onCancellationRequested(() => stop(cancelled()));
    if (options.cancellation?.isCancellationRequested) stop(cancelled());
    child.stdout.on('data', chunk => {
      size += chunk.length;
      if (size > 32 * 1024 * 1024) stop(new LoreError('unsupported', 'Lore result exceeds 32 MB'));
      else out += chunk;
    });
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', (chunk: string) => {
      pendingError += chunk;
      for (;;) {
        const newline = pendingError.indexOf('\n'); if (newline < 0) break;
        consumeError(pendingError.slice(0, newline)); pendingError = pendingError.slice(newline + 1);
      }
    });
    child.on('error', e => finish(stopped || new LoreError('unavailable', `Cannot run Lore CLI: ${e.message}`)));
    child.on('close', code => {
      if (pendingError) consumeError(pendingError);
      if (stopped) { finish(stopped); return; }
      if (code !== 0) {
        try { const parsed = JSON.parse(error).error; finish(new LoreError(parsed.kind, parsed.message)); }
        catch {
          let result: unknown; try { result = JSON.parse(out); } catch { /* Partial JSON is not a completed result. */ }
          finish(new LoreError('unavailable', error.trim() || (result ? 'Some items could not be fetched; successful items remain cached.' : 'Lore CLI failed'), result));
        }
      } else {
        try { finish(undefined, JSON.parse(out) as T); }
        catch { finish(new LoreError('corrupt', 'Lore CLI returned invalid JSON')); }
      }
    });
    child.stdin.on('error', () => { /* Process errors are reported through the exit result. */ });
    child.stdin.end(input);
  });
}
