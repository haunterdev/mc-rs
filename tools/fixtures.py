"""Build the Java reference harness and generate the fixture set the Rust tests diff against.

    python tools/fixtures.py build                compile harness/ against ../mc-1.12.2-src/server-mcp.jar
    python tools/fixtures.py gen [--seeds S ...]  misc dump, one JVM per seed, then fixtures/manifest.json
    python tools/fixtures.py pop [--seeds S ...]  only the props dump and population squares, then the manifest
    python tools/fixtures.py creative             creative tabs, potions, enchantments, eggs (fixtures/creative.json)
    python tools/fixtures.py bench [--side N]     Java single-thread chunks/sec (prints one JSON line)

When MCRS_RAMGUARD names a ramguard.py (a memory guard: Job Object, heap cap, working set
squeezed under memory pressure), every JVM runs behind it; otherwise java runs directly with
the same heap cap. Run tools/setup_src.py first. Fixtures land in fixtures/, which is gitignored.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

from jdk import find_jdk8

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT.parent / "mc-1.12.2-src"
JAR = SRC / "server-mcp.jar"
JDK8 = find_jdk8()
RAMGUARD = os.environ.get("MCRS_RAMGUARD")
CLASSES = ROOT / "harness" / "build" / "classes"
RUN_DIR = ROOT / "harness" / "build" / "run"
FIXTURES = ROOT / "fixtures"

SEEDS = [0, 1, -1, 123456789, -4172144997902289642, 9223372036854775807]


def build() -> None:
    if not JAR.exists():
        sys.exit(f"missing {JAR}; run tools/setup_src.py first")
    CLASSES.mkdir(parents=True, exist_ok=True)
    sources = sorted(str(p) for p in (ROOT / "harness" / "src").rglob("*.java"))
    subprocess.run([str(JDK8 / "bin" / "javac.exe"), "-encoding", "UTF-8", "-nowarn", "-cp", str(JAR),
                    "-d", str(CLASSES), *sources], check=True)
    print(f"compiled {len(sources)} sources into {CLASSES}")


def java(*args: str, heap: str = "1g") -> subprocess.CompletedProcess:
    RUN_DIR.mkdir(parents=True, exist_ok=True)
    cp = os.pathsep.join([str(CLASSES), str(JAR)])
    cmd = [str(JDK8 / "bin" / "java.exe"), "-cp", cp, "mcrs.harness.Harness", *args]
    if RAMGUARD:
        # Squeezed, not paused: the job runs smaller under memory pressure instead of waiting for headroom.
        cmd = [sys.executable, RAMGUARD, "run", "--heap", heap, "--job-mem", "2", "--squeeze",
               "--start-above", "1.0", "--pause-below", "1.5", "--resume-above", "2.2", "--kill-below", "0.3",
               "--io", "normal", "--vr-guard", "--wait", "900", "--", *cmd]
    else:
        cmd.insert(1, f"-Xmx{heap}")
    # cwd matters: the server log4j config writes logs/ relative to it.
    r = subprocess.run(cmd, cwd=RUN_DIR, capture_output=True, text=True)
    sys.stdout.write(r.stdout)
    if r.returncode != 0:
        sys.stderr.write(r.stderr[-4000:])
        sys.exit(f"harness {' '.join(args)} exited {r.returncode} (75 = the memory guard yielded, rerun later)")
    return r


def sha1(path: Path) -> str:
    h = hashlib.sha1()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def pop(seeds: list[int]) -> None:
    FIXTURES.mkdir(exist_ok=True)
    java("props", str(FIXTURES))
    for s in seeds:
        java("pop", str(s), str(FIXTURES))


def gen(seeds: list[int], stages: bool = True) -> None:
    FIXTURES.mkdir(exist_ok=True)
    if stages:
        java("misc", str(FIXTURES))
        for s in seeds:
            java("seed", str(s), str(FIXTURES))
    pop(seeds)
    misc = json.loads((FIXTURES / "misc.json").read_text(encoding="utf-8"))
    entries = []
    for s in seeds:
        meta = json.loads((FIXTURES / f"seed_{s}.json").read_text(encoding="utf-8"))
        popm = json.loads((FIXTURES / f"pop_{s}.json").read_text(encoding="utf-8"))
        entries.append({"seed": s, "json": f"seed_{s}.json", "chunks": len(meta["chunks"]),
                        "generate_chunk_mismatches": meta["generate_chunk_mismatches"],
                        "coverage_missing": meta["coverage_missing"],
                        "pop_json": f"pop_{s}.json", "pop_regions": len(popm["regions"]),
                        "pop_late_generations": popm["late_generations"], "pop_missing": popm["missing"]})
    manifest = {
        "format": "MCRSFIX1 (gzip): u16 name len, name, u8 dtype (1 i32, 2 u16, 3 f64, 4 f32, 5 i64), u32 count, LE payload",
        "generated": dt.datetime.now().isoformat(timespec="seconds"),
        "java_version": misc["java_version"],
        "java_vm": misc["java_vm"],
        "server_mcp_jar_sha1": sha1(JAR),
        "misc": "misc.json",
        "props": "props.json",
        "seeds": entries,
    }
    (FIXTURES / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {FIXTURES / 'manifest.json'}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("action", choices=["build", "gen", "pop", "creative", "bench"])
    ap.add_argument("--seeds", type=int, nargs="*", default=SEEDS)
    ap.add_argument("--side", type=int, default=32)
    ap.add_argument("--seed", type=int, default=0)
    a = ap.parse_args()
    if a.action == "build":
        build()
    elif a.action == "gen":
        build()
        gen(a.seeds)
    elif a.action == "pop":
        build()
        gen(a.seeds, stages=False)
    elif a.action == "creative":
        build()
        FIXTURES.mkdir(exist_ok=True)
        java("creative", str(FIXTURES))
    else:
        build()
        java("bench", str(a.seed), str(a.side))


if __name__ == "__main__":
    main()
