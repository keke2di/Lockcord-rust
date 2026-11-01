// Themes: each one is a whole look (shapes, frames, controls, icons, motion: its own stylesheet in
// css/skins/, none for the default) with a few colour schemes of its own, light and dark. A scheme
// is what's picked and saved (settings.json `theme`: "builtin:<scheme id>"); its tokens go on
// :root and its theme's stylesheet is linked. Nothing a theme is made of comes from disk or the
// user, so nothing here needs checking. Display settings (density, text size, motion) are the
// user's own and go on top of any theme.

// The themes; `skin` is their stylesheet's name (css/skins/<skin>.css; none for 'default').
export const THEMES = [
  { id: 'default', name: 'LockCord', description: 'Clean and familiar: Discord-like, tidied up.' },
  { id: 'oled', name: 'Midnight Pro', description: 'Near-black or bright, hairline borders, pill buttons, one strong accent.' },
  { id: 'clean', name: 'Blossom', description: 'A soft gradient behind floating cards, big round corners.' },
  { id: 'xp', name: 'Windows XP', description: 'Luna title bar, bevelled buttons, task panes, Tahoma.' },
  { id: 'glass', name: 'Aero Glass', description: 'Vista-style translucent glass with glossy highlights.' },
  { id: 'bulky', name: 'Sakura Retro', description: 'Pastel pixel windows, thick outlines, chunky buttons.' },
  { id: 'minimal', name: 'Minimal', description: 'No chrome: just text, spacing and one accent.' },
  { id: 'contrast', name: 'High Contrast', description: 'Ultra high contrast: black and white, thick outlines, a bright accent, focus always visible.' },
];

