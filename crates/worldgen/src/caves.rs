//! `net.minecraft.world.gen.MapGenCaves` and the `MapGenBase.generate` driver it shares with
//! ravines.

use crate::blocks::{self, id};
use crate::java_random::JavaRandom;
use crate::math_helper::{self, floor};
use crate::primer::ChunkPrimer;

/// `MapGenBase.range`
pub const RANGE: i32 = 8;

/// `MapGenBase.generate(World, int, int, ChunkPrimer)`: seeds `rand` for every chunk within
/// [`RANGE`] of `(x, z)` and lets `recursive_generate` carve whatever of it reaches this chunk.
pub fn for_each_source_chunk(world_seed: i64, x: i32, z: i32, mut recursive_generate: impl FnMut(&mut JavaRandom, i32, i32)) {
    let mut rand = JavaRandom::new(world_seed);
    let j = rand.next_long();
    let k = rand.next_long();
    for l in x - RANGE..=x + RANGE {
        for i1 in z - RANGE..=z + RANGE {
            let j1 = (l as i64).wrapping_mul(j);
            let k1 = (i1 as i64).wrapping_mul(k);
            rand.set_seed(j1 ^ k1 ^ world_seed);
            recursive_generate(&mut rand, l, i1);
        }
    }
}

/// One `MapGenCaves.generate` call: the chunk being carved and what it reads.
struct Caves<'a> {
    primer: &'a mut ChunkPrimer,
    /// `world.getBiome(column).topBlock` per local column, `x + z * 16`.
    column_top: &'a [u16; 256],
}

/// `MapGenCaves.generate`: carves cave tunnels and rooms into `primer` (chunk `x`, `z`).
pub fn generate(world_seed: i64, x: i32, z: i32, primer: &mut ChunkPrimer, column_top: &[u16; 256]) {
    let mut caves = Caves { primer, column_top };
    for_each_source_chunk(world_seed, x, z, |rand, chunk_x, chunk_z| {
        caves.recursive_generate(rand, chunk_x, chunk_z, x, z);
    });
}

