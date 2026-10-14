"""Talks to a running mc-rs-server's console window from another shell (Windows).

    python tools/server_console.py say "Server restarting in about 30 seconds"
    python tools/server_console.py read [lines]       print the end of the window's text
    python tools/server_console.py stop               Ctrl+C: the server saves and exits

`say` types any console command (with or without the slash) followed by Enter. `--pid N` picks the
server when more than one runs; by default it is the only mc-rs-server.exe.
"""
from __future__ import annotations

import argparse
import ctypes
import subprocess
import sys
from ctypes import wintypes

k32 = ctypes.WinDLL("kernel32", use_last_error=True)


class KEY_EVENT_RECORD(ctypes.Structure):
    _fields_ = [("bKeyDown", wintypes.BOOL), ("wRepeatCount", wintypes.WORD),
                ("wVirtualKeyCode", wintypes.WORD), ("wVirtualScanCode", wintypes.WORD),
                ("UnicodeChar", wintypes.WCHAR), ("dwControlKeyState", wintypes.DWORD)]


class _Event(ctypes.Union):
    _fields_ = [("KeyEvent", KEY_EVENT_RECORD), ("_pad", ctypes.c_byte * 16)]


class INPUT_RECORD(ctypes.Structure):
    _fields_ = [("EventType", wintypes.WORD), ("Event", _Event)]


class COORD(ctypes.Structure):
    _fields_ = [("X", ctypes.c_short), ("Y", ctypes.c_short)]


class SMALL_RECT(ctypes.Structure):
    _fields_ = [("L", ctypes.c_short), ("T", ctypes.c_short), ("R", ctypes.c_short), ("B", ctypes.c_short)]


class CSBI(ctypes.Structure):
    _fields_ = [("size", COORD), ("cursor", COORD), ("attr", wintypes.WORD),
                ("window", SMALL_RECT), ("max", COORD)]


def server_pid() -> int:
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq mc-rs-server.exe", "/FO", "CSV", "/NH"],
                         capture_output=True, text=True).stdout
    pids = [int(line.split('","')[1]) for line in out.splitlines() if line.startswith('"mc-rs-server')]
    if len(pids) != 1:
        sys.exit(f"{len(pids)} mc-rs-server processes running; pass --pid")
    return pids[0]


def attach(pid: int) -> None:
    k32.FreeConsole()
    if not k32.AttachConsole(pid):
        sys.exit(f"AttachConsole({pid}) failed: {ctypes.get_last_error()}")


def open_console(name: str) -> int:
    return k32.CreateFileW(name, 0xC0000000, 3, None, 3, 0, None)


def say(text: str) -> bool:
    text += "\r"
    recs = (INPUT_RECORD * (len(text) * 2))()
    for i, ch in enumerate(text):
        for j, down in enumerate((True, False)):
            k = recs[i * 2 + j]
            k.EventType = 1
            k.Event.KeyEvent.bKeyDown = down
            k.Event.KeyEvent.wRepeatCount = 1
            k.Event.KeyEvent.wVirtualKeyCode = 0x0D if ch == "\r" else 0
            k.Event.KeyEvent.UnicodeChar = ch
    h = open_console("CONIN$")
    n = wintypes.DWORD()
    ok = k32.WriteConsoleInputW(h, recs, len(recs), ctypes.byref(n))
    k32.CloseHandle(h)
    return bool(ok) and n.value == len(recs)


def read(lines: int) -> str:
    h = open_console("CONOUT$")
    info = CSBI()
    k32.GetConsoleScreenBufferInfo(h, ctypes.byref(info))
    w = info.size.X
    rows = []
    for y in range(max(0, info.cursor.Y - lines), info.cursor.Y + 1):
        buf = ctypes.create_unicode_buffer(w)
        got = wintypes.DWORD()
        k32.ReadConsoleOutputCharacterW(h, buf, w, COORD(0, y), ctypes.byref(got))
        rows.append(buf.value[: got.value].rstrip())
    k32.CloseHandle(h)
    return "\n".join(rows)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["say", "read", "stop"])
    ap.add_argument("arg", nargs="?", default="")
    ap.add_argument("--pid", type=int)
    a = ap.parse_args()
    pid = a.pid or server_pid()
    # Our own output goes to the caller's handles, which stay valid after FreeConsole.
    out = sys.stdout
    attach(pid)
    if a.action == "say":
        ok = say(a.arg)
    elif a.action == "read":
        out.write(read(int(a.arg or 20)) + "\n")
        ok = True
    else:
        k32.SetConsoleCtrlHandler(None, True)
        ok = bool(k32.GenerateConsoleCtrlEvent(0, 0))
    k32.FreeConsole()
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
