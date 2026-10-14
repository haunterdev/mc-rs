"""Plays a running mc-rs-server for a few seconds the way a browser would, and records what comes
back: the H.264 stream (to a file ffmpeg can decode), the audio volume, and the frame rate.

    python tools/stream_probe.py [--url http://127.0.0.1:25580] [--name probe] [--password probe1]
        [--seconds 15] [--w 1280] [--fps 60] [--hz 60] [--out probe.h264] [--walk]
    python tools/stream_probe.py --start target/release [--seed 42] [--rd 8] ...

It signs in (signing up the name the first time), clicks into the picture, optionally holds W
to walk, and prints one line per second and a summary. From a thread of its own, on its own
clock as a browser's mouse is, it sends a small mouse turn with an input sequence number
(`proto::Input::Seq`) `--hz` times a second (60, a browser sending one batch per display frame)
and times how long until a frame carrying that sequence arrives: the delay from input to
picture, minus the browser's decode. The summary splits that delay into the host pipeline the
frame carries (update start to encoded unit) and the rest (the wait for the next update plus
the relays). `ffmpeg -i probe.h264 -frames:v 1 -update 1 last.png` style commands turn the
recording into pictures.

`--start DIR` starts `DIR/mc-rs-server.exe` itself on a spare port with a fresh data folder, no
tunnel and `DIR/viewer.exe`, plays it, and stops it again, so two builds compare on equal terms.
"""
import argparse
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from server_e2e import Ws, post, spare_port  # noqa: E402


def key(tag, code):
    c = code.encode()
    return bytes([tag, len(c)]) + c


