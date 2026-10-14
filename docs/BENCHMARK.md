# BENCHMARK

Chunk generation throughput for the 0.1 scope: `setBlocksInChunk`, `getBiomes`,
`replaceBiomeBlocks`, `MapGenCaves` and `MapGenRavine`, per chunk. Each run generates two
32x32 warmup squares around (100000, 100000) and (-100000, 100000), then
times a 32x32 square (1024 chunks) centered on the origin. Seed 0,
median of 3 runs, measured 2026-10-02.

| Implementation | Chunks/s | ms/chunk | vs Java |
|---|---|---|---|
| Java 8, 1 thread | 991 | 1.009 | 1.00x |
| Rust, 1 thread | 1,774 | 0.564 | 1.79x |
| Rust, 12 threads | 11,250 | 0.089 | 11.35x |

All runs produced checksum 66819. The checksum sums the highest non-air y of one column per chunk, so a match means
both sides generated the same terrain.

Per-run chunks/s: Java 991, 1,000, 978; Rust 1 thread 1,768, 1,793, 1,774; Rust 12 threads 11,346, 11,250, 10,944.

## Hardware

| | |
|---|---|
| CPU | AMD Ryzen 5 9600X 6-Core Processor, 6 cores, 12 threads |
| RAM | 16 GB, 2x 8 GB at 6000 MT/s |
| OS | Microsoft Windows 11 Home (build 26200) |
| Rust | rustc 1.95.0 (59807616e 2026-04-14), `--release` |
| Java | openjdk version "1.8.0_492" OpenJDK Runtime Environment (Temurin)(build 1.8.0_492-b09) OpenJDK 64-Bit Server VM (Temurin)(build 25.492-b09, mixed mode) |

## How it runs

- Java: `python tools/fixtures.py bench --side 32 --seed 0`, the `Bench` class
  in `harness/src/mcrs/harness/Harness.java`, driving vanilla `ChunkGeneratorOverworld` from the
  remapped 1.12.2 server jar. One JVM per run, heap capped at 1 GB.
- Rust: `cargo run --release -p worldgen --example bench -- --side 32 --seed 0 --threads N`.
  The multi-thread case shares one `ChunkGenerator` across threads that pull chunk indices from
  an atomic counter.
- Every process ran at below-normal priority with background memory and I/O priority. The JVM
  was held to 50% of the CPU, six logical cores' worth, which covers the bench thread plus JIT
  and GC threads; the Rust process had no CPU cap.
- Regenerate with `python tools/bench.py --side 32 --seed 0 --runs 3`.
