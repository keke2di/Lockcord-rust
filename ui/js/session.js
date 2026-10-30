// One secret chat: signaling, the encrypted peer-to-peer connection and the chat protocol, on
// ecdh.chat's own modules (secure/js, unmodified). It's a port of ecdh's main.js without its UI,
// calls, voice messages, desktop app or relay prompt, so it speaks the same protocol: someone on
// ecdh.chat in a browser can be the other side.
//
// The difference that matters: nothing is sent until both sides have confirmed the code. The
// invite went through Discord, so the code is what proves who is on the other end.
//
// `view` gets everything to show (see the calls below); it holds no state of the chat.

import { INLINE_IMAGE_TYPES, MAX_FILE_SIZE, TYPING_DISPLAY_MS, TYPING_SEND_INTERVAL_MS } from '../secure/js/config.js';
import { Connection } from '../secure/js/connection.js';
import { cleanImage } from '../secure/js/images.js';
import { FEATURES, MessageType, NO_CALLS, encode, encodeChunk, encodeTyping, encodeVerified, randomId } from '../secure/js/messages.js';
import { decodeBase64Url, encodeBase64Url, randomSecret } from '../secure/js/secure-channel.js';
import { serverOrigin } from '../secure/js/platform.js';
import { connectSignaling } from '../secure/js/signaling.js';
import { readChunks } from '../secure/js/transfer.js';

const ROOM_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const INVITE = /^#([0-9a-f-]{36})\.([A-Za-z0-9_-]{22})$/;
const SERVER_ERRORS = { unavailable: 'unavailable', expired: 'expired', 'rate-limited': 'rate-limited', busy: 'busy' };
// Your own messages can be edited for 15 minutes; theirs are accepted with a minute's slack.
const EDIT_WINDOW_MS = 15 * 60 * 1000;
const EDIT_ACCEPT_MS = 16 * 60 * 1000;
// What this side can do: no calls, no screen sharing (FEATURES in messages.js).
const OUR_FEATURES = [...FEATURES.filter((name) => name !== 'screen'), NO_CALLS];

export const NORMAL_ENDS = new Set(['you-ended', 'peer-ended']);

export class Session {
  constructor(view) {
    this.view = view;
    this.phase = 'starting'; // starting → inviting | joining → connecting → open → ended
    this.roomId = null;
    this.secret = null;
    this.iceServers = [];
    this.signaling = null;
    this.connection = null;
    this.settings = { disappearAfter: 0, version: 0 };
    this.peerFeatures = [];
    this.messages = new Map();
    this.typingTimer = null;
    this.typingSent = false;
    this.typingSentAt = 0;
    this.fileQueue = Promise.resolve();
  }

  // fragment: "#room.secret" to join, or empty to start a room.
  start(fragment) {
    if (!window.isSecureContext || !window.crypto?.subtle || !window.RTCPeerConnection) return this.end('unsupported');
    if (!fragment) return this.create();
    const match = INVITE.exec(fragment);
    const decoded = match && ROOM_ID.test(match[1]) ? decodeBase64Url(match[2]) : null;
    if (!decoded || decoded.length !== 16) return this.end('invalid-link');
    this.roomId = match[1];
    this.secret = decoded;
    this.setPhase('joining');
    this.openSignaling().send({ type: 'join', roomId: this.roomId, relay: false });
  }

  create() {
    this.secret = randomSecret();
    this.setPhase('inviting');
    this.openSignaling().send({ type: 'create', relay: false });
  }

  get verified() {
    return !!this.connection && this.connection.verifiedByMe && this.connection.verifiedByPeer;
  }

  // Nothing goes out before both sides have confirmed the code.
  get canSend() {
    return this.phase === 'open' && this.verified;
  }

  setPhase(next) {
    this.phase = next;
    this.view.phase(next);
  }

