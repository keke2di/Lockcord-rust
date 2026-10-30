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
const MAX_LIST = 200;

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

const CHANNEL_GLYPH = { 0: '#', 5: '📢', 15: '💬', 16: '🖼️', 2: '🔊', 13: '🎙️' };

// ctx: { guild, current, unread(channel), mentions(channel), muted(channel), open(channel),
//        unsupported(channel), threads(channel): the threads listed under it }
export function renderChannels(list, ctx) {
  list.textContent = '';
  for (const group of grouped(ctx.guild)) {
    if (group.category) list.appendChild(h('div', { class: 'channel-category' }, group.category.name || ''));
    for (const c of group.channels) {
      const text = TEXT_TYPES.has(c.type);
      const opens = text || FORUMS.has(c.type);
      const count = text ? ctx.mentions(c) : 0;
      const cls = 'channel' + (c.id === ctx.current ? ' selected' : '') + (text && c.id !== ctx.current && ctx.unread(c) && !ctx.muted(c) ? ' unread' : '')
        + (ctx.muted(c) ? ' muted' : '') + (opens ? '' : ' inert');
      list.appendChild(h('button', {
        type: 'button', class: cls, title: c.topic || c.name,
        onclick: () => (opens ? ctx.open(c) : ctx.unsupported(c)),
      }, h('span', { class: 'channel-glyph', 'aria-hidden': 'true' }, CHANNEL_GLYPH[c.type] || '#'),
        h('span', { class: 'channel-name' }, c.name || ''),
        count ? h('span', { class: 'badge' }, count > 99 ? '99+' : String(count)) : null));
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

// A list as GUILD_MEMBER_LIST_UPDATE keeps it: { id, items, groups, count }. Only the first
// rows are asked for (guilds.rs `subscription`), so it stays small.
export function applyMemberList(list, update) {
  const next = list && list.id === update.id ? list : { id: update.id, items: [], groups: [], count: 0 };
  if (update.groups) next.groups = update.groups;
  if (typeof update.member_count === 'number') next.count = update.member_count;
  for (const op of update.ops || []) {
    if (op.op === 'SYNC' && Array.isArray(op.range) && Array.isArray(op.items)) {
      const [start, end] = op.range;
      if (start < 0 || end < start || start > MAX_LIST) continue;
      next.items.splice(start, Math.min(end, MAX_LIST) - start + 1, ...op.items.slice(0, MAX_LIST - start));
    } else if (op.op === 'INSERT' && op.index <= MAX_LIST) {
      next.items.splice(op.index, 0, op.item);
    } else if (op.op === 'UPDATE' && op.index < next.items.length) {
      next.items[op.index] = op.item;
    } else if (op.op === 'DELETE' && op.index < next.items.length) {
      next.items.splice(op.index, 1);
    } else if (op.op === 'INVALIDATE' && Array.isArray(op.range)) {
      next.items.splice(op.range[0], op.range[1] - op.range[0] + 1);
    }
  }
  next.items = next.items.filter(Boolean).slice(0, MAX_LIST);
  return next;
}

// open(user, anchor): a member's profile, on a click.
export function renderMembers(panel, guild, list, open) {
  panel.textContent = '';
  if (!list || !list.items.length) {
    panel.appendChild(h('div', { class: 'members-loading' }, list ? 'No one to show.' : 'Loading members…'));
    return;
  }
  for (const item of list.items) {
    if (item.group) {
      const role = roleOf(guild, item.group.id);
      const label = item.group.id === 'online' ? 'Online' : item.group.id === 'offline' ? 'Offline' : role ? role.name : '';
      panel.appendChild(h('div', { class: 'members-group' }, label + ' — ' + (item.group.count || 0)));
    } else if (item.member && item.member.user) {
      const m = item.member;
      remember(m.user);
      const status = (m.presence && m.presence.status) || 'offline';
      const name = h('span', { class: 'member-name' }, m.nick || displayName(m.user));
      const color = colorFor(guild, m.roles);
      if (color) name.style.color = color;
      panel.appendChild(h('button', { type: 'button', class: 'member clickable' + (status === 'offline' ? ' offline' : ''), onclick: (e) => { e.stopPropagation(); if (open) open(m.user, e.currentTarget); } },
        avatarWithStatus(m.user, status, 'member-avatar'), name));
    }
  }
}
