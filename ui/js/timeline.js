// The DM's messages, drawn the way Discord draws them (cozy mode): groups with avatars, names and
// times, Discord's markdown, replies, reactions and attachments. A secret chat's messages sit
// among them in time order, marked with a lock, with the hover actions (react, reply, edit,
// delete) that work in a secret chat.

import { h, icon, ICONS, formatBytes, sameDay, stamp, shortTime, formatTime } from './dom.js';
import { parse, render, jumbo, plain } from './markdown.js';
import { avatar, displayName, nameOf, remember } from './people.js';
import { encryptedMark, plainMark, secretCard } from './trust.js';

const GROUP_GAP_MS = 7 * 60 * 1000;

const END_TEXT = {
  'you-ended': 'You ended the secret chat.',
  'peer-ended': 'They ended the secret chat.',
  mismatch: 'You said the codes didn’t match, so the chat was ended. Someone may have been in the middle.',
  'peer-mismatch': 'They said the codes didn’t match, so the chat was ended. Someone may have been in the middle.',
  'peer-left': 'They left before the chat opened.',
  'connect-failed': 'Couldn’t connect directly to them.',
  'handshake-failed': 'The encrypted connection couldn’t be set up.',
  'network-lost': 'The connection was lost.',
  tampered: 'Something on the connection was tampered with, so the chat was ended.',
  'server-lost': 'Lost the signaling server before the chat opened.',
  unavailable: 'That invite is no longer available.',
  expired: 'That invite has expired.',
  'rate-limited': 'Too many chats were started from here. Wait a moment and try again.',
  busy: 'The signaling server is busy. Try again in a moment.',
  unsupported: 'Secret chats can’t run here.',
  'invalid-link': 'That invite link isn’t valid.',
  relay: 'They could only connect through a relay. Secret chats are direct only, so it was ended.',
};

// Discord's system messages, in words: [text, optional link [label, onclick]]. Others aren't shown.
const other = (m) => (m.mentions && m.mentions[0] ? displayName(m.mentions[0]) : 'someone');
const SYSTEM_TYPES = {
  1: (m, n) => [n + ' added ' + other(m) + ' to the group.'],
  2: (m, n) => [n + ' removed ' + other(m) + ' from the group.'],
  3: (m, n) => [n + ' started a call.'],
  4: (m, n) => [n + ' changed the group name' + (m.content ? ': ' + m.content : '.')],
  5: (m, n) => [n + ' changed the group icon.'],
  6: (m, n, d) => [n + ' pinned a message to this channel.', ['See all pinned messages', () => d.openPins()]],
  7: (m, n) => [n + ' joined the server.'],
  8: (m, n) => [n + ' just boosted the server!'],
  9: (m, n) => [n + ' just boosted the server! It has reached Level 1!'],
  10: (m, n) => [n + ' just boosted the server! It has reached Level 2!'],
  11: (m, n) => [n + ' just boosted the server! It has reached Level 3!'],
  12: (m, n) => [n + ' has added ' + (m.content || 'a channel') + ' to this channel. Its most important updates will show up here.'],
  18: (m, n, d) => [n + ' started a thread: ' + (m.content || 'a thread') + '.',
    m.message_reference && m.message_reference.channel_id ? ['Open it', () => d.openThread({ id: m.message_reference.channel_id, name: m.content })] : null],
  46: (m, n) => {
    const field = (name) => ((((m.embeds || [])[0] || {}).fields || []).find((f) => f.name === name) || {}).value;
    const q = field('poll_question_text');
    return [n + '’s poll ' + (q ? '“' + q + '” ' : '') + 'has closed.'];
  },
};

function systemRow(m, d) {
  const [text, link] = SYSTEM_TYPES[m.type](m, d.authorName(m), d);
  const li = h('li', { class: 'system-message' }, text);
  if (link) li.append(' ', h('button', { type: 'button', class: 'system-link', onclick: (e) => { e.stopPropagation(); link[1](); } }, link[0]));
  li.dataset.id = m.id;
  return li;
}