  openSignaling() {
    this.signaling = connectSignaling({ onMessage: (m) => this.handleSignaling(m), onClose: () => this.signalingClosed() });
    return this.signaling;
  }

  handleSignaling(message) {
    if (this.phase === 'ended') return;
    switch (message.type) {
      case 'created':
        if (this.phase !== 'inviting' || typeof message.roomId !== 'string' || !ROOM_ID.test(message.roomId)) return;
        this.roomId = message.roomId;
        this.iceServers = Array.isArray(message.iceServers) ? message.iceServers : [];
        this.view.invite(`${serverOrigin()}/#${this.roomId}.${encodeBase64Url(this.secret)}`);
        break;
      case 'joined':
        if (this.phase !== 'joining') return;
        this.iceServers = Array.isArray(message.iceServers) ? message.iceServers : [];
        this.connect('joiner');
        break;
      case 'peer-joined':
        if (this.phase !== 'inviting' || !this.roomId) return;
        this.connect('creator');
        break;
      // Once the chat is open its connection is set up, so nothing more is taken from the server.
      case 'signal':
        if (this.phase === 'open') return;
        if (message.signal && typeof message.signal === 'object') this.connection?.handleSignal(message.signal);
        break;
      // Once the chat is open it no longer depends on the server, so these can't end it.
      case 'peer-left':
        if (this.phase !== 'open') this.end('peer-left');
        break;
      case 'error':
        if (this.phase !== 'open') this.end(SERVER_ERRORS[message.code] || 'server-lost');
        break;
    }
  }

  signalingClosed() {
    this.signaling = null;
    if (this.phase !== 'open') this.end('server-lost');
  }

  connect(role) {
    this.setPhase('connecting');
    try {
      this.connection = new Connection({
        role,
        roomId: this.roomId,
        secret: this.secret,
        iceServers: this.iceServers,
        sendSignal: (signal) => this.signaling?.send({ type: 'signal', signal }),
        onOpen: () => this.opened(),
        onMessage: (m) => this.receive(m),
        onClose: (reason) => this.end(reason),
        onStateChange: (state) => { if (this.phase === 'open') this.view.connection(state); },
      });
    } catch {
      this.end('connect-failed');
    }
  }

  opened() {
    this.setPhase('open');
    // Older apps read only the fields they know from SETTINGS, so this is how both sides learn
    // what the other can do.
    this.sendMessage(encode(MessageType.SETTINGS, { ...this.settings, features: OUR_FEATURES }));
    this.updateVerification();
  }

  updateVerification() {
    if (!this.connection) return;
    const { sas, verifiedByMe, verifiedByPeer } = this.connection;
    this.view.verification({ sas, me: verifiedByMe, peer: verifiedByPeer });
  }

  verify(match) {
    if (this.phase !== 'open') return;
    if (!match) { this.connection.sayGoodbye('mismatch'); return; }
    if (this.connection.verifiedByMe) return;
    this.connection.verifiedByMe = true;
    this.sendMessage(encodeVerified());
    this.updateVerification();
  }

  async endChat() {
    if (this.phase === 'open') await this.connection.sayGoodbye('ended');
    this.end('you-ended');
  }

  // A connection that ended up going through a relay isn't peer to peer: it's ended.
  async refuseRelay() {
    if (this.phase === 'ended') return;
    if (this.phase === 'open') await this.connection.sayGoodbye('ended');
    this.end('relay');
  }

  end(reason) {
    if (this.phase === 'ended') return;
    this.phase = 'ended';
    this.connection?.close('you-ended');
    this.connection = null;
    for (const message of this.messages.values()) clearTimeout(message.timer);
    this.messages.clear();
    clearTimeout(this.typingTimer);
    this.signaling?.close();
    this.signaling = null;
    this.secret = null;
    this.view.ended(reason);
  }

  // ---------------------------------------------------------------------------------------------

