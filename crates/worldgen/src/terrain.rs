//! The base noise terrain from `net.minecraft.world.gen.ChunkGeneratorOverworld`: the 5x33x5
//! density field (`generateHeightmap`) and its trilinear fill into stone and water
//! (`setBlocksInChunk`).

use crate::biome::Biome;
use crate::biome_provider::BiomeProvider;
use crate::blocks;
use crate::java_random::JavaRandom;
use crate::math_helper;
use crate::noise::{NoiseGeneratorOctaves, NoiseGeneratorPerlin};
use crate::primer::ChunkPrimer;

/// `ChunkGeneratorSettings.Factory.setDefaults()`, the values `jsonToFactory("")` gives the default
/// world type. Kept as the `float`s vanilla stores so every widening happens where vanilla does it.
pub mod settings {
    pub const COORDINATE_SCALE: f32 = 684.412;
    pub const HEIGHT_SCALE: f32 = 684.412;
    pub const UPPER_LIMIT_SCALE: f32 = 512.0;
    pub const LOWER_LIMIT_SCALE: f32 = 512.0;
    pub const DEPTH_NOISE_SCALE_X: f32 = 200.0;
    pub const DEPTH_NOISE_SCALE_Z: f32 = 200.0;
    pub const DEPTH_NOISE_SCALE_EXPONENT: f32 = 0.5;
    pub const MAIN_NOISE_SCALE_X: f32 = 80.0;
    pub const MAIN_NOISE_SCALE_Y: f32 = 160.0;
    pub const MAIN_NOISE_SCALE_Z: f32 = 80.0;
    pub const BASE_SIZE: f32 = 8.5;
    pub const STRETCH_Y: f32 = 12.0;
    pub const BIOME_DEPTH_WEIGHT: f32 = 1.0;
    pub const BIOME_DEPTH_OFFSET: f32 = 0.0;
    pub const BIOME_SCALE_WEIGHT: f32 = 1.0;
    pub const BIOME_SCALE_OFFSET: f32 = 0.0;
    pub const SEA_LEVEL: i32 = 63;
}

use settings::*;

/// The noise fields of `ChunkGeneratorOverworld` that terrain shaping reads.
pub struct TerrainNoise {
    min_limit: NoiseGeneratorOctaves,
    max_limit: NoiseGeneratorOctaves,
    main: NoiseGeneratorOctaves,
    /// `surfaceNoise`, read by `replaceBiomeBlocks`.
    pub surface: NoiseGeneratorPerlin,
    depth: NoiseGeneratorOctaves,
    biome_weights: [f32; 25],
}

/// The arrays `generateHeightmap` leaves in generator fields, kept per call so stages can be
/// compared against the fixtures.
#[derive(Default, Clone)]
pub struct Heightmap {
    /// `biomesForGeneration` after `setBlocksInChunk`: 10x10 at 1:4, from `x * 4 - 2`.
    pub biomes_for_generation: Vec<&'static Biome>,
    pub depth_region: Vec<f64>,
    pub main_noise: Vec<f64>,
    pub min_limit: Vec<f64>,
    pub max_limit: Vec<f64>,
    /// 5x5 columns of 33 densities, indexed `(x * 5 + z) * 33 + y`.
    pub height_map: Vec<f64>,
}

