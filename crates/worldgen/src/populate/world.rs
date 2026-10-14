//! The `World` and `Chunk` population runs against: chunk sections with sky light, the height and
//! precipitation maps, `setBlockState` with its neighbour updates, and the immediate tick branch of
//! `WorldServer.updateBlockTick`.
//!
//! This is the world the fixture harness (`harness/.../HarnessWorld.java`) builds: chunks come from a
//! map that generates on access, a chunk counts as loaded once it exists, `checkLightFor` does nothing
//! (block light stays 0 and sky light only changes through `Chunk`'s own column updates), and
//! scheduled ticks only run while `scheduledUpdatesAreImmediate` is set.

use std::collections::HashMap;

use super::behavior;
use super::props::material;
use super::state::{self, Facing, Pos};
use crate::biome::{self, Biome};
use crate::blocks::{self, id};
use crate::generator::ChunkGenerator;
use crate::java_random::JavaRandom;
use crate::noise::perlin::NoiseGeneratorPerlin;

/// `ExtendedBlockStorage`: 16x16x16 states and their sky light, indexed `y << 8 | z << 4 | x`.
pub struct Section {
    pub blocks: [u16; 4096],
    pub sky: [u8; 4096],
}

#[inline(always)]
fn sidx(x: i32, y: i32, z: i32) -> usize {
    ((y & 15) << 8 | (z & 15) << 4 | (x & 15)) as usize
}

/// `net.minecraft.world.chunk.Chunk`, the parts population reads and writes.
pub struct PopChunk {
    pub x: i32,
    pub z: i32,
    pub sections: [Option<Box<Section>>; 16],
    /// `heightMap`, indexed `z << 4 | x`.
    pub height_map: [i32; 256],
    pub height_map_minimum: i32,
    /// `precipitationHeightMap`, indexed `z << 4 | x`.
    pub(super) precip: [i32; 256],
    /// `biomeArray`, indexed `z << 4 | x`.
    pub biomes: [u8; 256],
}

