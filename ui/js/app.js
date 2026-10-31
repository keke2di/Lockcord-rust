// LockCord's window: sign-in, the DM list, a DM's messages and the composer, with secret chats
// inline. State lives here and in Rust; Discord's data arrives as LockCord's own events
// (src-tauri/src/main.rs), never from Discord's page.

import { api, media, on } from './api.js';
import { formatTime, h, icon, ICONS, snowflakeTime } from './dom.js';
import { EMOJI_CATEGORIES, REACTIONS, replaceShortcodes } from './emoji.js';
import { avatarWithStatus, displayName, nameOf, remember, user } from './people.js';
import { applyMemberList, colorFor, firstChannel, renderChannels, renderMembers, renderRail, roleOf, TEXT_TYPES } from './servers.js';
import { removeWords, renderFriends, STATUS_TEXT } from './friends.js';
import { createSecret } from './secret.js';
import { applyTheme, currentTheme, preset } from './theme.js';
import { encryptedBar, encryptedPill, safetyGrid, secretBar, secretCard, secretLock, secretPill } from './trust.js';
import { renderSettings } from './settings.js';
import { LANGS, langName } from './langs.js';
import { dropdown, toggle } from './controls.js';
import { emojiKey, isForward, renderTimeline } from './timeline.js';
import { friendPicker, myDot, profileCard, STATUS_WORDS, statusMenu } from './profile.js';
import { FORUM_TYPES, pinsPopup, renderForum, threadsPopup } from './threads.js';
import { createSearchBox, parseQuery, renderResults } from './search.js';
import { createCalls } from './call.js';

const MAX_TEXT = 4000;
const SECRET_DELETE_TEXT = 'Are you sure you want to delete this message? It goes from their screen too.';
const TYPING_SHOW_MS = 9000;

const state = {
  signedIn: false,
  me: null,
  dms: [],
  current: null,
  messages: new Map(),   // channel id -> Discord messages, oldest first
  gateway: 'connecting',
  typing: new Map(),     // channel id -> Map(user id -> until)
  replyTo: null,         // a secret message being replied to
  editing: null,         // a secret message being edited
  discordReply: null,    // { m, ping }: a Discord message being replied to
  discordEditing: null,  // the id of a Discord message being edited
  olderDone: new Set(),  // channels whose history is loaded back to the start
  loadingOlder: false,
  selection: null,       // { keys: Set('discord:<id>' | 'secret:<id>'), running: { done, total, stop } | null }
  composeMode: null,     // 'secret' or 'discord': what the composer's draft and files are for
  uploading: false,
  staged: [],            // files for a secret message
  lastTypingSent: 0,
  view: 'friends',       // 'friends', or null while a DM is open
  friendsTab: 'online',
  relationships: [],     // friends, requests, blocks (main.rs rel_view)
  presences: new Map(),  // user id -> online, idle, dnd, offline
  reads: new Map(),      // channel id -> { last, mentions }
  guilds: [],            // servers as guilds.rs views them, in the user's order
  guildId: null,         // the server shown, or null for Direct Messages
  lastChannel: new Map(),// server id -> the channel last open there
  muted: { guilds: new Set(), channels: new Set() },
  memberList: null,      // the open channel's member list (servers.js applyMemberList)
  showMemberList: true,
  sidePinned: false,     // the side panel kept open in place, instead of sliding out on hover
  translations: new Map(), // message id -> { text, detected } | 'pending' | 'failed'
  e2e: { contacts: [], problem: null }, // main.rs e2e_view
  plainNext: null,       // a channel whose next message goes unencrypted, on purpose
  showOriginal: new Set(), // message ids shown untranslated
  myStatus: { status: 'online' }, // people.rs status_view
  extraThreads: new Map(), // threads opened from a list, a forum or a message (guilds.rs thread_view)
  detached: new Set(),   // channels whose loaded messages stop short of the newest (after a jump)
  forum: null,           // the open forum's posts: { channel, posts, has_more, offset, loading, error }
  search: null,          // { scope, text, query, results, total, has_more, offset, loading, error }
  pins: new Map(),       // channel id -> its pinned messages, until they change
};

const secret = createSecret(() => refresh());

// Calls (call.js). Its helpers are this file's functions, used only once it's drawing.
const calls = createCalls({
  me: () => state.me,
  dmById: (id) => dmById(id),
  refresh: () => refresh(),
  toast: (text) => showToast(text),
  select: (id) => select(id),
  confirm: (title, text, button, yes) => confirmAction(title, text, button, yes),
  popup: (anchor, node) => popupAt(anchor, node),
  modal: (node) => showModal(node),
  closeLayer: () => closeLayer(),
  voiceSettings: () => (prefs.settings && prefs.settings.voice) || {},
  channelName: (id) => { const c = dmById(id); return c ? (c.guild_id ? '🔊 ' + (c.name || 'voice') : dmName(c)) : 'a call'; },
});

// --- Layout ----------------------------------------------------------------------------------

const root = h('div', { class: 'root' });
document.body.appendChild(root);

function screen(...children) {
  root.textContent = '';
  for (const c of children) root.appendChild(c);
}

// --- Sign-in ---------------------------------------------------------------------------------

function showSignIn(reason, waiting) {
  const error = h('div', { class: 'signin-error' }, reason || '');
  const tokenInput = h('input', { class: 'input', type: 'password', placeholder: 'Session token', autocomplete: 'off', spellcheck: false });
  const tokenRow = h('div', { class: 'token-row', hidden: true }, tokenInput,
    h('button', { type: 'button', class: 'button brand', onclick: () => {
      error.textContent = '';
      api.signInWithToken(tokenInput.value).catch((e) => { error.textContent = String(e); });
      tokenInput.value = '';
    } }, 'Sign in'));
  screen(h('div', { class: 'signin' }, h('div', { class: 'signin-card' },
    h('div', { class: 'signin-logo' }, icon(ICONS.lock, 40)),
    h('h1', null, 'LockCord'),
    h('p', null, waiting ? 'Sign in in the Discord window that opened. LockCord never sees your password.' : 'Sign in with your Discord account. Discord’s own login page opens in a separate window; LockCord never sees your password.'),
    h('button', { type: 'button', class: 'button brand wide', disabled: !!waiting, onclick: () => { api.signIn().then(() => showSignIn(null, true)).catch((e) => showSignIn(String(e))); } }, waiting ? 'Waiting for Discord…' : 'Sign in with Discord'),
    h('button', { type: 'button', class: 'button link', onclick: () => { tokenRow.hidden = !tokenRow.hidden; } }, 'Sign in with a token instead'),
    tokenRow,
    error,
    h('p', { class: 'fine-print' }, 'Third-party clients are against Discord’s Terms of Service, and accounts can be disabled for it. Use an account you can afford to lose.'))));
}

function showConnecting() {
  screen(h('div', { class: 'signin' }, h('div', { class: 'signin-card' }, h('div', { class: 'spinner big' }), h('p', null, 'Connecting to Discord…'))));
}

// --- The main screen ---------------------------------------------------------------------------

const dmList = h('div', { class: 'dm-list', role: 'list' });
const userPanel = h('div', { class: 'user-panel' });
const friendsButton = h('button', { type: 'button', class: 'dm friends-button', onclick: () => showFriends() });
const newGroupButton = h('button', { type: 'button', class: 'sidebar-title-button', title: 'Create DM', 'aria-label': 'Create DM', onclick: (e) => { e.stopPropagation(); showNewGroup(); } }, icon(ICONS.add, 16));
const homeSide = h('div', { class: 'sidebar-face' }, h('div', { class: 'sidebar-top' }, friendsButton), h('div', { class: 'sidebar-title' }, h('span', null, 'Direct Messages'), newGroupButton), dmList);
const guildTitle = h('div', { class: 'guild-title' });
const channelList = h('div', { class: 'channel-list', role: 'list' });
const guildSide = h('div', { class: 'sidebar-face' }, guildTitle, channelList);
// While in a call: its bar, over the user panel.
const callPanel = h('div', { class: 'call-panel-slot' });
const sidebar = h('nav', { class: 'sidebar' }, homeSide, callPanel, userPanel);
const rail = h('nav', { class: 'rail', 'aria-label': 'Servers' });

const header = h('header', { class: 'chat-header' });
const timeline = h('ol', { class: 'messages', 'aria-label': 'Messages' });
const scroller = h('div', { class: 'scroller' }, h('div', { class: 'scroller-inner' }, timeline, h('div', { class: 'scroller-spacer' })));
const replyBar = h('div', { class: 'reply-bar', hidden: true });
const uploads = h('div', { class: 'uploads', hidden: true });
const fileInput = h('input', { type: 'file', multiple: true, hidden: true, onchange: () => { stage([...fileInput.files]); fileInput.value = ''; } });
const textarea = h('textarea', { class: 'composer-input', rows: 1, maxLength: MAX_TEXT, spellcheck: true, 'aria-label': 'Message' });
const attach = h('button', { type: 'button', class: 'composer-button attach', 'aria-label': 'Upload a file', onclick: () => fileInput.click() }, icon(ICONS.plus, 24));
const emojiButton = h('button', { type: 'button', class: 'composer-button emoji', 'aria-label': 'Select emoji', onclick: (e) => { e.stopPropagation(); toggleEmojiPicker(); } }, icon(ICONS.react, 24));
// Filled with trust.js's bar while the composer sends into a secret chat.
const composerMark = h('div', { class: 'composer-mark', hidden: true });
const translateHint = h('div', { class: 'translate-hint', hidden: true });
const composer = h('div', { class: 'composer' }, attach, textarea, emojiButton);
const typingLine = h('div', { class: 'typing', 'aria-live': 'polite' });
// After a jump to an old message the newest ones aren't loaded: this bar brings them back.
const presentBar = h('div', { class: 'present-bar', hidden: true }, h('span', null, 'You’re viewing older messages'),
  h('button', { type: 'button', class: 'present-button', onclick: () => jumpToPresent() }, 'Jump To Present'));
const form = h('form', { class: 'composer-wrap', onsubmit: (e) => { e.preventDefault(); submit(); } }, presentBar, composerMark, translateHint, replyBar, uploads, composer, fileInput, typingLine);
const memberPanel = h('aside', { class: 'member-panel', 'aria-label': 'Members' });
const searchPanel = h('aside', { class: 'search-panel', 'aria-label': 'Search results', hidden: true });
// A DM's call: its stage while this device is in it, or "Call in progress".
const callSlot = h('div', { class: 'call-slot' });
const chat = h('main', { class: 'chat' }, header, callSlot, h('div', { class: 'chat-body' }, h('div', { class: 'chat-column' }, scroller, form), memberPanel, searchPanel));
// A server voice channel: its header and stage.
const voiceHeader = h('header', { class: 'chat-header' });
const voiceStage = h('div', { class: 'voice-pane-body' });
const voicePane = h('main', { class: 'chat voice-pane' }, voiceHeader, voiceStage);
// Calls ringing this account, over everything.
const ringSlot = h('div', { class: 'ring-slot' });
const forumPane = h('main', { class: 'chat forum' });
const search = createSearchBox((text) => runSearch(text), () => closeSearch());
const friendsPane = h('main', { class: 'chat friends' });
const layer = h('div', { class: 'layer' });
const sideEdge = h('div', { class: 'side-edge', 'aria-hidden': 'true' });
const mainScreen = h('div', { class: 'app' }, rail, sidebar, friendsPane, sideEdge, ringSlot, layer);

// The side panel: tucked behind the rail so the chat has the room, sliding out while the mouse
// is over the rail, the panel or the edge beside the rail (or the keyboard is in it), unless
// pinned in place.
let sideTimer = null;
function openSide(open) {
  clearTimeout(sideTimer);
  if (open) mainScreen.classList.add('side-open');
  else sideTimer = setTimeout(() => mainScreen.classList.remove('side-open'), 250);
}
for (const node of [rail, sidebar, sideEdge]) {
  node.addEventListener('mouseenter', () => openSide(true));
  node.addEventListener('mouseleave', () => openSide(false));
}
sidebar.addEventListener('focusin', () => openSide(true));
sidebar.addEventListener('focusout', (e) => { if (!sidebar.contains(e.relatedTarget)) openSide(false); });

function pinSide(pinned) {
  state.sidePinned = pinned;
  mainScreen.classList.toggle('side-pinned', pinned);
  saveSettings({ sidePinned: pinned });
  refresh();
}

function showMain() {
  if (root.firstChild !== mainScreen) screen(mainScreen);
  refresh();
}

// The middle of the window: the Friends view or a DM.
let pane = friendsPane;
function showPane(node) {
  if (pane !== node) { pane.replaceWith(node); pane = node; }
}

function showFriends() {
  if (state.selection && state.selection.running) state.selection.running.stop = true;
  Object.assign(state, { view: 'friends', guildId: null, current: null, selection: null, replyTo: null, editing: null, discordReply: null, discordEditing: null, staged: [] });
  textarea.value = '';
  showPane(friendsPane);
  api.viewing(null).catch(() => {});
  refresh();
}

// --- Conversations ----------------------------------------------------------------------------

function guildById(id) {
  return state.guilds.find((g) => g.id === id) || null;
}

// A conversation: a DM (or group) or a server channel (guilds.rs channel view, with guild_id).
function dmById(id) {
  const dm = state.dms.find((d) => d.id === id);
  if (dm) return dm;
  for (const g of state.guilds) {
    const c = (g.channels || []).find((x) => x.id === id) || (g.threads || []).find((x) => x.id === id);
    if (c) return c;
  }
  return state.extraThreads.get(id) || null;
}

// A thread's channel (or a forum post's forum).
function parentOf(conv) {
  return conv && conv.thread && conv.parent_id ? dmById(conv.parent_id) : null;
}

function peerOf(dm) {
  return dm && dm.recipients && dm.recipients.length === 1 ? dm.recipients[0] : null;
}