def start_server(bin_dir, a):
    port = spare_port()
    data = tempfile.mkdtemp(prefix="mc-rs-probe-")
    exe = ".exe" if os.name == "nt" else ""
    viewer = os.path.abspath(os.path.join(bin_dir, "viewer" + exe))
    cfg = {"viewer": viewer, "tunnel": False, "port": port, "seed": a.seed, "render_distance": a.rd,
           "width": a.w, "height": a.w * 9 // 16, "fps": a.fps}
    with open(os.path.join(data, "server.json"), "w") as f:
        json.dump(cfg, f)
    log = open(os.path.join(data, "server.log"), "w")
    srv = subprocess.Popen([os.path.abspath(os.path.join(bin_dir, "mc-rs-server" + exe)), "--data", data, "--no-tunnel"],
                           stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
    base = f"http://127.0.0.1:{port}"
    stop = time.monotonic() + 30
    while time.monotonic() < stop:
        try:
            post(base, "/api/guest", {"name": ""})
            break
        except Exception:
            time.sleep(0.25)
    return srv, data, log, base


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:25580")
    ap.add_argument("--start", default="", help="a folder holding mc-rs-server.exe and viewer.exe to start on a spare port")
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--rd", type=int, default=8, help="render distance for --start")
    ap.add_argument("--name", default="probe")
    ap.add_argument("--password", default="probe1")
    ap.add_argument("--seconds", type=float, default=15)
    ap.add_argument("--w", type=int, default=1280)
    ap.add_argument("--fps", type=int, default=60)
    ap.add_argument("--hz", type=float, default=60, help="mouse turns a second, each with its own sequence number")
    ap.add_argument("--out", default="probe.h264")
    ap.add_argument("--walk", action="store_true")
    ap.add_argument("--br", type=int, default=0, help="kbit/s, 0 for Auto")
    a = ap.parse_args()
    srv = None
    if a.start:
        srv, data, log, a.url = start_server(a.start, a)
    try:
        run(a)
    finally:
        if srv:
            srv.terminate()
            srv.wait()
            log.close()
            shutil.rmtree(data, ignore_errors=True)


def run(a):
    u = urllib.parse.urlparse(a.url)
    r = post(a.url, "/api/login", {"name": a.name, "password": a.password})
    if not r.get("ok"):
        sys.exit(f"sign-in failed: {r}")
    ws = Ws(u.hostname, u.port or 80, f"/ws?token={r['token']}&w={a.w}&fps={a.fps}&br={a.br}")
    ws.s.settimeout(None)
    send_lock = threading.Lock()

    def send(b):
        with send_lock:
            ws.send(b)

    out = open(a.out, "wb")
    start = time.perf_counter()
    first_video = None
    tick = start
    frames = keys = vbytes = abytes = 0
    total_frames = total_keys = 0
    peak = 0
    texts = []
    sent_at = {}
    last_seen = 0
    lags, hosts, rests, gaps = [], [], [], []
    window_lags = []
    last_frame = None
    clicked = threading.Event()
    done = threading.Event()

    def turner():
        # The browser's clock, not the host's: turns go out at their own rate whatever the frames do.
        clicked.wait()
        seq = 0
        period = 1.0 / a.hz
        due = time.perf_counter()
        while not done.is_set():
            seq += 1
            sent_at[seq] = time.perf_counter()
            try:
                send(bytes([3]) + struct.pack("<ff", 2.0, 0.0) + bytes([9]) + struct.pack("<I", seq))
            except OSError:
                return
            due += period
            wait = due - time.perf_counter()
            if wait > 0:
                time.sleep(wait)
            else:
                due = time.perf_counter()

    threading.Thread(target=turner, daemon=True).start()
    walking = False
    while time.perf_counter() - start < a.seconds:
        try:
            m = ws.recv()
        except EOFError:
            print("socket closed")
            break
        now = time.perf_counter()
        if isinstance(m, str):
            t = json.loads(m)
            texts.append(t)
            print(f"{now - start:6.2f}s text {m}")
        elif m:
            if m[0] == 1:
                if first_video is None:
                    first_video = now - start
                flags, fseq, host_us = struct.unpack("<BII", m[1:10])
                hosts.append(host_us / 1000)
                if last_frame is not None and clicked.is_set():
                    gaps.append((now - last_frame) * 1000)
                last_frame = now
                if fseq > last_seen and fseq in sent_at:
                    lag = (now - sent_at[fseq]) * 1000
                    lags.append(lag)
                    rests.append(lag - host_us / 1000)
                    window_lags.append(lag)
                    last_seen = fseq
                out.write(m[10:])
                frames += 1
                total_frames += 1
                vbytes += len(m)
                if flags & 1:
                    keys += 1
                    total_keys += 1
            elif m[0] == 2:
                abytes += len(m) - 1
                pcm = struct.unpack(f"<{(len(m) - 1) // 2}h", m[1:1 + (len(m) - 1) // 2 * 2])
                peak = max(peak, max((abs(v) for v in pcm), default=0))
        if first_video is not None and not clicked.is_set() and now - start > first_video + 2:
            send(bytes([4, 0, 5, 0]))
            clicked.set()
        if a.walk and clicked.is_set() and not walking and now - start > first_video + 3:
            walking = True
            send(key(1, "KeyW"))
        if now - tick >= 1.0:
            wl = sorted(window_lags)
            lag_txt = f", input to frame {wl[len(wl) // 2]:.0f} ms" if wl else ""
            window_lags = []
            print(f"{now - start:6.2f}s video {frames} fps {keys} key {vbytes * 8 / 1e6:.2f} Mbit/s, audio {abytes} bytes, peak {peak}{lag_txt}")
            tick = now
            frames = keys = vbytes = abytes = 0
    done.set()
    if walking:
        send(key(2, "KeyW"))
    ws.close()
    out.close()
    secs = time.perf_counter() - start - (first_video or 0)

    def pc(v, p):
        v = sorted(v)
        return round(v[min(len(v) - 1, int(p * len(v)))], 1) if v else None
    print(f"SUMMARY first_video_s={first_video} frames={total_frames} keys={total_keys} avg_fps={total_frames / max(secs, 0.001):.1f} audio_peak={peak} "
          f"input_to_frame_ms p50={pc(lags, 0.5)} p95={pc(lags, 0.95)} n={len(lags)} host_pipeline_ms p50={pc(hosts, 0.5)} p95={pc(hosts, 0.95)} "
          f"rest_ms p50={pc(rests, 0.5)} p95={pc(rests, 0.95)} frame_gap_ms p50={pc(gaps, 0.5)} p95={pc(gaps, 0.95)} out={a.out}")


if __name__ == "__main__":
    main()
