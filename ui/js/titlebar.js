// LockCord's own title bar (the window has no Windows frame), so a skin can dress it too: the
// name to drag the window by (double-click maximizes), and minimize, maximize and close. Close
// goes through the usual close request, so "close to the tray" still applies. On a Mac the system's
// traffic-light buttons sit at its left instead (main.rs), and the bar leaves room for them.

import { call } from './api.js';
import { h, icon, ICONS, IS_MAC } from './dom.js';

const window_ = (command) => call('plugin:window|' + command, { label: 'main' }).catch(() => {});
const GLYPHS = {
  minimize: 'M0 5h10v1H0z',
  maximize: 'M0 0h10v10H0V0Zm1 1v8h8V1H1Z',
  restore: 'M2 0h8v8H8v2H0V2h2V0Zm1 2h5v5h1V1H3v1ZM1 3v6h6V3H1Z',
  close: 'M.7 0 5 4.3 9.3 0l.7.7L5.7 5l4.3 4.3-.7.7L5 5.7.7 10 0 9.3 4.3 5 0 .7.7 0Z',
};
const glyph = (d) => {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 10 10');
  svg.setAttribute('class', 'lc-icon titlebar-glyph');
  const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  path.setAttribute('d', d);
  path.setAttribute('fill', 'currentColor');
  svg.appendChild(path);
  return svg;
};

export function createTitlebar(title) {
  document.documentElement.classList.toggle('lc-os-mac', IS_MAC);
  if (IS_MAC) {
    return h('div', { class: 'titlebar' },
      h('div', { class: 'titlebar-title', 'data-tauri-drag-region': '' }, icon(ICONS.lock, 14), h('span', { 'data-tauri-drag-region': '' }, title)));
  }
  const maximize = h('button', { type: 'button', class: 'titlebar-button', title: 'Maximize', 'aria-label': 'Maximize', onclick: () => window_('toggle_maximize').then(sync) }, glyph(GLYPHS.maximize));
  const bar = h('div', { class: 'titlebar' },
    h('div', { class: 'titlebar-title', 'data-tauri-drag-region': '' }, icon(ICONS.lock, 14), h('span', { 'data-tauri-drag-region': '' }, title)),
    h('div', { class: 'titlebar-buttons' },
      h('button', { type: 'button', class: 'titlebar-button', title: 'Minimize', 'aria-label': 'Minimize', onclick: () => window_('minimize') }, glyph(GLYPHS.minimize)),
      maximize,
      h('button', { type: 'button', class: 'titlebar-button close', title: 'Close', 'aria-label': 'Close', onclick: () => window_('close') }, glyph(GLYPHS.close))));
  // The maximize button shows restore while maximized.
  const sync = () => call('plugin:window|is_maximized', { label: 'main' }).then((max) => {
    maximize.replaceChildren(glyph(max ? GLYPHS.restore : GLYPHS.maximize));
    maximize.title = max ? 'Restore' : 'Maximize';
    bar.classList.toggle('maximized', !!max);
  }).catch(() => {});
  let timer = null;
  window.addEventListener('resize', () => { clearTimeout(timer); timer = setTimeout(sync, 120); });
  sync();
  return bar;
}
