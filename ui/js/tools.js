// The settings window's Purge and Guard pages (deletion.rs, guard.rs, ported from Lazer V2).
// Rust does the scanning and deleting, through its one paced queue; these pages pick what, show
// how far it got, and ask before anything is deleted.

import { h, icon, ICONS } from './dom.js';
import { dropdown, radioCards, toggle } from './controls.js';

const RANGES = [['all', 'All time'], ['24h', 'Last 24 hours'], ['7d', 'Last 7 days'], ['31d', 'Last 31 days']];
const TTLS = [[0, 'Off'], [60, '1 minute'], [600, '10 minutes'], [3600, '1 hour'], [86400, '1 day'], [604800, '7 days'], [2592000, '30 days']];

function ttlText(seconds) {
  const hit = TTLS.find(([s]) => s === seconds);
  if (hit) return hit[1];
  return seconds + ' seconds';
}

const section = (title, ...children) => h('section', { class: 'settings-section' }, title ? h('h2', { class: 'section-title' }, title) : null, ...children);
const block = (title, description, control) => h('div', { class: 'control' }, h('div', { class: 'control-title' }, title),
  description ? h('div', { class: 'control-description' }, description) : null, control);

// --- App ----------------------------------------------------------------------------------------

// ctx: { appSettings(), saveAppSetting(change), saveFolder(), pickSaveFolder(), toast }
export function renderApp(body, ctx, redraw) {
  const s = ctx.appSettings();
  body.appendChild(section('Window',
    toggle('Close to the tray', 'Closing the window keeps LockCord running in the tray, so messages and Guard keep working. Quit from the tray icon.', s.closeToTray !== false, (on) => ctx.saveAppSetting({ closeToTray: on })),
    toggle('Start with Windows', null, !!s.autostart, (on) => ctx.saveAppSetting({ autostart: on })),
    toggle('Start hidden', 'When started with Windows, only the tray icon shows.', !!s.startHidden, (on) => ctx.saveAppSetting({ startHidden: on }))));
  body.appendChild(section('Notifications',
    radioCards([
      { value: 'name', label: 'Who it’s from', description: 'Not what they said. Encrypted DMs only say that one came.' },
      { value: 'text', label: 'Who, and the message', description: 'The text shows in Windows’ notifications, and stays in their history. Never for encrypted DMs.' },
      { value: 'off', label: 'Off', description: null },
    ], s.notify || 'name', (v) => { ctx.saveAppSetting({ notify: v }); redraw(); }),
    h('div', { class: 'control-description spaced' }, 'For DMs, and for mentions in servers; not for muted chats, nor the chat on screen. Secret chats never notify.')));
  const folder = h('span', { class: 'file-pick-name' }, '…');
  ctx.saveFolder().then((r) => { folder.textContent = (r && r.folder) || 'LockCord'; }).catch(() => {});
  body.appendChild(section('Saving files',
    block('Folder', '“Save media” in a message’s menu puts its files here. Programs and scripts get “.download” added, and every file is marked as from the internet.',
      h('div', { class: 'file-pick' }, folder, h('button', { type: 'button', class: 'button secondary', onclick: () => {
        ctx.pickSaveFolder().then((r) => { if (r) redraw(); }).catch((e) => ctx.toast(String(e)));
      } }, 'Change…')))));
}

// --- Encrypted DMs ------------------------------------------------------------------------------

const E2E_STATUS = { on: 'Encrypted', waiting: 'Waiting for their LockCord', changed: '⚠ Their key changed', lost: '⚠ Keys missing', off: 'Off' };

