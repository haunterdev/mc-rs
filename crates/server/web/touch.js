// Touch controls for phones and tablets, where there is no keyboard and no pointer lock. Every
// control speaks the input the desktop page sends (proto::Input): the stick on the left half holds
// W, A, S and D (and Ctrl for sprint at its edge), a drag on the right half moves the mouse, a
// tap there is a right click (place) and a press and hold a held left click (break), the buttons
// are keys, and a tapped hotbar slot is its number key. While the options menu is open a touch is
// the menu's pointer, as the mouse is on a desktop.
'use strict';

const touch = {
  // Auto shows the controls only where the main pointer is a finger, never on a desktop.
  auto: matchMedia('(pointer: coarse)').matches && ('ontouchstart' in window || navigator.maxTouchPoints > 0),
  mode: store.get('mcrs.touch') || 'auto',
  on: false,
  look: store.num('mcrs.touch.look', 2.5),
  size: store.num('mcrs.touch.size', 1),
  // The game's focus as the page last showed it; -1 until the picture is up.
  state: -1,
  sneak: false,
  roles: new Map(),
};

const layer = $('#touch');
const STICK_RADIUS = 56;
const STICK_DEAD = 0.3;
const SPRINT_AT = 0.95;
const HOLD_MS = 280;
const SLOP_PX = 10;
// A key held or let go for less than a game tick (50 ms) and the frame around it can be missed
// by the tick that reads it, which is where jump and its double tap for flying are read.
const KEY_MIN_MS = 70;

function applyTouchMode() {
  touch.on = touch.mode === 'on' || (touch.mode === 'auto' && touch.auto);
  $('#game').classList.toggle('touch', touch.on);
  layer.classList.toggle('hidden', !touch.on);
  layer.style.setProperty('--t', touch.size);
  if (touch.on && touch.state >= 0) setFocus(focusState);
}

// ---------- keys and buttons ----------

const touchKeys = new Map();

// Presses and releases queue per key, each one at least KEY_MIN_MS after the one before, so a
// quick double tap reaches the game as two presses. The stick's keys skip the queue (`now`).
function touchKey(code, down, now = false) {
  let k = touchKeys.get(code);
  if (!k) touchKeys.set(code, (k = { down: false, queue: [], at: 0, timer: 0 }));
  if (now) {
    clearTimeout(k.timer);
    k.timer = 0;
    k.queue = down === k.down ? [] : [down];
    k.at = 0;
  } else {
    const last = k.queue.length ? k.queue[k.queue.length - 1] : k.down;
    if (down === last) return;
    k.queue.push(down);
  }
  pumpKey(code, k);
}

function pumpKey(code, k) {
  if (k.timer || !k.queue.length) return;
  const wait = k.at + KEY_MIN_MS - performance.now();
  if (wait > 0) {
    k.timer = setTimeout(() => { k.timer = 0; pumpKey(code, k); }, wait);
    return;
  }
  k.down = k.queue.shift();
  k.at = performance.now();
  if (inGame()) {
    event(key(k.down ? 1 : 2, code));
    flush();
  }
  pumpKey(code, k);
}

function touchButton(b, down) {
  if (!inGame()) return;
  event(new Uint8Array([down ? 4 : 5, b]));
  flush();
}

// Lets go of everything the controls hold: on leaving the game for a menu or chat, or losing the
// page.
touch.release = () => {
  for (const [p, r] of touch.roles) endRole(p, r, null);
  touch.roles.clear();
  for (const code of touchKeys.keys()) touchKey(code, false);
  touch.sneak = false;
  $('#t-sneak').classList.remove('on');
  stickIdle();
};

touch.focus = (state) => {
  if (state !== 1) touch.release();
  touch.state = state;
  layer.dataset.focus = state;
};

touch.fullscreen = () => {
  fullscreen().then((ok) => {
    if (ok && screen.orientation && screen.orientation.lock) screen.orientation.lock('landscape').catch(() => {});
    fit();
  });
};

// ---------- the hotbar ----------
// The picture is a video, so the slots are where GuiIngame.renderHotbar puts them in the picture
// (crates/viewer/src/hotbar.rs): the 182 x 22 bar at the automatic GUI scale (hand::gui_scale, the
// largest factor that keeps 320 x 240), centered at the bottom, slots 20 apart from x + 1.

