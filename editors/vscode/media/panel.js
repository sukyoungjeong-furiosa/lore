(() => {
  const api = window.loreApi;
  const $ = id => document.getElementById(id);
  const persisted = api.getState() || {};
  const drafts = persisted.drafts || {};
  const trail = [];
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
    $('edit').hidden = !!editing || historical;
    $('save').hidden = !editing; $('cancel').hidden = !editing;
    $('save').disabled = busy || !d?.dirty;
    $('archive').disabled = busy || !!d?.dirty || historical;
    $('curate').disabled = busy || !!d?.dirty || historical;
    $('draft-status').textContent = d?.dirty ? 'Local editor draft. Save records a new workspace revision.' : 'No unsaved changes.';
  }
  function button(label, handler) { const b = document.createElement('button'); b.textContent = label; b.addEventListener('click', handler); return b; }
  function list(id, elements) { $(id).replaceChildren(...(elements.length ? elements : [document.createTextNode('None available')])); }
  function open(id) { send({ type: 'open', id }); }
  window.addEventListener('message', event => {
    const m = event.data;
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
    if (m.type === 'results' && m.root !== currentRoot) return;
    if (m.type === 'results') {
      $('results').hidden = false;
      list('results', m.results.map(d => button(`${d.title} · ${d.curated ? 'curated' : 'uncurated'}${d.archived ? ' · archived' : ''}`, () => open(d.id)))); return;
    }
    if (m.type !== 'document') return;
    $('error').textContent = ''; busy = false;
    if (currentRoot !== m.root) trail.length = 0;
    else if (current && current.document.id !== m.view.document.id) trail.push(current.document.id);
    currentRoot = m.root;
    current = m.view; historical = !!m.historical;
    const id = current.document.id;
    if (!drafts[key(id)]) drafts[key(id)] = { title: current.document.title, content: current.document.content, tags: current.document.tags.slice(), revision: current.revision, dirty: false, editing: false, serial: 0 };
    const d = drafts[key(id)];
    if (!d.tags) d.tags = current.document.tags.slice();
    if (!historical && m.saved !== undefined) {
      if (d.serial === m.saved) { d.dirty = false; d.editing = false; }
      d.revision = m.savedRevision || current.revision;
    }
    if (!historical && !d.dirty) { d.title = current.document.title; d.content = current.document.content; d.tags = current.document.tags.slice(); d.revision = current.revision; }
    $('document').hidden = false;
    $('title').textContent = current.document.title;
    $('status').textContent = `${current.curated ? 'Human curated' : 'Uncurated'} · ${current.document.authorship.join(', ')} · ${current.document.archived ? 'Archived' : 'Active'} · ${current.document.tags.map(t => '#' + t).join(' ')}${historical ? ' · Historical revision (read only)' : ''}`;
    if (m.html !== undefined) $('reader').innerHTML = m.html;
    else { const pre = document.createElement('pre'); pre.textContent = current.document.content; $('reader').replaceChildren(pre); }
    $('edit-title').value = d.title; $('body').value = d.content;
    $('archive').textContent = current.document.archived ? 'Restore' : 'Archive';
    const anchors = m.projections.flatMap(p => {
      const locations = p.locations.length ? p.locations : p.alternatives.flat();
      if (!locations.length) { const text = document.createElement('p'); text.textContent = p.explanation; return [text]; }
      return locations.map(l => button(`${l.path}:${l.start}-${l.end}${p.alternatives.length ? ' (alternative)' : ''}`, () => send({ type: 'code', anchor: p.anchor, ...l })));
    });
    list('anchors', anchors);
    list('links', m.connections.outgoing.map(l => button(`${l.label}${l.matches.length ? '' : ' (unavailable)'}`, () => send({ type: 'route', target: l.target }))));
    list('backlinks', m.connections.backlinks.map(l => button(l.title, () => open(l.id))));
    renderTags(); buttons(); remember();
  });
  $('edit').onclick = () => { if (!current) return; draft().editing = true; buttons(); remember(); };
  $('cancel').onclick = () => { if (!current) return; if (draft().dirty) send({ type: 'discard', id: current.document.id }); else { delete drafts[key(current.document.id)]; send({ type: 'refresh' }); remember(); } };
  function change() { if (!current || historical) return; const d = draft(); d.title = $('edit-title').value; d.content = $('body').value; d.dirty = true; d.serial = ++serial; buttons(); remember(); }
  function renderTags() {
    const d = draft(); if (!d) return;
    $('edit-tags').replaceChildren(...d.tags.map(tag => button('#' + tag + ' ×', () => { d.tags = d.tags.filter(t => t !== tag); change(); renderTags(); })));
  }
  function addTag() {
    const tag = $('tag-input').value.trim(); if (!tag || !draft()) return;
    if (!draft().tags.includes(tag)) { draft().tags.push(tag); change(); renderTags(); }
    $('tag-input').value = '';
  }
  $('add-tag').onclick = addTag;
  $('tag-input').onkeydown = event => { if (event.key === 'Enter') { event.preventDefault(); addTag(); } };
  $('body').oninput = change; $('edit-title').oninput = change;
  $('save').onclick = () => { if (!current || busy || !draft().dirty) return; busy = true; buttons(); const d = draft(); send({ type: 'save', id: current.document.id, title: d.title, content: d.content, revision: d.revision, serial: d.serial, tags: d.tags.slice() }); };
  $('refresh').onclick = () => send({ type: 'refresh' });
  $('archive').onclick = () => { if (!current || draft()?.dirty || historical) return; send({ type: 'archive', id: current.document.id, revision: current.revision }); };
  $('curate').onclick = () => { if (!current || draft()?.dirty || historical) return; send({ type: 'curate', id: current.document.id, revision: current.revision }); };
  $('history').onclick = () => { if (current) send({ type: 'history', id: current.document.id }); };
  $('new').onclick = () => send({ type: 'create' });
  $('back').onclick = () => { const id = trail.pop(); if (id) { current = undefined; open(id); } };
  $('search-form').onsubmit = event => { event.preventDefault(); send({ type: 'search', query: $('query').value, curation: $('curation').value, archived: $('include-archived').checked }); };
  document.addEventListener('click', event => {
    const element = event.target.closest('[data-route], a[href]');
    if (!element?.closest('#local-mode')) return;
    if (!element) return;
    event.preventDefault();
    if (element.dataset.route) send({ type: 'route', target: element.dataset.route });
    else send({ type: 'external', url: element.getAttribute('href') });
  });

})();
