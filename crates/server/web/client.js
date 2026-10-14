// mc-rs browser client: signs in, opens the game socket, decodes the H.264 stream with
// WebCodecs onto the canvas, plays the PCM audio through an AudioWorklet, and sends keys and
// mouse back in the proto::Input encoding (crates/proto/src/lib.rs).
'use strict';

// Saved settings live in prefs.js (`store`), the delay measurement in lag.js (`lag`, `stats`), the
// settings tuner in tune.js (`tune`), the picture settings and their Advanced section in
// picture.js (`conn`), and the touch controls in touch.js (`touch`); index.html loads the first
// four before this file and touch.js after it.
const $ = (s) => document.querySelector(s);

// ---------- sign in ----------

let token = store.get('mcrs.token');
let info = null;

async function boot() {
  // mcrs://join/<this host> opens the launcher's Join a server tab with this address.
  $('#nativejoin').href = 'mcrs://join/' + location.host;
  if (!('VideoDecoder' in window)) {
    $('#serverline').textContent = 'This browser cannot decode the game stream. Use a current Chrome, Edge, Firefox or Safari.';
    $('#form').classList.add('hidden');
    return;
  }
  try {
    info = await (await fetch('/api/info')).json();
    const n = info.players.length;
    $('#serverline').textContent = n ? `${n} of ${info.max} playing: ${info.players.join(', ')}` : `Nobody is playing yet (room for ${info.max}).`;
    if (info.join_password) { $('#joinrow').classList.remove('hidden'); $('#fields').classList.add('join'); }
    $('#guest').classList.toggle('hidden', !info.guests);
  } catch {
    $('#serverline').textContent = 'The server is not answering.';
  }
  fillConnection($('#login'));
  fillConnection($('#connpanel'));
  wireConnection($('#login'), true);
  wireConnection($('#connpanel'), false);
  // Measures in the background while the player signs in; Play waits for it.
  autoPick();
  if (token) {
    const r = await fetch('/api/me?token=' + encodeURIComponent(token)).catch(() => null);
    if (r && r.ok) {
      const me = await r.json();
      showSignedIn(me.name, me.guest);
      return;
    }
    token = null;
    store.set('mcrs.token', null);
  }
  const last = store.get('mcrs.name');
  if (last) { $('#name').value = last; $('#password').focus(); } else { $('#name').focus(); }
}

// The name and password fields leave the page once they are done with (the sign-in card only
// comes back through a reload): a password manager such as KeePassXC pins its icon to a field
// for as long as the field exists, hidden or not, and would float it over the game.
function dropFields() {
  if (document.activeElement && document.activeElement.closest('#fields')) document.activeElement.blur();
  const f = $('#fields');
  if (f) f.remove();
}

function showSignedIn(name, guest = false) {
  dropFields();
  $('#guest').classList.add('hidden');
  $('#who').innerHTML = '';
  const t = document.createElement('span');
  t.textContent = guest ? `Playing as ${name}, a guest. ` : `Signed in as ${name}. `;
  const a = document.createElement('a');
  a.href = '#';
  a.textContent = 'Sign out';
  a.style.color = 'var(--dim)';
  a.onclick = async (e) => {
    e.preventDefault();
    await fetch('/api/logout', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ token }) }).catch(() => {});
    token = null;
    store.set('mcrs.token', null);
    location.reload();
  };
  $('#who').append(t, a);
  $('#play').focus();
}

$('#form').addEventListener('submit', async (e) => {
  e.preventDefault();
  $('#err').textContent = '';
  // Asked first, while the tap still counts as the player's gesture.
  if (touch.on) touch.fullscreen();
  readConnection($('#login'));
  if (!token) {
    $('#play').disabled = true;
    try {
      const r = await fetch('/api/login', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ name: $('#name').value, password: $('#password').value, join: $('#join').value }),
      });
      const j = await r.json();
      if (!j.ok) { $('#err').textContent = j.error || 'Could not sign in.'; return; }
      token = j.token;
      store.set('mcrs.token', token);
      store.set('mcrs.name', j.name);
    } catch {
      $('#err').textContent = 'The server is not answering.';
      return;
    } finally {
      $('#play').disabled = false;
    }
  }
  await pictureReady($('#play'));
  start();
});

