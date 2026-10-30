// The secret-chat signals: the "Secret" bar over the composer, the header's pill, the code card
// in the timeline and the lock in the DM list. A theme's custom CSS must not be able to fake or
// hide them (a Discord message dressed up as a secret one would leak), so each is drawn inside a
// closed shadow root with a stylesheet of its own, which page CSS can't reach; its host's
// visibility, size and font are pinned with inline !important styles; and it uses system fonts
// only (a theme's font could draw one digit as another). Its colour is the theme's secret
// colour if that stays readable, a fixed green otherwise. The bar and the pill also show what no
// theme can know: the other person's name and the verified code.

import { icon, ICONS } from './dom.js';

const SAFE_GREEN = '#23a559';
const FONT = "'Segoe UI', system-ui, -apple-system, sans-serif";
const MONO = "Consolas, 'Cascadia Mono', 'Courier New', monospace";

const sheet = new CSSStyleSheet();
sheet.replaceSync(`
  :host { all: initial; font-family: ${FONT}; font-size: 14px; line-height: 1.3; color: var(--trust-text); }
  * { box-sizing: border-box; }
  svg { flex: none; fill: currentColor; }
  .bar { display: flex; align-items: center; gap: 6px; padding: 0 4px 4px; font-size: 12px; color: var(--trust); }
  .bar strong { font-weight: 700; }
  .bar .code { font-family: ${MONO}; font-weight: 700; letter-spacing: 0.04em; }
  .pill { display: inline-flex; align-items: center; gap: 4px; padding: 1px 8px; border: 1px solid currentColor; border-radius: 10px;
          font-size: 12px; font-weight: 600; color: var(--trust-warn); white-space: nowrap; }
  .pill.verified { color: var(--trust); }
  .pill .code { font-family: ${MONO}; }
  .lock { display: flex; color: var(--trust); }
  .lock.stale { color: var(--trust-warn); }
  .bar .warn { color: var(--trust-warn); }
  .bar button { margin-left: auto; padding: 0 10px; }
  .card { margin: 16px 16px 8px; padding: 16px; border-radius: 8px; border: 1px solid var(--trust-line);
          background: var(--trust-panel); color: var(--trust-text); }
  .card.quiet { margin: 12px 16px 4px 72px; padding: 0; border: 0; background: none; color: var(--trust-muted); font-size: 14px; }
  .card.verified { color: var(--trust); }
  .card.ended { border-color: var(--trust-danger); }
  .title { display: flex; align-items: center; gap: 6px; font-size: 16px; font-weight: 600; }
  .text { margin-top: 6px; line-height: 20px; }
  .quiet .text, .verified .text { margin-top: 0; }
  .text svg { vertical-align: -2px; }
  .big { margin: 12px 0 4px; font-family: ${MONO}; font-size: 32px; font-weight: 700; letter-spacing: 0.08em; }
  .status { margin-top: 8px; color: var(--trust-warn); }
  .buttons { display: flex; gap: 8px; margin-top: 12px; }
  button { height: 32px; min-width: 60px; padding: 2px 16px; border-radius: 8px; font: 500 14px ${FONT}; cursor: pointer; }
  button.yes { border: 0; background: var(--trust-brand); color: #fff; }
  button.no { border: 1px solid var(--trust-danger); background: transparent; color: var(--trust-danger); }
  .spinner { display: inline-block; width: 12px; height: 12px; border: 2px solid currentColor; border-top-color: transparent;
             border-radius: 50%; animation: spin 0.8s linear infinite; vertical-align: -1px; }
  @keyframes spin { to { transform: rotate(360deg); } }
`);

// --- Colours ----------------------------------------------------------------------------------

// A CSS colour as [r, g, b, a] (0-255, 0-1), by letting the browser resolve it.
function rgba(value) {
  const probe = document.createElement('span');
  probe.style.color = value;
  if (!probe.style.color) return null;
  document.body.appendChild(probe);
  const out = getComputedStyle(probe).color;
  probe.remove();
  const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?\)/.exec(out);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3]), m[4] === undefined ? 1 : Number(m[4])] : null;
}