// ctx: { me, peer, openLink, secret actions (verify, toggleReaction, startReply, startEdit,
//        confirmDelete, openReactionPicker, cancelFile, openViewer, editor(id), canEdit(id)),
//        editing, secretMenu(r, event), discord: Discord message actions (isMine, canReply,
//        canReact, authorName, authorColor, react, pick, reply, edit, remove, jump, menu, editor,
//        editing),
//        select: selection mode (active, has(key), toggle(key)),
//        roleOf(id) and channelName(id): a server's role and channel mentions,
//        media(url) and emojiUrl(id, animated): Discord's media as this page may load it }
export function renderTimeline(list, items, ctx) {
  const md = { nameOf, openLink: ctx.openLink, formatTime, emojiUrl: ctx.emojiUrl, roleOf: ctx.roleOf, channelName: ctx.channelName };
  list.textContent = '';
  generation++;
  list.classList.toggle('selecting', ctx.select.active);
  // A few new rows are messages arriving (they slide in); many are a history being drawn.
  const unseen = items.filter((i) => (i.kind === 'discord' || i.kind === 'secret') && !seen.has(rowKey(i))).length;
  animateNew = seen.size > 0 && unseen > 0 && unseen <= 3;
  if (seen.size > 20000) seen.clear();
  let prev = null;
  for (const item of items) {
    if (!prev || !sameDay(new Date(prev.at), new Date(item.at))) {
      list.appendChild(h('li', { class: 'day-divider', role: 'separator' }, h('span', null, new Date(item.at).toLocaleDateString([], { dateStyle: 'long' }))));
      prev = null;
    }
    if (item.kind === 'discord') {
      const node = safely(() => discordRow(item, prev, md, ctx), item);
      if (node) list.appendChild(node);
    } else if (item.kind === 'secret') {
      list.appendChild(safely(() => secretRow(item, prev, md, ctx), item));
    } else if (item.kind === 'card') {
      list.appendChild(card(item.chat, ctx));
    } else if (item.kind === 'offer') {
      list.appendChild(offerCard(ctx));
    } else if (item.kind === 'system') {
      list.appendChild(h('li', { class: 'system-message' }, icon(ICONS.lock, 14), ' ', item.r.text));
    }
    prev = item;
  }
  // Media not drawn this time is let go; GIF-videos moved by the redraw play on.
  for (const [key, entry] of kept) if (entry.gen !== generation) kept.delete(key);
  for (const v of list.querySelectorAll('video.media.gifv')) if (v.paused) v.play().catch(() => {});
}

const seen = new Set();
let animateNew = false;
const rowKey = (item) => (item.kind === 'discord' ? item.m.id : 'secret:' + item.r.id);

function markFresh(li, item) {
  const key = rowKey(item);
  if (seen.has(key)) return;
  seen.add(key);
  if (animateNew) li.classList.add('fresh');
}

// One message that can't be drawn mustn't take the rest of the timeline with it.
function safely(draw, item) {
  try {
    return draw();
  } catch (e) {
    console.error('message not drawn', item.m ? item.m.id : item.r && item.r.id, e);
    return h('li', { class: 'system-message' }, 'This message couldn’t be shown.');
  }
}

// The timeline is redrawn on every event (a typing indicator, a reaction), so media nodes are
// kept by key and moved into the new rows rather than rebuilt: images don't flicker and GIFs
// don't restart.
const kept = new Map();   // key -> { node, gen }
let generation = 0;
function keep(key, build) {
  const hit = kept.get(key);
  if (hit) { hit.gen = generation; return hit.node; }
  const node = build();
  kept.set(key, { node, gen: generation });
  return node;
}

function authorOf(item, ctx) {
  if (item.kind === 'discord') return item.m.author || {};
  return item.r.from === 'me' ? ctx.me : ctx.peer;
}

function startsGroup(item, prev, ctx) {
  if (!prev || (prev.kind !== 'discord' && prev.kind !== 'secret')) return true;
  if (prev.kind !== item.kind) return true;
  // After a system message (a join, a pin) a new group starts.
  if (prev.kind === 'discord' && ![0, 19, 20, 23].includes(prev.m.type)) return true;
  if ((authorOf(prev, ctx) || {}).id !== (authorOf(item, ctx) || {}).id) return true;
  if (item.at - prev.at > GROUP_GAP_MS) return true;
  if (item.kind === 'discord' && item.m.message_reference) return true;
  if (item.kind === 'secret' && item.r.replyTo) return true;
  return false;
}

// name and color: a server nickname and role colour, where the author has them; profile(anchor)
// opens the author's profile from their avatar or name.
function row({ start, author, at, body, cls, reply, name, color, profile }) {
  const li = h('li', { class: 'message' + (start ? ' group-start' : '') + (cls ? ' ' + cls : '') });
  if (reply) li.appendChild(reply);
  if (start) {
    const face = avatar(author);
    const username = h('span', { class: 'username' }, name || displayName(author));
    if (profile) {
      for (const node of [face, username]) {
        node.classList.add('opens-profile');
        node.addEventListener('click', (e) => { e.stopPropagation(); profile(e.currentTarget); });
      }
    }
    li.appendChild(face);
    if (color) username.style.color = color;
    li.appendChild(h('h3', { class: 'header' }, username,
      h('time', { class: 'timestamp', title: new Date(at).toLocaleString() }, stamp(at))));
  } else {
    li.appendChild(h('time', { class: 'gutter-time', title: new Date(at).toLocaleString() }, shortTime(at)));
  }
  li.appendChild(body);
  return li;
}

function content(text, md, edited, mentions) {
  for (const u of mentions || []) remember(u);
  const tree = parse(text);
  const node = h('div', { class: 'content' + (jumbo(tree) ? ' jumbo' : '') });
  node.appendChild(render(tree, document, md));
  if (edited) node.appendChild(h('span', { class: 'edited', title: 'Edited' }, ' (edited)'));
  return node;
}

// --- Discord's own messages -------------------------------------------------------------------

