// The Sakura Retro garden (css/skins/bulky.css): its blossoms, petals, hearts, sparkles, bows,
// branches and butterflies. The pictures are small SVGs drawn here in the active scheme's colours
// and handed to the stylesheet as --lc-art-* tokens (data: URLs, which the CSP allows; nothing is
// loaded). The moving parts live in two fixed layers that never take a click: falling petals and a
// scattered meadow behind the panes, and butterflies, rising hearts, a sparkle trail, bursts when a
// message goes and blooms when a chat opens in front. All of it stops with the Animations setting,
// under reduced motion and while the window is hidden, and goes away with the theme.

// --- The pictures ---------------------------------------------------------------------------------

const svg = (viewBox, body) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">${body}</svg>`;
const url = (s) => 'url("data:image/svg+xml,' + encodeURIComponent(s) + '")';
const at = (x, y, s = 1, r = 0, body = '') => `<g transform="translate(${x} ${y}) rotate(${r}) scale(${s})">${body}</g>`;
const radial = (id, inner, outer, r, cx = 0, cy = 0) =>
  `<radialGradient id="${id}" cx="${cx}" cy="${cy}" r="${r}" gradientUnits="userSpaceOnUse"><stop offset="0.12" stop-color="${inner}"/><stop offset="0.62" stop-color="${outer}"/></radialGradient>`;

// A scheme's garden colours (theme.js sets them on each Sakura Retro scheme).
function colours(t) {
  const k = (name, fallback) => t[name] || fallback;
  const b = k('--lc-flower-b', '#ff86bf');
  return {
    ink: k('--lc-ink', '#5a3a78'), a: k('--lc-flower-a', '#ffe1ef'), b, c: k('--lc-flower-c', '#ffd65c'),
    leaf: k('--lc-leaf', '#8fd6a0'), bark: k('--lc-bark', '#8c5a74'), wingA: k('--lc-wing-a', '#ffb3dc'),
    wingB: k('--lc-wing-b', '#c79bff'), heart: k('--lc-heart', b), spark: k('--lc-sparkle', '#ffd65c'),
  };
}

// A sakura blossom round the origin, about 44 across: five notched petals, stamens, a centre.
const PETAL = 'M0 -6C-15 -12 -24 -30 -13 -43L0 -36L13 -43C24 -30 15 -12 0 -6Z';
function blossom(k, w = 3) {
  let petals = '';
  let stamens = '';
  for (let i = 0; i < 5; i++) {
    petals += `<path d="${PETAL}" transform="rotate(${i * 72})"/>`;
    const r = (i * 72 + 36) * Math.PI / 180;
    const x = (Math.sin(r) * 16).toFixed(1);
    const y = (-Math.cos(r) * 16).toFixed(1);
    stamens += `<path d="M0 0L${x} ${y}"/><circle cx="${x}" cy="${y}" r="2.8"/>`;
  }
  return `<g fill="url(#bl)" stroke="${k.ink}" stroke-width="${w}" stroke-linejoin="round">${petals}</g>`
    + `<g stroke="${k.b}" stroke-width="2" fill="${k.c}">${stamens}</g><circle r="7" fill="${k.c}" stroke="${k.ink}" stroke-width="${w * 0.6}"/>`;
}
const blossomDefs = (k) => radial('bl', k.b, k.a, 46);

// A round daisy-like flower, for variety.
function daisy(k, w = 3) {
  let petals = '';
  for (let i = 0; i < 8; i++) petals += `<ellipse cy="-25" rx="10" ry="19" transform="rotate(${i * 45})"/>`;
  return `<g fill="${k.wingA}" stroke="${k.ink}" stroke-width="${w}">${petals}</g><circle r="12" fill="${k.c}" stroke="${k.ink}" stroke-width="${w}"/>`
    + `<g fill="${k.b}" opacity="0.55"><circle cx="-4" cy="-3" r="2"/><circle cx="4" cy="-1" r="2"/><circle cx="0" cy="5" r="2"/></g>`;
}

const HEART = 'M16 28C6 20 1 14 1 8.5C1 4 4.5 1 8.5 1C12 1 14.5 3.5 16 6C17.5 3.5 20 1 23.5 1C27.5 1 31 4 31 8.5C31 14 26 20 16 28Z';
const heart = (k, w = 2) => `<g transform="translate(-16 -15)"><path d="${HEART}" fill="${k.heart}" stroke="${k.ink}" stroke-width="${w}" stroke-linejoin="round"/>`
  + `<ellipse cx="9" cy="8" rx="3.2" ry="2.2" fill="#fff" opacity="0.75" transform="rotate(-30 9 8)"/></g>`;

