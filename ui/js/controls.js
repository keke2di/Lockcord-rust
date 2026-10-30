// The settings window's controls, in Discord's style: radio cards, switches, sliders with marks,
// a dropdown, and colour tiles with their picker popout (presets, a saturation square, hue and
// opacity sliders, a hex field, the eyedropper). Built from plain elements, styled in app.css.

import { h, icon, ICONS } from './dom.js';
import { format, fromHsv, hex, toHsv, toRgba } from './color.js';

// --- Radio cards ------------------------------------------------------------------------------

// options: [{ value, label, description?, visual? }]
export function radioCards(options, current, onChange, cls = '') {
  return h('div', { class: 'radio-cards ' + cls, role: 'radiogroup' }, options.map((o) => h('button', {
    type: 'button', role: 'radio', class: 'radio-card' + (o.value === current ? ' selected' : ''), 'aria-checked': o.value === current ? 'true' : 'false',
    onclick: () => onChange(o.value),
  }, o.visual || h('span', { class: 'radio-dot', 'aria-hidden': 'true' }),
    h('span', { class: 'radio-text' }, h('span', { class: 'radio-label' }, o.label), o.description ? h('span', { class: 'radio-description' }, o.description) : null))));
}

// --- Switch -----------------------------------------------------------------------------------

export function toggle(label, description, on, onChange) {
  const button = h('button', { type: 'button', role: 'switch', class: 'switch' + (on ? ' on' : ''), 'aria-checked': on ? 'true' : 'false', 'aria-label': label },
    h('span', { class: 'switch-knob' }));
  button.addEventListener('click', () => {
    const next = !button.classList.contains('on');
    button.classList.toggle('on', next);
    button.setAttribute('aria-checked', next ? 'true' : 'false');
    onChange(next);
  });
  return h('div', { class: 'switch-row' }, h('div', { class: 'switch-text' }, h('div', { class: 'control-title' }, label),
    description ? h('div', { class: 'control-description' }, description) : null), button);
}

// --- Slider -----------------------------------------------------------------------------------

// marks: values to label under the track; format(value) for the labels and the readout.
export function slider({ title, description, min, max, step, value, marks = [], format: fmt = String, onInput }) {
  const input = h('input', { type: 'range', class: 'range', min, max, step, value, 'aria-label': title });
  const readout = h('span', { class: 'control-value' }, fmt(Number(value)));
  const fill = () => input.style.setProperty('--fill', ((Number(input.value) - min) / (max - min)) * 100 + '%');
  fill();
  input.addEventListener('input', () => { fill(); readout.textContent = fmt(Number(input.value)); onInput(Number(input.value)); });
  const ticks = h('div', { class: 'range-marks', 'aria-hidden': 'true' }, marks.map((m) => {
    const tick = h('span', { class: 'range-mark' }, h('span', { class: 'range-tick' }), h('span', { class: 'range-label' }, fmt(m)));
    tick.style.left = ((m - min) / (max - min)) * 100 + '%';
    return tick;
  }));
  return h('div', { class: 'control' }, h('div', { class: 'control-head' }, h('span', { class: 'control-title' }, title), readout),
    description ? h('div', { class: 'control-description' }, description) : null,
    h('div', { class: 'range-wrap' + (marks.length ? ' marked' : '') }, input, ticks));
}

// --- Dropdown ---------------------------------------------------------------------------------

// options: [{ value, label, style? }] (style: CSS for the option's own look, e.g. its font)
export function dropdown(options, value, onChange, label) {
  const current = options.find((o) => o.value === value) || options[0];
  const shown = h('span', { class: 'dropdown-value' }, current ? current.label : '');
  if (current && current.style) shown.style.cssText = current.style;
  const box = h('div', { class: 'dropdown' });
  const button = h('button', { type: 'button', class: 'dropdown-button', 'aria-haspopup': 'listbox', 'aria-label': label }, shown, icon(ICONS.chevron, 18));
  let list = null;
  const close = () => { if (list) { list.remove(); list = null; document.removeEventListener('pointerdown', outside, true); } };
  const outside = (e) => { if (!box.contains(e.target)) close(); };
  button.addEventListener('click', () => {
    if (list) { close(); return; }
    list = h('div', { class: 'dropdown-list', role: 'listbox' }, options.map((o) => {
      const item = h('button', { type: 'button', role: 'option', class: 'dropdown-item' + (o.value === value ? ' selected' : ''), 'aria-selected': o.value === value ? 'true' : 'false',
        onclick: () => { close(); onChange(o.value); } }, o.label, o.value === value ? icon(ICONS.check, 18) : null);
      if (o.style) item.style.cssText = o.style;
      return item;
    }));
    box.appendChild(list);
    document.addEventListener('pointerdown', outside, true);
  });
  box.append(button);
  return box;
}

// --- Colours ----------------------------------------------------------------------------------

export const PRESET_COLOURS = [
  '#5865f2', '#3ba55c', '#23a559', '#faa61a', '#f0b232', '#ed4245', '#f23f43', '#eb459e', '#9b59b6', '#1abc9c',
  '#00a8fc', '#ffffff', '#dbdee1', '#949ba4', '#4e5058', '#313338', '#2b2d31', '#1e1f22', '#111214', '#000000',
];

