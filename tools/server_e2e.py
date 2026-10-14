"""End-to-end check of mc-rs-server with two browser players and a stand-in viewer.

    cargo build -p server
    python tools/server_e2e.py [path to mc-rs-server]

Starts the server on a spare port with a fresh data folder and tools/fake_viewer.py as its
viewer, signs two players in over HTTP, opens their game sockets (a small stdlib WebSocket
client below), and checks: sign-up and a refused password, the welcome and the picture size,
video and audio relayed to each browser, chat to both, input forwarded to the viewer, each
viewer told about the other player (join, state, block edits, leave), and the position saved
for the next session. Commands: feedback in the sender's chat only (red for an unknown one),
/say and the console to everyone, the clock, a teleport and the game mode to the right viewer,
the clock every second, new players sent to the world spawn, and the time, rules and game mode
saved. Prints PASS or the failed check; exits nonzero on failure. Needs only the
Python standard library (and Python on PATH for the stand-in viewer on Windows).
"""
import base64
import json
import os
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "debug", "mc-rs-server" + (".exe" if os.name == "nt" else ""))


def spare_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


class Ws:
    """A minimal client: text and binary frames, masked as the protocol asks of clients."""

    def __init__(self, host, port, path, headers=""):
        self.s = socket.create_connection((host, port))
        key = base64.b64encode(os.urandom(16)).decode()
        req = f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n{headers}\r\n"
        self.s.sendall(req.encode())
        head = b""
        while b"\r\n\r\n" not in head:
            head += self.s.recv(1)
        if b" 101 " not in head.split(b"\r\n")[0]:
            raise RuntimeError(head.decode(errors="replace"))

    def send(self, data):
        op = 1 if isinstance(data, str) else 2
        payload = data.encode() if isinstance(data, str) else data
        mask = os.urandom(4)
        n = len(payload)
        head = bytes([0x80 | op])
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        self.s.sendall(head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def _exact(self, n):
        out = b""
        while len(out) < n:
            c = self.s.recv(n - len(out))
            if not c:
                raise EOFError
            out += c
        return out

    def recv(self):
        b0, b1 = self._exact(2)
        n = b1 & 0x7F
        if n == 126:
            n = struct.unpack(">H", self._exact(2))[0]
        elif n == 127:
            n = struct.unpack(">Q", self._exact(8))[0]
        payload = self._exact(n)
        op = b0 & 0x0F
        if op == 8:
            raise EOFError
        return payload.decode() if op == 1 else payload

    def close(self):
        try:
            self.s.close()
        except OSError:
            pass


def post(base, path, body):
    req = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers={"content-type": "application/json"})
    try:
        return json.loads(urllib.request.urlopen(req).read())
    except urllib.error.HTTPError as e:
        return json.loads(e.read())


class Player:
    def __init__(self, port, name, inputs, say=()):
        self.port, self.name, self.inputs, self.say = port, name, inputs, say
        self.texts, self.binary = [], {}
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self):
        base = f"http://127.0.0.1:{self.port}"
        r = post(base, "/api/login", {"name": self.name, "password": "pass1"})
        self.login = r
        ws = Ws("127.0.0.1", self.port, f"/ws?token={r['token']}&w=960&fps=30")
        stop = time.time() + self.duration
        ws.s.settimeout(0.5)
        sent = False
        while time.time() < stop:
            if not sent and time.time() > stop - self.duration + 2:
                if self.inputs:
                    ws.send(bytes([1, 4]) + b"KeyW" + bytes([3]) + struct.pack("<ff", 5, -2))
                for line in self.say:
                    ws.send(json.dumps({"t": "chat", "text": line}))
                sent = True
            try:
                m = ws.recv()
            except socket.timeout:
                continue
            except EOFError:
                break
            if isinstance(m, str):
                self.texts.append(json.loads(m))
            else:
                self.binary[m[0]] = self.binary.get(m[0], 0) + 1
        ws.close()


