// Searching messages, as Discord's search box does it: words, plus from:, mentions:, has:, in:,
// before:, after:, during: and pinned:. Names are looked up here; Rust gets ids and times only
// (main.rs `search`, search.rs), and refuses encrypted DMs.

import { h, icon, ICONS } from './dom.js';
import { findUser } from './people.js';
import { miniMessage } from './threads.js';

const HAS = ['link', 'embed', 'file', 'image', 'video', 'sound', 'sticker', 'poll'];
const FILTER = /(^|\s)(from|mentions|has|in|before|after|during|pinned):("[^"]*"|\S+)/gi;

// "2026-10-30" (local time) in ms, or null.
function day(text) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!m) return null;
  const at = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])).getTime();
  return Number.isNaN(at) ? null : at;
}

// The text in the box as a query for Rust, or { error }. channels: [{ id, name }] for in:.
export function parseQuery(text, channels) {
  const q = { author_id: [], mentions: [], has: [], channel_id: [] };
  const problems = [];
  const content = String(text || '').replace(FILTER, (_, lead, key, raw) => {
    const value = raw.replace(/^"|"$/g, '');
    const k = key.toLowerCase();
    if (k === 'from' || k === 'mentions') {
      const u = /^\d{1,20}$/.test(value) ? { id: value } : findUser(value);
      if (u) q[k === 'from' ? 'author_id' : 'mentions'].push(u.id);
      else problems.push('No one called “' + value + '” seen yet');
    } else if (k === 'has') {
      if (HAS.includes(value.toLowerCase())) q.has.push(value.toLowerCase());
      else problems.push('has: takes ' + HAS.join(', '));
    } else if (k === 'in') {
      const name = value.replace(/^#/, '').toLowerCase();
      const c = (channels || []).find((x) => (x.name || '').toLowerCase() === name);
      if (c) q.channel_id.push(c.id);
      else problems.push('No channel #' + name + ' here');
    } else if (k === 'pinned') {
      q.pinned = value.toLowerCase() !== 'false';
    } else {
      const at = day(value);
      if (at === null) problems.push(k + ': takes a date like 2026-10-30');
      else if (k === 'before') q.before = at;
      else if (k === 'after') q.after = at + 24 * 3600 * 1000;
      else q.during = at;
    }
    return lead;
  }).trim();
  if (problems.length) return { error: problems.join('. ') + '.' };
  if (content) q.content = content;
  return q;
}

// The results panel. s: { text, results, total, has_more, loading, error }.
// ctx: { md, channelName(id), open(m), more(), close() }
export function renderResults(panel, s, ctx) {
  panel.textContent = '';
  const head = h('div', { class: 'search-head' },
    h('span', null, s.loading && !s.results.length ? 'Searching…' : s.error ? 'Search' : s.total + (s.total === 1 ? ' result' : ' results')),
    h('button', { type: 'button', class: 'panel-button', title: 'Close search', 'aria-label': 'Close search', onclick: ctx.close }, icon(ICONS.close, 16)));
  panel.appendChild(head);
  if (s.error) { panel.appendChild(h('div', { class: 'threads-empty' }, s.error)); return; }
  if (!s.results.length && !s.loading) panel.appendChild(h('div', { class: 'threads-empty' }, 'No results. Try other words, or fewer filters.'));
  let last = null;
  for (const m of s.results) {
    if (m.channel_id !== last) {
      last = m.channel_id;
      const name = ctx.channelName(m.channel_id);
      if (name) panel.appendChild(h('div', { class: 'search-channel' }, name));
    }
    panel.appendChild(miniMessage(m, ctx.md, null, () => ctx.open(m)));
  }
  if (s.has_more) panel.appendChild(h('button', { type: 'button', class: 'button secondary small forum-more', disabled: !!s.loading, onclick: ctx.more }, s.loading ? 'Loading…' : 'More results'));
}

// The search box in the header: made once and moved between headers, so what's typed survives
// redraws. onSearch(text) on Enter; Escape clears it.
export function createSearchBox(onSearch, onClear) {
  const input = h('input', { class: 'search-input', type: 'search', placeholder: 'Search', 'aria-label': 'Search', spellcheck: false });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); onSearch(input.value); }
    else if (e.key === 'Escape') { input.value = ''; onClear(); }
  });
  const node = h('div', { class: 'search-box', title: 'Words, and from:name  mentions:name  has:image  in:channel  before:/after:/during:2026-10-30  pinned:true' }, input, icon(ICONS.search, 16));
  return { node, input };
}