// A reaction's emoji as the react command takes it: the emoji itself, or `name:id` if custom.
export function emojiKey(emoji) {
  return emoji.id ? emoji.name + ':' + emoji.id : emoji.name;
}

function discordRow(item, prev, md, ctx) {
  const m = item.m;
  const d = ctx.discord;
  remember(m.author);
  if (SYSTEM_TYPES[m.type]) return systemRow(m, d);
  if (m.type === 21) return starterRow(item, md, ctx);
  // Messages, replies, and a bot's answers to slash commands and app commands.
  if (![0, 19, 20, 23].includes(m.type)) return null;
  // A LockCord key message (e2e.rs): a line, not its text.
  if (m.lc_key) return keyLine(m, d);

  const editing = d.editing === m.id;
  const forward = isForward(m);
  const body = editing ? d.editor(m) : h('div', { class: 'body' });
  if (!editing) parts(body, m, md, ctx, m.id);
  if (!editing) e2eNote(body, m, d);
  if (!editing && m.poll) body.appendChild(pollOf(m, d));
  if (m.thread && m.thread.id) body.appendChild(threadChip(m.thread, d));
  else for (const node of [...attachmentsOf(m, ctx, m.id), ...embedsOf(m, md, ctx, m.id)]) body.appendChild(node);
  // A forward carries the message it forwards as a snapshot, drawn as Discord does.
  if (forward) {
    const snap = m.message_snapshots[0].message || {};
    const box = h('div', { class: 'forward' }, h('div', { class: 'forward-label' }, icon(ICONS.reply, 14), ' Forwarded'));
    parts(box, snap, md, ctx, m.id + ':fwd');
    if (snap.lc_e2e_locked) box.appendChild(h('div', { class: 'e2e-note' }, 'An encrypted message, forwarded: it can only be read where it was sent.'));
    const at = Date.parse(snap.timestamp);
    if (at) box.appendChild(h('div', { class: 'forward-time', title: new Date(at).toLocaleString() }, stamp(at)));
    body.appendChild(box);
  }
  if (m.reactions && m.reactions.length) {
    body.appendChild(h('div', { class: 'reactions' }, m.reactions.map((r) => h('button', {
      type: 'button', class: 'reaction' + (r.me ? ' mine' : ''), title: r.emoji.id ? ':' + r.emoji.name + ':' : r.emoji.name,
      onclick: () => d.react(m, emojiKey(r.emoji), !r.me),
    }, h('span', { class: 'reaction-emoji' }, r.emoji.id
      ? h('img', { class: 'reaction-img', src: ctx.emojiUrl(r.emoji.id, r.emoji.animated), alt: ':' + r.emoji.name + ':', draggable: false })
      : r.emoji.name), h('span', { class: 'reaction-count' }, String(r.count))))));
  }

  let reply = null;
  if (m.message_reference && !forward) {
    const ref = m.referenced_message;
    reply = h('div', { class: 'reply-context' + (ref ? '' : ' missing') });
    if (ref) {
      reply.classList.add('jumps');
      reply.onclick = () => d.jump(ref.id);
      remember(ref.author);
      reply.appendChild(avatar(ref.author, 'reply-avatar'));
      const refName = h('span', { class: 'reply-name' }, d.authorName(ref));
      const refColor = d.authorColor(ref);
      if (refColor) refName.style.color = refColor;
      reply.appendChild(refName);
      reply.appendChild(h('span', { class: 'reply-snippet' }, plain(parse(ref.content || '')).slice(0, 120) || (isForward(ref) ? 'Forwarded message' : 'Click to see attachment')));
    } else {
      reply.appendChild(h('span', { class: 'reply-snippet' }, 'Original message was deleted'));
    }
  }
  const li = row({ start: startsGroup(item, prev, ctx), author: m.author, at: item.at, body, reply, cls: (editing ? 'editing' : '') + (d.isMine(m) ? ' mine' : ''), name: d.authorName(m), color: d.authorColor(m),
    profile: m.author && m.author.id ? (anchor) => d.profile(m.author, anchor) : null });
  li.dataset.id = m.id;
  markFresh(li, item);
  wire(li, 'discord:' + m.id, d.isMine(m), ctx, (e) => d.menu(m, e));
  li.appendChild(actionBar([
    d.canReact && ['Add Reaction', ICONS.react, (e) => { e.stopPropagation(); d.pick(m, e.currentTarget); }],
    d.canReply && ['Reply', ICONS.reply, () => d.reply(m)],
    d.isMine(m) && !forward && ['Edit', ICONS.edit, () => d.edit(m)],
    d.isMine(m) && ['Delete Message', ICONS.trash, (e) => { e.stopPropagation(); d.remove(m, e.shiftKey); }, 'danger'],
  ]));
  return li;
}

// Discord's message_reference type 1 is a forward; 0 (or none) a reply.
export function isForward(m) {
  return !!(m.message_reference && m.message_reference.type === 1 && m.message_snapshots && m.message_snapshots.length);
}

