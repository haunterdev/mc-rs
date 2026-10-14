//! `net.minecraft.world.biome.BiomeProvider` for the default world type.

use crate::biome::{self, Biome};
use crate::genlayer::BiomeLayers;

/// `BiomeProvider(long, WorldType.DEFAULT, "")`. `IntCache` and `BiomeCache` are dropped: neither
/// changes a result (the cache stores exactly what `biomeIndexLayer` returns).
pub struct BiomeProvider {
    pub layers: BiomeLayers,
}

impl BiomeProvider {
    pub fn new(seed: i64) -> Self {
        BiomeProvider { layers: BiomeLayers::new(seed) }
    }

    /// `BiomeProvider.getBiomesForGeneration`: 1:4 biomes from `genBiomes`.
    pub fn get_biomes_for_generation(&self, x: i32, z: i32, width: usize, height: usize) -> Vec<&'static Biome> {
        self.layers.gen_biomes.get_ints(x, z, width, height).into_iter().map(biome::get_or_default).collect()
    }

    /// `BiomeProvider.getBiomes(Biome[], int, int, int, int, boolean)`: 1:1 biomes from
    /// `biomeIndexLayer`, indexed `x + z * width`.
    pub fn get_biomes(&self, x: i32, z: i32, width: usize, length: usize) -> Vec<&'static Biome> {
        self.layers.biome_index.get_ints(x, z, width, length).into_iter().map(biome::get_or_default).collect()
    }
}
