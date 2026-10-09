(() => {
  const api = window.loreApi;
  window.addEventListener('message', event => {
    const m = event.data; if (m.type !== 'backendDocument') return;
    document.getElementById('backend-title').textContent = m.title;
    document.getElementById('backend-status').textContent = m.status;
    document.getElementById('backend-reader').innerHTML = m.html;
    document.getElementById('backend-warnings').replaceChildren(...m.warnings.map(warning => {
      const p = document.createElement('p'); p.setAttribute('role', 'alert'); p.textContent = warning; return p;
    }));
  });
  document.getElementById('backend-mode').addEventListener('click', event => {
    const element = event.target.closest('button[data-action], a[href]'); if (!element) return;
    if (element.tagName === 'A' && element.getAttribute('href').startsWith('#')) return;
    event.preventDefault();
    if (element.tagName === 'A') api.postMessage({ action: 'link', url: element.getAttribute('href') });
    else api.postMessage({ action: element.dataset.action, map: element.dataset.map, revision: element.dataset.revision,
      anchor: element.dataset.anchor, path: element.dataset.path, start: Number(element.dataset.start) });
  });
})();
