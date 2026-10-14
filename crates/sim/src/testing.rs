//! Test fixtures shared by sim's own tests and, through the `testing` feature, the viewer's:
//! `Blocks` (a world of placed blocks, from the viewer's collision.rs tests), `chunk` and
//! `region_of` (from the viewer's region.rs tests).

use std::collections::HashMap;

use worldgen::blocks::{BEDROCK, STONE};
use worldgen::{Chunk, ChunkPrimer};

use crate::collision::BlockAccess;
use crate::region::Region;

/// A world of explicitly placed blocks, loaded everywhere.
#[derive(Default)]
pub struct Blocks(pub HashMap<(i32, i32, i32), u16>);

impl Blocks {
    pub fn floor_at(y: i32, r: i32) -> Blocks {
        let mut b = Blocks::default();
        for x in -r..=r {
            for z in -r..=r {
                b.0.insert((x, y, z), STONE);
            }
        }
        b
    }
    pub fn set(&mut self, x: i32, y: i32, z: i32, s: u16) {
        self.0.insert((x, y, z), s);
    }
}

impl BlockAccess for Blocks {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        *self.0.get(&(x, y, z)).unwrap_or(&0)
    }
    fn loaded(&self, _: i32, _: i32) -> bool {
        true
    }
}

/// A chunk filled with stone below `ground`, with `f` applied after.
pub fn chunk(x: i32, z: i32, ground: i32, f: impl Fn(&mut ChunkPrimer)) -> Chunk {
    let mut blocks = ChunkPrimer::new();
    for bx in 0..16 {
        for bz in 0..16 {
            blocks.set(bx, 0, bz, BEDROCK);
            for y in 1..ground {
                blocks.set(bx, y, bz, STONE);
            }
        }
    }
    f(&mut blocks);
    Chunk { x, z, blocks, biomes: [1; 256] }
}

pub fn region_of(center: Chunk, ground: i32) -> Region {
    let ring: Vec<Chunk> = (0..9).map(|i| chunk(center.x + i % 3 - 1, center.z + i / 3 - 1, ground, |_| ())).collect();
    let mut refs: Vec<&Chunk> = ring.iter().collect();
    refs[4] = &center;
    Region::new(refs.try_into().ok().unwrap())
}
