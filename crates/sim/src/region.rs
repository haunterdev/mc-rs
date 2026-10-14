//! The block access a chunk is meshed against: the chunk and its eight neighbors, with sky and
//! block light computed over all nine, the way `RegionRenderCache` / `ChunkCache` serves
//! `RenderChunk.rebuildChunk`.
//!
//! The generator has no lighting engine, so light is computed here as the state vanilla's
//! `World.checkLightFor` converges to: sky light 15 at and above the height map, block light 15 in
//! lava, and elsewhere the brightest neighbor minus the block's opacity (at least 1). Light
//! travels at most 15 blocks, so the 16-block margin around the center chunk makes every value the
//! mesher reads (at most one block outside the center chunk) exact for the nine chunks given.

use std::sync::OnceLock;

use worldgen::blocks::id;
use worldgen::Chunk;

/// `Material` as far as rendering and lighting ask about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mat {
    Air,
    Water,
    Lava,
    /// A material that is not solid: `MaterialLogic` (plants, vines, snow layers) and the
    /// transparent ones.
    Logic,
    Ice,
    /// Every solid material (rock, ground, wood, leaves, cactus, gourd, crafted snow).
    Solid,
}

/// `BlockRenderLayer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Solid,
    CutoutMipped,
    Cutout,
    Translucent,
}

/// The per-block answers `BlockModelRenderer`, `BlockFluidRenderer` and the light engine need.
#[derive(Clone, Copy, Debug)]
pub struct Props {
    pub mat: Mat,
    /// `isOpaqueCube`
    pub opaque_cube: bool,
    /// `isFullCube`
    pub full_cube: bool,
    /// `getLightOpacity`
    pub opacity: u8,
    /// `getLightValue`
    pub light: u8,
    pub layer: Layer,
    /// `getRenderType() == LIQUID`
    pub liquid: bool,
    /// `getRenderType() == INVISIBLE`
    pub invisible: bool,
    /// `Block.useNeighborBrightness`
    pub neighbor_brightness: bool,
}

impl Props {
    /// `Material.isSolid`
    pub fn solid_material(&self) -> bool {
        !matches!(self.mat, Mat::Air | Mat::Water | Mat::Lava | Mat::Logic)
    }

    /// `Material.blocksMovement`
    pub fn blocks_movement(&self) -> bool {
        self.solid_material()
    }

    /// `Block.getAmbientOcclusionLightValue`: `isBlockNormalCube() ? 0.2 : 1.0`, where
    /// `isBlockNormalCube` is `material.blocksMovement() && isFullCube()`.
    pub fn ao_light_value(&self) -> f32 {
        if self.blocks_movement() && self.full_cube {
            0.2
        } else {
            1.0
        }
    }

    /// `Block.isTranslucent`: `!material.blocksLight()`. Liquids and ice block light.
    pub fn translucent(&self) -> bool {
        matches!(self.mat, Mat::Air | Mat::Logic)
    }

    /// `Block.useNeighborBrightness`, as `Block.registerBlocks` assigns it: every non-air block
    /// that is a `BlockStairs` or `BlockSlab`, farmland or a grass path, translucent, or of
    /// opacity 0.
    pub fn use_neighbor_brightness(&self) -> bool {
        self.neighbor_brightness
    }
}

/// `Block.getBlockLayer` of the blocks generation and population place.
fn layer_of(block: u16) -> Layer {
    match block {
        // BlockLeaves.getBlockLayer is CUTOUT_MIPPED with fancy graphics.
        id::GRASS | id::LEAVES | id::LEAVES2 => Layer::CutoutMipped,
        id::TALLGRASS
        | id::DEADBUSH
        | id::YELLOW_FLOWER
        | id::RED_FLOWER
        | id::BROWN_MUSHROOM
        | id::RED_MUSHROOM
        | id::DOUBLE_PLANT
        | id::REEDS
        | id::CACTUS
        | id::VINE
        | id::COCOA
        | id::MOB_SPAWNER
        | id::GLASS
        | id::TORCH
        | id::WATERLILY => Layer::Cutout,
        id::ICE | id::WATER | id::FLOWING_WATER => Layer::Translucent,
        _ => Layer::Solid,
    }
}