// A colour with transparency, from #rrggbb.
const alpha = (hex, a) => 'rgba(' + [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(', ') + ', ' + a + ')';

// The dark recipe: chat, side panel and rail backgrounds, the text, the muted text, an accent.
function dark(id, theme, name, p, tokens = {}) {
  return {
    id: 'builtin:' + id, theme, name, builtin: true, mode: 'dark',
    tokens: {
      '--lc-chat-bg': p.chat, '--lc-sidebar': p.side, '--lc-rail': p.rail, '--lc-deep': p.deep || p.rail, '--lc-panel': p.panel || p.rail,
      '--lc-popout': p.popout || p.side, '--lc-composer': p.composer, '--lc-code-bg': p.side,
      '--lc-header': p.header, '--lc-text': p.text, '--lc-muted': p.muted, '--lc-interactive': p.icons || p.muted,
      '--lc-interactive-hover': p.header, '--lc-brand': p.accent, '--lc-link': p.link || p.accent, '--lc-on-accent': p.onAccent || '#ffffff',
      '--lc-mention': alpha(p.accent, 0.22), '--lc-hover': 'rgba(0, 0, 0, 0.08)', '--lc-border': alpha(p.text, 0.12),
      '--lc-sidebar-hover': alpha(p.text, 0.06), '--lc-sidebar-selected': alpha(p.text, 0.12),
      '--lc-card': alpha(p.text, 0.035), '--lc-bubble': alpha(p.text, 0.07), '--lc-bubble-mine': alpha(p.accent, 0.3),
      ...tokens,
    },
  };
}

// The light recipe.
function light(id, theme, name, p, tokens = {}) {
  return {
    id: 'builtin:' + id, theme, name, builtin: true, mode: 'light',
    tokens: {
      '--lc-chat-bg': p.chat, '--lc-sidebar': p.side, '--lc-rail': p.rail, '--lc-deep': p.deep || p.side, '--lc-panel': p.panel || p.side,
      '--lc-popout': p.popout || p.chat, '--lc-composer': p.composer || p.chat, '--lc-code-bg': p.code || p.side,
      '--lc-header': p.header, '--lc-text': p.text, '--lc-muted': p.muted, '--lc-interactive': p.icons || p.muted,
      '--lc-interactive-hover': p.header, '--lc-brand': p.accent, '--lc-link': p.link || p.accent, '--lc-on-accent': p.onAccent || '#ffffff',
      '--lc-mention': alpha(p.accent, 0.14), '--lc-hover': 'rgba(0, 0, 0, 0.035)', '--lc-border': 'rgba(0, 0, 0, 0.1)',
      '--lc-sidebar-hover': alpha(p.text, 0.06), '--lc-sidebar-selected': alpha(p.text, 0.1),
      '--lc-overlay-soft': 'rgba(0, 0, 0, 0.04)', '--lc-overlay': 'rgba(0, 0, 0, 0.08)',
      '--lc-card': 'rgba(0, 0, 0, 0.03)', '--lc-bubble': 'rgba(0, 0, 0, 0.05)', '--lc-bubble-mine': alpha(p.accent, 0.16),
      '--lc-secret': '#1a7f43', '--lc-positive': '#1a7f43',
      '--lc-shadow-header': '0 1px 0 rgba(6, 6, 7, 0.08)', '--lc-shadow-divider': '0 1px 0 rgba(6, 6, 7, 0.08)',
      '--lc-shadow-popout': '0 8px 16px rgba(0, 0, 0, 0.12)',
      ...tokens,
    },
  };
}

const DISCORD_DARK = { id: 'builtin:dark', theme: 'default', name: 'Dark', builtin: true, mode: 'dark', tokens: {} };
const OLED = { '--lc-positive': '#3ddc84', '--lc-secret': '#3ddc84', '--lc-hover': 'rgba(255, 255, 255, 0.025)' };

export const PRESETS = [
  // LockCord: Discord-like.
  DISCORD_DARK,
  dark('midnight', 'default', 'Midnight', { chat: '#1e1f24', side: '#18191d', rail: '#121316', composer: '#26272d', header: '#f2f3f5', text: '#d4d6db', muted: '#8e929b', accent: '#7b83f6' }),
  dark('ocean', 'default', 'Ocean', { chat: '#1c2433', side: '#171e2b', rail: '#111723', composer: '#242e40', header: '#eef3fb', text: '#d3dceb', muted: '#8b97ad', accent: '#4aa3ff', link: '#6cb8ff' }),
  dark('forest', 'default', 'Forest', { chat: '#1f2622', side: '#1a201c', rail: '#141916', composer: '#28302b', header: '#eef4ee', text: '#d5dfd6', muted: '#8d9b90', accent: '#e0a458', link: '#9fd4a8' }),
  light('light', 'default', 'Light', { chat: '#f4f4f6', side: '#e9eaee', rail: '#dcdee3', deep: '#dcdee3', panel: '#e1e3e8', popout: '#f9f9fb', composer: '#e7e8ec', header: '#1f2125', text: '#33363c', muted: '#686c75', icons: '#565a63', accent: '#5865f2', link: '#1e6fd9' }),
  light('daybreak', 'default', 'Daybreak', { chat: '#ffffff', side: '#f3f6fa', rail: '#e6ecf3', composer: '#eef2f7', header: '#14202e', text: '#2a3747', muted: '#6b7a8c', accent: '#0e8a7e' }),

  // Midnight Pro (css/skins/oled.css).
  dark('oled-ember', 'oled', 'Ember', { chat: '#0c0c0d', side: '#0f0f11', rail: '#0a0a0b', panel: '#0a0a0b', composer: '#141416', popout: '#141416', header: '#f5f5f5', text: '#d6d6d8', muted: '#86868c', icons: '#a8a8ae', accent: '#ff5a1f', link: '#ff8a5c' }, OLED),
  dark('oled-black', 'oled', 'True Black OLED', { chat: '#000000', side: '#000000', rail: '#000000', panel: '#000000', composer: '#0b0b0b', popout: '#0b0b0b', header: '#ffffff', text: '#d9d9d9', muted: '#7d7d82', icons: '#a0a0a6', accent: '#ff5a1f', link: '#ff8a5c' }, { ...OLED, '--lc-skin-glow': '0', '--lc-skin-grain': '0' }),
  dark('oled-steel', 'oled', 'Steel', { chat: '#0d0e10', side: '#101114', rail: '#0a0b0d', panel: '#0a0b0d', composer: '#15171b', popout: '#15171b', header: '#f2f4f8', text: '#d3d7de', muted: '#858b96', icons: '#a6acb7', accent: '#7aa2ff', link: '#9db9ff' }, OLED),
  light('oled-daylight', 'oled', 'Daylight', { chat: '#fbfbfc', side: '#f3f3f5', rail: '#ebebee', header: '#0b0b0c', text: '#2a2a2e', muted: '#77777e', accent: '#ff5a1f', link: '#e2470e' }, { '--lc-skin-glow': '0.6', '--lc-skin-grain': '0.03' }),
  light('oled-porcelain', 'oled', 'Porcelain', { chat: '#ffffff', side: '#f6f7f9', rail: '#eef0f3', header: '#0d1117', text: '#262c36', muted: '#737b88', accent: '#2f6bff' }, { '--lc-skin-glow': '0.5', '--lc-skin-grain': '0.02' }),

  // Blossom (css/skins/clean.css).
  light('clean-blossom', 'clean', 'Blossom', { chat: '#ffffff', side: '#fff8fa', rail: '#ffffff', header: '#2a1a22', text: '#3b2a32', muted: '#8f7a84', accent: '#ec4778' }, { '--lc-gradient-a': '#ffe6d5', '--lc-gradient-b': '#fbcfe1' }),
  light('clean-peach', 'clean', 'Peach', { chat: '#ffffff', side: '#fffaf6', rail: '#ffffff', header: '#2b1d14', text: '#3d2c22', muted: '#93806f', accent: '#f97316' }, { '--lc-gradient-a': '#fff1e3', '--lc-gradient-b': '#ffd7bd' }),
  light('clean-lilac', 'clean', 'Lilac', { chat: '#ffffff', side: '#fbf9ff', rail: '#ffffff', header: '#1f1a2e', text: '#332c45', muted: '#857d99', accent: '#8b5cf6' }, { '--lc-gradient-a': '#f3e8ff', '--lc-gradient-b': '#dcd3ff' }),
  dark('clean-night', 'clean', 'Night Blossom', { chat: '#271a24', side: '#2e1f2b', rail: '#2e1f2b', composer: '#33232f', popout: '#2e1f2b', header: '#fff1f6', text: '#f0dbe5', muted: '#b493a5', accent: '#f472b6' }, { '--lc-gradient-a': '#1d0f19', '--lc-gradient-b': '#3d1838' }),
  dark('clean-plum', 'clean', 'Plum', { chat: '#231b2e', side: '#2a2137', rail: '#2a2137', composer: '#30263f', popout: '#2a2137', header: '#f6efff', text: '#e3d9f2', muted: '#a596bb', accent: '#c084fc' }, { '--lc-gradient-a': '#160f22', '--lc-gradient-b': '#34204a' }),

  // Windows XP (css/skins/xp.css): the Luna schemes, and the dark Royale Noir and Zune.
  light('xp-blue', 'xp', 'Luna Blue', { chat: '#ffffff', side: '#d6dff7', rail: '#245edb', header: '#000000', text: '#000000', muted: '#555555', accent: '#316ac5', link: '#0033cc', code: '#f4f4f4' }),
  light('xp-olive', 'xp', 'Olive Green', { chat: '#ffffff', side: '#e5e9cf', rail: '#8ba169', header: '#000000', text: '#000000', muted: '#555555', accent: '#93a070', link: '#3d5a1e', code: '#f4f4f4' },
    { '--lc-xp-title-a': '#8ba169', '--lc-xp-title-b': '#c3d5a0', '--lc-xp-title-c': '#6f8a4b', '--lc-xp-frame': '#8ba169', '--lc-xp-pane-a': '#c4cc9c', '--lc-xp-pane-b': '#a7b17d', '--lc-xp-select': '#93a070', '--lc-xp-pane-text': '#3d5a1e', '--lc-xp-nav-text': '#3d5a1e' }),
  light('xp-silver', 'xp', 'Silver', { chat: '#ffffff', side: '#e3e3ec', rail: '#a6a6bd', header: '#000000', text: '#000000', muted: '#555555', accent: '#9a9cb0', link: '#3a3d66', code: '#f4f4f4' },
    { '--lc-xp-title-a': '#a5a7b9', '--lc-xp-title-b': '#e6e7ef', '--lc-xp-title-c': '#8e90a3', '--lc-xp-frame': '#a6a6bd', '--lc-xp-pane-a': '#e3e3ec', '--lc-xp-pane-b': '#c8c8d8', '--lc-xp-select': '#9a9cb0', '--lc-xp-pane-text': '#3a3d66', '--lc-xp-nav-text': '#3a3d66' }),
  dark('xp-noir', 'xp', 'Royale Noir', { chat: '#1f1f22', side: '#2a2a2e', rail: '#141416', composer: '#1f1f22', popout: '#26262a', header: '#ffffff', text: '#e4e4e7', muted: '#a0a0a8', accent: '#4f7cc4', link: '#8fb4ff' },
    { '--lc-xp-title-a': '#232326', '--lc-xp-title-b': '#5a5a60', '--lc-xp-title-c': '#0d0d0f', '--lc-xp-frame': '#141416', '--lc-xp-pane-a': '#3a3a40', '--lc-xp-pane-b': '#25252a', '--lc-xp-select': '#3f5f96', '--lc-xp-pane-text': '#b8cdf5', '--lc-xp-face': '#3a3a3f', '--lc-xp-start': '#2f7d14' }),
  dark('xp-zune', 'xp', 'Zune', { chat: '#1b1b1b', side: '#262626', rail: '#101010', composer: '#1b1b1b', popout: '#232323', header: '#ffffff', text: '#e6e6e6', muted: '#a3a3a3', accent: '#f26b1d', link: '#ffa064' },
    { '--lc-xp-title-a': '#262626', '--lc-xp-title-b': '#5c5c5c', '--lc-xp-title-c': '#0f0f0f', '--lc-xp-frame': '#101010', '--lc-xp-pane-a': '#353535', '--lc-xp-pane-b': '#222222', '--lc-xp-select': '#e8650f', '--lc-xp-pane-text': '#ffb27a', '--lc-xp-face': '#3b3b3b', '--lc-xp-start': '#e8650f' }),

  // Aero Glass (css/skins/glass.css): the glass takes the side panel's colour.
  light('glass-clear', 'glass', 'Aero Clear', { chat: '#ffffff', side: '#dbe9f5', rail: '#dbe9f5', header: '#000000', text: '#1b1b1b', muted: '#5d6670', accent: '#2e8bd8', link: '#0066cc' }),
  light('glass-aqua', 'glass', 'Aqua', { chat: '#ffffff', side: '#cdeff0', rail: '#cdeff0', header: '#002a2c', text: '#14302f', muted: '#567070', accent: '#11a3a8', link: '#08797d' }),
  dark('glass-dark', 'glass', 'Aero Dark', { chat: '#18212c', side: '#0d141c', rail: '#0d141c', composer: '#202b38', popout: '#1b2531', header: '#ffffff', text: '#e3ebf3', muted: '#93a3b5', accent: '#4aa8ff', link: '#7cc1ff' }, { '--lc-glass': '0.45' }),
  dark('glass-graphite', 'glass', 'Graphite', { chat: '#1d2024', side: '#121417', rail: '#121417', composer: '#25292e', popout: '#1f2226', header: '#ffffff', text: '#dfe3e8', muted: '#99a1ab', accent: '#9aa7b8', link: '#c3cdd9', onAccent: '#121417' }, { '--lc-glass': '0.5' }),

  // Sakura Retro (css/skins/bulky.css): pastels with an ink outline.
  light('bulky-sakura', 'bulky', 'Sakura', { chat: '#fff7fd', side: '#fde4f6', rail: '#fde4f6', header: '#3d2652', text: '#4a3060', muted: '#8a6fa0', accent: '#ffb3d9', onAccent: '#3d2652' }, { '--lc-ink': '#5a3a78', '--lc-gradient-a': '#ffe2c6', '--lc-gradient-b': '#ffc6ec' }),
  light('bulky-mint', 'bulky', 'Mint', { chat: '#f5fffa', side: '#d9f7eb', rail: '#d9f7eb', header: '#1f3f3a', text: '#2a4a45', muted: '#5f8781', accent: '#9fe8c8', onAccent: '#1f3f3a' }, { '--lc-ink': '#2f5b54', '--lc-gradient-a': '#e3fff3', '--lc-gradient-b': '#c8eeff' }),
  light('bulky-sunset', 'bulky', 'Sunset', { chat: '#fff8f0', side: '#ffe3cb', rail: '#ffe3cb', header: '#4a2340', text: '#56304b', muted: '#946b83', accent: '#ffb38a', onAccent: '#4a2340' }, { '--lc-ink': '#6b3557', '--lc-gradient-a': '#ffd6a5', '--lc-gradient-b': '#e9b8ff' }),
  dark('bulky-night', 'bulky', 'Sakura Night', { chat: '#2a2038', side: '#33264a', rail: '#33264a', composer: '#2a2038', popout: '#2a2038', header: '#ffe8f7', text: '#f3dcf0', muted: '#b9a0c8', accent: '#ff9bd0', onAccent: '#2a2038' }, { '--lc-ink': '#f0c8ff', '--lc-gradient-a': '#1f1630', '--lc-gradient-b': '#3a1f45' }),
  dark('bulky-arcade', 'bulky', 'Arcade', { chat: '#17172b', side: '#202042', rail: '#202042', composer: '#17172b', popout: '#17172b', header: '#f4fdff', text: '#dff6ff', muted: '#8fa6c4', accent: '#7df9ff', onAccent: '#17172b' }, { '--lc-ink': '#ff6be6', '--lc-gradient-a': '#0f0f24', '--lc-gradient-b': '#2a0f3d' }),

  // Minimal (css/skins/minimal.css): one surface, one accent.
  light('minimal-paper', 'minimal', 'Paper', { chat: '#fafafa', side: '#fafafa', rail: '#fafafa', header: '#000000', text: '#222222', muted: '#777777', accent: '#111111' }),
  light('minimal-sepia', 'minimal', 'Sepia', { chat: '#f4ecd8', side: '#f4ecd8', rail: '#f4ecd8', header: '#2b2116', text: '#3b3024', muted: '#86765f', accent: '#7a4b1e' }),
  dark('minimal-ink', 'minimal', 'Ink', { chat: '#111111', side: '#111111', rail: '#111111', composer: '#111111', popout: '#111111', header: '#ffffff', text: '#dddddd', muted: '#888888', accent: '#f2f2f2', link: '#ffffff', onAccent: '#111111' }),
  dark('minimal-graphite', 'minimal', 'Graphite', { chat: '#1c1c1e', side: '#1c1c1e', rail: '#1c1c1e', composer: '#1c1c1e', popout: '#1c1c1e', header: '#f5f5f7', text: '#d1d1d6', muted: '#8e8e93', accent: '#0a84ff' }),
  // High Contrast (css/skins/contrast.css): black, white and one bright accent.
  dark('contrast-dark', 'contrast', 'Ultra Dark', { chat: '#000000', side: '#000000', rail: '#000000', deep: '#000000', panel: '#000000', composer: '#000000', popout: '#000000', header: '#ffffff', text: '#ffffff', muted: '#e6e6e6', icons: '#ffffff', accent: '#ffd400', link: '#00e5ff', onAccent: '#000000' },
    { '--lc-border': '#ffffff', '--lc-hover': 'rgba(255, 255, 255, 0.14)', '--lc-sidebar-hover': 'rgba(255, 255, 255, 0.14)', '--lc-sidebar-selected': '#ffd400', '--lc-mention': 'rgba(255, 212, 0, 0.35)', '--lc-positive': '#00ff7f', '--lc-secret': '#00ff7f', '--lc-danger': '#ff5c5c', '--lc-warning': '#ffd400', '--lc-interactive-hover': '#ffd400' }),
  dark('contrast-cyan', 'contrast', 'Ultra Dark Cyan', { chat: '#000000', side: '#000000', rail: '#000000', deep: '#000000', panel: '#000000', composer: '#000000', popout: '#000000', header: '#ffffff', text: '#ffffff', muted: '#e6e6e6', icons: '#ffffff', accent: '#00e5ff', link: '#ffd400', onAccent: '#000000' },
    { '--lc-border': '#ffffff', '--lc-hover': 'rgba(255, 255, 255, 0.14)', '--lc-sidebar-hover': 'rgba(255, 255, 255, 0.14)', '--lc-sidebar-selected': '#00e5ff', '--lc-mention': 'rgba(0, 229, 255, 0.35)', '--lc-positive': '#00ff7f', '--lc-secret': '#00ff7f', '--lc-danger': '#ff5c5c', '--lc-warning': '#ffd400', '--lc-interactive-hover': '#00e5ff' }),
];

export function preset(id) {
  return PRESETS.find((p) => p.id === id) || DISCORD_DARK;
}

export function schemesOf(theme) {
  return PRESETS.filter((p) => p.theme === theme);
}

const tokenSheet = new CSSStyleSheet();
const displaySheet = new CSSStyleSheet();
document.adoptedStyleSheets = [...document.adoptedStyleSheets, tokenSheet, displaySheet];

// The active theme's stylesheet, after app.css (and before the scheme's tokens, which win).
const skinLink = document.createElement('link');
skinLink.rel = 'stylesheet';
skinLink.id = 'lc-skin-sheet';
document.head.appendChild(skinLink);

let current = DISCORD_DARK;

export function currentTheme() {
  return current;
}

// Applies a colour scheme, and with it its theme. `safe` applies the default.
export function applyTheme(scheme, safe) {
  current = safe || !scheme ? DISCORD_DARK : scheme;
  const lines = Object.entries(current.tokens || {}).map(([name, value]) => name + ': ' + value + ';');
  lines.push('color-scheme: ' + current.mode + ';');
  tokenSheet.replaceSync(':root { ' + lines.join(' ') + ' }');
  const theme = THEMES.some((t) => t.id === current.theme) ? current.theme : 'default';
  if (theme === 'default') skinLink.removeAttribute('href');
  else skinLink.setAttribute('href', 'css/skins/' + theme + '.css');
  const root = document.documentElement;
  for (const t of THEMES) root.classList.toggle('lc-skin-' + t.id, t.id === theme);
  root.classList.toggle('lc-scheme-light', current.mode === 'light');
  root.classList.toggle('lc-scheme-dark', current.mode !== 'light');
}

// The user's display settings (settings.json `display`), over any theme.
export function applyDisplay(display) {
  const d = display || {};
  const size = Math.min(20, Math.max(12, Number(d.textSize) || 16));
  displaySheet.replaceSync(':root { --lc-text-scale: ' + (size / 16) + ';' + (d.motion === false ? ' --lc-motion: 0;' : '') + ' }');
  document.documentElement.classList.toggle('lc-density-compact', d.density === 'compact');
  document.documentElement.classList.toggle('lc-density-cozy', d.density !== 'compact');
}