// A guest needs no account: the name typed (when nobody has an account by it), or a Guest name.
$('#guest').addEventListener('click', async () => {
  $('#err').textContent = '';
  if (touch.on) touch.fullscreen();
  readConnection($('#login'));
  $('#guest').disabled = true;
  try {
    const r = await fetch('/api/guest', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ name: $('#name').value, join: $('#join').value }),
    });
    const j = await r.json();
    if (!j.ok) { $('#err').textContent = j.error || 'Could not join as a guest.'; return; }
    token = j.token;
    store.set('mcrs.token', token);
  } catch {
    $('#err').textContent = 'The server is not answering.';
    return;
  } finally {
    $('#guest').disabled = false;
  }
  await pictureReady($('#guest'));
  start();
});

// ---------- the game socket ----------

const canvas = $('#screen');
// A desynchronized canvas draws to the front buffer, which saves a frame of delay on a desktop
// but flashes white between frames on phone GPUs (S23 FE), so phones get the normal
// double-buffered canvas. `?desync=1` or `?desync=0` overrides it to compare.
const desyncParam = new URLSearchParams(location.search).get('desync');
const phone = matchMedia('(pointer: coarse)').matches || /Android|iPhone|iPad/i.test(navigator.userAgent);
const desync = desyncParam === null ? !phone : desyncParam === '1';
const ctx2d = canvas.getContext('2d', { alpha: false, desynchronized: desync });
let ws = null;
let focusState = 0; // proto FOCUS_NONE, FOCUS_GAME, FOCUS_MENU
let picture = { width: 1280, height: 720, fps: 60 };
let closedByServer = false;

function overlay(text, opts = {}) {
  $('#overlay').classList.toggle('hidden', text == null);
  if (text == null) return;
  $('#msg').textContent = text;
  const b = $('#bubble');
  b.classList.toggle('click', !!opts.click);
  b.querySelectorAll('button').forEach((x) => x.remove());
  if (opts.button) {
    const btn = document.createElement('button');
    btn.className = 'btn';
    btn.textContent = opts.button;
    btn.onclick = opts.onButton;
    b.appendChild(btn);
  }
}

function start() {
  dropFields();
  $('#login').classList.add('hidden');
  $('#game').classList.remove('hidden');
  overlay('Starting your game...');
  closedByServer = false;
  tune.resetWatch();
  audio.start();
  const params = new URLSearchParams({ token, w: conn.w, fps: conn.fps, br: conn.br });
  lag.reset();
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  ws = new WebSocket(`${proto}//${location.host}/ws?${params}`);
  ws.binaryType = 'arraybuffer';
  ws.onmessage = (ev) => (typeof ev.data === 'string' ? onText(JSON.parse(ev.data)) : onBinary(new Uint8Array(ev.data)));
  ws.onclose = () => {
    video.reset();
    if (document.pointerLockElement) document.exitPointerLock();
    if (!closedByServer) overlay('The connection to the server dropped.', { button: 'Reconnect', onButton: reconnect });
  };
  if (!pinging) pinging = setInterval(ping, 2000);
}
let pinging = null;

function reconnect() {
  if (ws && ws.readyState <= 1) ws.close();
  start();
}

function send(obj) {
  if (ws && ws.readyState === 1) ws.send(JSON.stringify(obj));
}

function onText(m) {
  switch (m.t) {
    case 'status': overlay(m.text); break;
    case 'welcome':
      picture = { width: m.width, height: m.height, fps: m.fps, bitrate: m.bitrate_kbps };
      canvas.width = m.width;
      canvas.height = m.height;
      fit();
      overlay('Loading the world...');
      break;
    // The picture changed mid-game (picture.js `setPicture`): the session's new encoder starts
    // with a key frame at the new size, so the decoder starts over from it.
    case 'picture':
      picture = { width: m.width, height: m.height, fps: m.fps, bitrate: m.bitrate_kbps };
      canvas.width = m.width;
      canvas.height = m.height;
      fit();
      video.reset();
      tune.resetWatch();
      break;
    case 'focus': setFocus(m.state); break;
    case 'chat': chatLine(m.from, m.text, m); break;
    case 'players': players = m.list; renderList(); break;
    case 'pong': stats.ping = Math.round(performance.now() - m.ts); break;
    case 'closed':
      closedByServer = true;
      overlay(m.text, { button: 'Reconnect', onButton: reconnect });
      break;
  }
}

