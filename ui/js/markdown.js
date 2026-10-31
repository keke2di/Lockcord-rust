// Discord's markdown, read into a tree. render() builds the tree as DOM nodes with textContent,
// so nothing anyone types is ever parsed as HTML; links only ever carry an http(s) address, and
// opening one goes through LockCord (main), never the page.
//
//   blocks   ```code```, > quote, >>> quote to the end, # ## ### headers, -# small text, lists
//   inline   **bold** *italic* _italic_ __underline__ ~~strike~~ ||spoiler|| `code`, \escapes,
//            links (bare, <bare>, [masked](https://…)), <@mentions>, <#channels>, <:custom:emoji>,
//            <t:timestamps>, :shortcode: emoji

import { SHORTCODES } from './emoji.js';

const URL_RE = /^https?:\/\/[^\s<]+[^\s<.,:;"')\]!?]/;
const MAX_DEPTH = 8;

export function parse(text) {
  return blocks(String(text || '').replace(/\r\n?/g, '\n'), 0);
}

// A message with nothing but up to 27 emoji shows them large, as Discord does.
export function jumbo(tree) {
  let count = 0;
  const walk = (nodes) => nodes.every((n) => {
    if (n.t === 'emoji' || n.t === 'uemoji') { count++; return true; }
    if (n.t === 'text') {
      const rest = n.v.replace(/\s+/g, '');
      if (!rest) return true;
      const emoji = rest.match(/\p{Extended_Pictographic}(?:️|‍\p{Extended_Pictographic}|[\u{1F3FB}-\u{1F3FF}])*|\p{Regional_Indicator}{2}/gu) || [];
      if (emoji.join('') !== rest) return false;
      count += emoji.length;
      return true;
    }
    if (n.t === 'p') return walk(n.c);
    return n.t === 'br';
  });
  return walk(tree) && count > 0 && count <= 27;
}

// The text without formatting, for a reply's preview.
export function plain(tree) {
  let out = '';
  const walk = (nodes) => {
    for (const n of nodes) {
      if (n.t === 'text' || n.t === 'code' || n.t === 'pre') out += n.v;
      else if (n.t === 'br') out += ' ';
      else if (n.t === 'emoji') out += ':' + n.name + ':';
      else if (n.t === 'uemoji') out += n.v;
      else if (n.t === 'mention') out += '@' + (n.name || 'unknown-user');
      else if (n.t === 'channel') out += '#channel';
      else if (n.t === 'time') out += new Date(n.ts * 1000).toLocaleString();
      else if (n.t === 'list') n.items.forEach((item) => { walk(item); out += ' '; });
      else if (n.c) { walk(n.c); if (n.t === 'p' || n.t === 'h' || n.t === 'quote' || n.t === 'sub') out += ' '; }
    }
  };
  walk(tree);
  return out.replace(/\s+/g, ' ').trim();
}

// ---------------------------------------------------------------------------------------------
// Blocks.

function blocks(text, depth) {
  const out = [];
  const lines = text.split('\n');
  let para = [];
  const flush = () => {
    if (!para.length) return;
    out.push({ t: 'p', c: inline(para.join('\n'), depth) });
    para = [];
  };

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // A fenced code block runs to its closing fence, or to the end.
    const fence = /^```([\w+-]{0,20})(.*)$/.exec(line);
    if (fence) {
      const rest = [fence[2]];
      let j = i + 1;
      let closed = fence[2].endsWith('```') && fence[2].length >= 3;
      if (closed) rest[0] = fence[2].slice(0, -3);
      while (!closed && j < lines.length) {
        if (lines[j].endsWith('```')) { rest.push(lines[j].slice(0, -3)); closed = true; } else rest.push(lines[j]);
        j++;
      }
      if (closed) {
        flush();
        // ```text``` on one line has no language: the word is the code.
        const lang = fence[1];
        const body = rest.join('\n');
        out.push({ t: 'pre', lang: rest.length > 1 ? lang : '', v: (rest.length > 1 ? body : lang + body).replace(/^\n|\n$/g, '') });
        i = j - 1;
        continue;
      }
    }

    if (depth < 1 && line.startsWith('>>> ')) {
      flush();
      out.push({ t: 'quote', c: blocks([line.slice(4)].concat(lines.slice(i + 1)).join('\n'), depth + 1) });
      break;
    }
    if (depth < 1 && /^> /.test(line)) {
      flush();
      const quoted = [];
      while (i < lines.length && /^> /.test(lines[i])) quoted.push(lines[i++].slice(2));
      i--;
      out.push({ t: 'quote', c: blocks(quoted.join('\n'), depth + 1) });
      continue;
    }

    const header = /^(#{1,3}) (.+)$/.exec(line);
    if (header) { flush(); out.push({ t: 'h', level: header[1].length, c: inline(header[2], depth) }); continue; }
    const sub = /^-# (.+)$/.exec(line);
    if (sub) { flush(); out.push({ t: 'sub', c: inline(sub[1], depth) }); continue; }

    const item = /^(\s*)([-*]|\d{1,9}\.) (.+)$/.exec(line);
    if (item) {
      flush();
      const ordered = /\d/.test(item[2]);
      const list = { t: 'list', ordered, start: ordered ? parseInt(item[2], 10) : 1, items: [] };
      while (i < lines.length) {
        const m = /^(\s*)([-*]|\d{1,9}\.) (.+)$/.exec(lines[i]);
        if (!m || /\d/.test(m[2]) !== ordered) break;
        list.items.push(inline(m[3], depth));
        i++;
      }
      i--;
      out.push(list);
      continue;
    }

    para.push(line);
  }
  flush();
  return out;
}

