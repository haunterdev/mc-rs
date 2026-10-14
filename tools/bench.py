"""Java single-thread vs Rust single-thread vs Rust multi-thread chunk generation, written to docs/BENCHMARK.md.

    python tools/bench.py [--side 32] [--seed 0] [--runs 3]
    python tools/bench.py --render results.json     rewrite docs/BENCHMARK.md from saved results

Each case runs --runs times and reports the median. When MCRS_RAMGUARD names a ramguard.py
(a memory guard: below-normal priority, a CPU cap, the working set squeezed under memory
pressure), every process runs behind it: the Java bench through tools/fixtures.py, the Rust
bench with its CPU cap lifted (--cpu 100) so the multi-thread pass gets every core.
Raw results land in harness/build/bench-results.json (gitignored).
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import statistics
import subprocess
import sys
from pathlib import Path

from jdk import find_jdk8

ROOT = Path(__file__).resolve().parent.parent
RAMGUARD = os.environ.get("MCRS_RAMGUARD")
JDK8 = find_jdk8()
BENCH_EXE = ROOT / "target" / "release" / "examples" / "bench.exe"
RESULTS = ROOT / "harness" / "build" / "bench-results.json"
OUT = ROOT / "docs" / "BENCHMARK.md"


def run(cmd: list[str], **kw) -> str:
    r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        sys.stderr.write(r.stdout[-2000:] + r.stderr[-4000:])
        sys.exit(f"{' '.join(cmd[:6])} ... exited {r.returncode} (75 = the memory guard yielded, rerun later)")
    return r.stdout


def json_lines(text: str) -> list[dict]:
    return [json.loads(l) for l in text.splitlines() if l.startswith("{")]


def guarded(*cmd: str, cpu: int | None = None) -> list[str]:
    if not RAMGUARD:
        return list(cmd)
    extra = ["--cpu", str(cpu)] if cpu is not None else []
    return [sys.executable, RAMGUARD, "run", "--wait", "900", *extra, "--", *cmd]


def ps(expr: str) -> str:
    return subprocess.run(["powershell", "-NoProfile", "-Command", expr], capture_output=True, text=True).stdout.strip()


def hardware() -> dict:
    java = subprocess.run([str(JDK8 / "bin" / "java.exe"), "-version"], capture_output=True, text=True).stderr
    return {
        "cpu": ps("(Get-CimInstance Win32_Processor).Name.Trim()"),
        "cores": ps("(Get-CimInstance Win32_Processor).NumberOfCores"),
        "threads": ps("(Get-CimInstance Win32_Processor).NumberOfLogicalProcessors"),
        "ram": ps("$m = Get-CimInstance Win32_PhysicalMemory; '{0} GB, {1}x {2} GB at {3} MT/s' -f "
                  "(($m | Measure-Object Capacity -Sum).Sum / 1GB), $m.Count, ($m[0].Capacity / 1GB), $m[0].ConfiguredClockSpeed"),
        "os": ps("$o = Get-CimInstance Win32_OperatingSystem; '{0} (build {1})' -f $o.Caption, $o.BuildNumber"),
        "rustc": subprocess.run(["rustc", "-V"], capture_output=True, text=True).stdout.strip(),
        "java": " ".join(l.strip() for l in java.splitlines()[:3]),
    }


def measure(side: int, seed: int, runs: int, threads: int) -> dict:
    run(guarded("cargo", "build", "--release", "-p", "worldgen", "--example", "bench", "-j", "2"))
    out: dict = {"side": side, "seed": seed, "runs": runs, "threads": threads,
                 "date": dt.datetime.now().isoformat(timespec="seconds"), "hardware": hardware(),
                 "java": [], "rust_1": [], "rust_n": []}
    for _ in range(runs):
        out["java"] += json_lines(run([sys.executable, "tools/fixtures.py", "bench", "--side", str(side),
                                       "--seed", str(seed)]))
        # One process prints the 1-thread line, then the N-thread line.
        rust = json_lines(run(guarded(str(BENCH_EXE), "--side", str(side), "--seed", str(seed), "--threads",
                                      str(threads), cpu=100)))
        out["rust_1"] += [r for r in rust if r["threads"] == 1]
        out["rust_n"] += [r for r in rust if r["threads"] == threads]
    return out


def render(res: dict) -> str:
    def med(key: str) -> float:
        return statistics.median(r["chunks_per_sec"] for r in res[key])

    java, r1, rn = med("java"), med("rust_1"), med("rust_n")
    sums = {r["checksum"] for k in ("java", "rust_1", "rust_n") for r in res[k]}
    hw = res["hardware"]
    n = res["side"] * res["side"]
    rows = [
        ("Java 8, 1 thread", java),
        ("Rust, 1 thread", r1),
        (f"Rust, {res['threads']} threads", rn),
    ]
    table = "\n".join(f"| {name} | {cps:,.0f} | {1000 / cps:.3f} | {cps / java:.2f}x |" for name, cps in rows)
    each = lambda k: ", ".join(f"{r['chunks_per_sec']:,.0f}" for r in res[k])
    agree = ("All runs produced checksum {}.".format(sums.pop()) if len(sums) == 1
             else f"Checksums differ between runs: {sorted(sums)}.")
    return f"""# BENCHMARK

