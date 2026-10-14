//! `net.minecraft.world.gen.MapGenRavine`

use crate::blocks::{self, id};
use crate::caves::{carve_bounds, for_each_source_chunk, hits_water, RANGE};
use crate::java_random::JavaRandom;
use crate::math_helper;
use crate::primer::ChunkPrimer;

/// One `MapGenRavine.generate` call: the chunk being carved and what it reads.
struct Ravines<'a> {
    primer: &'a mut ChunkPrimer,
    /// `world.getBiome(column).topBlock` per local column, `x + z * 16`.
    column_top: &'a [u16; 256],
    /// `MapGenRavine.rs`: 1024 floats in vanilla, of which `addTunnel` fills and reads the first
    /// 256 on every call.
    rs: [f32; 256],
}

/// `MapGenRavine.generate`: carves ravines into `primer` (chunk `x`, `z`).
pub fn generate(world_seed: i64, x: i32, z: i32, primer: &mut ChunkPrimer, column_top: &[u16; 256]) {
    let mut ravines = Ravines { primer, column_top, rs: [0.0; 256] };
    for_each_source_chunk(world_seed, x, z, |rand, chunk_x, chunk_z| {
        ravines.recursive_generate(rand, chunk_x, chunk_z, x, z);
    });
}

impl Ravines<'_> {
    /// `MapGenRavine.addTunnel`. Arguments as in [`crate::caves`]: carved chunk, start, width,
    /// yaw, pitch, start step, length, vertical scale.
    #[allow(clippy::too_many_arguments)]
    fn add_tunnel(
        &mut self,
        seed: i64,
        cx: i32,
        cz: i32,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        width: f32,
        mut yaw: f32,
        mut pitch: f32,
        mut step: i32,
        mut max_len: i32,
        y_scale: f64,
    ) {
        let mut random = JavaRandom::new(seed);
        let d0 = (cx * 16 + 8) as f64;
        let d1 = (cz * 16 + 8) as f64;
        let mut f = 0.0f32;
        let mut f1 = 0.0f32;
        if max_len <= 0 {
            let i = RANGE * 16 - 16;
            max_len = i - random.next_int_bounded(i / 4);
        }
        let mut flag1 = false;
        if step == -1 {
            step = max_len / 2;
            flag1 = true;
        }

        let mut f2 = 1.0f32;
        for j in 0..256 {
            if j == 0 || random.next_int_bounded(3) == 0 {
                f2 = 1.0f32 + random.next_float() * random.next_float();
            }
            self.rs[j] = f2 * f2;
        }

        while step < max_len {
            let mut d9 = 1.5 + (math_helper::sin(step as f32 * 3.1415927f32 / max_len as f32) * width) as f64;
            let mut d2 = d9 * y_scale;
            d9 *= random.next_float() as f64 * 0.25 + 0.75;
            d2 *= random.next_float() as f64 * 0.25 + 0.75;
            let f3 = math_helper::cos(pitch);
            let f4 = math_helper::sin(pitch);
            x += (math_helper::cos(yaw) * f3) as f64;
            y += f4 as f64;
            z += (math_helper::sin(yaw) * f3) as f64;
            pitch *= 0.7f32;
            pitch += f1 * 0.05f32;
            yaw += f * 0.05f32;
            f1 *= 0.8f32;
            f *= 0.5f32;
            let (a, b, c) = (random.next_float(), random.next_float(), random.next_float());
            f1 += (a - b) * c * 2.0f32;
            let (a, b, c) = (random.next_float(), random.next_float(), random.next_float());
            f += (a - b) * c * 4.0f32;

            if flag1 || random.next_int_bounded(4) != 0 {
                let d3 = x - d0;
                let d4 = z - d1;
                let d5 = (max_len - step) as f64;
                let d6 = (width + 2.0f32 + 16.0f32) as f64;
                if d3 * d3 + d4 * d4 - d5 * d5 > d6 * d6 {
                    return;
                }

                if x >= d0 - 16.0 - d9 * 2.0
                    && z >= d1 - 16.0 - d9 * 2.0
                    && x <= d0 + 16.0 + d9 * 2.0
                    && z <= d1 + 16.0 + d9 * 2.0
                {
                    let (k2, k, l2, l, i3, i1) = carve_bounds(cx, cz, x, y, z, d9, d2);
                    if !hits_water(self.primer, k2, k, l2, l, i3, i1) {
                        for j3 in k2..k {
                            let d10 = ((j3 + cx * 16) as f64 + 0.5 - x) / d9;
                            for i2 in i3..i1 {
                                let d7 = ((i2 + cz * 16) as f64 + 0.5 - z) / d9;
                                let mut flag = false;
                                if d10 * d10 + d7 * d7 < 1.0 {
                                    let mut j2 = l;
                                    while j2 > l2 {
                                        let d8 = ((j2 - 1) as f64 + 0.5 - y) / d2;
                                        if (d10 * d10 + d7 * d7) * self.rs[(j2 - 1) as usize] as f64 + d8 * d8 / 6.0 < 1.0 {
                                            let b = blocks::block_of(self.primer.get(j3, j2, i2));
                                            if b == id::GRASS {
                                                flag = true;
                                            }
                                            if b == id::STONE || b == id::DIRT || b == id::GRASS {
                                                if j2 - 1 < 10 {
                                                    self.primer.set(j3, j2, i2, blocks::FLOWING_LAVA);
                                                } else {
                                                    self.primer.set(j3, j2, i2, blocks::AIR);
                                                    if flag && blocks::block_of(self.primer.get(j3, j2 - 1, i2)) == id::DIRT {
                                                        let top = self.column_top[(j3 + i2 * 16) as usize];
                                                        self.primer.set(j3, j2 - 1, i2, top);
                                                    }
                                                }
                                            }
                                        }
                                        j2 -= 1;
                                    }
                                }
                            }
                        }
                        if flag1 {
                            break;
                        }
                    }
                }
            }
            step += 1;
        }
    }

    /// `MapGenRavine.recursiveGenerate`: one ravine in roughly 50 source chunks.
    fn recursive_generate(&mut self, rand: &mut JavaRandom, chunk_x: i32, chunk_z: i32, original_x: i32, original_z: i32) {
        if rand.next_int_bounded(50) == 0 {
            let d0 = (chunk_x * 16 + rand.next_int_bounded(16)) as f64;
            let n = rand.next_int_bounded(40) + 8;
            let d1 = (rand.next_int_bounded(n) + 20) as f64;
            let d2 = (chunk_z * 16 + rand.next_int_bounded(16)) as f64;
            let f = rand.next_float() * 6.2831855f32;
            let f1 = (rand.next_float() - 0.5f32) * 2.0f32 / 8.0f32;
            let (a, b) = (rand.next_float(), rand.next_float());
            let f2 = (a * 2.0f32 + b) * 2.0f32;
            let seed = rand.next_long();
            self.add_tunnel(seed, original_x, original_z, d0, d1, d2, f2, f, f1, 0, 0, 3.0);
        }
    }
}
