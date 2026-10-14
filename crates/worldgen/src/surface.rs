//! Surface blocks: `ChunkGeneratorOverworld.replaceBiomeBlocks` and every
//! `Biome.genTerrainBlocks` override the overworld uses.
//!
//! Vanilla biomes are singletons, and `BiomeHills`, `BiomeSavannaMutated` and the mega
//! `BiomeTaiga` variants rewrite their own `topBlock`/`fillerBlock` fields on every call. Caves and
//! ravines later read `topBlock` back, so they see whatever the biome's last column in this chunk
//! left there. [`SurfaceResult::column_top`] carries that value per column instead of mutating
//! shared state.

use crate::biome::{Biome, HillsType, Kind, TaigaType};
use crate::blocks::{self, dye, id};
use crate::java_random::JavaRandom;
use crate::noise::NoiseGeneratorPerlin;
use crate::primer::ChunkPrimer;
use crate::terrain::settings::SEA_LEVEL;

/// What `replaceBiomeBlocks` leaves behind besides the primer.
pub struct SurfaceResult {
    /// `depthBuffer`: surface noise, indexed `j + i * 16` like the biome array.
    pub depth_buffer: Vec<f64>,
    /// `world.getBiome(column).topBlock` as caves and ravines see it after this chunk's surface
    /// pass, indexed `x + z * 16` (local block coordinates).
    pub column_top: [u16; 256],
}

/// `BiomeMesa` per-instance data. Every Mesa instance starts with the same fields and is handed
/// the same world seed, so one copy stands in for all six.
pub struct MesaData {
    clay_bands: [u16; 64],
    clay_bands_offset_noise: NoiseGeneratorPerlin,
    pillar_noise: NoiseGeneratorPerlin,
    pillar_roof_noise: NoiseGeneratorPerlin,
}

impl MesaData {
    /// The state `BiomeMesa.genTerrainBlocks` builds on its first call in a fresh JVM. The pillar
    /// noises are seeded with the instance's previous `worldSeed`, which is still 0 at that point,
    /// before `worldSeed` is updated: they come from `Random(0)` whatever the world seed is.
    pub fn new(world_seed: i64) -> Self {
        let (clay_bands, clay_bands_offset_noise) = generate_bands(world_seed);
        let mut random = JavaRandom::new(0);
        let pillar_noise = NoiseGeneratorPerlin::new(&mut random, 4);
        let pillar_roof_noise = NoiseGeneratorPerlin::new(&mut random, 1);
        MesaData { clay_bands, clay_bands_offset_noise, pillar_noise, pillar_roof_noise }
    }

    /// `BiomeMesa.getBand(int, int, int)`. The offset noise samples x for both axes.
    fn get_band(&self, x: i32, y: i32, _z: i32) -> u16 {
        let i = crate::math_helper::java_round(
            self.clay_bands_offset_noise.get_value(x as f64 / 512.0, x as f64 / 512.0) * 2.0,
        ) as i32;
        self.clay_bands[((y + i + 64) % 64) as usize]
    }
}

/// `BiomeMesa.generateBands(long)`
fn generate_bands(seed: i64) -> ([u16; 64], NoiseGeneratorPerlin) {
    let mut clay_bands = [blocks::HARDENED_CLAY; 64];
    let mut random = JavaRandom::new(seed);
    let clay_bands_offset_noise = NoiseGeneratorPerlin::new(&mut random, 1);

    // `l1 += nextInt(5) + 1` inside a loop that also increments l1.
    let mut l1 = 0;
    while l1 < 64 {
        l1 += random.next_int_bounded(5) + 1;
        if l1 < 64 {
            clay_bands[l1 as usize] = blocks::stained_hardened_clay(dye::ORANGE);
        }
        l1 += 1;
    }

    let i2 = random.next_int_bounded(4) + 2;
    for _ in 0..i2 {
        let j = random.next_int_bounded(3) + 1;
        let k = random.next_int_bounded(64);
        let mut l = 0;
        while k + l < 64 && l < j {
            clay_bands[(k + l) as usize] = blocks::stained_hardened_clay(dye::YELLOW);
            l += 1;
        }
    }

    let j2 = random.next_int_bounded(4) + 2;
    for _ in 0..j2 {
        let i3 = random.next_int_bounded(3) + 2;
        let l3 = random.next_int_bounded(64);
        let mut i1 = 0;
        while l3 + i1 < 64 && i1 < i3 {
            clay_bands[(l3 + i1) as usize] = blocks::stained_hardened_clay(dye::BROWN);
            i1 += 1;
        }
    }

    let l2 = random.next_int_bounded(4) + 2;
    for _ in 0..l2 {
        let i4 = random.next_int_bounded(3) + 1;
        let k4 = random.next_int_bounded(64);
        let mut j1 = 0;
        while k4 + j1 < 64 && j1 < i4 {
            clay_bands[(k4 + j1) as usize] = blocks::stained_hardened_clay(dye::RED);
            j1 += 1;
        }
    }

    let k3 = random.next_int_bounded(3) + 3;
    let mut j4 = 0;
    for _ in 0..k3 {
        j4 += random.next_int_bounded(16) + 4;
        let mut k1 = 0;
        while j4 + k1 < 64 && k1 < 1 {
            clay_bands[(j4 + k1) as usize] = blocks::stained_hardened_clay(dye::WHITE);
            if j4 + k1 > 1 && random.next_boolean() {
                clay_bands[(j4 + k1 - 1) as usize] = blocks::stained_hardened_clay(dye::SILVER);
            }
            if j4 + k1 < 63 && random.next_boolean() {
                clay_bands[(j4 + k1 + 1) as usize] = blocks::stained_hardened_clay(dye::SILVER);
            }
            k1 += 1;
        }
    }
    (clay_bands, clay_bands_offset_noise)
}