  receive(message) {
    switch (message.type) {
      case MessageType.TEXT:
        if (this.messages.has(message.id)) return;
        this.peerTyping(false);
        this.remember(message.id, { from: 'them', kind: 'text', text: message.text, replyTo: message.replyTo });
        this.view.message(this.public(message.id));
        this.expireLater(message.id, message.expiresIn);
        this.sendMessage(encode(MessageType.RECEIPT, { id: message.id }));
        break;
      case MessageType.RECEIPT:
        if (this.messages.get(message.id)?.from === 'me') this.view.delivered(message.id);
        break;
      case MessageType.TYPING:
        this.peerTyping(message.typing);
        break;
      case MessageType.VERIFIED:
        this.connection.verifiedByPeer = true;
        this.updateVerification();
        break;
      case MessageType.FILE_OFFER:
        this.receiveOffer(message);
        break;
      case MessageType.FILE_CHUNK:
        this.receiveChunk(message);
        break;
      case MessageType.FILE_CANCEL:
        if (this.stopTransfer(message.id)) this.view.fileState(message.id, 'cancelled');
        break;
      case MessageType.REACTION:
        this.react(message.id, 'them', message.emoji, message.on);
        break;
      case MessageType.DELETE:
        this.deleted(message.id);
        break;
      case MessageType.SETTINGS:
        this.applySettings(message);
        break;
      // Calls aren't supported here (OUR_FEATURES says so); an older app may still try.
      case MessageType.CALL:
        if (message.action === 'invite') this.sendMessage(encode(MessageType.CALL, { action: 'decline', video: message.video }));
        break;
      case MessageType.EDIT:
        this.edited(message);
        break;
    }
  }

  sendMessage(bytes) {
    return this.connection ? this.connection.send(bytes) : Promise.resolve();
  }

  remember(id, message) {
    this.messages.set(id, { id, reactions: new Map(), timer: null, deleted: false, at: Date.now(), ...message });
  }

  // What the view gets of a message: never the transfer or timers.
  public(id) {
    const m = this.messages.get(id);
    return m && { id: m.id, from: m.from, kind: m.kind, text: m.text || '', name: m.name, size: m.size, replyTo: m.replyTo || null, at: m.at };
  }

  get(id) {
    return this.public(id);
  }

  hasFeature(name) {
    return this.peerFeatures.includes(name);
  }

  // Text and files, as typed in the composer. Returns false when nothing could go out.
  send({ text, files = [], replyTo = null }) {
    if (!this.canSend) return false;
    this.stopTyping();
    const reply = replyTo && this.messages.has(replyTo) ? replyTo : null;
    if (files.length === 1 && text && !reply) {
      this.queueFile(files[0], text);
      return true;
    }
    if (text) this.sendText(text, reply);
    for (const file of files) this.queueFile(file, '');
    return true;
  }

  sendText(text, reply) {
    const id = randomId();
    const expiresIn = this.settings.disappearAfter;
    this.remember(id, { from: 'me', kind: 'text', text, replyTo: reply });
    this.view.message({ ...this.public(id), pending: true });
    this.expireLater(id, expiresIn);
    this.sendMessage(encode(MessageType.TEXT, { id, text, replyTo: reply ?? undefined, expiresIn: expiresIn || undefined })).then(() => this.view.sent(id));
  }

  // Editing: only your own text messages, only for 15 minutes, only with an app that understands it.
  canEdit(id) {
    const m = this.messages.get(id);
    return this.canSend && this.hasFeature('edit') && m?.from === 'me' && m.kind === 'text' && !m.deleted && Date.now() - m.at < EDIT_WINDOW_MS;
  }

  lastEditable() {
    const mine = [...this.messages.values()].reverse().find((m) => m.from === 'me' && m.kind === 'text' && !m.deleted);
    return mine && this.canEdit(mine.id) ? mine.id : null;
  }