Chunk generation throughput for the 0.1 scope: `setBlocksInChunk`, `getBiomes`,
`replaceBiomeBlocks`, `MapGenCaves` and `MapGenRavine`, per chunk. Each run generates two
{res['side']}x{res['side']} warmup squares around (100000, 100000) and (-100000, 100000), then
times a {res['side']}x{res['side']} square ({n} chunks) centered on the origin. Seed {res['seed']},
median of {res['runs']} runs, measured {res['date'][:10]}.

| Implementation | Chunks/s | ms/chunk | vs Java |
|---|---|---|---|
{table}

{agree} The checksum sums the highest non-air y of one column per chunk, so a match means
both sides generated the same terrain.

Per-run chunks/s: Java {each('java')}; Rust 1 thread {each('rust_1')}; Rust {res['threads']} threads {each('rust_n')}.

## Hardware

| | |
|---|---|
| CPU | {hw['cpu']}, {hw['cores']} cores, {hw['threads']} threads |
| RAM | {hw['ram']} |
| OS | {hw['os']} |
| Rust | {hw['rustc']}, `--release` |
| Java | {hw['java']} |

## How it runs

- Java: `python tools/fixtures.py bench --side {res['side']} --seed {res['seed']}`, the `Bench` class
  in `harness/src/mcrs/harness/Harness.java`, driving vanilla `ChunkGeneratorOverworld` from the
  remapped 1.12.2 server jar. One JVM per run, heap capped at 1 GB.
- Rust: `cargo run --release -p worldgen --example bench -- --side {res['side']} --seed {res['seed']} --threads N`.
  The multi-thread case shares one `ChunkGenerator` across threads that pull chunk indices from
  an atomic counter.
- Every process ran at below-normal priority with background memory and I/O priority. The JVM
  was held to 50% of the CPU, six logical cores' worth, which covers the bench thread plus JIT
  and GC threads; the Rust process had no CPU cap.
- Regenerate with `python tools/bench.py --side {res['side']} --seed {res['seed']} --runs {res['runs']}`.
"""


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--side", type=int, default=32)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--threads", type=int, default=os.cpu_count() or 1)
    ap.add_argument("--render", type=Path, help="rewrite docs/BENCHMARK.md from a saved results file")
    a = ap.parse_args()
    if a.render:
        res = json.loads(a.render.read_text(encoding="utf-8"))
    else:
        res = measure(a.side, a.seed, a.runs, a.threads)
        RESULTS.parent.mkdir(parents=True, exist_ok=True)
        RESULTS.write_text(json.dumps(res, indent=2) + "\n", encoding="utf-8")
    OUT.write_text(render(res), encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
