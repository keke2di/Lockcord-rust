// Secret chats in a DM: ecdh.chat's encrypted, peer-to-peer session (session.js on ecdh's own
// modules), shown inline in the DM between its ordinary messages. Discord carries only the invite
// link; everything else goes straight to the other person.
//
// One chat at a time, belonging to one DM. Nothing can be read or sent until both sides have
// confirmed the code, and a connection that would go through a relay is refused (direct.js).

import { api } from './api.js';
import { installDirectOnly } from './direct.js';
import { NORMAL_ENDS, Session } from './session.js';
import { snowflakeTime } from './dom.js';

// The shape main.rs and ecdh.chat check: https://ecdh.chat/#<room>.<secret>
const INVITE = /https:\/\/ecdh\.chat\/#([0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.[A-Za-z0-9_-]{22})(?![\w-])/;
const INVITE_MAX_AGE_MS = 15 * 60 * 1000;
const ENDED_SHOW_MS = 10000;

export function inviteIn(text) {
  const m = INVITE.exec(String(text || ''));
  return m ? m[1] : null;
}

export function createSecret(onChange) {
  let chat = null;
  const handled = new Set();
  // An invite from the other person, waiting for a click: joining connects straight to them and
  // so shows them this computer's IP address, so it's never done unasked. One at a time.
  let invited = null;
  let session = null;
  const direct = installDirectOnly(window, () => session && session.refuseRelay());

  const changed = () => onChange();

  function record(id) {
    return chat && chat.records.find((r) => r.id === id);
  }

  function patch(id, values) {
    const r = record(id);
    if (r) { Object.assign(r, values); changed(); }
  }

  const view = {
    phase(next) { chat.phase = next; changed(); },
    invite(link) {
      const c = chat;
      api.send(c.channelId, '🔒 Secret chat: <' + link + '>')
        .then((m) => {
          c.invite = { channelId: c.channelId, id: m.id };
          if (c.ended) takeBack(c);
        })
        .catch((e) => { console.warn('invite not posted', e); session && session.end('server-lost'); });
    },
    verification(v) { chat.verification = v; changed(); },
    connection(state) { chat.reconnecting = state === 'disconnected'; changed(); },
    message(m) {
      chat.records.push(Object.assign({ reactions: [], at: Date.now() }, m));
      if (m.from === 'them') chat.typing = false;
      changed();
    },
    sent(id) { patch(id, { pending: false }); },
    delivered() {},
    edited(id, text) { patch(id, { text, edited: true }); },
    removed(id) {
      const r = record(id);
      if (r) { r.deleted = true; if (r.url) URL.revokeObjectURL(r.url); changed(); }
    },
    reactions(id, reactions) { patch(id, { reactions }); },
    progress(id, ratio) { patch(id, { progress: ratio }); },
    fileDone(id, { url }) { patch(id, { pending: false, url }); },
    fileState(id, state) { patch(id, { state, pending: false }); },
    typing(on) { if (chat) { chat.typing = on; changed(); } },
    system(text) { chat.records.push({ id: 'sys-' + Date.now(), kind: 'system', text, at: Date.now() }); changed(); },
    disappearing(seconds) {
      const label = { 30: '30 seconds', 300: '5 minutes', 3600: '1 hour' }[seconds];
      view.system(seconds ? 'They turned on disappearing messages. New ones vanish after ' + label + '.' : 'They turned off disappearing messages.');
    },
    ended(reason) {
      const c = chat;
      const why = reason === 'connect-failed' && direct.relaySeen() ? 'relay' : reason;
      c.ended = why;
      c.normalEnd = NORMAL_ENDS.has(why);
      // Its messages, files and pictures go from the page straight away.
      for (const r of c.records) if (r.url) URL.revokeObjectURL(r.url);
      c.records = [];
      c.typing = false;
      takeBack(c);
      session = null;
      changed();
      setTimeout(() => { if (chat === c) { chat = null; changed(); } }, ENDED_SHOW_MS);
    },
  };

  function takeBack(c) {
    if (!c.invite) return;
    const { channelId, id } = c.invite;
    c.invite = null;
    api.deleteMessage(channelId, id).catch(() => {});
  }

  function open(channelId, peer, fragment) {
    if (session) return false;
    invited = null;
    chat = {
      channelId, peer, role: fragment ? 'joiner' : 'creator', phase: 'starting', startedAt: Date.now(),
      verification: { sas: '', me: false, peer: false }, records: [], ended: null, invite: null, typing: false,
    };
    session = new Session(view);
    session.start(fragment || '');
    changed();
    return true;
  }

  return {
    // The chat in this DM, if there is one (ended ones show their reason for a moment).
    in(channelId) { return chat && chat.channelId === channelId ? chat : null; },
    live() { return !!session; },
    canSend(channelId) { return !!session && chat.channelId === channelId && session.canSend; },
    start(channelId, peer) { return open(channelId, peer, null); },
    end() {
      if (session) session.endChat();
      else if (chat) { chat = null; changed(); }
    },
    // An invite from the other person, recent and not seen before, waits to be joined or ignored.
    consider(channelId, message, me, peer) {
      if (!message || !message.author || message.author.id === me.id || message.author.bot) return;
      const fragment = inviteIn(message.content);
      if (!fragment || handled.has(fragment) || session) return;
      handled.add(fragment);
      const at = snowflakeTime(message.id);
      if (Date.now() - at > INVITE_MAX_AGE_MS) return;
      invited = { channelId, peer, fragment: '#' + fragment, messageId: message.id, at };
      changed();
    },
    // The invite waiting in this DM, while it's still fresh.
    invitation(channelId) {
      if (!invited || session || invited.channelId !== channelId) return null;
      if (Date.now() - invited.at > INVITE_MAX_AGE_MS) { invited = null; return null; }
      return invited;
    },
    // Joined on the user's click; false when it went stale meanwhile.
    join() {
      const i = invited;
      invited = null;
      if (!i || session || Date.now() - i.at > INVITE_MAX_AGE_MS) { changed(); return false; }
      return open(i.channelId, i.peer, i.fragment);
    },
    ignore() { invited = null; changed(); },
    // The invite's message was deleted: they took it back.
    withdrawn(messageId) {
      if (invited && invited.messageId === messageId) { invited = null; changed(); }
    },
    verify: (match) => session && session.verify(match),
    send: (m) => !!session && session.send(m),
    edit: (id, text) => session && session.edit(id, text),
    remove: (id) => session && session.remove(id),
    toggleReaction: (id, emoji) => session && session.toggleReaction(id, emoji),
    typing: (on) => session && session.typing(on),
    cancelFile: (id) => session && session.cancelFile(id),
    canEdit: (id) => !!session && session.canEdit(id),
    lastEditable: () => session && session.lastEditable(),
    // Quitting or signing out: say goodbye so the other side sees it end.
    shutdown() { if (session) session.endChat(); },
  };
}
