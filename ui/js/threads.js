// Threads, forum posts and pins: the lists LockCord shows for a channel. Rust fetches them and
// keeps the threads it saw (main.rs `threads`, `forum_posts`, `pins`; guilds.rs); this draws them.

import { h, icon, ICONS, snowflakeTime, stamp } from './dom.js';
import { parse, plain, render } from './markdown.js';
import { avatar, displayName } from './people.js';
import { channelIcon } from './servers.js';

export const FORUM_TYPES = new Set([15, 16]);

// "3 hours ago", from a snowflake.
function ago(id) {
  if (!id || !/^\d{1,20}$/.test(id)) return '';
  const s = Math.round((snowflakeTime(id) - Date.now()) / 1000);
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
  const abs = Math.abs(s);
  if (abs < 3600) return rtf.format(Math.round(s / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(s / 3600), 'hour');
  return rtf.format(Math.round(s / 86400), 'day');
}

function marks(t) {
  return [t.pinned ? h('span', { class: 'thread-mark', title: 'Pinned' }, '📌') : null,
    t.locked ? h('span', { class: 'thread-mark', title: 'Locked' }, '🔒') : null,
    t.archived ? h('span', { class: 'thread-mark', title: 'Archived' }, '🗄') : null];
}

// The Threads popout of a channel. data: { active, archived, has_more } or null while it loads.
// open(thread)
export function threadsPopup(channel, data, open) {
  const box = h('div', { class: 'popup threads-popup', onclick: (e) => e.stopPropagation() },
    h('div', { class: 'members-title' }, icon(ICONS.thread, 14), ' Threads in #' + (channel.name || '')));
  if (!data) {
    box.appendChild(h('div', { class: 'threads-empty' }, h('span', { class: 'spinner' }), ' Loading…'));
    return box;
  }
  const rowOf = (t) => h('button', { type: 'button', class: 'thread-row', onclick: () => open(t) },
    h('span', { class: 'thread-row-name' }, t.name || 'Thread', ...marks(t)),
    h('span', { class: 'thread-row-sub' }, (t.message_count || 0) + ' messages' + (t.last_message_id ? ' · ' + ago(t.last_message_id) : '')));
  const section = (title, list) => {
    if (!list.length) return;
    box.appendChild(h('div', { class: 'threads-section' }, title + ' — ' + list.length));
    for (const t of list) box.appendChild(rowOf(t));
  };
  section('Active', data.active || []);
  section('Archived', data.archived || []);
  if (!(data.active || []).length && !(data.archived || []).length) box.appendChild(h('div', { class: 'threads-empty' }, 'There are no threads here yet.'));
  else if (data.has_more) box.appendChild(h('div', { class: 'threads-empty' }, 'Older archived threads aren’t listed.'));
  return box;
}

// A forum or media channel: its posts as cards. data: { posts, has_more, loading, error }.
// ctx: { open(post), more(), md }
export function renderForum(pane, channel, data, ctx) {
  pane.textContent = '';
  const tags = new Map((channel.available_tags || []).map((t) => [t.id, t]));
  pane.appendChild(h('header', { class: 'chat-header' },
    h('div', { class: 'header-name' }, h('span', { class: 'header-hash', 'aria-hidden': 'true' }, channelIcon(channel.type, 24)), h('span', null, channel.name || ''),
      channel.topic ? h('span', { class: 'header-topic', title: channel.topic }, channel.topic) : null)));
  const list = h('div', { class: 'forum-list' });
  pane.appendChild(h('div', { class: 'forum-body' }, list));
  if (!data || (data.loading && !data.posts.length)) {
    list.appendChild(h('div', { class: 'threads-empty' }, h('span', { class: 'spinner' }), ' Loading posts…'));
    return;
  }
  if (data.error) list.appendChild(h('div', { class: 'threads-empty' }, 'Couldn’t load the posts: ' + data.error));
  for (const p of data.posts) list.appendChild(postCard(p, tags, ctx));
  if (!data.posts.length && !data.error) list.appendChild(h('div', { class: 'threads-empty' }, 'No posts here yet. LockCord can read and reply to posts; new ones are made in Discord for now.'));
  if (data.has_more) list.appendChild(h('button', { type: 'button', class: 'button secondary small forum-more', disabled: !!data.loading, onclick: ctx.more }, data.loading ? 'Loading…' : 'Load more posts'));
}

function postCard(p, tags, ctx) {
  const first = p.first || {};
  const image = (first.attachments || []).find((a) => /^image\//.test(a.content_type || '') && a.width);
  const excerpt = first.content ? plain(parse(first.content)).slice(0, 220) : '';
  const card = h('button', { type: 'button', class: 'forum-post', onclick: () => ctx.open(p) },
    h('div', { class: 'forum-post-main' },
      h('div', { class: 'forum-post-tags' }, (p.applied_tags || []).map((id) => tags.get(id)).filter(Boolean).map((t) => h('span', { class: 'forum-tag' }, (t.emoji ? t.emoji + ' ' : '') + t.name))),
      h('div', { class: 'forum-post-title' }, p.name || 'Post', ...marks(p)),
      excerpt ? h('div', { class: 'forum-post-excerpt' }, first.author ? h('strong', null, displayName(first.author) + ': ') : null, excerpt) : null,
      h('div', { class: 'forum-post-foot' }, icon(ICONS.chat, 14), ' ' + Math.max(0, (p.message_count || 0)) + ' · ' + (p.last_message_id ? ago(p.last_message_id) : ago(p.id)))),
    image ? h('img', { class: 'forum-post-image', src: ctx.media(image.proxy_url || image.url), alt: '', loading: 'lazy', draggable: false }) : null);
  return card;
}

// The pinned messages of a channel. list: messages, or null while loading.
// ctx: { md, canUnpin, jump(m), unpin(m) }
export function pinsPopup(list, ctx) {
  const box = h('div', { class: 'popup pins-popup', onclick: (e) => e.stopPropagation() },
    h('div', { class: 'members-title' }, icon(ICONS.pin, 14), ' Pinned Messages'));
  if (!list) {
    box.appendChild(h('div', { class: 'threads-empty' }, h('span', { class: 'spinner' }), ' Loading…'));
    return box;
  }
  if (!list.length) {
    box.appendChild(h('div', { class: 'threads-empty' }, 'This channel doesn’t have any pinned messages… yet.'));
    return box;
  }
  for (const m of list) box.appendChild(miniMessage(m, ctx.md, [
    ['Jump', () => ctx.jump(m)],
    ctx.canUnpin ? ['Unpin', () => ctx.unpin(m)] : null,
  ]));
  return box;
}

// A message drawn small, for pins and search results: avatar, name, time, text, and what else it
// carried. buttons: [label, onclick] or null.
export function miniMessage(m, md, buttons, onclick) {
  const files = (m.attachments || []).length;
  const text = m.content ? render(parse(m.content), document, md) : null;
  const node = h('div', { class: 'mini-message' + (onclick ? ' clickable' : ''), onclick },
    avatar(m.author || {}, 'mini-avatar'),
    h('div', { class: 'mini-main' },
      h('div', { class: 'mini-head' }, h('span', { class: 'mini-name' }, (m.member && m.member.nick) || displayName(m.author)),
        h('time', { class: 'timestamp' }, stamp(Date.parse(m.timestamp) || snowflakeTime(m.id)))),
      text ? h('div', { class: 'mini-text' }, text) : null,
      m.lc_e2e_locked || m.lc_e2e_failed ? h('div', { class: 'e2e-note' }, 'Encrypted message.') : null,
      files ? h('div', { class: 'mini-files' }, icon(ICONS.file, 14), ' ' + files + (files === 1 ? ' file' : ' files')) : null,
      m.poll ? h('div', { class: 'mini-files' }, '📊 ' + ((m.poll.question && m.poll.question.text) || 'Poll')) : null),
    h('div', { class: 'mini-buttons' }, (buttons || []).filter(Boolean).map(([label, fn]) => h('button', { type: 'button', class: 'button secondary small', onclick: (e) => { e.stopPropagation(); fn(); } }, label))));
  return node;
}
