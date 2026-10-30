// Small DOM helpers. Everything is built with DOM calls and textContent: nothing anyone sends is
// ever parsed as HTML.

export function h(tag, props, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k === 'class') node.className = v;
    else if (k === 'text') node.textContent = v;
    else if (k === 'dataset') Object.assign(node.dataset, v);
    else if (k.startsWith('on')) node.addEventListener(k.slice(2), v);
    else if (k in node && k !== 'list' && k !== 'role') node[k] = v;
    else node.setAttribute(k, v === true ? '' : v);
  }
  for (const child of children.flat()) if (child !== null && child !== undefined && child !== false) node.appendChild(typeof child === 'string' ? document.createTextNode(child) : child);
  return node;
}

const SVG = 'http://www.w3.org/2000/svg';
export function icon(path, size = 20) {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('width', size);
  svg.setAttribute('height', size);
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  for (const d of [].concat(path)) {
    const p = document.createElementNS(SVG, 'path');
    p.setAttribute('fill', 'currentColor');
    p.setAttribute('d', d);
    svg.appendChild(p);
  }
  return svg;
}

export const ICONS = {
  react: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm-3.5 7a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3Zm7 0a1.5 1.5 0 1 1 0 3 1.5 1.5 0 0 1 0-3ZM12 18a6 6 0 0 1-5.2-3h10.4A6 6 0 0 1 12 18Z',
  reply: 'M10 8V4l-7 7 7 7v-4.1c5 0 8.5 1.6 11 5.1-1-5-4-10-11-11Z',
  edit: 'M3 17.25V21h3.75L17.8 9.94l-3.75-3.75L3 17.25ZM20.7 7.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83Z',
  trash: 'M9 3v1H4v2h16V4h-5V3H9Zm-3 5 1 13h10l1-13H6Z',
  plus: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm5 11h-4v4h-2v-4H7v-2h4V7h2v4h4v2Z',
  file: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6Zm-1 7V3.5L18.5 9H13Z',
  close: 'M18.3 5.71 12 12.01l-6.29-6.3-1.42 1.42L10.59 13.4l-6.3 6.29 1.42 1.42L12 14.83l6.29 6.28 1.42-1.42-6.3-6.29 6.3-6.29-1.41-1.41Z',
  lock: 'M6 9h1V6a5 5 0 0 1 10 0v3h1a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-8a3 3 0 0 1 3-3Zm9-3v3H9V6a3 3 0 1 1 6 0Zm-1 8a2 2 0 0 1-1 1.73V18a1 1 0 1 1-2 0v-2.27A2 2 0 1 1 14 14Z',
  globe: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm6.9 6h-3a15.7 15.7 0 0 0-1.4-3.6A8 8 0 0 1 18.9 8ZM12 4c.8 1.2 1.5 2.5 1.9 4h-3.8c.4-1.5 1.1-2.8 1.9-4ZM4.3 14a8.2 8.2 0 0 1 0-4h3.4a16.5 16.5 0 0 0 0 4H4.3Zm.8 2h3a15.7 15.7 0 0 0 1.4 3.6A8 8 0 0 1 5.1 16Zm3-8h-3a8 8 0 0 1 4.4-3.6A15.7 15.7 0 0 0 8.1 8ZM12 20c-.8-1.2-1.5-2.5-1.9-4h3.8c-.4 1.5-1.1 2.8-1.9 4Zm2.3-6H9.7a14.7 14.7 0 0 1 0-4h4.6a14.7 14.7 0 0 1 0 4Zm.3 5.6c.6-1.1 1.1-2.3 1.4-3.6h3a8 8 0 0 1-4.4 3.6Zm1.7-5.6a16.5 16.5 0 0 0 0-4h3.4a8.2 8.2 0 0 1 0 4h-3.4Z',
  shield: 'M12 2 4 5v6c0 5 3.4 9.6 8 11 4.6-1.4 8-6 8-11V5l-8-3Zm0 2.2 6 2.2V11c0 4-2.6 7.7-6 8.9-3.4-1.2-6-4.9-6-8.9V6.4l6-2.2Z',
  palette: 'M12 3a9 9 0 0 0 0 18c.8 0 1.5-.7 1.5-1.5 0-.4-.1-.7-.4-1-.2-.3-.4-.6-.4-1 0-.8.7-1.5 1.5-1.5H16a5 5 0 0 0 5-5c0-4.4-4-8-9-8Zm-5.5 9a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3Zm3-4a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3Zm5 0a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3Zm3 4a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3Z',
  sparkle: 'M12 2l1.9 5.6 5.6 1.9-5.6 1.9L12 17l-1.9-5.6-5.6-1.9 5.6-1.9L12 2Zm7 12 .9 2.6 2.6.9-2.6.9L19 21l-.9-2.6-2.6-.9 2.6-.9L19 14Z',
  code: 'M8.6 16.6 4 12l4.6-4.6L7.2 6l-6 6 6 6 1.4-1.4Zm6.8 0L20 12l-4.6-4.6L16.8 6l6 6-6 6-1.4-1.4Z',
  folder: 'M10 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-8l-2-2Z',
  eye: 'M12 5c-5 0-9.3 3.1-11 7 1.7 3.9 6 7 11 7s9.3-3.1 11-7c-1.7-3.9-6-7-11-7Zm0 11.5a4.5 4.5 0 1 1 0-9 4.5 4.5 0 0 1 0 9Zm0-7a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Z',
  chevron: 'M7.4 8.6 12 13.2l4.6-4.6L18 10l-6 6-6-6 1.4-1.4Z',
  dropper: 'M20.7 5.6 18.4 3.3a1 1 0 0 0-1.4 0l-3.1 3.1-1.9-1.9-1.4 1.4 1.4 1.4L4 15.3V20h4.7l8-8 1.4 1.4 1.4-1.4-1.9-1.9 3.1-3.1a1 1 0 0 0 0-1.4ZM7.9 18H6v-1.9l7.4-7.4 1.9 1.9L7.9 18Z',
  gear: 'M19.4 13a7.6 7.6 0 0 0 0-2l2.1-1.6-2-3.5-2.5 1a7.4 7.4 0 0 0-1.7-1L15 3.3h-4L10.7 6a7.4 7.4 0 0 0-1.7 1l-2.5-1-2 3.5L6.6 11a7.6 7.6 0 0 0 0 2l-2.1 1.6 2 3.5 2.5-1a7.4 7.4 0 0 0 1.7 1l.3 2.6h4l.3-2.6a7.4 7.4 0 0 0 1.7-1l2.5 1 2-3.5L19.4 13ZM13 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7Z',
  pin: 'M14.5 2 22 9.5l-2 1-1.5-1.5-4 4 .5 4.5-2 2-4-4L4 20.5 3.5 20 8 15l-4-4 2-2 4.5.5 4-4L13 4l1.5-2Z',
  members: 'M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm-7 9a7 7 0 0 1 14 0v1H2v-1Zm14.5-9a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7ZM18 21v-1a8.9 8.9 0 0 0-2.4-6.1A6 6 0 0 1 23 20v1h-5Z',
  chat: 'M4 3h16a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H9l-5 4v-4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z',
  add: 'M20 11h-7V4h-2v7H4v2h7v7h2v-7h7v-2Z',
  thread: 'M5.4 21h1.6l.6-4h4l-.6 4h1.6l.6-4H17v-1.5h-3.6l.6-4H18V10h-3.8l.6-4h-1.6l-.6 4h-4l.6-4H7.6L7 10H3v1.5h3.8l-.6 4H2V17h4l-.6 4Zm2.6-9.5h4l-.6 4h-4l.6-4ZM18.5 13a4.5 4.5 0 0 0-4.5 4.5 4.4 4.4 0 0 0 .5 2L14 22l2.6-.5a4.5 4.5 0 1 0 1.9-8.5Z',
  chevronRight: 'M8.6 16.6 13.2 12 8.6 7.4 10 6l6 6-6 6-1.4-1.4Z',
  search: 'M15.5 14h-.8l-.3-.3A6.5 6.5 0 1 0 14 15.5l.3.3v.8l5 5 1.5-1.5-5-5Zm-6 0A4.5 4.5 0 1 1 14 9.5 4.5 4.5 0 0 1 9.5 14Z',
  check: 'M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4L9 16.2Z',
  at: 'M12 2a10 10 0 1 0 5.66 18.25l-1.13-1.65A8 8 0 1 1 20 12v1.5a1.5 1.5 0 0 1-3 0V12a5 5 0 1 0-1.46 3.53A3.5 3.5 0 0 0 22 13.5V12A10 10 0 0 0 12 2Zm0 13a3 3 0 1 1 0-6 3 3 0 0 1 0 6Z',
};