function dmName(dm) {
  if (!dm) return '';
  if (dm.guild_id) return dm.name || 'channel';
  if (dm.name) return dm.name;
  return (dm.recipients || []).map(displayName).join(', ') || 'Unnamed';
}

// Snowflakes are wider than a double; compare them as BigInts.
function newer(a, b) {
  if (!a) return false;
  if (!b) return true;
  return BigInt(a) > BigInt(b);
}

// A DM is unread when its last message is newer than what was last read (by any of this
// account's clients: Discord tells them all).
function unread(dm) {
  const read = state.reads.get(dm.id);
  return newer(dm.last_message_id, read && read.last);
}

function mentions(dm) {
  const read = state.reads.get(dm.id);
  return unread(dm) && read ? read.mentions || 0 : 0;
}

function channelMuted(c) {
  return state.muted.channels.has(c.id) || state.muted.guilds.has(c.guild_id);
}

// A server is unread when one of its text channels is (muted ones don't count); its badge is
// the mentions across them.
function guildUnread(g) {
  if (state.muted.guilds.has(g.id)) return false;
  return (g.channels || []).some((c) => TEXT_TYPES.has(c.type) && !state.muted.channels.has(c.id) && c.id !== state.current && unread(c));
}

function guildMentions(g) {
  return (g.channels || []).reduce((n, c) => n + (TEXT_TYPES.has(c.type) && c.id !== state.current ? mentions(c) : 0), 0);
}

function openGuild(g) {
  const channel = firstChannel(g, state.lastChannel.get(g.id));
  state.guildId = g.id;
  if (channel) select(channel);
  else { showFriends(); state.guildId = g.id; refresh(); }
}

function statusOf(user) {
  return (user && state.presences.get(user.id)) || 'offline';
}

// A group's own icon, or the first letter of its name.
function groupIcon(dm, cls) {
  if (dm.icon && /^\w{1,64}$/.test(dm.icon)) {
    const img = h('img', { class: cls, src: media('https://cdn.discordapp.com/channel-icons/' + dm.id + '/' + dm.icon + '.webp?size=80'), alt: '', draggable: false });
    img.addEventListener('error', () => img.replaceWith(h('div', { class: cls + ' avatar-fallback' }, icon(ICONS.members, 18))), { once: true });
    return img;
  }
  return h('div', { class: cls + ' avatar-fallback' }, icon(ICONS.members, 18));
}

function drawRail() {
  renderRail(rail, {
    guilds: state.guilds,
    selected: state.guildId,
    unread: guildUnread,
    mentions: guildMentions,
    homeMentions: state.dms.reduce((n, dm) => n + (dm.id !== state.current ? mentions(dm) : 0), 0),
    home: () => { const last = state.lastChannel.get('@me'); if (last && dmById(last)) select(last); else showFriends(); },
    open: openGuild,
  });
}

function drawGuildSide(g) {
  guildTitle.textContent = g.name || '';
  renderChannels(channelList, {
    guild: g,
    current: state.current,
    unread,
    mentions,
    muted: channelMuted,
    open: (c) => select(c.id),
    unsupported: (c) => showToast(c.type === 13 ? 'Stage channels aren’t supported.' : 'LockCord can’t open that kind of channel.'),
    voiceRoom: (c) => calls.roomList(c.id),
    // The threads this account is in, and the one open now, under their channel.
    threads: (c) => {
      const list = (g.threads || []).filter((t) => t.parent_id === c.id);
      const open = state.extraThreads.get(state.current);
      if (open && open.parent_id === c.id && !list.some((t) => t.id === open.id) && !FORUM_TYPES.has(c.type)) list.push(open);
      return FORUM_TYPES.has(c.type) ? list.filter((t) => t.id === state.current) : list;
    },
  });
}

function drawSidebar() {
  drawRail();
  const g = state.guildId && guildById(state.guildId);
  const face = g ? guildSide : homeSide;
  if (sidebar.firstChild !== face) sidebar.firstChild.replaceWith(face);
  if (g) drawGuildSide(g);
  drawUserPanel();
  callPanel.textContent = '';
  const bar = calls.panel();
  if (bar) callPanel.appendChild(bar);
  ringSlot.textContent = '';
  const cards = calls.ringCards();
  if (cards) ringSlot.appendChild(cards);
  if (g) return;
  const incoming = state.relationships.filter((r) => r.type === 3).length;
  friendsButton.className = 'dm friends-button' + (state.view === 'friends' ? ' selected' : '');
  friendsButton.textContent = '';
  friendsButton.append(h('span', { class: 'dm-avatar friends-icon' }, icon(ICONS.members, 22)), h('span', { class: 'dm-name' }, 'Friends'),
    incoming ? h('span', { class: 'badge', title: incoming + ' pending' }, String(incoming)) : '');

  dmList.textContent = '';
  for (const dm of state.dms) {
    for (const r of dm.recipients || []) remember(r);
    const peer = peerOf(dm);
    const live = secret.in(dm.id);
    const isUnread = dm.id !== state.current && unread(dm);
    const count = dm.id !== state.current ? mentions(dm) : 0;
    const names = h('span', { class: 'dm-names' }, h('span', { class: 'dm-name' }, dmName(dm)));
    if (!peer) names.appendChild(h('span', { class: 'dm-sub' }, ((dm.recipients || []).length + 1) + ' Members'));
    dmList.appendChild(h('button', {
      type: 'button', role: 'listitem',
      class: 'dm' + (dm.id === state.current ? ' selected' : '') + (isUnread ? ' unread' : ''),
      onclick: () => select(dm.id),
    }, peer ? avatarWithStatus(peer, statusOf(peer), 'dm-avatar') : groupIcon(dm, 'dm-avatar'),
      names,
      live && !live.ended ? secretLock() : null,
      count ? h('span', { class: 'badge', title: count + ' unread' }, count > 99 ? '99+' : String(count)) : null));
  }
}

function drawUserPanel() {
  userPanel.textContent = '';
  if (state.me) {
    remember(state.me);
    // Connected: this account's status; otherwise how the gateway is doing.
    const connected = state.gateway === 'connected';
    const line = !connected ? state.gateway[0].toUpperCase() + state.gateway.slice(1) + '…' : STATUS_WORDS[state.myStatus.status || 'online'][0];
    userPanel.appendChild(h('button', { type: 'button', class: 'panel-me', title: 'Set status', 'aria-label': 'Set status', onclick: (e) => { e.stopPropagation(); showStatusMenu(); } },
      avatarWithStatus(state.me, connected ? myDot(state.myStatus) : 'offline', 'panel-avatar'),
      h('div', { class: 'panel-names' }, h('div', { class: 'panel-name' }, displayName(state.me)),
        h('div', { class: 'panel-status ' + state.gateway }, line))));
    userPanel.appendChild(h('button', { type: 'button', class: 'panel-button', title: 'Settings', 'aria-label': 'Settings', onclick: () => openSettings() }, icon(ICONS.gear, 18)));
    userPanel.appendChild(h('button', {
      type: 'button', class: 'panel-button' + (state.sidePinned ? ' on' : ''), title: state.sidePinned ? 'Unpin the side panel' : 'Pin the side panel',
      'aria-pressed': state.sidePinned ? 'true' : 'false', onclick: () => pinSide(!state.sidePinned),
    }, icon(ICONS.pin, 18)));
    userPanel.appendChild(h('button', { type: 'button', class: 'panel-button', title: 'Sign out', onclick: () => { secret.shutdown(); api.signOut(); } }, icon(ICONS.close, 18)));
  }
}

async function select(id) {
  if (state.current === id) return;
  state.current = id;
  api.viewing(id).catch(() => {});
  state.replyTo = null;
  state.editing = null;
  state.discordReply = null;
  state.discordEditing = null;
  if (state.selection && state.selection.running) state.selection.running.stop = true;
  state.selection = null;
  state.staged = [];
  state.view = null;
  const conv = dmById(id);
  state.guildId = conv && conv.guild_id ? conv.guild_id : null;
  state.lastChannel.set(state.guildId || '@me', id);
  if (conv && conv.guild_id) {
    // The web client's op 37 for this channel (a thread's parent): typing, activity, members.
    const sub = conv.thread && conv.parent_id ? conv.parent_id : id;
    if (!state.memberList || state.memberList.channel !== sub) state.memberList = null;
    api.subscribe(conv.guild_id, sub).catch((e) => console.warn('subscribe', e));
  }
  textarea.value = '';
  // A DM's call, if it has one, is asked for (as Discord's app does when a DM opens).
  if (conv && !conv.guild_id) calls.peek(id);
  if (conv && conv.type === 2) {
    state.view = 'voice';
    showPane(voicePane);
    refresh();
    return;
  }
  if (conv && FORUM_TYPES.has(conv.type)) {
    state.view = 'forum';
    showPane(forumPane);
    loadForum(conv, 0);
    refresh();
    return;
  }
  showPane(chat);
  refresh(true);
  if (!state.messages.has(id)) {
    try {
      const list = await api.messages(id);
      state.messages.set(id, list.slice().reverse());
      if (list.length < 50) state.olderDone.add(id);
      // A secret-chat invite is only ever taken from the other person in a 1:1 DM.
      const peer = peerOf(dmById(id));
      if (peer) for (const m of list.slice(0, 5)) secret.consider(id, m, state.me, peer);
    } catch (e) {
      state.messages.set(id, []);
      console.warn('messages', e);
    }
    if (state.current === id) refresh(true);
  }
  textarea.focus();
}

// --- Drawing ----------------------------------------------------------------------------------

function itemsFor(id) {
  const items = (state.messages.get(id) || []).map((m) => ({ kind: 'discord', at: Date.parse(m.timestamp) || snowflakeTime(m.id), m }));
  // Their LockCord offered encryption: a card asks before anything is answered.
  const e = e2eOf(id);
  if (e && e.status === 'offered') items.push({ kind: 'offer', at: e.since || Date.now() });
  const c = secret.in(id);
  const invite = !c && secret.invitation(id);
  if (invite) {
    const friend = state.relationships.some((r) => r.type === 1 && r.id === invite.peer.id);
    items.push({ kind: 'card', at: invite.at, chat: { phase: 'invited', stranger: !friend, verification: {} } });
  }
  if (c) {
    items.push({ kind: 'card', at: c.startedAt, chat: c });
    const open = c.phase === 'open' && c.verification.me && c.verification.peer;
    // Until both have confirmed the code, nothing of the secret chat shows.
    if (open) for (const r of c.records) if (!r.deleted) items.push({ kind: r.kind === 'system' ? 'system' : 'secret', at: r.at, r, chat: c });
  }
  return items.sort((a, b) => a.at - b.at);
}

// Channels Guard watches (guard.rs), for the header's shield.
const guarded = new Set();
function refreshGuarded() {
  api.guardList().then((list) => { guarded.clear(); for (const r of list) guarded.add(r.channel); refresh(); }).catch(() => {});
}

function shieldButton(dm) {
  const on = guarded.has(dm.id);
  return h('button', {
    type: 'button', class: 'header-button' + (on ? ' guarded' : ''), title: on ? 'Guarded: edit' : 'Guard this channel', 'aria-label': 'Guard',
    onclick: () => openSettings({ page: 'guard', channel: dm.id }),
  }, icon(ICONS.shield, 22));
}

// --- Encrypted DMs (e2e.rs) ---------------------------------------------------------------------

// Rust's view of the DM's encryption: { status: off|waiting|on|changed|lost, safety, verified… }.
function e2eOf(channelId) {
  return state.e2e.contacts.find((c) => c.channel === channelId) || null;
}

// Where Rust seals everything (or blocks): translation and files are off there.
function encryptedHere(channelId) {
  const e = e2eOf(channelId);
  return !!e && ['on', 'changed', 'lost'].includes(e.status);
}

// The header's control for a 1:1 DM: turn it on, see that it's waiting, or the protected pill
// with the safety code; a changed key or missing keys are shown loud. A click opens the popover.
function encryptionControl(dm) {
  const e = e2eOf(dm.id);
  const status = e ? e.status : 'off';
  if (status === 'off') {
    return h('button', { type: 'button', class: 'header-text-button', title: 'End-to-end encrypt this DM (they need LockCord)', onclick: () => {
      api.e2eEnable(dm.id).then(() => showToast('Your key is posted. Encryption starts when ' + displayName(peerOf(dm)) + '’s LockCord answers.')).catch((err) => showToast('Couldn’t turn it on: ' + err));
    } }, icon(ICONS.lock, 16), 'Encrypt');
  }
  const open = (ev) => { ev.stopPropagation(); showEncryption(dm, ev.currentTarget); };
  if (status === 'on') return h('button', { type: 'button', class: 'header-e2e', title: 'Encrypted DM', onclick: open }, encryptedPill(e.safety));
  const label = status === 'waiting' ? 'Encryption: waiting for their LockCord' : status === 'offered' ? 'Encryption offered'
    : status === 'changed' ? '⚠ Their key changed' : '⚠ Encryption keys missing';
  return h('button', { type: 'button', class: 'header-text-button' + (status === 'waiting' || status === 'offered' ? '' : ' warn'), onclick: open }, icon(ICONS.lock, 16), label);
}

// The other side's offer to encrypt the DM, answered (Accept) or left alone (Ignore).
function answerOffer(dm, accept) {
  const e = e2eOf(dm.id);
  if (!e) return;
  const them = displayName(peerOf(dm));
  (accept ? api.e2eAcceptOffer(e.peer) : api.e2eIgnoreOffer(e.peer))
    .then(() => showToast(accept ? 'Your key is posted: this DM is encrypted from now on.' : 'Ignored. Nothing was posted to ' + them + '.'))
    .catch((err) => showToast(String(err)));
}

