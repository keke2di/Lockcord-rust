// Video tiles: a call's decoded pictures (call.rs Frames), pulled from Rust only while a tile is
// on screen, at most about 30 times a second, and drawn with WebGL, which turns the decoder's YUV
// into colour on the GPU. One canvas per tile, kept across redraws (a WebGL context is costly).

import { api } from './api.js';

const tiles = new Map(); // key -> tile
const FRAME_GAP_MS = 30;

const VERTEX = `
attribute vec2 pos;
attribute vec2 uv;
varying vec2 v;
void main() { v = uv; gl_Position = vec4(pos, 0.0, 1.0); }`;

// layout: 0 RGBA, 1 NV12, 2 I420, 3 RGB (call.rs Frames::take).
const FRAGMENT = `
precision mediump float;
varying vec2 v;
uniform sampler2D ty;
uniform sampler2D tu;
uniform sampler2D tv;
uniform int layout;
uniform vec3 kr;   // Kr, Kb, Kg
uniform vec2 range; // luma offset and scale; chroma scale follows
void main() {
  if (layout == 0 || layout == 3) { gl_FragColor = vec4(texture2D(ty, v).rgb, 1.0); return; }
  float y = (texture2D(ty, v).r - range.x) * range.y;
  vec2 c = layout == 1 ? texture2D(tu, v).ra : vec2(texture2D(tu, v).r, texture2D(tv, v).r);
  float cs = range.x > 0.0 ? 255.0 / 224.0 : 1.0;
  float cb = (c.x - 0.5) * cs;
  float cr = (c.y - 0.5) * cs;
  float r = y + 2.0 * (1.0 - kr.x) * cr;
  float b = y + 2.0 * (1.0 - kr.y) * cb;
  float g = (y - kr.x * r - kr.y * b) / kr.z;
  gl_FragColor = vec4(clamp(vec3(r, g, b), 0.0, 1.0), 1.0);
}`;

// Kr and Kb per matrix: BT.601, BT.709, BT.2020, SMPTE 240M.
const MATRICES = [[0.299, 0.114], [0.2126, 0.0722], [0.2627, 0.0593], [0.212, 0.087]];

function setup(canvas) {
  const gl = canvas.getContext('webgl', { alpha: false, antialias: false, preserveDrawingBuffer: false });
  if (!gl) return null;
  const shader = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); return s; };
  const program = gl.createProgram();
  gl.attachShader(program, shader(gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(program, shader(gl.FRAGMENT_SHADER, FRAGMENT));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return null;
  gl.useProgram(program);
  const buffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  const textures = [0, 1, 2].map((unit) => {
    const t = gl.createTexture();
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, t);
    for (const [k, v] of [[gl.TEXTURE_MIN_FILTER, gl.LINEAR], [gl.TEXTURE_MAG_FILTER, gl.LINEAR], [gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE], [gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE]]) gl.texParameteri(gl.TEXTURE_2D, k, v);
    return t;
  });
  gl.uniform1i(gl.getUniformLocation(program, 'ty'), 0);
  gl.uniform1i(gl.getUniformLocation(program, 'tu'), 1);
  gl.uniform1i(gl.getUniformLocation(program, 'tv'), 2);
  gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  return {
    gl, program, buffer, textures,
    pos: gl.getAttribLocation(program, 'pos'), uv: gl.getAttribLocation(program, 'uv'),
    layout: gl.getUniformLocation(program, 'layout'), kr: gl.getUniformLocation(program, 'kr'), range: gl.getUniformLocation(program, 'range'),
  };
}

// The quad, its texture corners turned by the picture's quarter turns.
function quad(g, turns) {
  const corners = [[0, 1], [1, 1], [0, 0], [1, 0]];
  const turned = corners.map(([u, v]) => {
    let [a, b] = [u, v];
    for (let i = 0; i < turns; i++) [a, b] = [b, 1 - a];
    return [a, b];
  });
  const pos = [[-1, -1], [1, -1], [-1, 1], [1, 1]];
  const data = new Float32Array(pos.flatMap((p, i) => [...p, ...turned[i]]));
  const { gl } = g;
  gl.bindBuffer(gl.ARRAY_BUFFER, g.buffer);
  gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
  gl.enableVertexAttribArray(g.pos);
  gl.vertexAttribPointer(g.pos, 2, gl.FLOAT, false, 16, 0);
  gl.enableVertexAttribArray(g.uv);
  gl.vertexAttribPointer(g.uv, 2, gl.FLOAT, false, 16, 8);
}

