"""Singleplayer survival for a playtest: a local mc-rs-server plus the viewer joined to it.

    python tools/check.py build server
    python tools/play_local.py [--name NAME] [--seed SEED] [--fresh]

`viewer --seed S --survival` is a HUD preview with no server (docs/RECURRING-ISSUES.md R1), so
drops, the crack overlay, break sounds and chat never happen there. This starts the real server
on a spare port with no tunnel, data in `%APPDATA%\\mc-rs\\local` (kept between runs, `--fresh`
starts a new world), signs NAME in at `/api/login` and runs the viewer windowed with `--join`.
Closing the viewer stops the server. Logs: `server.log` and `viewer.log` in the data folder.
"""
import json
import os
import random
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from server_e2e import post, spare_port  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
EXE = ".exe" if os.name == "nt" else ""


def main():
    argv = sys.argv[1:]
    # Names are 3 to 16 letters, digits or underscores (the server's sign-in rule).
    name, seed, fresh = "xy_local", None, False
    while argv:
        a = argv.pop(0)
        if a == "--name":
            name = argv.pop(0)
        elif a == "--seed":
            seed = int(argv.pop(0))
        elif a == "--fresh":
            fresh = True
        else:
            sys.exit(__doc__)
    data = Path(os.environ["APPDATA"]) / "mc-rs" / "local"
    if fresh and data.exists():
        shutil.rmtree(data)
    data.mkdir(parents=True, exist_ok=True)
    port = spare_port()
    cfg_path = data / "server.json"
    cfg = json.loads(cfg_path.read_text()) if cfg_path.exists() else {}
    cfg.update({"viewer": "", "tunnel": False, "port": port, "bind": "127.0.0.1", "max_players": 4})
    if seed is not None or "seed" not in cfg:
        cfg["seed"] = seed if seed is not None else random.getrandbits(63)
    cfg_path.write_text(json.dumps(cfg, indent=2))

    server = ROOT / "target" / "release" / ("mc-rs-server" + EXE)
    viewer = ROOT / "target" / "release" / ("viewer" + EXE)
    with open(data / "server.log", "w") as srv_log, open(data / "viewer.log", "w") as view_log:
        srv = subprocess.Popen([str(server), "--data", str(data)], stdout=srv_log, stderr=subprocess.STDOUT)
        try:
            base, r, stop = f"http://127.0.0.1:{port}", {}, time.monotonic() + 30
            while time.monotonic() < stop:
                try:
                    r = post(base, "/api/login", {"name": name, "password": "local"})
                    break
                except Exception:
                    time.sleep(0.25)
            if not r.get("token"):
                sys.exit(f"sign-in failed: {r}; see {data / 'server.log'}")
            print(f"server on port {port}, seed {cfg['seed']}, signed in as {name}", flush=True)
            env = dict(os.environ, MCRS_JOIN_TOKEN=r["token"], RUST_BACKTRACE="1")
            view = subprocess.Popen([str(viewer), "--join", f"ws://127.0.0.1:{port}/native"],
                                    cwd=ROOT, env=env, stdout=view_log, stderr=subprocess.STDOUT)
            print(f"viewer pid {view.pid}; logs in {data}", flush=True)
            view.wait()
            print(f"viewer exited {view.returncode}", flush=True)
        finally:
            srv.terminate()
            try:
                srv.wait(timeout=15)
            except subprocess.TimeoutExpired:
                srv.kill()


if __name__ == "__main__":
    main()
