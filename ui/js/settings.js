// The settings window: Themes first (a theme is the whole look; each has light and dark colour
// schemes, and picking one applies at once), then the app's own pages (tools.js).

import { h, icon, ICONS, MOD } from './dom.js';
import { radioCards, slider, toggle } from './controls.js';
import { parse, render } from './markdown.js';
import { avatar, displayName } from './people.js';
import { THEMES, schemesOf } from './theme.js';
import { renderApp, renderE2e, renderGuard, renderPrivacy, renderPurge, renderTranslation, renderVoice } from './tools.js';

// The pages, their icons, and the sections the nav jumps to.
const PAGES = [
  ['themes', 'Themes', 'palette', [['theme', 'Theme'], ['colours', 'Colours'], ['display', 'Display']]],
  ['app', 'App', 'gear', []],
  ['privacy', 'Privacy', 'eye', []],
  ['voice', 'Voice & Video', 'mic', []],
  ['e2e', 'Encrypted DMs', 'lock', []],
  ['translation', 'Translation', 'globe', []],
  ['purge', 'Purge', 'trash', []],
  ['guard', 'Guard', 'shield', []],
];

// --- The preview card -------------------------------------------------------------------------

// A small chat drawn with the real message styles, so the theme shows here. The people in it are
// made up (their avatars are initials) and nothing in it is loaded.
function previewCard(me) {
  const wren = { id: 'preview-wren', username: 'wren', global_name: 'Wren' };
  const you = me || { id: 'preview-you', username: 'you', global_name: 'You' };
  const md = { nameOf: (id) => (id === '1' ? 'Wren' : null), openLink: () => {}, formatTime: (d) => d.toLocaleString() };
  const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  const row = (who, text, { start, mine } = {}) => {
    const li = h('li', { class: 'message' + (start ? ' group-start' : '') + (mine ? ' mine' : '') });
    if (start) {
      li.append(avatar(who), h('h3', { class: 'header' }, h('span', { class: 'username' }, who.global_name || who.username),
        h('time', { class: 'timestamp' }, 'Today at ' + time)));
    } else {
      li.append(h('time', { class: 'gutter-time' }, time));
    }
    const content = h('div', { class: 'content' });
    content.appendChild(render(parse(text), document, md));
    li.append(h('div', { class: 'body' }, content));
    return li;
  };
  return h('div', { class: 'settings-preview', 'aria-label': 'Preview' },
    h('ol', { class: 'messages' },
      row(wren, 'Here’s how your chats will look ✨', { start: true }),
      row(wren, 'Pick a theme and a colour scheme, and this changes with it.'),
      row(you, 'Looks good. Do **bold**, `code` and <@1> still read well?', { start: true, mine: true }),
      row(wren, 'Perfectly 👍 ||and spoilers too||', { start: true })));
}

// A scheme as a small square: its chat colour, its side panel and accent in the corner.
function schemeTile(scheme, selected, onclick) {
  const t = scheme.tokens || {};
  const tile = h('button', { type: 'button', class: 'theme-tile' + (selected ? ' selected' : ''), title: scheme.name, 'aria-label': scheme.name, 'aria-pressed': selected ? 'true' : 'false', onclick },
    h('span', { class: 'theme-tile-side' }), h('span', { class: 'theme-tile-dot' }), selected ? h('span', { class: 'theme-check' }, icon(ICONS.check, 14)) : null);
  tile.style.background = t['--lc-chat-bg'] || '#313338';
  tile.firstChild.style.background = t['--lc-sidebar'] || '#2b2d31';
  tile.children[1].style.background = t['--lc-brand'] || '#5865f2';
  return h('div', { class: 'theme-tile-wrap' }, tile, h('span', { class: 'theme-tile-name' }, scheme.name));
}

// A theme as a card: a little window in its first scheme's colours.
function themeCard(theme, selected, onclick) {
  const t = (schemesOf(theme.id)[0] || {}).tokens || {};
  const mini = h('span', { class: 'skin-mini skin-mini-' + theme.id }, h('span', { class: 'skin-mini-rail' }), h('span', { class: 'skin-mini-side' }),
    h('span', { class: 'skin-mini-chat' }, h('span', { class: 'skin-mini-line' }), h('span', { class: 'skin-mini-line short' }), h('span', { class: 'skin-mini-button' })));
  mini.children[0].style.background = t['--lc-rail'] || '#1e1f22';
  mini.children[1].style.background = t['--lc-sidebar'] || '#2b2d31';
  mini.children[2].style.background = t['--lc-chat-bg'] || '#313338';
  mini.querySelector('.skin-mini-button').style.background = t['--lc-brand'] || '#5865f2';
  return h('button', { type: 'button', class: 'skin-card' + (selected ? ' selected' : ''), 'aria-pressed': selected ? 'true' : 'false', onclick },
    mini, h('span', { class: 'skin-card-name' }, theme.name), h('span', { class: 'skin-card-text' }, theme.description));
}