/// The `BlockStairs` ids: oak, stone, brick, stone brick, nether brick, sandstone, spruce, birch,
/// jungle, quartz, acacia, dark oak, red sandstone, purpur.
const STAIRS: [u16; 14] = [53, 67, 108, 109, 114, 128, 134, 135, 136, 156, 163, 164, 180, 203];
const GRASS_PATH: u16 = 208;

/// One state's properties, from the table `tools/gen_props.py` generates from the Java blocks.
fn derive(s: u16) -> Props {
    use worldgen::populate::props::material;
    use worldgen::populate::state as st;
    let block = s >> 4;
    // Block.getBlockById: an unregistered id is air.
    if block >= 256 {
        return derive(0);
    }
    let mat = match st::mat(s) {
        material::AIR => Mat::Air,
        material::WATER => Mat::Water,
        material::LAVA => Mat::Lava,
        material::ICE => Mat::Ice,
        _ if !st::mat_solid(s) => Mat::Logic,
        _ => Mat::Solid,
    };
    let leaves = matches!(block, id::LEAVES | id::LEAVES2);
    Props {
        mat,
        // BlockLeaves.isOpaqueCube is !leavesFancy; the table holds the fast value, the viewer
        // draws fancy leaves.
        opaque_cube: st::is_opaque_cube(s) && !leaves,
        full_cube: st::is_full_cube(s),
        opacity: st::opacity(s).clamp(0, 255) as u8,
        light: st::light_value(s) as u8,
        layer: layer_of(block),
        liquid: matches!(block, id::FLOWING_WATER | id::WATER | id::FLOWING_LAVA | id::LAVA),
        // getRenderType: INVISIBLE for air, ENTITYBLOCK_ANIMATED for the chest (a tile entity
        // renderer the viewer does not draw).
        invisible: mat == Mat::Air || block == id::CHEST,
        neighbor_brightness: mat != Mat::Air
            && (STAIRS.contains(&block)
                || matches!(
                    block,
                    id::STONE_SLAB
                        | id::DOUBLE_STONE_SLAB
                        | id::WOODEN_SLAB
                        | id::DOUBLE_WOODEN_SLAB
                        | id::STONE_SLAB2
                        | id::DOUBLE_STONE_SLAB2
                        | id::PURPUR_SLAB
                        | id::PURPUR_DOUBLE_SLAB
                        | id::FARMLAND
                        | GRASS_PATH
                )
                || matches!(mat, Mat::Air | Mat::Logic)
                || st::opacity(s) == 0),
    }
}

/// `Block` properties of every block state.
pub fn props(state: u16) -> &'static Props {
    static T: OnceLock<Vec<Props>> = OnceLock::new();
    let t = T.get_or_init(|| (0..=u16::MAX).map(derive).collect());
    &t[state as usize]
}

/// `getLightOpacity` by state id.
fn opacity_table() -> &'static [u8] {
    static T: OnceLock<Vec<u8>> = OnceLock::new();
    T.get_or_init(|| (0..1usize << 16).map(|s| props(s as u16).opacity).collect())
}

/// Edge of the region in blocks: the center chunk plus 16 on each side.
pub const W: i32 = 48;
const H: i32 = 256;

pub struct Region {
    /// World block coordinates of the center chunk's (0, 0, 0).
    pub origin: (i32, i32),
    states: Vec<u16>,
    /// `sky << 4 | block`
    light: Vec<u8>,
    biomes: Vec<u8>,
}

#[inline(always)]
fn cell(x: i32, y: i32, z: i32) -> usize {
    ((x * W + z) * H + y) as usize
}

impl Region {
    /// `chunks` is the 3x3 neighborhood, indexed `(dx + 1) + (dz + 1) * 3`.
    pub fn new(chunks: [&Chunk; 9]) -> Region {
        let c = chunks[4];
        let mut states = vec![0u16; (W * W * H) as usize];
        let mut biomes = vec![0u8; (W * W) as usize];
        for (i, ch) in chunks.iter().enumerate() {
            let (cx, cz) = ((i % 3) as i32 * 16, (i / 3) as i32 * 16);
            for x in 0..16 {
                for z in 0..16 {
                    let src = (x << 12 | z << 8) as usize;
                    let dst = cell(cx + x, 0, cz + z);
                    states[dst..dst + 256].copy_from_slice(&ch.blocks.data[src..src + 256]);
                    biomes[((cx + x) * W + cz + z) as usize] = ch.biomes[(x + z * 16) as usize];
                }
            }
        }
        let mut r = Region { origin: (c.x * 16, c.z * 16), states, light: vec![0; (W * W * H) as usize], biomes };
        r.compute_light();
        r
    }