impl Caves<'_> {
    /// `MapGenCaves.addRoom`: a wide, flat tunnel segment of the room's length. The width draw
    /// happens after the caller has drawn the seed.
    fn add_room(&mut self, rand: &mut JavaRandom, seed: i64, cx: i32, cz: i32, x: f64, y: f64, z: f64) {
        let width = 1.0f32 + rand.next_float() * 6.0f32;
        self.add_tunnel(seed, cx, cz, x, y, z, width, 0.0, 0.0, -1, -1, 0.5);
    }

    /// `MapGenCaves.addTunnel`. Parameter names follow the MCP argument order: `cx`/`cz` the chunk
    /// being carved, `x`/`y`/`z` the start, then width, yaw, pitch, start step, length and the
    /// vertical squash.
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
        let d0 = (cx * 16 + 8) as f64;
        let d1 = (cz * 16 + 8) as f64;
        let mut f = 0.0f32;
        let mut f1 = 0.0f32;
        let mut random = JavaRandom::new(seed);
        if max_len <= 0 {
            let i = RANGE * 16 - 16;
            max_len = i - random.next_int_bounded(i / 4);
        }
        let mut flag2 = false;
        if step == -1 {
            step = max_len / 2;
            flag2 = true;
        }
        let j = random.next_int_bounded(max_len / 2) + max_len / 4;
        let flag = random.next_int_bounded(6) == 0;

        while step < max_len {
            let d2 = 1.5 + (math_helper::sin(step as f32 * 3.1415927f32 / max_len as f32) * width) as f64;
            let d3 = d2 * y_scale;
            let f2 = math_helper::cos(pitch);
            let f3 = math_helper::sin(pitch);
            x += (math_helper::cos(yaw) * f2) as f64;
            y += f3 as f64;
            z += (math_helper::sin(yaw) * f2) as f64;
            if flag {
                pitch *= 0.92f32;
            } else {
                pitch *= 0.7f32;
            }
            pitch += f1 * 0.1f32;
            yaw += f * 0.1f32;
            f1 *= 0.9f32;
            f *= 0.75f32;
            let (a, b, c) = (random.next_float(), random.next_float(), random.next_float());
            f1 += (a - b) * c * 2.0f32;
            let (a, b, c) = (random.next_float(), random.next_float(), random.next_float());
            f += (a - b) * c * 4.0f32;

            if !flag2 && step == j && width > 1.0f32 && max_len > 0 {
                for side in [-1.5707964f32, 1.5707964f32] {
                    let s = random.next_long();
                    let w = random.next_float() * 0.5f32 + 0.5f32;
                    self.add_tunnel(s, cx, cz, x, y, z, w, yaw + side, pitch / 3.0f32, step, max_len, 1.0);
                }
                return;
            }

            if flag2 || random.next_int_bounded(4) != 0 {
                let d4 = x - d0;
                let d5 = z - d1;
                let d6 = (max_len - step) as f64;
                let d7 = (width + 2.0f32 + 16.0f32) as f64;
                if d4 * d4 + d5 * d5 - d6 * d6 > d7 * d7 {
                    return;
                }

                if x >= d0 - 16.0 - d2 * 2.0
                    && z >= d1 - 16.0 - d2 * 2.0
                    && x <= d0 + 16.0 + d2 * 2.0
                    && z <= d1 + 16.0 + d2 * 2.0
                {
                    let (k2, k, l2, l, i3, i1) = carve_bounds(cx, cz, x, y, z, d2, d3);
                    if !hits_water(self.primer, k2, k, l2, l, i3, i1) {
                        for j3 in k2..k {
                            let d10 = ((j3 + cx * 16) as f64 + 0.5 - x) / d2;
                            for i2 in i3..i1 {
                                let d8 = ((i2 + cz * 16) as f64 + 0.5 - z) / d2;
                                let mut flag1 = false;
                                if d10 * d10 + d8 * d8 < 1.0 {
                                    let mut j2 = l;
                                    while j2 > l2 {
                                        let d9 = ((j2 - 1) as f64 + 0.5 - y) / d3;
                                        if d9 > -0.7 && d10 * d10 + d9 * d9 + d8 * d8 < 1.0 {
                                            let iblockstate1 = self.primer.get(j3, j2, i2);
                                            let iblockstate2 = self.primer.get(j3, j2 + 1, i2);
                                            let b = blocks::block_of(iblockstate1);
                                            if b == id::GRASS || b == id::MYCELIUM {
                                                flag1 = true;
                                            }
                                            if can_replace_block(iblockstate1, iblockstate2) {
                                                if j2 - 1 < 10 {
                                                    self.primer.set(j3, j2, i2, blocks::LAVA);
                                                } else {
                                                    self.primer.set(j3, j2, i2, blocks::AIR);
                                                    if flag1 && blocks::block_of(self.primer.get(j3, j2 - 1, i2)) == id::DIRT {
                                                        let top = self.column_top[(j3 + i2 * 16) as usize];
                                                        self.primer.set(j3, j2 - 1, i2, blocks::default_state(top));
                                                    }
                                                }
                                            }
                                        }
                                        j2 -= 1;
                                    }
                                }
                            }
                        }
                        if flag2 {
                            break;
                        }
                    }
                }
            }
            step += 1;
        }
    }

    /// `MapGenCaves.recursiveGenerate`: rarely any caves; each is a room plus up to three tunnels,
    /// or a single tunnel.
    fn recursive_generate(&mut self, rand: &mut JavaRandom, chunk_x: i32, chunk_z: i32, original_x: i32, original_z: i32) {
        let a = rand.next_int_bounded(15) + 1;
        let b = rand.next_int_bounded(a) + 1;
        let mut i = rand.next_int_bounded(b);
        if rand.next_int_bounded(7) != 0 {
            i = 0;
        }
        for _ in 0..i {
            let d0 = (chunk_x * 16 + rand.next_int_bounded(16)) as f64;
            let n = rand.next_int_bounded(120) + 8;
            let d1 = rand.next_int_bounded(n) as f64;
            let d2 = (chunk_z * 16 + rand.next_int_bounded(16)) as f64;
            let mut k = 1;
            if rand.next_int_bounded(4) == 0 {
                let seed = rand.next_long();
                self.add_room(rand, seed, original_x, original_z, d0, d1, d2);
                k += rand.next_int_bounded(4);
            }
            for _ in 0..k {
                let f = rand.next_float() * 6.2831855f32;
                let f1 = (rand.next_float() - 0.5f32) * 2.0f32 / 8.0f32;
                let mut f2 = rand.next_float() * 2.0f32 + rand.next_float();
                if rand.next_int_bounded(10) == 0 {
                    let (p, q) = (rand.next_float(), rand.next_float());
                    f2 *= p * q * 3.0f32 + 1.0f32;
                }
                let seed = rand.next_long();
                self.add_tunnel(seed, original_x, original_z, d0, d1, d2, f2, f, f1, 0, 0, 1.0);
            }
        }
    }
}

