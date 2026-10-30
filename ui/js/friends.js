// The Friends view: Online, All, Pending and Blocked, as Discord lists them, and Add Friend.
// Accepting, ignoring, cancelling, removing and unblocking go through LockCord (main.rs
// `relationship`); a request by username through `add_friend`.

import { h, icon, ICONS } from './dom.js';
import { avatarWithStatus, displayName } from './people.js';

const TABS = [['online', 'Online'], ['all', 'All'], ['pending', 'Pending'], ['blocked', 'Blocked']];
export const STATUS_TEXT = { online: 'Online', idle: 'Idle', dnd: 'Do Not Disturb', offline: 'Offline' };

// Discord's relationship types.
const FRIEND = 1;
const BLOCKED = 2;
const INCOMING = 3;
const OUTGOING = 4;

// The Add Friend form outlives redraws (presence updates redraw the view), so what's typed stays.
const addInput = h('input', { class: 'input add-friend-input', type: 'text', maxLength: 37, placeholder: 'You can add friends with their Discord username.', autocomplete: 'off', spellcheck: false, 'aria-label': 'Username' });
const addNote = h('div', { class: 'add-friend-note' });
let addBusy = false;

// ctx: { relationships, presences (Map id -> status), tab, setTab(tab), message(id), accept(id),
//        remove(relationship), profile(relationship, anchor), addFriend(username) -> Promise }
export function renderFriends(pane, ctx) {
  // Redrawing takes the form out of the page for a moment: it keeps the focus it had.
  const typing = document.activeElement === addInput;
  pane.textContent = '';
  const incoming = ctx.relationships.filter((r) => r.type === INCOMING).length;
  pane.appendChild(h('header', { class: 'chat-header friends-header' },
    h('div', { class: 'header-name friends-title' }, icon(ICONS.members, 22), h('span', null, 'Friends')),
    h('div', { class: 'friends-tabs', role: 'tablist' }, TABS.map(([id, label]) => h('button', {
      type: 'button', role: 'tab', class: 'friends-tab' + (ctx.tab === id ? ' selected' : ''), 'aria-selected': ctx.tab === id ? 'true' : 'false',
      onclick: () => ctx.setTab(id),
    }, label, id === 'pending' && incoming ? h('span', { class: 'badge' }, String(incoming)) : null)),
      h('button', { type: 'button', role: 'tab', class: 'friends-tab add-friend-tab' + (ctx.tab === 'add' ? ' selected' : ''), 'aria-selected': ctx.tab === 'add' ? 'true' : 'false', onclick: () => ctx.setTab('add') }, 'Add Friend'))));

  if (ctx.tab === 'add') {
    pane.appendChild(addFriendBody(ctx));
    if (typing || document.activeElement === document.body) addInput.focus({ preventScroll: true });
    return;
  }
  const rows = listFor(ctx);
  const list = h('div', { class: 'friends-list', role: 'list' }, rows.map((r) => row(r, ctx)));
  pane.appendChild(h('div', { class: 'friends-body' },
    h('div', { class: 'friends-count' }, countText(ctx.tab, rows.length)),
    rows.length ? list : h('div', { class: 'friends-empty' }, emptyText(ctx.tab))));
}

function name(r) {
  return r.nickname || displayName(r.user);
}

function listFor(ctx) {
  const status = (r) => ctx.presences.get(r.id) || 'offline';
  const pick = {
    online: (r) => r.type === FRIEND && status(r) !== 'offline',
    all: (r) => r.type === FRIEND,
    pending: (r) => r.type === INCOMING || r.type === OUTGOING,
    blocked: (r) => r.type === BLOCKED,
  }[ctx.tab];
  return ctx.relationships.filter(pick).sort((a, b) => (a.type - b.type) || name(a).localeCompare(name(b)));
}

function countText(tab, n) {
  const what = { online: 'Online', all: 'All friends', pending: 'Pending', blocked: 'Blocked' }[tab];
  return what + ' — ' + n;
}