function onBinary(b) {
  if (b[0] === 1) {
    // proto::VideoHeader: flags, input sequence, host pipeline in microseconds.
    const v = new DataView(b.buffer, b.byteOffset + 1, 9);
    const seq = v.getUint32(1, true), hostUs = v.getUint32(5, true);
    video.push(b[1] & 1, b.subarray(10), seq, hostUs);
  }
  else if (b[0] === 2) audio.push(b.buffer, b.byteOffset + 1, b.byteLength - 1);
  stats.bytes += b.byteLength;
}

function ping() { send({ t: 'ping', ts: performance.now() }); }

// ---------- video ----------

const video = {
  decoder: null,
  needKey: true,
  ts: 0,
  shown: false,
  async configure() {
    // The host sends constrained baseline (no reordering), so frames leave the decoder at once.
    const configs = ['avc1.42E02A', 'avc1.42E028', 'avc1.42E01F', 'avc1.4D4028'].map((codec) => ({ codec, optimizeForLatency: true, hardwareAcceleration: 'prefer-hardware' }));
    for (const c of configs.slice()) configs.push({ codec: c.codec, optimizeForLatency: true });
    for (const c of configs) {
      try {
        const s = await VideoDecoder.isConfigSupported(c);
        if (s.supported) return c;
      } catch {}
    }
    return configs[0];
  },
  async make() {
    this.decoder = 'pending';
    const config = await this.configure();
    this.codec = `${config.codec}${config.hardwareAcceleration ? ' hw' : ''}`;
    const d = new VideoDecoder({
      output: (frame) => {
        ctx2d.drawImage(frame, 0, 0, canvas.width, canvas.height);
        lag.shown(frame.timestamp);
        frame.close();
        stats.frames++;
        if (!this.shown) { this.shown = true; setFocus(focusState); }
      },
      error: (e) => { console.warn('decoder error', e); this.reset(); },
    });
    d.configure(config);
    this.decoder = d;
  },
  reset() {
    if (this.decoder && this.decoder !== 'pending' && this.decoder.state !== 'closed') {
      try { this.decoder.close(); } catch {}
    }
    this.decoder = null;
    this.needKey = true;
  },
  push(key, data, seq, hostUs) {
    if (!this.decoder) { if (key) this.make().then(() => this.push(key, data, seq, hostUs)); return; }
    if (this.decoder === 'pending') return;
    lag.received(seq, hostUs);
    // Behind by more than a frame or two: skip to the next key frame rather than fall behind.
    lag.queue = Math.max(lag.queue, this.decoder.decodeQueueSize);
    if (this.decoder.decodeQueueSize > 2 && !key) { this.needKey = true; stats.dropped++; return; }
    if (this.needKey && !key) return;
    this.needKey = false;
    this.ts += 1;
    lag.decoding(this.ts, seq);
    try {
      this.decoder.decode(new EncodedVideoChunk({ type: key ? 'key' : 'delta', timestamp: this.ts, data }));
    } catch (e) {
      console.warn(e);
      this.reset();
    }
  },
};

function fit() {
  const s = Math.min(innerWidth / canvas.width, innerHeight / canvas.height);
  canvas.style.width = Math.floor(canvas.width * s) + 'px';
  canvas.style.height = Math.floor(canvas.height * s) + 'px';
}
addEventListener('resize', fit);

// ---------- audio ----------

