"""Isolated server + real viewer. A guest joins over /ws, uploads a PNG through /api/media, then
asks for a smaller picture mid-game; prints the replies, video frame sizes around the change, and
what the server stored."""
import json, os, socket, struct, subprocess, sys, tempfile, time, urllib.request, zlib
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tools"))
from server_e2e import Ws, post, spare_port  # noqa


def png(w, h):
    raw = b"".join(b"\x00" + b"".join(bytes((x * 255 // w, y * 255 // h, 128)) for x in range(w)) for y in range(h))
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


port = spare_port()
data = tempfile.mkdtemp(prefix="mcrs-pic-")
viewer = os.path.join(ROOT, "target", "release", "viewer.exe")
json.dump({"viewer": viewer, "tunnel": False, "port": port, "seed": 42, "width": 640, "height": 360, "fps": 30, "render_distance": 4}, open(os.path.join(data, "server.json"), "w"))
log = open(os.path.join(data, "server.log"), "w")
srv = subprocess.Popen([os.path.join(ROOT, "target", "release", "mc-rs-server.exe"), "--data", data, "--no-tunnel"], stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
time.sleep(1.5)
try:
    base = f"http://127.0.0.1:{port}"
    g = post(base, "/api/guest", {"name": ""})
    ws = Ws("127.0.0.1", port, f"/ws?token={g['token']}&w=640&fps=30")
    ws.s.settimeout(0.1)
    start = time.time()
    first = None
    steps = []
    frames = []  # (time, key, size)

    def upload():
        req = urllib.request.Request(f"{base}/api/media?token={g['token']}&name=probe.png", data=png(64, 32), method="POST")
        try:
            print("upload:", urllib.request.urlopen(req).read().decode())
        except urllib.error.HTTPError as e:
            print("upload:", e.code, e.read().decode())

    while time.time() - start < 50:
        try:
            m = ws.recv()
        except socket.timeout:
            m = None
        except EOFError:
            print("closed"); break
        now = time.time() - start
        if isinstance(m, str):
            print(f"{now:6.2f} text {m[:200]}")
        elif m and m[0] == 1:
            frames.append((now, m[1], len(m)))
            if first is None:
                first = now
                print(f"{now:6.2f} first video")
                t0 = now + 8
                steps += [[t0, bytes([4, 0, 5, 0]), "click to focus", False],
                          [t0 + 0.5, bytes([3]) + struct.pack("<ff", 0.0, 3000.0), "look down", False],
                          [t0 + 1, "upload", "upload a png", False],
                          [t0 + 6, json.dumps({"t": "picture", "w": 320, "fps": 20, "br": 800}), "ask 320 wide at 20 fps", False],
                          [t0 + 14, json.dumps({"t": "picture", "w": 960, "fps": 30, "br": 0}), "ask 960 wide at 30 fps", False],
                          [t0 + 20, b"", "end", False]]
        elif m and m[0] != 2:
            print(f"{now:6.2f} bin tag {m[0]} len {len(m)}")
        for s in steps:
            if not s[3] and now >= s[0]:
                s[3] = True
                print(f"{now:6.2f} {s[2]} (frames so far {len(frames)})")
                if s[1] == "upload":
                    upload()
                    print(f"{time.time() - start:6.2f} upload returned")
                elif s[1]:
                    ws.send(s[1])
        if steps and all(s[3] for s in steps):
            break
    # A second player joins after the painting is up: its viewer should fetch the file.
    cache = os.path.join(os.environ["APPDATA"], "mc-rs", "media-cache", "0ac615401ca46524.png")
    if os.path.exists(cache):
        os.remove(cache)
    g2 = post(base, "/api/guest", {"name": ""})
    ws2 = Ws("127.0.0.1", port, f"/ws?token={g2['token']}&w=640&fps=30")
    ws2.s.settimeout(0.1)
    t2 = time.time()
    while time.time() - t2 < 12:
        try:
            ws.recv(); ws2.recv()
        except socket.timeout:
            pass
    print("second player's cache has the file:", os.path.exists(cache), cache)
    ws2.close()
    ws.close()
finally:
    time.sleep(1)
    srv.stdin.write(b"stop\n"); srv.stdin.flush()
    try:
        srv.wait(10)
    except Exception:
        srv.kill()

# Frames per second and key frames in 2-second buckets.
if frames:
    t0 = frames[0][0]
    buckets = {}
    for t, key, n in frames:
        b = int((t - t0) // 2)
        c = buckets.setdefault(b, [0, 0, 0])
        c[0] += 1; c[1] += 1 if key else 0; c[2] += n
    for b in sorted(buckets):
        c = buckets[b]
        print(f"t+{b*2:3d}s  {c[0]/2:5.1f} fps  keys {c[1]}  avg {c[2]//max(1,c[0])} bytes")
for root, dirs, files in os.walk(data):
    for f in files:
        p = os.path.join(root, f)
        print("stored", os.path.relpath(p, data), os.path.getsize(p))
print("data", data)
text = open(os.path.join(data, "server.log"), errors="replace").read()
print("\n".join(l for l in text.splitlines() if any(k in l.lower() for k in ("picture", "media", "paint", "hang", "error", "panic", "warn")))[-4000:])
