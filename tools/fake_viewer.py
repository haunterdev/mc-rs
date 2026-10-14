#!/usr/bin/env python3
"""A stand-in for `viewer --stream`, for tools/server_e2e.py: speaks the crates/proto frames.

It says hello with its session token, reads the welcome, reports focus, sends one player
state and one block change, then a short run of fake video and audio frames, and prints every
frame it receives as `got tag N len M` (the server logs it with the player's name), the clock,
teleports and game mode decoded as `time T cycle C snap S`, `teleport X Y Z YAW PITCH FLAGS` and
`gamemode M`.
"""
import json
import socket
import struct
import sys
import threading
import time

args = sys.argv[1:]
opts = dict(zip(args[::2], args[1::2]))
host, port = opts["--stream"].rsplit(":", 1)
sock = socket.create_connection((host, int(port)))


def frame(tag, body):
    sock.sendall(struct.pack("<IB", len(body) + 1, tag) + body)


def read_exact(n):
    out = b""
    while len(out) < n:
        chunk = sock.recv(n - len(out))
        if not chunk:
            raise EOFError
        out += chunk
    return out


def read_frame():
    n = struct.unpack("<I", read_exact(4))[0]
    body = read_exact(n)
    return body[0], body[1:]


frame(1, opts["--session"].encode())
tag, body = read_frame()
welcome = json.loads(body)
print("welcome", json.dumps(welcome, sort_keys=True), "size", opts["--size"], flush=True)


def reader():
    try:
        while True:
            t, b = read_frame()
            print("got tag", t, "len", len(b), flush=True)
            if t == 120:
                time_, bits = struct.unpack("<qB", b)
                print("time", time_, "cycle", bits & 1, "snap", bits >> 1 & 1, flush=True)
            elif t == 121:
                print("teleport", *struct.unpack("<dddffB", b), flush=True)
            elif t == 122:
                print("gamemode", b[0], flush=True)
    except (EOFError, OSError):
        print("eof", flush=True)


threading.Thread(target=reader, daemon=True).start()
frame(7, bytes([1]))
pos = welcome["pos"] or [8.5, 70.0, 8.5]
frame(4, struct.pack("<dddffBBH", pos[0] + 1, pos[1], pos[2], 10.0, 5.0, 1, 0, 16))
frame(5, struct.pack("<iiiH", 1, 2, 3, 16))
for i in range(30):
    key = 1 if i % 10 == 0 else 0
    nal = b"\x00\x00\x00\x01\x65" if key else b"\x00\x00\x00\x01\x41"
    # proto::VideoHeader: flags, input sequence, host pipeline microseconds.
    frame(2, struct.pack("<BII", key, 0, 5000) + b"\x00\x00\x00\x01\x09\xf0" + nal + bytes(100))
    frame(3, bytes(960 * 4))
    time.sleep(0.05)
time.sleep(60)