const audio = {
  ctx: null,
  node: null,
  gain: null,
  volume: Math.min(100, Math.max(0, store.num('mcrs.volume', 100))),
  async start() {
    if (this.ctx) { this.ctx.resume(); return; }
    try {
      this.ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' });
      await this.ctx.audioWorklet.addModule('/audio-worklet.js');
      this.node = new AudioWorkletNode(this.ctx, 'pcm-player', { outputChannelCount: [2] });
      this.gain = this.ctx.createGain();
      this.gain.gain.value = this.volume / 100;
      this.node.connect(this.gain).connect(this.ctx.destination);
    } catch (e) {
      console.warn('no audio', e);
      this.ctx = null;
    }
  },
  push(buffer, offset, length) {
    if (!this.node) return;
    const pcm = new Int16Array(buffer.slice(offset, offset + (length & ~1)));
    this.node.port.postMessage(pcm, [pcm.buffer]);
  },
  // 0 to 100, kept for the next visit.
  setVolume(v) {
    this.volume = v;
    store.set('mcrs.volume', v);
    if (this.gain) this.gain.gain.value = v / 100;
  },
};

// ---------- input ----------

const enc = new TextEncoder();
let pending = [];
let moveX = 0, moveY = 0;
let cursor = null;
let lastFlush = 0;

function event(bytes) { pending.push(bytes); }
function f32pair(tag, a, b) {
  const v = new DataView(new ArrayBuffer(9));
  v.setUint8(0, tag); v.setFloat32(1, a, true); v.setFloat32(5, b, true);
  return new Uint8Array(v.buffer);
}
function key(tag, code) {
  const c = enc.encode(code).subarray(0, 255);
  const out = new Uint8Array(2 + c.length);
  out[0] = tag; out[1] = c.length; out.set(c, 2);
  return out;
}
function flush() {
  if (moveX || moveY) { event(f32pair(3, moveX, moveY)); moveX = moveY = 0; }
  if (cursor) { event(f32pair(7, cursor[0], cursor[1])); cursor = null; }
  if (!pending.length || !ws || ws.readyState !== 1) { pending = []; return; }
  // Input::Seq last: frames carry back the newest sequence they reflect.
  const seq = lag.sent();
  const sv = new DataView(new ArrayBuffer(5));
  sv.setUint8(0, 9); sv.setUint32(1, seq, true);
  pending.push(new Uint8Array(sv.buffer));
  let n = 0;
  for (const p of pending) n += p.length;
  const out = new Uint8Array(n);
  let o = 0;
  for (const p of pending) { out.set(p, o); o += p.length; }
  pending = [];
  ws.send(out);
  lastFlush = performance.now();
}
setInterval(flush, 4);
// Mouse turns go out as they happen (browsers hand mouse moves out once per display frame), at
// most one batch every 2 ms; the 4 ms interval only catches what that leaves behind.
function flushSoon() { if (performance.now() - lastFlush >= 2) flush(); }

const locked = () => document.pointerLockElement === canvas;
// Whether keys and the wheel go to the game: the pointer is locked, or, on a touch screen where
// there is no pointer lock, the game has focus.
const captured = () => locked() || (touch.on && focusState === 1);
const inGame = () => !$('#game').classList.contains('hidden') && ws && ws.readyState === 1;
const chatOpen = () => !$('#chatbox').classList.contains('hidden');
let leavingForChat = false;

function lock() {
  if (locked() || touch.on) return;
  const p = canvas.requestPointerLock({ unadjustedMovement: true });
  if (p && p.catch) p.catch(() => { const q = canvas.requestPointerLock(); if (q && q.catch) q.catch(() => {}); });
}

function setFocus(state) {
  focusState = state;
  if (!video.shown) return;
  if (touch.on) {
    touch.focus(state);
    overlay(state === 0 ? 'Tap to play' : null, { click: state === 0 });
    return;
  }
  if (state === 1) {
    if (!locked()) {
      lock();
      overlay('Click to keep playing', { click: true });
      setTimeout(() => { if (locked()) overlay(null); }, 300);
    } else overlay(null);
  } else {
    if (locked()) document.exitPointerLock();
    overlay(state === 0 ? 'Click to play' : null, { click: state === 0 });
  }
}

document.addEventListener('pointerlockchange', () => {
  $('#game').classList.toggle('locked', locked());
  if (locked()) { overlay(null); return; }
  if (leavingForChat) { leavingForChat = false; return; }
  // The browser takes Esc to free the pointer; the game opens its menu for it.
  if (focusState === 1 && inGame()) {
    event(key(1, 'Escape'));
    event(key(2, 'Escape'));
    flush();
  }
});