impl PopChunk {
    /// `Chunk(World, ChunkPrimer, int, int)`, then the biome array and `generateSkylightMap` from the
    /// end of `ChunkGeneratorOverworld.generateChunk`.
    pub fn from_generated(c: crate::Chunk) -> PopChunk {
        let mut chunk = PopChunk {
            x: c.x,
            z: c.z,
            sections: Default::default(),
            height_map: [0; 256],
            height_map_minimum: 0,
            precip: [-999; 256],
            biomes: c.biomes,
        };
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..256 {
                    let s = c.blocks.get(x, y, z);
                    if state::mat(s) != material::AIR {
                        let sec = chunk.sections[(y >> 4) as usize]
                            .get_or_insert_with(|| Box::new(Section { blocks: [blocks::AIR; 4096], sky: [0; 4096] }));
                        sec.blocks[sidx(x, y, z)] = s;
                    }
                }
            }
        }
        chunk.generate_skylight_map();
        chunk
    }

    /// `Chunk.getBlockState(int, int, int)`
    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        if (0..256).contains(&y) {
            if let Some(sec) = &self.sections[(y >> 4) as usize] {
                return sec.blocks[sidx(x, y, z)];
            }
        }
        blocks::AIR
    }

    fn opacity(&self, x: i32, y: i32, z: i32) -> i32 {
        state::opacity(self.get(x, y, z))
    }

    fn set_sky(&mut self, x: i32, y: i32, z: i32, v: i32) {
        if let Some(sec) = &mut self.sections[(y >> 4) as usize] {
            sec.sky[sidx(x, y, z)] = v as u8;
        }
    }

    /// `Chunk.getTopFilledSegment()`
    pub fn top_filled_segment(&self) -> i32 {
        for i in (0..16).rev() {
            if self.sections[i].is_some() {
                return (i as i32) << 4;
            }
        }
        0
    }

    /// `Chunk.getHeightValue(int, int)`
    pub fn height_value(&self, x: i32, z: i32) -> i32 {
        self.height_map[(z << 4 | x) as usize]
    }

    /// Not in the game: writes a state another world already settled (a block another player
    /// changed, a saved edit) with no block behavior. False where nothing changed. The caller
    /// runs [`Self::generate_skylight_map`] once its writes to this chunk are in.
    pub fn set_raw(&mut self, x: i32, y: i32, z: i32, s: u16) -> bool {
        if !(0..256).contains(&y) {
            return false;
        }
        let l = ((z & 15) << 4 | (x & 15)) as usize;
        if y >= self.precip[l] - 1 {
            self.precip[l] = -999;
        }
        let si = (y >> 4) as usize;
        if self.sections[si].is_none() {
            if s == blocks::AIR {
                return false;
            }
            self.sections[si] = Some(Box::new(Section { blocks: [blocks::AIR; 4096], sky: [0; 4096] }));
        }
        let b = &mut self.sections[si].as_mut().unwrap().blocks[sidx(x, y, z)];
        if *b == s {
            return false;
        }
        *b = s;
        true
    }

    /// `Chunk.generateSkylightMap()`
    pub fn generate_skylight_map(&mut self) {
        let i = self.top_filled_segment();
        self.height_map_minimum = i32::MAX;
        for j in 0..16 {
            for k in 0..16 {
                self.precip[(j + (k << 4)) as usize] = -999;
                let mut l = i + 16;
                while l > 0 {
                    if self.opacity(j, l - 1, k) != 0 {
                        self.height_map[(k << 4 | j) as usize] = l;
                        if l < self.height_map_minimum {
                            self.height_map_minimum = l;
                        }
                        break;
                    }
                    l -= 1;
                }
                let mut k1 = 15;
                let mut i1 = i + 16 - 1;
                loop {
                    let mut j1 = self.opacity(j, i1, k);
                    if j1 == 0 && k1 != 15 {
                        j1 = 1;
                    }
                    k1 -= j1;
                    if k1 > 0 {
                        self.set_sky(j, i1, k, k1);
                    }
                    i1 -= 1;
                    if i1 <= 0 || k1 <= 0 {
                        break;
                    }
                }
            }
        }
        // DROPPED: this.dirty = true
    }

    /// `Chunk.relightBlock(int, int, int)`
    fn relight_block(&mut self, x: i32, y: i32, z: i32) {
        let hi = (z << 4 | x) as usize;
        let i = self.height_map[hi] & 255;
        let mut j = i.max(y);
        while j > 0 && self.opacity(x, j - 1, z) == 0 {
            j -= 1;
        }
        if j != i {
            // DROPPED: world.markBlocksDirtyVertical (render only).
            self.height_map[hi] = j;
            if j < i {
                for j1 in j..i {
                    self.set_sky(x, j1, z, 15);
                }
            } else {
                for i1 in i..j {
                    self.set_sky(x, i1, z, 0);
                }
            }
            let mut k1 = 15;
            while j > 0 && k1 > 0 {
                j -= 1;
                let mut i2 = self.opacity(x, j, z);
                if i2 == 0 {
                    i2 = 1;
                }
                k1 -= i2;
                if k1 < 0 {
                    k1 = 0;
                }
                self.set_sky(x, j, z, k1);
            }
            let l1 = self.height_map[hi];
            if l1 < self.height_map_minimum {
                self.height_map_minimum = l1;
            }
            // DROPPED: updateSkylightNeighborHeight for the four neighbours and the column. It only
            // calls world.checkLightFor, which the harness world turns off.
        }
    }

    /// `Chunk.getLightSubtracted(pos, 0)`: block light is always 0 here.
    fn light_subtracted(&self, x: i32, y: i32, z: i32) -> i32 {
        match &self.sections[(y >> 4) as usize] {
            None => 15,
            Some(sec) => sec.sky[sidx(x, y, z)] as i32,
        }
    }

    /// `Chunk.getPrecipitationHeight(BlockPos)`, returning the y.
    fn precipitation_height(&mut self, x: i32, z: i32) -> i32 {
        let k = ((x & 15) | (z & 15) << 4) as usize;
        if self.precip[k] == -999 {
            let mut y = self.top_filled_segment() + 15;
            let mut i1 = -1;
            while y > 0 && i1 == -1 {
                let s = self.get(x & 15, y, z & 15);
                if !state::blocks_movement(s) && !state::mat_liquid(s) {
                    y -= 1;
                } else {
                    i1 = y + 1;
                }
            }
            self.precip[k] = i1;
        }
        self.precip[k]
    }
}

