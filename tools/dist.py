"""Builds the shareable mc-rs-launcher.exe players download, and the update manifest for it.

    python tools/dist.py [--jobs 4] [--notes "what changed"] [--update-url URL] [--release-base URL]
    python tools/dist.py --launcher-only     reuse the last viewer build
    python tools/dist.py --no-guard          skip the memory guard MCRS_RAMGUARD names

1. The viewer in Cargo profile `dist-viewer` (Cargo.toml: thin LTO over 16 codegen units,
   size-optimized, stripped) with its `share` feature (no console window). After a viewer change
   this takes a few minutes; the first build in that profile compiles every dependency once.
2. The viewer zstd-compressed (level 22) and built into crates/launcher, the launcher window that
   installs it to %LOCALAPPDATA%\\Programs\\mc-rs, downloads the 1.12.2 client jar when the PC has
   none, and checks the update channel.
3. dist/mc-rs-launcher.exe (under 20 MB or this exits nonzero) and dist/manifest.json, the file the
   launcher reads from --update-url. Publishing an update is uploading both so that the manifest
   is served at that URL and the exe at the manifest's "url" (--release-base plus mc-rs-launcher.exe).
   A launcher only sees versions newer than its own, so bump [workspace.package] version in
   Cargo.toml before a build that goes out.

Both exes link the MSVC runtime statically, so they run on a PC without the Visual C++
redistributable. When MCRS_RAMGUARD names a ramguard.py (a memory guard), builds run squeezed
behind it. Needs Python 3.14 or newer (compression.zstd).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
from compression import zstd
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RAMGUARD = os.environ.get("MCRS_RAMGUARD")
NO_GUARD = "--no-guard" in sys.argv
TARGET = "x86_64-pc-windows-msvc"
BUILT = ROOT / "target" / TARGET / "dist"
VIEWER = ROOT / "target" / TARGET / "dist-viewer" / "viewer.exe"
PAYLOAD = BUILT / "viewer.zst"
OUT = ROOT / "dist" / "mc-rs-launcher.exe"
MANIFEST = ROOT / "dist" / "manifest.json"
LIMIT = 20_000_000
# The channel baked into every launcher this builds. GitHub serves a release's assets at
# releases/download/<tag>/<name>, and the newest release's at releases/latest/download/<name>.
REPO = "https://github.com/haunterdev/mc-rs"
UPDATE_URL = f"{REPO}/releases/latest/download/manifest.json"
RELEASE_BASE = f"{REPO}/releases/download/v{{version}}/"


def cargo(profile: str, jobs: int, *args: str, **env: str) -> None:
    # An explicit --target keeps RUSTFLAGS off build scripts and proc macros, and keeps this
    # build's artifacts apart from target/release.
    cmd = ["cargo", "build", "--profile", profile, "--target", TARGET, "-j", str(jobs), *args]
    # With MCRS_RAMGUARD set the build runs squeezed under it (unless --no-guard); otherwise
    # cargo runs directly.
    if RAMGUARD and not NO_GUARD:
        cmd = [sys.executable, RAMGUARD, "run", "--squeeze", "--start-above", "0.5",
               "--pause-below", "1.5", "--resume-above", "2.2", "--kill-below", "0.3",
               "--io", "normal", "--vr-guard", "--job-mem", "0", "--", *cmd]
    start = time.monotonic()
    r = subprocess.run(cmd, cwd=ROOT, env=dict(os.environ, RUSTFLAGS="-C target-feature=+crt-static", **env))
    if r.returncode != 0:
        sys.exit(f"build exited {r.returncode} (75 = the memory guard ended it; rerun)")
    print(f"{args[1]}: built in {time.monotonic() - start:.0f} s")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--jobs", type=int, default=4, help="parallel rustc processes")
    ap.add_argument("--launcher-only", action="store_true", help="reuse target's last viewer build")
    ap.add_argument("--no-guard", action="store_true", help="build without the memory guard, at full speed")
    ap.add_argument("--notes", default="", help="the update banner's text")
    ap.add_argument("--update-url", default=UPDATE_URL, help="where launchers look for manifest.json")
    ap.add_argument("--release-base", default=RELEASE_BASE, help="the URL mc-rs-launcher.exe is served under")
    a = ap.parse_args()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]

    viewer = VIEWER
    if not a.launcher_only:
        cargo("dist-viewer", a.jobs, "-p", "viewer", "--features", "share")
    size = viewer.stat().st_size
    print(f"viewer: {size:,} bytes ({size / 1e6:.1f} MB)")
    PAYLOAD.write_bytes(zstd.compress(viewer.read_bytes(), level=22))
    print(f"compressed: {PAYLOAD.stat().st_size:,} bytes")
    cargo("dist", a.jobs, "-p", "launcher", MCRS_PAYLOAD=str(PAYLOAD), MCRS_UPDATE_URL=a.update_url)

    OUT.parent.mkdir(exist_ok=True)
    shutil.copy2(BUILT / "mc-rs-launcher.exe", OUT)
    data = OUT.read_bytes()
    print(f"{OUT}: {len(data):,} bytes ({len(data) / 1e6:.1f} MB), v{version}")
    MANIFEST.write_text(json.dumps({
        "version": version,
        "url": a.release_base.format(version=version) + OUT.name,
        "sha256": hashlib.sha256(data).hexdigest(),
        "size": len(data),
        "notes": a.notes,
    }, indent=2) + "\n")
    print(f"{MANIFEST}: update channel {a.update_url}")
    if len(data) >= LIMIT:
        sys.exit(f"over the {LIMIT / 1e6:.0f} MB limit")


if __name__ == "__main__":
    main()
