// The picker, as Discord's: a GIFs tab (Discord's own GIF search and trending, Tenor behind it,
// and the account's favourites, read only; gifs.rs) and an Emoji tab (every Unicode emoji this
// system can draw, by category, searchable, with the open server's own emoji, and every server's
// with Nitro). Pictures load through the media scheme like every other picture here.

import { api, media } from './api.js';
import { h, icon, ICONS } from './dom.js';
import { EMOJI, EMOJI_CATEGORIES_FULL } from './emoji-data.js';

const MAX_RECENT = 24;
const SEARCH_DELAY_MS = 300;

// --- Which emoji this system can draw ------------------------------------------------------------

// Emoji newer than the system's emoji font draw as empty boxes (Windows 10 stops at Emoji 12-13):
// for each newer Unicode version a few of its emoji are drawn once, and the version counts if
// one of them comes out in colour.
let drawable = null;
function drawableVersions() {
  if (drawable) return drawable;
  drawable = new Set();
  const byVersion = new Map();
  for (const e of EMOJI) {
    if (e[4] <= 12.1) continue;
    const list = byVersion.get(e[4]) || [];
    if (list.length < 3 && !e[0].includes('‍')) list.push(e[0]);
    byVersion.set(e[4], list);
  }
  let ctx = null;
  try { ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true }); } catch { ctx = null; }
  for (const [version, samples] of byVersion) {
    if (!ctx) break;
    if (samples.some((ch) => inColour(ctx, ch))) drawable.add(version);
  }
  return drawable;
}

function inColour(ctx, ch) {
  ctx.canvas.width = 24;
  ctx.canvas.height = 24;
  ctx.clearRect(0, 0, 24, 24);
  ctx.font = '20px "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", sans-serif';
  ctx.textBaseline = 'top';
  ctx.fillStyle = '#000';
  ctx.fillText(ch, 0, 0);
  const d = ctx.getImageData(0, 0, 24, 24).data;
  for (let i = 0; i < d.length; i += 4) {
    if (d[i + 3] > 0 && (Math.abs(d[i] - d[i + 1]) > 24 || Math.abs(d[i + 1] - d[i + 2]) > 24)) return true;
  }
  return false;
}

const canDraw = (e) => e[4] <= 12.1 || drawableVersions().has(e[4]);

// A Unicode emoji as the picker hands it on: text to insert, the key a reaction takes, its name.
const unicode = (e) => ({ text: e[0], key: e[0], name: (e[2].split(' ')[0] || e[1].replace(/\s+/g, '_')), label: e[1], char: e[0] });
// A server's emoji: <:name:id> to insert, name:id for a reaction.
const custom = (e, guild, emojiUrl) => ({
  text: '<' + (e.animated ? 'a' : '') + ':' + e.name + ':' + e.id + '>', key: e.name + ':' + e.id, recent: (e.animated ? 'a:' : '') + e.name + ':' + e.id,
  name: e.name, label: guild ? 'from ' + guild.name : '', url: emojiUrl(e.id, e.animated),
});

// opts: { tab ('emoji' | 'gif'), gifs (offer the GIF tab), guild (the open server's view, or
//         null), guilds (all servers, for Nitro), nitro, customAllowed (server emoji may be used
//         here at all), emojiUrl(id, animated), recent (keys), onEmoji(pick), onGif(gif),
//         remember(pick) (a pick to keep as recent) }
export function createPicker(opts) {
  const node = h('div', { class: 'popup emoji-picker lc-picker', role: 'dialog', 'aria-label': 'Emoji and GIFs', onclick: (e) => e.stopPropagation() });
  const body = h('div', { class: 'picker-body' });
  let tab = opts.gifs ? opts.tab : 'emoji';
  const tabs = h('div', { class: 'picker-tabs', role: 'tablist' });
  const drawTabs = () => {
    tabs.textContent = '';
    if (!opts.gifs) return;
    for (const [key, label] of [['gif', 'GIFs'], ['emoji', 'Emoji']]) {
      tabs.appendChild(h('button', { type: 'button', role: 'tab', class: 'picker-tab', 'aria-selected': tab === key ? 'true' : 'false',
        onclick: () => { tab = key; drawTabs(); draw(); } }, label));
    }
  };
  const draw = () => {
    body.textContent = '';
    body.appendChild(tab === 'gif' ? gifPane(opts) : emojiPane(opts));
  };
  drawTabs();
  draw();
  node.append(tabs, body);
  return node;
}