/// The static noise and per-world biome state surface generation reads.
pub struct SurfaceGen {
    /// `Biome.GRASS_COLOR_NOISE`
    grass_color_noise: NoiseGeneratorPerlin,
    mesa: MesaData,
}

impl SurfaceGen {
    pub fn new(world_seed: i64) -> Self {
        SurfaceGen {
            grass_color_noise: NoiseGeneratorPerlin::new(&mut JavaRandom::new(2345), 1),
            mesa: MesaData::new(world_seed),
        }
    }

    /// `ChunkGeneratorOverworld.replaceBiomeBlocks(int, int, ChunkPrimer, Biome[])`. `biomes` is the
    /// 16x16 index-layer array, indexed `x + z * 16`.
    pub fn replace_biome_blocks(
        &self,
        surface_noise: &NoiseGeneratorPerlin,
        rand: &mut JavaRandom,
        x: i32,
        z: i32,
        primer: &mut ChunkPrimer,
        biomes: &[&'static Biome],
    ) -> SurfaceResult {
        let mut depth_buffer = Vec::new();
        surface_noise.get_region(&mut depth_buffer, (x * 16) as f64, (z * 16) as f64, 16, 16, 0.0625, 0.0625, 1.0);
        // The last topBlock each biome id was left with; biomes that never rewrite it keep the
        // constructor value.
        let mut top_by_id = [0u16; 256];
        for b in biomes {
            top_by_id[b.id as usize] = b.top_block;
        }
        for i in 0..16 {
            for j in 0..16 {
                let n = (j + i * 16) as usize;
                let biome = biomes[n];
                let top = self.gen_terrain_blocks(biome, rand, primer, x * 16 + i, z * 16 + j, depth_buffer[n]);
                top_by_id[biome.id as usize] = top;
            }
        }
        let mut column_top = [0u16; 256];
        for (n, b) in biomes.iter().enumerate() {
            column_top[n] = top_by_id[b.id as usize];
        }
        SurfaceResult { depth_buffer, column_top }
    }

    /// `Biome.genTerrainBlocks` and its overrides. Returns the `topBlock` the biome is left with.
    fn gen_terrain_blocks(
        &self,
        biome: &Biome,
        rand: &mut JavaRandom,
        primer: &mut ChunkPrimer,
        x: i32,
        z: i32,
        noise_val: f64,
    ) -> u16 {
        match biome.kind {
            // BiomeHills.genTerrainBlocks
            Kind::Hills(ty) => {
                let (mut top, mut filler) = (blocks::GRASS, blocks::DIRT);
                if (noise_val < -1.0 || noise_val > 2.0) && ty == HillsType::Mutated {
                    top = blocks::GRAVEL;
                    filler = blocks::GRAVEL;
                } else if noise_val > 1.0 && ty != HillsType::ExtraTrees {
                    top = blocks::STONE;
                    filler = blocks::STONE;
                }
                generate_biome_terrain(biome, top, filler, rand, primer, x, z, noise_val);
                top
            }
            // BiomeSavannaMutated.genTerrainBlocks
            Kind::SavannaMutated => {
                let (mut top, mut filler) = (blocks::GRASS, blocks::DIRT);
                if noise_val > 1.75 {
                    top = blocks::STONE;
                    filler = blocks::STONE;
                } else if noise_val > -0.5 {
                    top = blocks::COARSE_DIRT;
                }
                generate_biome_terrain(biome, top, filler, rand, primer, x, z, noise_val);
                top
            }
            // BiomeTaiga.genTerrainBlocks
            Kind::Taiga(TaigaType::Mega | TaigaType::MegaSpruce) => {
                let (mut top, filler) = (blocks::GRASS, blocks::DIRT);
                if noise_val > 1.75 {
                    top = blocks::COARSE_DIRT;
                } else if noise_val > -0.95 {
                    top = blocks::PODZOL;
                }
                generate_biome_terrain(biome, top, filler, rand, primer, x, z, noise_val);
                top
            }
            Kind::Swamp => {
                self.swamp(primer, x, z);
                generate_biome_terrain(biome, biome.top_block, biome.filler_block, rand, primer, x, z, noise_val);
                biome.top_block
            }
            Kind::Mesa { bryce_pillars, has_forest } => {
                self.mesa(biome, bryce_pillars, has_forest, rand, primer, x, z, noise_val);
                biome.top_block
            }
            _ => {
                generate_biome_terrain(biome, biome.top_block, biome.filler_block, rand, primer, x, z, noise_val);
                biome.top_block
            }
        }
    }

    /// The part of `BiomeSwamp.genTerrainBlocks` before `generateBiomeTerrain`: puddles and lily pads
    /// at y 62 where the grass colour noise is positive.
    fn swamp(&self, primer: &mut ChunkPrimer, x: i32, z: i32) {
        let d0 = self.grass_color_noise.get_value(x as f64 * 0.25, z as f64 * 0.25);
        if d0 > 0.0 {
            let i = x & 15;
            let j = z & 15;
            for k in (0..=255).rev() {
                if !blocks::is_air(primer.get(j, k, i)) {
                    if k == 62 && blocks::block_of(primer.get(j, k, i)) != id::WATER {
                        primer.set(j, k, i, blocks::WATER);
                        if d0 < 0.12 {
                            primer.set(j, k + 1, i, blocks::WATERLILY);
                        }
                    }
                    break;
                }
            }
        }
    }

    /// `BiomeMesa.genTerrainBlocks`
    #[allow(clippy::too_many_arguments)]
    fn mesa(
        &self,
        biome: &Biome,
        bryce_pillars: bool,
        has_forest: bool,
        rand: &mut JavaRandom,
        primer: &mut ChunkPrimer,
        x: i32,
        z: i32,
        noise_val: f64,
    ) {
        let m = &self.mesa;
        let mut d4 = 0.0f64;
        if bryce_pillars {
            let i = (x & -16) + (z & 15);
            let j = (z & -16) + (x & 15);
            let d0 = java_min(noise_val.abs(), m.pillar_noise.get_value(i as f64 * 0.25, j as f64 * 0.25));
            if d0 > 0.0 {
                let d2 = m.pillar_roof_noise.get_value(i as f64 * 0.001953125, j as f64 * 0.001953125).abs();
                d4 = d0 * d0 * 2.5;
                let d3 = (d2 * 50.0).ceil() + 14.0;
                if d4 > d3 {
                    d4 = d3;
                }
                d4 += 64.0;
            }
        }

        let k1 = x & 15;
        let l1 = z & 15;
        let i2 = SEA_LEVEL;
        let orange = blocks::stained_hardened_clay(dye::ORANGE);
        // DROPPED: `iblockstate` (the would-be top block, set to STAINED_HARDENED_CLAY, AIR or WATER)
        // is assigned along the way but never written to the primer.
        let mut iblockstate3 = biome.filler_block;
        let k = (noise_val / 3.0 + 3.0 + rand.next_double() * 0.25) as i32;
        // Only the sign of the cosine matters here, so the platform cos matches Math.cos.
        let flag = (noise_val / 3.0 * std::f64::consts::PI).cos() > 0.0;
        let mut l = -1;
        let mut flag1 = false;
        let mut i1 = 0;

        for j1 in (0..=255).rev() {
            if blocks::is_air(primer.get(l1, j1, k1)) && j1 < d4 as i32 {
                primer.set(l1, j1, k1, blocks::STONE);
            }
            if j1 <= rand.next_int_bounded(5) {
                primer.set(l1, j1, k1, blocks::BEDROCK);
            } else if i1 < 15 || bryce_pillars {
                let iblockstate1 = primer.get(l1, j1, k1);
                if blocks::is_air(iblockstate1) {
                    l = -1;
                } else if blocks::block_of(iblockstate1) == id::STONE {
                    if l == -1 {
                        flag1 = false;
                        if k <= 0 {
                            iblockstate3 = blocks::STONE;
                        } else if j1 >= i2 - 4 && j1 <= i2 + 1 {
                            iblockstate3 = biome.filler_block;
                        }
                        l = k + 0.max(j1 - i2);
                        if j1 >= i2 - 1 {
                            if has_forest && j1 > 86 + k * 2 {
                                primer.set(l1, j1, k1, if flag { blocks::COARSE_DIRT } else { blocks::GRASS });
                            } else if j1 > i2 + 3 + k {
                                let iblockstate2 = if (64..=127).contains(&j1) {
                                    if flag {
                                        blocks::HARDENED_CLAY
                                    } else {
                                        m.get_band(x, j1, z)
                                    }
                                } else {
                                    orange
                                };
                                primer.set(l1, j1, k1, iblockstate2);
                            } else {
                                primer.set(l1, j1, k1, biome.top_block);
                                flag1 = true;
                            }
                        } else {
                            primer.set(l1, j1, k1, iblockstate3);
                            if blocks::block_of(iblockstate3) == id::STAINED_HARDENED_CLAY {
                                primer.set(l1, j1, k1, orange);
                            }
                        }
                    } else if l > 0 {
                        l -= 1;
                        if flag1 {
                            primer.set(l1, j1, k1, orange);
                        } else {
                            primer.set(l1, j1, k1, m.get_band(x, j1, z));
                        }
                    }
                    i1 += 1;
                }
            }
        }
    }
}

/// `Math.min(double, double)`: NaN in `a` wins, and a tie between 0.0 and -0.0 returns -0.0.
#[inline]
fn java_min(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if a == 0.0 && b == 0.0 && (a.is_sign_negative() || b.is_sign_negative()) {
        -0.0
    } else if a <= b {
        a
    } else {
        b
    }
}

/// `Biome.generateBiomeTerrain(World, Random, ChunkPrimer, int, int, double)`. `top` and `filler`
/// are the biome's `topBlock` and `fillerBlock` at the time of the call.
#[allow(clippy::too_many_arguments)]
fn generate_biome_terrain(
    biome: &Biome,
    top: u16,
    filler: u16,
    rand: &mut JavaRandom,
    primer: &mut ChunkPrimer,
    x: i32,
    z: i32,
    noise_val: f64,
) {
    let i = SEA_LEVEL;
    let mut iblockstate = top;
    let mut iblockstate1 = filler;
    let mut j = -1;
    let k = (noise_val / 3.0 + 3.0 + rand.next_double() * 0.25) as i32;
    let l = x & 15;
    let i1 = z & 15;

    for j1 in (0..=255).rev() {
        if j1 <= rand.next_int_bounded(5) {
            primer.set(i1, j1, l, blocks::BEDROCK);
        } else {
            let iblockstate2 = primer.get(i1, j1, l);
            if blocks::is_air(iblockstate2) {
                j = -1;
            } else if blocks::block_of(iblockstate2) == id::STONE {
                if j == -1 {
                    if k <= 0 {
                        iblockstate = blocks::AIR;
                        iblockstate1 = blocks::STONE;
                    } else if j1 >= i - 4 && j1 <= i + 1 {
                        iblockstate = top;
                        iblockstate1 = filler;
                    }
                    if j1 < i && blocks::is_air(iblockstate) {
                        // DROPPED: the y > 64 branch of Biome.getTemperature; j1 < 63 here, so it
                        // returns the default temperature.
                        iblockstate = if biome.temperature < 0.15f32 { blocks::ICE } else { blocks::WATER };
                    }
                    j = k;
                    if j1 >= i - 1 {
                        primer.set(i1, j1, l, iblockstate);
                    } else if j1 < i - 7 - k {
                        iblockstate = blocks::AIR;
                        iblockstate1 = blocks::STONE;
                        primer.set(i1, j1, l, blocks::GRAVEL);
                    } else {
                        primer.set(i1, j1, l, iblockstate1);
                    }
                } else if j > 0 {
                    j -= 1;
                    primer.set(i1, j1, l, iblockstate1);
                    if j == 0 && blocks::block_of(iblockstate1) == id::SAND && k > 1 {
                        j = rand.next_int_bounded(4) + 0.max(j1 - 63);
                        iblockstate1 = if iblockstate1 == blocks::RED_SAND { blocks::RED_SANDSTONE } else { blocks::SANDSTONE };
                    }
                }
            }
        }
    }
}
