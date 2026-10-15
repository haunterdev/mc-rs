// The settings tuner for the mc-rs browser client: measures this device and this connection and
// picks the picture size, frame rate and bitrate that keep the delay low and the picture sharp,
// so a player never has to choose them (they stay under Advanced). client.js asks `tune.pick()`
// before it joins and feeds the 5-second stats to `tune.watch()` while playing.
//
// What it measures, once a day per screen (kept in `mcrs.tune`):
// - the screen: its long side in device pixels (a bigger picture is only scaled down) and its
//   refresh rate (frames past it never reach the eye);
// - the line to the host: the ping (`/api/ping`, median of five) and its spread, and the speed
//   (`/api/probe`, incompressible bytes, larger blocks until one takes long enough to time);
// - the H.264 decoder: a short clip of blocky panning terrain, made here with WebCodecs'
//   VideoEncoder at the size in question, then decoded at the stream's own pace, frame by frame,
//   with the same decoder settings the game uses. Decode time adds straight to the delay.
'use strict';

const TUNE_VERSION = 1;
const TUNE_SIZES = [[640, 360], [768, 432], [960, 540], [1280, 720], [1600, 900], [1920, 1080]];
// A decoder slower than this per frame at a size rules that size out (and frame rates where it
// takes more than half the frame).
const DECODE_OK_MS = 7;
const TUNE_CODECS = ['avc1.42E02A', 'avc1.42E028', 'avc1.42E01F'];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const median = (a) => { if (!a.length) return null; const s = a.slice().sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
const quantile = (a, p) => { if (!a.length) return null; const s = a.slice().sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.floor(p * s.length))]; };
const pixels = (w) => w * w * 9 / 16;

// The bitrate a picture needs to stay clean in a moving world, in kbit/s: 6 Mbit/s at 1280x720,
// 60 fps (the server's Auto), less than in proportion for more pixels or frames (each frame
// differs less from the one before at a higher rate, and big pictures compress better).
function needKbps(w, fps) {
  return Math.round(6000 * Math.pow(pixels(w) / pixels(1280), 0.85) * Math.pow(fps / 60, 0.6));
}

// The host's own time per frame (render, grab, encode), from latency.log: about 10.5 ms at
// 960x540 and 14 at 1280x720, 60 fps.
const hostMs = (w) => 6 + 8 * pixels(w) / pixels(1280);

