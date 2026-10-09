(() => {
  const api = acquireVsCodeApi();
  const filter = document.getElementById('knowledge-filter');
  const query = document.getElementById('knowledge-query');
  const regex = document.getElementById('knowledge-regex');
  const error = document.getElementById('filter-error');
  const tree = document.getElementById('knowledge-tree');
  const treeError = document.getElementById('tree-error');
  const empty = document.getElementById('knowledge-empty');
  let sequence = 0;
  let regularExpression = false;
  let rows = [];
  let selected;
  let focused;
  const changed = () => api.postMessage({ type: 'filter', query: query.value, regex: regularExpression, sequence: ++sequence });
  query.addEventListener('input', changed);
  regex.addEventListener('click', () => {
    regularExpression = !regularExpression;
    regex.setAttribute('aria-pressed', String(regularExpression));
    changed(); query.focus();
  });
  const toggle = row => api.postMessage({ type: 'expand', id: row.id, expanded: !row.expanded });
  const activate = row => {
    selected = row.id; selectRows();
    if (row.openable) api.postMessage({ type: 'open', id: row.id });
    else if (row.expandable) toggle(row);
  };
  const selectRows = () => {
    for (const element of tree.children) element.setAttribute('aria-selected', String(element.dataset.id === selected));
  };
  const icons = {
    cloud: '<path d="M4 12a3 3 0 0 1 0-6 4 4 0 0 1 8-1 3.5 3.5 0 0 1 0 7Z"/>',
    warning: '<path d="m8 1 7 13H1Z"/><path d="M8 5v4m0 2v1"/>',
    book: '<path d="M8 3C5 1 2 2 1 2v11c2-1 4-1 7 1 3-2 5-2 7-1V2c-2-1-5-1-7 1Zm0 0v11"/>',
    folder: '<path d="M1 4h5l2 2h7v7H1Zm0 0V2h5l2 2h7v2"/>',
    archive: '<path d="M1 2h14v3H1Zm1 3v9h12V5M6 8h4"/>',
  };
  const icon = name => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('viewBox', '0 0 16 16'); svg.setAttribute('aria-hidden', 'true');
    svg.innerHTML = icons[name] || icons.folder;
    return svg;
  };
  const render = () => {
    const active = tree.contains(document.activeElement);
    const elements = rows.map(row => {
      const element = document.createElement('div'); element.className = 'row'; element.dataset.id = row.id;
      element.style.paddingLeft = `${8 + row.depth * 16}px`; element.tabIndex = -1;
      element.setAttribute('role', 'treeitem'); element.setAttribute('aria-level', String(row.depth + 1));
      element.setAttribute('aria-label', row.label); element.setAttribute('aria-selected', String(row.id === selected));
      element.title = row.tooltip || row.label;
      element.dataset.vscodeContext = JSON.stringify({ webviewSection: row.context, loreEntryId: row.id, preventDefaultContextMenuItems: true });
      if (row.expandable) element.setAttribute('aria-expanded', String(row.expanded));
      const twistie = document.createElement('button'); twistie.className = 'twistie'; twistie.tabIndex = -1;
      twistie.textContent = row.expandable ? (row.expanded ? '⌄' : '›') : '';
      twistie.disabled = !row.expandable; twistie.setAttribute('aria-label', row.expanded ? 'Collapse' : 'Expand');
      twistie.addEventListener('click', event => { event.stopPropagation(); element.focus(); toggle(row); }); element.append(twistie, icon(row.icon));
      const label = document.createElement('span'); label.className = 'label'; label.textContent = row.label; element.append(label);
      if (row.description) { const description = document.createElement('span'); description.className = 'description'; description.textContent = row.description; element.append(description); }
      if (['lore.backend', 'lore.backendDocument'].includes(row.context)) {
        const fetch = document.createElement('button'); fetch.className = 'fetch'; fetch.textContent = '↓'; fetch.title = 'Fetch from Backend'; fetch.setAttribute('aria-label', 'Fetch from Backend');
        fetch.addEventListener('click', event => { event.stopPropagation(); element.focus(); api.postMessage({ type: 'fetch', id: row.id }); }); element.append(fetch);
      }
      element.addEventListener('click', () => { element.focus(); activate(row); });
      element.addEventListener('focus', () => {
        focused = row.id;
        for (const element of tree.children) element.tabIndex = element.dataset.id === focused ? 0 : -1;
      });
      return element;
    });
    tree.replaceChildren(...elements);
    const target = elements.find(element => element.dataset.id === focused) || elements[0];
    if (target) { target.tabIndex = 0; if (active) target.focus(); }
    empty.hidden = rows.length !== 0; treeError.hidden = true;
  };
  tree.addEventListener('keydown', event => {
    if (event.target instanceof HTMLButtonElement) return;
    const index = rows.findIndex(row => row.id === focused); const row = rows[index]; if (!row) return;
    let target;
    if (event.key === 'ArrowDown') target = tree.children[Math.min(rows.length - 1, index + 1)];
    else if (event.key === 'ArrowUp') target = tree.children[Math.max(0, index - 1)];
    else if (event.key === 'Home') target = tree.firstElementChild;
    else if (event.key === 'End') target = tree.lastElementChild;
    else if (event.key === 'ArrowRight') {
      if (row.expandable && !row.expanded) toggle(row);
      else if (row.expanded) target = tree.children[index + 1];
    } else if (event.key === 'ArrowLeft') {
      if (row.expanded) toggle(row);
      else { for (let i = index - 1; i >= 0; i--) if (rows[i].depth < row.depth) { target = tree.children[i]; break; } }
    } else if (event.key === 'Enter' || event.key === ' ') activate(row);
    else return;
    event.preventDefault();
    if (target) { for (const element of tree.children) element.tabIndex = -1; target.tabIndex = 0; target.focus(); }
  });
  window.addEventListener('message', ({ data }) => {
    if (data.type === 'focus') { query.focus(); return; }
    if (data.type === 'tree') { rows = data.rows; render(); return; }
    if (data.type === 'treeError') { treeError.textContent = data.message; treeError.hidden = false; return; }
    if (data.type !== 'state') return;
    filter.hidden = !data.visible;
    if ((data.sequence !== undefined && data.sequence !== sequence) || (data.sequence === undefined && sequence !== 0)) return;
    if (query.value !== data.query) query.value = data.query;
    regularExpression = data.regex; regex.setAttribute('aria-pressed', String(regularExpression));
    query.setAttribute('aria-invalid', String(!!data.error)); error.hidden = !data.error; error.textContent = data.error || '';
  });
  api.postMessage({ type: 'ready' });
})();