// The whole safety code, with a way to copy it (to read it out on a call, or paste it elsewhere).
function safetyBlock(label, code) {
  return h('div', { class: 'safety' }, h('span', null, label), safetyGrid(code),
    h('button', { type: 'button', class: 'button secondary small', onclick: () => {
      navigator.clipboard.writeText(code || '').then(() => showToast('Safety code copied.'), () => showToast('Couldn’t copy it.'));
    } }, 'Copy'));
}

function showEncryption(dm, anchor) {
  const e = e2eOf(dm.id);
  if (!e) return;
  const them = displayName(peerOf(dm));
  const done = (p, ok) => p.then(() => { closeLayer(); if (ok) showToast(ok); }).catch((err) => showToast(String(err)));
  const rows = [h('div', { class: 'members-title' }, 'Encryption with ' + them)];
  if (e.status === 'on') {
    rows.push(h('div', { class: 'control-description' }, 'Everything you send here is end-to-end encrypted: Discord stores only ciphertext.'),
      safetyBlock('Safety code', e.safety),
      h('div', { class: 'control-description' }, 'The same 60 digits on ' + them + '’s screen mean nobody is in between. Compare them once, on a call or in person.'),
      toggle('We compared it', null, !!e.verified, (v) => done(api.e2eVerify(e.peer, v))),
      h('div', { class: 'settings-row' },
        h('button', { type: 'button', class: 'button secondary small', onclick: () => done(api.e2eEnable(dm.id), 'New keys offered.') }, 'Renew keys'),
        h('button', { type: 'button', class: 'button danger-outline small', onclick: (ev) => {
          const b = ev.currentTarget;
          if (!b.dataset.armed) { b.dataset.armed = '1'; b.textContent = 'Click again: old messages become unreadable'; return; }
          done(api.e2eForget(e.peer), 'Encryption with ' + them + ' is off.');
        } }, 'Turn off')));
  } else if (e.status === 'changed') {
    rows.push(h('div', { class: 'control-description' }, them + '’s LockCord now uses a different key. That happens when they reinstall or move to a new computer, but it’s also what someone in between would do. Nothing is sent to them, and messages under the new key aren’t shown, until you accept it.'),
      safetyBlock('New safety code', e.newSafety),
      h('div', { class: 'control-description' }, 'Compare the new code with ' + them + ' first, on a call.'),
      h('div', { class: 'settings-row' },
        h('button', { type: 'button', class: 'button brand small', onclick: () => done(api.e2eAcceptChange(e.peer), 'New key accepted.') }, 'Accept the new key'),
        h('button', { type: 'button', class: 'button danger-outline small', onclick: () => done(api.e2eForget(e.peer), 'Encryption with ' + them + ' is off.') }, 'Turn encryption off')));
  } else if (e.status === 'offered') {
    rows.push(h('div', { class: 'control-description' }, them + '’s LockCord offered to encrypt this DM. Accepting posts your key; answering also tells them you use LockCord. Ignoring posts nothing.'),
      h('div', { class: 'settings-row' },
        h('button', { type: 'button', class: 'button brand small', onclick: () => { closeLayer(); answerOffer(dm, true); } }, 'Accept'),
        h('button', { type: 'button', class: 'button secondary small', onclick: () => { closeLayer(); answerOffer(dm, false); } }, 'Ignore')));
  } else if (e.status === 'waiting') {
    rows.push(h('div', { class: 'control-description' }, 'Your key is posted in this DM. Encryption starts by itself when ' + them + '’s LockCord answers. If they don’t use LockCord, nothing happens and messages stay plain.'),
      h('button', { type: 'button', class: 'button danger-outline small', onclick: () => done(api.e2eForget(e.peer)) }, 'Cancel'));
  } else {
    rows.push(h('div', { class: 'control-description' }, 'This DM was encrypted, but the keys aren’t here any more (the contacts file was lost or changed). Sending is blocked so nothing goes out plain. Offer new keys, or turn encryption off.'),
      h('div', { class: 'settings-row' },
        h('button', { type: 'button', class: 'button brand small', onclick: () => done(api.e2eEnable(dm.id), 'New keys offered.') }, 'Offer new keys'),
        h('button', { type: 'button', class: 'button danger-outline small', onclick: () => done(api.e2eForget(e.peer)) }, 'Turn encryption off')));
  }
  popupAt(anchor, h('div', { class: 'popup encryption-popup', onclick: (ev) => ev.stopPropagation() }, rows));
}

// --- Translation (translate.rs) -------------------------------------------------------------------

// A channel's translation settings, or the defaults (settings.rs clean_translation).
function translationFor(channelId) {
  if (encryptedHere(channelId)) return { incoming: false, incomingLang: 'en', outgoing: false, outgoingLang: 'en' };
  const t = (prefs.settings.translate) || {};
  return (t.channels && t.channels[channelId]) || t.defaults || { incoming: false, incomingLang: 'en', outgoing: false, outgoingLang: 'en' };
}

function setTranslation(channelId, change) {
  const t = prefs.settings.translate || { defaults: {}, channels: {} };
  const channels = { ...(t.channels || {}) };
  channels[channelId] = { ...translationFor(channelId), ...change };
  saveSettings({ translate: { ...t, channels } });
  state.translations.clear();
  refresh();
}

const langOptions = LANGS.map(([value, label]) => ({ value, label }));

function globeButton(dm) {
  const t = translationFor(dm.id);
  const on = t.incoming || t.outgoing;
  return h('button', {
    type: 'button', class: 'header-button' + (on ? ' on translate-on' : ''), title: 'Translation', 'aria-label': 'Translation',
    onclick: (e) => { e.stopPropagation(); showTranslation(dm, e.currentTarget); },
  }, icon(ICONS.globe, 22));
}

// The header's translation popover for one channel.
function showTranslation(dm, anchor) {
  const t = translationFor(dm.id);
  popupAt(anchor, h('div', { class: 'popup translate-popup', onclick: (e) => e.stopPropagation() },
    h('div', { class: 'members-title' }, 'Translation here'),
    toggle('Translate messages', 'Others’ messages, shown in your language.', t.incoming, (v) => setTranslation(dm.id, { incoming: v })),
    h('div', { class: 'translate-lang' }, dropdown(langOptions, t.incomingLang, (v) => { setTranslation(dm.id, { incomingLang: v }); showTranslation(dm, anchor); }, 'Into')),
    toggle('Translate what I send', 'Your message is translated, then sent.', t.outgoing, (v) => setTranslation(dm.id, { outgoing: v })),
    h('div', { class: 'translate-lang' }, dropdown(langOptions, t.outgoingLang, (v) => { setTranslation(dm.id, { outgoingLang: v }); showTranslation(dm, anchor); }, 'Into')),
    h('div', { class: 'control-description' }, 'The text of what’s translated goes to Google. Secret chats never are.')));
}

// Others' messages in the open channel that want a translation, asked for 20 at a time.
let translating = false;
function translateVisible(dm) {
  const t = translationFor(dm.id);
  if (!t.incoming || translating) return;
  const list = (state.messages.get(dm.id) || []).slice(-60);
  const want = list.filter((m) => m.content && !isMine(m) && (m.type === 0 || m.type === 19) && !m.lc_e2e && !state.translations.has(m.id)).slice(-20);
  if (!want.length) return;
  translating = true;
  for (const m of want) state.translations.set(m.id, 'pending');
  api.translateTexts(dm.id, t.incomingLang, want.map((m) => m.content))
    .then((out) => want.forEach((m, i) => state.translations.set(m.id, out[i] || 'failed')))
    .catch((e) => { for (const m of want) state.translations.set(m.id, 'failed'); console.warn('translate', e); })
    .finally(() => { translating = false; refresh(); });
}

// What the timeline shows for a message: its translation, unless it's already in that language.
function translationOf(m) {
  const t = translationFor(m.channel_id);
  const got = state.translations.get(m.id);
  if (!t.incoming || !got || typeof got !== 'object' || !got.text || got.text === m.content) return null;
  if (got.detected && got.detected.toLowerCase().startsWith(t.incomingLang.toLowerCase().split('-')[0])) return null;
  return { text: got.text, from: got.detected ? langName(got.detected) : 'another language', original: state.showOriginal.has(m.id) };
}

function pinsButton(dm) {
  return h('button', { type: 'button', class: 'header-button pins-button', title: 'Pinned Messages', 'aria-label': 'Pinned Messages',
    onclick: (e) => { e.stopPropagation(); showPins(dm, e.currentTarget); } }, icon(ICONS.pin, 22));
}

// The search box, kept across redraws (and its focus with it); none in an encrypted DM.
let searchFocused = false;
function addSearchBox(dm) {
  if (!dm.guild_id && encryptedHere(dm.id)) return;
  const focused = searchFocused;
  search.input.placeholder = dm.guild_id ? 'Search ' + ((guildById(dm.guild_id) || {}).name || 'server') : 'Search';
  header.appendChild(search.node);
  if (focused) search.input.focus({ preventScroll: true });
}

function drawHeader(dm) {
  const typing = document.activeElement === search.input;
  searchFocused = typing;
  header.textContent = '';
  if (typing && state.selection) search.input.blur();
  if (state.selection) { drawSelectionBar(); return; }
  if (dm.guild_id) {
    const parent = parentOf(dm);
    if (dm.thread) {
      header.appendChild(h('div', { class: 'header-name' },
        parent ? h('button', { type: 'button', class: 'header-parent', title: 'Back to #' + parent.name, onclick: () => select(parent.id) }, (FORUM_TYPES.has(parent.type) ? '💬 ' : '# ') + parent.name) : null,
        parent ? icon(ICONS.chevronRight, 16) : null,
        icon(ICONS.thread, 20), h('span', null, dm.name || ''),
        dm.archived ? h('span', { class: 'header-topic' }, 'Archived') : null));
      header.appendChild(h('button', { type: 'button', class: 'header-text-button', onclick: () => api.threadJoin(dm.id, !dm.joined).then(() => { dm.joined = !dm.joined; refresh(); }).catch((e) => showToast(String(e))) },
        dm.joined ? 'Leave Thread' : 'Join Thread'));
    } else {
      header.appendChild(h('div', { class: 'header-name' }, h('span', { class: 'header-hash', 'aria-hidden': 'true' }, dm.type === 5 ? '📢' : '#'), h('span', null, dm.name || ''),
        dm.topic ? h('span', { class: 'header-topic', title: dm.topic }, dm.topic) : null));
      header.appendChild(h('button', { type: 'button', class: 'header-button', title: 'Threads', 'aria-label': 'Threads', onclick: (e) => { e.stopPropagation(); showThreads(dm, e.currentTarget); } }, icon(ICONS.thread, 22)));
    }
    header.appendChild(pinsButton(dm));
    header.appendChild(globeButton(dm));
    header.appendChild(shieldButton(dm));
    if (!dm.thread) {
      header.appendChild(h('button', {
        type: 'button', class: 'header-button' + (state.showMemberList ? ' on' : ''), title: state.showMemberList ? 'Hide Member List' : 'Show Member List', 'aria-label': 'Member list',
        onclick: () => { state.showMemberList = !state.showMemberList; saveSettings({ memberList: state.showMemberList }); refresh(); },
      }, icon(ICONS.members, 22)));
    }
    addSearchBox(dm);
    return;
  }
  const peer = peerOf(dm);
  const c = secret.in(dm.id);
  const live = c && !c.ended;
  const mine = secret.live() && c;
  header.appendChild(h('div', { class: 'header-name' }, peer ? icon(ICONS.at, 22) : groupIcon(dm, 'header-icon'),
    peer ? h('button', { type: 'button', class: 'header-person', title: 'Profile', onclick: (e) => { e.stopPropagation(); openProfile(peer, e.currentTarget); } }, dmName(dm)) : h('span', null, dmName(dm)),
    peer ? h('span', { class: 'header-status status-text-' + statusOf(peer) }, STATUS_TEXT[statusOf(peer)]) : null));
  if (!peer) {
    header.appendChild(h('button', {
      type: 'button', class: 'header-button', title: 'Members', 'aria-label': 'Members',
      onclick: (e) => { e.stopPropagation(); showMembers(dm, e.currentTarget); },
    }, icon(ICONS.members, 22)));
  }
  for (const b of calls.headerButtons(dm)) header.appendChild(b);
  header.appendChild(globeButton(dm));
  header.appendChild(shieldButton(dm));
  if (live) header.appendChild(secretPill(secret.canSend(dm.id), c.verification && c.verification.sas));
  if (peer) header.appendChild(encryptionControl(dm));
  if (peer) {
    const busyElsewhere = secret.live() && !mine;
    header.appendChild(h('button', {
      type: 'button',
      class: 'header-button lock' + (live ? ' on' : ''),
      disabled: busyElsewhere,
      title: busyElsewhere ? 'A secret chat is open in another DM' : live ? 'End secret chat' : 'Start secret chat',
      'aria-label': live ? 'End secret chat' : 'Start secret chat',
      onclick: () => { if (live) secret.end(); else secret.start(dm.id, peer); },
    }, icon(ICONS.lock, 22)));
  }
  header.appendChild(pinsButton(dm));
  addSearchBox(dm);
}