// --- Emoji ---------------------------------------------------------------------------------------

function emojiPane(opts) {
  const sections = [];   // { id, title, icon (node), picks }
  const ownGuilds = [];
  if (opts.customAllowed) {
    if (opts.guild) ownGuilds.push(opts.guild);
    if (opts.nitro) for (const g of opts.guilds || []) if (!opts.guild || g.id !== opts.guild.id) ownGuilds.push(g);
  }
  const customs = new Map(); // recent key -> pick
  for (const g of ownGuilds) {
    const picks = (g.emojis || []).filter((e) => e && e.id && e.available !== false).map((e) => custom(e, g, opts.emojiUrl));
    for (const p of picks) customs.set(p.recent, p);
    if (picks.length) sections.push({ id: 'guild-' + g.id, title: g.name, guild: g, picks });
  }
  const all = EMOJI.filter(canDraw);
  const byChar = new Map(all.map((e) => [e[0], e]));
  const recent = (opts.recent || []).map((k) => customs.get(k) || (byChar.has(k) ? unicode(byChar.get(k)) : null)).filter(Boolean);
  if (recent.length) sections.unshift({ id: 'recent', title: 'Frequently Used', picks: recent });
  EMOJI_CATEGORIES_FULL.forEach(([id, title, glyph], n) => {
    sections.push({ id, title, glyph, picks: all.filter((e) => e[3] === n).map(unicode) });
  });

  const search = h('input', { class: 'input picker-search', type: 'search', placeholder: 'Find the perfect emoji', 'aria-label': 'Search emoji', spellcheck: false });
  const scroll = h('div', { class: 'emoji-scroll' });
  const preview = h('div', { class: 'emoji-preview' });
  const rail = h('nav', { class: 'emoji-rail', 'aria-label': 'Categories' });
  let shown = [];

  const showPreview = (p) => {
    preview.textContent = '';
    if (!p) return;
    preview.append(p.url ? h('img', { class: 'emoji-preview-glyph', src: p.url, alt: '' }) : h('span', { class: 'emoji-preview-glyph' }, p.char),
      h('span', { class: 'emoji-preview-words' }, h('strong', null, ':' + p.name + ':'), p.label ? h('span', null, p.label) : null));
  };
  const cell = (p, i) => {
    const b = h('button', { type: 'button', class: 'emoji-cell', title: ':' + p.name + ':', 'aria-label': p.name }, p.url ? h('img', { class: 'emoji-cell-img', src: p.url, alt: ':' + p.name + ':', loading: 'lazy', draggable: false }) : p.char);
    b.dataset.i = String(i);
    return b;
  };
  const drawSections = (list) => {
    scroll.textContent = '';
    shown = [];
    for (const s of list) {
      const grid = h('div', { class: 'emoji-grid' });
      for (const p of s.picks) { grid.appendChild(cell(p, shown.length)); shown.push(p); }
      const head = h('div', { class: 'emoji-section-title' }, s.title);
      head.dataset.section = s.id;
      scroll.append(head, grid);
    }
    if (!list.length) scroll.appendChild(h('div', { class: 'picker-empty' }, 'No emoji match that.'));
    showPreview(shown[0] || null);
  };
  // One listener for every cell: hovering previews, a click picks.
  scroll.addEventListener('mouseover', (e) => { const b = e.target.closest('.emoji-cell'); if (b) showPreview(shown[Number(b.dataset.i)]); });
  scroll.addEventListener('click', (e) => {
    const b = e.target.closest('.emoji-cell');
    if (!b) return;
    e.stopPropagation();
    const p = shown[Number(b.dataset.i)];
    if (opts.remember) opts.remember(p);
    opts.onEmoji(p);
  });

  for (const s of sections) {
    const iconNode = s.id === 'recent' ? icon(ICONS.clock, 18)
      : s.guild ? guildBadge(s.guild)
      : h('span', { class: 'emoji-rail-glyph' }, s.glyph);
    rail.appendChild(h('button', { type: 'button', class: 'emoji-rail-button', title: s.title, 'aria-label': s.title, onclick: (e) => {
      e.stopPropagation();
      if (search.value) { search.value = ''; drawSections(sections); }
      const head = scroll.querySelector('[data-section="' + CSS.escape(s.id) + '"]');
      if (head) scroll.scrollTop = head.offsetTop - scroll.offsetTop;
    } }, iconNode));
  }

  search.addEventListener('input', () => {
    const q = search.value.trim().toLowerCase().replace(/^:|:$/g, '');
    if (!q) { drawSections(sections); return; }
    const hits = [];
    for (const s of sections) {
      if (s.id === 'recent') continue;
      for (const p of s.picks) if (p.name.toLowerCase().includes(q) || (p.label && !p.url && p.label.includes(q))) hits.push(p);
    }
    // Names that start with it first.
    hits.sort((a, b) => Number(!a.name.startsWith(q)) - Number(!b.name.startsWith(q)));
    drawSections(hits.length ? [{ id: 'search', title: 'Search results', picks: hits.slice(0, 400) }] : []);
  });
  search.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && shown[0]) { e.preventDefault(); if (opts.remember) opts.remember(shown[0]); opts.onEmoji(shown[0]); }
  });
  drawSections(sections);
  setTimeout(() => search.focus(), 0);
  return h('div', { class: 'emoji-pane' }, search, h('div', { class: 'emoji-main' }, rail, scroll), preview);
}

