// Servers in the window: the rail of server icons, a server's channel list (by category, only
// what Rust says this account can see), its member list, and names in role colours. All of it
// is drawn from LockCord's own events (main.rs, guilds.rs).

import { media } from './api.js';
import { h, icon, ICONS } from './dom.js';
import { avatarWithStatus, displayName, remember } from './people.js';

// Channel types: 0 text, 2 voice, 4 category, 5 announcement, 13 stage, 15 forum, 16 media.
export const TEXT_TYPES = new Set([0, 5]);
const FORUMS = new Set([15, 16]);
const VOICE_TYPES = new Set([2, 13]);

// A role's colour as CSS, or null for none.
function roleColor(role) {
  return role && role.color ? '#' + Number(role.color).toString(16).padStart(6, '0') : null;
}

// A member's colour: their highest-placed role that has one.
export function colorFor(guild, roleIds) {
  if (!guild || !roleIds || !roleIds.length) return null;
  let best = null;
  for (const role of guild.roles || []) {
    if (!role.color || !roleIds.includes(role.id)) continue;
    if (!best || role.position > best.position) best = role;
  }
  return roleColor(best);
}

export function roleOf(guild, id) {
  const role = guild && (guild.roles || []).find((r) => r.id === id);
  return role ? { name: role.name === '@everyone' ? 'everyone' : role.name, color: roleColor(role) } : null;
}

function initials(name) {
  return (name || '?').split(/\s+/).filter(Boolean).map((w) => [...w][0]).join('').slice(0, 4);
}

export function guildIcon(guild, cls) {
  if (guild.icon && /^\w{1,64}$/.test(guild.icon)) {
    const img = h('img', { class: cls, src: media('https://cdn.discordapp.com/icons/' + guild.id + '/' + guild.icon + '.webp?size=96'), alt: '', draggable: false });
    img.addEventListener('error', () => img.replaceWith(h('div', { class: cls + ' guild-initials' }, initials(guild.name))), { once: true });
    return img;
  }
  return h('div', { class: cls + ' guild-initials' }, initials(guild.name));
}

// --- The rail -------------------------------------------------------------------------------------

// ctx: { guilds, selected (guild id or null for home), unread(guild), mentions(guild),
//        homeMentions, home(), open(guild) }
export function renderRail(rail, ctx) {
  rail.textContent = '';
  const item = (selected, unread, count, title, node, onclick) => h('div', { class: 'rail-item' + (selected ? ' selected' : '') + (unread ? ' unread' : '') },
    h('span', { class: 'rail-pill' }),
    h('button', { type: 'button', class: 'rail-button', title, 'aria-label': title, onclick }, node,
      count ? h('span', { class: 'badge rail-badge' }, count > 99 ? '99+' : String(count)) : null));
  rail.appendChild(item(ctx.selected === null, false, ctx.homeMentions, 'Direct Messages', h('span', { class: 'rail-home' }, icon(ICONS.lock, 26)), () => ctx.home()));
  rail.appendChild(h('div', { class: 'rail-separator' }));
  for (const g of ctx.guilds) {
    if (g.unavailable) continue;
    rail.appendChild(item(ctx.selected === g.id, ctx.unread(g), ctx.mentions(g), g.name, guildIcon(g, 'rail-icon'), () => ctx.open(g)));
  }
}

// --- A server's channels ----------------------------------------------------------------------------

// Categories in order, each with its channels in order; channels outside any category first.
function grouped(guild) {
  const channels = (guild.channels || []).slice().sort((a, b) => (a.position - b.position) || a.id.localeCompare(b.id));
  const categories = channels.filter((c) => c.type === 4);
  const inside = (parent) => channels.filter((c) => c.type !== 4 && (c.parent_id || null) === parent)
    .sort((a, b) => (VOICE_TYPES.has(a.type) - VOICE_TYPES.has(b.type)) || (a.position - b.position));
  const out = [{ category: null, channels: inside(null) }];
  for (const cat of categories) out.push({ category: cat, channels: inside(cat.id) });
  return out.filter((g) => g.channels.length);
}

// The channel a server opens on: the one last open there, or its first text channel.
export function firstChannel(guild, last) {
  const text = (guild.channels || []).filter((c) => TEXT_TYPES.has(c.type));
  if (last && text.some((c) => c.id === last)) return last;
  for (const group of grouped(guild)) {
    const c = group.channels.find((x) => TEXT_TYPES.has(x.type));
    if (c) return c.id;
  }
  return null;
}

