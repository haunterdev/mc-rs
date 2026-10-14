// The picture settings on the sign-in card and in the in-game Settings panel: one line saying
// what is in use (Auto: what tune.js picked for this device and line, and the delay to expect),
// and under Advanced the picture size, frame rate and bitrate by hand. While playing it passes the
// 5-second reports (lag.js) to `tune.watch`, and on Auto changes the picture in the running game:
// a step down when the device or line falls behind, a step back up after a minute with
// room to spare. Apply in the in-game panel changes it the same way, without rejoining. Uses
// client.js's `$`, `info`, `picture`, `video`, `send` and `chatLine`, which exist by the time
// anything here runs (index.html loads this file before client.js).
'use strict';

const SIZES = [[640, 360], [768, 432], [960, 540], [1280, 720], [1600, 900], [1920, 1080]];
const RATES = [30, 45, 60, 90, 120];
const BITRATES = [0, 1000, 2000, 3000, 4000, 6000, 8000, 12000, 16000, 20000, 30000];
// What the next join asks for. Auto fills it from the tuner's pick; by hand, from the dropdowns,
// which are the values kept in `mcrs.w`, `mcrs.fps` and `mcrs.br`.
const conn = {
  auto: store.flag('mcrs.auto', true),
  w: store.num('mcrs.w', 1280),
  fps: store.num('mcrs.fps', 60),
  br: store.num('mcrs.br', 0),
};
let picking = null;

const brLabel = (b) => (b ? `${+(b / 1000).toFixed(1)} Mbit/s` : 'Auto');
const heightOf = (w) => Math.round(w * 9 / 16);

function option(sel, value, label, before = null) {
  const o = document.createElement('option');
  o.value = value;
  o.textContent = label;
  sel.insertBefore(o, before);
}

function fillConnection(root) {
  const size = root.querySelector('.c-size'), rate = root.querySelector('.c-fps'), br = root.querySelector('.c-br');
  size.innerHTML = rate.innerHTML = br.innerHTML = '';
  for (const [w, h] of SIZES) if (!info || w <= info.width) option(size, w, `${w} x ${h}`);
  for (const f of RATES) if (!info || f <= info.fps) option(rate, f, `${f} fps`);
  for (const b of BITRATES) if (!info || b <= (info.max_bitrate_kbps || 30000)) option(br, b, b ? `${b / 1000} Mbit/s` : 'Auto');
  // The tuner's bitrate is rounded to 100 kbit/s, so it gets its own entry in order.
  if (!BITRATES.includes(conn.br)) option(br, conn.br, brLabel(conn.br), [...br.options].find((o) => +o.value > conn.br) || null);
  const pick = (sel, v) => { sel.value = v; if (sel.selectedIndex < 0) sel.selectedIndex = Math.max(0, sel.options.length - 1); };
  pick(size, conn.w);
  pick(rate, conn.fps);
  pick(br, conn.br);
  root.querySelector('.c-auto').checked = conn.auto;
  showPick(root);
}

// The line under the fields and the measurements under Advanced, for what `root` shows now.
function showPick(root) {
  const p = tune.last;
  const line = root.querySelector('.c-line');
  if (root.querySelector('.c-auto').checked) {
    line.textContent = 'Picture: Auto. ' + tune.describe(p);
  } else {
    const w = +root.querySelector('.c-size').value, fps = +root.querySelector('.c-fps').value, br = +root.querySelector('.c-br').value;
    let s = `Picture: set by hand, ${w} x ${heightOf(w)}, ${fps} fps, ${br ? brLabel(br) : 'Auto bitrate'}.`;
    if (p && (p.w !== w || p.fps !== fps)) s += ` Auto would pick ${p.w} x ${heightOf(p.w)}, ${p.fps} fps, about ${p.est} ms delay.`;
    line.textContent = s;
  }
  const why = [tune.details(tune.measured)];
  if (p && p.why.length) why.push('Held back by ' + p.why.join('; ') + '.');
  root.querySelector('.c-why').textContent = why.filter(Boolean).join(' ');
}