// A message's text, attachments, stickers and embeds, into node: a message itself or a forward's
// snapshot. A message that's only the link of the GIF or image it embeds shows just the picture,
// as Discord does.
function parts(node, m, md, ctx, key) {
  const text = (m.content || '').trim();
  const onlyLink = (m.embeds || []).some((e) => (e.type === 'gifv' || e.type === 'image') && e.url === text);
  // A translation shows in place of the text, with where it came from and the original a
  // click away (ctx.discord.translation; messages only, never a forward's snapshot).
  const t = key === m.id && ctx.discord.translation ? ctx.discord.translation(m) : null;
  if (m.content && !onlyLink) node.appendChild(content(t && !t.original ? t.text : m.content, md, !!m.edited_timestamp, m.mentions));
  if (t) {
    node.appendChild(h('div', { class: 'translated' }, ctx.discord.globe(),
      t.original ? 'Original' : 'Translated from ' + t.from, ' · ',
      h('button', { type: 'button', class: 'translated-toggle', onclick: (e) => { e.stopPropagation(); ctx.discord.toggleOriginal(m.id); } }, t.original ? 'Show translation' : 'Show original')));
  }
  for (const n of [...attachmentsOf(m, ctx, key), ...stickersOf(m, ctx, key), ...embedsOf(m, md, ctx, key)]) node.appendChild(n);
}

const MAX_MEDIA_W = 400;
const MAX_MEDIA_H = 300;
const IMAGE_TYPES = /^image\/(png|jpeg|gif|webp)$/;
const VIDEO_TYPES = /^video\/(mp4|webm)$/;
const MAX_VIDEO_BYTES = 25 * 1024 * 1024;

// A video attachment with the player's controls; nothing loads until it's played. The poster is
// Discord's media proxy's still of it.
function videoPlayer(a, ctx) {
  const [dw, dh] = fit(a.width || 400, a.height || 225);
  let poster = null;
  try {
    const u = new URL(a.proxy_url || a.url);
    if (u.hostname === 'media.discordapp.net') { u.searchParams.set('format', 'jpeg'); u.searchParams.set('width', String(dw)); u.searchParams.set('height', String(dh)); poster = ctx.media(u.toString()); }
  } catch (e) { /* no poster */ }
  const video = h('video', { class: 'attachment-video', src: ctx.media(a.proxy_url || a.url), controls: true, preload: 'none', width: dw, height: dh, playsInline: true, poster });
  video.addEventListener('click', (e) => e.stopPropagation());
  const spoiler = (a.filename || '').startsWith('SPOILER_') || ((a.flags || 0) & 8) !== 0;
  const wrap = h('div', { class: 'media-wrap video-wrap' + (spoiler ? ' media-spoiler' : ''), style: 'width:' + dw + 'px;height:' + dh + 'px' }, video);
  if (spoiler) wrap.addEventListener('click', () => wrap.classList.remove('media-spoiler'), { once: true });
  return wrap;
}

// w×h scaled down to fit the box, never up.
function fit(width, height, maxW = MAX_MEDIA_W, maxH = MAX_MEDIA_H) {
  const scale = Math.min(1, maxW / width, maxH / height);
  return [Math.max(1, Math.round(width * scale)), Math.max(1, Math.round(height * scale))];
}

// Discord's media proxy resizes on request, as the web client asks it to; GIFs stay as they are
// so they keep moving.
function sized(url, width, height, gif) {
  try {
    const u = new URL(url);
    if (u.hostname !== 'media.discordapp.net' || gif) return url;
    u.searchParams.set('width', String(width));
    u.searchParams.set('height', String(height));
    return u.toString();
  } catch (e) {
    return url;
  }
}

// An inline picture (or GIF-video) of a known size; a click opens the full one. Spoilers stay
// blurred until clicked once.
function picture(key, src, width, height, opts, ctx) {
  return keep(key + '|' + src, () => buildPicture(src, width, height, opts, ctx));
}

function buildPicture(src, width, height, { full, name, video, spoiler }, ctx) {
  const [dw, dh] = fit(width, height);
  const node = video
    ? h('video', { class: 'media gifv', src: ctx.media(src), width: dw, height: dh, autoplay: true, loop: true, muted: true, playsInline: true, preload: 'auto' })
    : h('img', { class: 'media', src: ctx.media(src), width: dw, height: dh, alt: name || '', loading: 'lazy', draggable: false });
  if (video) node.muted = true;
  const wrap = h('div', { class: 'media-wrap' + (spoiler ? ' media-spoiler' : ''), style: 'width:' + dw + 'px;height:' + dh + 'px' }, node);
  wrap.addEventListener('click', (e) => {
    e.stopPropagation();
    if (wrap.classList.contains('media-spoiler')) { wrap.classList.remove('media-spoiler'); return; }
    if (full) ctx.openViewer({ url: ctx.media(full), name });
  });
  return wrap;
}