// Each channel kind's icon (dom.js ICONS): drawn as .lc-icon, so it follows the theme.
export const CHANNEL_ICON = { 0: 'hash', 5: 'megaphone', 15: 'forum', 16: 'media', 2: 'speaker', 13: 'stage' };
export const channelIcon = (type, size = 20) => icon(ICONS[CHANNEL_ICON[type] || 'hash'], size);

// ctx: { guild, current, unread(channel), mentions(channel), muted(channel), open(channel),
//        unsupported(channel), threads(channel): the threads listed under it,
//        voiceRoom(channel): who is in a voice channel (call.js), or null }
export function renderChannels(list, ctx) {
  list.textContent = '';
  for (const group of grouped(ctx.guild)) {
    if (group.category) list.appendChild(h('div', { class: 'channel-category' }, group.category.name || ''));
    for (const c of group.channels) {
      const text = TEXT_TYPES.has(c.type);
      const opens = text || FORUMS.has(c.type) || c.type === 2;
      const count = text ? ctx.mentions(c) : 0;
      const cls = 'channel' + (c.id === ctx.current ? ' selected' : '') + (text && c.id !== ctx.current && ctx.unread(c) && !ctx.muted(c) ? ' unread' : '')
        + (ctx.muted(c) ? ' muted' : '') + (opens ? '' : ' inert');
      list.appendChild(h('button', {
        type: 'button', class: cls, title: c.topic || c.name,
        onclick: () => (opens ? ctx.open(c) : ctx.unsupported(c)),
      }, h('span', { class: 'channel-glyph', 'aria-hidden': 'true' }, channelIcon(c.type)),
        h('span', { class: 'channel-name' }, c.name || ''),
        count ? h('span', { class: 'badge' }, count > 99 ? '99+' : String(count)) : null));
      // A voice channel's people, under it.
      const room = c.type === 2 && ctx.voiceRoom ? ctx.voiceRoom(c) : null;
      if (room) list.appendChild(room);
      // The threads this account is in, under their channel, as Discord lists them.
      for (const t of ctx.threads(c)) {
        const tCount = ctx.mentions(t);
        list.appendChild(h('button', {
          type: 'button', class: 'channel thread-item' + (t.id === ctx.current ? ' selected' : '') + (t.id !== ctx.current && ctx.unread(t) ? ' unread' : ''),
          title: t.name, onclick: () => ctx.open(t),
        }, h('span', { class: 'thread-branch', 'aria-hidden': 'true' }),
          h('span', { class: 'channel-name' }, t.name || ''),
          tCount ? h('span', { class: 'badge' }, tCount > 99 ? '99+' : String(tCount)) : null));
      }
    }
  }
}

// --- The member list ------------------------------------------------------------------------------

// The furthest row asked for (guilds.rs `member_ranges` stops below 10 000).
const MAX_ROWS = 10000;

// A list as GUILD_MEMBER_LIST_UPDATE keeps it: { id, items, groups, count, total }. items is
// sparse and kept by index: Discord only sends the windows subscribed to (servers' op 37), and
// INVALIDATE empties a window that's no longer watched, so rows never shift to fill a gap.
export function applyMemberList(list, update) {
  const next = list && list.id === update.id ? list : { id: update.id, items: [], groups: [], count: 0, total: 0 };
  if (update.groups) next.groups = update.groups;
  if (typeof update.member_count === 'number') next.count = update.member_count;
  const items = next.items;
  const ok = (i) => Number.isInteger(i) && i >= 0 && i < MAX_ROWS;
  for (const op of update.ops || []) {
    if (op.op === 'SYNC' && Array.isArray(op.range) && Array.isArray(op.items)) {
      const [start, end] = op.range;
      if (!ok(start) || !(end >= start)) continue;
      for (let i = start; i <= end && i < MAX_ROWS; i++) items[i] = op.items[i - start];
    } else if (op.op === 'INSERT' && ok(op.index)) {
      if (op.index > items.length) items[op.index] = op.item; else items.splice(op.index, 0, op.item);
    } else if (op.op === 'UPDATE' && ok(op.index)) {
      items[op.index] = op.item;
    } else if (op.op === 'DELETE' && ok(op.index) && op.index < items.length) {
      items.splice(op.index, 1);
    } else if (op.op === 'INVALIDATE' && Array.isArray(op.range)) {
      for (let i = Math.max(0, op.range[0]); i <= op.range[1] && i < items.length; i++) items[i] = undefined;
    }
  }
  // The whole list's length, from its groups (a header row each, then its members).
  const total = next.groups.reduce((n, g) => n + (g.count ? g.count + 1 : 0), 0);
  next.total = Math.min(MAX_ROWS, total || items.length);
  if (items.length > next.total) items.length = next.total;
  return next;
}