function hotbarSlot(x, y) {
  const w = canvas.width, h = canvas.height;
  let k = 1;
  while (w / (k + 1) >= 320 && h / (k + 1) >= 240) k++;
  const sw = Math.ceil(w / k), sh = Math.ceil(h / k);
  const r = canvas.getBoundingClientRect();
  const s = r.width / w;
  const left = r.left + (Math.floor(sw / 2) - 91 + 1) * k * s;
  const top = r.top + (sh - 22) * k * s;
  const slot = 20 * k * s;
  // A finger is wider than a slot is tall at phone sizes: take a little above the bar too.
  if (y < top - slot * 0.4 || y > r.bottom || x < left - k * s || x > left + 9 * slot + k * s) return -1;
  return Math.min(8, Math.max(0, Math.floor((x - left) / slot)));
}

// ---------- the stick ----------

const stick = $('#t-stick'), knob = $('#t-knob');

function stickIdle() {
  stick.classList.remove('active');
  stick.style.left = stick.style.top = '';
  knob.style.transform = '';
}

function stickMove(r, x, y) {
  const R = STICK_RADIUS * touch.size;
  let dx = x - r.ox, dy = y - r.oy;
  const d = Math.hypot(dx, dy);
  if (d > R) { dx *= R / d; dy *= R / d; }
  knob.style.transform = `translate(${dx}px, ${dy}px)`;
  const m = Math.min(d, R) / R;
  // Eight equal directions past the dead zone.
  const nx = d ? dx / Math.min(d, R) : 0, ny = d ? dy / Math.min(d, R) : 0;
  const live = m >= STICK_DEAD;
  const want = {
    KeyW: live && ny < -0.38,
    KeyS: live && ny > 0.38,
    KeyA: live && nx < -0.38,
    KeyD: live && nx > 0.38,
  };
  want.ControlLeft = want.KeyW && m >= SPRINT_AT;
  for (const code in want) touchKey(code, want[code], true);
  stick.classList.toggle('sprint', want.ControlLeft);
}

// ---------- one finger, one role ----------

function canvasPoint(e) {
  const r = canvas.getBoundingClientRect();
  return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height];
}

function startRole(e) {
  const btn = e.target.closest('[data-key]');
  if (btn && layer.contains(btn)) return { kind: 'button', el: btn, code: btn.dataset.key };
  if (touch.state === 0) return { kind: 'play' };
  if (touch.state === 2) return { kind: 'menu' };
  if (touch.state !== 1) return null;
  const slot = hotbarSlot(e.clientX, e.clientY);
  if (slot >= 0) return { kind: 'slot', code: `Digit${slot + 1}` };
  if (e.clientX < innerWidth / 2) return { kind: 'stick' };
  return { kind: 'look' };
}

layer.addEventListener('pointerdown', (e) => {
  e.preventDefault();
  if (!inGame()) return;
  if (!audio.ctx || audio.ctx.state !== 'running') audio.start();
  const r = startRole(e);
  if (!r) return;
  r.x = r.x0 = e.clientX;
  r.y = r.y0 = e.clientY;
  r.t0 = performance.now();
  touch.roles.set(e.pointerId, r);
  try { layer.setPointerCapture(e.pointerId); } catch {}
  switch (r.kind) {
    case 'button':
      r.el.classList.add('down');
      if (r.code === 'Sneak') {
        touch.sneak = !touch.sneak;
        r.el.classList.toggle('on', touch.sneak);
        touchKey('ShiftLeft', touch.sneak);
      } else if (r.code !== 'Chat') touchKey(r.code, true);
      break;
    case 'play': touchButton(0, true); break;
    case 'menu':
      cursor = canvasPoint(e);
      flush();
      touchButton(0, true);
      break;
    case 'slot': touchKey(r.code, true); break;
    case 'stick': {
      // The stick centers under the thumb, kept whole on the screen.
      const R = STICK_RADIUS * touch.size;
      r.ox = Math.max(R + 8, Math.min(innerWidth / 2 - R, e.clientX));
      r.oy = Math.max(R + 8, Math.min(innerHeight - R - 8, e.clientY));
      stick.classList.add('active');
      stick.style.left = `${r.ox}px`;
      stick.style.top = `${r.oy}px`;
      stickMove(r, e.clientX, e.clientY);
      break;
    }
    case 'look':
      r.timer = setTimeout(() => {
        if (r.moved || touch.roles.get(e.pointerId) !== r) return;
        r.breaking = true;
        touchButton(0, true);
      }, HOLD_MS);
      break;
  }
});

