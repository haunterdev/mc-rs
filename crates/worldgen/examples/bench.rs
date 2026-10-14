//! Chunks per second, mirroring the Java harness's `Bench` (harness/src/mcrs/harness/Harness.java):
//! two warmup squares far from the origin, then a timed `side x side` square centered on it.
//! The checksum sums the highest non-air y of each chunk's (x & 15, z & 15) column, the same
//! scan as the Java `Bench.top`, so it equals the Java number for the same seed and side.
//!
//! `cargo run --release -p worldgen --example bench -- [--seed S] [--side N] [--threads T]`
//!
//! Prints one JSON line for one thread and one for `T` threads (default: every core).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use worldgen::{ChunkGenerator, ChunkPrimer};

/// Highest non-air y in the column, or 0. The harness's `Bench.top`, not vanilla's
/// `ChunkPrimer.findGroundBlockIdx`, which reads the next column (DECOMP_NOTES).
fn top(p: &ChunkPrimer, x: i32, z: i32) -> i32 {
    (0..256).rev().find(|&y| p.get(x, y, z) != 0).unwrap_or(0)
}

fn chunk(g: &ChunkGenerator, side: i32, x0: i32, z0: i32, i: usize) -> i32 {
    let (x, z) = (i as i32 % side, i as i32 / side);
    let c = g.generate(x0 + x, z0 + z);
    top(&c.blocks, x & 15, z & 15)
}

/// Generates the square on `threads` threads; returns the checksum.
fn square(g: &ChunkGenerator, x0: i32, z0: i32, side: i32, threads: usize) -> i32 {
    let n = (side * side) as usize;
    if threads == 1 {
        return (0..n).fold(0i32, |acc, i| acc.wrapping_add(chunk(g, side, x0, z0, i)));
    }
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut acc = 0i32;
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            return acc;
                        }
                        acc = acc.wrapping_add(chunk(g, side, x0, z0, i));
                    }
                })
            })
            .collect();
        handles.into_iter().fold(0i32, |acc, h| acc.wrapping_add(h.join().unwrap()))
    })
}

fn run(g: &ChunkGenerator, side: i32, threads: usize) {
    square(g, 100000, 100000, side, threads);
    square(g, -100000, 100000, side, threads);
    let t = Instant::now();
    let acc = square(g, -side / 2, -side / 2, side, threads);
    let secs = t.elapsed().as_secs_f64();
    let n = side * side;
    println!(
        "{{\"seed\": {}, \"side\": {side}, \"chunks\": {n}, \"threads\": {threads}, \"seconds\": {secs}, \"chunks_per_sec\": {}, \"checksum\": {acc}}}",
        g.seed(),
        n as f64 / secs
    );
}

fn main() {
    let mut seed = 0i64;
    let mut side = 32i32;
    let mut threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let v = it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--seed" => seed = v.parse().expect("--seed takes a number"),
            "--side" => side = v.parse().expect("--side takes a number"),
            "--threads" => threads = v.parse::<usize>().expect("--threads takes a number").max(1),
            _ => panic!("unknown argument {flag}"),
        }
    }
    let g = ChunkGenerator::new(seed);
    run(&g, side, 1);
    if threads > 1 {
        run(&g, side, threads);
    }
}