    #[inline(always)]
    fn inside(x: i32, z: i32) -> bool {
        (-16..32).contains(&x) && (-16..32).contains(&z)
    }

    /// `getBlockState` at center-chunk-relative coordinates. Outside the height range it is air.
    #[inline(always)]
    pub fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        if !(0..H).contains(&y) || !Self::inside(x, z) {
            return 0;
        }
        self.states[cell(x + 16, y, z + 16)]
    }

    #[inline(always)]
    pub fn props(&self, x: i32, y: i32, z: i32) -> &'static Props {
        props(self.state(x, y, z))
    }

    /// `ChunkCache.getLightForExt(SKY / BLOCK)`; outside the height range it is
    /// `EnumSkyBlock.defaultLightValue`: sky 15, block 0.
    #[inline(always)]
    pub fn light(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        if !(0..H).contains(&y) {
            return (15, 0);
        }
        if !Self::inside(x, z) {
            return (0, 0);
        }
        let v = self.light[cell(x + 16, y, z + 16)];
        (v >> 4, v & 15)
    }

    /// `ChunkCache.getCombinedLight(pos, lightValue)`: `sky << 20 | max(block, lightValue) << 4`.
    #[inline(always)]
    pub fn combined_light(&self, x: i32, y: i32, z: i32, light_value: u8) -> i32 {
        // ChunkCache.getLightForExt: a block that uses neighbor brightness (slabs, stairs, plants)
        // takes the brightest of its six neighbors, each light type on its own.
        let (s, b) = if (0..256).contains(&y) && self.props(x, y, z).use_neighbor_brightness() {
            [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)]
                .iter()
                .map(|&(dx, dy, dz)| self.light(x + dx, y + dy, z + dz))
                .fold((0, 0), |(s, b), (s1, b1)| (s.max(s1), b.max(b1)))
        } else {
            self.light(x, y, z)
        };
        (s as i32) << 20 | (b.max(light_value) as i32) << 4
    }

    /// The biome id at a column, center-chunk relative.
    #[inline(always)]
    pub fn biome(&self, x: i32, z: i32) -> u8 {
        if !Self::inside(x, z) {
            return 0;
        }
        self.biomes[((x + 16) * W + z + 16) as usize]
    }

    /// The center chunk's light, run-length coded per column, for lookups at the camera after the
    /// region is gone.
    pub fn center_light(&self) -> ColumnLight {
        ColumnLight(Columns::build(|x, y, z| self.light[cell(x + 16, y, z + 16)]))
    }

    /// The center chunk's block states, run-length coded per column: what the chunk is kept as
    /// once it is meshed, for lookups and to mesh it again.
    pub fn center_states(&self) -> Columns<u16> {
        Columns::build(|x, y, z| self.states[cell(x + 16, y, z + 16)])
    }

    /// The center chunk's biome ids, `x + z * 16` as `Chunk.biomes`.
    pub fn center_biomes(&self) -> [u8; 256] {
        std::array::from_fn(|i| self.biome(i as i32 % 16, i as i32 / 16))
    }

    /// Sky and block light over the whole region, by levels: every cell that holds level `n` hands
    /// `n - max(1, opacity)` to its neighbors before any cell at level `n - 1` is processed.
    fn compute_light(&mut self) {
        let opacity = opacity_table();
        let mut heights = vec![0i32; (W * W) as usize];
        for x in 0..W {
            for z in 0..W {
                let base = cell(x, 0, z);
                let col = &self.states[base..base + H as usize];
                // Chunk.generateHeightMap: one above the highest block with any opacity.
                let h = col.iter().rposition(|&s| opacity[s as usize] != 0).map_or(0, |y| y as i32 + 1);
                heights[(x * W + z) as usize] = h;
                for y in h..H {
                    self.light[base + y as usize] = 15 << 4;
                }
            }
        }
        let mut sky: Vec<Vec<u32>> = vec![Vec::new(); 16];
        let mut block: Vec<Vec<u32>> = vec![Vec::new(); 16];
        for x in 0..W {
            for z in 0..W {
                let h = heights[(x * W + z) as usize];
                // Seeds: the lowest directly lit cell, and every directly lit cell beside a
                // column that is still dark at that height.
                let mut top = h;
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (nx, nz) = (x + dx, z + dz);
                    if (0..W).contains(&nx) && (0..W).contains(&nz) {
                        top = top.max(heights[(nx * W + nz) as usize]);
                    }
                }
                for y in h..top.max(h + 1).min(H) {
                    sky[15].push(cell(x, y, z) as u32);
                }
                let base = cell(x, 0, z);
                for y in 0..H {
                    let l = props(self.states[base + y as usize]).light;
                    if l > 0 {
                        self.light[base + y as usize] |= l;
                        block[l as usize].push((base + y as usize) as u32);
                    }
                }
            }
        }
        self.spread(&mut sky, opacity, 4);
        self.spread(&mut block, opacity, 0);
    }

    fn spread(&mut self, queues: &mut [Vec<u32>], opacity: &[u8], shift: u8) {
        let mask = 15u8 << shift;
        for level in (1..16).rev() {
            let q = std::mem::take(&mut queues[level]);
            for &c in &q {
                let c = c as i32;
                if ((self.light[c as usize] & mask) >> shift) as usize != level {
                    continue;
                }
                let (y, xz) = (c % H, c / H);
                let (x, z) = (xz / W, xz % W);
                for (dx, dy, dz) in [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)] {
                    let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                    if !(0..W).contains(&nx) || !(0..W).contains(&nz) || !(0..H).contains(&ny) {
                        continue;
                    }
                    let n = cell(nx, ny, nz);
                    let op = opacity[self.states[n] as usize];
                    if op >= 15 {
                        continue;
                    }
                    let new = level as i32 - op.max(1) as i32;
                    if new <= 0 {
                        continue;
                    }
                    let old = ((self.light[n] & mask) >> shift) as i32;
                    if new > old {
                        self.light[n] = (self.light[n] & !mask) | ((new as u8) << shift);
                        queues[new as usize].push(n as u32);
                    }
                }
            }
        }
    }
}