function attachmentsOf(m, ctx, key) {
  return (m.attachments || []).map((a, i) => {
    const type = (a.content_type || '').split(';')[0];
    if (IMAGE_TYPES.test(type) && a.width && a.height) {
      const gif = type === 'image/gif';
      const [w, ht] = fit(a.width, a.height);
      return picture(key + ':a' + i, sized(a.proxy_url || a.url, w, ht, gif), a.width, a.height, { full: a.url, name: a.filename, spoiler: (a.filename || '').startsWith('SPOILER_') || ((a.flags || 0) & 8) !== 0 }, ctx);
    }
    // A video small enough for LockCord's media fetch (media.rs) plays in place.
    if (VIDEO_TYPES.test(type) && (a.size || 0) <= MAX_VIDEO_BYTES) {
      return keep(key + ':v' + i + '|' + a.url, () => videoPlayer(a, ctx));
    }
    return h('div', { class: 'file-card' }, h('div', { class: 'file-icon' }, icon(ICONS.file, 28)),
      h('div', { class: 'file-meta' },
        h('a', { class: 'file-name link', href: '#', title: 'Open in your browser', onclick: (e) => { e.preventDefault(); ctx.openLink(a.url); } }, a.filename || 'file'),
        h('div', { class: 'file-size' }, formatBytes(a.size || 0))));
  });
}

// Discord's sticker formats: 1 PNG, 2 APNG, 3 Lottie (not drawn here: its name instead), 4 GIF.
function stickersOf(m, ctx, key) {
  return (m.sticker_items || []).map((s) => {
    if (!/^\d{1,20}$/.test(s.id) || s.format_type === 3) return h('div', { class: 'sticker-name' }, '[' + (s.name || 'sticker') + ']');
    const ext = s.format_type === 4 ? 'gif' : 'png';
    return keep(key + ':s' + s.id, () => h('img', { class: 'sticker', src: ctx.media('https://media.discordapp.net/stickers/' + s.id + '.' + ext + '?size=160'), width: 160, height: 160, alt: s.name || 'sticker', title: s.name || '', loading: 'lazy', draggable: false }));
  });
}

function embedsOf(m, md, ctx, key) {
  return (m.embeds || []).map((e, i) => embed(e, md, ctx, key + ':e' + i)).filter(Boolean);
}

function embed(e, md, ctx, key) {
  // A GIF from Tenor, Giphy and the like is a looping video, as in Discord.
  if (e.type === 'gifv' && e.video && e.video.proxy_url && e.video.width && e.video.height) {
    return picture(key, e.video.proxy_url, e.video.width, e.video.height, { video: true }, ctx);
  }
  const pic = e.image && e.image.proxy_url && e.image.width ? e.image : null;
  const thumb = e.thumbnail && e.thumbnail.proxy_url && e.thumbnail.width ? e.thumbnail : null;
  // A link straight to an image is just the image.
  if (e.type === 'image' && (pic || thumb)) {
    const p = pic || thumb;
    return picture(key, p.proxy_url, p.width, p.height, { full: p.proxy_url, name: '' }, ctx);
  }
  if (!e.title && !e.description && !pic && !thumb) return null;
  const box = h('div', { class: 'embed' });
  if (e.color) box.style.borderLeftColor = '#' + Number(e.color).toString(16).padStart(6, '0');
  const main = h('div', { class: 'embed-main' });
  if (e.provider && e.provider.name) main.appendChild(h('div', { class: 'embed-provider' }, e.provider.name));
  if (e.author && e.author.name) main.appendChild(h('div', { class: 'embed-author' }, e.author.name));
  if (e.title) main.appendChild(e.url
    ? h('a', { class: 'embed-title md-link', href: '#', onclick: (ev) => { ev.preventDefault(); ctx.openLink(e.url); } }, e.title)
    : h('div', { class: 'embed-title' }, e.title));
  if (e.description) main.appendChild(content(e.description, md, false));
  box.appendChild(main);
  if (thumb && !pic) {
    const [w, h2] = fit(thumb.width, thumb.height, 80, 80);
    box.appendChild(h('img', { class: 'embed-thumb', src: ctx.media(thumb.proxy_url), width: w, height: h2, alt: '', loading: 'lazy', draggable: false }));
  }
  if (pic) box.appendChild(picture(key + ':img', pic.proxy_url, pic.width, pic.height, { full: pic.proxy_url, name: '' }, ctx));
  return box;
}

// Right-click opens a message's menu. While selecting, a click ticks one of your own messages
// (key: 'discord:<id>' or 'secret:<id>'); nothing else in the row reacts to clicks then.
function wire(li, key, mine, ctx, menu) {
  li.addEventListener('contextmenu', (e) => { e.preventDefault(); if (!ctx.select.active) menu(e); });
  if (!ctx.select.active || !mine) return;
  li.classList.add('selectable');
  if (ctx.select.has(key)) li.classList.add('selected');
  li.addEventListener('click', () => ctx.select.toggle(key));
}

