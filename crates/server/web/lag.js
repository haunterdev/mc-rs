// Delay measurement for the mc-rs browser client (client.js): input batch sequence numbers,
// input to frame and to shown, decode time, the corner readout and the 5-second report the
// server appends to latency.log.
'use strict';

// Every input batch carries a sequence number; every frame carries back the newest one it
// reflects. The time from sending a batch to showing its first frame is the delay a player feels
// (input up, render, encode, the way back, decode). Reported every 5 s to the server's
// latency.log and shown in the corner.

const pct = (a, p) => {
  if (!a.length) return null;
  const s = a.slice().sort((x, y) => x - y);
  return Math.round(s[Math.min(s.length - 1, Math.floor(p * s.length))] * 10) / 10;
};

const lag = {
  seq: 0,
  sentAt: new Map(),
  lastSeen: 0,
  decodingSeq: new Map(),
  recvAt: new Map(),
  queue: 0,
  win: { toFrame: [], toShown: [], decode: [], host: [], gap: [], frames: 0, bytes: 0, dropped: 0 },
  lastRecv: 0,
  reset() { this.sentAt.clear(); this.decodingSeq.clear(); this.recvAt.clear(); this.lastSeen = 0; },
  sent() {
    this.seq += 1;
    this.sentAt.set(this.seq, performance.now());
    if (this.sentAt.size > 2000) this.sentAt.delete(this.sentAt.keys().next().value);
    return this.seq;
  },
  received(seq, hostUs) {
    const now = performance.now();
    if (this.lastRecv) this.win.gap.push(now - this.lastRecv);
    this.lastRecv = now;
    this.win.host.push(hostUs / 1000);
    if (seq > this.lastSeen && this.sentAt.has(seq)) {
      this.win.toFrame.push(now - this.sentAt.get(seq));
      this.pendingSeq = seq;
    }
  },
  decoding(ts, seq) {
    this.recvAt.set(ts, performance.now());
    if (this.pendingSeq === seq) { this.decodingSeq.set(ts, seq); this.pendingSeq = 0; }
    if (this.recvAt.size > 300) this.recvAt.delete(this.recvAt.keys().next().value);
  },
  shown(ts) {
    const now = performance.now();
    const r = this.recvAt.get(ts);
    if (r) { this.win.decode.push(now - r); this.recvAt.delete(ts); }
    const seq = this.decodingSeq.get(ts);
    if (seq && seq > this.lastSeen && this.sentAt.has(seq)) {
      this.win.toShown.push(now - this.sentAt.get(seq));
      this.lastSeen = seq;
      this.decodingSeq.delete(ts);
    }
  },
};

const stats = { frames: 0, bytes: 0, dropped: 0, ping: 0 };
let statsShown = store.flag('mcrs.lagstats', true);
let lastLine = '';
setInterval(() => {
  const s = $('#stats');
  if (!inGame()) { s.textContent = ''; return; }
  const w = lag.win;
  const shown = pct(w.toShown.slice(-60), 0.5);
  lastLine = `${shown == null ? '-' : Math.round(shown)} ms lag  ${stats.ping} ms ping  ${stats.frames} fps  ${(stats.bytes * 8 / 1e6).toFixed(1)} Mbit/s` + (stats.dropped ? `  ${stats.dropped} skipped` : '');
  s.textContent = statsShown ? lastLine : '';
  w.frames += stats.frames;
  w.bytes += stats.bytes;
  w.dropped += stats.dropped;
  stats.frames = 0;
  stats.bytes = 0;
  stats.dropped = 0;
}, 1000);

// The 5-second report for latency.log.
setInterval(() => {
  if (!inGame()) return;
  const w = lag.win;
  const sum = (a) => ({ p50: pct(a, 0.5), p95: pct(a, 0.95), n: a.length });
  const report = {
    t: 'stats',
    ping_ms: stats.ping,
    input_to_frame_ms: sum(w.toFrame),
    input_to_shown_ms: sum(w.toShown),
    decode_ms: sum(w.decode),
    host_pipeline_ms: sum(w.host),
    frame_gap_ms: sum(w.gap),
    fps_shown: w.frames / 5,
    kbit_s: Math.round(w.bytes * 8 / 1000 / 5),
    skipped_frames: w.dropped,
    decode_queue_max: lag.queue,
    picture: `${picture.width}x${picture.height}@${picture.fps} ${picture.bitrate}k`,
    codec: video.codec || '',
    desync,
    pointer_locked: locked(),
    touch: touch.on,
    visible: document.visibilityState,
    ua: navigator.userAgent.slice(0, 160),
  };
  send(report);
  tuneWatch(report);
  lag.win = { toFrame: [], toShown: [], decode: [], host: [], gap: [], frames: 0, bytes: 0, dropped: 0 };
  lag.queue = 0;
}, 5000);