/// One chunk's values, per column `x * 16 + z`, as runs of `(first y, value)`.
pub struct Columns<T> {
    starts: Vec<u32>,
    runs: Vec<(u8, T)>,
}

impl<T: Copy + PartialEq> Columns<T> {
    /// Codes `get(x, y, z)` over `x, z` in 0..16 and `y` in 0..256.
    pub fn build(get: impl Fn(i32, i32, i32) -> T) -> Columns<T> {
        let mut starts = Vec::with_capacity(257);
        let mut runs: Vec<(u8, T)> = Vec::new();
        for x in 0..16 {
            for z in 0..16 {
                starts.push(runs.len() as u32);
                let first = runs.len();
                for y in 0..H {
                    let v = get(x, y, z);
                    if runs.len() == first || runs[runs.len() - 1].1 != v {
                        runs.push((y as u8, v));
                    }
                }
            }
        }
        starts.push(runs.len() as u32);
        runs.shrink_to_fit();
        Columns { starts, runs }
    }

    /// The value at chunk-relative `x, z` and `y` in 0..256.
    pub fn get(&self, x: i32, y: i32, z: i32) -> T {
        let c = (x * 16 + z) as usize;
        let col = &self.runs[self.starts[c] as usize..self.starts[c + 1] as usize];
        let i = col.partition_point(|&(y0, _)| y0 as i32 <= y);
        col[i.saturating_sub(1)].1
    }

    /// Every run of one column, lowest first, with the y it ends before.
    pub fn column(&self, x: i32, z: i32) -> impl Iterator<Item = (i32, i32, T)> + '_ {
        let c = (x * 16 + z) as usize;
        let col = &self.runs[self.starts[c] as usize..self.starts[c + 1] as usize];
        col.iter().enumerate().map(move |(i, &(y0, v))| (y0 as i32, col.get(i + 1).map_or(H, |r| r.0 as i32), v))
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.starts.len() * 4 + self.runs.len() * std::mem::size_of::<(u8, T)>()
    }
}

/// One chunk's light, `sky << 4 | block` per block.
pub struct ColumnLight(pub Columns<u8>);

