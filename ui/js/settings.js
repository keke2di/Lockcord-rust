// The settings window: Appearance, Effects, Custom CSS and Theme files, in Discord's style
// (controls.js). Every page edits one draft of the theme, previewed live on the whole window and
// in the preview card at the top; the bar at the bottom saves or discards it. Saving goes through
// Rust (settings.rs), which checks the theme again; closing without saving puts the saved theme
// back (app.js closeSettings).

import { h, icon, ICONS } from './dom.js';
import { colourTile, dropdown, radioCards, slider, toggle } from './controls.js';
import { parse, render } from './markdown.js';
import { avatar, displayName } from './people.js';
import { applyTheme, OPTIONS, PRESETS } from './theme.js';
import { renderApp, renderE2e, renderGuard, renderPurge, renderTranslation, renderVoice } from './tools.js';

const COLOR_GROUPS = [
  ['Backgrounds', [['--lc-chat-bg', 'Chat'], ['--lc-sidebar', 'Side panel'], ['--lc-rail', 'Server rail'], ['--lc-panel', 'User panel'],
    ['--lc-popout', 'Popups'], ['--lc-composer', 'Message box'], ['--lc-deep', 'Deepest']]],
  ['Text', [['--lc-header', 'Names and titles'], ['--lc-text', 'Text'], ['--lc-muted', 'Muted'], ['--lc-link', 'Links'],
    ['--lc-interactive', 'Icons'], ['--lc-interactive-hover', 'Icons, hovered']]],
  ['Accents', [['--lc-brand', 'Accent'], ['--lc-secret', 'Secret chats'], ['--lc-positive', 'Online, success'], ['--lc-danger', 'Danger'],
    ['--lc-warning', 'Warning'], ['--lc-mention', 'Mentions']]],
  ['Surfaces', [['--lc-hover', 'Message hover'], ['--lc-sidebar-hover', 'List hover'], ['--lc-sidebar-selected', 'List selected'],
    ['--lc-border', 'Borders'], ['--lc-card', 'Message cards'], ['--lc-bubble', 'Bubbles'], ['--lc-bubble-mine', 'Your bubbles'], ['--lc-code-bg', 'Code']]],
];

const FONTS = [
  ['Default', "'gg sans', 'Segoe UI', 'Noto Sans', 'Helvetica Neue', Helvetica, Arial, sans-serif"],
  ['Segoe UI', "'Segoe UI', sans-serif"],
  ['System', 'system-ui, sans-serif'],
  ['Arial', 'Arial, sans-serif'],
  ['Verdana', 'Verdana, sans-serif'],
  ['Trebuchet', "'Trebuchet MS', sans-serif"],
  ['Georgia', 'Georgia, serif'],
  ['Consolas', 'Consolas, monospace'],
  ['Comic Sans', "'Comic Sans MS', 'Comic Sans', cursive"],
];

const AVATAR_SHAPES = [['Circle', '50%'], ['Squircle', '30%'], ['Rounded', '15%'], ['Square', '0']];

const CHOICES = {
  scheme: [['dark', 'Dark', 'Dark controls, scrollbars and pickers.'], ['light', 'Light', 'Light controls, for light themes.']],
  messages: [['flat', 'Flat', 'Like Discord.'], ['cards', 'Cards', 'Each run of messages on a card of its own.'], ['bubbles', 'Bubbles', 'Text in bubbles, yours tinted.']],
  density: [['cozy', 'Cozy', 'Roomy, with big avatars.'], ['compact', 'Compact', 'More messages on screen.']],
  background: [['none', 'None', 'The theme’s colours only.'], ['image', 'Image', 'A picture of your own.'], ['gradient', 'Gradient', 'Three colours, blended.'], ['animated', 'Animated gradient', 'The blend, slowly moving.']],
  panels: [['solid', 'Solid', 'Panels in their own colours.'], ['glass', 'Glass', 'The background shows through, blurred.']],
};