function drawComposer(dm) {
  const c = secret.in(dm.id);
  const secretMode = secret.canSend(dm.id);
  // While a secret chat here is starting or waiting for the code, nothing can be typed: text
  // meant for it must not end up on Discord.
  const held = !!c && !c.ended && !secretMode;
  // In an encrypted DM Rust seals what's sent; plainNext is the one message sent without it.
  // A changed key or missing keys block the composer: only an unencrypted message, on purpose.
  const e2e = !secretMode ? e2eOf(dm.id) : null;
  const e2eMode = !!e2e && ['on', 'changed', 'lost'].includes(e2e.status);
  const plain = e2eMode && state.plainNext === dm.id;
  const blocked = e2eMode && !plain && e2e.status !== 'on';
  // Until a DM's history is in, its keys may not be: nothing is typed before it is.
  const loading = !dm.guild_id && !state.messages.has(dm.id);
  if (state.plainNext && state.plainNext !== dm.id) state.plainNext = null;
  // What was typed or staged for one side never goes to the other: when a secret chat or an
  // encrypted DM is left, the draft and the files are dropped.
  const mode = secretMode ? 'secret' : e2eMode ? 'e2e' : 'discord';
  if (state.composeMode !== mode) {
    if (state.composeMode === 'secret' || state.composeMode === 'e2e') { textarea.value = ''; autosize(textarea); }
    state.staged = [];
    state.composeMode = mode;
  }
  // In a server, what Rust worked out this account may do here.
  const mute = !!dm.guild_id && !dm.can_send;
  textarea.disabled = held || mute || state.uploading || blocked || loading;
  emojiButton.disabled = held || mute;
  attach.disabled = held || mute || state.uploading || (!!dm.guild_id && !dm.can_attach) || (e2eMode && !plain);
  attach.title = secretMode ? 'Upload a file (secret)' : dm.guild_id && !dm.can_attach ? 'You can’t upload files here'
    : e2eMode && !plain ? 'Files can’t go encrypted yet' : 'Upload a file';
  composer.classList.toggle('secret', secretMode);
  composer.classList.toggle('e2e', e2eMode && !plain);
  composer.classList.toggle('plain-next', plain);
  composerMark.hidden = !(secretMode || e2eMode);
  composerMark.textContent = '';
  if (secretMode) composerMark.appendChild(secretBar(displayName(peerOf(dm)), c.verification && c.verification.sas));
  else if (e2eMode) composerMark.appendChild(encryptedBar(displayName(peerOf(dm)), e2e.safety, plain, () => { state.plainNext = plain ? null : dm.id; refresh(); },
    e2e.status === 'changed' ? displayName(peerOf(dm)) + '’s key changed. Check it in the header before anything more is sent.'
      : e2e.status === 'lost' ? 'The keys for this encrypted DM are missing. Turn encryption on again in the header.' : null));
  textarea.placeholder = held ? 'Compare the code to start the secret chat'
    : mute ? 'You do not have permission to send messages in this channel.'
    : state.uploading ? 'Uploading…'
    : secretMode ? 'Message @' + displayName(peerOf(dm)) + ' (secret)'
    : loading ? 'Loading…'
    : blocked ? 'Encryption needs a look: see the header'
    : e2eMode ? 'Message @' + displayName(peerOf(dm)) + (plain ? ' (NOT encrypted)' : ' (encrypted)')
    : 'Message ' + (dm.guild_id ? '#' : peerOf(dm) ? '@' : '') + dmName(dm);
  if (!secretMode) state.replyTo = null;
  const out = !secretMode && translationFor(dm.id).outgoing;
  translateHint.hidden = !out;
  if (out) {
    translateHint.textContent = '';
    translateHint.append(icon(ICONS.globe, 14), ' Sent in ' + langName(translationFor(dm.id).outgoingLang) + ': your message is translated first. ',
      h('button', { type: 'button', class: 'translated-toggle', onclick: () => setTranslation(dm.id, { outgoing: false }) }, 'Turn off'));
  }
  if (!canReplyOnDiscord(dm.id)) state.discordReply = null;
  const reply = state.replyTo || state.discordReply;
  replyBar.hidden = !reply;
  if (reply) {
    replyBar.textContent = '';
    const who = state.replyTo ? (state.replyTo.from === 'me' ? state.me : peerOf(dm)) : state.discordReply.m.author;
    const whoName = state.discordReply ? authorName(state.discordReply.m) : displayName(who);
    replyBar.appendChild(h('span', null, 'Replying to ', h('strong', null, whoName)));
    const d = state.discordReply;
    // Like Discord's "@ ON": whether the reply notifies them. Not offered on your own messages.
    if (d && !isMine(d.m)) {
      replyBar.appendChild(h('button', {
        type: 'button', class: 'reply-ping' + (d.ping ? ' on' : ''), title: d.ping ? 'They will be notified' : 'They won’t be notified',
        onclick: () => { d.ping = !d.ping; refresh(); },
      }, '@ ' + (d.ping ? 'ON' : 'OFF')));
    }
    replyBar.appendChild(h('button', { type: 'button', class: 'reply-cancel', 'aria-label': 'Cancel reply', onclick: () => { state.replyTo = null; state.discordReply = null; refresh(); } }, icon(ICONS.close, 16)));
  }
  drawStaged();
}

// --- Discord messages: reply, edit, delete, react -----------------------------------------------

// A custom emoji's image, from Discord's CDN by way of Rust.
function emojiUrl(id, animated) {
  return /^\d{1,20}$/.test(id) ? media('https://cdn.discordapp.com/emojis/' + id + '.' + (animated ? 'gif' : 'webp') + '?size=48') : null;
}

// In a server, a message's author goes by their server nickname, in their top role's colour.
function authorName(m) {
  return (m.member && m.member.nick) || displayName(m.author);
}

function authorColor(m) {
  const conv = dmById(m.channel_id);
  return conv && conv.guild_id && m.member ? colorFor(guildById(conv.guild_id), m.member.roles) : null;
}

function isMine(m) {
  return !!(m && m.author && state.me && m.author.id === state.me.id);
}

// While a secret chat is live in a DM, the composer is the secret chat's, so a reply to a Discord
// message would have nowhere to go.
function canReplyOnDiscord(channelId) {
  const c = secret.in(channelId);
  return !c || !!c.ended;
}

function reactTo(m, key, add) {
  api.react(m.channel_id, m.id, key, add).catch((e) => showToast('Reaction failed: ' + e));
}

function pickReaction(m, anchor) {
  emojiPicker(anchor, (e) => {
    closeLayer();
    const mine = (m.reactions || []).some((r) => r.me && emojiKey(r.emoji) === e);
    reactTo(m, e, !mine);
  });
}

function removeDiscord(m, now) {
  const go = () => api.deleteMessage(m.channel_id, m.id).catch((e) => showToast('Not deleted: ' + e));
  if (now) go();
  else confirmDelete('Are you sure you want to delete this message?', go);
}

function lastOwnDiscord(channelId) {
  const list = state.messages.get(channelId) || [];
  for (let i = list.length - 1; i >= 0; i--) {
    const m = list[i];
    if (isMine(m) && (m.type === 0 || m.type === 19) && m.content) return m;
  }
  return null;
}

function jumpTo(id) {
  const node = timeline.querySelector('li[data-id="' + CSS.escape(id) + '"]');
  if (!node) { goToMessage(state.current, id); return; }
  flash(node);
}

function flash(node) {
  node.scrollIntoView({ block: 'center', behavior: 'smooth' });
  node.classList.remove('flash');
  void node.offsetWidth;
  node.classList.add('flash');
}

// Any message, in any channel this account can see: the channel opens, and when the message
// isn't loaded the 50 around it are, until "Jump To Present".
async function goToMessage(channelId, messageId) {
  closeLayer();
  if (state.current !== channelId) {
    if (!dmById(channelId)) {
      try { const t = await api.threadOpen(channelId); state.extraThreads.set(t.id, t); } catch (e) { showToast('You can’t see that channel.'); return; }
    }
    await select(channelId);
  }
  if (state.current !== channelId) return;
  const list = state.messages.get(channelId) || [];
  if (!list.some((m) => m.id === messageId)) {
    try {
      const around = await api.messages(channelId, null, { around: messageId });
      if (state.current !== channelId) return;
      const conv = dmById(channelId);
      const sorted = around.slice().sort((a, b) => (BigInt(a.id) < BigInt(b.id) ? -1 : 1));
      state.messages.set(channelId, sorted);
      state.olderDone.delete(channelId);
      // Detached unless the newest message came with it.
      const newest = conv && conv.last_message_id;
      if (!newest || !sorted.some((m) => m.id === newest)) state.detached.add(channelId);
      else state.detached.delete(channelId);
    } catch (e) {
      showToast('Couldn’t load that message: ' + e);
      return;
    }
  }
  draw(false);
  const node = timeline.querySelector('li[data-id="' + CSS.escape(messageId) + '"]');
  if (node) flash(node);
  else showToast('That message is gone.');
}

// Back to the newest messages after a jump.
async function jumpToPresent() {
  const id = state.current;
  if (!id) return;
  try {
    const list = await api.messages(id);
    if (state.current !== id) return;
    state.messages.set(id, list.slice().reverse());
    state.detached.delete(id);
    if (list.length < 50) state.olderDone.add(id); else state.olderDone.delete(id);
  } catch (e) {
    showToast('Couldn’t load the newest messages: ' + e);
    return;
  }
  refresh(true);
}

// Newer history after a jump, 50 at a time, when the timeline is scrolled near its bottom.
let loadingNewer = false;
let keepTop = false;
async function loadNewer() {
  const id = state.current;
  const list = state.messages.get(id);
  if (!id || !list || !list.length || loadingNewer || !state.detached.has(id)) return;
  loadingNewer = true;
  try {
    const newer = await api.messages(id, null, { after: list[list.length - 1].id });
    if (state.messages.get(id) !== list) return;
    const known = new Set(list.map((m) => m.id));
    list.push(...newer.filter((m) => !known.has(m.id)).sort((a, b) => (BigInt(a.id) < BigInt(b.id) ? -1 : 1)));
    if (newer.length < 50) state.detached.delete(id);
    keepTop = true;
  } catch (e) {
    console.warn('newer messages', e);
  } finally {
    loadingNewer = false;
  }
  if (state.current === id) refresh();
}
scroller.addEventListener('scroll', () => { if (scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 300) loadNewer(); });

// Older history, 50 at a time, when the timeline is scrolled near its top.
async function loadOlder() {
  const id = state.current;
  const list = state.messages.get(id);
  if (!id || !list || !list.length || state.loadingOlder || state.olderDone.has(id)) return;
  state.loadingOlder = true;
  try {
    const older = await api.messages(id, list[0].id);
    if (older.length < 50) state.olderDone.add(id);
    const known = new Set(list.map((m) => m.id));
    list.unshift(...older.slice().reverse().filter((m) => !known.has(m.id)));
  } catch (e) {
    console.warn('older messages', e);
  } finally {
    state.loadingOlder = false;
  }
  if (state.current === id) refresh();
}
scroller.addEventListener('scroll', () => { if (scroller.scrollTop < 300) loadOlder(); });

function replyToDiscord(m) {
  state.discordReply = { m, ping: true };
  refresh();
  textarea.focus();
}

function editDiscord(m) {
  state.discordEditing = m.id;
  state.editing = null;
  refresh();
}

// --- The message menu (right-click) and selecting messages to delete ----------------------------

// Discord deletes go through Rust's one delete queue (deletion.rs), paced as Lazer did.
const BULK_DELETE = true;

function copy(text) {
  navigator.clipboard.writeText(text).then(() => showToast('Copied.'), () => showToast('Couldn’t copy.'));
}

// A popup's anchor at the point a menu was opened.
const pointOf = (e) => ({ getBoundingClientRect: () => ({ left: e.clientX, right: e.clientX, top: e.clientY, bottom: e.clientY }) });

// items: [label, onclick, class] per entry, '-' for a separator; falsy entries are skipped.
function menuAt(e, items) {
  const node = h('div', { class: 'popup context-menu', role: 'menu', onclick: (ev) => ev.stopPropagation() });
  for (const item of items) {
    if (!item) continue;
    if (item === '-') { node.appendChild(h('div', { class: 'menu-separator' })); continue; }
    const [label, fn, cls] = item;
    node.appendChild(h('button', { type: 'button', role: 'menuitem', class: 'menu-item' + (cls ? ' ' + cls : ''), onclick: (ev) => { ev.stopPropagation(); closeLayer(); fn(ev); } }, label));
  }
  closeLayer();
  layer.appendChild(node);
  node.style.left = Math.max(8, Math.min(e.clientX, window.innerWidth - node.offsetWidth - 8)) + 'px';
  node.style.top = Math.max(8, Math.min(e.clientY, window.innerHeight - node.offsetHeight - 8)) + 'px';
}

// A message's files, saved by Rust to the folder in Settings, App.
function saveMedia(m) {
  const files = (m.attachments || []).map((a) => ({ url: a.url, filename: a.filename }));
  api.saveMedia(files)
    .then((r) => showToast('Saved ' + r.saved + ' file' + (r.saved === 1 ? '' : 's') + ' to ' + r.folder + (r.failed.length ? '. Not saved: ' + r.failed.join(', ') : '.')))
    .catch((e) => showToast('Not saved: ' + e));
}

function discordMenu(m, e) {
  const mine = isMine(m);
  menuAt(e, [
    ['Add Reaction', () => pickReaction(m, pointOf(e))],
    canReplyOnDiscord(m.channel_id) && ['Reply', () => replyToDiscord(m)],
    mine && !isForward(m) && ['Edit Message', () => editDiscord(m)],
    m.content && ['Copy Text', () => copy(m.content)],
    (m.attachments || []).length && ['Save media', () => saveMedia(m)],
    canPin(m.channel_id) && [m.pinned ? 'Unpin Message' : 'Pin Message', () => pinMessage(m, !m.pinned)],
    ['Copy Message Link', () => copy('https://discord.com/channels/' + (m.guild_id || '@me') + '/' + m.channel_id + '/' + m.id)],
    ['Copy Message ID', () => copy(m.id)],
    mine && '-',
    mine && ['Select Message', () => startSelecting('discord:' + m.id)],
    mine && ['Delete Message', (ev) => removeDiscord(m, ev.shiftKey), 'danger'],
  ]);
}

