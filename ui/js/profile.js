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

// A custom status's emoji: Discord's image for a custom one, the character otherwise.
function customEmoji(emoji, emojiUrl) {
  if (!emoji || !emoji.name) return null;
  if (emoji.id && emojiUrl) return h('img', { class: 'status-bubble-emoji', src: emojiUrl(emoji.id, emoji.animated), alt: ':' + emoji.name + ':', draggable: false });
  return h('span', { class: 'status-bubble-emoji' }, emoji.name);
}

// The full profile, over the whole window as Discord's is. p: the profile (or null while it
// loads); u: the user as already known.
// ctx: { me, status (theirs), custom ({ text, emoji } or null), relationship (type or null),
//        guild (the open server, for roles, or null), md, emojiUrl, known(id) (a server this
//        account is in), guildIcon(guild), openGuild(id), openUser(user), close(),
//        message(), add(), accept(), remove(), block(), unblock(), editStatus() }
export function profileCard(u, p, ctx) {
  const user = p ? Object.assign({}, u, p.user) : u;
  const self = ctx.me && user.id === ctx.me.id;
  const banner = p && bannerUrl(user.id, p.banner);
  const top = h('div', { class: 'profile-banner' + (banner ? ' image' : '') });
  if (banner) top.appendChild(h('img', { src: banner, alt: '', draggable: false }));
  else if (p && p.accent != null) top.style.background = hex(p.accent);
  const head = h('div', { class: 'profile-head' },
    h('div', { class: 'profile-avatar' }, avatarWithStatus(user, ctx.status, 'profile-avatar-img')));
  if (ctx.custom && (ctx.custom.text || ctx.custom.emoji)) {
    head.appendChild(h('div', { class: 'status-bubble', title: ctx.custom.text || '' },
      customEmoji(ctx.custom.emoji, ctx.emojiUrl), ctx.custom.text ? h('span', { class: 'status-bubble-text' }, ctx.custom.text) : null));
  }
  if (p) head.appendChild(actions(user, self, ctx));
  const card = h('div', { class: 'profile-card' }, top,
    h('button', { type: 'button', class: 'profile-close', 'aria-label': 'Close', title: 'Close', onclick: (e) => { e.stopPropagation(); ctx.close(); } }, icon(ICONS.close, 20)),
    head);

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

  // The tabs: About Me, and (for someone else) the servers and friends in common.
  const servers = self ? [] : (p.mutual_guilds || []).map((g) => ctx.known(g.id)).filter(Boolean);
  const friends = self ? [] : (p.mutual_friends_list || []);
  const friendCount = self ? 0 : Math.max(friends.length, p.mutual_friends || 0);
  const tabs = [['about', 'About Me']];
  if (!self) tabs.push(['servers', servers.length + ' Mutual Server' + (servers.length === 1 ? '' : 's')], ['friends', friendCount + ' Mutual Friend' + (friendCount === 1 ? '' : 's')]);
  const panel = h('div', { class: 'profile-tab-panel', role: 'tabpanel' });
  const bar = h('div', { class: 'profile-tabs', role: 'tablist' });
  const show = (key) => {
    for (const b of bar.children) b.setAttribute('aria-selected', b.dataset.key === key ? 'true' : 'false');
    panel.textContent = '';
    if (key === 'about') {
      about(panel, user, p, ctx);
    } else if (key === 'servers') {
      if (!servers.length) panel.appendChild(h('div', { class: 'profile-empty' }, 'No servers in common.'));
      for (const g of servers) {
        const nickThere = ((p.mutual_guilds || []).find((x) => x.id === g.id) || {}).nick;
        panel.appendChild(h('button', { type: 'button', class: 'profile-row', onclick: (e) => { e.stopPropagation(); ctx.openGuild(g.id); } },
          ctx.guildIcon(g), h('span', { class: 'profile-row-name' }, h('span', null, g.name), nickThere ? h('span', { class: 'profile-row-sub' }, nickThere) : null)));
      }
    } else {
      if (!friends.length) panel.appendChild(h('div', { class: 'profile-empty' }, friendCount ? friendCount + ' friends in common, but Discord didn’t list them.' : 'No friends in common.'));
      for (const f of friends) {
        panel.appendChild(h('button', { type: 'button', class: 'profile-row', onclick: (e) => { e.stopPropagation(); ctx.openUser(f); } },
          avatarWithStatus(f, null, 'member-avatar'), h('span', { class: 'profile-row-name' }, h('span', null, displayName(f)), h('span', { class: 'profile-row-sub' }, f.username || ''))));
      }
    }
  };
  for (const [key, label] of tabs) {
    const tab = h('button', { type: 'button', role: 'tab', class: 'profile-tab', onclick: (e) => { e.stopPropagation(); show(key); } }, label);
    tab.dataset.key = key;
    bar.appendChild(tab);
  }
  if (tabs.length > 1) card.appendChild(bar);
  card.appendChild(panel);
  show('about');
  return card;
}

function about(panel, user, p, ctx) {
  const section = (title, ...children) => panel.appendChild(h('div', { class: 'profile-section' }, h('div', { class: 'profile-heading' }, title), ...children));
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
}

// Small buttons beside the avatar, as Discord's full profile has them.
function actions(user, self, ctx) {
  const row = h('div', { class: 'profile-actions' });
  const button = (label, path, fn, cls) => h('button', { type: 'button', class: 'button tiny ' + (cls || 'secondary'), title: label, onclick: (e) => { e.stopPropagation(); fn(); } },
    path ? icon(path, 14) : null, h('span', null, label));
  if (self) {
    row.appendChild(button('Set status', null, ctx.editStatus, 'brand'));
    return row;
  }
  const rel = ctx.relationship;
  if (rel !== BLOCKED && !user.bot) row.appendChild(button('Message', ICONS.chat, ctx.message, 'brand'));
  if (rel === FRIEND) row.appendChild(button('Remove Friend', null, ctx.remove, 'danger-outline'));
  else if (rel === INCOMING) row.appendChild(button('Accept Request', ICONS.check, ctx.accept, 'positive'));
  else if (rel === OUTGOING) row.appendChild(button('Request Sent', null, ctx.remove));
  else if (rel !== BLOCKED && !user.bot) row.appendChild(button('Add Friend', ICONS.add, ctx.add, 'positive'));
  row.appendChild(rel === BLOCKED ? button('Unblock', null, ctx.unblock) : button('Block', null, ctx.block, 'danger-outline'));
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