function readConnection(root) {
  conn.auto = root.querySelector('.c-auto').checked;
  store.setFlag('mcrs.auto', conn.auto);
  if (conn.auto) {
    if (tune.last) Object.assign(conn, { w: tune.last.w, fps: tune.last.fps, br: tune.last.br });
    return;
  }
  conn.w = +root.querySelector('.c-size').value;
  conn.fps = +root.querySelector('.c-fps').value;
  conn.br = +root.querySelector('.c-br').value;
  store.set('mcrs.w', conn.w);
  store.set('mcrs.fps', conn.fps);
  store.set('mcrs.br', conn.br);
}

// A dropdown changed by hand turns Auto off. `live` keeps changes at once (the sign-in card); the
// in-game panel keeps them on Apply.
function wireConnection(root, live) {
  root.querySelectorAll('.conn select').forEach((s) => s.addEventListener('change', () => {
    root.querySelector('.c-auto').checked = false;
    if (live) readConnection(root);
    showPick(root);
  }));
  root.querySelector('.c-auto').addEventListener('change', (e) => {
    if (e.target.checked && tune.last) {
      root.querySelector('.c-size').value = tune.last.w;
      root.querySelector('.c-fps').value = tune.last.fps;
    }
    if (live) { readConnection(root); fillConnection(root); } else showPick(root);
  });
  root.querySelector('.c-again').addEventListener('click', () => autoPick(true));
}

// Measures (or takes today's measurement) and picks; Auto takes the pick for the next join.
function autoPick(force = false) {
  picking = tune.measure(force).then((m) => {
    const p = tune.choose(m, info);
    if (conn.auto) Object.assign(conn, { w: p.w, fps: p.fps, br: p.br });
    fillConnection($('#login'));
    if ($('#connpanel').classList.contains('hidden')) fillConnection($('#connpanel')); else showPick($('#connpanel'));
    return p;
  }).catch((e) => {
    console.warn('tune', e);
    tune.say('The measurement did not finish, so the last settings stay.');
  });
  return picking;
}
tune.onchange = () => { showPick($('#login')); showPick($('#connpanel')); };

// Holds a join until the measurement is in, at most 12 seconds, saying so on `button`.
async function pictureReady(button) {
  if (!conn.auto || !picking) return;
  const label = button.textContent;
  if (tune.running) { button.textContent = 'Measuring your connection...'; button.disabled = true; }
  await Promise.race([picking, new Promise((r) => setTimeout(r, 12000))]);
  button.textContent = label;
  button.disabled = false;
}

// ---------- while playing ----------

// Asks the running game for a picture; the server answers with what it settled on (client.js).
function setPicture(p) {
  send({ t: 'picture', w: p.w, fps: p.fps, br: p.br });
}

// Each 5-second report from lag.js.
function tuneWatch(s) {
  if (document.visibilityState !== 'visible' || !video.shown) return;
  tune.watch(s, picture, stepPicture);
}

// The watcher's verdict: 'down' when two reports in a row fell behind, 'up' after a minute of
// reports with room to spare. The step is kept in `mcrs.tune.cap` for the next visit. A step up
// that falls behind within 30 seconds goes back down and stays there for this game.
let steppedUpAt = 0;
let upFailed = false;
let toldManual = false;
function stepPicture(dir) {
  if (!tune.measured) return;
  if (!conn.auto) {
    if (dir === 'down' && !toldManual) {
      toldManual = true;
      chatLine(null, `This device is falling behind at ${picture.width} x ${picture.height}, ${picture.fps} fps. Auto in Settings would adjust the picture as you play.`);
    }
    return;
  }
  let cap = store.num('mcrs.tune.cap', 0);
  if (dir === 'down') {
    if (performance.now() - steppedUpAt < 30000) upFailed = true;
    cap = Math.min(6, cap + 1);
  } else {
    if (upFailed || cap === 0) return;
    cap -= 1;
    steppedUpAt = performance.now();
  }
  store.set('mcrs.tune.cap', cap);
  const p = tune.choose(tune.measured, info);
  if (p.w === picture.width && p.fps === picture.fps) return;
  Object.assign(conn, { w: p.w, fps: p.fps, br: p.br });
  chatLine(null, `Picture ${dir === 'down' ? 'lowered' : 'raised'} to ${p.w} x ${heightOf(p.w)}, ${p.fps} fps to ${dir === 'down' ? 'keep up' : 'use the room this device has'}.`);
  setPicture(p);
}