function secretMenu(r, e) {
  const mine = r.from === 'me';
  menuAt(e, [
    ['Add Reaction', () => openReactionPicker(r.id, pointOf(e))],
    ['Reply', () => { state.replyTo = r; refresh(); textarea.focus(); }],
    mine && r.kind === 'text' && secret.canEdit(r.id) && ['Edit Message', () => { state.editing = r.id; state.discordEditing = null; refresh(); }],
    r.text && ['Copy Text', () => copy(r.text)],
    mine && '-',
    mine && ['Select Message', () => startSelecting('secret:' + r.id)],
    mine && ['Delete Message', (ev) => (ev.shiftKey ? secret.remove(r.id) : confirmDelete(SECRET_DELETE_TEXT, () => secret.remove(r.id))), 'danger'],
  ]);
}

function startSelecting(key) {
  state.editing = null;
  state.discordEditing = null;
  state.selection = { keys: new Set([key]), running: null };
  refresh();
}

function stopSelecting() {
  if (!state.selection) return;
  if (state.selection.running) state.selection.running.stop = true;
  state.selection = null;
  refresh();
}

function selectionCtx() {
  const s = state.selection;
  return {
    active: !!s,
    has: (key) => !!s && s.keys.has(key),
    toggle: (key) => {
      if (!s || s.running) return;
      if (s.keys.has(key)) s.keys.delete(key);
      else s.keys.add(key);
      refresh();
    },
  };
}

// The header while selecting: the count with Delete selected and Cancel, or the progress.
function drawSelectionBar() {
  const s = state.selection;
  if (s.running) {
    header.appendChild(h('div', { class: 'header-name' }, h('span', null, 'Deleting… ' + s.running.done + ' of ' + s.running.total)));
    header.appendChild(h('button', { type: 'button', class: 'button link', onclick: stopSelecting }, 'Stop'));
    return;
  }
  const n = s.keys.size;
  header.appendChild(h('div', { class: 'header-name' }, h('span', null, n === 1 ? '1 message selected' : n + ' messages selected')));
  if (BULK_DELETE) header.appendChild(h('button', { type: 'button', class: 'button danger', disabled: !n, onclick: (e) => { e.stopPropagation(); confirmDeleteSelected(); } }, 'Delete selected'));
  header.appendChild(h('button', { type: 'button', class: 'button link', onclick: stopSelecting }, 'Cancel'));
}

function confirmDeleteSelected() {
  const n = state.selection.keys.size;
  const onDiscord = [...state.selection.keys].some((k) => k.startsWith('discord:'));
  confirmDelete((n === 1 ? 'Delete this message?' : 'Delete these ' + n + ' messages?')
    + (onDiscord && n > 1 ? ' They go one at a time, paced the way Lazer does, so Discord sees no burst.' : ''), deleteSelected);
}

// Secret ones go at once (Discord never sees them); Discord's one by one through Rust's queue,
// newest first: a burst of deletes is what self-bot purges look like.
async function deleteSelected() {
  const s = state.selection;
  const channel = state.current;
  if (!s || s.running || !channel) return;
  const at = (key) => (key.startsWith('secret:') ? Infinity : snowflakeTime(key.slice(8)));
  const keys = [...s.keys].sort((a, b) => at(b) - at(a));
  s.running = { done: 0, total: keys.length, stop: false };
  refresh();
  let failed = 0;
  for (const key of keys) {
    if (s.running.stop) break;
    if (key.startsWith('secret:')) {
      secret.remove(key.slice(7));
    } else {
      // One at a time, so Stop works between them; Rust's queue does the pacing.
      try { await api.deleteMessage(channel, key.slice(8)); } catch (e) { failed++; console.warn('delete', e); }
    }
    s.running.done++;
    s.keys.delete(key);
    refresh();
  }
  if (state.selection === s) state.selection = null;
  refresh();
  if (failed) showToast(failed === 1 ? 'One message couldn’t be deleted.' : failed + ' messages couldn’t be deleted.');
}

function drawTyping(dm) {
  typingLine.textContent = '';
  const names = [];
  const c = secret.in(dm.id);
  if (c && c.typing) names.push(displayName(peerOf(dm)));
  const now = Date.now();
  for (const [userId, until] of state.typing.get(dm.id) || []) {
    if (until < now || userId === (state.me || {}).id) continue;
    const u = (dm.recipients || []).find((r) => r.id === userId) || user(userId);
    if (u && !names.includes(displayName(u))) names.push(displayName(u));
  }
  if (!names.length) return;
  typingLine.appendChild(h('span', { class: 'typing-dots', 'aria-hidden': 'true' }, h('i'), h('i'), h('i')));
  typingLine.appendChild(h('span', null, h('strong', null, names.join(', ')), names.length > 1 ? ' are typing…' : ' is typing…'));
}

let refreshQueued = false;
function refresh(toEnd) {
  if (toEnd) { draw(true); return; }
  if (refreshQueued) return;
  refreshQueued = true;
  requestAnimationFrame(() => { refreshQueued = false; draw(false); });
}

function draw(toEnd) {
  if (root.firstChild !== mainScreen) return;
  drawSidebar();
  if (state.view === 'friends') { drawFriends(); return; }
  const dm = dmById(state.current);
  if (!dm) return;
  if (state.view === 'forum') { drawForum(dm); return; }
  if (state.view === 'voice') { drawVoice(dm); return; }
  ackIfSeen(dm);
  // Away from the newest messages (after a jump) nothing pulls the view down.
  const detached = state.detached.has(dm.id);
  const stick = toEnd || (!detached && scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 120);
  const keep = scroller.scrollHeight - scroller.scrollTop;
  const top = scroller.scrollTop;
  presentBar.hidden = !detached;
  drawHeader(dm);
  callSlot.textContent = '';
  const area = !dm.guild_id ? calls.dmArea(dm) : null;
  if (area) callSlot.appendChild(area);
  const peer = peerOf(dm);
  if (!state.editing && !state.discordEditing) liveEditor = null;
  const editorFocused = !!liveEditor && document.activeElement === liveEditor.input;
  renderTimeline(timeline, itemsFor(dm.id), {
    me: state.me, peer, editing: state.editing,
    roleOf: (id) => roleOf(guildById(dm.guild_id), id),
    channelName: (id) => { const c = dmById(id); return c && c.guild_id ? c.name : null; },
    openLink,
    verify: (match) => secret.verify(match),
    joinSecret: () => { if (!secret.join()) showToast('That invite has expired.'); },
    ignoreSecret: () => secret.ignore(),
    acceptOffer: () => answerOffer(dm, true),
    ignoreOffer: () => answerOffer(dm, false),
    toggleReaction: (id, emoji) => secret.toggleReaction(id, emoji),
    startReply: (r) => { state.replyTo = r; refresh(); textarea.focus(); },
    startEdit: (id) => { state.editing = id; state.discordEditing = null; refresh(); },
    canEdit: (id) => secret.canEdit(id),
    cancelFile: (id) => secret.cancelFile(id),
    editor: (r) => editorFor('secret:' + r.id, r.text, (text) => secret.edit(r.id, text)),
    confirmDelete: (r, now) => (now ? secret.remove(r.id) : confirmDelete(SECRET_DELETE_TEXT, () => secret.remove(r.id))),
    openReactionPicker: (id, anchor) => openReactionPicker(id, anchor),
    openViewer: (r) => openViewer(r),
    media,
    emojiUrl,
    secretMenu,
    select: selectionCtx(),
    discord: {
      editing: state.discordEditing,
      isMine,
      canReply: canReplyOnDiscord(dm.id) && (!dm.guild_id || dm.can_send),
      canReact: !dm.guild_id || dm.can_react,
      authorName,
      authorColor,
      translation: translationOf,
      toggleOriginal: (id) => { if (state.showOriginal.has(id)) state.showOriginal.delete(id); else state.showOriginal.add(id); refresh(); },
      globe: () => icon(ICONS.globe, 12),
      e2eHere: !!e2eOf(dm.id) && e2eOf(dm.id).status === 'on',
      peerName: () => displayName(peerOf(dm)),
      react: reactTo,
      pick: pickReaction,
      reply: replyToDiscord,
      edit: editDiscord,
      remove: removeDiscord,
      jump: jumpTo,
      menu: discordMenu,
      profile: (u, anchor) => openProfile(u, anchor, dm.guild_id),
      openThread: (t) => openThread(t),
      openPins: () => showPins(dm, header.querySelector('.pins-button') || header),
      vote: (m, answers) => api.pollVote(m.channel_id, m.id, answers).catch((e) => showToast('Vote not counted: ' + e)),
      redraw: () => refresh(),
      editor: (m) => editorFor('discord:' + m.id, m.content, (text) => api.edit(m.channel_id, m.id, text).catch((e) => showToast('Not saved: ' + e)), () => removeDiscord(m, false)),
    },
  });
  if (editorFocused && liveEditor) liveEditor.input.focus({ preventScroll: true });
  drawComposer(dm);
  drawTyping(dm);
  translateVisible(dm);
  const searching = !!state.search && state.search.scope === searchScope(dm);
  const members = !!dm.guild_id && state.showMemberList && !dm.thread && !searching;
  memberPanel.hidden = !members;
  if (members) renderMembers(memberPanel, guildById(dm.guild_id), state.memberList && state.memberList.channel === dm.id ? state.memberList : null, (u, anchor) => openProfile(u, anchor, dm.guild_id));
  searchPanel.hidden = !searching;
  if (searching) drawSearch(dm);
  if (keepTop) { keepTop = false; scroller.scrollTop = top; }
  else if (stick) scroller.scrollTop = scroller.scrollHeight;
  else scroller.scrollTop = scroller.scrollHeight - keep;
}

function drawFriends() {
  renderFriends(friendsPane, {
    relationships: state.relationships,
    presences: state.presences,
    tab: state.friendsTab,
    setTab: (tab) => { state.friendsTab = tab; refresh(); },
    message: (id) => api.openDm(id).then((channel) => select(channel)).catch((e) => showToast('Couldn’t open the DM: ' + e)),
    accept: (id) => api.relationship(id, 'accept').catch((e) => showToast('Not accepted: ' + e)),
    profile: (r, anchor) => openProfile(r.user, anchor),
    addFriend: (name) => api.addFriend(name),
    remove: (r) => {
      const [title, text, button] = removeWords(r);
      confirmAction(title, text, button, () => api.relationship(r.id, 'remove').catch((e) => showToast('Didn’t work: ' + e)));
    },
  });
}

// A group's members, you included, with their status; a click opens a profile. Friends can be
// added (ten people at most), and the group left.
function showMembers(dm, anchor) {
  const people = [state.me, ...(dm.recipients || [])].filter(Boolean);
  const full = people.length >= 10;
  popupAt(anchor, h('div', { class: 'popup members-popup', onclick: (e) => e.stopPropagation() },
    h('div', { class: 'members-title' }, 'Members — ' + people.length),
    people.map((u) => h('button', { type: 'button', class: 'member clickable', onclick: (e) => { e.stopPropagation(); openProfile(u, e.currentTarget); } },
      avatarWithStatus(u, u.id === (state.me || {}).id ? myDot(state.myStatus) : statusOf(u), 'member-avatar'),
      h('span', { class: 'member-name' }, displayName(u)), u.id === dm.owner_id ? h('span', { class: 'member-owner', title: 'Group owner' }, '👑') : null)),
    h('div', { class: 'menu-separator' }),
    h('button', { type: 'button', class: 'menu-item', disabled: full, title: full ? 'A group holds ten people at most' : null, onclick: () => showAddToGroup(dm) }, 'Add Friends'),
    h('button', { type: 'button', class: 'menu-item danger', onclick: () => confirmAction('Leave ‘' + dmName(dm) + '’', 'Are you sure you want to leave ' + dmName(dm) + '? You won’t be able to rejoin unless you are re-invited.', 'Leave Group',
      () => api.groupLeave(dm.id).then(() => { if (state.current === dm.id) showFriends(); }).catch((e) => showToast('Didn’t leave: ' + e))) }, 'Leave Group')));
}

// Viewing a DM marks it read, as in Discord: once its messages are loaded, the window has focus
// and the newest is in view. One ack per channel at a time.
const acking = new Set();
function ackIfSeen(dm) {
  const list = state.messages.get(dm.id);
  if (!list || !list.length || !document.hasFocus() || acking.has(dm.id) || !unread(dm) || state.detached.has(dm.id)) return;
  if (scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight > 200) return;
  const last = list[list.length - 1].id;
  acking.add(dm.id);
  setTimeout(() => {
    api.ack(dm.id, last).catch((e) => console.warn('ack', e)).finally(() => acking.delete(dm.id));
  }, 600);
}
window.addEventListener('focus', () => refresh());
scroller.addEventListener('scroll', () => { const dm = dmById(state.current); if (dm) ackIfSeen(dm); });

// --- Composing ---------------------------------------------------------------------------------

function autosize(node) {
  node.style.height = 'auto';
  node.style.height = Math.min(node.scrollHeight, Math.round(window.innerHeight * 0.5)) + 'px';
}

function submit() {
  const dm = dmById(state.current);
  if (!dm || state.uploading) return;
  const text = replaceShortcodes(textarea.value.trim());
  if (secret.canSend(dm.id)) {
    if (!text && !state.staged.length) return;
    if (!secret.send({ text, files: state.staged.slice(), replyTo: state.replyTo ? state.replyTo.id : null })) return;
    secret.typing(false);
    clearComposer();
    return;
  }
  // The secret chat ended in the frame before the redraw: its draft is still in the box, and never
  // goes to Discord (the redraw drops it).
  if (state.composeMode === 'secret') return;
  if ((!text && !state.staged.length) || textarea.disabled) return;
  const reply = state.discordReply;
  // Unencrypted in an encrypted DM only on purpose, and only this once.
  const plain = state.composeMode === 'e2e' && state.plainNext === dm.id;
  if (state.composeMode === 'e2e') state.plainNext = null;
  if (state.staged.length) { uploadAndSend(dm.id, text, reply, state.staged.slice(), plain); return; }
  const t = translationFor(dm.id);
  if (t.outgoing && text) { translateAndSend(dm.id, text, reply, t.outgoingLang); return; }
  const sending = textarea.value;
  api.send(dm.id, text, reply && reply.m.id, reply ? reply.ping : true, null, plain).catch((e) => {
    // Not sent (too long to encrypt, keys missing…): the draft comes back rather than being lost.
    showToast('Not sent: ' + e);
    if (state.current === dm.id && !textarea.value) { textarea.value = sending; autosize(textarea); }
  });
  clearComposer();
}

