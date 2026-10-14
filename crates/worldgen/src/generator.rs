//! `net.minecraft.world.gen.ChunkGeneratorOverworld.generateChunk` for the default world type.

use crate::biome::Biome;
use crate::biome_provider::BiomeProvider;
use crate::java_random::JavaRandom;
use crate::primer::ChunkPrimer;
use crate::surface::SurfaceGen;
use crate::terrain::{Heightmap, TerrainNoise};
use crate::{caves, ravines};

/// A generated chunk: the primer `generateChunk` builds its `Chunk` from, and the chunk's biome
/// byte array.
#[derive(Clone)]
pub struct Chunk {
    pub x: i32,
    pub z: i32,
    pub blocks: ChunkPrimer,
    /// `Chunk.getBiomeArray()`: `(byte) Biome.getIdForBiome`, indexed `x + z * 16`.
    pub biomes: [u8; 256],
}

/// Every intermediate `generateChunk` produces, for the per-stage fixture tests.
pub struct Stages {
    pub heightmap: Heightmap,
    /// The 1:1 biome array `replaceBiomeBlocks` is given, indexed `x + z * 16`.
    pub biomes: Vec<&'static Biome>,
    pub depth_buffer: Vec<f64>,
    /// The primer after `setBlocksInChunk`.
    pub noise: ChunkPrimer,
    /// After `replaceBiomeBlocks`.
    pub surface: ChunkPrimer,
    /// After `MapGenCaves`.
    pub caves: ChunkPrimer,
    /// After `MapGenRavine`; the final blocks.
    pub ravines: ChunkPrimer,
}

/// `ChunkGeneratorOverworld` with its `BiomeProvider`, for one world seed. Shared across threads:
/// every per-chunk value lives on the stack of a `generate` call.
pub struct ChunkGenerator {
    seed: i64,
    pub provider: BiomeProvider,
    noise: TerrainNoise,
    surface: SurfaceGen,
}

impl ChunkGenerator {
    pub fn new(seed: i64) -> Self {
        ChunkGenerator {
            seed,
            provider: BiomeProvider::new(seed),
            noise: TerrainNoise::new(seed),
            surface: SurfaceGen::new(seed),
        }
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// `ChunkGeneratorOverworld.generateChunk(int, int)`
    pub fn generate(&self, x: i32, z: i32) -> Chunk {
        self.run(x, z, |_, _| {}).0
    }

    /// [`Self::generate`], keeping a copy of the primer after every stage.
    pub fn generate_staged(&self, x: i32, z: i32) -> Stages {
        let mut snaps: Vec<ChunkPrimer> = Vec::with_capacity(4);
        let (_, heightmap, biomes, depth_buffer) = self.run(x, z, |_, p| snaps.push(p.clone()));
        let mut s = snaps.into_iter();
        let mut next = || s.next().expect("four stage snapshots");
        Stages { heightmap, biomes, depth_buffer, noise: next(), surface: next(), caves: next(), ravines: next() }
    }

    /// The body of `generateChunk`. `snapshot` sees the primer after each of the four stages.
    fn run(
        &self,
        x: i32,
        z: i32,
        mut snapshot: impl FnMut(usize, &ChunkPrimer),
    ) -> (Chunk, Heightmap, Vec<&'static Biome>, Vec<f64>) {
        let mut rand = JavaRandom::new((x as i64).wrapping_mul(341873128712).wrapping_add((z as i64).wrapping_mul(132897987541)));
        let mut primer = ChunkPrimer::new();
        let heightmap = self.noise.set_blocks_in_chunk(&self.provider, x, z, &mut primer);
        snapshot(0, &primer);
        let biomes = self.provider.get_biomes(x * 16, z * 16, 16, 16);
        let surface = self.surface.replace_biome_blocks(&self.noise.surface, &mut rand, x, z, &mut primer, &biomes);
        snapshot(1, &primer);
        caves::generate(self.seed, x, z, &mut primer, &surface.column_top);
        snapshot(2, &primer);
        ravines::generate(self.seed, x, z, &mut primer, &surface.column_top);
        snapshot(3, &primer);
        // DROPPED: mineshafts, villages, strongholds, temples, monuments and woodland mansions.
        // In generateChunk they only record structure starts; their blocks go in at population.
        let mut biome_bytes = [0u8; 256];
        for (b, biome) in biome_bytes.iter_mut().zip(&biomes) {
            *b = biome.id;
        }
        // DROPPED: chunk.generateSkylightMap(); lighting is out of scope.
        let chunk = Chunk { x, z, blocks: primer, biomes: biome_bytes };
        (chunk, heightmap, biomes, surface.depth_buffer)
    }
}