function luminance([r, g, b]) {
  const lin = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function contrast(a, b) {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

const token = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

// The theme's colour for a signal if it's solid and readable on the chat, the fallback if not.
function readable(name, fallback) {
  const colour = rgba(token(name));
  const bg = rgba(token('--lc-chat-bg')) || [49, 51, 56, 1];
  if (!colour || colour[3] < 0.85 || contrast(colour, bg) < 2.5) return fallback;
  return 'rgb(' + colour.slice(0, 3).join(', ') + ')';
}

function palette() {
  const bg = rgba(token('--lc-chat-bg')) || [49, 51, 56, 1];
  const light = luminance(bg) > 0.4;
  return {
    '--trust': readable('--lc-secret', light ? '#1a7f43' : SAFE_GREEN),
    '--trust-warn': readable('--lc-warning', light ? '#a15c00' : '#f0b232'),
    '--trust-danger': readable('--lc-danger', light ? '#c62828' : '#f23f43'),
    '--trust-brand': readable('--lc-brand', '#5865f2'),
    '--trust-text': light ? '#2e3338' : '#dbdee1',
    '--trust-muted': light ? '#5c5e66' : '#949ba4',
    '--trust-panel': light ? '#f2f3f5' : '#2b2d31',
    '--trust-line': light ? 'rgba(6, 6, 7, 0.15)' : 'rgba(78, 80, 88, 0.48)',
  };
}

// --- Hosts ------------------------------------------------------------------------------------

// A host page CSS can't hide, shrink, move, fade, cover with filters or re-font.
function host(display) {
  const el = document.createElement('span');
  const pinned = {
    display, visibility: 'visible', opacity: '1', filter: 'none', transform: 'none', 'clip-path': 'none', 'mask-image': 'none',
    'mix-blend-mode': 'normal', position: 'relative', 'z-index': '5', 'font-size': '14px', 'font-family': FONT,
    'pointer-events': 'auto', 'content-visibility': 'visible', 'max-height': 'none', overflow: 'visible', width: 'auto', height: 'auto',
  };
  for (const [k, v] of Object.entries(pinned)) el.style.setProperty(k, v, 'important');
  for (const [k, v] of Object.entries(palette())) el.style.setProperty(k, v, 'important');
  const root = el.attachShadow({ mode: 'closed' });
  root.adoptedStyleSheets = [sheet];
  return [el, root];
}

function node(tag, cls, ...children) {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  for (const c of children.flat()) if (c !== null && c !== undefined && c !== false) el.append(c);
  return el;
}

export function formatCode(sas) {
  return String(sas || '').replace(/^(\d{3})(\d{3})$/, '$1 $2');
}

// --- The signals ------------------------------------------------------------------------------

// Over the composer while it sends into a secret chat: to whom, and their verified code.
export function secretBar(peerName, sas) {
  const [el, root] = host('block');
  root.append(node('div', 'bar', icon(ICONS.lock, 14),
    node('span', null, 'Secret with ', node('strong', null, peerName), ' · code ', node('span', 'code', formatCode(sas)), ': goes straight to them, end-to-end encrypted. Discord never sees it.')));
  return el;
}

// In the DM header: verified (with the code) or waiting for the code.
export function secretPill(verified, sas) {
  const [el, root] = host('inline-flex');
  root.append(node('span', 'pill' + (verified ? ' verified' : ''), icon(ICONS.lock, 12),
    verified ? node('span', null, 'Secret chat: verified · ', node('span', 'code', formatCode(sas))) : node('span', null, 'Secret chat: compare the code')));
  return el;
}

// In the DM list, a DM with a live secret chat.
export function secretLock() {
  const [el, root] = host('inline-flex');
  el.title = 'Secret chat';
  root.append(node('span', 'lock', icon(ICONS.lock, 14)));
  return el;
}

// The secret chat's status card in the timeline. spec: { tone: 'quiet'|'gate'|'verified'|'ended',
// title?, text: [parts], code?, status?, buttons?: [{ label, yes, onclick }], spinner? }
export function secretCard(spec) {
  const [el, root] = host('block');
  const card = node('div', 'card ' + spec.tone);
  if (spec.title) card.append(node('div', 'title', icon(ICONS.lock, 18), ' ' + spec.title));
  if (spec.code) card.append(node('div', 'big', formatCode(spec.code)));
  card.append(node('div', 'text', spec.spinner ? node('span', 'spinner') : null, spec.spinner ? ' ' : null,
    spec.lock ? icon(ICONS.lock, 14) : null, spec.lock ? ' ' : null,
    ...spec.text.map((p) => (typeof p === 'object' && p.code ? node('strong', null, formatCode(p.code)) : p))));
  if (spec.status) card.append(node('div', 'status', spec.status));
  if (spec.buttons && spec.buttons.length) {
    card.append(node('div', 'buttons', spec.buttons.map((b) => {
      const button = node('button', b.yes ? 'yes' : 'no', b.label);
      button.type = 'button';
      button.addEventListener('click', b.onclick);
      return button;
    })));
  }
  root.append(card);
  return el;
}

// --- Encrypted DMs (e2e.rs) ----------------------------------------------------------------------

// Over the composer in an encrypted DM: to whom, the contact's safety code (the same on both
// sides; no theme can know it), and the one way to send a message unencrypted, on purpose.
// plainNext: the next message goes unencrypted; onToggle flips it.
export function encryptedBar(peerName, safety, plainNext, onToggle, blocked) {
  const [el, root] = host('block');
  const button = node('button', 'no', plainNext ? 'Keep it encrypted' : 'Send the next one unencrypted');
  button.type = 'button';
  button.style.height = '24px';
  button.style.fontSize = '12px';
  button.addEventListener('click', onToggle);
  root.append(node('div', 'bar', icon(ICONS.lock, 14),
    plainNext
      ? node('span', 'warn', node('strong', null, 'Next message unencrypted: '), 'Discord will be able to read it.')
      : blocked
        ? node('span', 'warn', node('strong', null, 'Not sending: '), blocked)
        : node('span', null, 'Encrypted with ', node('strong', null, peerName), ' · safety code ', node('span', 'code', safety || '—'), ': Discord stores only ciphertext.'),
    button));
  return el;
}

// In the DM header: this DM is encrypted, with its safety code.
export function encryptedPill(safety) {
  const [el, root] = host('inline-flex');
  root.append(node('span', 'pill verified', icon(ICONS.lock, 12), node('span', null, 'Encrypted DM · ', node('span', 'code', safety || '—'))));
  return el;
}

// Beside an encrypted message: it was. Not drawn for messages that weren't.
export function encryptedMark(stale) {
  const [el, root] = host('inline-flex');
  el.title = stale ? 'Encrypted, but sent long before it was posted: it may be an old message posted again.' : 'Encrypted: Discord only has ciphertext';
  root.append(node('span', stale ? 'lock stale' : 'lock', icon(ICONS.lock, 12)));
  return el;
}
