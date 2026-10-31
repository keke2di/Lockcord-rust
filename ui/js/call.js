// Calls (call.rs): who is in which voice channel or DM call, the call this device is in, and
// drawing them: the header's call buttons, the stage with its tiles, the bar over the user panel,
// the ringing card, the share picker. Media never touches the page: microphone, speakers, camera
// and screen are Rust's; the page only shows pictures it pulls (videotile.js).

import { api, on } from './api.js';
import { h, icon, ICONS } from './dom.js';
import { avatar, displayName, remember, user } from './people.js';
import { dropTiles, videoTile } from './videotile.js';

const PHASE_TEXT = {
  joining: 'Joining…', connecting: 'Connecting…', securing: 'Securing the call…', waiting: 'Waiting for others',
  connected: 'Voice connected', failed: 'Call failed',
};
const QUALITIES = [
  { value: '720-30', label: '720p · 30 fps', height: 720, fps: 30 },
  { value: '1080-30', label: '1080p · 30 fps', height: 1080, fps: 30 },
  { value: '1080-60', label: '1080p · 60 fps', height: 1080, fps: 60 },
  { value: '480-15', label: '480p · 15 fps (light)', height: 480, fps: 15 },
];

// ctx: { me(), dmById(id), refresh(), toast(text), select(id), confirm(title, text, button, yes),
//        popup(anchor, node), modal(node), closeLayer(), voiceSettings(), channelName(id) }
export function createCalls(ctx) {
  const state = {
    rooms: new Map(),   // channel -> { states, users } (call.rs room_view)
    dmCalls: new Map(), // channel -> { ringing: [user ids] }
    active: null,       // call.rs active_view
    speaking: new Set(),
    declined: new Set(), // DM calls this device declined (until their ringing changes)
  };

  const me = () => (ctx.me() || {}).id;
  const roomOf = (channel) => state.rooms.get(channel) || { states: [], users: [] };
  const inCall = (channel) => !!state.active && state.active.channel === channel;
  const connected = () => !!state.active && ['connected', 'waiting'].includes(state.active.phase);
  const ringingMe = () => [...state.dmCalls.entries()]
    .filter(([channel, c]) => (c.ringing || []).includes(me()) && !inCall(channel) && !state.declined.has(channel))
    .map(([channel]) => channel);

  // --- From Rust ---------------------------------------------------------------------------------

  function load(voice) {
    state.rooms = new Map(Object.entries((voice && voice.rooms) || {}));
    state.dmCalls = new Map(Object.entries((voice && voice.dmCalls) || {}));
    state.active = (voice && voice.active) || null;
    for (const r of state.rooms.values()) for (const u of r.users || []) remember(u);
  }

  on('lc://voice-room', (room) => {
    for (const u of room.users || []) remember(u);
    if ((room.states || []).length) state.rooms.set(room.channel, room);
    else state.rooms.delete(room.channel);
    ctx.refresh();
  });
  on('lc://dm-call', ({ channel, call }) => {
    if (call) state.dmCalls.set(channel, call);
    else state.dmCalls.delete(channel);
    if (!call || !(call.ringing || []).includes(me())) state.declined.delete(channel);
    ring();
    ctx.refresh();
  });
  on('lc://call', (active) => {
    const was = state.active;
    state.active = active;
    if (!active) { state.speaking.clear(); dropTiles(); }
    if (active && (!was || was.channel !== active.channel)) dropTiles();
    if (active && active.error && (!was || was.error !== active.error)) ctx.toast(active.error);
    ring();
    ctx.refresh();
  });
  on('lc://call-speaking', (ids) => {
    state.speaking = new Set(ids);
    for (const el of document.querySelectorAll('[data-speaker]')) el.classList.toggle('speaking', state.speaking.has(el.dataset.speaker));
  });
  on('lc://ring', () => { ring(); ctx.refresh(); });
  on('lc://call-ended', ({ reason }) => { if (reason) ctx.toast(reason); });
  on('lc://call-notice', ({ text }) => { if (text) ctx.toast(text); });

  // --- Ringing -----------------------------------------------------------------------------------

  // A soft two-tone ring, every two seconds while someone is calling (WebAudio, nothing loaded).
  let ringer = null;
  function ring() {
    const want = ringingMe().length > 0;
    if (want && !ringer) {
      let audio = null;
      const beep = () => {
        try {
          audio = audio || new AudioContext();
          const t = audio.currentTime;
          for (const [i, f] of [[0, 660], [1, 880]].entries()) {
            const o = audio.createOscillator();
            const g = audio.createGain();
            o.frequency.value = f[1];
            g.gain.setValueAtTime(0.0001, t + i * 0.25);
            g.gain.exponentialRampToValueAtTime(0.12, t + i * 0.25 + 0.02);
            g.gain.exponentialRampToValueAtTime(0.0001, t + i * 0.25 + 0.22);
            o.connect(g).connect(audio.destination);
            o.start(t + i * 0.25);
            o.stop(t + i * 0.25 + 0.25);
          }
        } catch { /* no sound */ }
      };
      beep();
      ringer = { timer: setInterval(beep, 2000), close: () => { if (audio) audio.close().catch(() => {}); } };
    } else if (!want && ringer) {
      clearInterval(ringer.timer);
      ringer.close();
      ringer = null;
    }
  }

  // --- Actions -----------------------------------------------------------------------------------

  const fail = (e) => ctx.toast(String(e));

  // Joins (asking first when it means leaving another call).
  function join(channel, then) {
    const go = () => api.callJoin(channel).then(() => { state.declined.delete(channel); if (then) then(); }).catch(fail);
    if (state.active && state.active.channel !== channel) {
      ctx.confirm('Switch calls?', 'You’re in a call in ' + ctx.channelName(state.active.channel) + '. Joining this one leaves it.', 'Switch', go);
    } else go();
  }
  const leave = () => api.callLeave().catch(fail);
  const decline = (channel) => { state.declined.add(channel); ring(); ctx.refresh(); api.callDecline(channel).catch(() => {}); };
  const setControls = (muted, deafened) => api.callControls(muted, deafened).catch(fail);
  const toggleMute = () => { const a = state.active; if (a) setControls(!a.muted, a.deafened && a.muted ? false : a.deafened); };
  const toggleDeafen = () => { const a = state.active; if (a) setControls(a.muted, !a.deafened); };
  const toggleCamera = () => { const a = state.active; if (a) api.callCamera(!a.camera).catch(fail); };
  const toggleShare = () => {
    const a = state.active;
    if (!a) return;
    if (a.sharing) api.callShareStop().catch(fail);
    else sharePicker();
  };

  // --- Pieces ------------------------------------------------------------------------------------

  const roundButton = (title, path, onclick, cls = '', disabled = false) => h('button', {
    type: 'button', class: 'call-button ' + cls, title, 'aria-label': title, disabled, onclick: (e) => { e.stopPropagation(); onclick(e); },
  }, icon(path, 22));

  // The controls under the stage and in the bar over the user panel.
  function controls(compact) {
    const a = state.active;
    if (!a) return null;
    const cam = a.camera;
    const share = !!a.sharing;
    return h('div', { class: 'call-controls' + (compact ? ' compact' : '') },
      roundButton(a.muted ? 'Unmute' : 'Mute', a.muted ? ICONS.micOff : ICONS.mic, toggleMute, a.muted ? 'off' : ''),
      roundButton(a.deafened ? 'Undeafen' : 'Deafen', a.deafened ? ICONS.headphonesOff : ICONS.headphones, toggleDeafen, a.deafened ? 'off' : ''),
      compact ? null : roundButton(cam ? 'Turn camera off' : 'Turn camera on', cam ? ICONS.video : ICONS.videoOff, toggleCamera, cam ? 'on' : '', !connected() || (!a.cameraAvailable && !cam)),
      compact ? null : roundButton(share ? 'Stop sharing' : 'Share your screen', ICONS.screen, toggleShare, share ? 'on' : '', !connected()),
      roundButton('Leave call', ICONS.hangup, leave, 'danger'));
  }

  // The privacy code: Discord's end-to-end encryption, the same on every screen in the call.
  function privacy(a) {
    if (!a.privacy) return null;
    return h('button', { type: 'button', class: 'call-privacy', title: 'End-to-end encrypted. Everyone in the call sees the same code.', onclick: (e) => {
      e.stopPropagation();
      ctx.popup(e.currentTarget, h('div', { class: 'popup call-privacy-popup', onclick: (ev) => ev.stopPropagation() },
        h('div', { class: 'members-title' }, 'End-to-end encrypted'),
        h('div', { class: 'control-description' }, 'Discord’s DAVE encryption: only the people in this call can hear or see it. Compare this code with them; it changes when someone joins or leaves.'),
        h('div', { class: 'call-code' }, a.privacy)));
    } }, icon(ICONS.lock, 14), 'Encrypted');
  }

  // Someone in the call: their video if it's on, else their avatar; mute and deafen marks.
  function tile(s, big) {
    const id = s.user_id;
    const self = id === me();
    const u = user(id) || { id, username: 'Someone' };
    const a = state.active;
    const video = s.self_video && inCall(s.channel_id) && connected();
    const key = self ? 'me' : id;
    const marks = h('div', { class: 'tile-marks' },
      s.self_mute || s.mute ? icon(ICONS.micOff, 16) : null,
      s.self_deaf || s.deaf ? icon(ICONS.headphonesOff, 16) : null);
    const live = s.self_stream && !self && a && inCall(s.channel_id);
    const watching = a && a.watching && a.watching.user === id;
    const el = h('div', {
      class: 'call-tile' + (big ? ' big' : '') + (state.speaking.has(id) ? ' speaking' : ''), 'data-speaker': id,
      oncontextmenu: (e) => { if (!self && inCall(s.channel_id)) { e.preventDefault(); volumeMenu(e.currentTarget, id); } },
    },
      video ? videoTile(key) : h('div', { class: 'tile-face' }, avatar(u, 'tile-avatar')),
      h('div', { class: 'tile-name' }, displayName(u), marks),
      s.self_stream ? h('span', { class: 'tile-live' }, 'LIVE') : null,
      live ? h('button', { type: 'button', class: 'button small ' + (watching ? 'secondary' : 'brand') + ' tile-watch', onclick: (e) => {
        e.stopPropagation();
        (watching ? api.callWatchStop() : api.callWatch(id)).catch(fail);
      } }, watching ? 'Stop watching' : 'Watch') : null);
    return el;
  }

  // The stream being watched, large, above the tiles.
  function streamView(a) {
    if (!a.watching) return null;
    const u = user(a.watching.user) || { id: a.watching.user, username: 'Someone' };
    return h('div', { class: 'call-stream' },
      videoTile('stream:' + a.watching.user, 'stream'),
      h('div', { class: 'tile-name' }, displayName(u) + '’s screen' + (a.watching.phase !== 'connected' ? ' · ' + (PHASE_TEXT[a.watching.phase] || 'Connecting…') : '')),
      h('button', { type: 'button', class: 'button small secondary stream-stop', onclick: () => api.callWatchStop().catch(fail) }, 'Stop watching'));
  }

  // The stage for a channel: who is in its call, and the controls when this device is in it.
  function stage(channel, opts = {}) {
    const room = roomOf(channel);
    const a = inCall(channel) ? state.active : null;
    const states = room.states.slice();
    // This device, while joining, before Discord lists it.
    if (a && !states.some((s) => s.user_id === me())) states.push({ user_id: me(), channel_id: channel, self_mute: a.muted, self_deaf: a.deafened, self_video: a.camera });
    if (a) for (const s of states) if (s.user_id === me()) { s.self_mute = a.muted; s.self_deaf = a.deafened; s.self_video = a.camera; }
    const status = a ? h('div', { class: 'call-status' }, h('span', { class: 'call-phase ' + a.phase }, PHASE_TEXT[a.phase] || a.phase),
      a.sharing ? h('span', { class: 'call-sharing' }, icon(ICONS.screen, 14), a.sharing.phase === 'connected' ? 'You’re sharing your screen' : 'Starting your stream…') : null,
      privacy(a)) : null;
    const join_ = !a ? h('button', { type: 'button', class: 'button brand', onclick: () => join(channel) }, icon(ICONS.phone, 18), states.length ? 'Join Call' : opts.voice ? 'Join Voice' : 'Start Call') : null;
    return h('section', { class: 'call-stage' + (opts.voice ? ' voice' : '') + (a && a.watching ? ' watching' : '') },
      status,
      a ? streamView(a) : null,
      h('div', { class: 'call-tiles' + (states.length <= 2 ? ' few' : '') }, states.map((s) => tile(s, states.length <= 2))),
      a ? controls(false) : h('div', { class: 'call-join' }, join_));
  }

  // Over the chat of a DM: the stage while this device is in its call, or "Call in progress".
  function dmArea(dm) {
    if (inCall(dm.id)) return stage(dm.id);
    const room = roomOf(dm.id);
    if (!room.states.length) return null;
    const names = room.states.map((s) => displayName(user(s.user_id) || { username: 'Someone' })).join(', ');
    return h('div', { class: 'call-banner' }, icon(ICONS.phone, 18), h('span', null, 'Call in progress · ' + names),
      h('button', { type: 'button', class: 'button small brand', onclick: () => join(dm.id) }, 'Join Call'));
  }

  // The DM header's call buttons: a voice call, or one with your camera on.
  function headerButtons(dm) {
    const here = inCall(dm.id);
    return [
      h('button', { type: 'button', class: 'header-button' + (here ? ' on' : ''), title: here ? 'In this call' : 'Start Voice Call', 'aria-label': 'Start Voice Call', disabled: here, onclick: () => join(dm.id) }, icon(ICONS.phone, 22)),
      h('button', { type: 'button', class: 'header-button', title: 'Start Video Call', 'aria-label': 'Start Video Call', onclick: () => {
        if (here) { if (connected() && !state.active.camera) api.callCamera(true).catch(fail); return; }
        join(dm.id, () => { pendingCamera = dm.id; });
      } }, icon(ICONS.video, 22)),
    ];
  }
  // A video call turns the camera on once the call is connected.
  let pendingCamera = null;
  on('lc://call', (a) => {
    if (pendingCamera && a && a.channel === pendingCamera && ['connected', 'waiting'].includes(a.phase) && a.cameraAvailable) {
      pendingCamera = null;
      if (!a.camera) api.callCamera(true).catch(fail);
    } else if (!a) pendingCamera = null;
  });

  // Over the user panel while in a call, wherever the window is.
  function panel() {
    const a = state.active;
    if (!a) return null;
    return h('div', { class: 'call-panel' },
      h('div', { class: 'call-panel-text' },
        h('div', { class: 'call-phase ' + a.phase }, PHASE_TEXT[a.phase] || a.phase),
        h('button', { type: 'button', class: 'call-panel-where', onclick: () => ctx.select(a.channel) }, ctx.channelName(a.channel))),
      controls(true));
  }

  // Under a server voice channel in the sidebar: who is in it.
  function roomList(channel) {
    const room = roomOf(channel);
    if (!room.states.length) return null;
    return h('div', { class: 'voice-room' }, room.states.map((s) => {
      const u = user(s.user_id) || { id: s.user_id, username: 'Someone' };
      return h('div', { class: 'voice-member' + (state.speaking.has(s.user_id) ? ' speaking' : ''), 'data-speaker': s.user_id },
        avatar(u, 'voice-avatar'), h('span', { class: 'voice-name' }, displayName(u)),
        s.self_stream ? h('span', { class: 'tile-live small' }, 'LIVE') : null,
        s.self_video ? icon(ICONS.video, 14) : null,
        s.self_mute || s.mute ? icon(ICONS.micOff, 14) : null,
        s.self_deaf || s.deaf ? icon(ICONS.headphonesOff, 14) : null);
    }));
  }

  // The cards for calls ringing this account: Answer or Decline.
  function ringCards() {
    const list = ringingMe();
    if (!list.length) return null;
    return h('div', { class: 'ring-cards' }, list.map((channel) => {
      const dm = ctx.dmById(channel);
      const peer = dm && dm.type === 1 ? (dm.recipients || [])[0] : null;
      return h('div', { class: 'ring-card' },
        peer ? avatar(peer, 'ring-avatar') : h('div', { class: 'ring-avatar group' }, icon(ICONS.members, 28)),
        h('div', { class: 'ring-text' }, h('div', { class: 'ring-name' }, ctx.channelName(channel)), h('div', { class: 'ring-sub' }, 'Incoming call…')),
        h('button', { type: 'button', class: 'call-button accept', title: 'Answer', 'aria-label': 'Answer', onclick: () => join(channel, () => ctx.select(channel)) }, icon(ICONS.phone, 22)),
        h('button', { type: 'button', class: 'call-button danger', title: 'Decline', 'aria-label': 'Decline', onclick: () => decline(channel) }, icon(ICONS.hangup, 22)));
    }));
  }

  // Someone's volume in this call (right-click their tile).
  function volumeMenu(anchor, id) {
    const a = state.active;
    const now = (a && a.volumes && a.volumes[id]) || 100;
    const value = h('span', { class: 'control-value' }, now + '%');
    const input = h('input', { type: 'range', class: 'range', min: 0, max: 200, step: 5, value: now, 'aria-label': 'User volume' });
    input.addEventListener('input', () => { value.textContent = input.value + '%'; });
    input.addEventListener('change', () => api.callVolume(id, Number(input.value)).catch(fail));
    ctx.popup(anchor, h('div', { class: 'popup volume-popup', onclick: (e) => e.stopPropagation() },
      h('div', { class: 'control-head' }, h('span', { class: 'control-title' }, 'User volume'), value), input,
      h('button', { type: 'button', class: 'button link small', onclick: () => { input.value = 100; value.textContent = '100%'; api.callVolume(id, 100).catch(fail); } }, 'Reset')));
  }

  // Picking a screen or window to share, its quality, and whether its sound goes too.
  async function sharePicker() {
    let sources = [];
    try { sources = await api.callShareSources(); } catch (e) { fail(e); return; }
    if (!sources.length) { ctx.toast('Nothing to share was found.'); return; }
    let picked = sources[0];
    let quality = QUALITIES[0];
    let audio = false;
    const list = h('div', { class: 'share-sources', role: 'listbox' });
    const drawList = () => {
      list.textContent = '';
      for (const s of sources) {
        list.appendChild(h('button', { type: 'button', role: 'option', class: 'share-source' + (s === picked ? ' selected' : ''), onclick: () => { picked = s; drawList(); } },
          icon(s.kind === 'display' ? ICONS.screen : ICONS.file, 20), h('span', null, s.name)));
      }
    };
    drawList();
    const select = h('select', { class: 'text-input', onchange: () => { quality = QUALITIES.find((q) => q.value === select.value) || QUALITIES[0]; } },
      QUALITIES.map((q) => h('option', { value: q.value }, q.label)));
    const sound = h('input', { type: 'checkbox', onchange: () => { audio = sound.checked; } });
    ctx.modal(h('div', { class: 'modal share-modal', role: 'dialog', 'aria-modal': 'true', onclick: (e) => e.stopPropagation() },
      h('h2', { class: 'modal-title' }, 'Share your screen'),
      h('div', { class: 'modal-text' }, 'Everyone in the call can watch it. It’s end-to-end encrypted like the call.'),
      list,
      h('div', { class: 'share-options' }, h('label', null, 'Quality ', select), h('label', null, sound, ' Share sound too')),
      h('div', { class: 'modal-buttons' },
        h('button', { type: 'button', class: 'button link', onclick: () => ctx.closeLayer() }, 'Cancel'),
        h('button', { type: 'button', class: 'button brand', onclick: () => {
          ctx.closeLayer();
          api.callShare(picked.kind, picked.id, quality.height, quality.fps, audio).catch(fail);
        } }, 'Go Live'))));
  }

  // --- Push-to-talk ------------------------------------------------------------------------------

  // Held while LockCord has focus (and not while typing): the microphone opens.
  let held = false;
  const pttKey = () => { const v = ctx.voiceSettings(); return v && v.mode === 'ptt' ? v.pttKey || 'Backquote' : null; };
  const typingIn = (el) => el && (el.tagName === 'TEXTAREA' || (el.tagName === 'INPUT' && el.type !== 'range' && el.type !== 'checkbox'));
  const setHeld = (on) => { if (held !== on) { held = on; api.callPtt(on).catch(() => {}); } };
  document.addEventListener('keydown', (e) => {
    if (!state.active || e.repeat || e.code !== pttKey() || typingIn(document.activeElement)) return;
    setHeld(true);
  });
  document.addEventListener('keyup', (e) => { if (e.code === pttKey()) setHeld(false); });
  window.addEventListener('blur', () => setHeld(false));

  return {
    state, load, join, leave, inCall, stage, dmArea, headerButtons, panel, roomList, ringCards,
    peek: (channel) => api.callPeek(channel).catch(() => {}),
  };
}