  edit(id, text) {
    const m = this.messages.get(id);
    if (!text || !this.canEdit(id) || text === m.text) return;
    m.text = text;
    this.view.edited(id, text);
    this.sendMessage(encode(MessageType.EDIT, { id, text }));
  }

  // Their edit is only taken for their own, still-visible text message, sent less than 16 minutes ago.
  edited({ id, text }) {
    const m = this.messages.get(id);
    if (!m || m.from !== 'them' || m.kind !== 'text' || m.deleted || Date.now() - m.at > EDIT_ACCEPT_MS) return;
    m.text = text;
    this.view.edited(id, text);
  }

  typing(hasText) {
    if (!this.canSend) return;
    if (hasText && (!this.typingSent || Date.now() - this.typingSentAt > TYPING_SEND_INTERVAL_MS)) {
      this.typingSent = true;
      this.typingSentAt = Date.now();
      this.sendMessage(encodeTyping(true));
    } else if (!hasText) {
      this.stopTyping();
    }
  }

  stopTyping() {
    if (!this.typingSent) return;
    this.typingSent = false;
    this.sendMessage(encodeTyping(false));
  }

  peerTyping(isTyping) {
    clearTimeout(this.typingTimer);
    this.view.typing(isTyping);
    if (isTyping) this.typingTimer = setTimeout(() => this.view.typing(false), TYPING_DISPLAY_MS);
  }

  toggleReaction(id, emoji) {
    const m = this.messages.get(id);
    if (!m || m.deleted || !this.canSend) return;
    const on = !m.reactions.get(emoji)?.has('me');
    this.react(id, 'me', emoji, on);
    this.sendMessage(encode(MessageType.REACTION, { id, emoji, on }));
  }

  react(id, who, emoji, on) {
    const m = this.messages.get(id);
    if (!m || m.deleted) return;
    const people = m.reactions.get(emoji) ?? new Set();
    if (on) people.add(who);
    else people.delete(who);
    m.reactions.set(emoji, people);
    this.view.reactions(id, [...m.reactions]
      .filter(([, reacted]) => reacted.size > 0)
      .map(([symbol, reacted]) => ({ emoji: symbol, count: reacted.size, mine: reacted.has('me'), them: reacted.has('them') })));
  }

  remove(id) {
    const m = this.messages.get(id);
    if (!m || m.from !== 'me' || m.deleted || !this.canSend) return;
    if (m.transfer) m.transfer.cancelled = true;
    this.forget(id);
    this.sendMessage(encode(MessageType.DELETE, { id }));
  }

  // Only the person who sent a message can delete it.
  deleted(id) {
    const m = this.messages.get(id);
    if (!m || m.from !== 'them' || m.deleted) return;
    this.connection.incoming.cancel(id);
    this.forget(id);
  }

  forget(id) {
    const m = this.messages.get(id);
    if (!m) return;
    clearTimeout(m.timer);
    m.deleted = true;
    m.text = '';
    m.reactions.clear();
    this.view.removed(id);
  }

  expireLater(id, seconds) {
    const m = this.messages.get(id);
    if (!m || !seconds) return;
    m.timer = setTimeout(() => {
      if (m.transfer) m.transfer.cancelled = true;
      if (m.from === 'them') this.connection?.incoming.cancel(id);
      this.forget(id);
    }, seconds * 1000);
  }

  // The newest change wins on both sides; on a tie, whoever made the room wins.
  applySettings({ disappearAfter, version, features }) {
    if (features) {
      this.peerFeatures = features;
      if (this.connection) this.connection.padding = this.hasFeature('pad');
      if (this.hasFeature('seen')) this.sendMessage(encode(MessageType.PREFS, { readReceipts: false }));
    }
    if (version < this.settings.version || (version === this.settings.version && this.connection.role === 'creator')) return;
    const changed = disappearAfter !== this.settings.disappearAfter;
    this.settings = { disappearAfter, version };
    if (changed) this.view.disappearing(disappearAfter);
  }