function emptyText(tab) {
  return {
    online: 'No one’s around right now.',
    all: 'No friends yet.',
    pending: 'No pending friend requests.',
    blocked: 'You haven’t blocked anyone.',
  }[tab];
}

function row(r, ctx) {
  const status = ctx.presences.get(r.id) || 'offline';
  const sub = r.type === FRIEND ? STATUS_TEXT[status]
    : r.type === INCOMING ? 'Incoming Friend Request'
    : r.type === OUTGOING ? 'Outgoing Friend Request'
    : 'Blocked';
  const button = (title, path, fn, cls) => h('button', {
    type: 'button', class: 'friend-action' + (cls ? ' ' + cls : ''), title, 'aria-label': title,
    onclick: (e) => { e.stopPropagation(); fn(e); },
  }, icon(path, 20));
  const actions = [];
  if (r.type === FRIEND) actions.push(button('Message', ICONS.chat, () => ctx.message(r.id)), button('Remove Friend', ICONS.close, () => ctx.remove(r), 'danger'));
  if (r.type === INCOMING) actions.push(button('Accept', ICONS.check, () => ctx.accept(r.id), 'positive'), button('Ignore', ICONS.close, () => ctx.remove(r), 'danger'));
  if (r.type === OUTGOING) actions.push(button('Cancel', ICONS.close, () => ctx.remove(r), 'danger'));
  if (r.type === BLOCKED) actions.push(button('Unblock', ICONS.close, () => ctx.remove(r), 'danger'));
  return h('div', { class: 'friend', role: 'listitem', title: 'Profile', onclick: (e) => { e.stopPropagation(); ctx.profile(r, e.currentTarget); }, ondblclick: r.type === FRIEND ? () => ctx.message(r.id) : null },
    avatarWithStatus(r.user, r.type === FRIEND ? status : null, 'friend-avatar'),
    h('div', { class: 'friend-names' },
      h('div', { class: 'friend-name' }, name(r), h('span', { class: 'friend-username' }, r.user.username || '')),
      h('div', { class: 'friend-sub' }, sub)),
    h('div', { class: 'friend-actions' }, actions));
}

// What removing means for each kind, for the confirm dialog: [title, text, button].
export function removeWords(r) {
  const who = name(r);
  return {
    [FRIEND]: ['Remove ‘' + who + '’', 'Are you sure you want to remove ' + who + ' from your friends?', 'Remove Friend'],
    [INCOMING]: ['Ignore request', 'Ignore ' + who + '’s friend request?', 'Ignore'],
    [OUTGOING]: ['Cancel request', 'Cancel your friend request to ' + who + '?', 'Cancel Request'],
    [BLOCKED]: ['Unblock ‘' + who + '’', 'Unblock ' + who + '? They’ll be able to message you again.', 'Unblock'],
  }[r.type];
}

function addFriendBody(ctx) {
  const send = () => {
    const name = addInput.value.trim();
    if (!name || addBusy) return;
    addBusy = true;
    addNote.className = 'add-friend-note';
    addNote.textContent = 'Sending…';
    ctx.addFriend(name)
      .then(() => { addNote.className = 'add-friend-note ok'; addNote.textContent = 'Success! Your friend request to ' + name + ' was sent.'; addInput.value = ''; })
      .catch((e) => { addNote.className = 'add-friend-note error'; addNote.textContent = String(e); })
      .finally(() => { addBusy = false; });
  };
  addInput.onkeydown = (e) => { if (e.key === 'Enter') { e.preventDefault(); send(); } };
  const body = h('div', { class: 'friends-body add-friend' },
    h('h2', { class: 'add-friend-title' }, 'Add Friend'),
    h('div', { class: 'add-friend-text' }, 'You can add friends with their Discord username. Discord may ask for a captcha; it opens in its own window.'),
    h('div', { class: 'add-friend-row' }, addInput, h('button', { type: 'button', class: 'button brand small', onclick: send }, 'Send Friend Request')),
    addNote);
  return body;
}
