// What the page remembers between visits, in localStorage under "mcrs.*": the sign-in token
// (never the password), the last name, the picture, frame rate and bitrate, the delay readout,
// the volume and the touch controls. Every access is wrapped: with storage blocked (a private
// window, strict cookie settings) the page still works and keeps the values for this visit only.
'use strict';

const store = {
  mem: new Map(),
  get(k) {
    try {
      const v = localStorage.getItem(k);
      if (v != null) return v;
    } catch {}
    return this.mem.has(k) ? this.mem.get(k) : null;
  },
  set(k, v) {
    if (v == null) this.mem.delete(k); else this.mem.set(k, String(v));
    try { v == null ? localStorage.removeItem(k) : localStorage.setItem(k, v); } catch {}
  },
  // A number, or `def` when unset or not a number.
  num(k, def) {
    const v = this.get(k);
    const n = v == null || v === '' ? NaN : +v;
    return Number.isFinite(n) ? n : def;
  },
  // An on/off stored as '1' or '0'.
  flag(k, def) {
    const v = this.get(k);
    return v == null ? def : v === '1';
  },
  setFlag(k, on) { this.set(k, on ? '1' : '0'); },
};