impl TerrainNoise {
    /// The noise part of `ChunkGeneratorOverworld(World, long, boolean, String)`: every generator
    /// draws from one `Random(seed)` in declaration order.
    pub fn new(seed: i64) -> Self {
        let mut rand = JavaRandom::new(seed);
        let min_limit = NoiseGeneratorOctaves::new(&mut rand, 16);
        let max_limit = NoiseGeneratorOctaves::new(&mut rand, 16);
        let main = NoiseGeneratorOctaves::new(&mut rand, 8);
        let surface = NoiseGeneratorPerlin::new(&mut rand, 4);
        // `scaleNoise` is never sampled during terrain generation, but building it advances the
        // shared Random that `depthNoise` is built from next.
        let _scale = NoiseGeneratorOctaves::new(&mut rand, 10);
        let depth = NoiseGeneratorOctaves::new(&mut rand, 16);
        // DROPPED: forestNoise = new NoiseGeneratorOctaves(rand, 8); only population reads it.
        let mut biome_weights = [0.0f32; 25];
        for i in -2i32..=2 {
            for j in -2i32..=2 {
                let f = 10.0f32 / math_helper::sqrt_f((i * i + j * j) as f32 + 0.2f32);
                biome_weights[(i + 2 + (j + 2) * 5) as usize] = f;
            }
        }
        TerrainNoise { min_limit, max_limit, main, surface, depth, biome_weights }
    }

    /// `ChunkGeneratorOverworld.setBlocksInChunk(int, int, ChunkPrimer)`
    pub fn set_blocks_in_chunk(&self, provider: &BiomeProvider, x: i32, z: i32, primer: &mut ChunkPrimer) -> Heightmap {
        let mut hm = Heightmap {
            biomes_for_generation: provider.get_biomes_for_generation(x * 4 - 2, z * 4 - 2, 10, 10),
            ..Default::default()
        };
        self.generate_heightmap(&mut hm, x * 4, 0, z * 4);
        let height_map = &hm.height_map;

        for i in 0..4usize {
            let j = i * 5;
            let k = (i + 1) * 5;
            for l in 0..4usize {
                let i1 = (j + l) * 33;
                let j1 = (j + l + 1) * 33;
                let k1 = (k + l) * 33;
                let l1 = (k + l + 1) * 33;
                for i2 in 0..32usize {
                    let mut d1 = height_map[i1 + i2];
                    let mut d2 = height_map[j1 + i2];
                    let mut d3 = height_map[k1 + i2];
                    let mut d4 = height_map[l1 + i2];
                    let d5 = (height_map[i1 + i2 + 1] - d1) * 0.125;
                    let d6 = (height_map[j1 + i2 + 1] - d2) * 0.125;
                    let d7 = (height_map[k1 + i2 + 1] - d3) * 0.125;
                    let d8 = (height_map[l1 + i2 + 1] - d4) * 0.125;
                    for j2 in 0..8usize {
                        let mut d10 = d1;
                        let mut d11 = d2;
                        let d12 = (d3 - d1) * 0.25;
                        let d13 = (d4 - d2) * 0.25;
                        for k2 in 0..4usize {
                            let d16 = (d11 - d10) * 0.25;
                            let mut lvt_45_1_ = d10 - d16;
                            for l2 in 0..4usize {
                                let (bx, by, bz) = ((i * 4 + k2) as i32, (i2 * 8 + j2) as i32, (l * 4 + l2) as i32);
                                lvt_45_1_ += d16;
                                if lvt_45_1_ > 0.0 {
                                    primer.set(bx, by, bz, blocks::STONE);
                                } else if by < SEA_LEVEL {
                                    primer.set(bx, by, bz, blocks::WATER);
                                }
                            }
                            d10 += d12;
                            d11 += d13;
                        }
                        d1 += d5;
                        d2 += d6;
                        d3 += d7;
                        d4 += d8;
                    }
                }
            }
        }
        hm
    }

