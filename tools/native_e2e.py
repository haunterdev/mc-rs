"""End-to-end check of a native client (the launcher's Join) on mc-rs-server, next to a streamed
player.

    cargo build -p server
    python tools/native_e2e.py [path to mc-rs-server]

Starts the server on a spare port with a fresh data folder, a cap of two players and
tools/fake_viewer.py as the streamed player's viewer. A native client written here (a WebSocket on
`/native` carrying crates/proto frames) signs in with a token from `/api/login`, as the launcher
does, and the check covers: a wrong password refused at the login, the socket refused without a
token or with a bad one, the welcome with the world seed, the streamed player's join, state and
edit reaching the native client, the native client's state and edit reaching the streamed
player's viewer, an edit out of reach dropped, chat both ways, a slash command's answer, the
clock, the player cap counting the native client, the leave, and the native player's position
and edit saved. Prints PASS or the failed checks; exits nonzero on failure. Needs only the Python
standard library.
"""
import json
import os
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from server_e2e import EXE, ROOT, Player, Ws, post, spare_port  # noqa: E402

V_STATE, V_BLOCKS, V_TEXT = 4, 5, 40
S_WELCOME, S_JOIN, S_LEAVE, S_STATE, S_BLOCKS, S_TIME, S_TEXT = 101, 103, 104, 105, 106, 120, 140


def frame(tag, body):
    return struct.pack("<IB", len(body) + 1, tag) + body


def split(msg):
    out = []
    while msg:
        n = struct.unpack("<I", msg[:4])[0]
        out.append((msg[4], msg[5:4 + n]))
        msg = msg[4 + n:]
    return out


def state(pos, yaw=0.0, pitch=0.0, flags=1):
    return struct.pack("<dddffBBH", pos[0], pos[1], pos[2], yaw, pitch, flags, 0, 0)


def blocks(changes):
    return b"".join(struct.pack("<iiiH", x, y, z, s) for (x, y, z), s in changes)


class Native:
    """A native client: what `viewer --join` sends and receives, minus the rendering."""

    def __init__(self, port, token):
        self.ws = Ws("127.0.0.1", port, "/native", f"Authorization: Bearer {token}\r\n")
        self.frames, self.texts = [], []
        self.closed = False
        threading.Thread(target=self.run, daemon=True).start()

    def run(self):
        try:
            while True:
                m = self.ws.recv()
                if isinstance(m, str):
                    continue
                for tag, body in split(m):
                    if tag == S_TEXT:
                        self.texts.append(json.loads(body))
                    else:
                        self.frames.append((tag, body))
        except (EOFError, OSError):
            self.closed = True

    def send(self, tag, body):
        self.ws.send(frame(tag, body))

    def has(self, tag, pred=lambda b: True):
        return any(t == tag and pred(b) for t, b in list(self.frames))

    def said(self, pred):
        return any(pred(t) for t in list(self.texts))


def wait_for(cond, secs=10.0):
    stop = time.time() + secs
    while time.time() < stop:
        if cond():
            return True
        time.sleep(0.05)
    return False


def refused(port, headers):
    try:
        Ws("127.0.0.1", port, "/native", headers).close()
        return False
    except RuntimeError as e:
        return " 401 " in str(e).split("\r\n")[0]