/// The clamped carve box shared by caves and ravines: `(k2, k, l2, l, i3, i1)` as vanilla names
/// them, x in `k2..k`, y in `l2..=l`, z in `i3..i1`, all local to chunk `cx`, `cz`.
#[inline]
pub(crate) fn carve_bounds(cx: i32, cz: i32, x: f64, y: f64, z: f64, dh: f64, dv: f64) -> (i32, i32, i32, i32, i32, i32) {
    let mut k2 = floor(x - dh) - cx * 16 - 1;
    let mut k = floor(x + dh) - cx * 16 + 1;
    let mut l2 = floor(y - dv) - 1;
    let mut l = floor(y + dv) + 1;
    let mut i3 = floor(z - dh) - cz * 16 - 1;
    let mut i1 = floor(z + dh) - cz * 16 + 1;
    if k2 < 0 {
        k2 = 0;
    }
    if k > 16 {
        k = 16;
    }
    if l2 < 1 {
        l2 = 1;
    }
    if l > 248 {
        l = 248;
    }
    if i3 < 0 {
        i3 = 0;
    }
    if i1 > 16 {
        i1 = 16;
    }
    (k2, k, l2, l, i3, i1)
}

/// The water scan before carving, shared by caves and ravines. Edge columns are scanned top to
/// bottom; interior columns only at `l + 1` and `l2 - 1` (the loop variable jumps to `l2`).
#[inline]
pub(crate) fn hits_water(primer: &ChunkPrimer, k2: i32, k: i32, l2: i32, l: i32, i3: i32, i1: i32) -> bool {
    let mut flag3 = false;
    let mut j1 = k2;
    while !flag3 && j1 < k {
        let mut k1 = i3;
        while !flag3 && k1 < i1 {
            let mut l1 = l + 1;
            while !flag3 && l1 >= l2 - 1 {
                if (0..256).contains(&l1) {
                    let b = blocks::block_of(primer.get(j1, l1, k1));
                    if b == id::FLOWING_WATER || b == id::WATER {
                        flag3 = true;
                    }
                    if l1 != l2 - 1 && j1 != k2 && j1 != k - 1 && k1 != i3 && k1 != i1 - 1 {
                        l1 = l2;
                    }
                }
                l1 -= 1;
            }
            k1 += 1;
        }
        j1 += 1;
    }
    flag3
}

/// `MapGenCaves.canReplaceBlock(IBlockState, IBlockState)`
fn can_replace_block(state: u16, above: u16) -> bool {
    match blocks::block_of(state) {
        id::STONE
        | id::DIRT
        | id::GRASS
        | id::HARDENED_CLAY
        | id::STAINED_HARDENED_CLAY
        | id::SANDSTONE
        | id::RED_SANDSTONE
        | id::MYCELIUM
        | id::SNOW_LAYER => true,
        id::SAND | id::GRAVEL => !blocks::is_water(above),
        _ => false,
    }
}