const STAR = 'M0 -10Q1.3 -1.3 10 0Q1.3 1.3 0 10Q-1.3 1.3 -10 0Q-1.3 -1.3 0 -10Z';
const sparkle = (k, w = 1.2) => `<path d="${STAR}" fill="${k.spark}" stroke="${k.ink}" stroke-width="${w}" stroke-linejoin="round"/><path d="${STAR}" fill="#fff" transform="scale(0.4)"/>`;

// One loose petal round the origin.
const petal = (k) => `<path d="${PETAL}" transform="translate(0 24)" fill="url(#pt)" stroke="${k.b}" stroke-width="1.5" stroke-linejoin="round"/>`;
const petalDefs = (k) => radial('pt', k.b, k.a, 40, 0, 18);

const leaf = (k) => `<path d="M-10 0C-5 -6 5 -6 10 0C5 6 -5 6 -10 0Z" fill="${k.leaf}" stroke="${k.ink}" stroke-width="1.4"/><path d="M-8 0H8" stroke="${k.ink}" stroke-width="0.8" opacity="0.5"/>`;

// A stem: an ink outline under the bark colour, like the theme's outlined boxes.
const stem = (k, d, w) => `<path d="${d}" fill="none" stroke="${k.ink}" stroke-width="${w + 2.4}" stroke-linecap="round"/><path d="${d}" fill="none" stroke="${k.bark}" stroke-width="${w}" stroke-linecap="round"/>`;
const bud = (k, x, y, r = 3) => `<circle cx="${x}" cy="${y}" r="${r}" fill="${k.b}" stroke="${k.ink}" stroke-width="1"/>`;

// A butterfly's wings (left; the right one is its mirror), and its body.
const wings = (k) => `<defs>${radial('w', k.wingB, k.wingA, 40, 40, 32)}</defs>`
  + `<g fill="url(#w)" stroke="${k.ink}" stroke-width="2.4" stroke-linejoin="round"><path d="M39 30C30 4 4 -1 2 17C1 30 16 34 39 32Z"/><path d="M39 34C22 36 9 46 14 58C20 66 34 54 39 38Z"/></g>`
  + `<g fill="#fff" opacity="0.85"><circle cx="13" cy="15" r="4"/><circle cx="23" cy="11" r="2.4"/><circle cx="20" cy="50" r="3"/></g><circle cx="28" cy="22" r="2.6" fill="${k.heart}"/>`;
const body = (k) => `<g stroke="${k.ink}" stroke-width="1.4" fill="none" stroke-linecap="round"><path d="M5 16Q2 7 1 3"/><path d="M7 16Q10 7 11 3"/></g>`
  + `<g fill="${k.ink}"><circle cx="1.2" cy="3" r="1.4"/><circle cx="10.8" cy="3" r="1.4"/><circle cx="6" cy="18" r="3.6"/><ellipse cx="6" cy="36" rx="3.2" ry="15"/></g>`;

const bow = (k) => `<g fill="${k.heart}" stroke="${k.ink}" stroke-width="2.2" stroke-linejoin="round"><path d="M28 21L19 38L25 36L29 24Z"/><path d="M32 21L41 38L35 36L31 24Z"/>`
  + `<path d="M30 18C18 2 2 4 4 18C6 32 20 30 30 18Z"/><path d="M30 18C42 2 58 4 56 18C54 32 40 30 30 18Z"/><rect x="25" y="12" width="10" height="12" rx="4"/></g>`
  + `<g fill="#fff" opacity="0.6"><ellipse cx="12" cy="13" rx="4" ry="2.4" transform="rotate(-25 12 13)"/><ellipse cx="48" cy="13" rx="4" ry="2.4" transform="rotate(25 48 13)"/></g>`
  + `<g fill="none" stroke="${k.ink}" stroke-width="1" opacity="0.45"><path d="M10 18C15 16 21 17 26 19"/><path d="M50 18C45 16 39 17 34 19"/></g>`;