  // ---------------------------------------------------------------------------------------------
  // Files.

  queueFile(file, caption) {
    this.fileQueue = this.fileQueue.then(() => this.sendFile(file, caption)).catch(() => {});
  }

  async sendFile(file, caption) {
    if (!this.canSend) return;
    let blob = file;
    let name = shortName(file.name || 'file');
    let mime = (file.type || '').split(';')[0];
    let kind = INLINE_IMAGE_TYPES.includes(mime) ? 'image' : 'file';
    // Pictures are redrawn, which drops their EXIF data (location, camera) and shrinks huge ones.
    if (file.type && file.type.startsWith('image/')) {
      try {
        const cleaned = await cleanImage(file);
        blob = cleaned.blob;
        name = shortName(cleaned.name);
        mime = cleaned.type;
        kind = 'image';
      } catch {
        kind = 'file';
      }
    }
    if (kind === 'file') mime = '';
    if (!this.canSend) return;
    if (blob.size > MAX_FILE_SIZE) {
      this.view.system(`“${name}” is larger than 100 MB, so it can’t be sent.`);
      return;
    }

    const id = randomId();
    const expiresIn = this.settings.disappearAfter;
    const transfer = { cancelled: false };
    this.remember(id, { from: 'me', kind, name, size: blob.size, text: caption, transfer });
    this.view.message({ ...this.public(id), pending: true, url: kind === 'image' ? URL.createObjectURL(blob) : null, blob });
    this.expireLater(id, expiresIn);
    await this.sendMessage(encode(MessageType.FILE_OFFER, { id, name, size: blob.size, mime, kind, caption, expiresIn: expiresIn || undefined }));

    let index = 0;
    let sent = 0;
    for await (const data of readChunks(blob)) {
      if (transfer.cancelled || this.phase !== 'open') return;
      await this.sendMessage(encodeChunk(id, index++, data));
      sent += data.length;
      this.view.progress(id, blob.size ? sent / blob.size : 1);
    }
    const m = this.messages.get(id);
    if (!m || transfer.cancelled || this.phase !== 'open') return;
    m.transfer = null;
    this.view.sent(id);
  }

  cancelFile(id) {
    if (!this.stopTransfer(id)) return;
    this.view.fileState(id, 'cancelled');
    this.sendMessage(encode(MessageType.FILE_CANCEL, { id }));
  }

  stopTransfer(id) {
    const transfer = this.messages.get(id)?.transfer;
    if (transfer) {
      if (transfer.cancelled) return false;
      transfer.cancelled = true;
      return true;
    }
    return this.connection?.incoming.cancel(id) ?? false;
  }

  receiveOffer(offer) {
    if (this.messages.has(offer.id)) return;
    const result = this.connection.incoming.offer(offer);
    const { transfer } = result;
    this.remember(transfer.id, { from: 'them', kind: transfer.kind === 'audio' ? 'file' : transfer.kind, name: transfer.name, size: transfer.size, text: transfer.caption });
    this.view.message({ ...this.public(transfer.id), pending: true });
    this.expireLater(transfer.id, offer.expiresIn);
    if (result.done) this.received(transfer, result.blob);
  }

  receiveChunk({ id, index, data }) {
    const result = this.connection.incoming.chunk(id, index, data);
    if (!result) return;
    if (result.done) this.received(result.transfer, result.blob);
    else this.view.progress(id, result.transfer.received / result.transfer.size);
  }

  received(transfer, blob) {
    this.view.fileDone(transfer.id, { url: URL.createObjectURL(blob), blob });
    this.sendMessage(encode(MessageType.RECEIPT, { id: transfer.id }));
  }
}

function shortName(name) {
  if (name.length <= 255) return name;
  const dot = name.lastIndexOf('.');
  const extension = dot > 0 && name.length - dot <= 16 ? name.slice(dot) : '';
  return name.slice(0, 255 - extension.length) + extension;
}
