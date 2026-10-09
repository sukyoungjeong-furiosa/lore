import MarkdownIt from 'markdown-it';
export const markdown = new MarkdownIt({ html: false, linkify: true });
export function escape(value: string): string { return markdown.utils.escapeHtml(value); }
markdown.inline.ruler.before('link', 'lore_wiki', (state, silent) => {
  if (!state.src.startsWith('[[', state.pos)) return false;
  const end = state.src.indexOf(']]', state.pos + 2);
  if (end < 0) return false;
  const inner = state.src.slice(state.pos + 2, end);
  if (inner.includes('\n')) return false;
  const divider = inner.indexOf('|');
  const target = (divider < 0 ? inner : inner.slice(0, divider)).trim();
  const label = (divider < 0 ? inner : inner.slice(divider + 1)).trim();
  if (!target) return false;
  if (!silent) { const token = state.push('lore_wiki', '', 0); token.content = label; token.meta = { target }; }
  state.pos = end + 2; return true;
});
markdown.renderer.rules.lore_wiki = (tokens, index) => `<button class="wiki" data-route="${escape(tokens[index].meta.target)}">${escape(tokens[index].content)}</button>`;