// A garland that tiles sideways (120 wide): a wavy stem, leaves, little blossoms and a bud.
function vine(k) {
  const d = 'M0 11C20 3 40 19 60 11C80 3 100 19 120 11';
  return `<defs>${blossomDefs(k)}</defs><path d="${d}" fill="none" stroke="${k.ink}" stroke-width="4"/><path d="${d}" fill="none" stroke="${k.leaf}" stroke-width="2"/>`
    + at(14, 7, 0.6, -30, leaf(k)) + at(46, 15, 0.6, 25, leaf(k)) + at(75, 7, 0.6, -25, leaf(k)) + at(105, 15, 0.6, 30, leaf(k))
    + at(30, 11, 0.19, 0, blossom(k, 6)) + at(90, 11, 0.17, 20, blossom(k, 6)) + bud(k, 60, 11, 2.6);
}

// A flowering branch across the title bar (260 × 38).
function titleBranch(k) {
  return `<defs>${blossomDefs(k)}</defs>`
    + stem(k, 'M262 4C220 10 180 26 120 22C80 19 40 30 6 24', 3) + stem(k, 'M182 18C176 9 166 5 154 7', 2) + stem(k, 'M100 21C96 28 90 32 82 32', 2)
    + at(222, 12, 0.5, -20, leaf(k)) + at(64, 26, 0.5, 15, leaf(k)) + at(140, 25, 0.45, 30, leaf(k))
    + bud(k, 236, 8, 2.4) + bud(k, 58, 21, 2.2) + bud(k, 168, 26, 2.2)
    + at(152, 8, 0.17, 10, blossom(k, 6)) + at(118, 21, 0.2, -10, blossom(k, 6)) + at(82, 31, 0.13, 30, blossom(k, 7))
    + at(38, 26, 0.18, 0, blossom(k, 6)) + at(202, 15, 0.16, 25, blossom(k, 6)) + at(10, 24, 0.12, 0, daisy(k, 7));
}

// A big soft branch for the corner behind the messages (300 × 240), faint.
function cornerBranch(k) {
  return `<defs>${blossomDefs(k)}</defs><g opacity="0.4">`
    + stem(k, 'M305 205C250 172 210 152 150 150C110 149 82 130 60 100', 5) + stem(k, 'M210 160C205 130 215 106 236 90', 3.5)
    + stem(k, 'M150 150C140 174 120 189 96 194', 3) + stem(k, 'M100 132C95 110 82 94 62 86', 2.5)
    + at(190, 156, 1.1, -20, leaf(k)) + at(112, 138, 1, 40, leaf(k)) + at(250, 118, 1, -60, leaf(k)) + at(78, 116, 0.9, 70, leaf(k))
    + bud(k, 200, 168, 4) + bud(k, 80, 140, 3.5) + bud(k, 132, 180, 3.5) + bud(k, 270, 175, 3.5)
    + at(60, 98, 0.42, 0, blossom(k)) + at(236, 88, 0.38, 20, blossom(k)) + at(96, 194, 0.34, -15, blossom(k))
    + at(168, 150, 0.28, 30, blossom(k)) + at(62, 84, 0.2, 10, daisy(k, 4)) + '</g>';
}

// A blossom branch for a card's corner (120 × 96), full strength.
function cardBranch(k) {
  return `<defs>${blossomDefs(k)}</defs>`
    + stem(k, 'M124 2C100 10 76 26 52 30C36 33 22 44 14 58', 3) + stem(k, 'M78 24C80 40 74 54 62 64', 2.4) + stem(k, 'M48 31C40 20 30 16 20 16', 2)
    + at(98, 14, 0.7, -30, leaf(k)) + at(70, 46, 0.6, 50, leaf(k)) + bud(k, 110, 8, 2.8) + bud(k, 30, 46, 2.6)
    + at(20, 18, 0.24, 0, blossom(k, 5)) + at(62, 66, 0.26, 25, blossom(k, 5)) + at(14, 60, 0.2, -10, blossom(k, 5)) + at(52, 30, 0.18, 10, daisy(k, 5));
}

// Loose petals for the falling layer, tiling 300 × 300 (nothing crosses the edges).
function petalTile(k) {
  const spots = [[30, 40, 20, 0.42], [120, 92, -35, 0.34], [222, 30, 60, 0.48], [262, 170, -10, 0.38], [72, 200, 110, 0.46], [170, 250, 30, 0.3], [24, 272, -60, 0.36], [152, 160, 80, 0.28]];
  return `<defs>${petalDefs(k)}${blossomDefs(k)}</defs>` + spots.map(([x, y, r, s]) => at(x, y, s, r, petal(k))).join('')
    + at(200, 112, 0.2, 15, blossom(k, 6)) + at(96, 26, 0.14, -20, blossom(k, 7));
}

