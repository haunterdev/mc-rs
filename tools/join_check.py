"""Runs the real viewer's native join against a local mc-rs-server.

    python tools/check.py build viewer server
    python tools/join_check.py [--seconds 20] [path to mc-rs-server] [path to mc-rs-viewer]

Starts the server on a spare port with a fresh data folder and no tunnel, signs a player in at
`/api/login`, then runs the shipping viewer with `--join` on that token. `--smoke` keeps the
window hidden off-screen and takes the settings from the command line rather than the user's
file, so the run is mute and repeatable; the join branch owns the frame loop either way. Checks
the welcome line, that the process is still alive with no panic at the end of the run, and that
the server logged the player. Prints PASS or the failed checks; exits nonzero on failure.
`MCRS_RAMGUARD` wraps the viewer run the way `tools/dist.py` wraps a build. Needs only the
Python standard library.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from server_e2e import post, spare_port  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
EXE_SUFFIX = ".exe" if os.name == "nt" else ""
RAMGUARD = os.environ.get("MCRS_RAMGUARD")
GUARD = ["--squeeze", "--start-above", "1.0", "--pause-below", "1.5", "--resume-above", "2.2",
         "--kill-below", "0.3", "--io", "normal", "--vr-guard", "--job-mem", "0", "--gpu-low"]


def main():
    argv = sys.argv[1:]
    seconds = 20
    if argv[:1] == ["--seconds"]:
        seconds, argv = int(argv[1]), argv[2:]
    # Windows CreateProcess takes native separators only, so a path given with slashes resolves here.
    server = os.path.abspath(argv[0]) if argv else str(ROOT / "target" / "release" / ("mc-rs-server" + EXE_SUFFIX))
    viewer = (os.path.abspath(argv[1]) if len(argv) > 1
              else str(ROOT / "target" / "release" / ("viewer" + EXE_SUFFIX)))
    failures = []

    def check(cond, what):
        print(("ok   " if cond else "FAIL ") + what, flush=True)
        if not cond:
            failures.append(what)

    port = spare_port()
    data = tempfile.mkdtemp(prefix="mc-rs-join-check-")
    try:
        with open(os.path.join(data, "server.json"), "w") as f:
            json.dump({"viewer": str(ROOT / "tools" / "fake_viewer.py"), "tunnel": False,
                       "port": port, "seed": 42, "max_players": 2}, f)
        srv_log_path = os.path.join(data, "server.log")
        srv_log = open(srv_log_path, "w")
        srv = subprocess.Popen([server, "--data", data], stdout=srv_log, stderr=subprocess.STDOUT)
        try:
            base = f"http://127.0.0.1:{port}"
            r, stop = {}, time.monotonic() + 20
            while time.monotonic() < stop:
                try:
                    r = post(base, "/api/login", {"name": "join_check", "password": "pass1"})
                    break
                except Exception:
                    time.sleep(0.25)
            check(r.get("ok"), "the player signs in at /api/login")
            token = r.get("token", "")
            if not token:
                srv_log.close()
                return report(failures)

            cmd = [viewer, "--join", f"ws://127.0.0.1:{port}/native", "--smoke", str(seconds), "--workers", "1"]
            if RAMGUARD:
                cmd = [sys.executable, RAMGUARD, "run", *GUARD, "--", *cmd]
            env = dict(os.environ, MCRS_JOIN_TOKEN=token)
            view = subprocess.Popen(cmd, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT, text=True, errors="replace")
            try:
                out, _ = view.communicate(timeout=seconds + 20)
                died = view.returncode
            except subprocess.TimeoutExpired:
                view.terminate()
                try:
                    out, _ = view.communicate(timeout=15)
                except subprocess.TimeoutExpired:
                    view.kill()
                    out, _ = view.communicate()
                died = None
            print("     viewer said:", json.dumps(out.strip().splitlines()[-6:]), flush=True)
            check("joined ws://127.0.0.1:%d/native as join_check (player" % port in out, "the viewer is welcomed by name")
            check("panicked" not in out, "no panic")
            check(died is None, f"the viewer is still running after {seconds}s (exit {died})")
            srv_log.flush()
            log = open(srv_log_path, errors="replace").read()
            check("join_check" in log, "the server logged the player")
        finally:
            srv.terminate()
            srv.wait()
            srv_log.close()
    finally:
        # The server's log can still be held by Windows for a moment after it exits.
        shutil.rmtree(data, ignore_errors=True)
    return report(failures)


def report(failures):
    if failures:
        print(f"FAIL ({len(failures)}): " + "; ".join(failures), flush=True)
        return 1
    print("PASS", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