def main():
    port = spare_port()
    data = tempfile.mkdtemp(prefix="mcrs-native-e2e-")
    fake = os.path.join(ROOT, "tools", "fake_viewer.py")
    viewer = fake
    if os.name == "nt":
        viewer = os.path.join(data, "fake_viewer.cmd")
        with open(viewer, "w") as f:
            f.write(f'@python "{fake}" %*\n')
    else:
        os.chmod(fake, 0o755)
    with open(os.path.join(data, "server.json"), "w") as f:
        json.dump({"viewer": viewer, "tunnel": False, "port": port, "seed": 42, "max_players": 2}, f)
    log_path = os.path.join(data, "server.log")
    failures = []

    def check(cond, what):
        print(("ok   " if cond else "FAIL ") + what, flush=True)
        if not cond:
            failures.append(what)

    log = open(log_path, "w")
    srv = subprocess.Popen([EXE, "--data", data], stdout=log, stderr=subprocess.STDOUT)
    time.sleep(1.5)
    try:
        base = f"http://127.0.0.1:{port}"
        r = post(base, "/api/login", {"name": "native_p", "password": "secret1"})
        check(r.get("ok"), "native player signs up at /api/login")
        token = r.get("token", "")
        bad = post(base, "/api/login", {"name": "native_p", "password": "wrong"})
        check(not bad.get("ok") and "Wrong" in bad.get("error", ""), "a wrong password is refused")
        check(refused(port, ""), "/native without a token is refused (401)")
        check(refused(port, "Authorization: Bearer 0123456789abcdef\r\n"), "/native with a bad token is refused (401)")

        n = Native(port, token)
        check(wait_for(lambda: n.has(S_WELCOME)), "native client welcomed")
        w = next((json.loads(b) for t, b in n.frames if t == S_WELCOME), {})
        check(w.get("seed") == 42 and w.get("name") == "native_p", f"welcome carries the seed and the name ({w})")

        streamer = Player(port, "streamer", False)
        streamer.duration = 7
        streamer.thread.start()
        check(wait_for(lambda: n.has(S_JOIN, lambda b: b[4:] == b"streamer")), "native client sees the streamed player join")
        check(wait_for(lambda: n.has(S_STATE, lambda b: struct.unpack("<ddd", b[4:28]) == (9.5, 70.0, 8.5))), "native client gets the streamed player's state")
        check(wait_for(lambda: n.has(S_BLOCKS, lambda b: b == blocks([((1, 2, 3), 16)]))), "native client gets the streamed player's edit")

        n.send(V_STATE, state([20.5, 70.0, 8.5], 45.0, 10.0))
        n.send(V_BLOCKS, blocks([((21, 70, 8), 16), ((1000, 70, 8), 16)]))
        n.send(V_TEXT, json.dumps({"t": "chat", "text": "hello from native"}).encode())
        check(wait_for(lambda: n.said(lambda t: t.get("t") == "chat" and t.get("text") == "hello from native" and t.get("from") == "native_p")), "native chat comes back to everyone")
        check(n.said(lambda t: t.get("t") == "chat" and t.get("text") == "streamer joined the game"), "native client gets the join line in chat")
        n.send(V_TEXT, json.dumps({"t": "chat", "text": "/seed"}).encode())
        check(wait_for(lambda: n.said(lambda t: t.get("t") == "chat" and t.get("text") == "Seed: 42")), "a slash command from the native client answers it in chat")
        check(n.has(S_TIME), "native client gets the server clock")

        r3 = post(base, "/api/login", {"name": "third_p", "password": "secret3"})
        third = Native(port, r3["token"])
        check(wait_for(lambda: third.said(lambda t: t.get("t") == "closed" and "full" in t.get("text", ""))), "a third player is refused: the cap counts native clients")

        streamer.thread.join()
        check(wait_for(lambda: n.has(S_LEAVE)), "native client sees the streamed player leave")
        check(any(t["t"] == "chat" and t.get("text") == "hello from native" for t in streamer.texts), "the streamed player's page gets the native chat")
        check(any(t["t"] == "players" and t["list"] == ["native_p", "streamer"] for t in streamer.texts), "the player list shows both")
        print("     native client got tags", sorted({t for t, _ in n.frames}), "and texts", sorted({t.get("t") for t in n.texts}), flush=True)
        n.ws.close()
        time.sleep(1.0)
    finally:
        srv.terminate()
        srv.wait()
        log.close()
    text = open(log_path, encoding="utf-8", errors="replace").read()
    check("[streamer] got tag 103 " in text, "the streamed player's viewer hears of the native player (join)")
    check("[streamer] got tag 105 len 40" in text, "the streamed player's viewer gets the native player's state")
    check("[streamer] got tag 106 len 14" in text, "the streamed player's viewer gets the native edit, the far one dropped")
    check("1 block changes out of reach or not allowed dropped" in text, "the server logs the dropped edit")
    players = json.load(open(os.path.join(data, "players.json")))
    check(players.get("native_p", {}).get("pos") == [20.5, 70.0, 8.5], "the native player's position is saved")
    edits = open(os.path.join(data, "edits.bin"), "rb").read()
    check(blocks([((21, 70, 8), 16)]) in edits and blocks([((1000, 70, 8), 16)]) not in edits, "the native edit is saved, the far one is not")
    if failures:
        print("FAIL")
        print("server log:", log_path)
        sys.exit(1)
    print(f"PASS ({data})")


if __name__ == "__main__":
    main()