// ---------------------------------------------------------------------------------------------
// Inline. At each position the rules are tried in order; anything else is text.

const WRAPS = [
  ['**', 'b'], ['__', 'u'], ['~~', 's'], ['||', 'spoiler'], ['*', 'i'], ['_', 'i'],
];

function inline(text, depth) {
  const out = [];
  let buf = '';
  const push = (node) => {
    if (buf) { out.push({ t: 'text', v: buf }); buf = ''; }
    out.push(node);
  };

  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    const ch = text[i];
    let m;

    if (ch === '\\' && /^\\[^\w\s]/.test(rest)) { buf += rest[1]; i += 2; continue; }
    if (ch === '\n') { push({ t: 'br' }); i++; continue; }

    if (ch === '`') {
      m = /^(``?)([^`]|[^`][\s\S]*?[^`])\1(?!`)/.exec(rest);
      if (m) { push({ t: 'code', v: m[2].replace(/^ (.*) $/, '$1') }); i += m[0].length; continue; }
    }

    if (ch === '<') {
      if ((m = /^<(https?:\/\/[^\s>]+)>/.exec(rest))) { push({ t: 'link', href: m[1], c: [{ t: 'text', v: m[1] }] }); i += m[0].length; continue; }
      if ((m = /^<@!?(\d{1,20})>/.exec(rest))) { push({ t: 'mention', id: m[1] }); i += m[0].length; continue; }
      if ((m = /^<@&(\d{1,20})>/.exec(rest))) { push({ t: 'role', id: m[1] }); i += m[0].length; continue; }
      if ((m = /^<#(\d{1,20})>/.exec(rest))) { push({ t: 'channel', id: m[1] }); i += m[0].length; continue; }
      if ((m = /^<(a?):(\w{2,32}):(\d{1,20})>/.exec(rest))) { push({ t: 'emoji', name: m[2], id: m[3], animated: !!m[1] }); i += m[0].length; continue; }
      if ((m = /^<t:(-?\d{1,13})(?::([tTdDfFR]))?>/.exec(rest))) { push({ t: 'time', ts: Number(m[1]), style: m[2] || 'f' }); i += m[0].length; continue; }
    }

    if (ch === '[' && depth < MAX_DEPTH) {
      m = /^\[([^\]\n]{1,256})\]\(<?(https?:\/\/[^\s)>]+)>?\)/.exec(rest);
      if (m) { push({ t: 'link', href: m[2], masked: true, c: inline(m[1], depth + 1) }); i += m[0].length; continue; }
    }

    if ((ch === 'h' || ch === 'H') && (i === 0 || /[^\w]/.test(text[i - 1]))) {
      m = URL_RE.exec(rest);
      if (m) { push({ t: 'link', href: m[0], c: [{ t: 'text', v: m[0] }] }); i += m[0].length; continue; }
    }

    if (ch === ':') {
      m = /^:([a-z0-9_+-]{1,32}):/.exec(rest);
      if (m && SHORTCODES[m[1]]) { push({ t: 'uemoji', v: SHORTCODES[m[1]], name: m[1] }); i += m[0].length; continue; }
    }

    if (depth < MAX_DEPTH) {
      let wrapped = false;
      for (const [mark, type] of WRAPS) {
        if (!rest.startsWith(mark)) continue;
        // _italic_ only between word boundaries, as in Discord.
        if (mark === '_' && i > 0 && /\w/.test(text[i - 1])) continue;
        const end = closing(text, i + mark.length, mark);
        if (end < 0) continue;
        if (mark === '_' && end + 1 < text.length && /\w/.test(text[end + 1])) continue;
        const inner = text.slice(i + mark.length, end);
        if (!inner.trim() || (mark === '*' && /^\s/.test(inner))) continue;
        push({ t: type, c: inline(inner, depth + 1) });
        i = end + mark.length;
        wrapped = true;
        break;
      }
      if (wrapped) continue;
    }

    buf += ch;
    i++;
  }
  if (buf) out.push({ t: 'text', v: buf });
  return out;
}