function guildBadge(g) {
  if (g.icon && /^\w{1,64}$/.test(g.icon)) return h('img', { class: 'emoji-rail-guild', src: media('https://cdn.discordapp.com/icons/' + g.id + '/' + g.icon + '.webp?size=48'), alt: '', draggable: false });
  return h('span', { class: 'emoji-rail-guild initials' }, (g.name || '?').slice(0, 2));
}

// --- GIFs ----------------------------------------------------------------------------------------

// Asked of Discord once per picker opening at most; favourites once a session.
let favorites = null;

function gifPane(opts) {
  const search = h('input', { class: 'input picker-search', type: 'search', placeholder: 'Search Tenor', 'aria-label': 'Search GIFs', spellcheck: false });
  const head = h('div', { class: 'gif-head' });
  const area = h('div', { class: 'gif-scroll' });
  let timer = null;
  let asked = 0;

  // Previews load only near the view and pause out of it (favourites can be hundreds). A video
  // that won't play is tried once as a picture; one that won't load either says so.
  const near = new IntersectionObserver((entries) => {
    for (const e of entries) {
      const tile = e.target;
      if (e.isIntersecting && tile.lcLoad) { tile.lcLoad(); tile.lcLoad = null; }
      const v = tile.querySelector('video');
      if (v && v.src) { if (e.isIntersecting) v.play().catch(() => {}); else v.pause(); }
    }
  }, { root: area, rootMargin: '300px 0px' });
  const picture = (tile, src) => {
    const img = h('img', { class: 'gif-video', alt: '', draggable: false, decoding: 'async' });
    img.addEventListener('error', () => { img.remove(); tile.classList.add('gif-dead'); tile.title = 'This GIF isn’t available any more'; }, { once: true });
    img.src = media(src);
    tile.prepend(img);
  };
  const video = (tile, src) => {
    const v = h('video', { class: 'gif-video', loop: true, autoplay: true, playsInline: true, preload: 'auto' });
    v.muted = true;
    v.defaultMuted = true;
    v.addEventListener('error', () => { v.remove(); picture(tile, src); }, { once: true });
    v.src = media(src);
    tile.prepend(v);
  };
  // Gives a tile its preview once it comes near the view.
  const preview = (tile, src, kind) => {
    if (!src) return tile;
    const isVideo = (kind || (/\.(mp4|webm)(?:[?#]|$)/i.test(src) ? 'video' : 'image')) === 'video';
    tile.lcLoad = () => (isVideo ? video(tile, src) : picture(tile, src));
    near.observe(tile);
    return tile;
  };
  // Emptied before each view: the tiles it held aren't watched any more.
  const clear = () => { near.disconnect(); area.textContent = ''; };
  const message = (text) => { clear(); area.appendChild(h('div', { class: 'picker-empty' }, text)); };

  // A list of GIFs in two columns, each in its own aspect.
  const showList = (title, load) => {
    head.textContent = '';
    if (title) {
      head.append(h('button', { type: 'button', class: 'gif-back', 'aria-label': 'Back', onclick: (e) => { e.stopPropagation(); search.value = ''; home(); } }, icon(ICONS.chevronRight, 18)),
        h('span', { class: 'gif-title' }, title));
    }
    message('Loading…');
    const mine = ++asked;
    load().then((gifs) => {
      if (mine !== asked) return;
      clear();
      if (!gifs.length) { message('No GIFs found.'); return; }
      const cols = [h('div', { class: 'gif-col' }), h('div', { class: 'gif-col' })];
      const heights = [0, 0];
      for (const g of gifs) {
        const ratio = g.width && g.height ? g.height / g.width : 0.6;
        const col = heights[0] <= heights[1] ? 0 : 1;
        heights[col] += ratio;
        const tile = preview(h('button', { type: 'button', class: 'gif-tile', title: 'Send', onclick: (e) => { e.stopPropagation(); opts.onGif(g); } }), g.src, g.kind);
        tile.style.aspectRatio = (g.width || 5) + ' / ' + (g.height || 3);
        cols[col].appendChild(tile);
      }
      area.appendChild(h('div', { class: 'gif-columns' }, cols));
    }).catch((e) => { if (mine === asked) message('Couldn’t load GIFs: ' + e); });
  };

  // The first view: Favorites, Trending GIFs, then the trending categories.
  const home = () => {
    head.textContent = '';
    message('Loading…');
    const mine = ++asked;
    const favs = favorites ? Promise.resolve(favorites) : api.gifFavorites().then((f) => (favorites = f)).catch(() => []);
    Promise.all([favs, api.gifTrending()]).then(([fav, trending]) => {
      if (mine !== asked) return;
      clear();
      const tiles = h('div', { class: 'gif-categories' });
      const tile = (label, src, kind, glyph, open) => tiles.appendChild(preview(h('button', { type: 'button', class: 'gif-category', onclick: (e) => { e.stopPropagation(); open(); } },
        h('span', { class: 'gif-category-label' }, glyph ? icon(glyph, 16) : null, label)), src, kind));
      if (fav.length) tile('Favorites', fav[0].src, fav[0].kind, ICONS.star, () => showList('Favorites', () => Promise.resolve(fav)));
      tile('Trending GIFs', trending.preview, null, null, () => showList('Trending GIFs', () => api.gifSearch(null)));
      for (const c of trending.categories || []) tile(c.name, c.src, null, null, () => { showList(c.name, () => api.gifSearch(c.name)); });
      area.appendChild(tiles);
    }).catch((e) => { if (mine === asked) message('Couldn’t load GIFs: ' + e); });
  };

  search.addEventListener('input', () => {
    clearTimeout(timer);
    const q = search.value.trim();
    timer = setTimeout(() => { if (q) showList(q, () => api.gifSearch(q)); else home(); }, SEARCH_DELAY_MS);
  });
  home();
  setTimeout(() => search.focus(), 0);
  return h('div', { class: 'gif-pane' }, search, head, area);
}

// The recent list with a pick put first (keys: the character, or [a:]name:id).
export function withRecent(list, pick) {
  const key = pick.recent || pick.key;
  return [key, ...(list || []).filter((k) => k !== key)].slice(0, MAX_RECENT);
}