// A tile: the colour, its name, its value; a click opens the picker.
export function colourTile(label, value, onChange) {
  const chip = h('span', { class: 'colour-chip' }, h('span', { class: 'colour-chip-fill' }));
  const valueText = h('span', { class: 'colour-tile-value' }, value);
  const paint = (v) => { chip.firstChild.style.background = v; valueText.textContent = v; };
  paint(value);
  const tile = h('button', { type: 'button', class: 'colour-tile', 'aria-label': label + ': ' + value },
    chip, h('span', { class: 'colour-tile-text' }, h('span', { class: 'colour-tile-label' }, label), valueText));
  tile.addEventListener('click', (e) => {
    e.stopPropagation();
    openPicker(tile, valueText.textContent, (v) => { paint(v); onChange(v); });
  });
  return tile;
}

let openOne = null;

function openPicker(anchor, value, onChange) {
  if (openOne) openOne.close();
  let colour = toRgba(value) || { r: 0, g: 0, b: 0, a: 1 };
  let hsv = toHsv(colour);

  const square = h('div', { class: 'picker-square' }, h('span', { class: 'picker-handle' }));
  const hue = h('div', { class: 'picker-bar picker-hue' }, h('span', { class: 'picker-thumb' }));
  const alpha = h('div', { class: 'picker-bar picker-alpha' }, h('span', { class: 'picker-alpha-fill' }), h('span', { class: 'picker-thumb' }));
  const field = h('input', { class: 'picker-input', spellcheck: false, 'aria-label': 'Colour value' });
  const swatch = h('span', { class: 'picker-current' }, h('span', { class: 'colour-chip-fill' }));
  const presets = h('div', { class: 'picker-presets' }, PRESET_COLOURS.map((c) => {
    const b = h('button', { type: 'button', class: 'picker-preset', title: c, 'aria-label': c });
    b.style.background = c;
    b.addEventListener('click', () => { colour = { ...toRgba(c), a: colour.a }; hsv = toHsv(colour); draw(true); });
    return b;
  }));
  const dropper = 'EyeDropper' in window ? h('button', { type: 'button', class: 'picker-dropper', title: 'Pick from the screen', 'aria-label': 'Pick from the screen',
    onclick: async () => {
      try {
        const { sRGBHex } = await new window.EyeDropper().open();
        colour = { ...toRgba(sRGBHex), a: colour.a };
        hsv = toHsv(colour);
        draw(true);
      } catch (e) { /* cancelled */ }
    } }, icon(ICONS.dropper, 18)) : null;

  const pop = h('div', { class: 'picker', role: 'dialog', 'aria-label': 'Colour picker' },
    presets, square, h('div', { class: 'picker-row' }, h('div', { class: 'picker-bars' }, hue, alpha), swatch),
    h('div', { class: 'picker-row' }, field, dropper));

  // Drawn from hsv; `emit` sends the colour on (not while it's only being opened).
  const draw = (emit) => {
    colour = fromHsv(hsv);
    const solid = hex(colour);
    square.style.background = 'linear-gradient(to top, #000, transparent), linear-gradient(to right, #fff, hsl(' + hsv.h + ', 100%, 50%))';
    square.firstChild.style.left = hsv.s * 100 + '%';
    square.firstChild.style.top = (1 - hsv.v) * 100 + '%';
    hue.firstChild.style.left = (hsv.h / 360) * 100 + '%';
    alpha.firstChild.style.background = 'linear-gradient(to right, transparent, ' + solid + ')';
    alpha.lastChild.style.left = hsv.a * 100 + '%';
    swatch.firstChild.style.background = format(colour);
    if (document.activeElement !== field) field.value = format(colour);
    if (emit) onChange(format(colour));
  };

  // Dragging on the square or a bar: pointer capture keeps it while the mouse leaves.
  const drag = (el, apply) => {
    const move = (e) => {
      const r = el.getBoundingClientRect();
      const x = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
      const y = Math.max(0, Math.min(1, (e.clientY - r.top) / r.height));
      apply(x, y);
      draw(true);
    };
    el.addEventListener('pointerdown', (e) => { el.setPointerCapture(e.pointerId); move(e); });
    el.addEventListener('pointermove', (e) => { if (el.hasPointerCapture(e.pointerId)) move(e); });
  };
  drag(square, (x, y) => { hsv.s = x; hsv.v = 1 - y; });
  drag(hue, (x) => { hsv.h = Math.min(359.9, x * 360); });
  drag(alpha, (x) => { hsv.a = Math.round(x * 100) / 100; });
  field.addEventListener('change', () => {
    const parsed = toRgba(field.value.trim());
    if (!parsed) { field.value = format(colour); return; }
    hsv = toHsv(parsed);
    draw(true);
  });

  const close = () => {
    pop.remove();
    document.removeEventListener('pointerdown', outside, true);
    document.removeEventListener('keydown', escape, true);
    openOne = null;
  };
  const outside = (e) => { if (!pop.contains(e.target) && !anchor.contains(e.target)) close(); };
  // Esc closes the picker before anything else (the settings window, behind it, stays).
  const escape = (e) => { if (e.key === 'Escape') { e.stopImmediatePropagation(); e.preventDefault(); close(); } };
  document.addEventListener('pointerdown', outside, true);
  document.addEventListener('keydown', escape, true);
  openOne = { close };

  const screen = anchor.closest('.settings-screen') || document.body;
  screen.appendChild(pop);
  const a = anchor.getBoundingClientRect();
  const s = screen.getBoundingClientRect();
  const left = Math.min(a.left - s.left, s.width - pop.offsetWidth - 12);
  const below = a.bottom - s.top + 8;
  const top = below + pop.offsetHeight > s.height - 12 ? a.top - s.top - pop.offsetHeight - 8 : below;
  pop.style.left = Math.max(12, left) + 'px';
  pop.style.top = Math.max(12, top) + 'px';
  draw(false);
}