// Where `mark` closes, skipping escapes and code, or -1. A single * doesn't close on **.
function closing(text, from, mark) {
  for (let j = from; j < text.length; j++) {
    const c = text[j];
    if (c === '\\') { j++; continue; }
    if (c === '`') {
      const end = text.indexOf('`', j + 1);
      if (end > 0) { j = end; continue; }
    }
    if (text.startsWith(mark, j)) {
      if (mark === '*' && text[j + 1] === '*') { j++; continue; }
      if (mark === '_' && text[j + 1] === '_') { j++; continue; }
      if (j > from) return j;
    }
  }
  return -1;
}

// ---------------------------------------------------------------------------------------------
// DOM. `ctx` names people for mentions and formats times; `openLink` takes a click on a link.

export function render(tree, doc, ctx = {}) {
  const frag = doc.createDocumentFragment();
  for (const node of tree) frag.appendChild(build(node, doc, ctx));
  return frag;
}

function el(doc, tag, cls, children) {
  const node = doc.createElement(tag);
  if (cls) node.className = cls;
  for (const child of children || []) node.appendChild(child);
  return node;
}

function text(doc, v) {
  return doc.createTextNode(v);
}

function kids(nodes, doc, ctx) {
  return nodes.map((n) => build(n, doc, ctx));
}

function build(n, doc, ctx) {
  switch (n.t) {
    case 'text': return text(doc, n.v);
    case 'br': return doc.createElement('br');
    case 'p': return el(doc, 'div', 'md-p', kids(n.c, doc, ctx));
    case 'b': return el(doc, 'strong', null, kids(n.c, doc, ctx));
    case 'i': return el(doc, 'em', null, kids(n.c, doc, ctx));
    case 'u': return el(doc, 'u', null, kids(n.c, doc, ctx));
    case 's': return el(doc, 's', null, kids(n.c, doc, ctx));
    case 'spoiler': {
      const node = el(doc, 'span', 'md-spoiler', kids(n.c, doc, ctx));
      node.setAttribute('role', 'button');
      node.tabIndex = 0;
      node.addEventListener('click', () => node.classList.add('revealed'));
      return node;
    }
    case 'code': return el(doc, 'code', 'md-code', [text(doc, n.v)]);
    case 'pre': {
      const code = el(doc, 'code', null, [text(doc, n.v)]);
      return el(doc, 'pre', 'md-pre', [code]);
    }
    case 'quote': return el(doc, 'blockquote', 'md-quote', kids(n.c, doc, ctx));
    case 'h': return el(doc, 'div', 'md-h md-h' + n.level, kids(n.c, doc, ctx));
    case 'sub': return el(doc, 'div', 'md-sub', kids(n.c, doc, ctx));
    case 'list': {
      const list = el(doc, n.ordered ? 'ol' : 'ul', 'md-list', n.items.map((item) => el(doc, 'li', null, kids(item, doc, ctx))));
      if (n.ordered && n.start !== 1) list.start = n.start;
      return list;
    }
    case 'link': {
      const a = el(doc, 'a', 'md-link', kids(n.c, doc, ctx));
      a.href = n.href;
      a.title = n.href;
      // A masked link also passes its text, so a text naming another site can be questioned.
      a.addEventListener('click', (e) => { e.preventDefault(); if (ctx.openLink) ctx.openLink(n.href, n.masked ? a.textContent : null); });
      return a;
    }
    case 'mention': return el(doc, 'span', 'md-mention', [text(doc, '@' + ((ctx.nameOf && ctx.nameOf(n.id)) || 'unknown-user'))]);
    case 'role': {
      // A server's role, in its colour, where the page knows it (ctx.roleOf).
      const role = ctx.roleOf && n.id && ctx.roleOf(n.id);
      const node = el(doc, 'span', 'md-mention', [text(doc, '@' + (role ? role.name : 'role'))]);
      if (role && role.color) node.style.color = role.color;
      return node;
    }
    case 'channel': return el(doc, 'span', 'md-mention', [text(doc, '#' + ((ctx.channelName && ctx.channelName(n.id)) || 'channel'))]);
    case 'emoji': {
      // An image where the page can load one (ctx.emojiUrl), the :name: otherwise.
      const src = ctx.emojiUrl && n.id && ctx.emojiUrl(n.id, n.animated);
      if (!src) return el(doc, 'span', 'md-custom-emoji', [text(doc, ':' + n.name + ':')]);
      const img = el(doc, 'img', 'md-emoji-img', []);
      img.src = src;
      img.alt = ':' + n.name + ':';
      img.title = ':' + n.name + ':';
      img.draggable = false;
      return img;
    }
    case 'uemoji': return el(doc, 'span', 'md-emoji', [text(doc, n.v)]);
    case 'time': {
      const when = new Date(n.ts * 1000);
      const node = el(doc, 'span', 'md-time', [text(doc, ctx.formatTime ? ctx.formatTime(when, n.style) : when.toLocaleString())]);
      node.title = when.toLocaleString();
      return node;
    }
    default: return text(doc, '');
  }
}
