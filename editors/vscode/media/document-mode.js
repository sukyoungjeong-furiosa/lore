(() => {
  const modes = { document: 'local-mode', backendDocument: 'backend-mode' };
  window.addEventListener('message', event => {
    const m = event.data;
    if (m.type === 'surfaceLoading' || m.type === 'surfaceError') {
      for (const id of ['local-mode', 'backend-mode']) document.getElementById(id).hidden = true;
      const empty = document.getElementById('empty-mode'); empty.hidden = false;
      empty.textContent = m.type === 'surfaceLoading' ? 'Loading…' : m.message;
      return;
    }
    if (modes[m.type]) for (const id of ['empty-mode', 'local-mode', 'backend-mode']) document.getElementById(id).hidden = id !== modes[m.type];
  });
  window.loreApi.postMessage({ type: 'surfaceReady' });
})();