impl ColumnLight {
    /// `(sky, block)` at chunk-relative `x, z` and `y` in 0..256.
    pub fn get(&self, x: i32, y: i32, z: i32) -> (u8, u8) {
        let v = self.0.get(x, y, z);
        (v >> 4, v & 15)
    }

    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.0.bytes()
    }
}

/// `BlockLiquid.getLiquidHeightPercent`
pub fn liquid_height_percent(meta: i32) -> f32 {
    let meta = if meta >= 8 { 0 } else { meta };
    (meta + 1) as f32 / 9.0
}

/// `MathHelper.getCoordinateRandom`.
pub fn coordinate_random(x: i32, y: i32, z: i32) -> i64 {
    let mut i = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ y as i64;
    i = i.wrapping_mul(i).wrapping_mul(42317861).wrapping_add(i.wrapping_mul(11));
    i
}

/// `Block.getOffset`: `EnumOffsetType.XZ` for `BlockFlower` and `BlockDoublePlant`, `XYZ` for
/// `BlockTallGrass`.
pub fn offset(state: u16, wx: i32, wz: i32) -> [f64; 3] {
    let xyz = match state >> 4 {
        id::YELLOW_FLOWER | id::RED_FLOWER | id::DOUBLE_PLANT => false,
        id::TALLGRASS => true,
        _ => return [0.0; 3],
    };
    let i = coordinate_random(wx, 0, wz);
    let n = |shift: u32| (i >> shift & 15) as f32 / 15.0;
    let y = if xyz { (n(20) as f64 - 1.0) * 0.2 } else { 0.0 };
    [(n(16) as f64 - 0.5) * 0.5, y, (n(24) as f64 - 0.5) * 0.5]
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::testing::{chunk, region_of};
    use worldgen::blocks::{state, LAVA, STONE, WATER};

    #[test]
    fn open_sky_is_full_and_rock_is_dark() {
        let r = region_of(chunk(0, 0, 64, |_| ()), 64);
        assert_eq!(r.light(5, 64, 5), (15, 0));
        assert_eq!(r.light(5, 63, 5), (0, 0));
        assert_eq!(r.light(5, 300, 5), (15, 0));
    }

    #[test]
    fn water_takes_three_per_block_and_caves_fall_off_by_one() {
        let r = region_of(
            chunk(0, 0, 64, |p| {
                for y in 60..64 {
                    p.set(8, y, 8, WATER);
                }
                // A tunnel two blocks under the surface, open to a shaft at x = 2.
                for y in 61..64 {
                    p.set(2, y, 2, 0);
                }
                for x in 2..8 {
                    p.set(x, 61, 2, 0);
                }
                p.set(12, 30, 12, LAVA);
                p.set(13, 30, 12, 0);
            }),
            64,
        );
        assert_eq!(r.light(8, 63, 8).0, 12);
        assert_eq!(r.light(8, 60, 8).0, 3);
        assert_eq!(r.light(2, 61, 2).0, 15);
        assert_eq!(r.light(7, 61, 2).0, 10);
        assert_eq!(r.light(12, 30, 12), (0, 15));
        assert_eq!(r.light(13, 30, 12), (0, 14));
        assert_eq!(state(id::LAVA, 0), LAVA);
    }

    #[test]
    fn column_light_matches_the_region() {
        let r = region_of(
            chunk(0, 0, 64, |p| {
                for y in 60..64 {
                    p.set(8, y, 8, WATER);
                }
                p.set(12, 30, 12, LAVA);
                p.set(13, 30, 12, 0);
            }),
            64,
        );
        let c = r.center_light();
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    assert_eq!(c.get(x, y, z), r.light(x, y, z), "{x} {y} {z}");
                }
            }
        }
        assert!(c.bytes() < 16 * 16 * 256 / 8);
        let states = r.center_states();
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    assert_eq!(states.get(x, y, z), r.state(x, y, z));
                }
                let mut next = 0;
                for (y0, y1, v) in states.column(x, z) {
                    assert_eq!(y0, next);
                    assert!(y1 > y0);
                    assert_eq!(v, r.state(x, y0, z));
                    next = y1;
                }
                assert_eq!(next, 256);
            }
        }
        assert_eq!(r.center_biomes(), [1; 256]);
        assert!(props(LAVA).use_neighbor_brightness());
        assert!(!props(WATER).use_neighbor_brightness());
        assert!(!props(STONE).use_neighbor_brightness());
    }
}
