import { runTests } from '@vscode/test-electron';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createServer } from 'node:net';
import { createServer as createHttpServer } from 'node:http';
async function main() {
  delete process.env.ELECTRON_RUN_AS_NODE;
  const extension = path.resolve(__dirname, '../../..');
  const executable = path.resolve(extension, '../../target/debug/lore');
  const root = mkdtempSync(path.join(tmpdir(), 'lore-vscode-'));
  const workspace = path.join(root, 'code'); mkdirSync(workspace);
  const git = (args: string[]) => execFileSync('git', ['-C', workspace, ...args]);
  git(['init', '-q']); git(['config', 'user.email', 'test@example.invalid']); git(['config', 'user.name', 'Lore Test']);
  writeFileSync(path.join(workspace, 'scheduler.py'), '# Scheduler\ndef schedule():\n    return 42\n');
  git(['add', '.']); git(['commit', '-qm', 'initial']);
  const probe = createServer(); await new Promise<void>(resolve => probe.listen(0, '127.0.0.1', resolve));
  const port = (probe.address() as { port: number }).port; await new Promise<void>(resolve => probe.close(() => resolve()));
  const testEnv = { LORE_HOME: path.join(root, 'knowledge'), LORE_TEST_ROOT: root, LORE_TEST_EXECUTABLE: executable, LORE_TEST_PORT: String(port) };
  const rootId = 'b81457a4-a41d-45b4-9ae4-82d0c0e9e00c';
  const childId = '00000000-0000-4000-8000-000000000001';
  const nestedId = '00000000-0000-4000-8000-000000000002';
  let requestCount = 0; let backendVersion = 0;
  const notion = createHttpServer((request, response) => {
    const url = new URL(request.url!, 'http://localhost');
    if (url.pathname === '/test/state' || url.pathname === '/test/change') {
      if (url.pathname === '/test/change') backendVersion++;
      response.writeHead(200, { 'Content-Type': 'application/json' }); response.end(JSON.stringify({ requestCount, backendVersion })); return;
    }
    requestCount++;
    if (request.headers.authorization !== 'Bearer test-only-token') {
      response.writeHead(401, { 'Content-Type': 'application/json' }); response.end('{}'); return;
    }
    let value: unknown;
    const id = url.pathname.split('/')[3];
    if (url.pathname.match(/^\/v1\/pages\/[\w-]+$/)) {
      const title = id === rootId ? 'PR' : id === childId ? 'Notion Scheduler Design' : 'Nested Notion Design';
      value = { id, properties: { title: { type: 'title', title: [{ plain_text: title }] } }, last_edited_time: '2026-10-09T00:00:00Z', url: `https://www.notion.so/${id.replaceAll('-', '')}` };
    } else if (url.pathname.endsWith('/markdown')) {
      value = { markdown: `# External knowledge\nOwned by Notion. Backend update ${backendVersion}. [Related design](https://www.notion.so/${nestedId.replaceAll('-', '')})\n<script>window.injection = true</script>\n[unsafe](command:workbench.action.closeWindow)`, truncated: false, unknown_block_ids: [] };
    } else if (url.pathname.endsWith('/children')) {
      value = { results: id === rootId ? [{ id: childId, type: 'child_page', has_children: true, child_page: { title: 'Notion Scheduler Design' } }] : id === childId ? [{ id: nestedId, type: 'child_page', has_children: false, child_page: { title: 'Nested Notion Design' } }] : [], has_more: false, next_cursor: null };
    }
    response.writeHead(value ? 200 : 404, { 'Content-Type': 'application/json' }); response.end(JSON.stringify(value || {}));
  });
  await new Promise<void>(resolve => notion.listen(0, '127.0.0.1', resolve));
  Object.assign(testEnv, { LORE_TEST_NOTION_URL: `http://127.0.0.1:${(notion.address() as { port: number }).port}/v1`, LORE_TEST_NOTION_TOKEN: 'test-only-token' });
  try {
    await runTests({
      vscodeExecutablePath: process.env.LORE_VSCODE_EXECUTABLE, version: '1.95.3',
      extensionDevelopmentPath: extension, extensionTestsPath: path.join(extension, 'dist/test/test/extension.test.js'),
      launchArgs: [workspace, '--no-sandbox', '--disable-gpu', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes', '--user-data-dir', path.join(root, 'user'), '--extensions-dir', path.join(root, 'extensions'), `--remote-debugging-port=${port}`],
      extensionTestsEnv: testEnv,
    });
  } finally { await new Promise<void>(resolve => notion.close(() => resolve())); rmSync(root, { recursive: true, force: true }); }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
