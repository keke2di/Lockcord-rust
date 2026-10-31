// The composer's autocomplete: @people (and roles, @everyone, @here), #channels and :emoji:,
// suggested at the caret while typing. Up/Down pick, Tab or Enter insert, Escape closes.
//
// In a server, people not seen yet are searched through the gateway (main.rs member_search, as
// Discord's app does), and only there: in a DM, an encrypted DM or a secret chat nothing typed
// leaves for a search; those only suggest the people of that DM.

import { api, on } from './api.js';
import { h } from './dom.js';
import { avatar, displayName } from './people.js';
import { SHORTCODES } from './emoji.js';

const MAX_SHOWN = 10;
const SEARCH_DELAY_MS = 300;
const TEXTUAL = new Set([0, 2, 5, 15, 16]);

// ctx: { where() -> { channel, guild (its view) | null, dm (the conversation), private }, known(guild)
//        -> [{ user, nick }] seen this session, emojiUrl(id, animated) }
export function attachAutocomplete(input, ctx) {
  const box = h('div', { class: 'autocomplete', role: 'listbox', hidden: true });
  let items = [];
  let index = 0;
  let token = null; // { kind, query, start, end }
  let timer = null;
  let nonce = 0;
  const found = new Map(); // guild id -> Map(user id -> { user, nick }) from searches

  on('lc://members-chunk', (chunk) => {
    const map = found.get(chunk.guild_id) || new Map();
    for (const m of chunk.members || []) if (m.user && m.user.id) map.set(m.user.id, m);
    found.set(chunk.guild_id, map);
    if (token && token.kind === '@') update();
  });

  // The word being typed at the caret, if it starts with @, # or : (two letters for emoji).
  function current() {
    const end = input.selectionStart;
    if (end !== input.selectionEnd) return null;
    const before = input.value.slice(0, end);
    const m = /(^|[\s(])([@#:])([^\s@#:<>]{0,32})$/.exec(before);
    if (!m) return null;
    if (m[2] === ':' && m[3].length < 2) return null;
    return { kind: m[2], query: m[3].toLowerCase(), start: end - m[3].length - 1, end };
  }

  const matches = (q, ...names) => names.some((n) => n && n.toLowerCase().includes(q));
  const startsFirst = (q) => (a, b) => (b.sort.startsWith(q) - a.sort.startsWith(q)) || a.sort.localeCompare(b.sort);

  function people(w, q) {
    const out = new Map();
    const add = (user, nick) => {
      if (!user || !user.id || out.has(user.id)) return;
      if (matches(q, nick, user.global_name, user.username)) out.set(user.id, { user, nick });
    };
    if (w.guild) {
      for (const m of ctx.known(w.guild)) add(m.user, m.nick);
      for (const m of (found.get(w.guild.id) || new Map()).values()) add(m.user, m.nick);
    } else {
      for (const u of (w.dm && w.dm.recipients) || []) add(u, null);
    }
    return [...out.values()].map((m) => ({
      kind: 'person', sort: (m.nick || displayName(m.user)).toLowerCase(), insert: '<@' + m.user.id + '>',
      draw: () => [avatar(m.user, 'ac-avatar'), h('span', { class: 'ac-name' }, m.nick || displayName(m.user)), h('span', { class: 'ac-sub' }, m.user.username || '')],
    })).sort(startsFirst(q));
  }

  function extras(w, q) {
    if (!w.guild) return [];
    const channel = (w.guild.channels || []).find((c) => c.id === w.channel);
    const everyone = !!(channel && channel.can_mention_everyone);
    const out = [];
    for (const r of w.guild.roles || []) {
      if (r.id === w.guild.id || !(r.mentionable || everyone) || !matches(q, r.name)) continue;
      out.push({ kind: 'role', sort: r.name.toLowerCase(), insert: '<@&' + r.id + '>', draw: () => [h('span', { class: 'ac-glyph' }, '@'), h('span', { class: 'ac-name' }, r.name), h('span', { class: 'ac-sub' }, 'role')] });
    }
    if (everyone) {
      for (const [name, sub] of [['everyone', 'everyone in this channel'], ['here', 'everyone online here']]) {
        if (name.startsWith(q)) out.push({ kind: 'all', sort: name, insert: '@' + name, draw: () => [h('span', { class: 'ac-glyph' }, '@'), h('span', { class: 'ac-name' }, name), h('span', { class: 'ac-sub' }, sub)] });
      }
    }
    return out;
  }

  function channels(w, q) {
    if (!w.guild) return [];
    return (w.guild.channels || []).filter((c) => TEXTUAL.has(c.type) && matches(q, c.name))
      .map((c) => ({ kind: 'channel', sort: c.name.toLowerCase(), insert: '<#' + c.id + '>', draw: () => [h('span', { class: 'ac-glyph' }, c.type === 2 ? '🔊' : '#'), h('span', { class: 'ac-name' }, c.name)] }))
      .sort(startsFirst(q));
  }

  function emoji(w, q) {
    const out = [];
    // The server's own custom emoji (only usable here, so only offered here).
    if (w.guild && !w.private) {
      for (const e of w.guild.emojis || []) {
        if (!e.available || !matches(q, e.name)) continue;
        const url = ctx.emojiUrl(e.id, e.animated);
        out.push({ kind: 'emoji', sort: e.name.toLowerCase(), insert: '<' + (e.animated ? 'a' : '') + ':' + e.name + ':' + e.id + '>',
          draw: () => [h('img', { class: 'ac-emoji', src: url, alt: '' }), h('span', { class: 'ac-name' }, ':' + e.name + ':')] });
      }
    }
    for (const [name, char] of Object.entries(SHORTCODES)) {
      if (!name.includes(q)) continue;
      out.push({ kind: 'emoji', sort: name, insert: char, draw: () => [h('span', { class: 'ac-glyph emoji' }, char), h('span', { class: 'ac-name' }, ':' + name + ':')] });
    }
    return out.sort(startsFirst(q));
  }

  // People not seen yet, asked of the server (never in a private conversation).
  function search(w, q) {
    clearTimeout(timer);
    if (!w.guild || w.private || !q) return;
    timer = setTimeout(() => { api.memberSearch(w.guild.id, q, 'lc-' + (++nonce)).catch(() => {}); }, SEARCH_DELAY_MS);
  }

  function update() {
    token = current();
    if (!token) { close(); return; }
    const w = ctx.where();
    if (!w) { close(); return; }
    const q = token.query;
    if (token.kind === '@') {
      items = [...people(w, q), ...extras(w, q)];
      if (items.length < MAX_SHOWN) search(w, q);
    } else if (token.kind === '#') items = channels(w, q);
    else items = emoji(w, q);
    items = items.slice(0, MAX_SHOWN);
    if (!items.length) { close(true); return; }
    index = Math.min(index, items.length - 1);
    draw();
  }

  function draw() {
    box.textContent = '';
    box.hidden = false;
    box.appendChild(h('div', { class: 'ac-title' }, token.kind === '@' ? 'Members' : token.kind === '#' ? 'Channels' : 'Emoji matching :' + token.query));
    items.forEach((item, i) => {
      box.appendChild(h('button', {
        type: 'button', role: 'option', class: 'ac-item' + (i === index ? ' selected' : ''), 'aria-selected': i === index ? 'true' : 'false',
        onmousedown: (e) => { e.preventDefault(); pick(i); }, onmouseenter: () => { index = i; markSelected(); },
      }, ...item.draw()));
    });
  }

  function markSelected() {
    box.querySelectorAll('.ac-item').forEach((el, i) => el.classList.toggle('selected', i === index));
  }

  function close(keepToken) {
    box.hidden = true;
    box.textContent = '';
    items = [];
    index = 0;
    if (!keepToken) token = null;
  }

  function pick(i) {
    const item = items[i];
    if (!item || !token) return;
    const value = input.value;
    const text = item.insert + ' ';
    input.value = value.slice(0, token.start) + text + value.slice(token.end);
    const caret = token.start + text.length;
    input.setSelectionRange(caret, caret);
    close();
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.focus();
  }

  // Before the composer's own keys (capture runs first on the textarea), only while open.
  input.addEventListener('keydown', (e) => {
    if (box.hidden || !items.length) return;
    let handled = true;
    if (e.key === 'ArrowDown') { index = (index + 1) % items.length; markSelected(); }
    else if (e.key === 'ArrowUp') { index = (index - 1 + items.length) % items.length; markSelected(); }
    else if ((e.key === 'Enter' && !e.shiftKey) || e.key === 'Tab') pick(index);
    else if (e.key === 'Escape') close();
    else handled = false;
    if (handled) { e.preventDefault(); e.stopImmediatePropagation(); }
  }, true);
  input.addEventListener('input', update);
  input.addEventListener('click', update);
  input.addEventListener('blur', () => setTimeout(() => close(), 100));

  return { box, close };
}