def main():
    port = spare_port()
    data = tempfile.mkdtemp(prefix="mcrs-e2e-")
    fake = os.path.join(ROOT, "tools", "fake_viewer.py")
    viewer = fake
    if os.name == "nt":
        # The server starts the viewer directly; a .cmd wrapper runs the script with Python.
        viewer = os.path.join(data, "fake_viewer.cmd")
        with open(viewer, "w") as f:
            f.write(f'@python "{fake}" %*\n')
    else:
        os.chmod(fake, 0o755)
    with open(os.path.join(data, "server.json"), "w") as f:
        json.dump({"viewer": viewer, "tunnel": False, "port": port, "seed": 42}, f)
    log_path = os.path.join(data, "server.log")
    failures = []

    def check(cond, what):
        if not cond:
            failures.append(what)

    def run_server():
        log = open(log_path, "w")
        return subprocess.Popen([EXE, "--data", data], stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)

    srv = run_server()
    time.sleep(1.5)
    try:
        base = f"http://127.0.0.1:{port}"
        check(post(base, "/api/login", {"name": "xy_test", "password": "pass1"}).get("ok"), "sign-up")
        check(not post(base, "/api/login", {"name": "xy_test", "password": "nope"}).get("ok"), "a wrong password is refused")
        # Guests: a picked name, a name of their own, never an account's name; the tuner's probes.
        g = post(base, "/api/guest", {"name": ""})
        check(g.get("ok") and g.get("name", "").startswith("Guest"), f"a guest gets a Guest name: {g}")
        g2 = post(base, "/api/guest", {"name": "visitor"})
        check(g2.get("name") == "visitor", f"a guest keeps the name typed: {g2}")
        check(not post(base, "/api/guest", {"name": "xy_test"}).get("ok"), "a guest cannot take an account's name")
        check(not post(base, "/api/guest", {"name": "visitor"}).get("ok"), "two guests cannot share a name")
        me = json.loads(urllib.request.urlopen(f"{base}/api/me?token={g2.get('token')}").read())
        check(me.get("guest") is True and me.get("name") == "visitor", f"/api/me marks a guest: {me}")
        check(json.loads(urllib.request.urlopen(base + "/api/info").read()).get("guests") is True, "/api/info offers guests")
        check(urllib.request.urlopen(base + "/api/ping").read() == b"pong", "/api/ping")
        check(len(urllib.request.urlopen(base + "/api/probe?kb=64").read()) == 65536, "/api/probe size")
        for page in ("/tune.js", "/picture.js"):
            check(b"use strict" in urllib.request.urlopen(base + page).read(), f"{page} is served")
        commands = ["/gamerule doDaylightCycle false", "/time set 6000", "/tp ~ ~5 ~", "/gamemode survival", "/frobnicate", "/say hi all"]
        a = Player(port, "xy_test", True, ["hello from xy_test"] + commands)
        b = Player(port, "friend", False, ["/spawn"])
        a.duration, b.duration = 7, 4
        a.thread.start()
        time.sleep(1.0)
        b.thread.start()
        time.sleep(2.5)
        srv.stdin.write(b"say from the console\n")
        srv.stdin.flush()
        a.thread.join()
        b.thread.join()
        time.sleep(1.0)
        for p in (a, b):
            w = [t for t in p.texts if t["t"] == "welcome"]
            check(w and (w[0]["width"], w[0]["height"], w[0]["fps"]) == (960, 540, 30), f"{p.name}: welcome with 960x540 at 30 fps")
            check(p.binary.get(1, 0) >= 20 and p.binary.get(2, 0) >= 20, f"{p.name}: video and audio relayed ({p.binary})")
            check(any(t["t"] == "chat" and t.get("text") == "hello from xy_test" for t in p.texts), f"{p.name}: chat arrives")
            check(any(t["t"] == "focus" and t.get("state") == 1 for t in p.texts), f"{p.name}: focus relayed")
        check(any(t["t"] == "players" and t["list"] == ["friend", "xy_test"] for t in a.texts), "player list shows both")

        def said(p, text, **kw):
            return any(t["t"] == "chat" and t.get("from") == "" and t.get("text") == text and all(t.get(k) == v for k, v in kw.items()) for t in p.texts)

        check(said(a, "Game rule doDaylightCycle has been updated to false") and said(a, "Set the time to 6000"), "command feedback in the sender's chat")
        check(said(a, "Teleported xy_test to 9.5, 75.0, 8.5"), "/tp feedback with the coordinates")
        check(said(a, "Unknown command. Try /help for a list of commands", color="red", kind="command"), "an unknown command answers in red")
        check(said(a, "Set the time to 6000", color="white"), "feedback is white, not the yellow of joins")
        check(not said(b, "Set the time to 6000"), "feedback goes to the sender only")
        check(said(a, "[xy_test] hi all") and said(b, "[xy_test] hi all"), "/say reaches everyone")
        check(said(a, "[Server] from the console") and said(b, "[Server] from the console"), "the console's /say reaches everyone")
        check(said(b, "Teleported friend to the world spawn (8, 64, 8)"), "anyone may /spawn")
    finally:
        srv.terminate()
        srv.wait()
    log = open(log_path, encoding="utf-8", errors="replace").read()

    def got(name, tag):
        return f"[{name}] got tag {tag} " in log

    check(got("xy_test", 102), "input reached xy_test's viewer")
    check(got("friend", 103) and got("friend", 105) and got("friend", 106), "friend's viewer heard of xy_test (join, state, saved edits)")
    check(got("xy_test", 103) and got("xy_test", 104) and got("xy_test", 106), "xy_test's viewer saw friend join, edit and leave")
    check("[xy_test] time 6000 cycle 0 snap 1" in log, "/time jumps xy_test's clock")
    check(log.count("[friend] time ") >= 3, "the clock goes out every second")
    check("[xy_test] teleport 0.0 5.0 0.0 0.0 0.0 31" in log, "/tp moves xy_test's own viewer, relative")
    check("[xy_test] gamemode 0" in log, "/gamemode reaches the viewer")
    check(log.count("[friend] teleport 8.5 64.0 8.5 0.0 0.0 96") >= 2, "a new player and /spawn go to the world spawn")
    pj = os.path.join(data, "players.json")
    players = json.load(open(pj)) if os.path.exists(pj) else {}
    check(players.get("xy_test", {}).get("pos") == [9.5, 70.0, 8.5], "xy_test's position saved")
    check(players.get("xy_test", {}).get("game_mode") == 0 and players.get("friend", {}).get("game_mode") == 1, "game modes saved")
    world = json.load(open(os.path.join(data, "world.json")))
    check(world.get("time") == 6000 and world.get("rules", {}).get("doDaylightCycle") == "false", f"time and rules saved ({world.get('time')})")
    check(os.path.getsize(os.path.join(data, "edits.bin")) % 14 == 0 and os.path.getsize(os.path.join(data, "edits.bin")) > 0, "edits saved")
    # A second start welcomes the player back where they were.
    srv = run_server()
    time.sleep(1.5)
    try:
        c = Player(port, "xy_test", False)
        c.duration = 3
        c.run()
    finally:
        srv.terminate()
        srv.wait()
    log = open(log_path, encoding="utf-8", errors="replace").read()
    check('"pos": [9.5, 70.0, 8.5]' in log, "the welcome carries the saved position after a restart")
    if failures:
        print("FAIL")
        for f in failures:
            print("  " + f)
        print("server log:", log_path)
        sys.exit(1)
    print(f"PASS ({data})")


if __name__ == "__main__":
    main()