// ctx: { e2eStatus() (main.rs e2e_view), e2eForget(peerId), e2eVerify(peerId, v), peerName(peerId), toast }
export function renderE2e(body, ctx, redraw) {
  const view = ctx.e2eStatus();
  body.appendChild(h('div', { class: 'notice' },
    h('div', null, 'An encrypted DM is your normal DM with another LockCord user, end-to-end encrypted: Discord stores only ciphertext, and only your two LockCords can read it. The messages stay in the DM, readable later; for a live, off-the-record chat use a secret chat instead.'),
    h('div', null, 'To turn it on, press “Encrypt” in a DM’s header. LockCord posts its public key there once; their LockCord answers by itself. Your secret key never leaves Windows Credential Manager.'),
    h('div', null, 'Compare the safety code with them once, on a call. If their key ever changes, LockCord stops sending to them until you check it. Text only for now: files go unencrypted, and only on purpose.')));
  if (view.problem) body.appendChild(h('div', { class: 'notice warn' }, view.problem));
  const contacts = (view.contacts || []).filter((c) => c.status !== 'off');
  if (!contacts.length) {
    body.appendChild(h('div', { class: 'empty-note' }, 'No encrypted DMs yet.'));
    return;
  }
  body.appendChild(section('Encrypted DMs', h('div', { class: 'theme-rows' }, contacts.map((c) => h('div', { class: 'theme-row' },
    h('span', { class: 'guard-shield' + (c.status === 'on' ? '' : ' warn') }, icon(ICONS.lock, 20)),
    h('div', { class: 'theme-row-text' }, h('div', { class: 'theme-row-name' }, ctx.peerName(c.peer)),
      h('div', { class: 'control-description' }, E2E_STATUS[c.status] + (c.safety ? ' · safety code ' + c.safety : '') + (c.verified ? ' · compared ✓' : '')
        + (c.since ? ' · since ' + new Date(c.since).toLocaleDateString() : ''))),
    c.status === 'on' ? h('button', { type: 'button', class: 'button secondary', onclick: () => ctx.e2eVerify(c.peer, !c.verified).then(redraw).catch((e) => ctx.toast(String(e))) }, c.verified ? 'Unmark compared' : 'Mark compared') : null,
    h('button', { type: 'button', class: 'button danger-outline', onclick: (e) => {
      const b = e.currentTarget;
      if (!b.dataset.armed) { b.dataset.armed = '1'; b.textContent = 'Click again: old messages become unreadable'; return; }
      ctx.e2eForget(c.peer).then(redraw).catch((err) => ctx.toast(String(err)));
    } }, 'Turn off'))))));
}

// --- Translation defaults ---------------------------------------------------------------------

// ctx: { translationDefaults(), setTranslationDefaults(change), langOptions }
export function renderTranslation(body, ctx, redraw) {
  const t = ctx.translationDefaults();
  body.appendChild(h('div', { class: 'notice' },
    h('div', null, 'Translation goes through Google, as it did in LockCord before: the text of whatever is translated is sent to Google. Secret chats and encrypted DMs are never translated.'),
    h('div', null, 'These are the defaults for chats you haven’t set up yourself; each chat’s own settings are under the globe in its header.')));
  body.appendChild(section('Messages from others',
    toggle('Translate messages', 'Shown in place of the original, which is a click away.', !!t.incoming, (on) => ctx.setTranslationDefaults({ incoming: on })),
    block('Into', null, dropdown(ctx.langOptions, t.incomingLang || 'en', (v) => { ctx.setTranslationDefaults({ incomingLang: v }); redraw(); }, 'Into'))));
  body.appendChild(section('What you send',
    toggle('Translate what I send', 'Your message is translated, then sent; you see the original in the box until it’s gone.', !!t.outgoing, (on) => ctx.setTranslationDefaults({ outgoing: on })),
    block('Into', null, dropdown(ctx.langOptions, t.outgoingLang || 'en', (v) => { ctx.setTranslationDefaults({ outgoingLang: v }); redraw(); }, 'Into'))));
}

// --- Purge --------------------------------------------------------------------------------------

// Kept while the window stays open, so leaving the page and coming back keeps the choices.
const pick = { selected: new Set(), filter: '', keywords: '', mode: 'exclude', range: 'all', attachments: false, open: new Set(['dms']) };

