(() => {
  const api = window.loreApi;
  const $ = id => document.getElementById(id);
  const persisted = api.getState() || {};
  const drafts = persisted.drafts || {};
  let currentRoot;
  const key = (id, root = currentRoot) => `${root}:${id}`;
  let current, historical = false, busy = false, serial = Math.max(0, ...Object.values(drafts).map(d => d.serial || 0));
  const send = message => api.postMessage(message);
  const remember = () => api.setState({ drafts });
  function draft() { return current && drafts[key(current.document.id)]; }
  function buttons() {
    const d = draft(); const editing = d?.editing && !historical;
    $('reader').hidden = !!editing;
    $('editor').hidden = !editing;
    $('save').hidden = !editing; $('cancel').hidden = !editing;
    $('save').disabled = busy || !d?.dirty;
    $('draft-status').textContent = d?.dirty ? 'Unsaved changes.' : '';
    if (current) send({ type: 'localEditorState', root: currentRoot, id: current.document.id, editing: !!editing, dirty: !!d?.dirty, busy });
  }
  function backlink(document) { const a = window.document.createElement('a'); a.textContent = document.title; a.href = '#'; a.addEventListener('click', event => { event.preventDefault(); open(document.id); }); return a; }
  function open(id) { send({ type: 'open', id }); }
  window.addEventListener('message', event => {
    const m = event.data;
    if (m.type === 'localAction') {
      if (!current || m.root !== currentRoot || m.id !== current.document.id || $('local-mode').hidden) return;
      if (m.action === 'edit' && !historical && !busy) { draft().editing = true; buttons(); remember(); }
      else if (m.action === 'archive' && !historical && !busy && !draft()?.dirty) send({ type: 'archive', id: current.document.id, revision: current.revision });
      else if (m.action === 'history' && !busy) send({ type: 'history', id: current.document.id });
      return;
    }
    if (m.type === 'saved') {
      const d = drafts[key(m.id, m.root)];
      if (d) { if (d.serial === m.serial) { d.dirty = false; d.editing = false; } d.revision = m.revision; remember(); }
      busy = false; buttons(); return;
    }
    if (m.type === 'discarded') {
      delete drafts[key(m.id, m.root)]; remember();
      if (currentRoot === m.root && current?.document.id === m.id && !document.getElementById('local-mode').hidden) send({ type: 'refresh' });
      return;
    }
    if (m.type === 'error' && m.root !== currentRoot) return;
    if (m.type === 'error') { $('error').textContent = m.message; busy = false; buttons(); return; }
    if (m.type !== 'document') return;
    $('error').textContent = ''; busy = false;
    currentRoot = m.root;
    current = m.view; historical = !!m.historical;
    const id = current.document.id;
    if (!drafts[key(id)]) drafts[key(id)] = { title: current.document.title, content: current.document.content, directory: current.document.directory, revision: current.revision, dirty: false, editing: false, serial: 0 };
    const d = drafts[key(id)];
    if (d.directory === undefined) d.directory = current.document.directory;
    if (!historical && m.saved !== undefined) {
      if (d.serial === m.saved) { d.dirty = false; d.editing = false; }
      d.revision = m.savedRevision || current.revision;
    }
    if (!historical && !d.dirty) { d.title = current.document.title; d.content = current.document.content; d.directory = current.document.directory; d.revision = current.revision; }
    $('document').hidden = false;
    $('title').textContent = current.document.title;
    $('status').textContent = `Last modified: ${new Date(current.last_modified_ms).toLocaleString()} · Contributors: ${current.document.authorship.join(', ')} · Local Documents${current.document.archived ? ' · Archived' : ''}${historical ? ' · Historical revision (read only)' : ''}`;
    if (m.html !== undefined) $('reader').innerHTML = m.html;
    else { const pre = document.createElement('pre'); pre.textContent = current.document.content; $('reader').replaceChildren(pre); }
    $('edit-title').value = d.title; $('body').value = d.content; $('edit-directory').value = d.directory;
    $('backlinks').replaceChildren(...m.connections.backlinks.map(backlink));
    $('references').hidden = !m.connections.backlinks.length;
    buttons(); remember();
  });
  $('cancel').onclick = () => { if (!current) return; if (draft().dirty) send({ type: 'discard', id: current.document.id }); else { delete drafts[key(current.document.id)]; send({ type: 'refresh' }); remember(); } };
  function change() { if (!current || historical) return; const d = draft(); d.title = $('edit-title').value; d.content = $('body').value; d.directory = $('edit-directory').value; d.dirty = true; d.serial = ++serial; buttons(); remember(); }
  $('body').oninput = change; $('edit-title').oninput = change; $('edit-directory').oninput = change;
  $('save').onclick = () => { if (!current || busy || !draft().dirty) return; busy = true; buttons(); const d = draft(); send({ type: 'save', id: current.document.id, title: d.title, content: d.content, revision: d.revision, serial: d.serial, directory: d.directory }); };
  document.addEventListener('click', event => {
    const element = event.target.closest('[data-route], a[href]');
    if (!element?.closest('#local-mode')) return;
    if (element.closest('#backlinks')) return;
    if (element.tagName === 'A' && element.getAttribute('href').startsWith('#') && !element.dataset.route) return;
    event.preventDefault();
    if (element.dataset.route) send({ type: 'route', target: element.dataset.route });
    else send({ type: 'external', url: element.getAttribute('href') });
  });

})();