function upload(g, unit, format, w, h, data) {
  const { gl } = g;
  gl.activeTexture(gl.TEXTURE0 + unit);
  gl.bindTexture(gl.TEXTURE_2D, g.textures[unit]);
  gl.texImage2D(gl.TEXTURE_2D, 0, format, w, h, 0, format, gl.UNSIGNED_BYTE, data);
}

// One picture from call.rs: a 16-byte header, then the planes.
function draw(t, buffer) {
  const head = new DataView(buffer, 0, 16);
  const id = head.getUint32(0, true);
  const w = head.getUint16(4, true);
  const h = head.getUint16(6, true);
  const [layout, matrix, full, turns] = [head.getUint8(8), head.getUint8(9), head.getUint8(10), head.getUint8(11)];
  const planes = new Uint8Array(buffer, 16);
  const cw = Math.ceil(w / 2);
  const ch = Math.ceil(h / 2);
  const need = layout === 0 ? w * h * 4 : layout === 3 ? w * h * 3 : w * h + cw * ch * 2;
  if (!w || !h || planes.length < need) return;
  t.id = id;
  const g = t.g;
  const { gl } = g;
  const sideways = turns % 2 === 1;
  const [dw, dh] = sideways ? [h, w] : [w, h];
  if (t.canvas.width !== dw || t.canvas.height !== dh) { t.canvas.width = dw; t.canvas.height = dh; }
  if (t.turns !== turns) { quad(g, turns); t.turns = turns; }
  gl.viewport(0, 0, dw, dh);
  if (layout === 0) upload(g, 0, gl.RGBA, w, h, planes.subarray(0, need));
  else if (layout === 3) upload(g, 0, gl.RGB, w, h, planes.subarray(0, need));
  else {
    upload(g, 0, gl.LUMINANCE, w, h, planes.subarray(0, w * h));
    if (layout === 1) upload(g, 1, gl.LUMINANCE_ALPHA, cw, ch, planes.subarray(w * h, w * h + cw * ch * 2));
    else {
      upload(g, 1, gl.LUMINANCE, cw, ch, planes.subarray(w * h, w * h + cw * ch));
      upload(g, 2, gl.LUMINANCE, cw, ch, planes.subarray(w * h + cw * ch, w * h + cw * ch * 2));
    }
  }
  const [r, b] = MATRICES[matrix] || MATRICES[0];
  gl.uniform1i(g.layout, layout);
  gl.uniform3f(g.kr, r, b, 1 - r - b);
  gl.uniform2f(g.range, full ? 0 : 16 / 255, full ? 1 : 255 / 219);
  gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  t.canvas.classList.add('has-picture');
}

function createTile(key) {
  const canvas = document.createElement('canvas');
  const t = { key, canvas, g: null, id: 0, turns: -1, running: false, pending: false, last: 0 };
  t.g = setup(canvas);
  const loop = (now) => {
    if (!canvas.isConnected || !t.g) { t.running = false; return; }
    requestAnimationFrame(loop);
    if (t.pending || now - t.last < FRAME_GAP_MS) return;
    t.pending = true;
    t.last = now;
    const width = Math.round((canvas.clientWidth || 320) * (window.devicePixelRatio || 1));
    api.callFrame(key, t.id, width).then((buffer) => {
      if (buffer && buffer.byteLength > 16) draw(t, buffer);
    }).catch(() => {}).finally(() => { t.pending = false; });
  };
  t.start = () => {
    if (t.running || !t.g) return;
    t.running = true;
    // Started once the canvas is in the page (the redraw adds it after this returns).
    requestAnimationFrame(loop);
  };
  return t;
}

// The canvas for a tile ("<user id>", "stream:<user id>", "me"), drawing as long as it's shown.
export function videoTile(key, cls) {
  let t = tiles.get(key);
  if (!t) {
    t = createTile(key);
    tiles.set(key, t);
  }
  t.canvas.className = 'video-canvas' + (t.id ? ' has-picture' : '') + (cls ? ' ' + cls : '');
  t.start();
  return t.canvas;
}

// After a call: every tile and its WebGL context let go.
export function dropTiles(keep = () => false) {
  for (const [key, t] of tiles) {
    if (keep(key)) continue;
    const lose = t.g && t.g.gl.getExtension('WEBGL_lose_context');
    if (lose) lose.loseContext();
    t.g = null;
    tiles.delete(key);
  }
}