// ctx: { channelGroups() -> [{ id, label, channels: [{ id, name }] }], purgeStatus(), purgeScan, purgeRun,
//        purgeStop, toast }
export function renderPurge(body, ctx, redraw) {
  body.appendChild(h('div', { class: 'notice' },
    h('div', null, 'Deletes your own messages: pick channels, scan, check the count, then confirm. Nothing is deleted before you confirm.'),
    h('div', null, 'Deletes are paced the way Lazer does it (one every 350 ms), so a big purge takes a while. That’s on purpose: a burst of deletes is what gets accounts flagged.')));

  const status = ctx.purgeStatus();
  const busy = ['scanning', 'review', 'deleting'].includes(status.state);

  // Channels: DMs and each server, with a filter box and "all" per group.
  const groups = ctx.channelGroups();
  const filterBox = h('input', { class: 'text-input', placeholder: 'Find a channel or server', value: pick.filter, 'aria-label': 'Find a channel' });
  filterBox.addEventListener('input', () => { pick.filter = filterBox.value; drawList(); });
  const list = h('div', { class: 'channel-pick' });
  const count = h('span', { class: 'control-value' });
  const drawCount = () => { count.textContent = pick.selected.size + ' selected'; };
  const drawList = () => {
    list.textContent = '';
    const q = pick.filter.trim().toLowerCase();
    for (const g of groups) {
      const shown = g.channels.filter((c) => !q || c.name.toLowerCase().includes(q) || g.label.toLowerCase().includes(q));
      if (!shown.length) continue;
      const open = pick.open.has(g.id) || !!q;
      const allOn = shown.every((c) => pick.selected.has(c.id));
      list.appendChild(h('div', { class: 'pick-group' },
        h('button', { type: 'button', class: 'pick-group-head', onclick: () => { if (pick.open.has(g.id)) pick.open.delete(g.id); else pick.open.add(g.id); drawList(); } },
          h('span', { class: 'pick-caret' + (open ? ' open' : '') }, icon(ICONS.chevron, 16)), h('span', { class: 'pick-group-name' }, g.label),
          h('span', { class: 'control-value' }, String(g.channels.filter((c) => pick.selected.has(c.id)).length || ''))),
        h('button', { type: 'button', class: 'button link small', disabled: busy, onclick: () => {
          for (const c of shown) { if (allOn) pick.selected.delete(c.id); else pick.selected.add(c.id); }
          drawList(); drawCount();
        } }, allOn ? 'None' : 'All')));
      if (!open) continue;
      for (const c of shown) {
        const box = h('input', { type: 'checkbox', checked: pick.selected.has(c.id), disabled: busy });
        box.addEventListener('change', () => { if (box.checked) pick.selected.add(c.id); else pick.selected.delete(c.id); drawCount(); drawList(); });
        list.appendChild(h('label', { class: 'pick-row' }, box, h('span', null, c.name)));
      }
    }
  };
  drawList();
  drawCount();

  const keywords = h('input', { class: 'text-input', placeholder: 'e.g. address, phone, lol', value: pick.keywords, disabled: busy, 'aria-label': 'Keywords' });
  keywords.addEventListener('input', () => { pick.keywords = keywords.value; });

  body.appendChild(section('Channels', h('div', { class: 'control-head' }, h('span', { class: 'control-title' }, 'Where'), count), filterBox, list));
  body.appendChild(section('Filters',
    block('Keywords', 'Separated by commas; leave empty for all your messages.', keywords),
    radioCards([{ value: 'exclude', label: 'All except these' }, { value: 'only', label: 'Only these' }], pick.mode, (v) => { pick.mode = v; redraw(); }),
    block('From', null, radioCards(RANGES.map(([value, label]) => ({ value, label })), pick.range, (v) => { pick.range = v; redraw(); })),
    toggle('Only messages with files', 'Images, videos and other attachments.', pick.attachments, (on) => { pick.attachments = on; })));

  // What Rust is doing, and the buttons for it.
  const panel = h('div', { class: 'purge-status' });
  const stop = () => ctx.purgeStop().catch((e) => ctx.toast(String(e)));
  if (status.state === 'scanning') {
    panel.append(h('div', { class: 'purge-line' }, h('span', { class: 'spinner' }), ' Scanning ' + (status.channel || '') + '… ' + (status.found || 0) + ' found'),
      h('button', { type: 'button', class: 'button secondary', onclick: stop }, 'Stop'));
  } else if (status.state === 'review') {
    panel.append(h('div', { class: 'purge-line' }, h('strong', null, status.total.toLocaleString() + ' of your messages'), ' in ' + status.channels + ' channel' + (status.channels === 1 ? '' : 's') + '. Delete them? This can’t be undone.'),
      h('div', { class: 'settings-row' },
        h('button', { type: 'button', class: 'button danger', onclick: () => ctx.purgeRun().catch((e) => ctx.toast(String(e))) }, 'Delete ' + status.total.toLocaleString() + ' messages'),
        h('button', { type: 'button', class: 'button link', onclick: stop }, 'Cancel')));
  } else if (status.state === 'deleting') {
    const done = (status.deleted || 0) + (status.failed || 0);
    const bar = h('div', { class: 'progress' }, h('div', { class: 'progress-fill' }));
    bar.firstChild.style.width = (status.total ? (done / status.total) * 100 : 0) + '%';
    panel.append(h('div', { class: 'purge-line' }, 'Deleting… ' + done.toLocaleString() + ' of ' + status.total.toLocaleString()), bar,
      h('button', { type: 'button', class: 'button secondary', onclick: stop }, 'Stop'));
  } else {
    if (status.message) panel.append(h('div', { class: 'purge-line' }, status.message));
    panel.append(h('button', {
      type: 'button', class: 'button brand', disabled: !pick.selected.size, onclick: () => {
        const names = new Map(groups.flatMap((g) => g.channels.map((c) => [c.id, (g.id === 'dms' ? '' : g.label + ' ') + c.name])));
        const channels = [...pick.selected].filter((id) => names.has(id)).map((id) => ({ id, name: names.get(id) }));
        const filters = { keywords: pick.keywords.split(',').map((k) => k.trim()).filter(Boolean), keywordMode: pick.mode, range: pick.range, attachmentsOnly: pick.attachments };
        ctx.purgeScan(channels, filters).catch((e) => ctx.toast(String(e)));
      },
    }, 'Scan ' + (pick.selected.size || '') + ' channel' + (pick.selected.size === 1 ? '' : 's')));
  }
  if (status.unread && status.unread.length) panel.append(h('div', { class: 'control-description' }, 'Couldn’t read: ' + status.unread.join(', ')));
  body.appendChild(section('Purge', panel));
}