$('#overlay').addEventListener('mousedown', (e) => {
  if (!video.shown || !$('#bubble').classList.contains('click')) return;
  e.preventDefault();
  audio.start();
  lock();
  event(new Uint8Array([4, e.button]));
  event(new Uint8Array([5, e.button]));
  flush();
});

canvas.addEventListener('mousedown', (e) => {
  if (!inGame()) return;
  e.preventDefault();
  audio.start();
  if (focusState !== 2 && !locked()) lock();
  if (focusState === 2) cursorAt(e);
  event(new Uint8Array([4, e.button]));
  flush();
});
addEventListener('mouseup', (e) => {
  if (!inGame()) return;
  event(new Uint8Array([5, e.button]));
  flush();
});
addEventListener('contextmenu', (e) => { if (inGame()) e.preventDefault(); });

function cursorAt(e) {
  const r = canvas.getBoundingClientRect();
  cursor = [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height];
}
addEventListener('mousemove', (e) => {
  if (!inGame()) return;
  if (locked()) { moveX += e.movementX; moveY += e.movementY; }
  else if (focusState === 2) cursorAt(e);
  flushSoon();
});
addEventListener('wheel', (e) => {
  if (!inGame() || (!captured() && focusState !== 2)) return;
  e.preventDefault();
  // One notch, one slot. A mouse wheel's event is a notch whatever its size (browsers report
  // 100 or 120 pixels, or 3 lines); a touchpad's small steps add up to a notch first.
  const big = e.deltaMode !== 0 || Math.abs(e.deltaY) >= 50;
  wheelAcc = big ? Math.sign(e.deltaY) : wheelAcc + e.deltaY / 100;
  if (Math.abs(wheelAcc) < 1) return;
  const notch = -Math.sign(wheelAcc);
  wheelAcc = 0;
  const v = new DataView(new ArrayBuffer(5));
  v.setUint8(0, 6); v.setFloat32(1, notch, true);
  event(new Uint8Array(v.buffer));
  flush();
}, { passive: false });
let wheelAcc = 0;

// Only full screen and the developer tools stay with the browser; F1, F3, F5 and the rest go to
// the game.
const BROWSER_KEYS = new Set(['F11', 'F12']);
addEventListener('keydown', (e) => {
  if (!inGame() || chatOpen()) return;
  if (e.code === 'Tab') { e.preventDefault(); showList(true); return; }
  if (captured() && (e.code === 'KeyT' || e.code === 'Enter' || e.code === 'Slash')) {
    e.preventDefault();
    openChat(e.code === 'Slash' ? '/' : '');
    return;
  }
  if (BROWSER_KEYS.has(e.code)) return;
  if (!captured() && focusState !== 2) return;
  e.preventDefault();
  if (e.repeat) return;
  event(key(1, e.code));
  flush();
});
addEventListener('keyup', (e) => {
  if (!inGame()) return;
  if (e.code === 'Tab') { showList(false); return; }
  if (chatOpen()) return;
  event(key(2, e.code));
  flush();
});
addEventListener('blur', () => { if (inGame()) { event(new Uint8Array([8])); flush(); } });

// Full screen where the browser has it for the page (Safari on an iPhone does not).
const canFullscreen = !!(document.fullscreenEnabled || document.webkitFullscreenEnabled);
async function fullscreen() {
  const el = document.documentElement;
  const req = el.requestFullscreen || el.webkitRequestFullscreen;
  if (!req || document.fullscreenElement || document.webkitFullscreenElement) return !!req;
  try { await req.call(el, { navigationUI: 'hide' }); return true; } catch { return false; }
}
$('#full').classList.toggle('hidden', !canFullscreen);
$('#full').onclick = async () => {
  try {
    if (await fullscreen() && navigator.keyboard && navigator.keyboard.lock) await navigator.keyboard.lock();
  } catch {}
  fit();
};
$('#leave').onclick = () => { if (ws) ws.close(); location.reload(); };

// ---------- chat and the player list ----------

let players = [];