// Outgoing translation: the composer keeps the original until the translation is sent.
async function translateAndSend(channelId, text, reply, to) {
  state.uploading = true;
  refresh();
  try {
    const translated = await api.translateOutgoing(channelId, to, text);
    await api.send(channelId, translated || text, reply && reply.m.id, reply ? reply.ping : true);
    state.uploading = false;
    if (state.current === channelId) clearComposer();
  } catch (e) {
    showToast('Not sent: ' + e);
  } finally {
    state.uploading = false;
    refresh();
  }
}

function clearComposer() {
  textarea.value = '';
  autosize(textarea);
  state.replyTo = null;
  state.discordReply = null;
  state.staged = [];
  refresh(true);
}

// Files for Discord: each uploaded through Rust to Discord's bucket, then one message names them.
// The composer keeps its text and files until that message is sent.
async function uploadAndSend(channelId, text, reply, files, plain) {
  state.uploading = true;
  refresh();
  try {
    const uploaded = [];
    for (const f of files) uploaded.push(await api.upload(channelId, f.name || 'file', new Uint8Array(await f.arrayBuffer()), plain));
    await api.send(channelId, text, reply && reply.m.id, reply ? reply.ping : true, uploaded, plain);
    state.uploading = false;
    if (state.current === channelId) clearComposer();
  } catch (e) {
    showToast('Not sent: ' + e);
  } finally {
    state.uploading = false;
    refresh();
  }
}

textarea.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); submit(); }
  else if (e.key === 'Escape') { state.replyTo = null; state.discordReply = null; closeLayer(); refresh(); }
  else if (e.key === 'ArrowUp' && !textarea.value && state.current) {
    if (secret.canSend(state.current)) {
      const id = secret.lastEditable();
      if (id) { e.preventDefault(); state.editing = id; state.discordEditing = null; refresh(); }
    } else if (!textarea.disabled) {
      const m = lastOwnDiscord(state.current);
      if (m) { e.preventDefault(); state.discordEditing = m.id; state.editing = null; refresh(); }
    }
  }
});
textarea.addEventListener('input', () => {
  autosize(textarea);
  const dm = state.current;
  if (!dm) return;
  const typed = textarea.value.trim().length > 0;
  if (secret.canSend(dm)) secret.typing(typed);
  else if (typed && Date.now() - state.lastTypingSent > 8000) {
    state.lastTypingSent = Date.now();
    api.typing(dm).catch(() => {});
  }
});

// An edit in place, of a secret or a Discord message. Saving it empty asks to delete instead, as
// Discord does, where onEmpty is given. The timeline is redrawn on every event, so the editor is
// kept across redraws by its key and what's typed in it survives.
let liveEditor = null;   // { key, node, input }
function editorFor(key, original, onSave, onEmpty) {
  if (liveEditor && liveEditor.key === key) return liveEditor.node;
  const input = h('textarea', { class: 'edit-input', maxLength: MAX_TEXT, value: original, rows: 1 });
  const cancel = () => { liveEditor = null; state.editing = null; state.discordEditing = null; refresh(); textarea.focus(); };
  const save = () => {
    const text = replaceShortcodes(input.value.trim());
    cancel();
    if (!text) { if (onEmpty) onEmpty(); }
    else if (text !== original) onSave(text);
  };
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); cancel(); }
    else if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); save(); }
  });
  input.addEventListener('input', () => autosize(input));
  setTimeout(() => { autosize(input); input.focus(); input.setSelectionRange(input.value.length, input.value.length); }, 0);
  const node = h('div', { class: 'body editor' }, input, h('div', { class: 'edit-hint' }, 'escape to ',
    h('a', { href: '#', onclick: (e) => { e.preventDefault(); cancel(); } }, 'cancel'), ' • enter to ',
    h('a', { href: '#', onclick: (e) => { e.preventDefault(); e.stopPropagation(); save(); } }, 'save')));
  liveEditor = { key, node, input };
  return node;
}

// Files for the next message: the + button, dropped or pasted. A secret message's go straight to
// the other side; a Discord message's are uploaded when it's sent.
function canStage() {
  // Files can't go encrypted yet: in an encrypted DM only when the next message is unencrypted on purpose.
  const e2eLocked = state.composeMode === 'e2e' && state.plainNext !== state.current;
  return !!state.current && !textarea.disabled && !e2eLocked;
}

function stage(files) {
  if (!canStage()) return;
  for (const f of files) if (state.staged.length < 10) state.staged.push(f);
  refresh();
}

const previews = new Map();
function drawStaged() {
  uploads.textContent = '';
  uploads.hidden = !state.staged.length;
  for (const [file, url] of previews) if (!state.staged.includes(file)) { URL.revokeObjectURL(url); previews.delete(file); }
  state.staged.forEach((file, i) => {
    if (file.type.startsWith('image/') && !previews.has(file)) previews.set(file, URL.createObjectURL(file));
    uploads.appendChild(h('div', { class: 'upload' },
      previews.has(file) ? h('img', { class: 'upload-preview', src: previews.get(file), alt: '' }) : h('div', { class: 'upload-icon' }, icon(ICONS.file, 40)),
      h('div', { class: 'upload-name', title: file.name }, file.name),
      h('button', { type: 'button', class: 'upload-remove', 'aria-label': 'Remove ' + file.name, onclick: () => { state.staged.splice(i, 1); refresh(); } }, icon(ICONS.trash, 16))));
  });
}

window.addEventListener('dragover', (e) => { if (e.dataTransfer && [...e.dataTransfer.types].includes('Files')) e.preventDefault(); });
window.addEventListener('drop', (e) => {
  if (!e.dataTransfer || !e.dataTransfer.files.length) return;
  e.preventDefault();
  stage([...e.dataTransfer.files]);
});
window.addEventListener('paste', (e) => {
  const files = [...((e.clipboardData && e.clipboardData.files) || [])];
  if (!files.length || !canStage()) return;
  e.preventDefault();
  stage(files);
});

// --- People: profiles, this account's status, group DMs (profile.js, people.rs) ------------------

const profiles = new Map(); // 'user|guild' -> the profile, once fetched this session
const relationOf = (id) => { const r = state.relationships.find((x) => x.id === id); return r ? r.type : null; };

// A popout beside what was clicked (to its right, or its left where there's no room).
function popoutBeside(anchor, node) {
  closeLayer();
  layer.appendChild(node);
  const a = anchor.getBoundingClientRect();
  const w = node.offsetWidth;
  const left = a.right + 8 + w <= window.innerWidth - 8 ? a.right + 8 : Math.max(8, a.left - w - 8);
  node.style.left = left + 'px';
  node.style.top = Math.max(8, Math.min(a.top, window.innerHeight - node.offsetHeight - 8)) + 'px';
}

// A user's profile: drawn at once from what's known, filled in when Rust has fetched it.
function openProfile(u, anchor, guildId) {
  if (!u || !u.id) return;
  // Where it was clicked, kept: a redraw meanwhile may take the clicked node away.
  const rect = anchor.getBoundingClientRect();
  anchor = { getBoundingClientRect: () => rect };
  const key = u.id + '|' + (guildId || '');
  const guild = guildId ? guildById(guildId) : null;
  const self = !!state.me && u.id === state.me.id;
  const close = (fn) => () => { closeLayer(); fn(); };
  const ctx = () => ({
    me: state.me, guild, relationship: relationOf(u.id),
    status: self ? myDot(state.myStatus) : statusOf(u),
    md: { nameOf, openLink, formatTime, emojiUrl, roleOf: (id) => roleOf(guild, id), channelName: () => null },
    guildName: (id) => { const g = guildById(id); return g && g.name; },
    message: close(() => api.openDm(u.id).then((channel) => select(channel)).catch((e) => showToast('Couldn’t open the DM: ' + e))),
    add: close(() => api.relationship(u.id, 'add').then(() => showToast('Friend request sent.')).catch((e) => showToast('Not sent: ' + e))),
    accept: close(() => api.relationship(u.id, 'accept').catch((e) => showToast('Not accepted: ' + e))),
    remove: close(() => {
      const r = state.relationships.find((x) => x.id === u.id);
      if (!r) return;
      const [title, text, button] = removeWords(r);
      confirmAction(title, text, button, () => api.relationship(u.id, 'remove').catch((e) => showToast('Didn’t work: ' + e)));
    }),
    block: close(() => confirmAction('Block ‘' + displayName(u) + '’', 'Are you sure you want to block ' + displayName(u) + '? Blocking also removes them from your friends.', 'Block',
      () => api.relationship(u.id, 'block').catch((e) => showToast('Not blocked: ' + e)))),
    unblock: close(() => api.relationship(u.id, 'remove').catch((e) => showToast('Not unblocked: ' + e))),
    editStatus: close(() => showStatusMenu()),
  });
  const show = (p) => popoutBeside(anchor, h('div', { class: 'popup profile-popout', role: 'dialog', 'aria-label': displayName(u), onclick: (e) => e.stopPropagation() }, profileCard(u, p, ctx())));
  const known = profiles.get(key);
  show(known || null);
  if (known) return;
  const popout = layer.firstChild;
  api.userProfile(u.id, guildId).then((p) => {
    profiles.set(key, p);
    remember(p.user);
    if (layer.firstChild === popout) show(p);
  }).catch((e) => {
    const note = layer.firstChild === popout && popout.querySelector('.profile-loading');
    if (note) note.replaceChildren('Couldn’t load the profile: ' + e);
  });
}

// The status menu, over the user panel.
function showStatusMenu() {
  const node = statusMenu(state.myStatus, {
    pick: (status) => { closeLayer(); api.setStatus(status).catch((e) => showToast('Status not changed: ' + e)); },
  });
  closeLayer();
  layer.appendChild(node);
  // The panel may be tucked behind the rail: the menu stays on screen.
  const a = userPanel.getBoundingClientRect();
  node.style.left = Math.max(8, Math.min(a.left + 8, window.innerWidth - node.offsetWidth - 8)) + 'px';
  node.style.top = Math.max(8, a.top - node.offsetHeight - 8) + 'px';
}

function modal(title, body) {
  closeLayer();
  layer.className = 'layer modal-layer';
  layer.appendChild(h('div', { class: 'modal-backdrop', onclick: closeLayer }));
  layer.appendChild(h('div', { class: 'modal', role: 'dialog', 'aria-modal': 'true', onclick: (e) => e.stopPropagation() }, h('h2', { class: 'modal-title' }, title), body));
}

const friendsList = () => state.relationships.filter((r) => r.type === 1).sort((a, b) => displayName(a.user).localeCompare(displayName(b.user)));

// A new DM: one friend opens your DM with them, more make a group (nine at most).
function showNewGroup() {
  modal('Select Friends', friendPicker(friendsList(), 9, 'Create DM', {
    done: (ids) => {
      closeLayer();
      const go = ids.length === 1 ? api.openDm(ids[0]) : api.createGroup(ids);
      go.then((channel) => select(channel)).catch((e) => showToast('Couldn’t make the DM: ' + e));
    },
    cancel: closeLayer,
  }));
}

function showAddToGroup(dm) {
  const inside = new Set((dm.recipients || []).map((u) => u.id));
  const room = 10 - 1 - inside.size;
  modal('Add Friends to ' + dmName(dm), friendPicker(friendsList().filter((r) => !inside.has(r.id)), Math.max(0, room), 'Add', {
    done: async (ids) => {
      closeLayer();
      for (const id of ids) {
        try { await api.groupAdd(dm.id, id); } catch (e) { showToast('Not added: ' + e); break; }
      }
    },
    cancel: closeLayer,
  }));
}

// --- Channels: pins, threads, forums, polls (threads.js; main.rs, guilds.rs) ----------------------

const mdFor = (conv) => {
  const guild = conv && conv.guild_id ? guildById(conv.guild_id) : null;
  return { nameOf, openLink, formatTime, emojiUrl, roleOf: (id) => roleOf(guild, id), channelName: (id) => { const c = dmById(id); return c && c.guild_id ? c.name : null; } };
};

// Anyone may pin in a DM; in a server, where Rust says so.
function canPin(channelId) {
  const conv = dmById(channelId);
  return !!conv && (!conv.guild_id || !!conv.can_pin);
}

function pinMessage(m, on) {
  api.pin(m.channel_id, m.id, on).then(() => { state.pins.delete(m.channel_id); showToast(on ? 'Pinned.' : 'Unpinned.'); }).catch((e) => showToast((on ? 'Not pinned: ' : 'Not unpinned: ') + e));
}

function showPins(conv, anchor) {
  const draw = (list) => popupAt(anchor, pinsPopup(list, {
    md: mdFor(conv),
    canUnpin: canPin(conv.id),
    jump: (m) => goToMessage(conv.id, m.id),
    unpin: (m) => { closeLayer(); pinMessage(m, false); },
  }));
  const cached = state.pins.get(conv.id);
  draw(cached || null);
  if (cached) return;
  const popup = layer.firstChild;
  api.pins(conv.id).then((list) => {
    for (const m of list) remember(m.author);
    state.pins.set(conv.id, list);
    if (layer.firstChild === popup) draw(list);
  }).catch((e) => { if (layer.firstChild === popup) { closeLayer(); showToast('Couldn’t load the pins: ' + e); } });
}

