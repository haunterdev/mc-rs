//! A port of the Minecraft 1.12.2 overworld terrain generator (default world type): the biome
//! GenLayer stack, base noise terrain, `replaceBiomeBlocks`, `MapGenCaves` and `MapGenRavine`.
//!
//! Every ported function names its source class and method (MCP names). Output is meant to be
//! identical to vanilla, block for block; the per-stage fixture tests in `tests/` check that.
//! `docs/DECOMP_NOTES.md` lists the behaviour that is easy to get wrong.
//!
//! All generator state is read-only after construction, so one [`ChunkGenerator`] can be shared
//! across threads and each call to [`ChunkGenerator::generate`] is independent.

pub mod biome;
pub mod biome_provider;
pub mod blocks;
pub mod caves;
pub mod fdlibm;
pub mod generator;
pub mod genlayer;
pub mod java_random;
pub mod math_helper;
pub mod noise;
pub mod populate;
pub mod primer;
pub mod ravines;
pub mod surface;
pub mod terrain;

pub use generator::{Chunk, ChunkGenerator, Stages};
pub use java_random::JavaRandom;
pub use primer::ChunkPrimer;