// The meadow scattered over the backdrop, tiling 360 × 360.
function meadowTile(k) {
  return `<defs>${blossomDefs(k)}</defs><g opacity="0.7">`
    + at(60, 70, 0.28, 10, blossom(k, 5)) + at(250, 140, 0.22, -20, blossom(k, 5)) + at(130, 284, 0.25, 30, blossom(k, 5))
    + at(300, 300, 0.2, 0, daisy(k, 5)) + at(200, 40, 0.16, 15, daisy(k, 6))
    + at(330, 64, 0.4, -12, heart(k)) + at(40, 200, 0.35, 10, heart(k)) + at(222, 224, 0.3, -6, heart(k))
    + at(152, 122, 0.6, 0, sparkle(k)) + at(92, 330, 0.5, 0, sparkle(k)) + at(320, 200, 0.7, 0, sparkle(k)) + at(270, 30, 0.5, 0, sparkle(k))
    + `<g fill="${k.spark}"><circle cx="20" cy="130" r="1.6"/><circle cx="180" cy="190" r="1.4"/><circle cx="300" cy="110" r="1.6"/><circle cx="110" cy="20" r="1.3"/><circle cx="240" cy="330" r="1.5"/></g></g>`;
}

// The --lc-art-* tokens for a scheme's tokens.
export function gardenTokens(tokens) {
  const k = colours(tokens || {});
  const flower = svg('-50 -50 100 100', `<defs>${blossomDefs(k)}</defs>${blossom(k, 4)}`);
  return {
    '--lc-art-blossom': url(flower),
    '--lc-art-daisy': url(svg('-50 -50 100 100', daisy(k, 4))),
    '--lc-art-heart': url(svg('-17 -16 34 32', heart(k))),
    '--lc-art-sparkle': url(svg('-11 -11 22 22', sparkle(k))),
    '--lc-art-petal': url(svg('-22 -22 44 44', `<defs>${petalDefs(k)}</defs>${petal(k)}`)),
    '--lc-art-wing-l': url(svg('0 0 40 64', wings(k))),
    '--lc-art-wing-r': url(svg('0 0 40 64', `<g transform="translate(40 0) scale(-1 1)">${wings(k)}</g>`)),
    '--lc-art-body': url(svg('0 0 12 64', body(k))),
    '--lc-art-bow': url(svg('0 0 60 40', bow(k))),
    '--lc-art-vine': url(svg('0 0 120 22', vine(k))),
    '--lc-art-title-branch': url(svg('0 0 260 38', titleBranch(k))),
    '--lc-art-corner-branch': url(svg('0 0 300 240', cornerBranch(k))),
    '--lc-art-card-branch': url(svg('0 0 120 96', cardBranch(k))),
    '--lc-art-petals': url(svg('0 0 300 300', petalTile(k))),
    '--lc-art-meadow': url(svg('0 0 360 360', meadowTile(k))),
  };
}

// --- The moving garden ----------------------------------------------------------------------------

const MAX_SPRITES = 40;
const reduce = window.matchMedia('(prefers-reduced-motion: reduce)');
const html = document.documentElement;
const rand = (a, b) => a + Math.random() * (b - a);
const pick = (list) => list[Math.floor(Math.random() * list.length)];
const div = (cls) => { const d = document.createElement('div'); d.className = cls; return d; };

let on = false;
let motion = true;
let back = null;
let front = null;
let events = null;
let timers = [];
let flies = [];
let trailAt = 0;
let trailX = 0;
let trailY = 0;

const moving = () => on && motion && !reduce.matches && !document.hidden;

// One short-lived picture in the front layer, gone when its animation ends.
function sprite(kind, x, y, size, vars) {
  if (!front || front.childElementCount >= MAX_SPRITES + flies.length) return;
  const s = div('lc-sprite ' + kind);
  s.style.left = (x - size / 2) + 'px';
  s.style.top = (y - size / 2) + 'px';
  s.style.width = s.style.height = size + 'px';
  for (const [name, value] of Object.entries(vars || {})) s.style.setProperty(name, value);
  s.addEventListener('animationend', () => s.remove(), { once: true });
  front.appendChild(s);
}

// Hearts, petals and sparkles flying out of a point.
function burst(x, y, count = 10) {
  for (let i = 0; i < count; i++) {
    const angle = rand(-Math.PI * 0.95, -Math.PI * 0.05);
    const dist = rand(50, 120);
    sprite(pick(['heart', 'heart', 'petal', 'sparkle', 'blossom']), x, y, rand(12, 22), {
      '--dx': (Math.cos(angle) * dist).toFixed(0) + 'px', '--dy': (Math.sin(angle) * dist).toFixed(0) + 'px', '--rot': rand(-200, 200).toFixed(0) + 'deg',
    });
  }
}

