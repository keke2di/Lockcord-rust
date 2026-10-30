// Names and avatars. Avatars are Discord's CDN images through LockCord's `lcmedia:` scheme
// (src-tauri/src/media.rs), so this page never loads anything from Discord itself.

import { media } from './api.js';
import { h } from './dom.js';

const users = new Map();

export function remember(user) {
  if (user && user.id) users.set(user.id, Object.assign(users.get(user.id) || {}, user));
  return user;
}

export function user(id) {
  return users.get(id) || null;
}

export function displayName(u) {
  if (!u) return 'Unknown';
  return u.global_name || u.username || 'Unknown';
}

export function nameOf(id) {
  const u = users.get(id);
  return u ? displayName(u) : null;
}

// Discord's avatar URL: the user's own, or the default one Discord picks for them.
export function avatarUrl(u) {
  if (!u || !u.id || !/^\d{1,20}$/.test(u.id)) return null;
  if (u.avatar && /^\w{1,64}$/.test(u.avatar)) return media('https://cdn.discordapp.com/avatars/' + u.id + '/' + u.avatar + '.webp?size=80');
  const disc = Number(u.discriminator) || 0;
  const index = disc > 0 ? disc % 5 : Number((BigInt(u.id) >> 22n) % 6n);
  return media('https://cdn.discordapp.com/embed/avatars/' + index + '.png');
}

export function avatar(u, cls = 'avatar') {
  const fallback = () => h('div', { class: cls + ' avatar-fallback', 'aria-hidden': 'true' }, displayName(u).slice(0, 1).toUpperCase());
  const url = avatarUrl(u);
  if (!url) return fallback();
  const img = h('img', { class: cls, src: url, alt: '', draggable: false });
  img.addEventListener('error', () => img.replaceWith(fallback()), { once: true });
  return img;
}

// An avatar with Discord's status dot in its corner (online, idle, dnd, offline); none if status
// is null.
export function avatarWithStatus(u, status, cls = 'avatar') {
  const wrap = h('div', { class: 'avatar-wrap ' + cls + '-wrap' }, avatar(u, cls));
  if (status) wrap.appendChild(h('span', { class: 'status-dot status-' + status, title: status }));
  return wrap;
}

// Someone already seen, by username or display name (a search's from: and mentions:).
export function findUser(name) {
  const q = String(name || '').trim().replace(/^@/, '').toLowerCase();
  if (!q) return null;
  let partial = null;
  for (const u of users.values()) {
    const names = [u.username, u.global_name].filter(Boolean).map((n) => n.toLowerCase());
    if (names.includes(q)) return u;
    if (!partial && names.some((n) => n.startsWith(q))) partial = u;
  }
  return partial;
}