function keyLine(m, d) {
  if (m.lc_key === 'ignored') return null;
  const mine = m.lc_key === 'mine';
  const text = m.lc_key_changed ? '⚠ ' + d.peerName() + '’s LockCord posted a different key. Nothing is sent until you check it in the header.'
    : mine ? (m.lc_key_role === 'offer' ? 'You turned on encryption here.' : 'Your LockCord answered with its key.')
    : (m.lc_key_role === 'offer' ? d.peerName() + '’s LockCord shared its key.' : d.peerName() + '’s LockCord answered: this DM is encrypted.');
  return h('li', { class: 'system-message key-line' + (m.lc_key_changed ? ' warn' : '') }, icon(ICONS.lock, 14), ' ', text);
}

// Encrypted DMs (e2e.rs opened them in Rust): a lock on those that were; a note on those that
// can't be read; and, in an encrypted DM, a word on those that weren't encrypted.
function e2eNote(body, m, d) {
  if (m.lc_e2e) {
    const text = body.querySelector('.content');
    (text || body).appendChild(encryptedMark(m.lc_e2e_replayed ? 'copy' : m.lc_e2e_stale ? 'stale' : null));
    if (m.lc_e2e_replayed) body.appendChild(h('div', { class: 'e2e-note warn' }, 'A copy: this exact encrypted message was already posted here before, so someone posted it again.'));
    else if (m.lc_e2e_stale) body.appendChild(h('div', { class: 'e2e-note warn' }, 'Sent long before it was posted: it may be an old message posted again.'));
  } else if (m.lc_e2e_failed) {
    body.appendChild(h('div', { class: 'e2e-note warn' }, 'This encrypted message couldn’t be decrypted: it was changed, or isn’t from who it says.'));
  } else if (m.lc_e2e_untrusted) {
    body.appendChild(h('div', { class: 'e2e-note warn' }, 'Encrypted with a key you haven’t accepted (their key changed). See the header.'));
  } else if (m.lc_e2e_locked) {
    body.appendChild(h('div', { class: 'e2e-note' }, 'Encrypted message. You don’t have the key for it.'));
  } else if (d.e2eHere && m.content) {
    body.appendChild(h('div', { class: 'e2e-note plain' }, plainMark()));
  }
}

// The hover bar: [title, icon, onclick, class] per button; falsy entries are skipped.
function actionBar(buttons) {
  const bar = h('div', { class: 'actions', role: 'group', 'aria-label': 'Message actions' });
  for (const b of buttons) {
    if (!b) continue;
    const [title, path, fn, cls] = b;
    bar.appendChild(h('button', { type: 'button', class: 'action' + (cls ? ' ' + cls : ''), title, 'aria-label': title, onclick: fn }, icon(path, 20)));
  }
  return bar;
}

// --- A secret chat's messages ------------------------------------------------------------------

function secretRow(item, prev, md, ctx) {
  const r = item.r;
  const author = r.from === 'me' ? ctx.me : ctx.peer;
  let body = h('div', { class: 'body' });
  if (ctx.editing === r.id) {
    body = ctx.editor(r);
  } else {
    if (r.text) body.appendChild(content(r.text, md, r.edited));
    if (r.kind === 'image' && r.url) {
      body.appendChild(h('div', { class: 'attachment' }, h('img', { class: 'attachment-image', src: r.url, alt: r.name || 'image', draggable: false, onclick: (e) => { e.stopPropagation(); ctx.openViewer(r); } })));
    } else if (r.kind !== 'text') {
      body.appendChild(fileCard(r, ctx));
    }
    if (r.reactions && r.reactions.length) {
      body.appendChild(h('div', { class: 'reactions' }, r.reactions.map((x) => h('button', {
        type: 'button', class: 'reaction' + (x.mine ? ' mine' : ''), onclick: () => ctx.toggleReaction(r.id, x.emoji),
      }, h('span', { class: 'reaction-emoji' }, x.emoji), h('span', { class: 'reaction-count' }, String(x.count))))));
    }
  }

  let reply = null;
  if (r.replyTo) {
    const target = item.chat.records.find((x) => x.id === r.replyTo && !x.deleted);
    reply = h('div', { class: 'reply-context' + (target ? '' : ' missing') });
    if (target) {
      const who = target.from === 'me' ? ctx.me : ctx.peer;
      reply.appendChild(avatar(who, 'reply-avatar'));
      reply.appendChild(h('span', { class: 'reply-name' }, displayName(who)));
      reply.appendChild(h('span', { class: 'reply-snippet' }, (target.kind === 'text' ? plain(parse(target.text)) : target.text || target.name || 'Attachment').slice(0, 120)));
    } else {
      reply.appendChild(h('span', { class: 'reply-snippet' }, 'Original message was deleted'));
    }
  }

  const li = row({ start: startsGroup(item, prev, ctx), author, at: item.at, body, reply, cls: 'secret' + (r.from === 'me' ? ' mine' : '') + (r.pending && r.kind === 'text' ? ' pending' : '') + (ctx.editing === r.id ? ' editing' : ''),
    profile: author && author.id ? (anchor) => ctx.discord.profile(author, anchor) : null });
  li.dataset.id = r.id;
  markFresh(li, item);
  wire(li, 'secret:' + r.id, r.from === 'me', ctx, (e) => ctx.secretMenu(r, e));
  li.appendChild(actionBar([
    ['Add Reaction', ICONS.react, (e) => { e.stopPropagation(); ctx.openReactionPicker(r.id, e.currentTarget); }],
    ['Reply', ICONS.reply, () => ctx.startReply(r)],
    r.from === 'me' && r.kind === 'text' && ctx.canEdit(r.id) && ['Edit', ICONS.edit, () => ctx.startEdit(r.id)],
    r.from === 'me' && ['Delete Message', ICONS.trash, (e) => { e.stopPropagation(); ctx.confirmDelete(r, e.shiftKey); }, 'danger'],
  ]));
  return li;
}