/// Values that are Java statics in vanilla, so they carry from one populated chunk to the next for
/// as long as the JVM lives (the harness runs every region of a seed in one JVM).
#[derive(Clone)]
pub struct PopStatics {
    /// `WorldGenBigTree.heightLimit` on the shared `Biome.BIG_TREE_FEATURE` instance.
    pub big_tree_height_limit: i32,
    /// `WorldGenBigTree.leafDistanceLimit` on the same instance: 4 until `setDecorationDefaults`.
    pub big_tree_leaf_distance_limit: i32,
}

impl Default for PopStatics {
    fn default() -> Self {
        PopStatics { big_tree_height_limit: 0, big_tree_leaf_distance_limit: 4 }
    }
}

/// Which `Random` a scheduled tick runs with: the generator's (from `immediateBlockTick`) or the
/// world's (`WorldServer.updateBlockTick` passes `this.rand`).
pub enum TickRand<'a> {
    World,
    Gen(&'a mut JavaRandom),
}

impl TickRand<'_> {
    pub fn next_int(&mut self, w: &mut PopWorld, bound: i32) -> i32 {
        match self {
            TickRand::World => w.rand.next_int_bounded(bound),
            TickRand::Gen(r) => r.next_int_bounded(bound),
        }
    }
}

/// The world population runs in.
pub struct PopWorld<'g> {
    pub gen: &'g ChunkGenerator,
    chunks: HashMap<(i32, i32), PopChunk>,
    /// `World.rand`
    pub rand: JavaRandom,
    /// `World.scheduledUpdatesAreImmediate`
    pub immediate: bool,
    /// `BlockFalling.fallInstantly`
    pub fall_instantly: bool,
    frozen: bool,
    /// Chunks generated on access after [`Self::freeze`].
    pub late_generations: u32,
    /// `Biome.TEMPERATURE_NOISE`
    temperature_noise: NoiseGeneratorPerlin,
    pub statics: PopStatics,
    /// Debug trace box (x0 y0 z0 x1 y1 z1, inclusive), the same lines as the harness `trace` mode.
    pub trace: Option<[i32; 6]>,
    /// When set, every block write lands here in order: the viewer's edits read back what a
    /// break or place changed (plants losing their support and the like).
    pub changes: Option<Vec<(Pos, u16)>>,
}

impl<'g> PopWorld<'g> {
    /// `HarnessWorld(seed)`: `World.rand` is reseeded to 0.
    pub fn new(gen: &'g ChunkGenerator, statics: PopStatics) -> Self {
        PopWorld {
            gen,
            chunks: HashMap::new(),
            rand: JavaRandom::new(0),
            immediate: false,
            fall_instantly: false,
            frozen: false,
            late_generations: 0,
            temperature_noise: NoiseGeneratorPerlin::new(&mut JavaRandom::new(1234), 1),
            statics,
            trace: None,
            changes: None,
        }
    }

