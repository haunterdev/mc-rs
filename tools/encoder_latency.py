"""Feeds raw BGRA frames to ffmpeg at a steady rate and times each frame from the moment ffmpeg
has all of it to its first and last output byte. Isolates the encoder from the game.

python tools/encoder_latency.py --w 1280 --h 720 --fps 60 --seconds 6 -- <extra ffmpeg args>
The default ffmpeg arguments are the viewer's (encode.rs) for h264_amf; extra args go in front
of the output. `--plain` keeps only the input and output, for any encoder in the extra args;
`--source` loops real frames (raw BGRA, e.g. decoded from a stream_probe recording). The 20
warm-up frames run in lockstep, so an encoder that holds a frame until the next one shows as
held_in_lockstep=20/20.
"""
import argparse, statistics, subprocess, threading, time

ap = argparse.ArgumentParser()
ap.add_argument("--w", type=int, default=1280)
ap.add_argument("--h", type=int, default=720)
ap.add_argument("--fps", type=int, default=60)
ap.add_argument("--seconds", type=float, default=6)
ap.add_argument("--br", type=int, default=6000)
ap.add_argument("--pre", default="", help="args before -i (e.g. hardware device init)")
ap.add_argument("--source", help="a raw BGRA file of frames at --w x --h to loop instead of the test pattern")
ap.add_argument("--plain", action="store_true", help="only input, -c:v from the extra args, and output")
ap.add_argument("extra", nargs="*")
a = ap.parse_args()

br = a.br
args = ["ffmpeg", "-hide_banner", "-loglevel", "error"] + (a.pre.split() if a.pre else []) + [
    "-f", "rawvideo", "-pix_fmt", "bgra", "-video_size", f"{a.w}x{a.h}", "-framerate", str(a.fps), "-i", "pipe:0",
    "-fps_mode", "passthrough", "-an", "-c:v", "h264_amf", "-b:v", f"{br}k", "-maxrate", f"{br}k",
    "-bufsize", f"{max(br * 2 // a.fps, 100)}k", "-bf", "0", "-usage", "ultralowlatency", "-quality", "speed",
    "-rc", "cbr", "-aud", "1", "-async_depth", "1", "-profile:v", "constrained_baseline", "-preanalysis", "0",
    "-enforce_hrd", "1"] + a.extra + ["-flush_packets", "1", "-f", "h264", "pipe:1"]
if a.plain:
    args = ["ffmpeg", "-hide_banner", "-loglevel", "error"] + (a.pre.split() if a.pre else []) + [
        "-f", "rawvideo", "-pix_fmt", "bgra", "-video_size", f"{a.w}x{a.h}", "-framerate", str(a.fps), "-i", "pipe:0",
        "-fps_mode", "passthrough", "-an"] + a.extra + ["-flush_packets", "1", "-f", "h264", "pipe:1"]
p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=0)

px = bytearray()
for x in range(a.w):
    v = x * 255 // a.w
    px += bytes((v, (v * 3) & 255, 255 - v, 255))
base = bytes(px) * a.h
frames = [base[s:] + base[:s] for s in ((k * 4 * 37) % len(base) for k in range(8))]
if a.source:
    size = a.w * a.h * 4
    with open(a.source, "rb") as f:
        data = f.read()
    frames = [data[i : i + size] for i in range(0, len(data) - size + 1, size)]
written = []
auds = []  # when each access unit delimiter (one per frame) arrived
lock = threading.Condition()

def reader():
    tail = b""
    while True:
        b = p.stdout.read(1 << 16)
        if not b:
            return
        now = time.perf_counter()
        buf = tail + b
        n = buf.count(b"\x00\x00\x01\x09")
        tail = buf[-3:]
        if n:
            with lock:
                auds.extend([now] * n)
                lock.notify_all()

t = threading.Thread(target=reader, daemon=True)
t.start()

def write(k):
    p.stdin.write(frames[k % len(frames)])
    p.stdin.flush()
    written.append(time.perf_counter())

# Warm up in lockstep: each frame waits for its own output (up to 2 s), which drains anything
# ffmpeg queued while the encoder started. A frame the encoder holds until the next one comes
# in shows here as a timeout.
held = 0
for k in range(20):
    write(k)
    with lock:
        if not lock.wait_for(lambda: len(auds) >= len(written), timeout=0.1 if k > 3 else 2):
            held += 1
with lock:
    lock.wait_for(lambda: len(auds) >= len(written), timeout=0.5)
skip = len(written)
n = int(a.seconds * a.fps)
start = time.perf_counter()
for k in range(n):
    target = start + k / a.fps
    while time.perf_counter() < target:
        time.sleep(0.0005)
    write(skip + k)
time.sleep(0.3)
cpu = ""
try:
    import psutil
    c = psutil.Process(p.pid).cpu_times()
    cpu = f" cpu_pct_of_one_core={(c.user + c.system) / (time.perf_counter() - start) * 100:.0f}"
except Exception as e:
    cpu = f" cpu=? ({type(e).__name__})"
p.stdin.close()
p.wait()
t.join(1)
first = [(auds[i] - written[i]) * 1000 for i in range(skip + 5, min(len(auds), len(written)))]
med = lambda v: statistics.median(v) if v else float("nan")
print(f"RESULT written={len(written)} auds={len(auds)} held_in_lockstep={held}/20 aud_ms p50={med(first):.1f} "
      f"p95={sorted(first)[int(len(first) * .95)] if first else 0:.1f} {a.w}x{a.h}@{a.fps}{cpu} extra={' '.join(a.extra)} pre={a.pre}")