export function formatBytes(n) {
  if (n < 1024) return n + ' bytes';
  if (n < 1024 * 1024) return (n / 1024).toFixed(2) + ' KB';
  return (n / 1024 / 1024).toFixed(2) + ' MB';
}

export function sameDay(a, b) {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

// "Today at 19:09", "Yesterday at 08:15", or the date, as Discord shows a group's time.
export function stamp(at) {
  const when = new Date(at);
  const now = new Date();
  const time = when.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (sameDay(when, now)) return 'Today at ' + time;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (sameDay(when, yesterday)) return 'Yesterday at ' + time;
  return when.toLocaleDateString() + ' ' + time;
}

export function shortTime(at) {
  return new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

export function formatTime(when, style) {
  switch (style) {
    case 't': return when.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    case 'T': return when.toLocaleTimeString();
    case 'd': return when.toLocaleDateString();
    case 'D': return when.toLocaleDateString([], { dateStyle: 'long' });
    case 'F': return when.toLocaleString([], { dateStyle: 'full', timeStyle: 'short' });
    case 'R': {
      const s = Math.round((when - Date.now()) / 1000);
      const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
      const abs = Math.abs(s);
      if (abs < 60) return rtf.format(s, 'second');
      if (abs < 3600) return rtf.format(Math.round(s / 60), 'minute');
      if (abs < 86400) return rtf.format(Math.round(s / 3600), 'hour');
      return rtf.format(Math.round(s / 86400), 'day');
    }
    default: return when.toLocaleString([], { dateStyle: 'long', timeStyle: 'short' });
  }
}

// A Discord snowflake's time.
export function snowflakeTime(id) {
  return Number(BigInt(id) >> 22n) + 1420070400000;
}