function showThreads(conv, anchor) {
  const open = (t) => { closeLayer(); openThread(t); };
  popupAt(anchor, threadsPopup(conv, null, open));
  const popup = layer.firstChild;
  api.threads(conv.id).then((data) => {
    for (const t of [...data.active, ...data.archived]) state.extraThreads.set(t.id, t);
    if (layer.firstChild === popup) popupAt(anchor, threadsPopup(conv, data, open));
  }).catch((e) => { if (layer.firstChild === popup) { closeLayer(); showToast('Couldn’t load the threads: ' + e); } });
}

// A thread or forum post: known, or asked of Rust by id.
async function openThread(t) {
  if (!t || !t.id) return;
  if (!dmById(t.id) || !dmById(t.id).thread) {
    try {
      const view = await api.threadOpen(t.id);
      state.extraThreads.set(view.id, view);
    } catch (e) {
      showToast('Couldn’t open the thread: ' + e);
      return;
    }
  }
  select(t.id);
}

async function loadForum(conv, offset) {
  const f = state.forum && state.forum.channel === conv.id ? state.forum : { channel: conv.id, posts: [], has_more: false, offset: 0 };
  state.forum = f;
  if (offset === 0) f.posts = [];
  f.loading = true;
  f.error = null;
  refresh();
  try {
    const page = await api.forumPosts(conv.id, offset);
    const known = new Set(f.posts.map((p) => p.id));
    for (const p of page.posts) if (!known.has(p.id)) { f.posts.push(p); state.extraThreads.set(p.id, p); }
    f.has_more = page.has_more;
    f.offset = offset;
  } catch (e) {
    f.error = String(e);
  } finally {
    f.loading = false;
  }
  refresh();
}

// A server voice channel: who is in it, and its call.
function drawVoice(conv) {
  voiceHeader.textContent = '';
  voiceHeader.appendChild(h('div', { class: 'header-name' }, icon(ICONS.speaker, 22), h('span', null, conv.name || ''),
    !conv.can_speak ? h('span', { class: 'header-topic' }, 'You can listen, not speak') : null));
  voiceStage.textContent = '';
  if (!conv.can_connect) {
    voiceStage.appendChild(h('div', { class: 'empty-note' }, 'You can’t connect to this channel.'));
    return;
  }
  voiceStage.appendChild(calls.stage(conv.id, { voice: true }));
}

function drawForum(conv) {
  renderForum(forumPane, conv, state.forum && state.forum.channel === conv.id ? state.forum : null, {
    media,
    open: (p) => openThread(p),
    more: () => loadForum(conv, (state.forum.offset || 0) + 25),
  });
}

// A vote event on a loaded poll: its counts, and whether it's this account's.
function applyVote(m, add, d) {
  const results = m.poll.results || (m.poll.results = { answer_counts: [], is_finalized: false });
  const counts = results.answer_counts || (results.answer_counts = []);
  let c = counts.find((a) => a.id === d.answer_id);
  if (!c) { c = { id: d.answer_id, count: 0, me_voted: false }; counts.push(c); }
  c.count = Math.max(0, (c.count || 0) + (add ? 1 : -1));
  if (d.user_id === (state.me || {}).id) c.me_voted = add;
}

// --- Search (search.js; main.rs `search`) ---------------------------------------------------------

// What a search covers from here: the server, or the DM.
function searchScope(conv) {
  return conv.guild_id ? 'g:' + conv.guild_id : 'c:' + conv.id;
}

async function runSearch(text, more) {
  const conv = dmById(state.current);
  if (!conv) return;
  const scope = searchScope(conv);
  const channels = conv.guild_id ? ((guildById(conv.guild_id) || {}).channels || []) : [];
  if (!more) {
    const query = parseQuery(text, channels);
    state.search = { scope, text, query, results: [], total: 0, has_more: false, offset: 0, loading: !query.error, error: query.error || null };
    if (query.error) { refresh(); return; }
  }
  const s = state.search;
  s.loading = true;
  refresh();
  try {
    const page = await api.search(conv.guild_id || null, conv.guild_id ? null : conv.id, s.query, more ? s.offset + 25 : 0);
    if (state.search !== s) return;
    for (const m of page.messages) remember(m.author);
    s.results.push(...page.messages);
    s.total = page.total;
    s.has_more = page.has_more;
    if (more) s.offset += 25;
  } catch (e) {
    if (state.search === s) s.error = String(e);
  } finally {
    if (state.search === s) s.loading = false;
  }
  refresh();
}

function closeSearch() {
  state.search = null;
  search.input.value = '';
  refresh();
}

function drawSearch(conv) {
  renderResults(searchPanel, state.search, {
    md: mdFor(conv),
    channelName: (id) => { const c = dmById(id); return c && c.guild_id ? (c.thread ? '🧵 ' : '#') + c.name : null; },
    open: (m) => goToMessage(m.channel_id, m.id),
    more: () => runSearch(state.search.text, true),
    close: closeSearch,
  });
}

// --- Popups ------------------------------------------------------------------------------------

// Called once when the layer closes, however it closes (confirmChoice's Cancel).
let layerClosed = null;
function closeLayer() {
  layer.textContent = '';
  layer.className = 'layer';
  const f = layerClosed;
  layerClosed = null;
  if (f) f();
}
document.addEventListener('click', (e) => { if (layer.firstChild && !layer.contains(e.target)) closeLayer(); });
document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  if (layer.firstChild) closeLayer();
  else if (state.selection && !state.selection.running) stopSelecting();
});

// A dialog of its own in the layer (closed by Escape, a click outside, or its buttons).
function showModal(node) {
  closeLayer();
  layer.className = 'layer modal-layer top';
  layer.appendChild(h('div', { class: 'modal-backdrop', onclick: closeLayer }));
  layer.appendChild(node);
}

function popupAt(anchor, node) {
  closeLayer();
  layer.appendChild(node);
  const a = anchor.getBoundingClientRect();
  node.style.left = Math.max(8, Math.min(window.innerWidth - node.offsetWidth - 8, a.right - node.offsetWidth)) + 'px';
  const above = a.top - node.offsetHeight - 8;
  node.style.top = (above < 8 ? a.bottom + 8 : above) + 'px';
}

function toggleEmojiPicker() {
  if (layer.querySelector('.emoji-picker')) { closeLayer(); return; }
  emojiPicker(emojiButton, insert);
}

// The full picker: the composer's (stays open) and Discord reactions' (onPick closes it).
function emojiPicker(anchor, onPick) {
  const grid = h('div', { class: 'emoji-grid' });
  const show = (category) => {
    grid.textContent = '';
    for (const e of category.emoji) grid.appendChild(h('button', { type: 'button', class: 'emoji-cell', onclick: (ev) => { ev.stopPropagation(); onPick(e); } }, e));
  };
  const tabs = h('div', { class: 'emoji-tabs' }, EMOJI_CATEGORIES.map((c) => h('button', { type: 'button', class: 'emoji-tab', title: c.name, onclick: (ev) => { ev.stopPropagation(); show(c); } }, c.icon)));
  show(EMOJI_CATEGORIES[0]);
  popupAt(anchor, h('div', { class: 'popup emoji-picker', onclick: (ev) => ev.stopPropagation() }, tabs, grid));
}

function insert(text) {
  const start = textarea.selectionStart;
  const end = textarea.selectionEnd;
  textarea.value = textarea.value.slice(0, start) + text + textarea.value.slice(end);
  textarea.setSelectionRange(start + text.length, start + text.length);
  textarea.focus();
  autosize(textarea);
}

function openReactionPicker(id, anchor) {
  popupAt(anchor, h('div', { class: 'popup reaction-picker', onclick: (ev) => ev.stopPropagation() },
    REACTIONS.map((e) => h('button', { type: 'button', class: 'emoji-cell', onclick: () => { closeLayer(); secret.toggleReaction(id, e); } }, e))));
}

function confirmAction(title, text, button, onConfirm) {
  closeLayer();
  layer.className = 'layer modal-layer';
  layer.appendChild(h('div', { class: 'modal-backdrop', onclick: closeLayer }));
  layer.appendChild(h('div', { class: 'modal', role: 'dialog', 'aria-modal': 'true', onclick: (e) => e.stopPropagation() },
    h('h2', { class: 'modal-title' }, title),
    h('div', { class: 'modal-text' }, text),
    h('div', { class: 'modal-buttons' },
      h('button', { type: 'button', class: 'button link', onclick: closeLayer }, 'Cancel'),
      h('button', { type: 'button', class: 'button danger', onclick: () => { closeLayer(); onConfirm(); } }, button))));
}