// --- Guard --------------------------------------------------------------------------------------

const form = { channel: null, trigger: '', ttl: 0 };

// ctx: { channelGroups(), guardList(), guardSet(rule), guardRemove(id), toast }; preset: a channel id to start with.
export function renderGuard(body, ctx, redraw, preset) {
  if (preset) form.channel = preset;
  body.appendChild(h('div', { class: 'notice' },
    h('div', null, h('strong', null, 'Trigger word: '), 'send a message containing it in a guarded channel and all your messages there are wiped, the trigger too. Only a trigger sent after Guard was switched on counts.'),
    h('div', null, h('strong', null, 'Time limit: '), 'your messages there are deleted once they’re that old. Only messages sent after Guard was switched on; turning it on never touches the past.')));

  const groups = ctx.channelGroups();
  const options = groups.flatMap((g) => g.channels.map((c) => ({ value: c.id, label: (g.id === 'dms' ? '' : g.label + ' · ') + c.name })));
  const nameOf = (id) => (options.find((o) => o.value === id) || {}).label || id;
  if (!form.channel && options.length) form.channel = options[0].value;

  const trigger = h('input', { class: 'text-input', placeholder: 'e.g. nuke', value: form.trigger, maxLength: 100, 'aria-label': 'Trigger word' });
  trigger.addEventListener('input', () => { form.trigger = trigger.value; });
  body.appendChild(section('Guard a channel',
    block('Channel', null, dropdown(options.length ? options : [{ value: '', label: 'No channels' }], form.channel, (v) => { form.channel = v; redraw(); }, 'Channel')),
    block('Trigger word', 'Leave empty for none.', trigger),
    block('Time limit', null, dropdown(TTLS.map(([value, label]) => ({ value, label })), form.ttl, (v) => { form.ttl = v; redraw(); }, 'Time limit')),
    h('div', { class: 'settings-row spaced' }, h('button', {
      type: 'button', class: 'button brand', onclick: async () => {
        try {
          await ctx.guardSet({ channel: form.channel, name: nameOf(form.channel), trigger: form.trigger.trim(), ttl: form.ttl });
          ctx.toast('Guard is on for ' + nameOf(form.channel) + '.');
          form.trigger = '';
          redraw();
        } catch (e) {
          ctx.toast('Not saved: ' + e);
        }
      },
    }, 'Guard this channel'))));

  const rules = h('div', { class: 'theme-rows' }, h('div', { class: 'control-description' }, 'Loading…'));
  body.appendChild(section('Guarded channels', rules));
  ctx.guardList().then((list) => {
    rules.textContent = '';
    if (!list.length) { rules.appendChild(h('div', { class: 'empty-note' }, 'No channel is guarded.')); return; }
    for (const r of list) {
      const parts = [r.trigger ? 'Trigger “' + r.trigger + '”' : null, r.ttl ? 'deleted after ' + ttlText(r.ttl) : null].filter(Boolean).join(' · ');
      rules.appendChild(h('div', { class: 'theme-row' },
        h('span', { class: 'guard-shield' }, icon(ICONS.shield, 20)),
        h('div', { class: 'theme-row-text' }, h('div', { class: 'theme-row-name' }, r.name || nameOf(r.channel)),
          h('div', { class: 'control-description' }, parts + ' · since ' + new Date(r.armedAt).toLocaleString())),
        h('button', { type: 'button', class: 'button secondary', onclick: () => { form.channel = r.channel; form.trigger = r.trigger; form.ttl = r.ttl; redraw(); } }, 'Edit'),
        h('button', { type: 'button', class: 'button danger-outline', onclick: (e) => {
          const b = e.currentTarget;
          if (!b.dataset.armed) { b.dataset.armed = '1'; b.textContent = 'Click again to stop'; return; }
          ctx.guardRemove(r.channel).then(redraw).catch((err) => ctx.toast(String(err)));
        } }, 'Stop guarding')));
    }
  }).catch((e) => { rules.textContent = String(e); });
}