function fileCard(r, ctx) {
  const done = !r.pending && r.state !== 'cancelled';
  const name = done && r.url
    ? h('a', { class: 'file-name link', href: r.url, download: r.name || 'file', title: r.name }, r.name)
    : h('div', { class: 'file-name' }, r.name || 'file');
  const pct = Math.round((r.progress || 0) * 100);
  const meta = h('div', { class: 'file-meta' }, name, h('div', { class: 'file-size' },
    r.state === 'cancelled' ? 'Cancelled' : r.pending ? (r.from === 'me' ? 'Sending… ' : 'Receiving… ') + pct + '%' : formatBytes(r.size || 0)));
  const node = h('div', { class: 'file-card' }, h('div', { class: 'file-icon' }, icon(ICONS.file, 28)), meta);
  if (r.pending && r.state !== 'cancelled') {
    node.appendChild(h('button', { type: 'button', class: 'file-cancel', title: 'Cancel', 'aria-label': 'Cancel', onclick: () => ctx.cancelFile(r.id) }, icon(ICONS.close, 16)));
    const bar = h('div', { class: 'file-progress' }, h('div', { class: 'file-progress-fill' }));
    bar.firstChild.style.width = pct + '%';
    node.appendChild(bar);
  }
  return node;
}

// --- The secret chat's status: waiting, the code, verified, ended ------------------------------

// The secret chat's status, drawn by trust.js where themes can't reach it.
function card(chat, ctx) {
  const them = displayName(ctx.peer);
  const v = chat.verification;
  let spec;
  if (chat.phase === 'invited') {
    spec = {
      tone: 'gate', title: them + ' invited you to a secret chat',
      text: ['A secret chat connects you straight to ' + them + ', so joining shows them your IP address, before you compare the code.'
        + (chat.stranger ? ' ' + them + ' isn’t on your friends list.' : '')],
      buttons: [{ label: 'Join', yes: true, onclick: () => ctx.joinSecret() }, { label: 'Ignore', onclick: () => ctx.ignoreSecret() }],
    };
  } else if (chat.ended) {
    spec = { tone: 'ended', title: 'Secret chat ended', text: [(END_TEXT[chat.ended] || 'The secret chat ended.') + ' Its messages are gone.'] };
  } else if (chat.phase === 'open' && v.me && v.peer) {
    spec = { tone: 'quiet verified', lock: true, text: ['Secret chat with ' + them + ': end-to-end encrypted, peer to peer. You both confirmed the code ',
      { code: v.sas }, '. Discord sees none of it.' + (chat.reconnecting ? ' Reconnecting…' : '')] };
  } else if (chat.phase === 'open') {
    spec = {
      tone: 'gate', title: 'Compare the code with ' + them, code: v.sas,
      text: ['Read it to them on a call or in person. The secret chat opens once you have both confirmed it. If it’s different, someone may be in the middle.'],
      status: v.me ? 'You confirmed. Waiting for ' + them + ' to confirm…' : v.peer ? them + ' says the codes match. Check yours.' : '',
      buttons: v.me ? [] : [{ label: 'Codes match', yes: true, onclick: () => ctx.verify(true) }, { label: 'They don’t match', onclick: () => ctx.verify(false) }],
    };
  } else {
    const text = chat.phase === 'inviting' ? 'Secret chat: waiting for ' + them + ' to join. The invite was posted here.'
      : chat.phase === 'joining' ? 'Joining ' + them + '’s secret chat…'
      : chat.phase === 'connecting' ? 'Connecting to ' + them + ' directly…'
      : 'Starting a secret chat…';
    spec = { tone: 'quiet', spinner: true, text: [text] };
  }
  const li = h('li', { class: 'status-card' }, secretCard(spec));
  li.style.setProperty('display', 'list-item', 'important');
  li.style.setProperty('visibility', 'visible', 'important');
  return li;
}

// The other side's LockCord offered to encrypt this DM: nothing is answered until the user says
// so (answering tells them this account runs LockCord).
function offerCard(ctx) {
  const them = displayName(ctx.peer);
  const li = h('li', { class: 'status-card' }, secretCard({
    tone: 'gate', title: them + ' wants to encrypt this DM',
    text: ['Their LockCord posted its key. Accepting posts yours, and from then on everything here is end-to-end encrypted: Discord stores only ciphertext. '
      + 'Answering also tells ' + them + ' that you use LockCord. Ignoring posts nothing.'],
    buttons: [{ label: 'Accept', yes: true, onclick: () => ctx.acceptOffer() }, { label: 'Ignore', onclick: () => ctx.ignoreOffer() }],
  }));
  li.style.setProperty('display', 'list-item', 'important');
  li.style.setProperty('visibility', 'visible', 'important');
  return li;
}