// The host a link's text names, if the text looks like an address ("discord.com", "https://…").
function namedHost(label) {
  const m = /^\s*(?:https?:\/\/)?((?:[a-z0-9-]+\.)+[a-z]{2,})(?:[:/?#]\S*)?\s*$/i.exec(label || '');
  return m ? m[1].toLowerCase().replace(/^www\./, '') : null;
}

// Opens a link in the browser. A masked link whose text names another site than it goes to
// ([discord.com](https://elsewhere)) is shown as it is first.
function openLink(url, label) {
  const named = namedHost(label);
  let target = null;
  try { target = new URL(url).hostname.toLowerCase().replace(/^www\./, ''); } catch { /* opened as is: Rust checks it */ }
  if (!named || !target || named === target) { api.openLink(url); return; }
  // After this click has finished bubbling (it would close the layer).
  setTimeout(() => confirmChoice('This link goes somewhere else', 'The link says ' + named + ', but it opens ' + url + ' (' + target + ').', 'Open ' + target,
    (yes) => { if (yes) api.openLink(url); }), 0);
}

// A yes/no question: resolve(true) on the button, resolve(false) on Cancel or a click outside.
// Over the settings screen too; Escape or a click elsewhere counts as Cancel.
function confirmChoice(title, text, button, resolve) {
  closeLayer();
  let done = false;
  const answer = (v) => { if (done) return; done = true; layerClosed = null; closeLayer(); resolve(v); };
  layer.className = 'layer modal-layer top';
  layer.appendChild(h('div', { class: 'modal-backdrop', onclick: () => answer(false) }));
  layer.appendChild(h('div', { class: 'modal', role: 'dialog', 'aria-modal': 'true', onclick: (e) => e.stopPropagation() },
    h('h2', { class: 'modal-title' }, title),
    h('div', { class: 'modal-text' }, text),
    h('div', { class: 'modal-buttons' },
      h('button', { type: 'button', class: 'button link', onclick: () => answer(false) }, 'Cancel'),
      h('button', { type: 'button', class: 'button brand', onclick: () => answer(true) }, button))));
  layerClosed = () => answer(false);
}

function confirmDelete(text, onDelete) {
  closeLayer();
  layer.className = 'layer modal-layer';
  layer.appendChild(h('div', { class: 'modal-backdrop', onclick: closeLayer }));
  layer.appendChild(h('div', { class: 'modal', role: 'dialog', 'aria-modal': 'true', onclick: (e) => e.stopPropagation() },
    h('h2', { class: 'modal-title' }, 'Delete Message'),
    h('div', { class: 'modal-text' }, text),
    h('div', { class: 'modal-buttons' },
      h('button', { type: 'button', class: 'button link', onclick: closeLayer }, 'Cancel'),
      h('button', { type: 'button', class: 'button danger', onclick: () => { closeLayer(); onDelete(); } }, 'Delete'))));
}

function openViewer(r) {
  closeLayer();
  layer.className = 'layer modal-layer';
  layer.appendChild(h('div', { class: 'modal-backdrop dark', onclick: closeLayer }));
  layer.appendChild(h('div', { class: 'viewer', onclick: closeLayer },
    h('img', { class: 'viewer-image', src: r.url, alt: r.name || '', onclick: (e) => e.stopPropagation() }),
    h('a', { class: 'viewer-save', href: r.url, download: r.name || 'image', onclick: (e) => e.stopPropagation() }, 'Save')));
}

let toastTimer = null;
function showToast(text) {
  const old = document.querySelector('.toast');
  if (old) old.remove();
  const t = h('div', { class: 'toast', role: 'status' }, text);
  document.body.appendChild(t);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.remove(), 5000);
}

// --- Events from LockCord ----------------------------------------------------------------------

on('lc://session', (s) => {
  state.signedIn = !!s.signedIn;
  if (s.signedIn) { state.me = s.me; showConnecting(); }
  else {
    secret.shutdown();
    Object.assign(state, { me: null, dms: [], current: null, view: 'friends', relationships: [], presences: new Map(), reads: new Map(), guilds: [], guildId: null, memberList: null });
    showPane(friendsPane);
    state.messages.clear();
    state.olderDone.clear();
    showSignIn(s.reason);
  }
});
on('lc://login-closed', () => { if (!state.signedIn) showSignIn(); });
// READY's view of the account (main.rs ready_view), also what `status` gives a reloaded page.
function takeReady(r) {
  state.me = r.me;
  state.dms = r.dms || [];
  state.relationships = r.relationships || [];
  state.presences = new Map(Object.entries(r.presences || {}));
  state.reads = new Map(Object.entries(r.reads || {}));
  state.e2e = r.e2e || { contacts: [], problem: null };
  calls.load(r.voice);
  if (state.e2e.problem) showToast(state.e2e.problem);
  else if (state.e2e.notice) showToast(state.e2e.notice);
  state.guilds = r.guilds || [];
  state.myStatus = r.my_status || { status: 'online' };
  const muted = r.muted || {};
  state.muted = { guilds: new Set(muted.guilds || []), channels: new Set(muted.channels || []) };
}

on('lc://ready', (r) => {
  takeReady(r);
  refreshGuarded();
  state.gateway = 'connected';
  // Anything loaded before a reconnect may have missed events: it's loaded again when opened.
  const current = state.current;
  state.messages.clear();
  state.olderDone.clear();
  state.detached.clear();
  state.pins.clear();
  showMain();
  if (current) { state.current = null; select(current); }
});
on('lc://dms', (dms) => {
  state.dms = dms;
  if (state.current && !dmById(state.current)) showFriends();
  refresh();
});
on('lc://relationships', (list) => { state.relationships = list; refresh(); });
on('lc://my-status', (my) => { state.myStatus = my; refresh(); });
on('lc://thread', (t) => {
  // Kept for threads opened from lists; the joined, active ones come with their server too.
  if (state.extraThreads.has(t.id) || t.id === state.current) state.extraThreads.set(t.id, t);
  refresh();
});
on('lc://thread-removed', ({ id }) => {
  const t = dmById(id);
  state.extraThreads.delete(id);
  if (state.current === id) { if (t && t.parent_id && dmById(t.parent_id)) select(t.parent_id); else showFriends(); }
  refresh();
});
on('lc://e2e', (view) => { state.e2e = view; refresh(); });
on('lc://e2e-alert', ({ peer }) => {
  const dm = state.dms.find((d) => peerOf(d) && peerOf(d).id === peer);
  showToast((dm ? displayName(peerOf(dm)) : 'Someone') + '’s encryption key changed. Nothing more is sent to them until you check it (the warning in that DM’s header).');
});
on('lc://guilds', (list) => { state.guilds = list; refresh(); });
on('lc://guild', (g) => {
  const i = state.guilds.findIndex((x) => x.id === g.id);
  if (i >= 0) state.guilds[i] = g;
  else state.guilds.push(g);
  // The open channel may have just become one this account can't see.
  if (state.current && !dmById(state.current)) showFriends();
  refresh();
});
on('lc://guild-removed', ({ id }) => {
  state.guilds = state.guilds.filter((g) => g.id !== id);
  if (state.guildId === id) showFriends();
  refresh();
});
on('lc://member-list', (update) => {
  const conv = dmById(state.current);
  if (!conv || conv.guild_id !== update.guild_id || conv.member_list !== update.id) return;
  const list = applyMemberList(state.memberList && state.memberList.channel === conv.id ? state.memberList : null, update);
  list.channel = conv.id;
  state.memberList = list;
  refresh();
});
on('lc://presence', ({ id, status }) => { state.presences.set(id, status); refresh(); });
on('lc://read', ({ id, read }) => { state.reads.set(id, read); refresh(); });
on('lc://gateway', (g) => { state.gateway = g.status; refresh(); });
on('lc://event', ({ t, d }) => {
  const id = d.channel_id;
  const list = state.messages.get(id);
  // A deleted invite was taken back, whether or not this DM's history is loaded.
  if (t === 'MESSAGE_DELETE') secret.withdrawn(d.id);
  if (t === 'MESSAGE_CREATE') {
    const dm = dmById(id);
    if (dm) {
      dm.last_message_id = d.id;
      if (!dm.guild_id) state.dms = [dm].concat(state.dms.filter((x) => x !== dm));
    }
    // Away from the newest (after a jump), the new one waits for "Jump To Present".
    if (list && !state.detached.has(id) && !list.some((m) => m.id === d.id)) list.push(d);
    const typing = state.typing.get(id);
    if (typing && d.author) typing.delete(d.author.id);
    // A secret-chat invite is only ever taken from the other person in a 1:1 DM.
    const peer = peerOf(dm);
    if (peer) secret.consider(id, d, state.me, peer);
  } else if (t === 'MESSAGE_UPDATE' && list) {
    const m = list.find((x) => x.id === d.id);
    if (m) Object.assign(m, d);
  } else if (t === 'MESSAGE_DELETE' && list) {
    const i = list.findIndex((x) => x.id === d.id);
    if (i >= 0) list.splice(i, 1);
    if (state.discordEditing === d.id) state.discordEditing = null;
    if (state.discordReply && state.discordReply.m.id === d.id) state.discordReply = null;
    if (state.selection && !state.selection.running) state.selection.keys.delete('discord:' + d.id);
  } else if (t === 'CHANNEL_PINS_UPDATE') {
    state.pins.delete(id);
  } else if (t.startsWith('MESSAGE_POLL_VOTE_') && list) {
    const m = list.find((x) => x.id === d.message_id);
    if (m && m.poll) applyVote(m, t === 'MESSAGE_POLL_VOTE_ADD', d);
  } else if (t.startsWith('MESSAGE_REACTION_') && list) {
    const m = list.find((x) => x.id === d.message_id);
    if (m) applyReaction(m, t, d);
  } else if (t === 'TYPING_START') {
    if (d.member && d.member.user) remember(d.member.user);
    if (!state.typing.has(id)) state.typing.set(id, new Map());
    state.typing.get(id).set(d.user_id, Date.now() + TYPING_SHOW_MS);
    setTimeout(refresh, TYPING_SHOW_MS + 100);
  }
  if (id === state.current || t === 'MESSAGE_CREATE') refresh();
});

// A reaction event on a loaded message, applied to its reactions the way Discord counts them.
function applyReaction(m, t, d) {
  if (t === 'MESSAGE_REACTION_REMOVE_ALL') { m.reactions = []; return; }
  const list = m.reactions || (m.reactions = []);
  const key = emojiKey(d.emoji);
  const i = list.findIndex((r) => emojiKey(r.emoji) === key);
  if (t === 'MESSAGE_REACTION_REMOVE_EMOJI') { if (i >= 0) list.splice(i, 1); return; }
  const mine = d.user_id === (state.me || {}).id;
  if (t === 'MESSAGE_REACTION_ADD') {
    if (i < 0) list.push({ emoji: d.emoji, count: 1, me: mine });
    else { list[i].count += 1; if (mine) list[i].me = true; }
  } else if (t === 'MESSAGE_REACTION_REMOVE' && i >= 0) {
    list[i].count -= 1;
    if (mine) list[i].me = false;
    if (list[i].count <= 0) list.splice(i, 1);
  }
}

window.addEventListener('beforeunload', () => secret.shutdown());

// --- Settings and the theme (theme.js; kept by Rust, settings.rs) -----------------------------------

const prefs = { settings: { theme: 'builtin:dark', sidePinned: false, memberList: true }, themes: [], active: null, safeMode: false };
let safeNow = false;

function themeFromPrefs() {
  const id = prefs.settings.theme;
  return id.startsWith('builtin:') ? preset(id) : prefs.active || preset('builtin:dark');
}

async function loadSettings() {
  try {
    Object.assign(prefs, await api.settingsGet());
  } catch (e) {
    console.warn('settings', e);
  }
  safeNow = prefs.safeMode;
  applyTheme(themeFromPrefs(), safeNow);
  state.sidePinned = !!prefs.settings.sidePinned;
  mainScreen.classList.toggle('side-pinned', state.sidePinned);
  state.showMemberList = prefs.settings.memberList !== false;
  if (safeNow) showToast('Safe mode: the default theme is on. Ctrl+Shift+0 brings yours back.');
  refresh();
}

function saveSettings(change) {
  Object.assign(prefs.settings, change);
  api.settingsSave(prefs.settings).then((s) => { prefs.settings = s; }).catch((e) => console.warn('settings', e));
}

// --- The settings window (settings.js) ---------------------------------------------------------------

const settingsScreen = h('div', { class: 'settings-screen', role: 'dialog', 'aria-modal': 'true', 'aria-label': 'Settings' });
let settingsDispose = null;

// Rust's purge, as its last event left it (lc://purge), and who's listening.
let purgeStatus = { state: 'idle' };
const purgeListeners = new Set();
on('lc://purge', (s) => { purgeStatus = s; for (const fn of purgeListeners) fn(s); });
on('lc://guard', ({ text }) => showToast(text));

// The channels a purge or Guard can pick: DMs, then each server's text channels.
function channelGroups() {
  const groups = [{ id: 'dms', label: 'Direct Messages', channels: state.dms.map((d) => ({ id: d.id, name: (peerOf(d) ? '@' : '') + dmName(d) })) }];
  for (const g of state.guilds) {
    if (g.unavailable) continue;
    const channels = (g.channels || []).filter((c) => TEXT_TYPES.has(c.type)).sort((a, b) => a.position - b.position).map((c) => ({ id: c.id, name: '#' + c.name }));
    if (channels.length) groups.push({ id: g.id, label: g.name, channels });
  }
  return groups;
}

function openSettings(start) {
  if (settingsScreen.isConnected) return;
  document.body.appendChild(settingsScreen);
  settingsDispose = renderSettings(settingsScreen, {
    start,
    channelGroups,
    purgeStatus: () => purgeStatus,
    purgeScan: (channels, filters) => api.purgeScan(channels, filters),
    purgeRun: () => api.purgeRun(),
    purgeStop: () => api.purgeStop(),
    onPurge: (fn) => { purgeListeners.add(fn); return () => purgeListeners.delete(fn); },
    guardList: () => api.guardList(),
    guardSet: (rule) => api.guardSet(rule).then((r) => { guarded.add(r.channel); refresh(); return r; }),
    guardRemove: (id) => api.guardRemove(id).then(() => { guarded.delete(id); refresh(); }),
    e2eStatus: () => state.e2e,
    e2eForget: (peerId) => api.e2eForget(peerId),
    e2eVerify: (peerId, v) => api.e2eVerify(peerId, v),
    peerName: (peerId) => { const dm = state.dms.find((d) => peerOf(d) && peerOf(d).id === peerId); return dm ? displayName(peerOf(dm)) : 'Someone (' + peerId + ')'; },
    appSettings: () => prefs.settings,
    saveAppSetting: (change) => saveSettings(change),
    saveFolder: () => api.saveFolder(),
    pickSaveFolder: () => api.pickSaveFolder(),
    voiceDevices: () => api.voiceDevices(),
    voiceTest: (on) => api.voiceTest(on),
    voiceTestLevel: () => api.voiceTestLevel(),
    translationDefaults: () => (prefs.settings.translate || {}).defaults || {},
    setTranslationDefaults: (change) => {
      const t = prefs.settings.translate || { defaults: {}, channels: {} };
      saveSettings({ translate: { ...t, defaults: { ...(t.defaults || {}), ...change } } });
      state.translations.clear();
    },
    langOptions,
    prefs,
    me: state.me,
    get active() { return themeFromPrefs(); },
    choose: async (theme) => {
      const full = theme.builtin ? preset(theme.id) : await api.themeGet(theme.id);
      prefs.active = full.builtin ? null : full;
      saveSettings({ theme: full.id });
      applyTheme(full, safeNow);
      return full;
    },
    save: async (draft) => {
      const saved = await api.themeSave(draft);
      prefs.active = saved;
      prefs.themes = prefs.themes.filter((t) => t.id !== saved.id).concat([{ id: saved.id, name: saved.name, author: saved.author }]);
      saveSettings({ theme: saved.id });
      applyTheme(saved, safeNow);
      return saved;
    },
    remove: async (id) => {
      await api.themeDelete(id);
      prefs.themes = prefs.themes.filter((t) => t.id !== id);
      if (prefs.settings.theme === id) { prefs.active = null; saveSettings({ theme: 'builtin:dark' }); applyTheme(preset('builtin:dark'), safeNow); }
    },
    addAsset: async (id, file) => api.themeAssetAdd(id, file.name, new Uint8Array(await file.arrayBuffer())),
    importTheme: async () => {
      const saved = await api.themeImport();
      if (!saved) return null;
      prefs.themes = prefs.themes.filter((t) => t.id !== saved.id).concat([{ id: saved.id, name: saved.name, author: saved.author }]);
      // Custom CSS can change how anything looks, even draw a fake lock: asked about first.
      if (saved.css && saved.css.trim()) {
        const keep = await new Promise((resolve) => confirmChoice('Keep this theme?',
          '‘' + saved.name + '’ has custom CSS. It can’t run code or load anything, but it can change how anything looks, even draw a lock next to a message that isn’t encrypted. '
          + 'Only keep themes from people you trust. LockCord’s own “Not encrypted” marks and safety codes can’t be changed by a theme.', 'Keep it', resolve));
        if (!keep) {
          await api.themeDelete(saved.id).catch(() => {});
          prefs.themes = prefs.themes.filter((t) => t.id !== saved.id);
          return null;
        }
      }
      return saved;
    },
    exportTheme: (id) => api.themeExport(id),
    toast: showToast,
    close: () => closeSettings(),
  });
}

// Closing puts back the saved theme, so an unsaved draft's preview goes with it.
function closeSettings() {
  if (settingsDispose) { settingsDispose(); settingsDispose = null; }
  settingsScreen.remove();
  settingsScreen.textContent = '';
  applyTheme(themeFromPrefs(), safeNow);
}
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && settingsScreen.isConnected && !layer.firstChild) closeSettings(); });

// The way out of a theme that broke the window: Ctrl+Shift+0 switches to the default theme for
// this session, and back. (`--safe-mode` starts that way.)
document.addEventListener('keydown', (e) => {
  if (e.ctrlKey && e.shiftKey && (e.key === '0' || e.code === 'Digit0')) {
    e.preventDefault();
    safeNow = !safeNow;
    applyTheme(safeNow ? null : themeFromPrefs(), safeNow);
    showToast(safeNow ? 'Default theme, for now. Ctrl+Shift+0 brings yours back.' : 'Your theme is back: ' + currentTheme().name + '.');
  }
});

// --- Start --------------------------------------------------------------------------------------

loadSettings();

api.status().then((s) => {
  if (!s.signedIn) { showSignIn(); return; }
  state.signedIn = true;
  state.me = s.me;
  if (s.gateway === 'connected' || (s.dms && s.dms.length)) { takeReady(s); state.gateway = s.gateway; showMain(); }
  else showConnecting();
}).catch(() => showSignIn());