    fn traced(&self, p: Pos) -> bool {
        matches!(self.trace, Some(b) if p.x >= b[0] && p.y >= b[1] && p.z >= b[2] && p.x <= b[3] && p.y <= b[4] && p.z <= b[5])
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// `IChunkProvider.provideChunk(x, z)`: generates the chunk if it does not exist.
    pub fn chunk(&mut self, cx: i32, cz: i32) -> &mut PopChunk {
        if !self.chunks.contains_key(&(cx, cz)) {
            if self.frozen {
                self.late_generations += 1;
            }
            let c = PopChunk::from_generated(self.gen.generate(cx, cz));
            self.chunks.insert((cx, cz), c);
        }
        self.chunks.get_mut(&(cx, cz)).unwrap()
    }

    pub fn take_chunk(&mut self, cx: i32, cz: i32) -> Option<PopChunk> {
        self.chunks.remove(&(cx, cz))
    }

    pub fn insert_chunk(&mut self, c: PopChunk) {
        self.chunks.insert((c.x, c.z), c);
    }

    /// A held chunk, never generating one.
    pub fn chunk_if_loaded(&mut self, cx: i32, cz: i32) -> Option<&mut PopChunk> {
        self.chunks.get_mut(&(cx, cz))
    }

    /// A held chunk for reading, never generating one.
    pub fn chunk_ref(&self, cx: i32, cz: i32) -> Option<&PopChunk> {
        self.chunks.get(&(cx, cz))
    }

    /// The positions of every chunk held.
    pub fn loaded(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.chunks.keys().copied()
    }

    /// `isChunkLoaded`: the chunk exists.
    pub fn is_chunk_loaded(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    /// `World.isAreaLoaded(BlockPos, BlockPos)`
    pub fn is_area_loaded(&self, from: Pos, to: Pos) -> bool {
        if to.y >= 0 && from.y < 256 {
            let (x0, z0, x1, z1) = (from.x >> 4, from.z >> 4, to.x >> 4, to.z >> 4);
            for cx in x0..=x1 {
                for cz in z0..=z1 {
                    if !self.is_chunk_loaded(cx, cz) {
                        return false;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// `World.isAreaLoaded(BlockPos center, int radius)`
    pub fn is_area_loaded_r(&self, c: Pos, r: i32) -> bool {
        self.is_area_loaded(c.add(-r, -r, -r), c.add(r, r, r))
    }

    /// `World.getBlockState(BlockPos)`
    pub fn get(&mut self, p: Pos) -> u16 {
        if !(0..256).contains(&p.y) {
            return blocks::AIR;
        }
        self.chunk(p.x >> 4, p.z >> 4).get(p.x & 15, p.y, p.z & 15)
    }

    /// `World.isAirBlock(BlockPos)`
    pub fn is_air(&mut self, p: Pos) -> bool {
        state::mat(self.get(p)) == material::AIR
    }

    /// `World.setBlockState(BlockPos, IBlockState, int)`
    pub fn set(&mut self, p: Pos, s: u16, flags: i32) -> bool {
        if self.traced(p) {
            println!("set {} {} {} {}:{} f{flags} imm {}", p.x, p.y, p.z, s >> 4, s & 15, self.immediate);
        }
        if !(0..256).contains(&p.y) {
            return false;
        }
        let old = match self.chunk_set(p, s) {
            Some(old) => old,
            None => return false,
        };
        // DROPPED: checkLight (the harness world's checkLightFor does nothing).
        // DROPPED: notifyBlockUpdate (render and listener only).
        if flags & 1 != 0 {
            self.notify_neighbors(p, blocks::block_of(old));
            // DROPPED: updateComparatorOutputLevel; nothing population places reads comparators.
        }
        // DROPPED: updateObservingBlocksAt when (flags & 16) == 0; population places no observers.
        true
    }

    /// `World.setBlockState(BlockPos, IBlockState)`: flags 3.
    pub fn set3(&mut self, p: Pos, s: u16) -> bool {
        self.set(p, s, 3)
    }

    /// `World.setBlockToAir(BlockPos)`
    pub fn set_air(&mut self, p: Pos) -> bool {
        self.set(p, blocks::AIR, 3)
    }

    /// `World.destroyBlock(BlockPos, boolean)`
    pub fn destroy_block(&mut self, p: Pos, drop: bool) -> bool {
        let s = self.get(p);
        if state::mat(s) == material::AIR {
            return false;
        }
        // DROPPED: playEvent(2001) (sound and particles).
        if drop {
            behavior::drop_block_as_item(self, p, s);
        }
        self.set(p, blocks::AIR, 3)
    }

    /// `Chunk.setBlockState(BlockPos, IBlockState)`: the old state, or None where vanilla returns null.
    fn chunk_set(&mut self, p: Pos, s: u16) -> Option<u16> {
        let (i, j, k) = (p.x & 15, p.y, p.z & 15);
        let (cx, cz) = (p.x >> 4, p.z >> 4);
        let c = self.chunk(cx, cz);
        let l = (k << 4 | i) as usize;
        if j >= c.precip[l] - 1 {
            c.precip[l] = -999;
        }
        let i1 = c.height_map[l];
        let old = c.get(i, j, k);
        if old == s {
            return None;
        }
        let block = blocks::block_of(s);
        let block1 = blocks::block_of(old);
        let mut flag = false;
        let si = (j >> 4) as usize;
        if c.sections[si].is_none() {
            if block == id::AIR {
                return None;
            }
            c.sections[si] = Some(Box::new(Section { blocks: [blocks::AIR; 4096], sky: [0; 4096] }));
            flag = j >= i1;
        }
        c.sections[si].as_mut().unwrap().blocks[sidx(i, j, k)] = s;
        if let Some(log) = self.changes.as_mut() {
            log.push((p, s));
        }
        if block1 != block {
            behavior::break_block(self, p, old);
        }
        let c = self.chunk(cx, cz);
        if blocks::block_of(c.get(i, j, k)) != block {
            return None;
        }
        if flag {
            c.generate_skylight_map();
        } else {
            let j1 = state::opacity(s);
            if j1 > 0 {
                if j >= i1 {
                    c.relight_block(i, j + 1, k);
                }
            } else if j == i1 - 1 {
                c.relight_block(i, j, k);
            }
            // DROPPED: propagateSkylightOcclusion; it only queues gap-light work for the next chunk tick.
        }
        if block1 != block {
            behavior::on_block_added(self, p, s);
        }
        // DROPPED: tile entities (the dungeon chest's loot and the spawner's mob live there).
        Some(old)
    }

    /// `World.notifyNeighborsOfStateChange(pos, blockType, updateObservers)`
    pub fn notify_neighbors(&mut self, p: Pos, block: u16) {
        for f in [Facing::West, Facing::East, Facing::Down, Facing::Up, Facing::North, Facing::South] {
            self.neighbor_changed(p.offset(f), block, p);
        }
        // DROPPED: the observer pass (updateObservers); population places no observers.
    }

    /// `World.neighborChanged(pos, blockIn, fromPos)`
    pub fn neighbor_changed(&mut self, p: Pos, block: u16, from: Pos) {
        let s = self.get(p);
        behavior::neighbor_changed(self, p, s, block, from);
    }

    /// `World.scheduleUpdate(pos, block, delay)` through the harness `updateBlockTick`: only the
    /// immediate branch runs; the pending tick list is DROPPED.
    pub fn schedule_update(&mut self, p: Pos, block: u16) {
        if self.traced(p) {
            println!("sched {} {} {} {block} imm {}", p.x, p.y, p.z, self.immediate);
        }
        let def = block << 4;
        if self.immediate && state::mat(def) != material::AIR && state::requires_updates(def) {
            if self.is_area_loaded(p.add(-8, -8, -8), p.add(8, 8, 8)) {
                let s = self.get(p);
                if state::mat(s) != material::AIR && blocks::block_of(s) == block {
                    behavior::update_tick(self, p, s, TickRand::World);
                }
            }
        }
        // DROPPED: the pending tick list; nothing ticks after population in the harness.
    }

    /// `WorldServer.immediateBlockTick(pos, state, random)`
    pub fn immediate_block_tick(&mut self, p: Pos, s: u16, rand: &mut JavaRandom) {
        self.immediate = true;
        behavior::update_tick(self, p, s, TickRand::Gen(rand));
        self.immediate = false;
    }

    /// `World.getHeight(int, int)`
    pub fn get_height(&self, x: i32, z: i32) -> i32 {
        if x >= -30000000 && z >= -30000000 && x < 30000000 && z < 30000000 {
            match self.chunks.get(&(x >> 4, z >> 4)) {
                Some(c) => c.height_value(x & 15, z & 15),
                None => 0,
            }
        } else {
            64
        }
    }

    /// `World.getHeight(BlockPos)`
    pub fn height_pos(&self, p: Pos) -> Pos {
        Pos::new(p.x, self.get_height(p.x, p.z), p.z)
    }

    /// `World.getTopSolidOrLiquidBlock(BlockPos)`
    pub fn top_solid_or_liquid(&mut self, p: Pos) -> Pos {
        let c = self.chunk(p.x >> 4, p.z >> 4);
        let mut y = c.top_filled_segment() + 16;
        while y >= 0 {
            let s = c.get(p.x & 15, y - 1, p.z & 15);
            if state::blocks_movement(s) && state::mat(s) != material::LEAVES {
                break;
            }
            y -= 1;
        }
        Pos::new(p.x, y, p.z)
    }

    /// `World.getPrecipitationHeight(BlockPos)`
    pub fn precipitation_height(&mut self, p: Pos) -> Pos {
        let y = self.chunk(p.x >> 4, p.z >> 4).precipitation_height(p.x, p.z);
        Pos::new(p.x, y, p.z)
    }

    /// `World.getLight(BlockPos)`
    pub fn get_light(&mut self, p: Pos) -> i32 {
        if p.y < 0 {
            return 0;
        }
        let y = p.y.min(255);
        self.chunk(p.x >> 4, p.z >> 4).light_subtracted(p.x & 15, y, p.z & 15)
    }

    /// `World.getLightFor(EnumSkyBlock.SKY, pos)`
    pub fn get_sky_light(&mut self, p: Pos) -> i32 {
        let p = if p.y < 0 { Pos::new(p.x, 0, p.z) } else { p };
        if !(p.x >= -30000000 && p.z >= -30000000 && p.x < 30000000 && p.z < 30000000 && p.y < 256) {
            return 15;
        }
        let Some(c) = self.chunks.get(&(p.x >> 4, p.z >> 4)) else {
            return 15;
        };
        // Chunk.getLightFor(SKY, pos)
        match &c.sections[(p.y >> 4) as usize] {
            None => {
                if p.y >= c.height_value(p.x & 15, p.z & 15) {
                    15
                } else {
                    0
                }
            }
            Some(sec) => sec.sky[sidx(p.x, p.y, p.z)] as i32,
        }
    }

    /// `World.canSeeSky(BlockPos)`
    pub fn can_see_sky(&mut self, p: Pos) -> bool {
        let c = self.chunk(p.x >> 4, p.z >> 4);
        p.y >= c.height_value(p.x & 15, p.z & 15)
    }

    /// `World.getBiome(BlockPos)`
    pub fn biome(&self, p: Pos) -> &'static Biome {
        match self.chunks.get(&(p.x >> 4, p.z >> 4)) {
            Some(c) => biome::get_or_default(c.biomes[((p.z & 15) << 4 | (p.x & 15)) as usize] as i32),
            None => self.gen.provider.get_biomes(p.x, p.z, 1, 1)[0],
        }
    }

    /// `Biome.getTemperature(BlockPos)`
    pub fn temperature(&self, b: &Biome, p: Pos) -> f32 {
        if p.y > 64 {
            let f = (self
                .temperature_noise
                .get_value((p.x as f32 / 8.0) as f64, (p.z as f32 / 8.0) as f64)
                * 4.0) as f32;
            b.temperature - (f + p.y as f32 - 64.0) * 0.05 / 30.0
        } else {
            b.temperature
        }
    }

    fn is_water(&mut self, p: Pos) -> bool {
        state::mat(self.get(p)) == material::WATER
    }

    /// `World.canBlockFreeze(pos, noWaterAdj)`. Block light is always 0, so the light test passes.
    pub fn can_block_freeze(&mut self, p: Pos, no_water_adj: bool) -> bool {
        let b = self.biome(p);
        if self.temperature(b, p) >= 0.15 {
            return false;
        }
        if (0..256).contains(&p.y) {
            let s = self.get(p);
            let block = blocks::block_of(s);
            if (block == id::WATER || block == id::FLOWING_WATER) && s & 15 == 0 {
                if !no_water_adj {
                    return true;
                }
                let flag = self.is_water(p.west())
                    && self.is_water(p.east())
                    && self.is_water(p.north())
                    && self.is_water(p.south());
                if !flag {
                    return true;
                }
            }
        }
        false
    }

    /// `World.canSnowAt(pos, true)`
    pub fn can_snow_at(&mut self, p: Pos) -> bool {
        let b = self.biome(p);
        if self.temperature(b, p) >= 0.15 {
            return false;
        }
        if (0..256).contains(&p.y) {
            let s = self.get(p);
            if state::mat(s) == material::AIR && behavior::snow_can_place_at(self, p) {
                return true;
            }
        }
        false
    }
}