// The pages, their icons, and the sections the nav jumps to.
const PAGES = [
  ['appearance', 'Appearance', 'palette', [['theme', 'Theme'], ['messages', 'Messages'], ['text', 'Text'], ['layout', 'Layout'], ['colours', 'Colours']]],
  ['effects', 'Effects', 'sparkle', [['background', 'Background'], ['panels', 'Panels'], ['accents', 'Accents']]],
  ['css', 'Custom CSS', 'code', []],
  ['themes', 'Theme files', 'folder', []],
  ['app', 'App', 'gear', []],
  ['voice', 'Voice & Video', 'mic', []],
  ['e2e', 'Encrypted DMs', 'lock', []],
  ['translation', 'Translation', 'globe', []],
  ['purge', 'Purge', 'trash', []],
  ['guard', 'Guard', 'shield', []],
];

// The value the window is using for a token right now (the draft's, the theme's, or app.css's).
function live(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function num(value, fallback) {
  const n = parseFloat(value);
  return Number.isFinite(n) ? n : fallback;
}

// --- The preview card -------------------------------------------------------------------------

// A small chat drawn with the real message styles, so whatever the theme does shows here. The
// people in it are made up (their avatars are initials) and nothing in it is loaded.
function previewCard(me) {
  const wren = { id: 'preview-wren', username: 'wren', global_name: 'Wren' };
  const you = me || { id: 'preview-you', username: 'you', global_name: 'You' };
  const md = { nameOf: (id) => (id === '1' ? 'Wren' : null), openLink: () => {}, formatTime: (d) => d.toLocaleString() };
  const now = new Date();
  const time = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
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
      row(wren, 'Change anything below and this updates as you go.'),
      row(you, 'Looks good. Do **bold**, `code` and <@1> still read well?', { start: true, mine: true }),
      row(wren, 'Perfectly 👍 ||and spoilers too||', { start: true })));
}