    /// `ChunkGeneratorOverworld.generateHeightmap(int, int, int)` for the default world type
    /// (the AMPLIFIED branch never runs).
    fn generate_heightmap(&self, hm: &mut Heightmap, x: i32, y: i32, z: i32) {
        self.depth.generate_noise_octaves_2d(
            &mut hm.depth_region,
            x,
            z,
            5,
            5,
            DEPTH_NOISE_SCALE_X as f64,
            DEPTH_NOISE_SCALE_Z as f64,
            DEPTH_NOISE_SCALE_EXPONENT as f64,
        );
        let f = COORDINATE_SCALE;
        let f1 = HEIGHT_SCALE;
        self.main.generate_noise_octaves(
            &mut hm.main_noise,
            x,
            y,
            z,
            5,
            33,
            5,
            (f / MAIN_NOISE_SCALE_X) as f64,
            (f1 / MAIN_NOISE_SCALE_Y) as f64,
            (f / MAIN_NOISE_SCALE_Z) as f64,
        );
        self.min_limit.generate_noise_octaves(&mut hm.min_limit, x, y, z, 5, 33, 5, f as f64, f1 as f64, f as f64);
        self.max_limit.generate_noise_octaves(&mut hm.max_limit, x, y, z, 5, 33, 5, f as f64, f1 as f64, f as f64);
        hm.height_map.clear();
        hm.height_map.resize(825, 0.0);
        let biomes = &hm.biomes_for_generation;
        let mut i = 0usize;
        let mut j = 0usize;

        for k in 0..5usize {
            for l in 0..5usize {
                let mut f2 = 0.0f32;
                let mut f3 = 0.0f32;
                let mut f4 = 0.0f32;
                let biome = biomes[k + 2 + (l + 2) * 10];
                for j1 in -2i32..=2 {
                    for k1 in -2i32..=2 {
                        let biome1 = biomes[(k as i32 + j1 + 2 + (l as i32 + k1 + 2) * 10) as usize];
                        let f5 = BIOME_DEPTH_OFFSET + biome1.base_height * BIOME_DEPTH_WEIGHT;
                        let f6 = BIOME_SCALE_OFFSET + biome1.height_variation * BIOME_SCALE_WEIGHT;
                        let mut f7 = self.biome_weights[(j1 + 2 + (k1 + 2) * 5) as usize] / (f5 + 2.0f32);
                        if biome1.base_height > biome.base_height {
                            f7 /= 2.0f32;
                        }
                        f2 += f6 * f7;
                        f3 += f5 * f7;
                        f4 += f7;
                    }
                }
                f2 /= f4;
                f3 /= f4;
                f2 = f2 * 0.9f32 + 0.1f32;
                f3 = (f3 * 4.0f32 - 1.0f32) / 8.0f32;
                let mut d7 = hm.depth_region[j] / 8000.0;
                if d7 < 0.0 {
                    d7 = -d7 * 0.3;
                }
                d7 = d7 * 3.0 - 2.0;
                if d7 < 0.0 {
                    d7 /= 2.0;
                    if d7 < -1.0 {
                        d7 = -1.0;
                    }
                    d7 /= 1.4;
                    d7 /= 2.0;
                } else {
                    if d7 > 1.0 {
                        d7 = 1.0;
                    }
                    d7 /= 8.0;
                }
                j += 1;
                let mut d8 = f3 as f64;
                let d9 = f2 as f64;
                d8 += d7 * 0.2;
                d8 = d8 * BASE_SIZE as f64 / 8.0;
                let d0 = BASE_SIZE as f64 + d8 * 4.0;

                for l1 in 0..33i32 {
                    let mut d1 = (l1 as f64 - d0) * STRETCH_Y as f64 * 128.0 / 256.0 / d9;
                    if d1 < 0.0 {
                        d1 *= 4.0;
                    }
                    let d2 = hm.min_limit[i] / LOWER_LIMIT_SCALE as f64;
                    let d3 = hm.max_limit[i] / UPPER_LIMIT_SCALE as f64;
                    let d4 = (hm.main_noise[i] / 10.0 + 1.0) / 2.0;
                    let mut d5 = math_helper::clamped_lerp(d2, d3, d4) - d1;
                    if l1 > 29 {
                        let d6 = ((l1 - 29) as f32 / 3.0f32) as f64;
                        d5 = d5 * (1.0 - d6) + -10.0 * d6;
                    }
                    hm.height_map[i] = d5;
                    i += 1;
                }
            }
        }
    }
}
