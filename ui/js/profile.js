// People's profiles, as Discord's popout shows them, and this account's own status menu. The
// profile comes from Rust (main.rs `user_profile`); banners and avatars through the `lcmedia:`
// scheme like every other picture.

import { media } from './api.js';
import { h, icon, ICONS, snowflakeTime } from './dom.js';
import { parse, render } from './markdown.js';
import { avatarWithStatus, displayName } from './people.js';

export const STATUS_WORDS = {
  online: ['Online', null],
  idle: ['Idle', null],
  dnd: ['Do Not Disturb', 'You won’t get desktop notifications.'],
  invisible: ['Invisible', 'You’ll look offline, but can still use LockCord.'],
};

// Discord's relationship types.
const FRIEND = 1;
const BLOCKED = 2;
const INCOMING = 3;
const OUTGOING = 4;

function bannerUrl(userId, hash) {
  if (!hash || !/^\w{1,64}$/.test(hash) || !/^\d{1,20}$/.test(userId)) return null;
  return media('https://cdn.discordapp.com/banners/' + userId + '/' + hash + '.' + (hash.startsWith('a_') ? 'gif' : 'webp') + '?size=600');
}

const hex = (n) => '#' + Number(n).toString(16).padStart(6, '0');
const longDate = (ms) => new Date(ms).toLocaleDateString([], { dateStyle: 'medium' });

// The popout's body. p: the profile (or null while it loads); u: the user as already known.
// ctx: { me, status (theirs), relationship (type or null), guild (server roles, or null), md,
//        guildName(id), message(), add(), accept(), remove(), block(), unblock(), editStatus() }
export function profileCard(u, p, ctx) {
  const user = p ? Object.assign({}, u, p.user) : u;
  const self = ctx.me && user.id === ctx.me.id;
  const banner = p && bannerUrl(user.id, p.banner);
  const top = h('div', { class: 'profile-banner' + (banner ? ' image' : '') });
  if (banner) top.appendChild(h('img', { src: banner, alt: '', draggable: false }));
  else if (p && p.accent != null) top.style.background = hex(p.accent);
  const card = h('div', { class: 'profile-card' }, top,
    h('div', { class: 'profile-avatar' }, avatarWithStatus(user, ctx.status, 'profile-avatar-img')));

  const nick = p && p.member && p.member.nick;
  const body = h('div', { class: 'profile-body' },
    h('div', { class: 'profile-name' }, nick || displayName(user)),
    h('div', { class: 'profile-tag' }, user.username || '', p && p.pronouns ? h('span', { class: 'profile-pronouns' }, ' • ' + p.pronouns) : null,
      user.bot ? h('span', { class: 'profile-bot' }, 'BOT') : null));
  card.appendChild(body);

  if (!p) {
    body.appendChild(h('div', { class: 'profile-loading' }, h('span', { class: 'spinner' }), ' Loading profile…'));
    return card;
  }
  const section = (title, ...children) => body.appendChild(h('div', { class: 'profile-section' }, h('div', { class: 'profile-heading' }, title), ...children));
  if (p.bio) section('About Me', h('div', { class: 'profile-bio' }, render(parse(p.bio), document, ctx.md)));
  const since = [h('div', { class: 'profile-since' }, icon(ICONS.chat, 14), ' Discord: ' + longDate(snowflakeTime(user.id)))];
  if (p.member && p.member.joined_at && ctx.guild) since.push(h('div', { class: 'profile-since' }, icon(ICONS.members, 14), ' ' + ctx.guild.name + ': ' + longDate(Date.parse(p.member.joined_at))));
  section('Member Since', ...since);
  if (p.member && ctx.guild) {
    const roles = (ctx.guild.roles || []).filter((r) => (p.member.roles || []).includes(r.id)).sort((a, b) => b.position - a.position);
    if (roles.length) {
      section('Roles', h('div', { class: 'profile-roles' }, roles.map((r) => h('span', { class: 'profile-role' },
        h('span', { class: 'profile-role-dot', style: 'background:' + (r.color ? hex(r.color) : 'var(--lc-status-offline)') }), r.name))));
    }
  }
  if (!self) {
    const mutual = (p.mutual_guilds || []).map((g) => ctx.guildName(g.id)).filter(Boolean);
    const lines = [];
    if (mutual.length) lines.push(h('div', { class: 'profile-mutual', title: mutual.join(', ') }, mutual.length + ' mutual server' + (mutual.length === 1 ? '' : 's') + ': ' + mutual.slice(0, 4).join(', ') + (mutual.length > 4 ? '…' : '')));
    if (p.mutual_friends) lines.push(h('div', { class: 'profile-mutual' }, p.mutual_friends + ' mutual friend' + (p.mutual_friends === 1 ? '' : 's')));
    if (lines.length) section('In Common', ...lines);
  }
  body.appendChild(actions(user, self, ctx));
  return card;
}