const tune = {
  measured: null,
  running: null,
  // What was picked last, for the watcher's step down.
  last: null,
  status: '',
  onchange: null,

  screenKey() {
    const dpr = window.devicePixelRatio || 1;
    return `${Math.max(screen.width, screen.height)}x${Math.min(screen.width, screen.height)}@${dpr}`;
  },

  // The saved measurement when it is from today and this screen, else a fresh one.
  measure(force = false) {
    if (this.running) return this.running;
    if (!force && !this.measured) {
      try {
        const saved = JSON.parse(store.get('mcrs.tune') || 'null');
        if (saved && saved.v === TUNE_VERSION && saved.screen === this.screenKey() && Date.now() - saved.at < 86400e3) this.measured = saved;
      } catch {}
    }
    if (!force && this.measured) return Promise.resolve(this.measured);
    if (force) store.set('mcrs.tune.cap', null);
    this.running = this.run().then((m) => {
      this.measured = m;
      this.running = null;
      store.set('mcrs.tune', JSON.stringify(m));
      return m;
    }).finally(() => { this.running = null; });
    return this.running;
  },

  say(text) { this.status = text; if (this.onchange) this.onchange(); },

  async run() {
    const m = { v: TUNE_VERSION, at: Date.now(), screen: this.screenKey() };
    const dpr = window.devicePixelRatio || 1;
    m.screenW = Math.round(Math.max(screen.width, screen.height) * dpr);
    this.say('Measuring your screen...');
    m.hz = await refreshRate();
    this.say('Measuring your connection...');
    Object.assign(m, await network());
    this.say('Measuring your video decoder...');
    m.decode = await decoderTimes(Math.min(m.screenW, 1920));
    this.say('');
    return m;
  },

  // The settings for this device, connection and server; `info` is /api/info.
  choose(m, info) {
    const why = [];
    const maxW = (info && info.width) || 1920;
    const maxFps = (info && info.fps) || 120;
    const maxBr = (info && info.max_bitrate_kbps) || 30000;
    const sizes = TUNE_SIZES.map((s) => s[0]).filter((w) => w <= maxW);
    if (!sizes.length) sizes.push(TUNE_SIZES[0][0]);

    // The screen: the largest picture that is not wider than it.
    let wCap = sizes.filter((w) => w <= m.screenW * 1.05).pop() || sizes[0];

    // The decoder: measured sizes as they came out, larger ones by their share of pixels.
    const decodeAt = (w) => {
      const d = m.decode || {};
      if (d[w]) return d[w].p50;
      const known = Object.keys(d).map(Number).filter((k) => d[k] && d[k].p50 != null);
      if (!known.length) return d.hardware === false ? 4 * pixels(w) / pixels(1280) : 2;
      const k = known.reduce((a, b) => (Math.abs(b - w) < Math.abs(a - w) ? b : a));
      return d[k].p50 * pixels(w) / pixels(k);
    };
    while (wCap > sizes[0] && decodeAt(wCap) > DECODE_OK_MS) {
      why.push(`decoder ${decodeAt(wCap).toFixed(1)} ms a frame at ${wCap} wide`);
      wCap = sizes[sizes.indexOf(wCap) - 1];
    }

    // The line: what the picture may use of it, with room for key frames and other traffic.
    let budget = m.kbps ? m.kbps * (m.jitter > 30 ? 0.5 : 0.65) : 6000;
    budget = Math.min(budget, maxBr);
    if (!m.kbps) why.push('line speed unknown');

    // Frame rates: the screen's, 60 at least; a host serving several players keeps them at 60.
    const busy = info && info.players && info.players.length >= 1;
    let rates = [120, 90, 60].filter((f) => f <= maxFps && f <= Math.max(60, m.hz + 5));
    if (busy && rates.length > 1) { rates = rates.filter((f) => f <= 60); why.push('the host is serving others, so 60 fps'); }
    if (!rates.length) rates = [Math.min(60, maxFps)];

    // Size first at 60 fps, then frames on top while the line and the decoder allow.
    const cap = store.num('mcrs.tune.cap', 0);
    let w = sizes.filter((x) => x <= wCap && needKbps(x, 60) * 0.75 <= budget).pop();
    let fps = Math.min(60, maxFps);
    if (w == null) {
      // A slow line: the smallest picture, then fewer frames.
      w = sizes[0];
      for (const f of [45, 30]) if (needKbps(w, fps) * 0.75 > budget && f <= maxFps) fps = f;
      why.push('a slow line');
    } else {
      for (const f of rates) {
        if (f > fps && needKbps(w, f) * 0.75 <= budget && decodeAt(w) <= 500 / f) { fps = f; break; }
      }
    }
    // Until a minute has run smooth on this device (`mcrs.tune.held`), start no higher than
    // 1280x720 at 60 fps: players who saw a slow first picture left before the watcher stepped
    // down, and the watcher raises it after a minute with room to spare.
    if (store.get('mcrs.tune.held') !== '1' && (w > 1280 || fps > 60)) {
      w = sizes.filter((x) => x <= Math.min(w, 1280)).pop() || sizes[0];
      fps = Math.min(fps, 60);
      why.push('starting at 720p60 until a minute runs smooth');
    }
    // Steps down the watcher took after a session that could not keep up.
    for (let i = 0; i < cap; i++) {
      if (fps > 60) fps = 60;
      else if (sizes.indexOf(w) > 0) w = sizes[sizes.indexOf(w) - 1];
      else if (fps > 30) fps = Math.max(30, fps - 15);
    }
    if (cap) why.push(`stepped down ${cap}x after a session that fell behind`);
    const br = Math.max(1000, Math.min(Math.round(Math.min(needKbps(w, fps), budget) / 100) * 100, maxBr));

    // The delay to expect: the way there and back, the host, half a frame of waiting for the next
    // one, sending the frame, and decoding it.
    const rtt = m.rtt || 0;
    const send = (br / fps) / Math.max(m.kbps || br * 2, 1) * 1000;
    const est = Math.round(rtt + hostMs(w) + 500 / fps + send + decodeAt(w));
    this.last = { w, fps, br, est, rtt: Math.round(rtt), why };
    return this.last;
  },

  // One line about a pick, for the card and the panel.
  describe(p) {
    if (!p) return this.status || 'Measuring this device and connection...';
    const h = Math.round(p.w * 9 / 16);
    let s = `${p.w} x ${h}, ${p.fps} fps, ${(p.br / 1000).toFixed(1)} Mbit/s. Expected delay about ${p.est} ms`;
    s += p.rtt ? `, ${p.rtt} of it the distance to the host.` : '.';
    return s;
  },

  // The measurements behind a pick, for the Advanced section.
  details(m) {
    if (!m) return '';
    const parts = [`screen ${m.screenW} px at ${m.hz} Hz`];
    if (m.rtt != null) parts.push(`ping ${Math.round(m.rtt)} ms (spread ${Math.round(m.jitter)})`);
    if (m.kbps) parts.push(`line ${(m.kbps / 1000).toFixed(1)} Mbit/s`);
    const d = m.decode || {};
    for (const k of Object.keys(d)) if (d[k] && d[k].p50 != null) parts.push(`decode ${d[k].p50.toFixed(1)} ms at ${k} wide${d.hardware ? ' (hardware)' : ''}`);
    if (d.error) parts.push(`decoder test: ${d.error}`);
    return 'Measured: ' + parts.join(', ') + '.';
  },

  // The 5-second stats while playing. Two windows in a row that fall behind (frames short of the
  // rate, frames skipped, or decoding past half a frame) call `step('down')`; twelve in a row,
  // a minute, with room to spare (every frame shown, none skipped, decoding under a third of a
  // frame) call `step('up')`. picture.js changes the running picture on either.
  strikes: 0,
  windows: 0,
  roomy: 0,
  watch(s, pic, step) {
    this.windows++;
    if (this.windows <= 2) return; // the world is still loading, or the picture just changed
    const frame = 1000 / pic.fps;
    const decode = s.decode_ms && s.decode_ms.p50;
    const behind = s.fps_shown < pic.fps * 0.8 || s.skipped_frames >= 3 || (decode != null && decode > frame * 0.6);
    const room = s.fps_shown >= pic.fps * 0.95 && !s.skipped_frames && (decode == null || decode < frame / 3);
    this.strikes = behind ? this.strikes + 1 : 0;
    this.roomy = room ? this.roomy + 1 : 0;
    if (this.strikes >= 2) {
      this.resetWatch();
      step('down');
    } else if (this.roomy >= 12) {
      this.resetWatch();
      step('up');
    }
  },
  resetWatch() { this.strikes = 0; this.windows = 0; this.roomy = 0; },
};