// Little flowers opening round a point.
function bloom(x, y) {
  for (let i = 0; i < 5; i++) {
    const angle = (i / 5) * Math.PI * 2 + rand(-0.3, 0.3);
    sprite(i % 2 ? 'bloom daisy' : 'bloom', x, y, rand(14, 20), { '--dx': (Math.cos(angle) * 34).toFixed(0) + 'px', '--dy': (Math.sin(angle) * 22).toFixed(0) + 'px' });
  }
}

// The steady extras: a petal drifting down over everything, a heart floating up.
function schedule(fn, min, max) {
  const run = () => { if (moving()) fn(); timers.push(setTimeout(run, rand(min, max))); };
  timers.push(setTimeout(run, rand(min, max) / 2));
}
const dropPetal = () => sprite('petal falling', rand(0, innerWidth), -20, rand(12, 20), {
  '--sway': rand(-60, 60).toFixed(0) + 'px', '--rot': rand(-540, 540).toFixed(0) + 'deg', '--dur': rand(9, 15).toFixed(1) + 's',
});
const riseHeart = () => sprite('heart rising', pick([rand(20, innerWidth * 0.3), rand(innerWidth * 0.7, innerWidth - 20)]), innerHeight + 10, rand(14, 24), {
  '--sway': rand(-40, 40).toFixed(0) + 'px', '--dur': rand(6, 9).toFixed(1) + 's',
});

// Butterflies: each flies a leg at a time toward a point (a CSS transition, so the compositor moves
// it, not a script every frame), bobbing and flapping as it goes (CSS animations); now and then it
// lands on a pane's title strip for a few seconds, and one that leaves the window comes back in from
// another side.
const PANES = '.app > :is(.rail, .sidebar, .chat, .member-panel), .settings-nav, .settings-main, .signin-card, .titlebar';

function perch() {
  const panes = [...document.querySelectorAll(PANES)].map((p) => p.getBoundingClientRect()).filter((r) => r.width > 80 && r.height > 0);
  if (!panes.length) return null;
  const r = pick(panes);
  return { x: r.left + rand(24, r.width - 24), y: r.top - 4, land: true };
}

function target(fly) {
  const roll = Math.random();
  if (roll < 0.22) { const p = perch(); if (p) return p; }
  if (roll > 0.94) return { x: pick([-80, innerWidth + 80]), y: rand(0, innerHeight), away: true };
  // Mostly a hop of a few hundred pixels, so the path zigzags about rather than crossing the window.
  const reach = rand(140, 360);
  const angle = rand(0, Math.PI * 2);
  return {
    x: Math.max(40, Math.min(innerWidth - 40, fly.x + Math.cos(angle) * reach)),
    y: Math.max(60, Math.min(innerHeight - 40, fly.y + Math.sin(angle) * reach)),
  };
}

function makeFly() {
  const el = div('lc-fly');
  const bob = div('lc-fly-bob');
  bob.append(div('lc-fly-wing l'), div('lc-fly-body'), div('lc-fly-wing r'));
  el.appendChild(bob);
  el.style.setProperty('--flap', rand(0.22, 0.32).toFixed(2) + 's');
  bob.style.animationDelay = rand(-3, 0).toFixed(2) + 's';
  const fly = { el, x: rand(60, innerWidth - 60), y: rand(80, innerHeight - 60), size: rand(0.9, 1.35), speed: rand(70, 110), tilt: 0, timer: 0 };
  front.appendChild(el);
  return fly;
}

// Puts a butterfly where it is (no transition), or sends it there over `seconds`.
function place(fly, seconds = 0) {
  fly.el.style.transition = seconds ? `transform ${seconds.toFixed(2)}s cubic-bezier(0.45, 0.05, 0.4, 1)` : 'none';
  fly.el.style.transform = `translate(${fly.x.toFixed(1)}px, ${fly.y.toFixed(1)}px) rotate(${fly.tilt.toFixed(1)}deg) scale(${fly.size.toFixed(2)})`;
  // A jump is committed at once, so the next leg flies from here.
  if (!seconds) void fly.el.offsetWidth;
}

const quiet = () => !motion || reduce.matches;

