//! A populated chunk set aside while it is out of range, the way the server writes a chunk to its
//! region file and reads it back: block states, sky light, the height map and biomes are kept;
//! the precipitation height map is not saved by `AnvilChunkLoader` and starts over at -999.

use super::world::{PopChunk, Section};
use crate::blocks;
use crate::{Chunk, ChunkPrimer};

/// Run-length coded section arrays, `(run, value)` in `y << 8 | z << 4 | x` order.
struct PackedSection {
    y: u8,
    blocks: Vec<(u16, u16)>,
    sky: Vec<(u16, u8)>,
}

pub struct PackedChunk {
    x: i32,
    z: i32,
    sections: Vec<PackedSection>,
    height_map: Box<[i32; 256]>,
    height_map_minimum: i32,
    biomes: [u8; 256],
}

fn runs<T: Copy + PartialEq>(v: &[T]) -> Vec<(u16, T)> {
    let mut out: Vec<(u16, T)> = Vec::new();
    for &x in v {
        match out.last_mut() {
            Some((n, y)) if *y == x && *n < u16::MAX => *n += 1,
            _ => out.push((1, x)),
        }
    }
    out
}

fn unrun<T: Copy, const N: usize>(r: &[(u16, T)], out: &mut [T; N]) {
    let mut i = 0;
    for &(n, x) in r {
        out[i..i + n as usize].fill(x);
        i += n as usize;
    }
}

impl PackedChunk {
    pub fn pack(c: &PopChunk) -> PackedChunk {
        let sections = c
            .sections
            .iter()
            .enumerate()
            .filter_map(|(y, s)| s.as_ref().map(|s| PackedSection { y: y as u8, blocks: runs(&s.blocks), sky: runs(&s.sky) }))
            .collect();
        PackedChunk {
            x: c.x,
            z: c.z,
            sections,
            height_map: Box::new(c.height_map),
            height_map_minimum: c.height_map_minimum,
            biomes: c.biomes,
        }
    }

    pub fn unpack(&self) -> PopChunk {
        let mut c = PopChunk {
            x: self.x,
            z: self.z,
            sections: Default::default(),
            height_map: *self.height_map,
            height_map_minimum: self.height_map_minimum,
            precip: [-999; 256],
            biomes: self.biomes,
        };
        for s in &self.sections {
            let mut sec = Box::new(Section { blocks: [blocks::AIR; 4096], sky: [0; 4096] });
            unrun(&s.blocks, &mut sec.blocks);
            unrun(&s.sky, &mut sec.sky);
            c.sections[s.y as usize] = Some(sec);
        }
        c
    }
}

impl PopChunk {
    /// The block states and biomes as a generated [`Chunk`], for a renderer that lights its own.
    pub fn to_chunk(&self) -> Chunk {
        let mut blocks = ChunkPrimer::new();
        for (i, s) in self.sections.iter().enumerate() {
            let Some(s) = s else { continue };
            for (k, &b) in s.blocks.iter().enumerate() {
                if b != blocks::AIR {
                    let k = k as i32;
                    blocks.set(k & 15, (i as i32) << 4 | k >> 8, k >> 4 & 15, b);
                }
            }
        }
        Chunk { x: self.x, z: self.z, blocks, biomes: self.biomes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChunkGenerator;

    #[test]
    fn packing_keeps_blocks_light_and_heights() {
        let g = ChunkGenerator::new(1);
        let c = PopChunk::from_generated(g.generate(3, -2));
        let back = PackedChunk::pack(&c).unpack();
        assert_eq!((back.x, back.z), (3, -2));
        assert_eq!(back.height_map, c.height_map);
        assert_eq!(back.height_map_minimum, c.height_map_minimum);
        for (a, b) in c.sections.iter().zip(&back.sections) {
            match (a, b) {
                (Some(a), Some(b)) => assert!(a.blocks == b.blocks && a.sky == b.sky),
                (None, None) => {}
                _ => panic!("section presence differs"),
            }
        }
        let chunk = c.to_chunk();
        let raw = g.generate(3, -2);
        assert!(chunk.blocks.data == raw.blocks.data);
    }
}
