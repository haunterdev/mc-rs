//! Per-stage diffs of `generateChunk` against the Java dumps, one test per stage in pipeline
//! order: biomes, base terrain, surface, caves, ravines, then the whole chunk.

mod common;

use common::*;
use worldgen::{ChunkGenerator, Stages};

/// Runs `check` on every fixture chunk of every seed.
fn each_chunk(mut check: impl FnMut(i64, &Fixture, &ChunkGenerator, i32, i32, &str)) {
    for seed in seeds() {
        let js = json(&format!("seed_{seed}.json"));
        let fx = Fixture::load(js["file"].as_str().unwrap());
        let gen = ChunkGenerator::new(seed);
        for c in js["chunks"].as_array().unwrap() {
            let (x, z) = (c["x"].as_i64().unwrap() as i32, c["z"].as_i64().unwrap() as i32);
            check(seed, &fx, &gen, x, z, &format!("c/{x}/{z}/"));
        }
    }
}

fn staged(gen: &ChunkGenerator, x: i32, z: i32) -> Stages {
    gen.generate_staged(x, z)
}

/// Biome ids as `int`, the way the dumps store them.
fn ids(b: &[&worldgen::biome::Biome]) -> Vec<i32> {
    b.iter().map(|b| b.id as i32).collect()
}

#[test]
fn stage1_biomes() {
    each_chunk(|seed, fx, gen, x, z, p| {
        let s = staged(gen, x, z);
        let label = |what: &str| format!("seed {seed} chunk ({x}, {z}) {what}");
        assert_same(&label("biomesForGeneration (1:4)"), fx.i32(&format!("{p}gen_biomes")), &ids(&s.heightmap.biomes_for_generation), |n| {
            format!("1:4 cell x={} z={}", x * 4 - 2 + (n % 10) as i32, z * 4 - 2 + (n / 10) as i32)
        });
        assert_same(&label("getBiomes (1:1)"), fx.i32(&format!("{p}biomes")), &ids(&s.biomes), |n| {
            format!("block x={} z={}", x * 16 + (n % 16) as i32, z * 16 + (n / 16) as i32)
        });
    });
}

#[test]
fn stage2_terrain() {
    each_chunk(|seed, fx, gen, x, z, p| {
        let s = staged(gen, x, z);
        let hm = &s.heightmap;
        let label = |what: &str| format!("seed {seed} chunk ({x}, {z}) {what}");
        let col = |n: usize| format!("x={} z={}", n / 5, n % 5);
        let cell = |n: usize| format!("x={} z={} y={}", n / (33 * 5), (n / 33) % 5, n % 33);
        assert_same(&label("depthRegion"), fx.f64(&format!("{p}depth_region")), &hm.depth_region, col);
        assert_same(&label("mainNoiseRegion"), fx.f64(&format!("{p}main_noise")), &hm.main_noise, cell);
        assert_same(&label("minLimitRegion"), fx.f64(&format!("{p}min_limit")), &hm.min_limit, cell);
        assert_same(&label("maxLimitRegion"), fx.f64(&format!("{p}max_limit")), &hm.max_limit, cell);
        assert_same(&label("heightMap"), fx.f64(&format!("{p}height_map")), &hm.height_map, cell);
        assert_blocks(&label("setBlocksInChunk"), x, z, fx.u16(&format!("{p}noise")), &s.noise.data);
    });
}

#[test]
fn stage3_surface() {
    each_chunk(|seed, fx, gen, x, z, p| {
        let s = staged(gen, x, z);
        let label = |what: &str| format!("seed {seed} chunk ({x}, {z}) {what}");
        assert_same(&label("depthBuffer"), fx.f64(&format!("{p}depth_buffer")), &s.depth_buffer, |n| {
            format!("column x={} z={}", x * 16 + (n / 16) as i32, z * 16 + (n % 16) as i32)
        });
        assert_blocks(&label("replaceBiomeBlocks"), x, z, fx.u16(&format!("{p}surface")), &s.surface.data);
    });
}

#[test]
fn stage4_caves() {
    each_chunk(|seed, fx, gen, x, z, p| {
        let s = staged(gen, x, z);
        assert_blocks(&format!("seed {seed} MapGenCaves"), x, z, fx.u16(&format!("{p}caves")), &s.caves.data);
    });
}

#[test]
fn stage5_ravines() {
    each_chunk(|seed, fx, gen, x, z, p| {
        let s = staged(gen, x, z);
        assert_blocks(&format!("seed {seed} MapGenRavine"), x, z, fx.u16(&format!("{p}ravines")), &s.ravines.data);
    });
}

/// `generate` (the path the viewer uses) against the final dump, blocks and biome bytes.
#[test]
fn generate_chunk() {
    each_chunk(|seed, fx, gen, x, z, p| {
        let c = gen.generate(x, z);
        assert_blocks(&format!("seed {seed} generateChunk"), x, z, fx.u16(&format!("{p}ravines")), &c.blocks.data);
        let exp: Vec<i32> = fx.i32(&format!("{p}biomes")).iter().map(|&b| b as u8 as i32).collect();
        let got: Vec<i32> = c.biomes.iter().map(|&b| b as i32).collect();
        assert_same(&format!("seed {seed} chunk ({x}, {z}) biome bytes"), &exp, &got, flat);
    });
}