// The display's refresh rate from 30 animation frames; 60 when the page is hidden (no frames).
async function refreshRate() {
  if (document.visibilityState !== 'visible') return 60;
  const times = [];
  await new Promise((done) => {
    const step = (t) => { times.push(t); if (times.length < 31) requestAnimationFrame(step); else done(); };
    requestAnimationFrame(step);
    setTimeout(done, 1500);
  });
  const gaps = times.slice(1).map((t, i) => t - times[i]);
  const g = median(gaps);
  if (!g) return 60;
  const hz = 1000 / g;
  // The nearest common rate, so 59.94 reads as 60 and 143.9 as 144.
  return [30, 48, 50, 60, 72, 75, 90, 100, 120, 144, 165, 170, 180, 240, 360].reduce((a, b) => (Math.abs(b - hz) < Math.abs(a - hz) ? b : a));
}

// Ping and line speed to the host.
async function network() {
  const out = { rtt: null, jitter: 0, kbps: null };
  try {
    const pings = [];
    for (let i = 0; i < 6; i++) {
      const t = performance.now();
      const r = await fetch('/api/ping', { cache: 'no-store' });
      await r.text();
      if (i) pings.push(performance.now() - t); // the first one opens the connection
    }
    out.rtt = median(pings);
    out.jitter = quantile(pings, 0.9) - quantile(pings, 0.1);
    // Larger blocks until one takes long enough (300 ms) to time well, 8 MB at most.
    for (const kb of [256, 1024, 4096, 8192]) {
      const t = performance.now();
      const r = await fetch(`/api/probe?kb=${kb}&n=${Math.random()}`, { cache: 'no-store' });
      const b = await r.arrayBuffer();
      const ms = performance.now() - t;
      const moving = Math.max(1, ms - out.rtt);
      out.kbps = Math.round(b.byteLength * 8 / moving);
      if (ms > 300) break;
    }
  } catch (e) {
    console.warn('tune: network', e);
  }
  return out;
}

// The first decoder settings this browser takes, as client.js picks them: hardware first.
async function decoderConfig(codec) {
  for (const c of [{ codec, optimizeForLatency: true, hardwareAcceleration: 'prefer-hardware' }, { codec, optimizeForLatency: true }]) {
    try { if ((await VideoDecoder.isConfigSupported(c)).supported) return c; } catch {}
  }
  return null;
}