// The members a list update carries, for their status and custom status elsewhere (profiles).
export function membersIn(update) {
  const out = [];
  for (const op of update.ops || []) {
    for (const it of op.items || (op.item ? [op.item] : [])) if (it && it.member && it.member.user) out.push(it.member);
  }
  return out;
}

// A presence's custom status (activity type 4): { text, emoji } or null.
export function customStatus(presence) {
  const a = presence && (presence.activities || []).find((x) => x && x.type === 4);
  if (!a || (!a.state && !a.emoji)) return null;
  return { text: a.state || '', emoji: a.emoji || null };
}

// open(user, anchor): a member's profile, on a click. Rows not loaded (outside the windows
// subscribed to) are drawn as placeholders of a row's height, so the scroll bar is the whole list.
export function renderMembers(panel, guild, list, open) {
  const top = panel.scrollTop;
  panel.textContent = '';
  if (!list || !list.total) {
    panel.appendChild(h('div', { class: 'members-loading' }, list ? 'No one to show.' : 'Loading members…'));
    return;
  }
  let gap = 0;
  const flush = (at) => {
    if (!gap) return;
    const spacer = h('div', { class: 'members-gap' });
    spacer.style.height = 'calc(' + gap + ' * var(--lc-member-row, 44px))';
    spacer.dataset.from = String(at - gap);
    spacer.dataset.n = String(gap);
    panel.appendChild(spacer);
    gap = 0;
  };
  for (let i = 0; i < list.total; i++) {
    const item = list.items[i];
    if (!item || (!item.group && !(item.member && item.member.user))) { gap++; continue; }
    flush(i);
    let node;
    if (item.group) {
      const role = roleOf(guild, item.group.id);
      const label = item.group.id === 'online' ? 'Online' : item.group.id === 'offline' ? 'Offline' : role ? role.name : '';
      node = h('div', { class: 'members-group' }, label + ' — ' + (item.group.count || 0));
    } else {
      const m = item.member;
      remember(m.user);
      const status = (m.presence && m.presence.status) || 'offline';
      const custom = customStatus(m.presence);
      const name = h('span', { class: 'member-name' }, m.nick || displayName(m.user));
      const color = colorFor(guild, m.roles);
      if (color) name.style.color = color;
      node = h('button', { type: 'button', class: 'member clickable' + (status === 'offline' ? ' offline' : ''), onclick: (e) => { e.stopPropagation(); if (open) open(m.user, e.currentTarget); } },
        avatarWithStatus(m.user, status, 'member-avatar'),
        h('span', { class: 'member-text' }, name, custom && custom.text ? h('span', { class: 'member-custom', title: custom.text }, custom.text) : null));
    }
    node.dataset.from = String(i);
    node.dataset.n = '1';
    panel.appendChild(node);
  }
  flush(list.total);
  panel.scrollTop = top;
}

// The rows in view [first, last], from what renderMembers drew (rows and gaps carry their index).
export function rowsInView(panel) {
  const from = panel.scrollTop;
  const to = from + panel.clientHeight;
  let first = null;
  let last = 0;
  for (const el of panel.children) {
    if (!el.dataset.from) continue;
    const top = el.offsetTop;
    const height = el.offsetHeight || 1;
    if (top + height < from) continue;
    if (top > to) break;
    const n = Number(el.dataset.n) || 1;
    const per = height / n;
    const a = Number(el.dataset.from) + Math.max(0, Math.floor((from - top) / per));
    const b = Number(el.dataset.from) + Math.min(n - 1, Math.floor((to - top) / per));
    if (first === null) first = a;
    last = Math.max(last, b);
  }
  return first === null ? null : [first, last];
}

// The 100-row windows to subscribe to for the rows in view (with [0, 99], as the web client does).
export function rangesFor(view) {
  const out = [[0, 99]];
  if (!view) return out;
  for (const row of [view[0], view[1]]) {
    const at = Math.floor(Math.min(row, MAX_ROWS - 1) / 100) * 100;
    if (!out.some((r) => r[0] === at)) out.push([at, at + 99]);
  }
  return out.slice(0, 3);
}
