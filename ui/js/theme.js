// Themes: a theme's tokens go on :root, its options become classes on <html> (lc-<option>-<value>),
// its background image comes from its own files, and its custom CSS goes last. All of it through
// constructable stylesheets: the CSP's style-src 'self' allows the CSSOM, not inline <style>.
// Themes the user saved were checked by Rust (settings.rs); values typed into the editor are
// checked here the same way before they're previewed.

const tauri = window.__TAURI__;

// The options and their values; the first is the default (settings.rs OPTIONS).
export const OPTIONS = {
  scheme: ['dark', 'light'],
  density: ['cozy', 'compact'],
  messages: ['flat', 'cards', 'bubbles'],
  panels: ['solid', 'glass'],
  background: ['none', 'image', 'gradient', 'animated'],
  effects: ['on', 'off'],
  animations: ['on', 'off'],
};

// A colour with transparency, from #rrggbb.
const alpha = (hex, a) => 'rgba(' + [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(', ') + ', ' + a + ')';

// One recipe for every dark preset, so they share Discord Dark's contrast: three backgrounds
// (chat, side panel, rail), the text, the muted text and an accent; the rest follows from them.
function dark(id, name, p, extra = {}) {
  return {
    id: 'builtin:' + id, name, builtin: true, options: extra.options || {},
    tokens: {
      '--lc-chat-bg': p.chat, '--lc-sidebar': p.side, '--lc-rail': p.rail, '--lc-deep': p.rail, '--lc-panel': p.panel || p.rail,
      '--lc-popout': p.side, '--lc-composer': p.composer, '--lc-code-bg': p.side,
      '--lc-header': p.header, '--lc-text': p.text, '--lc-muted': p.muted, '--lc-interactive': p.icons || p.muted,
      '--lc-interactive-hover': p.header, '--lc-brand': p.accent, '--lc-link': p.link || p.accent,
      '--lc-mention': alpha(p.accent, 0.22), '--lc-hover': 'rgba(0, 0, 0, 0.08)', '--lc-border': alpha(p.text, 0.12),
      '--lc-sidebar-hover': alpha(p.text, 0.06), '--lc-sidebar-selected': alpha(p.text, 0.12),
      '--lc-card': alpha(p.text, 0.035), '--lc-bubble': alpha(p.text, 0.07), '--lc-bubble-mine': alpha(p.accent, 0.3),
      ...(extra.tokens || {}),
    },
  };
}

const DISCORD_DARK = { id: 'builtin:dark', name: 'Discord Dark', builtin: true, tokens: {}, options: {} };

export const PRESETS = [
  DISCORD_DARK,
  dark('midnight', 'Midnight', {
    chat: '#1e1f24', side: '#18191d', rail: '#121316', composer: '#26272d',
    header: '#f2f3f5', text: '#d4d6db', muted: '#8e929b', accent: '#7b83f6',
  }),
  {
    id: 'builtin:light', name: 'Soft Light', builtin: true,
    options: { scheme: 'light' },
    tokens: {
      '--lc-chat-bg': '#f4f4f6', '--lc-sidebar': '#e9eaee', '--lc-rail': '#dcdee3', '--lc-deep': '#dcdee3', '--lc-panel': '#e1e3e8',
      '--lc-popout': '#f9f9fb', '--lc-composer': '#e7e8ec', '--lc-code-bg': '#e7e8ec',
      '--lc-header': '#1f2125', '--lc-text': '#33363c', '--lc-muted': '#686c75', '--lc-interactive': '#565a63',
      '--lc-interactive-hover': '#1f2125', '--lc-brand': '#5865f2', '--lc-link': '#1e6fd9',
      '--lc-hover': 'rgba(6, 6, 7, 0.035)', '--lc-border': 'rgba(6, 6, 7, 0.1)', '--lc-mention': 'rgba(88, 101, 242, 0.14)',
      '--lc-sidebar-hover': 'rgba(6, 6, 7, 0.05)', '--lc-sidebar-selected': 'rgba(6, 6, 7, 0.1)',
      '--lc-overlay-soft': 'rgba(0, 0, 0, 0.04)', '--lc-overlay': 'rgba(0, 0, 0, 0.08)',
      '--lc-card': 'rgba(0, 0, 0, 0.03)', '--lc-bubble': 'rgba(0, 0, 0, 0.05)', '--lc-bubble-mine': 'rgba(88, 101, 242, 0.16)',
      '--lc-secret': '#1a7f43', '--lc-positive': '#1a7f43',
      '--lc-shadow-header': '0 1px 0 rgba(6, 6, 7, 0.08)', '--lc-shadow-divider': '0 1px 0 rgba(6, 6, 7, 0.08)',
      '--lc-shadow-popout': '0 8px 16px rgba(0, 0, 0, 0.12)',
    },
  },
  dark('rose', 'Rosé', {
    chat: '#2b2028', side: '#241a21', rail: '#1c141a', composer: '#352832',
    header: '#fbeaf2', text: '#ecd6e1', muted: '#b08ea0', icons: '#c9a5b7', accent: '#f472b6', link: '#f9a8d4',
  }, { tokens: { '--lc-avatar-radius': '30%' } }),
  dark('ocean', 'Ocean', {
    chat: '#1c2433', side: '#171e2b', rail: '#111723', composer: '#242e40',
    header: '#eef3fb', text: '#d3dceb', muted: '#8b97ad', accent: '#4aa3ff', link: '#6cb8ff',
  }),
  dark('forest', 'Forest', {
    chat: '#1f2622', side: '#1a201c', rail: '#141916', composer: '#28302b',
    header: '#eef4ee', text: '#d5dfd6', muted: '#8d9b90', accent: '#e0a458', link: '#9fd4a8',
  }),
  dark('espresso', 'Espresso', {
    chat: '#29231f', side: '#221d1a', rail: '#1b1714', composer: '#332c27',
    header: '#f7eee6', text: '#e5d8cb', muted: '#a39282', accent: '#d4a373', link: '#e8c39e',
  }),
  dark('neon', 'Midnight Neon', {
    chat: '#0e0e1f', side: '#121229', rail: '#0a0a1c', composer: '#1b1b3c',
    header: '#ffffff', text: '#e6e6ff', muted: '#8f8fbd', icons: '#b8b8e6', accent: '#ff2bd6', link: '#00e5ff',
  }, {
    options: { messages: 'cards', panels: 'glass', background: 'animated', animations: 'on' },
    tokens: {
      '--lc-secret': '#39ff88', '--lc-positive': '#39ff88', '--lc-radius-m': '12px', '--lc-radius-l': '20px',
      '--lc-shadow-popout': '0 0 24px rgba(255, 43, 214, 0.35)',
      '--lc-gradient-a': '#12002b', '--lc-gradient-b': '#3b0a5c', '--lc-gradient-c': '#002a3a', '--lc-gradient-speed': '40',
      '--lc-background-dim': '0.1', '--lc-glass-alpha': '0.6', '--lc-glass-blur': '18px', '--lc-glow': '10px',
    },
  }),
];

// Effects rest while the window is hidden (an animated background costs nothing then).
document.addEventListener('visibilitychange', () => document.documentElement.classList.toggle('lc-paused', document.hidden));

export function preset(id) {
  return PRESETS.find((p) => p.id === id) || DISCORD_DARK;
}

// The same rule as settings.rs: nothing that could end the declaration, open a block, load
// something or escape. Rust checks each kind more closely when a theme is saved.
function safeValue(v) {
  return typeof v === 'string' && v.length <= 300 && !/[;{}\\@<>!]|url\(|\/\*|expression|var\(/i.test(v);
}

const tokenSheet = new CSSStyleSheet();
const userSheet = new CSSStyleSheet();
document.adoptedStyleSheets = [...document.adoptedStyleSheets, tokenSheet, userSheet];

// A theme file's asset, as the window may load it (settings.rs serves lctheme:).
export function assetUrl(themeId, name) {
  return tauri.core.convertFileSrc(themeId + '/' + name, 'lctheme');
}

let current = DISCORD_DARK;

export function currentTheme() {
  return current;
}

// Applies a theme: a preset, a saved theme, or the editor's draft. `safe` applies the default.
export function applyTheme(theme, safe) {
  current = safe ? DISCORD_DARK : theme || DISCORD_DARK;
  const lines = [];
  for (const [name, value] of Object.entries(current.tokens || {})) {
    if (/^--lc-[a-z-]+$/.test(name) && safeValue(value)) lines.push(name + ': ' + value.trim() + ';');
  }
  const options = current.options || {};
  if (current.backgroundAsset && current.id && !current.builtin) {
    lines.push('--lc-background-image: url("' + assetUrl(current.id, current.backgroundAsset) + '");');
  }
  lines.push('color-scheme: ' + (options.scheme === 'light' ? 'light' : 'dark') + ';');
  // The theme's own font file, under a name of its own, ahead of the theme's font list.
  let fontFace = '';
  if (current.fontAsset && current.id && !current.builtin) {
    fontFace = '@font-face { font-family: "LockCord Theme Font"; src: url("' + assetUrl(current.id, current.fontAsset) + '"); font-display: swap; } ';
    const list = safeValue((current.tokens || {})['--lc-font']) ? current.tokens['--lc-font'] : "'gg sans', 'Segoe UI', sans-serif";
    lines.push('--lc-font: "LockCord Theme Font", ' + list + ';');
  }
  tokenSheet.replaceSync(fontFace + ':root { ' + lines.join(' ') + ' }');

  const root = document.documentElement;
  for (const [option, values] of Object.entries(OPTIONS)) {
    const value = values.includes(options[option]) ? options[option] : values[0];
    for (const v of values) root.classList.toggle('lc-' + option + '-' + v, v === value);
  }
  // Custom CSS can't run code, and the CSP blocks anything it would load from outside.
  try {
    userSheet.replaceSync(safe ? '' : current.css || '');
  } catch (e) {
    userSheet.replaceSync('');
    console.warn('custom CSS', e);
  }
}
