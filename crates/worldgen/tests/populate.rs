//! Diffs `populate` against the Java `Pop` dumps: per region, generate a 8x8 chunk square, run
//! `ChunkGeneratorOverworld.populate` over the inner 4x4, then compare the 3x3 chunks whose every
//! populating neighbour has run. The loops mirror `harness/src/mcrs/harness/Pop.java` exactly.
//! `MCRS_POP_SEED` limits the run to one seed.

mod common;

use common::*;
use worldgen::populate::{self, PopStatics, PopWorld};
use worldgen::ChunkGenerator;

const SIDE: i32 = 4;

/// One mismatching chunk: label, first difference, and how many blocks differ.
fn diff_chunk(label: &str, cx: i32, cz: i32, exp: &[u16], got: &[u16]) -> Option<String> {
    let (i, e, a) = first_diff(exp, got)?;
    let total = exp.iter().zip(got).filter(|(e, a)| e != a).count();
    Some(format!("{label}: {} expected {}, got {} ({total} differ)", block_at(cx, cz)(i), state(e), state(a)))
}

fn run_seed(seed: i64, failures: &mut Vec<String>) {
    let js = json(&format!("pop_{seed}.json"));
    let fx = Fixture::load(js["file"].as_str().unwrap());
    assert_eq!(js["side"].as_i64().unwrap() as i32, SIDE);
    let gen = ChunkGenerator::new(seed);
    let mut statics = PopStatics::default();
    for r in js["regions"].as_array().unwrap() {
        let i = r["i"].as_i64().unwrap();
        let tag = r["tag"].as_str().unwrap();
        let (ox, oz) = (r["x"].as_i64().unwrap() as i32, r["z"].as_i64().unwrap() as i32);
        let mut w = PopWorld::new(&gen, std::mem::take(&mut statics));
        for z in oz - 2..oz + SIDE + 2 {
            for x in ox - 2..ox + SIDE + 2 {
                w.chunk(x, z);
            }
        }
        w.freeze();
        for z in oz..oz + SIDE {
            for x in ox..ox + SIDE {
                populate::populate(&mut w, x, z);
            }
        }
        let late = r["late_generations"].as_u64().unwrap() as u32;
        if w.late_generations != late {
            failures.push(format!("seed {seed} region {i} ({tag}): late_generations expected {late}, got {}", w.late_generations));
        }
        for z in oz + 1..oz + SIDE {
            for x in ox + 1..ox + SIDE {
                let p = format!("pop/{i}/{x}/{z}/");
                let c = w.chunk(x, z);
                let mut d = vec![0u16; 65536];
                let mut hm = vec![0i32; 256];
                for bx in 0..16 {
                    for bz in 0..16 {
                        for y in 0..256 {
                            d[(bx << 12 | bz << 8 | y) as usize] = c.get(bx, y, bz);
                        }
                        hm[(bz << 4 | bx) as usize] = c.height_value(bx, bz);
                    }
                }
                let label = format!("seed {seed} region {i} ({tag}) chunk ({x}, {z})");
                if let Some(f) = diff_chunk(&label, x, z, fx.u16(&format!("{p}blocks")), &d) {
                    failures.push(f);
                } else if let Some((n, e, a)) = first_diff(fx.i32(&format!("{p}height")), &hm) {
                    failures.push(format!("{label}: height at x={} z={} expected {e}, got {a}", n & 15, n >> 4));
                }
            }
        }
        statics = std::mem::take(&mut w.statics);
    }
}

/// The Rust side of the harness `trace` mode: `MCRS_TRACE="seed ox oz x0 y0 z0 x1 y1 z1"` prints
/// every block set and scheduled tick inside the box, in the harness's format.
#[test]
#[ignore]
fn trace_region() {
    let v: Vec<i64> = std::env::var("MCRS_TRACE").unwrap().split_whitespace().map(|s| s.parse().unwrap()).collect();
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let gen = ChunkGenerator::new(v[0]);
            let (ox, oz) = (v[1] as i32, v[2] as i32);
            let mut w = PopWorld::new(&gen, PopStatics::default());
            for z in oz - 2..oz + SIDE + 2 {
                for x in ox - 2..ox + SIDE + 2 {
                    w.chunk(x, z);
                }
            }
            w.freeze();
            w.trace = Some(std::array::from_fn(|i| v[3 + i] as i32));
            for z in oz..oz + SIDE {
                for x in ox..ox + SIDE {
                    println!("populate {x} {z}");
                    populate::populate(&mut w, x, z);
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

/// Prints every block state the Java dumps hold, for the viewer's model and tint coverage.
#[test]
#[ignore]
fn distinct_states() {
    let mut seen = std::collections::BTreeMap::<u16, u64>::new();
    for seed in seeds() {
        let js = json(&format!("pop_{seed}.json"));
        let fx = Fixture::load(js["file"].as_str().unwrap());
        for r in js["regions"].as_array().unwrap() {
            let (i, ox, oz) = (r["i"].as_i64().unwrap(), r["x"].as_i64().unwrap() as i32, r["z"].as_i64().unwrap() as i32);
            for z in oz + 1..oz + SIDE {
                for x in ox + 1..ox + SIDE {
                    for &s in fx.u16(&format!("pop/{i}/{x}/{z}/blocks")) {
                        *seen.entry(s).or_default() += 1;
                    }
                }
            }
        }
    }
    for (s, n) in seen {
        println!("state {}:{} {n}", s >> 4, s & 15);
    }
}

#[test]
fn populate_matches_java() {
    let only = std::env::var("MCRS_POP_SEED").ok().map(|s| s.parse::<i64>().unwrap());
    let h = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let mut failures = Vec::new();
            for seed in seeds() {
                if only.is_none_or(|o| o == seed) {
                    run_seed(seed, &mut failures);
                }
            }
            failures
        })
        .unwrap();
    let failures = h.join().unwrap();
    assert!(failures.is_empty(), "{} chunks differ:\n{}", failures.len(), failures.join("\n"));
}