function actions(user, self, ctx) {
  const row = h('div', { class: 'profile-actions' });
  const button = (label, fn, cls) => h('button', { type: 'button', class: 'button small ' + (cls || 'secondary'), onclick: (e) => { e.stopPropagation(); fn(); } }, label);
  if (self) {
    row.appendChild(button('Set status', ctx.editStatus, 'brand'));
    return row;
  }
  const rel = ctx.relationship;
  if (rel !== BLOCKED && !user.bot) row.appendChild(button('Message', ctx.message, 'brand'));
  if (rel === FRIEND) row.appendChild(button('Remove Friend', ctx.remove, 'danger-outline'));
  else if (rel === INCOMING) row.appendChild(button('Accept Request', ctx.accept, 'positive'));
  else if (rel === OUTGOING) row.appendChild(button('Request Sent', ctx.remove));
  else if (rel !== BLOCKED && !user.bot) row.appendChild(button('Add Friend', ctx.add, 'positive'));
  row.appendChild(rel === BLOCKED ? button('Unblock', ctx.unblock) : button('Block', ctx.block, 'danger-outline'));
  return row;
}

// --- This account's status -----------------------------------------------------------------------

// The dot this account shows: invisible looks offline.
export function myDot(my) {
  const s = (my && my.status) || 'online';
  return s === 'invisible' ? 'offline' : s;
}

// The status menu over the user panel. on: { pick(status) }
export function statusMenu(my, on) {
  const current = (my && my.status) || 'online';
  const item = (status) => {
    const [label, note] = STATUS_WORDS[status];
    return h('button', { type: 'button', role: 'menuitemradio', 'aria-checked': current === status ? 'true' : 'false', class: 'menu-item status-item' + (current === status ? ' current' : ''), onclick: (e) => { e.stopPropagation(); on.pick(status); } },
      h('span', { class: 'status-swatch status-' + (status === 'invisible' ? 'offline' : status) }),
      h('span', { class: 'status-words' }, h('span', null, label), note ? h('span', { class: 'status-note' }, note) : null));
  };
  return h('div', { class: 'popup context-menu status-menu', role: 'menu', onclick: (e) => e.stopPropagation() },
    ['online', 'idle', 'dnd', 'invisible'].map(item));
}

// A list of friends to tick, for a new group or adding to one. on: { done(ids), cancel() }
export function friendPicker(friends, max, label, on) {
  const picked = new Set();
  const search = h('input', { class: 'input', type: 'text', placeholder: 'Type the username of a friend', 'aria-label': 'Search friends' });
  const list = h('div', { class: 'picker-list', role: 'list' });
  const count = h('div', { class: 'picker-count' });
  const go = h('button', { type: 'button', class: 'button brand', disabled: true, onclick: () => on.done([...picked]) }, label);
  const draw = () => {
    const q = search.value.trim().toLowerCase();
    list.textContent = '';
    const shown = friends.filter((r) => !q || displayName(r.user).toLowerCase().includes(q) || (r.user.username || '').includes(q));
    for (const r of shown) {
      const ticked = picked.has(r.id);
      list.appendChild(h('label', { class: 'picker-row' + (ticked ? ' on' : ''), role: 'listitem' },
        avatarWithStatus(r.user, null, 'member-avatar'),
        h('span', { class: 'member-name' }, r.nickname || displayName(r.user), h('span', { class: 'friend-username' }, r.user.username || '')),
        h('input', { type: 'checkbox', checked: ticked, disabled: !ticked && picked.size >= max, onchange: (e) => { if (e.target.checked) picked.add(r.id); else picked.delete(r.id); draw(); } })));
    }
    if (!shown.length) list.appendChild(h('div', { class: 'friends-empty' }, friends.length ? 'No friends match that.' : 'No friends to add.'));
    count.textContent = picked.size ? picked.size + ' of ' + max + ' picked' : 'You can add ' + max + ' more friend' + (max === 1 ? '' : 's') + '.';
    go.disabled = !picked.size;
  };
  search.addEventListener('input', draw);
  draw();
  setTimeout(() => search.focus(), 0);
  return h('div', { class: 'friend-picker' }, count, search, list,
    h('div', { class: 'modal-buttons' }, h('button', { type: 'button', class: 'button link', onclick: on.cancel }, 'Cancel'), go));
}