// ctx: { me, scheme() (the scheme in use), choose(scheme), display(), setDisplay(change), and the
//        pages' own (tools.js) }
// ctx.start: { page, channel } to open on (the header's shield opens Guard for its channel).
// Returns a function that lets go of what the window listens to.
export function renderSettings(screen, ctx) {
  let page = (ctx.start && ctx.start.page) || 'themes';
  if (!PAGES.some((p) => p[0] === page)) page = 'themes';
  let presetChannel = ctx.start && ctx.start.channel;

  const nav = h('nav', { class: 'settings-nav' });
  const title = h('h1', { class: 'settings-head-title' });
  const head = h('header', { class: 'settings-head' }, title,
    h('button', { type: 'button', class: 'settings-x', title: 'Close (Esc)', 'aria-label': 'Close settings', onclick: () => ctx.close() }, icon(ICONS.close, 24)));
  const body = h('div', { class: 'settings-page' });
  const scroll = h('div', { class: 'settings-scroll' }, body);
  screen.textContent = '';
  screen.append(nav, h('div', { class: 'settings-main' }, head, scroll));

  const jump = (anchor) => {
    const target = body.querySelector('[data-anchor="' + anchor + '"]');
    if (target) target.scrollIntoView({ block: 'start', behavior: 'smooth' });
  };

  const drawNav = () => {
    nav.textContent = '';
    const me = ctx.me;
    if (me) nav.appendChild(h('div', { class: 'settings-me' }, avatar(me, 'settings-me-avatar'), h('div', { class: 'settings-me-name' }, displayName(me))));
    nav.appendChild(h('div', { class: 'settings-nav-title' }, 'LockCord'));
    for (const [id, label, glyph, parts] of PAGES) {
      nav.appendChild(h('button', { type: 'button', class: 'settings-nav-item' + (page === id ? ' selected' : ''), onclick: () => { page = id; scroll.scrollTop = 0; draw(); } },
        icon(ICONS[glyph], 20), h('span', null, label)));
      if (page === id && parts.length) {
        nav.appendChild(h('div', { class: 'settings-nav-sub' }, parts.map(([anchor, text]) => h('button', { type: 'button', class: 'settings-nav-subitem', onclick: () => jump(anchor) }, text))));
      }
    }
  };

  const section = (heading, anchor, ...children) => h('section', { class: 'settings-section', 'data-anchor': anchor }, heading ? h('h2', { class: 'section-title' }, heading) : null, ...children);
  const block = (heading, description, control) => h('div', { class: 'control' }, h('div', { class: 'control-title' }, heading),
    description ? h('div', { class: 'control-description' }, description) : null, control);

  // --- Themes -----------------------------------------------------------------------------------

  const themes = () => {
    const now = ctx.scheme();
    const pick = (scheme) => Promise.resolve(ctx.choose(scheme)).then(draw);
    body.appendChild(previewCard(ctx.me));
    body.appendChild(section('Theme', 'theme',
      h('div', { class: 'control-description' }, 'A theme is the whole look: shapes, frames, buttons, icons and how things move.'),
      h('div', { class: 'skin-cards' }, THEMES.map((t) => themeCard(t, now.theme === t.id, () => {
        if (now.theme === t.id) return;
        // The same brightness as now, where the theme has it.
        const list = schemesOf(t.id);
        pick(list.find((s) => s.mode === now.mode) || list[0]);
      })))));
    const list = schemesOf(now.theme);
    const row = (mode, label) => {
      const of = list.filter((s) => s.mode === mode);
      return of.length ? h('div', null, h('div', { class: 'control-title spaced' }, label), h('div', { class: 'theme-tiles' }, of.map((s) => schemeTile(s, s.id === now.id, () => pick(s))))) : null;
    };
    body.appendChild(section('Colours', 'colours',
      h('div', { class: 'control-description' }, 'Colour schemes for ' + (THEMES.find((t) => t.id === now.theme) || THEMES[0]).name + '.'),
      row('light', 'Light'), row('dark', 'Dark')));
    const d = ctx.display();
    body.appendChild(section('Display', 'display',
      block('Messages', null, radioCards([
        { value: 'cozy', label: 'Cozy', description: 'Roomy, with big avatars.' },
        { value: 'compact', label: 'Compact', description: 'More messages on screen.' },
      ], d.density === 'compact' ? 'compact' : 'cozy', (density) => { ctx.setDisplay({ density }); draw(); }, 'two')),
      slider({ title: 'Text size', min: 12, max: 20, step: 1, value: d.textSize || 16, marks: [12, 16, 20], format: (n) => n + 'px', onInput: (n) => ctx.setDisplay({ textSize: n }) }),
      toggle('Animations', 'Off keeps everything still (the system’s “reduce motion” turns them off too).', d.motion !== false, (motion) => ctx.setDisplay({ motion })),
      h('div', { class: 'control-description spaced' }, MOD + '+Shift+0 switches to the default theme for a moment, if a theme ever looks wrong.')));
  };

  const purge = () => renderPurge(body, ctx, draw);
  const translation = () => renderTranslation(body, ctx, draw);
  const e2e = () => renderE2e(body, ctx, draw);
  const app = () => renderApp(body, ctx, draw);
  const privacy = () => renderPrivacy(body, ctx, draw);
  const guard = () => { renderGuard(body, ctx, draw, presetChannel); presetChannel = null; };

  // What a page leaves running (Voice & Video's microphone test), stopped when it's redrawn or left.
  let cleanup = null;
  const voice = () => { cleanup = renderVoice(body, ctx, draw); };
  const draw = () => {
    const keep = scroll.scrollTop;
    if (cleanup) { cleanup(); cleanup = null; }
    drawNav();
    title.textContent = PAGES.find((p) => p[0] === page)[1];
    body.textContent = '';
    ({ themes, app, privacy, voice, e2e, translation, purge, guard })[page]();
    scroll.scrollTop = keep;
  };
  // Rust's purge progress redraws the Purge page while it's open.
  const stopListening = ctx.onPurge(() => { if (page === 'purge') draw(); });

  draw();
  return () => { stopListening(); if (cleanup) cleanup(); };
}