// Minecraft's chat colors by name, for command answers (`kind: "command"`).
const CHAT_COLORS = {
  black: '#000000', dark_blue: '#0000aa', dark_green: '#00aa00', dark_aqua: '#00aaaa', dark_red: '#aa0000',
  dark_purple: '#aa00aa', gold: '#ffaa00', gray: '#aaaaaa', dark_gray: '#555555', blue: '#5555ff',
  green: '#55ff55', aqua: '#55ffff', red: '#ff5555', light_purple: '#ff55ff', yellow: '#ffff55', white: '#ffffff',
};

function chatLine(from, text, m = {}) {
  const line = document.createElement('div');
  line.className = 'line';
  if (from) {
    const f = document.createElement('span');
    f.className = 'from';
    f.textContent = `<${from}> `;
    line.append(f, document.createTextNode(text));
  } else {
    const s = document.createElement('span');
    s.className = 'sys';
    s.textContent = text;
    // Command answers carry their own color (white, red for errors, gray for whispers).
    if (m.kind === 'command') {
      s.style.color = CHAT_COLORS[m.color] || CHAT_COLORS.white;
      if (m.italic) s.style.fontStyle = 'italic';
    }
    line.append(s);
  }
  const box = $('#chat');
  box.appendChild(line);
  while (box.children.length > 12) box.firstChild.remove();
  setTimeout(() => line.classList.add('fade'), 10000);
}

function openChat(prefix) {
  event(new Uint8Array([8]));
  flush();
  leavingForChat = locked();
  if (locked()) document.exitPointerLock();
  const box = $('#chatbox');
  box.classList.remove('hidden');
  $('#chat').classList.add('open');
  box.value = prefix;
  setTimeout(() => box.focus(), 0);
}

function closeChat() {
  const box = $('#chatbox');
  box.classList.add('hidden');
  box.blur();
  $('#chat').classList.remove('open');
  if (focusState === 1) lock();
}

$('#chatbox').addEventListener('keydown', (e) => {
  e.stopPropagation();
  if (e.key === 'Enter') {
    const text = e.target.value.trim();
    if (text) send({ t: 'chat', text });
    closeChat();
  } else if (e.key === 'Escape') {
    e.preventDefault();
    closeChat();
  }
});

function renderList() {
  const l = $('#list');
  l.innerHTML = '';
  const h = document.createElement('div');
  h.style.color = 'var(--dim)';
  h.textContent = `${players.length} online`;
  l.appendChild(h);
  for (const p of players) {
    const d = document.createElement('div');
    d.textContent = p;
    l.appendChild(d);
  }
}
function showList(on) { renderList(); $('#list').classList.toggle('hidden', !on); }

// ---------- the connection panel in game ----------

$('#conn').onclick = () => { fillConnection($('#connpanel')); $('#connpanel').classList.toggle('hidden'); };
$('#connapply').onclick = () => {
  readConnection($('#connpanel'));
  $('#connpanel').classList.add('hidden');
  setPicture(conn);
};
// ---------- paintings: a file sent from here hangs on the block the crosshair is on ----------

async function hangFile(file) {
  if (!file || !inGame()) return;
  chatLine(null, `Uploading ${file.name}...`);
  const url = `/api/media?token=${encodeURIComponent(token)}&name=${encodeURIComponent(file.name)}`;
  const r = await fetch(url, { method: 'POST', body: file }).catch(() => null);
  const j = r && r.status === 413 ? { error: 'That file is over 16 MB.' } : await r?.json().catch(() => null);
  if (j && j.ok) chatLine(null, `Look at a block face: ${file.name} hangs where the crosshair is.`);
  else chatLine(null, `Could not hang ${file.name}: ${(j && j.error) || 'the upload failed.'}`, { kind: 'command', color: 'red' });
}
$('#hang').onclick = () => $('#hangfile').click();
$('#hangfile').addEventListener('change', (e) => { hangFile(e.target.files[0]); e.target.value = ''; });
$('#game').addEventListener('dragover', (e) => { if (e.dataTransfer.types.includes('Files')) e.preventDefault(); });
$('#game').addEventListener('drop', (e) => { e.preventDefault(); hangFile(e.dataTransfer.files[0]); });
$('#lagtoggle').onclick =() => { statsShown = !statsShown; store.setFlag('mcrs.lagstats', statsShown); };
$('#volume').value = audio.volume;
$('#volume').addEventListener('input', (e) => audio.setVolume(+e.target.value));

boot();
