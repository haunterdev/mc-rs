//! `net.minecraft.world.chunk.ChunkPrimer`

use crate::blocks;

/// `ChunkPrimer`: one chunk's block states, indexed `x << 12 | z << 8 | y`.
#[derive(Clone)]
pub struct ChunkPrimer {
    pub data: Box<[u16]>,
}

impl Default for ChunkPrimer {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkPrimer {
    pub fn new() -> Self {
        ChunkPrimer { data: vec![blocks::AIR; 65536].into_boxed_slice() }
    }

    /// `ChunkPrimer.getBlockIndex(int, int, int)`
    #[inline(always)]
    pub fn index(x: i32, y: i32, z: i32) -> usize {
        (x << 12 | z << 8 | y) as usize
    }

    /// `ChunkPrimer.getBlockState(int, int, int)`. Every state id the generator writes is
    /// registered, so the null-state fallback to air never applies.
    #[inline(always)]
    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        self.data[Self::index(x, y, z)]
    }

    /// `ChunkPrimer.setBlockState(int, int, int, IBlockState)`
    #[inline(always)]
    pub fn set(&mut self, x: i32, y: i32, z: i32, state: u16) {
        self.data[Self::index(x, y, z)] = state;
    }
}