layer.addEventListener('pointermove', (e) => {
  const r = touch.roles.get(e.pointerId);
  if (!r) return;
  e.preventDefault();
  const dx = e.clientX - r.x, dy = e.clientY - r.y;
  r.x = e.clientX;
  r.y = e.clientY;
  if (!r.moved && Math.hypot(r.x - r.x0, r.y - r.y0) > SLOP_PX) r.moved = true;
  if (r.kind === 'stick') stickMove(r, r.x, r.y);
  else if (r.kind === 'menu') cursor = canvasPoint(e);
  else if (r.kind === 'look' && inGame()) {
    moveX += dx * touch.look;
    moveY += dy * touch.look;
  }
});

function endRole(id, r, e) {
  clearTimeout(r.timer);
  switch (r.kind) {
    case 'button':
      r.el.classList.remove('down');
      if (r.code === 'Chat') {
        // Only a tap that ends on the button, so a slip off it does not pop up the keyboard.
        if (e && r.el.contains(document.elementFromPoint(e.clientX, e.clientY))) { touch.release(); openChat(''); }
      } else if (r.code !== 'Sneak') touchKey(r.code, false);
      break;
    case 'play': touchButton(0, false); break;
    case 'menu':
      if (e) cursor = canvasPoint(e);
      flush();
      touchButton(0, false);
      break;
    case 'slot': touchKey(r.code, false); break;
    case 'stick':
      for (const code of ['KeyW', 'KeyA', 'KeyS', 'KeyD', 'ControlLeft']) touchKey(code, false, true);
      stickIdle();
      break;
    case 'look':
      if (r.breaking) touchButton(0, false);
      else if (e && !r.moved && performance.now() - r.t0 < HOLD_MS) { touchButton(2, true); touchButton(2, false); }
      break;
  }
}

function pointerEnd(e) {
  const r = touch.roles.get(e.pointerId);
  if (!r) return;
  touch.roles.delete(e.pointerId);
  endRole(e.pointerId, r, e.type === 'pointerup' ? e : null);
}
layer.addEventListener('pointerup', pointerEnd);
layer.addEventListener('pointercancel', pointerEnd);

// No page zoom, scroll, long-press menu or text selection under the controls (touch-action does
// most of it; Safari also wants its gesture events stopped).
// Safari zooms on a pinch or a double tap whatever the viewport tag says, so on a touch device
// both are stopped everywhere on the page, not only under the controls.
const phone = () => touch.on || touch.auto;
for (const t of ['gesturestart', 'gesturechange', 'gestureend']) {
  document.addEventListener(t, (e) => { if (phone()) e.preventDefault(); }, { passive: false });
}
let lastTouchEnd = 0;
document.addEventListener('touchend', (e) => {
  const now = performance.now();
  const field = e.target.closest && e.target.closest('input, select, textarea');
  if (phone() && !field && now - lastTouchEnd < 350) e.preventDefault();
  lastTouchEnd = now;
}, { passive: false });
document.addEventListener('dblclick', (e) => { if (phone()) e.preventDefault(); }, { passive: false });
addEventListener('blur', () => { if (touch.on) touch.release(); });
document.addEventListener('visibilitychange', () => { if (touch.on && document.hidden) touch.release(); });

// Chat on a phone: putting the keyboard away without sending closes the box.
$('#chatbox').addEventListener('blur', () => { if (touch.on && chatOpen()) closeChat(); });

// ---------- the touch settings (in the Settings panel), kept for the next visit ----------

const tMode = $('#t-mode'), tLook = $('#t-look'), tSize = $('#t-size');
$('#touchprefs').classList.toggle('hidden', !(touch.auto || navigator.maxTouchPoints > 0 || touch.mode === 'on'));
tMode.value = touch.mode;
tLook.value = touch.look;
tSize.value = touch.size;
tMode.addEventListener('change', () => {
  touch.mode = tMode.value;
  store.set('mcrs.touch', touch.mode);
  if (touch.on) touch.release();
  applyTouchMode();
});
tLook.addEventListener('input', () => { touch.look = +tLook.value; store.set('mcrs.touch.look', touch.look); });
tSize.addEventListener('input', () => {
  touch.size = +tSize.value;
  store.set('mcrs.touch.size', touch.size);
  layer.style.setProperty('--t', touch.size);
});

applyTouchMode();