// --- Threads and polls ---------------------------------------------------------------------------

// A thread's first row: the message it was started from, as it is in the channel.
function starterRow(item, md, ctx) {
  const m = item.m;
  const d = ctx.discord;
  const ref = m.referenced_message;
  if (!ref) return h('li', { class: 'system-message' }, 'The message this thread started from was deleted.');
  remember(ref.author);
  const body = h('div', { class: 'body' });
  parts(body, ref, md, ctx, m.id + ':starter');
  const at = Date.parse(ref.timestamp) || item.at;
  const li = row({ start: true, author: ref.author, at, body, cls: 'starter', name: d.authorName(ref), color: d.authorColor(ref),
    profile: ref.author && ref.author.id ? (anchor) => d.profile(ref.author, anchor) : null });
  li.dataset.id = m.id;
  return li;
}

// Under a message a thread was started from: the thread, a click away.
function threadChip(t, d) {
  const count = t.message_count || 0;
  return h('button', { type: 'button', class: 'thread-chip', onclick: (e) => { e.stopPropagation(); d.openThread(t); } },
    icon(ICONS.thread, 16), h('span', { class: 'thread-chip-name' }, t.name || 'Thread'),
    h('span', { class: 'thread-chip-count' }, count === 1 ? '1 message' : count + ' messages'), icon(ICONS.chevronRight, 16));
}

// Picks in a poll that allows several answers, kept across redraws until the vote goes.
const pollPicks = new Map();

function pollOf(m, d) {
  const p = m.poll;
  const counts = new Map(((p.results && p.results.answer_counts) || []).map((a) => [a.id, a]));
  const total = [...counts.values()].reduce((n, a) => n + (a.count || 0), 0);
  const expiry = p.expiry ? Date.parse(p.expiry) : 0;
  const closed = !!(p.results && p.results.is_finalized) || (!!expiry && expiry <= Date.now());
  const voted = [...counts.values()].some((a) => a.me_voted);
  const showResults = voted || closed;
  const multi = !!p.allow_multiselect;
  const picks = pollPicks.get(m.id) || new Set();
  const box = h('div', { class: 'poll' + (closed ? ' closed' : '') },
    h('div', { class: 'poll-question' }, (p.question && p.question.text) || 'Poll'),
    h('div', { class: 'poll-hint' }, closed ? 'Poll closed' : voted ? 'You voted' : multi ? 'Select one or more answers' : 'Select one answer'));
  for (const a of p.answers || []) {
    const c = counts.get(a.answer_id) || { count: 0 };
    const pct = total ? Math.round((c.count || 0) * 100 / total) : 0;
    const media = a.poll_media || {};
    const picked = picks.has(a.answer_id);
    const answer = h('button', {
      type: 'button', class: 'poll-answer' + (c.me_voted ? ' mine' : '') + (picked ? ' picked' : ''), disabled: showResults,
      onclick: (e) => {
        e.stopPropagation();
        if (!multi) { d.vote(m, [a.answer_id]); return; }
        if (picks.has(a.answer_id)) picks.delete(a.answer_id); else picks.add(a.answer_id);
        pollPicks.set(m.id, picks);
        d.redraw();
      },
    }, h('span', { class: 'poll-answer-text' }, media.emoji && media.emoji.name && !media.emoji.id ? media.emoji.name + ' ' : '', media.text || ''),
      showResults ? h('span', { class: 'poll-answer-count' }, (c.count || 0) + ' · ' + pct + '%') : null,
      c.me_voted ? icon(ICONS.check, 16) : null);
    if (showResults) answer.style.setProperty('--poll-share', pct + '%');
    box.appendChild(answer);
  }
  const left = expiry && !closed ? timeLeft(expiry - Date.now()) : null;
  const foot = h('div', { class: 'poll-foot' }, total + (total === 1 ? ' vote' : ' votes') + (left ? ' · ' + left + ' left' : ''));
  if (!closed && multi && !voted) foot.appendChild(h('button', { type: 'button', class: 'button brand small', disabled: !picks.size, onclick: (e) => { e.stopPropagation(); pollPicks.delete(m.id); d.vote(m, [...picks]); } }, 'Vote'));
  if (!closed && voted) foot.appendChild(h('button', { type: 'button', class: 'button link small', onclick: (e) => { e.stopPropagation(); d.vote(m, []); } }, 'Remove Vote'));
  box.appendChild(foot);
  return box;
}

function timeLeft(ms) {
  const hours = Math.floor(ms / 3600000);
  if (hours >= 24) return Math.floor(hours / 24) + 'd';
  if (hours >= 1) return hours + 'h';
  return Math.max(1, Math.round(ms / 60000)) + 'm';
}
