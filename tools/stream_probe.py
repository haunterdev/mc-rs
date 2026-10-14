"""Plays a running mc-rs-server for a few seconds the way a browser would, and records what comes
back: the H.264 stream (to a file ffmpeg can decode), the audio volume, and the frame rate.

    python tools/stream_probe.py [--url http://127.0.0.1:25580] [--name probe] [--password probe1]
        [--seconds 15] [--w 1280] [--fps 60] [--out probe.h264] [--walk]

It signs in (signing up the name the first time), clicks into the picture, optionally holds W
to walk, and prints one line per second and a summary. Ten times a second it sends a small
mouse turn with an input sequence number (`proto::Input::Seq`) and times how long until a frame
carrying that sequence arrives: the delay from input to picture, minus the browser's decode. `ffmpeg -i probe.h264 -frames:v 1
-update 1 last.png` style commands turn the recording into pictures.
"""
import argparse
import json
import os
import socket
import struct
import sys
import time
import urllib.parse

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from server_e2e import Ws, post  # noqa: E402


def key(tag, code):
    c = code.encode()
    return bytes([tag, len(c)]) + c


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:25580")
    ap.add_argument("--name", default="probe")
    ap.add_argument("--password", default="probe1")
    ap.add_argument("--seconds", type=float, default=15)
    ap.add_argument("--w", type=int, default=1280)
    ap.add_argument("--fps", type=int, default=60)
    ap.add_argument("--out", default="probe.h264")
    ap.add_argument("--walk", action="store_true")
    ap.add_argument("--br", type=int, default=0, help="kbit/s, 0 for Auto")
    a = ap.parse_args()
    u = urllib.parse.urlparse(a.url)
    r = post(a.url, "/api/login", {"name": a.name, "password": a.password})
    if not r.get("ok"):
        sys.exit(f"sign-in failed: {r}")
    ws = Ws(u.hostname, u.port or 80, f"/ws?token={r['token']}&w={a.w}&fps={a.fps}&br={a.br}")
    ws.s.settimeout(0.2)
    out = open(a.out, "wb")
    start = time.time()
    first_video = None
    tick = start
    frames = keys = vbytes = abytes = 0
    total_frames = total_keys = 0
    peak = 0
    clicked = walking = False
    texts = []
    seq = 0
    sent_at = {}
    last_seen = 0
    last_turn = 0.0
    lags, hosts = [], []
    window_lags = []
    while time.time() - start < a.seconds:
        try:
            m = ws.recv()
        except socket.timeout:
            m = None
        except EOFError:
            print("socket closed")
            break
        now = time.time()
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
                if fseq > last_seen and fseq in sent_at:
                    lags.append((now - sent_at[fseq]) * 1000)
                    window_lags.append(lags[-1])
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
        if first_video is not None and not clicked and now - start > first_video + 2:
            clicked = True
            ws.send(bytes([4, 0, 5, 0]))
        if clicked and now - last_turn > 0.1:
            last_turn = now
            seq += 1
            sent_at[seq] = time.time()
            ws.send(bytes([3]) + struct.pack("<ff", 2.0, 0.0) + bytes([9]) + struct.pack("<I", seq))
        if a.walk and clicked and not walking and now - start > first_video + 3:
            walking = True
            ws.send(key(1, "KeyW"))
        if now - tick >= 1.0:
            wl = sorted(window_lags)
            lag_txt = f", input to frame {wl[len(wl) // 2]:.0f} ms" if wl else ""
            window_lags = []
            print(f"{now - start:6.2f}s video {frames} fps {keys} key {vbytes * 8 / 1e6:.2f} Mbit/s, audio {abytes} bytes, peak {peak}{lag_txt}")
            tick = now
            frames = keys = vbytes = abytes = 0
    if walking:
        ws.send(key(2, "KeyW"))
    ws.close()
    out.close()
    secs = time.time() - start - (first_video or 0)
    def pc(v, p):
        v = sorted(v)
        return round(v[min(len(v) - 1, int(p * len(v)))], 1) if v else None
    print(f"SUMMARY first_video_s={first_video} frames={total_frames} keys={total_keys} avg_fps={total_frames / max(secs, 0.001):.1f} audio_peak={peak} "
          f"input_to_frame_ms p50={pc(lags, 0.5)} p95={pc(lags, 0.95)} n={len(lags)} host_pipeline_ms p50={pc(hosts, 0.5)} p95={pc(hosts, 0.95)} out={a.out}")


if __name__ == "__main__":
    main()