// ctx: { prefs, me, active (the saved theme in use), choose(theme), save(draft) -> theme,
//        remove(id), addAsset(id, file) -> name, importTheme() -> theme|null, exportTheme(id),
//        toast(text), close() }
// ctx.start: { page, channel } to open on (the header's shield opens Guard for its channel).
// Returns a function that lets go of what the window listens to.
export function renderSettings(screen, ctx) {
  let page = (ctx.start && ctx.start.page) || 'appearance';
  let presetChannel = ctx.start && ctx.start.channel;
  let draft = null; // the theme being edited: a copy until it's saved
  let saving = false;

  let floating = false; // the Preview button's floating preview
  const nav = h('nav', { class: 'settings-nav' });
  const title = h('h1', { class: 'settings-head-title' });
  const previewButton = h('button', { type: 'button', class: 'button secondary small', onclick: () => { floating = !floating; drawFloating(); } }, icon(ICONS.eye, 16), 'Preview');
  const head = h('header', { class: 'settings-head' }, title, previewButton,
    h('button', { type: 'button', class: 'settings-x', title: 'Close (Esc)', 'aria-label': 'Close settings', onclick: () => ctx.close() }, icon(ICONS.close, 24)));
  const body = h('div', { class: 'settings-page' });
  const scroll = h('div', { class: 'settings-scroll' }, body);
  const float = h('div', { class: 'settings-float' });
  const bar = h('div', { class: 'settings-bar', role: 'status' });
  screen.textContent = '';
  screen.append(nav, h('div', { class: 'settings-main' }, head, scroll, float, bar));

  const drawFloating = () => {
    float.textContent = '';
    float.hidden = !floating;
    previewButton.classList.toggle('on', floating);
    if (floating) float.append(previewCard(ctx.me));
  };

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

  // What a page leaves running (Voice & Video's microphone test), stopped when it's redrawn or left.
  let cleanup = null;
  const draw = () => {
    const keep = scroll.scrollTop;
    if (cleanup) { cleanup(); cleanup = null; }
    drawNav();
    title.textContent = PAGES.find((p) => p[0] === page)[1];
    body.textContent = '';
    ({ appearance, effects, css, themes, app, voice, e2e, translation, purge, guard })[page]();
    drawBar();
    drawFloating();
    scroll.scrollTop = keep;
  };

  // --- The draft --------------------------------------------------------------------------------

  // The first change starts a draft from the theme in use; a preset's becomes a new theme.
  const edit = () => {
    if (draft) return draft;
    const base = ctx.active;
    draft = {
      id: base.builtin ? null : base.id,
      name: base.builtin ? base.name + ' (mine)' : base.name,
      author: base.author || '',
      tokens: { ...(base.tokens || {}) },
      options: { ...(base.options || {}) },
      css: base.css || '',
      backgroundAsset: base.backgroundAsset || '',
      fontAsset: base.fontAsset || '',
      assets: base.assets || [],
    };
    drawBar();
    return draft;
  };
  const preview = () => applyTheme({ ...draft, id: draft.id || 'draft', builtin: !draft.id });
  const setToken = (name, value) => { edit().tokens[name] = value; preview(); };
  const setOption = (name, value) => { edit().options[name] = value; preview(); };
  const shown = () => draft || ctx.active;
  const tokenOf = (name) => (shown().tokens || {})[name] || live(name);
  const optionOf = (name) => ((shown().options || {})[name]) || OPTIONS[name][0];

  const drawBar = () => {
    bar.textContent = '';
    bar.hidden = !draft;
    if (!draft) return;
    const name = h('input', { class: 'text-input bar-name', value: draft.name, maxLength: 60, 'aria-label': 'Theme name' });
    name.addEventListener('input', () => { draft.name = name.value; });
    bar.append(h('span', { class: 'bar-text' }, draft.id ? 'Careful, unsaved changes to' : 'Your new theme:'), name,
      h('button', { type: 'button', class: 'button link', onclick: () => discard() }, 'Reset'),
      h('button', { type: 'button', class: 'button positive', disabled: saving, onclick: () => save() }, 'Save Changes'));
  };

  const discard = async () => {
    draft = null;
    await ctx.choose(ctx.active);
    draw();
  };

  const save = async (quiet) => {
    if (!draft.name.trim()) { ctx.toast('Give the theme a name.'); throw new Error('no name'); }
    saving = true;
    drawBar();
    try {
      const saved = await ctx.save(draft);
      draft.id = saved.id;
      draft.assets = saved.assets;
      if (!quiet) { ctx.toast('Saved: ' + saved.name + '.'); draft = null; draw(); }
      return saved;
    } catch (e) {
      ctx.toast('Not saved: ' + e);
      throw e;
    } finally {
      saving = false;
      drawBar();
    }
  };

  // A font or background file goes into the theme, which is saved first so it has a folder.
  const attach = async (file, slot) => {
    if (!file) return;
    try {
      edit();
      if (!draft.id) await save(true);
      draft[slot] = await ctx.addAsset(draft.id, file);
      if (slot === 'backgroundAsset') draft.options.background = 'image';
      await save(true);
      preview();
      draw();
    } catch (e) {
      ctx.toast('Couldn’t add that file: ' + e);
    }
  };

  // --- Building blocks ----------------------------------------------------------------------------

  const section = (title, ...children) => {
    const anchor = typeof children[0] === 'string' ? children.shift() : null;
    return h('section', { class: 'settings-section', 'data-anchor': anchor }, title ? h('h2', { class: 'section-title' }, title) : null, ...children);
  };
  const block = (title, description, control) => h('div', { class: 'control' }, h('div', { class: 'control-title' }, title),
    description ? h('div', { class: 'control-description' }, description) : null, control);
  const choice = (name, cls) => radioCards(CHOICES[name].map(([value, label, description]) => ({ value, label, description })), optionOf(name),
    (v) => { setOption(name, v); draw(); }, cls);
  // Steps like 0.025 leave float dust (1.3750000000000002): values are rounded to 3 places.
  const round = (v) => Math.round(v * 1000) / 1000;
  const tokenSlider = (name, title, description, min, max, step, unit, fallback, marks) => slider({
    title, description, min, max, step, marks, value: num(tokenOf(name), fallback),
    format: (v) => (unit === 's' ? round(v) + ' s' : round(v) + unit),
    onInput: (v) => setToken(name, round(v) + (unit === 's' ? '' : unit)),
  });
  const fileButton = (label, accept, slot) => {
    const input = h('input', { type: 'file', accept, hidden: true });
    input.addEventListener('change', () => { const f = input.files[0]; input.value = ''; attach(f, slot); });
    const has = shown()[slot];
    return h('div', { class: 'file-pick' },
      h('button', { type: 'button', class: 'button secondary', onclick: () => input.click() }, has ? 'Replace' : label),
      has ? h('span', { class: 'file-pick-name' }, has) : null,
      has ? h('button', { type: 'button', class: 'button link', onclick: () => { edit()[slot] = ''; if (slot === 'backgroundAsset') draft.options.background = 'none'; preview(); draw(); } }, 'Remove') : null,
      input);
  };
  const colours = (list) => h('div', { class: 'colour-tiles' }, list.map(([name, label]) => colourTile(label, tokenOf(name), (v) => setToken(name, v))));

  // --- Pages ------------------------------------------------------------------------------------

  const appearance = () => {
    // A theme as a small square: its chat colour, its side panel and accent in the corner.
    const card = (theme, selected, onclick) => {
      const t = theme.tokens || {};
      const tile = h('button', { type: 'button', class: 'theme-tile' + (selected ? ' selected' : ''), title: theme.name, 'aria-label': theme.name, onclick },
        h('span', { class: 'theme-tile-side' }), h('span', { class: 'theme-tile-dot' }), selected ? h('span', { class: 'theme-check' }, icon(ICONS.check, 14)) : null);
      tile.style.background = t['--lc-chat-bg'] || '#313338';
      tile.firstChild.style.background = t['--lc-sidebar'] || '#2b2d31';
      tile.children[1].style.background = t['--lc-brand'] || '#5865f2';
      return tile;
    };
    const pickTheme = (t) => { draft = null; Promise.resolve(ctx.choose(t)).then(draw).catch((e) => ctx.toast('Couldn’t load that theme: ' + e)); };
    const activeId = ctx.active.id;
    const mine = ctx.prefs.themes.map((t) => ({ ...t, tokens: t.id === activeId ? ctx.active.tokens : {} }));
    body.appendChild(section('Theme', 'theme',
      h('div', { class: 'control-description' }, 'Pick a base. Change anything below and it becomes a theme of your own, saved with the bar at the bottom.'),
      h('div', { class: 'control-title' }, 'Base themes'),
      h('div', { class: 'theme-tiles' }, PRESETS.map((p) => card(p, !draft && activeId === p.id, () => pickTheme(p)))),
      mine.length ? h('div', { class: 'control-title spaced' }, 'Your themes') : null,
      mine.length ? h('div', { class: 'theme-tiles' }, mine.map((t) => card(t, !draft && activeId === t.id, () => pickTheme(t)))) : null,
      h('div', { class: 'theme-current' }, 'In use: ', h('strong', null, draft ? draft.name + ' (unsaved)' : ctx.active.name))));

    // Roundness: one slider, the three radii follow it (small half, large double).
    const roundness = slider({
      title: 'Roundness', description: 'Corners of panels, cards, buttons and popups.', min: 0, max: 24, step: 1, marks: [0, 4, 8, 12, 16, 24],
      value: num(tokenOf('--lc-radius-m'), 8), format: (v) => v + 'px',
      onInput: (r) => {
        const d = edit();
        d.tokens['--lc-radius-s'] = Math.round(r / 2) + 'px';
        d.tokens['--lc-radius-m'] = r + 'px';
        d.tokens['--lc-radius-l'] = r * 2 + 'px';
        preview();
      },
    });
    const avatarShape = tokenOf('--lc-avatar-radius') || '50%';
    body.appendChild(section('Messages', 'messages',
      previewCard(ctx.me),
      block('Message style', null, choice('messages', 'three')),
      block('Message display', null, choice('density', 'two')),
      block('Scheme', 'Light or dark for controls, scrollbars and pickers.', choice('scheme', 'two')),
      block('Avatar shape', null, radioCards(AVATAR_SHAPES.map(([label, value]) => {
        const sample = h('span', { class: 'shape-sample' });
        sample.style.borderRadius = value;
        return { value, label, visual: sample };
      }), avatarShape, (v) => { setToken('--lc-avatar-radius', v); draw(); }, 'shapes')),
      roundness));

    const font = tokenOf('--lc-font');
    const fontValue = FONTS.some(([, v]) => v === font) ? font : FONTS[0][1];
    body.appendChild(section('Text', 'text',
      block('Font', null, dropdown(FONTS.map(([label, value]) => ({ value, label, style: 'font-family: ' + value })), fontValue, (v) => { setToken('--lc-font', v); draw(); }, 'Font')),
      block('Your own font', 'A WOFF2, WOFF, TTF or OTF file; it goes inside the theme and is used first.', fileButton('Choose a font file', '.woff2,.woff,.ttf,.otf', 'fontAsset')),
      tokenSlider('--lc-font-size', 'Chat font scaling', null, 12, 20, 1, 'px', 16, [12, 14, 15, 16, 18, 20]),
      tokenSlider('--lc-line-height', 'Line height', null, 1.1, 1.8, 0.025, '', 1.375, [1.2, 1.375, 1.6, 1.8])));
    body.appendChild(section('Layout', 'layout',
      tokenSlider('--lc-group-gap', 'Space between message groups', null, 0, 32, 1, 'px', 17, [0, 4, 8, 16, 24, 32]),
      tokenSlider('--lc-avatar-size', 'Avatar size', null, 24, 64, 2, 'px', 40, [24, 32, 40, 48, 56, 64]),
      tokenSlider('--lc-sidebar-width', 'Side panel width', null, 200, 360, 10, 'px', 240, [200, 240, 280, 320, 360]),
      tokenSlider('--lc-members-width', 'Member list width', null, 200, 360, 10, 'px', 240, [200, 240, 280, 320, 360])));
    body.appendChild(section('Colours', 'colours', ...COLOR_GROUPS.map(([group, list]) => block(group, null, colours(list)))));
  };

  const effects = () => {
    body.appendChild(section(null, toggle('Effects', 'Backgrounds, glass, glow and animations. Off turns all of it off at once. They rest while the window is hidden, and follow Windows’ reduce motion.',
      optionOf('effects') === 'on', (on) => { setOption('effects', on ? 'on' : 'off'); draw(); })));
    const bg = optionOf('background');
    body.appendChild(section('Background', 'background',
      choice('background', 'two'),
      bg === 'image' || shown().backgroundAsset ? block('Image', 'PNG, JPEG, GIF or WebP, up to 10 MB; it goes inside the theme.', fileButton('Choose an image', 'image/png,image/jpeg,image/gif,image/webp', 'backgroundAsset')) : null,
      bg === 'gradient' || bg === 'animated' ? block('Gradient colours', null, colours([['--lc-gradient-a', 'First'], ['--lc-gradient-b', 'Middle'], ['--lc-gradient-c', 'Last']])) : null,
      bg === 'animated' ? tokenSlider('--lc-gradient-speed', 'Animation length', 'One full drift, in seconds.', 5, 120, 5, 's', 30, [5, 30, 60, 90, 120]) : null,
      bg !== 'none' ? tokenSlider('--lc-background-dim', 'Dim the background', null, 0, 0.9, 0.05, '', 0.25, [0, 0.25, 0.5, 0.75]) : null));
    body.appendChild(section('Panels', 'panels',
      choice('panels', 'two'),
      tokenSlider('--lc-glass-alpha', 'Panel opacity', 'How solid the panels stay over a background.', 0.2, 1, 0.02, '', 0.72, [0.2, 0.4, 0.6, 0.8, 1]),
      tokenSlider('--lc-glass-blur', 'Glass blur', null, 0, 40, 1, 'px', 16, [0, 8, 16, 24, 32, 40])));
    body.appendChild(section('Accents', 'accents',
      tokenSlider('--lc-glow', 'Glow', 'Around the accent colour: selected items and buttons.', 0, 24, 1, 'px', 0, [0, 6, 12, 18, 24]),
      toggle('Message animations', 'New messages slide in.', optionOf('animations') === 'on', (on) => setOption('animations', on ? 'on' : 'off'))));
  };

  const css = () => {
    body.appendChild(h('div', { class: 'notice' },
      h('div', null, 'Anything goes, applied after everything else. It can’t run code, and LockCord blocks anything it would load from the internet.'),
      h('div', null, 'Secret-chat signals (the lock, the code, the “Secret” bar) can’t be restyled or hidden. If something breaks the window, ', h('strong', null, 'Ctrl+Shift+0'), ' switches to the default theme.')));
    const area = h('textarea', { class: 'css-editor', spellcheck: false, value: shown().css || '', placeholder: '/* e.g. */\n.message { letter-spacing: 0.02em; }', 'aria-label': 'Custom CSS' });
    const size = h('div', { class: 'control-description' }, '');
    const count = () => { size.textContent = (area.value.length / 1024).toFixed(1) + ' KB of 256 KB'; };
    let timer = null;
    area.addEventListener('input', () => {
      count();
      clearTimeout(timer);
      timer = setTimeout(() => { edit().css = area.value; preview(); }, 300);
    });
    // Tab indents instead of leaving the box.
    area.addEventListener('keydown', (e) => {
      if (e.key !== 'Tab') return;
      e.preventDefault();
      const at = area.selectionStart;
      area.value = area.value.slice(0, at) + '  ' + area.value.slice(area.selectionEnd);
      area.setSelectionRange(at + 2, at + 2);
      area.dispatchEvent(new Event('input'));
    });
    count();
    body.appendChild(section(null, area, size));
  };

  const themes = () => {
    body.appendChild(h('div', { class: 'control-description' }, 'A theme travels as one .lctheme file with its images and fonts inside. An imported theme is checked first and added as a new theme; nothing in it can run code.'));
    body.appendChild(section(null, h('button', {
      type: 'button', class: 'button brand', onclick: async () => {
        try {
          const t = await ctx.importTheme();
          if (!t) return;
          ctx.toast('Imported: ' + t.name + '. It’s under Appearance, Your themes.');
          draw();
        } catch (e) {
          ctx.toast('Not imported: ' + e);
        }
      },
    }, 'Import a Theme')));
    if (!ctx.prefs.themes.length) {
      body.appendChild(h('div', { class: 'empty-note' }, 'Your own themes show here once you’ve saved or imported one.'));
      return;
    }
    body.appendChild(section('Your themes', h('div', { class: 'theme-rows' }, ctx.prefs.themes.map((t) => h('div', { class: 'theme-row' },
      h('div', { class: 'theme-row-text' }, h('div', { class: 'theme-row-name' }, t.name), t.author ? h('div', { class: 'control-description' }, 'by ' + t.author) : null),
      h('button', { type: 'button', class: 'button secondary', onclick: () => ctx.exportTheme(t.id).then((done) => done && ctx.toast('Exported.')).catch((e) => ctx.toast('Not exported: ' + e)) }, 'Export'),
      // Deleting takes a second click.
      h('button', { type: 'button', class: 'button danger-outline', onclick: (e) => {
        const b = e.currentTarget;
        if (!b.dataset.armed) { b.dataset.armed = '1'; b.textContent = 'Click again to delete'; return; }
        ctx.remove(t.id).then(() => { if (draft && draft.id === t.id) draft = null; draw(); }).catch((err) => ctx.toast('Not deleted: ' + err));
      } }, 'Delete'))))));
  };

  const purge = () => renderPurge(body, ctx, draw);
  const translation = () => renderTranslation(body, ctx, draw);
  const e2e = () => renderE2e(body, ctx, draw);
  const app = () => renderApp(body, ctx, draw);
  const voice = () => { cleanup = renderVoice(body, ctx, draw); };
  const guard = () => { renderGuard(body, ctx, draw, presetChannel); presetChannel = null; };
  // Rust's purge progress redraws the Purge page while it's open.
  const stopListening = ctx.onPurge(() => { if (page === 'purge') draw(); });

  draw();
  return () => { stopListening(); if (cleanup) cleanup(); };
}