function leg(fly) {
  clearTimeout(fly.timer);
  if (!on || quiet()) return;
  if (!moving()) { fly.timer = setTimeout(() => leg(fly), 1000); return; }
  fly.el.classList.remove('resting');
  const t = target(fly);
  const dx = t.x - fly.x;
  const dist = Math.hypot(dx, t.y - fly.y);
  const seconds = Math.max(0.9, dist / fly.speed);
  fly.x = t.x;
  fly.y = t.y;
  fly.tilt = t.land ? 0 : Math.max(-28, Math.min(28, dx / Math.max(dist, 1) * 28));
  place(fly, seconds);
  fly.timer = setTimeout(() => {
    if (t.land) { fly.el.classList.add('resting'); fly.timer = setTimeout(() => leg(fly), rand(3000, 7000)); return; }
    if (t.away) { fly.x = t.x < 0 ? innerWidth + 60 : -60; fly.y = rand(60, innerHeight - 60); fly.size = rand(0.9, 1.35); place(fly); }
    leg(fly);
  }, seconds * 1000);
}

// With motion off the butterflies sit still on the panes, like the other decorations.
function settle() {
  for (const fly of flies) {
    clearTimeout(fly.timer);
    const p = perch();
    if (p) { fly.x = p.x; fly.y = p.y; }
    fly.tilt = 0;
    fly.el.classList.add('resting');
    place(fly);
  }
}

function start() {
  back = div('lc-garden-back');
  back.setAttribute('aria-hidden', 'true');
  const sway = (cls) => { const s = div('lc-garden-sway ' + cls); s.appendChild(div('lc-garden-petals')); return s; };
  back.append(div('lc-garden-meadow'), sway('far'), sway('near'));
  front = div('lc-garden-front');
  front.setAttribute('aria-hidden', 'true');
  document.body.insertBefore(back, document.body.firstChild);
  document.body.appendChild(front);
  flies = [makeFly(), makeFly(), makeFly()];
  for (const fly of flies) place(fly);

  events = new AbortController();
  const opts = { signal: events.signal, passive: true };
  document.addEventListener('lc-sent', () => {
    if (!moving()) return;
    const box = document.querySelector('.composer');
    if (box) { const r = box.getBoundingClientRect(); burst(r.right - 40, r.top, 12); }
  }, opts);
  document.addEventListener('pointerdown', (e) => {
    if (!moving() || !(e.target instanceof Element)) return;
    if (e.target.closest('.reaction, .emoji-cell, .reaction-add')) burst(e.clientX, e.clientY, 8);
    else if (e.target.closest('.dm, .channel, .settings-nav-item, .rail-button, .friend')) bloom(e.clientX, e.clientY);
  }, { ...opts, capture: true });
  document.addEventListener('pointermove', (e) => {
    if (!moving() || e.pointerType === 'touch') return;
    const now = performance.now();
    if (now - trailAt < 45 || Math.hypot(e.clientX - trailX, e.clientY - trailY) < 18) return;
    trailAt = now;
    trailX = e.clientX;
    trailY = e.clientY;
    sprite(Math.random() < 0.7 ? 'sparkle trail' : 'heart trail', e.clientX + rand(-6, 6), e.clientY + rand(4, 12), rand(8, 14), { '--dx': rand(-14, 14).toFixed(0) + 'px', '--rot': rand(-90, 90).toFixed(0) + 'deg' });
  }, opts);
  reduce.addEventListener('change', still, opts);
  schedule(dropPetal, 2200, 4200);
  schedule(riseHeart, 5000, 9000);
  still();
}

function stop() {
  if (events) events.abort();
  events = null;
  for (const t of timers) clearTimeout(t);
  timers = [];
  for (const fly of flies) clearTimeout(fly.timer);
  back?.remove();
  front?.remove();
  back = front = null;
  flies = [];
  html.classList.remove('lc-garden-still');
}

// Motion off (the setting or the system's reduced motion): petals and wings stop, butterflies rest.
function still() {
  html.classList.toggle('lc-garden-still', on && quiet());
  if (!on) return;
  if (quiet()) {
    for (const s of front.querySelectorAll('.lc-sprite')) s.remove();
    requestAnimationFrame(settle);
  } else {
    for (const fly of flies) leg(fly);
  }
}

// The theme is (or isn't) Sakura Retro.
export function setGarden(active) {
  if (active === on) return;
  on = active;
  if (on) start();
  else stop();
}

// The Animations setting (settings.display.motion).
export function gardenMotion(enabled) {
  if (motion === enabled) return;
  motion = enabled;
  still();
}