// A clip of `n` frames at `w` wide: blocky terrain-like noise panning sideways, as turning in game
// looks to an encoder. Returns the encoded chunks and the codec, or null.
async function makeClip(w, n) {
  const h = Math.round(w * 9 / 16 / 2) * 2;
  if (!('VideoEncoder' in window) || typeof OffscreenCanvas === 'undefined') return null;
  let codec = null;
  for (const c of TUNE_CODECS) {
    try {
      const cfg = { codec: c, width: w, height: h, bitrate: needKbps(w, 60) * 1000, framerate: 60, latencyMode: 'realtime', avc: { format: 'annexb' } };
      if ((await VideoEncoder.isConfigSupported(cfg)).supported) { codec = cfg; break; }
    } catch {}
  }
  if (!codec) return null;
  // The world: 16-pixel blocks of earthy colors, twice as wide as the picture to pan across.
  const tile = new OffscreenCanvas(Math.ceil(w * 2 / 16), Math.ceil(h / 16));
  const tc = tile.getContext('2d');
  const img = tc.createImageData(tile.width, tile.height);
  let seed = 12345;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) >>> 0) / 4294967296);
  for (let i = 0; i < img.data.length; i += 4) {
    const base = rnd() < 0.5 ? [96, 140, 60] : [120, 86, 58];
    const k = 0.7 + rnd() * 0.5;
    img.data[i] = base[0] * k; img.data[i + 1] = base[1] * k; img.data[i + 2] = base[2] * k; img.data[i + 3] = 255;
  }
  tc.putImageData(img, 0, 0);
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext('2d');
  ctx.imageSmoothingEnabled = false;
  const chunks = [];
  let failed = null;
  const enc = new VideoEncoder({
    output: (chunk) => { const data = new Uint8Array(chunk.byteLength); chunk.copyTo(data); chunks.push({ key: chunk.type === 'key', data }); },
    error: (e) => { failed = e; },
  });
  enc.configure(codec);
  const started = performance.now();
  for (let i = 0; i < n && !failed; i++) {
    ctx.drawImage(tile, -i * 6, 0, tile.width * 16, tile.height * 16);
    const f = new VideoFrame(canvas, { timestamp: i * 16667 });
    enc.encode(f, { keyFrame: i === 0 });
    f.close();
    // A slow software encoder at this size: stop here and judge from what came out.
    if (performance.now() - started > 2500) break;
    if (enc.encodeQueueSize > 4) await sleep(4);
  }
  try { await enc.flush(); } catch {}
  enc.close();
  return failed || !chunks.length ? null : { chunks, codec: codec.codec };
}

// Decodes a clip at 60 frames a second and times each frame from decode() to its picture.
async function timeDecode(clip) {
  const config = await decoderConfig(clip.codec);
  if (!config) return null;
  const sent = new Map();
  const times = [];
  const d = new VideoDecoder({
    output: (frame) => { const t = sent.get(frame.timestamp); if (t != null) times.push(performance.now() - t); frame.close(); },
    error: () => {},
  });
  d.configure(config);
  for (let i = 0; i < clip.chunks.length; i++) {
    const c = clip.chunks[i];
    sent.set(i, performance.now());
    d.decode(new EncodedVideoChunk({ type: c.key ? 'key' : 'delta', timestamp: i, data: c.data }));
    await sleep(16);
  }
  try { await Promise.race([d.flush(), sleep(1000)]); } catch {}
  try { d.close(); } catch {}
  // The first few frames warm the decoder up.
  const steady = times.slice(3);
  return { p50: median(steady), p90: quantile(steady, 0.9), hardware: config.hardwareAcceleration === 'prefer-hardware' };
}

// Decode times at the size the screen wants, and at 1280 too when that one is slow.
async function decoderTimes(screenW) {
  const out = {};
  if (!('VideoDecoder' in window)) return { error: 'no WebCodecs' };
  const first = TUNE_SIZES.map((s) => s[0]).filter((x) => x <= screenW * 1.05).pop() || 640;
  for (const w of first > 1280 ? [first, 1280] : [first]) {
    try {
      const clip = await makeClip(w, 32);
      if (!clip) { out.error = 'no H.264 encoder here to make a test clip'; break; }
      const t = await timeDecode(clip);
      if (!t) { out.error = 'no H.264 decoder'; break; }
      out[w] = { p50: Math.round(t.p50 * 10) / 10, p90: Math.round(t.p90 * 10) / 10 };
      out.hardware = t.hardware;
      if (t.p50 <= DECODE_OK_MS) break;
    } catch (e) {
      out.error = String(e && e.message || e);
      break;
    }
  }
  if (out.hardware === undefined) {
    // No clip to time: whether the browser has a hardware decoder at all.
    const c = await decoderConfig('avc1.42E02A');
    out.hardware = !!(c && c.hardwareAcceleration);
  }
  return out;
}
