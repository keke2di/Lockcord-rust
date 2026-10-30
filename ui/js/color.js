// Colours for the settings window: any CSS colour to RGBA and HSV and back, and the form a
// token is saved in (#rrggbb when solid, rgba() when not).

// Any CSS colour as { r, g, b, a } (0-255, 0-1), by letting the browser resolve it.
export function toRgba(value) {
  const probe = document.createElement('span');
  probe.style.color = value || '';
  if (!probe.style.color) return null;
  probe.style.display = 'none';
  document.body.appendChild(probe);
  const out = getComputedStyle(probe).color;
  probe.remove();
  const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?\)/.exec(out);
  return m ? { r: Number(m[1]), g: Number(m[2]), b: Number(m[3]), a: m[4] === undefined ? 1 : Number(m[4]) } : null;
}

const hex2 = (n) => Math.round(Math.max(0, Math.min(255, n))).toString(16).padStart(2, '0');

// The saved form: #rrggbb when solid, rgba(r, g, b, a) when see-through.
export function format({ r, g, b, a }) {
  if (a >= 0.995) return '#' + hex2(r) + hex2(g) + hex2(b);
  return 'rgba(' + Math.round(r) + ', ' + Math.round(g) + ', ' + Math.round(b) + ', ' + Math.round(a * 100) / 100 + ')';
}

export function hex({ r, g, b }) {
  return '#' + hex2(r) + hex2(g) + hex2(b);
}

export function toHsv({ r, g, b, a }) {
  const [R, G, B] = [r / 255, g / 255, b / 255];
  const max = Math.max(R, G, B);
  const d = max - Math.min(R, G, B);
  let h = 0;
  if (d) {
    if (max === R) h = ((G - B) / d) % 6;
    else if (max === G) h = (B - R) / d + 2;
    else h = (R - G) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: max ? d / max : 0, v: max, a };
}

export function fromHsv({ h, s, v, a }) {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  const [R, G, B] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
  return { r: (R + m) * 255, g: (G + m) * 255, b: (B + m) * 255, a };
}
