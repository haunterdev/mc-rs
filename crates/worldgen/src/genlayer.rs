//! `net.minecraft.world.gen.layer`: the biome GenLayer stack.
//!
//! Each Java layer object becomes a [`GenLayer`] node. Vanilla keeps the chunk seed in a field and
//! mutates it from `getInts`; here it lives in a [`LayerRng`] local to each cell, so a built stack
//! is immutable and can be shared across threads. Every vanilla layer calls `initChunkSeed`
//! before its first `nextInt` for a cell, so the two are equivalent.
//!
//! `IntCache` is replaced by fresh `Vec`s. Vanilla reads only the cells it wrote, with one
//! exception noted on [`GenLayer::voronoi_zoom`].

use std::sync::Arc;

use crate::biome::{self, ids, TempCategory};

const MUL: i64 = 6364136223846793005;
const ADD: i64 = 1442695040888963407;

/// The LCG step every GenLayer seed goes through: `s *= s * 6364136223846793005L + 1442695040888963407L; s += add`.
#[inline(always)]
fn mix(s: i64, add: i64) -> i64 {
    s.wrapping_mul(s.wrapping_mul(MUL).wrapping_add(ADD)).wrapping_add(add)
}

/// `GenLayer(long)`: the base seed mixes the constructor seed in three times.
pub fn base_seed(seed: i64) -> i64 {
    let mut b = seed;
    b = mix(b, seed);
    b = mix(b, seed);
    mix(b, seed)
}

/// `GenLayer.initWorldGenSeed(long)` for one node: the world seed mixed with the base seed three times.
pub fn world_gen_seed(world_seed: i64, base_seed: i64) -> i64 {
    let mut w = world_seed;
    w = mix(w, base_seed);
    w = mix(w, base_seed);
    mix(w, base_seed)
}

/// The per-cell part of a GenLayer: `chunkSeed` and the methods that consume it.
pub struct LayerRng {
    chunk_seed: i64,
    world_gen_seed: i64,
}

impl LayerRng {
    /// `GenLayer.initChunkSeed(long, long)`
    #[inline(always)]
    pub fn new(world_gen_seed: i64, x: i64, z: i64) -> Self {
        let mut c = world_gen_seed;
        c = mix(c, x);
        c = mix(c, z);
        c = mix(c, x);
        c = mix(c, z);
        LayerRng { chunk_seed: c, world_gen_seed }
    }

    /// `GenLayer.nextInt(int)`: `(chunkSeed >> 24) % bound`, made non-negative. The seed advances
    /// on every call, `nextInt(1)` included.
    #[inline(always)]
    pub fn next_int(&mut self, bound: i32) -> i32 {
        let mut i = ((self.chunk_seed >> 24) % bound as i64) as i32;
        if i < 0 {
            i += bound;
        }
        self.chunk_seed = mix(self.chunk_seed, self.world_gen_seed);
        i
    }

    /// `GenLayer.selectRandom(int...)`
    #[inline(always)]
    fn select_random(&mut self, options: &[i32]) -> i32 {
        options[self.next_int(options.len() as i32) as usize]
    }

    /// `GenLayer.selectModeOrRandom(int, int, int, int)`
    #[inline(always)]
    fn select_mode_or_random(&mut self, a: i32, b: i32, c: i32, d: i32) -> i32 {
        if b == c && c == d {
            b
        } else if a == b && a == c {
            a
        } else if a == b && a == d {
            a
        } else if a == c && a == d {
            a
        } else if a == b && c != d {
            a
        } else if a == c && b != d {
            a
        } else if a == d && b != c {
            a
        } else if b == c && a != d {
            b
        } else if b == d && a != c {
            b
        } else if c == d && a != b {
            c
        } else {
            self.select_random(&[a, b, c, d])
        }
    }
}

/// `GenLayerEdge.Mode`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeMode {
    CoolWarm,
    HeatIce,
    Special,
}

/// One layer and the layers it reads.
pub enum Kind {
    Island,
    FuzzyZoom(Arc<GenLayer>),
    Zoom(Arc<GenLayer>),
    AddIsland(Arc<GenLayer>),
    RemoveTooMuchOcean(Arc<GenLayer>),
    AddSnow(Arc<GenLayer>),
    Edge(Arc<GenLayer>, EdgeMode),
    AddMushroomIsland(Arc<GenLayer>),
    DeepOcean(Arc<GenLayer>),
    RiverInit(Arc<GenLayer>),
    Biome(Arc<GenLayer>),
    BiomeEdge(Arc<GenLayer>),
    Hills { parent: Arc<GenLayer>, river: Arc<GenLayer> },
    River(Arc<GenLayer>),
    Smooth(Arc<GenLayer>),
    RareBiome(Arc<GenLayer>),
    Shore(Arc<GenLayer>),
    RiverMix { biome: Arc<GenLayer>, river: Arc<GenLayer> },
    VoronoiZoom(Arc<GenLayer>),
}

/// `GenLayer`: `baseSeed`, `worldGenSeed` and the layer's own logic.
pub struct GenLayer {
    pub base_seed: i64,
    pub world_gen_seed: i64,
    pub kind: Kind,
}

/// `GenLayer.biomesEqualOrMesaPlateau(int, int)`
fn biomes_equal_or_mesa_plateau(a: i32, b: i32) -> bool {
    if a == b {
        return true;
    }
    match (biome::get(a), biome::get(b)) {
        (Some(ba), Some(bb)) => {
            if a != ids::MESA_ROCK && a != ids::MESA_CLEAR_ROCK {
                ba.id == bb.id || ba.biome_class() == bb.biome_class()
            } else {
                b == ids::MESA_ROCK || b == ids::MESA_CLEAR_ROCK
            }
        }
        _ => false,
    }
}

use biome::is_oceanic;

impl GenLayer {
    /// Builds a node. `world_seed` is `Some` when `initWorldGenSeed` reaches this node and `None`
    /// when it does not, in which case `worldGenSeed` stays 0 as in vanilla.
    pub fn new(seed: i64, kind: Kind, world_seed: Option<i64>) -> Arc<GenLayer> {
        let base = base_seed(seed);
        let world_gen_seed = world_seed.map_or(0, |w| world_gen_seed(w, base));
        Arc::new(GenLayer { base_seed: base, world_gen_seed, kind })
    }

    #[inline(always)]
    fn rng(&self, x: i32, z: i32) -> LayerRng {
        LayerRng::new(self.world_gen_seed, x as i64, z as i64)
    }

    /// The Java class name, for matching against harness dumps.
    pub fn class_name(&self) -> &'static str {
        match &self.kind {
            Kind::Island => "GenLayerIsland",
            Kind::FuzzyZoom(_) => "GenLayerFuzzyZoom",
            Kind::Zoom(_) => "GenLayerZoom",
            Kind::AddIsland(_) => "GenLayerAddIsland",
            Kind::RemoveTooMuchOcean(_) => "GenLayerRemoveTooMuchOcean",
            Kind::AddSnow(_) => "GenLayerAddSnow",
            Kind::Edge(..) => "GenLayerEdge",
            Kind::AddMushroomIsland(_) => "GenLayerAddMushroomIsland",
            Kind::DeepOcean(_) => "GenLayerDeepOcean",
            Kind::RiverInit(_) => "GenLayerRiverInit",
            Kind::Biome(_) => "GenLayerBiome",
            Kind::BiomeEdge(_) => "GenLayerBiomeEdge",
            Kind::Hills { .. } => "GenLayerHills",
            Kind::River(_) => "GenLayerRiver",
            Kind::Smooth(_) => "GenLayerSmooth",
            Kind::RareBiome(_) => "GenLayerRareBiome",
            Kind::Shore(_) => "GenLayerShore",
            Kind::RiverMix { .. } => "GenLayerRiverMix",
            Kind::VoronoiZoom(_) => "GenLayerVoronoiZoom",
        }
    }

    /// The layers this one reads, in the order the harness walks them: `parent`, then the
    /// RiverMix chains, then the Hills river layer.
    pub fn children(&self) -> Vec<&Arc<GenLayer>> {
        match &self.kind {
            Kind::Island => vec![],
            Kind::FuzzyZoom(p)
            | Kind::Zoom(p)
            | Kind::AddIsland(p)
            | Kind::RemoveTooMuchOcean(p)
            | Kind::AddSnow(p)
            | Kind::Edge(p, _)
            | Kind::AddMushroomIsland(p)
            | Kind::DeepOcean(p)
            | Kind::RiverInit(p)
            | Kind::Biome(p)
            | Kind::BiomeEdge(p)
            | Kind::River(p)
            | Kind::Smooth(p)
            | Kind::RareBiome(p)
            | Kind::Shore(p)
            | Kind::VoronoiZoom(p) => vec![p],
            Kind::Hills { parent, river } => vec![parent, river],
            Kind::RiverMix { biome, river } => vec![biome, river],
        }
    }

    /// `GenLayer.getInts(int, int, int, int)`: a `width * height` row-major grid (x inner).
    pub fn get_ints(&self, x: i32, z: i32, width: usize, height: usize) -> Vec<i32> {
        match &self.kind {
            Kind::Island => self.island(x, z, width, height),
            Kind::FuzzyZoom(p) => self.zoom(p, true, x, z, width, height),
            Kind::Zoom(p) => self.zoom(p, false, x, z, width, height),
            Kind::AddIsland(p) => self.add_island(p, x, z, width, height),
            Kind::RemoveTooMuchOcean(p) => self.remove_too_much_ocean(p, x, z, width, height),
            Kind::AddSnow(p) => self.add_snow(p, x, z, width, height),
            Kind::Edge(p, EdgeMode::CoolWarm) => self.edge_cool_warm(p, x, z, width, height),
            Kind::Edge(p, EdgeMode::HeatIce) => Self::edge_heat_ice(p, x, z, width, height),
            Kind::Edge(p, EdgeMode::Special) => self.edge_special(p, x, z, width, height),
            Kind::AddMushroomIsland(p) => self.add_mushroom_island(p, x, z, width, height),
            Kind::DeepOcean(p) => Self::deep_ocean(p, x, z, width, height),
            Kind::RiverInit(p) => self.river_init(p, x, z, width, height),
            Kind::Biome(p) => self.biome(p, x, z, width, height),
            Kind::BiomeEdge(p) => self.biome_edge(p, x, z, width, height),
            Kind::Hills { parent, river } => self.hills(parent, river, x, z, width, height),
            Kind::River(p) => Self::river(p, x, z, width, height),
            Kind::Smooth(p) => self.smooth(p, x, z, width, height),
            Kind::RareBiome(p) => self.rare_biome(p, x, z, width, height),
            Kind::Shore(p) => self.shore(p, x, z, width, height),
            Kind::RiverMix { biome, river } => Self::river_mix(biome, river, x, z, width, height),
            Kind::VoronoiZoom(p) => self.voronoi_zoom(p, x, z, width, height),
        }
    }

    /// `GenLayerIsland.getInts`: 1 in 10 land, and land forced at the origin.
    fn island(&self, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let mut r = self.rng(area_x + j as i32, area_y + i as i32);
                out[j + i * w] = if r.next_int(10) == 0 { 1 } else { 0 };
            }
        }
        let (wi, hi) = (w as i32, h as i32);
        if area_x > -wi && area_x <= 0 && area_y > -hi && area_y <= 0 {
            out[(-area_x + -area_y * wi) as usize] = 1;
        }
        out
    }

    /// `GenLayerZoom.getInts`, and `GenLayerFuzzyZoom` when `fuzzy` (its `selectModeOrRandom`
    /// override always picks at random).
    fn zoom(&self, parent: &GenLayer, fuzzy: bool, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let i = area_x >> 1;
        let j = area_y >> 1;
        let k = (w >> 1) + 2;
        let l = (h >> 1) + 2;
        let aint = parent.get_ints(i, j, k, l);
        let i1 = (k - 1) << 1;
        let j1 = (l - 1) << 1;
        let mut aint1 = vec![0; i1 * j1];
        for k1 in 0..l - 1 {
            let mut l1 = (k1 << 1) * i1;
            let mut i2 = 0;
            let mut j2 = aint[i2 + k1 * k];
            let mut k2 = aint[i2 + (k1 + 1) * k];
            while i2 < k - 1 {
                let mut r = self.rng((i2 as i32 + i) << 1, (k1 as i32 + j) << 1);
                let l2 = aint[i2 + 1 + k1 * k];
                let i3 = aint[i2 + 1 + (k1 + 1) * k];
                aint1[l1] = j2;
                aint1[l1 + i1] = r.select_random(&[j2, k2]);
                l1 += 1;
                aint1[l1] = r.select_random(&[j2, l2]);
                aint1[l1 + i1] =
                    if fuzzy { r.select_random(&[j2, l2, k2, i3]) } else { r.select_mode_or_random(j2, l2, k2, i3) };
                l1 += 1;
                j2 = l2;
                k2 = i3;
                i2 += 1;
            }
        }
        let mut out = vec![0; w * h];
        let ox = (area_x & 1) as usize;
        let oy = (area_y & 1) as usize;
        for j3 in 0..h {
            let src = (j3 + oy) * i1 + ox;
            out[j3 * w..j3 * w + w].copy_from_slice(&aint1[src..src + w]);
        }
        out
    }

    /// `GenLayerAddIsland.getInts`
    fn add_island(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let k1 = aint[j1 + i1 * k];
                let l1 = aint[j1 + 2 + i1 * k];
                let i2 = aint[j1 + (i1 + 2) * k];
                let j2 = aint[j1 + 2 + (i1 + 2) * k];
                let k2 = aint[j1 + 1 + (i1 + 1) * k];
                let mut r = self.rng(j1 as i32 + area_x, i1 as i32 + area_y);
                let idx = j1 + i1 * w;
                if k2 != 0 || (k1 == 0 && l1 == 0 && i2 == 0 && j2 == 0) {
                    if k2 > 0 && (k1 == 0 || l1 == 0 || i2 == 0 || j2 == 0) {
                        if r.next_int(5) == 0 {
                            out[idx] = if k2 == 4 { 4 } else { 0 };
                        } else {
                            out[idx] = k2;
                        }
                    } else {
                        out[idx] = k2;
                    }
                } else {
                    // `nextInt(l2++)` only runs (and only increments) when the neighbour is land.
                    let mut l2 = 1;
                    let mut i3 = 1;
                    for n in [k1, l1, i2, j2] {
                        if n != 0 {
                            let v = r.next_int(l2);
                            l2 += 1;
                            if v == 0 {
                                i3 = n;
                            }
                        }
                    }
                    if r.next_int(3) == 0 {
                        out[idx] = i3;
                    } else if i3 == 4 {
                        out[idx] = 4;
                    } else {
                        out[idx] = 0;
                    }
                }
            }
        }
        out
    }

    /// `GenLayerRemoveTooMuchOcean.getInts`
    fn remove_too_much_ocean(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let k1 = aint[j1 + 1 + i1 * k];
                let l1 = aint[j1 + 2 + (i1 + 1) * k];
                let i2 = aint[j1 + (i1 + 1) * k];
                let j2 = aint[j1 + 1 + (i1 + 2) * k];
                let k2 = aint[j1 + 1 + (i1 + 1) * k];
                out[j1 + i1 * w] = k2;
                let mut r = self.rng(j1 as i32 + area_x, i1 as i32 + area_y);
                if k2 == 0 && k1 == 0 && l1 == 0 && i2 == 0 && j2 == 0 && r.next_int(2) == 0 {
                    out[j1 + i1 * w] = 1;
                }
            }
        }
        out
    }

    /// `GenLayerAddSnow.getInts`
    fn add_snow(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let k1 = aint[j1 + 1 + (i1 + 1) * k];
                let mut r = self.rng(j1 as i32 + area_x, i1 as i32 + area_y);
                if k1 == 0 {
                    out[j1 + i1 * w] = 0;
                } else {
                    let mut l1 = r.next_int(6);
                    if l1 == 0 {
                        l1 = 4;
                    } else if l1 <= 1 {
                        l1 = 3;
                    } else {
                        l1 = 1;
                    }
                    out[j1 + i1 * w] = l1;
                }
            }
        }
        out
    }

    /// `GenLayerEdge.getIntsCoolWarm`. Vanilla seeds the cell RNG here but never draws from it.
    fn edge_cool_warm(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = 1 + w + 1;
        let aint = parent.get_ints(area_x - 1, area_y - 1, k, 1 + h + 1);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let mut k1 = aint[j1 + 1 + (i1 + 1) * k];
                if k1 == 1 {
                    let l1 = aint[j1 + 1 + i1 * k];
                    let i2 = aint[j1 + 2 + (i1 + 1) * k];
                    let j2 = aint[j1 + (i1 + 1) * k];
                    let k2 = aint[j1 + 1 + (i1 + 2) * k];
                    let flag = l1 == 3 || i2 == 3 || j2 == 3 || k2 == 3;
                    let flag1 = l1 == 4 || i2 == 4 || j2 == 4 || k2 == 4;
                    if flag || flag1 {
                        k1 = 2;
                    }
                }
                out[j1 + i1 * w] = k1;
            }
        }
        out
    }

    /// `GenLayerEdge.getIntsHeatIce`: no randomness.
    fn edge_heat_ice(parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = 1 + w + 1;
        let aint = parent.get_ints(area_x - 1, area_y - 1, k, 1 + h + 1);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let mut k1 = aint[j1 + 1 + (i1 + 1) * k];
                if k1 == 4 {
                    let l1 = aint[j1 + 1 + i1 * k];
                    let i2 = aint[j1 + 2 + (i1 + 1) * k];
                    let j2 = aint[j1 + (i1 + 1) * k];
                    let k2 = aint[j1 + 1 + (i1 + 2) * k];
                    let flag = l1 == 2 || i2 == 2 || j2 == 2 || k2 == 2;
                    let flag1 = l1 == 1 || i2 == 1 || j2 == 1 || k2 == 1;
                    if flag1 || flag {
                        k1 = 3;
                    }
                }
                out[j1 + i1 * w] = k1;
            }
        }
        out
    }

    /// `GenLayerEdge.getIntsSpecial`: tags 1 in 13 land cells with a 1..15 "special" value in bits 8-11.
    fn edge_special(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let aint = parent.get_ints(area_x, area_y, w, h);
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let mut r = self.rng(j as i32 + area_x, i as i32 + area_y);
                let mut k = aint[j + i * w];
                if k != 0 && r.next_int(13) == 0 {
                    k |= ((1 + r.next_int(15)) << 8) & 3840;
                }
                out[j + i * w] = k;
            }
        }
        out
    }

    /// `GenLayerAddMushroomIsland.getInts`
    fn add_mushroom_island(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let k1 = aint[j1 + i1 * k];
                let l1 = aint[j1 + 2 + i1 * k];
                let i2 = aint[j1 + (i1 + 2) * k];
                let j2 = aint[j1 + 2 + (i1 + 2) * k];
                let k2 = aint[j1 + 1 + (i1 + 1) * k];
                let mut r = self.rng(j1 as i32 + area_x, i1 as i32 + area_y);
                out[j1 + i1 * w] = if k2 == 0 && k1 == 0 && l1 == 0 && i2 == 0 && j2 == 0 && r.next_int(100) == 0 {
                    ids::MUSHROOM_ISLAND
                } else {
                    k2
                };
            }
        }
        out
    }

    /// `GenLayerDeepOcean.getInts`: no randomness.
    fn deep_ocean(parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let k1 = aint[j1 + 1 + i1 * k];
                let l1 = aint[j1 + 2 + (i1 + 1) * k];
                let i2 = aint[j1 + (i1 + 1) * k];
                let j2 = aint[j1 + 1 + (i1 + 2) * k];
                let k2 = aint[j1 + 1 + (i1 + 1) * k];
                let l2 = [k1, l1, i2, j2].iter().filter(|&&n| n == 0).count();
                out[j1 + i1 * w] = if k2 == 0 && l2 > 3 { ids::DEEP_OCEAN } else { k2 };
            }
        }
        out
    }

    /// `GenLayerRiverInit.getInts`
    fn river_init(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let aint = parent.get_ints(area_x, area_y, w, h);
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let mut r = self.rng(j as i32 + area_x, i as i32 + area_y);
                out[j + i * w] = if aint[j + i * w] > 0 { r.next_int(299999) + 2 } else { 0 };
            }
        }
        out
    }

    /// `GenLayerBiome.getInts` for the default world type (null settings, so no fixed biome).
    fn biome(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        const WARM: [i32; 6] = [ids::DESERT, ids::DESERT, ids::DESERT, ids::SAVANNA, ids::SAVANNA, ids::PLAINS];
        const MEDIUM: [i32; 6] =
            [ids::FOREST, ids::ROOFED_FOREST, ids::EXTREME_HILLS, ids::PLAINS, ids::BIRCH_FOREST, ids::SWAMPLAND];
        const COLD: [i32; 4] = [ids::FOREST, ids::EXTREME_HILLS, ids::TAIGA, ids::PLAINS];
        const ICE: [i32; 4] = [ids::ICE_PLAINS, ids::ICE_PLAINS, ids::ICE_PLAINS, ids::COLD_TAIGA];
        let aint = parent.get_ints(area_x, area_y, w, h);
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let mut r = self.rng(j as i32 + area_x, i as i32 + area_y);
                let mut k = aint[j + i * w];
                let l = (k & 3840) >> 8;
                k &= -3841;
                out[j + i * w] = if is_oceanic(k) || k == ids::MUSHROOM_ISLAND {
                    k
                } else if k == 1 {
                    if l > 0 {
                        if r.next_int(3) == 0 {
                            ids::MESA_CLEAR_ROCK
                        } else {
                            ids::MESA_ROCK
                        }
                    } else {
                        WARM[r.next_int(WARM.len() as i32) as usize]
                    }
                } else if k == 2 {
                    if l > 0 {
                        ids::JUNGLE
                    } else {
                        MEDIUM[r.next_int(MEDIUM.len() as i32) as usize]
                    }
                } else if k == 3 {
                    if l > 0 {
                        ids::REDWOOD_TAIGA
                    } else {
                        COLD[r.next_int(COLD.len() as i32) as usize]
                    }
                } else if k == 4 {
                    ICE[r.next_int(ICE.len() as i32) as usize]
                } else {
                    ids::MUSHROOM_ISLAND
                };
            }
        }
        out
    }

    /// `GenLayerBiomeEdge.getInts`. Vanilla seeds the cell RNG but never draws from it.
    fn biome_edge(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let c = aint[j + 1 + (i + 1) * k];
                let n = aint[j + 1 + i * k];
                let e = aint[j + 2 + (i + 1) * k];
                let wv = aint[j + (i + 1) * k];
                let s = aint[j + 1 + (i + 2) * k];
                let idx = j + i * w;
                if let Some(v) = replace_biome_edge_if_necessary(c, [n, e, wv, s], ids::EXTREME_HILLS, ids::EXTREME_HILLS_EDGE)
                    .or_else(|| replace_biome_edge(c, [n, e, wv, s], ids::MESA_ROCK, ids::MESA))
                    .or_else(|| replace_biome_edge(c, [n, e, wv, s], ids::MESA_CLEAR_ROCK, ids::MESA))
                    .or_else(|| replace_biome_edge(c, [n, e, wv, s], ids::REDWOOD_TAIGA, ids::TAIGA))
                {
                    out[idx] = v;
                } else if c == ids::DESERT {
                    out[idx] = if n != ids::ICE_PLAINS && e != ids::ICE_PLAINS && wv != ids::ICE_PLAINS && s != ids::ICE_PLAINS {
                        c
                    } else {
                        ids::EXTREME_HILLS_WITH_TREES
                    };
                } else if c == ids::SWAMPLAND {
                    let any = |id: i32| n == id || e == id || wv == id || s == id;
                    out[idx] = if !any(ids::DESERT) && !any(ids::COLD_TAIGA) && !any(ids::ICE_PLAINS) {
                        if !any(ids::JUNGLE) {
                            c
                        } else {
                            ids::JUNGLE_EDGE
                        }
                    } else {
                        ids::PLAINS
                    };
                } else {
                    out[idx] = c;
                }
            }
        }
        out
    }

    /// `GenLayerHills.getInts`: hills, mutations (river noise `(l - 2) % 29 == 1`) and the hill
    /// variant chosen 1 in 3, or always where the river noise hits `(l - 2) % 29 == 0`.
    fn hills(&self, parent: &GenLayer, river: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k_w = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let aint1 = river.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let mut r = self.rng(j as i32 + area_x, i as i32 + area_y);
                let k = aint[j + 1 + (i + 1) * k_w];
                let l = aint1[j + 1 + (i + 1) * k_w];
                let flag = (l - 2) % 29 == 0;
                let biome = biome::get(k);
                let flag1 = biome.is_some_and(|b| b.is_mutation());
                let idx = j + i * w;
                if k != 0 && l >= 2 && (l - 2) % 29 == 1 && !flag1 {
                    out[idx] = biome::mutation_for(k).unwrap_or(k);
                } else if r.next_int(3) != 0 && !flag {
                    out[idx] = k;
                } else {
                    let biome1 = if k == ids::DESERT {
                        ids::DESERT_HILLS
                    } else if k == ids::FOREST {
                        ids::FOREST_HILLS
                    } else if k == ids::BIRCH_FOREST {
                        ids::BIRCH_FOREST_HILLS
                    } else if k == ids::ROOFED_FOREST {
                        ids::PLAINS
                    } else if k == ids::TAIGA {
                        ids::TAIGA_HILLS
                    } else if k == ids::REDWOOD_TAIGA {
                        ids::REDWOOD_TAIGA_HILLS
                    } else if k == ids::COLD_TAIGA {
                        ids::COLD_TAIGA_HILLS
                    } else if k == ids::PLAINS {
                        if r.next_int(3) == 0 {
                            ids::FOREST_HILLS
                        } else {
                            ids::FOREST
                        }
                    } else if k == ids::ICE_PLAINS {
                        ids::ICE_MOUNTAINS
                    } else if k == ids::JUNGLE {
                        ids::JUNGLE_HILLS
                    } else if k == ids::OCEAN {
                        ids::DEEP_OCEAN
                    } else if k == ids::EXTREME_HILLS {
                        ids::EXTREME_HILLS_WITH_TREES
                    } else if k == ids::SAVANNA {
                        ids::SAVANNA_PLATEAU
                    } else if biomes_equal_or_mesa_plateau(k, ids::MESA_ROCK) {
                        ids::MESA
                    } else if k == ids::DEEP_OCEAN && r.next_int(3) == 0 {
                        if r.next_int(2) == 0 {
                            ids::PLAINS
                        } else {
                            ids::FOREST
                        }
                    } else {
                        k
                    };
                    let mut j2 = biome1;
                    if flag && j2 != k {
                        j2 = biome::mutation_for(biome1).unwrap_or(k);
                    }
                    if j2 == k {
                        out[idx] = k;
                    } else {
                        let k2 = aint[j + 1 + i * k_w];
                        let j1 = aint[j + 2 + (i + 1) * k_w];
                        let k1 = aint[j + (i + 1) * k_w];
                        let l1 = aint[j + 1 + (i + 2) * k_w];
                        let i2 = [k2, j1, k1, l1].iter().filter(|&&n| biomes_equal_or_mesa_plateau(n, k)).count();
                        out[idx] = if i2 >= 3 { j2 } else { k };
                    }
                }
            }
        }
        out
    }

    /// `GenLayerRiver.getInts`: river (7) wherever the filtered river noise changes, -1 elsewhere.
    fn river(parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        fn river_filter(v: i32) -> i32 {
            if v >= 2 {
                2 + (v & 1)
            } else {
                v
            }
        }
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let k1 = river_filter(aint[j1 + (i1 + 1) * k]);
                let l1 = river_filter(aint[j1 + 2 + (i1 + 1) * k]);
                let i2 = river_filter(aint[j1 + 1 + i1 * k]);
                let j2 = river_filter(aint[j1 + 1 + (i1 + 2) * k]);
                let k2 = river_filter(aint[j1 + 1 + (i1 + 1) * k]);
                out[j1 + i1 * w] =
                    if k2 == k1 && k2 == i2 && k2 == l1 && k2 == j2 { -1 } else { ids::RIVER };
            }
        }
        out
    }

    /// `GenLayerSmooth.getInts`. The cell RNG is seeded only on the branch that draws from it.
    fn smooth(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let k = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i1 in 0..h {
            for j1 in 0..w {
                let k1 = aint[j1 + (i1 + 1) * k];
                let l1 = aint[j1 + 2 + (i1 + 1) * k];
                let i2 = aint[j1 + 1 + i1 * k];
                let j2 = aint[j1 + 1 + (i1 + 2) * k];
                let mut k2 = aint[j1 + 1 + (i1 + 1) * k];
                if k1 == l1 && i2 == j2 {
                    let mut r = self.rng(j1 as i32 + area_x, i1 as i32 + area_y);
                    k2 = if r.next_int(2) == 0 { k1 } else { i2 };
                } else {
                    if k1 == l1 {
                        k2 = k1;
                    }
                    if i2 == j2 {
                        k2 = i2;
                    }
                }
                out[j1 + i1 * w] = k2;
            }
        }
        out
    }

    /// `GenLayerRareBiome.getInts`: 1 in 57 plains become sunflower plains.
    fn rare_biome(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let mut r = self.rng(j as i32 + area_x, i as i32 + area_y);
                let k = aint[j + 1 + (i + 1) * (w + 2)];
                out[j + i * w] = if r.next_int(57) == 0 && k == ids::PLAINS { ids::MUTATED_PLAINS } else { k };
            }
        }
        out
    }

    /// `GenLayerShore.getInts`. Vanilla seeds the cell RNG but never draws from it.
    fn shore(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let kw = w + 2;
        let aint = parent.get_ints(area_x - 1, area_y - 1, w + 2, h + 2);
        let mut out = vec![0; w * h];
        for i in 0..h {
            for j in 0..w {
                let k = aint[j + 1 + (i + 1) * kw];
                let n = aint[j + 1 + i * kw];
                let e = aint[j + 2 + (i + 1) * kw];
                let wv = aint[j + (i + 1) * kw];
                let s = aint[j + 1 + (i + 2) * kw];
                let neigh = [n, e, wv, s];
                let biome = biome::get(k);
                let idx = j + i * w;
                out[idx] = if k == ids::MUSHROOM_ISLAND {
                    if n != ids::OCEAN && e != ids::OCEAN && wv != ids::OCEAN && s != ids::OCEAN {
                        k
                    } else {
                        ids::MUSHROOM_ISLAND_SHORE
                    }
                } else if biome.is_some_and(|b| b.biome_class() == biome::BiomeClass::Jungle) {
                    if neigh.iter().all(|&v| is_jungle_compatible(v)) {
                        if !neigh.iter().any(|&v| is_oceanic(v)) {
                            k
                        } else {
                            ids::BEACH
                        }
                    } else {
                        ids::JUNGLE_EDGE
                    }
                } else if k != ids::EXTREME_HILLS && k != ids::EXTREME_HILLS_WITH_TREES && k != ids::EXTREME_HILLS_EDGE {
                    if biome.is_some_and(|b| b.snowy) {
                        replace_if_neighbor_ocean(k, neigh, ids::COLD_BEACH)
                    } else if k != ids::MESA && k != ids::MESA_ROCK {
                        if k != ids::OCEAN && k != ids::DEEP_OCEAN && k != ids::RIVER && k != ids::SWAMPLAND {
                            if !neigh.iter().any(|&v| is_oceanic(v)) {
                                k
                            } else {
                                ids::BEACH
                            }
                        } else {
                            k
                        }
                    } else if !neigh.iter().any(|&v| is_oceanic(v)) {
                        if neigh.iter().all(|&v| is_mesa(v)) {
                            k
                        } else {
                            ids::DESERT
                        }
                    } else {
                        k
                    }
                } else {
                    replace_if_neighbor_ocean(k, neigh, ids::STONE_BEACH)
                };
            }
        }
        out
    }

    /// `GenLayerRiverMix.getInts`: rivers cut into land, frozen over ice plains, mushroom shore on
    /// mushroom islands.
    fn river_mix(biome: &GenLayer, river: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let aint = biome.get_ints(area_x, area_y, w, h);
        let aint1 = river.get_ints(area_x, area_y, w, h);
        let mut out = vec![0; w * h];
        for i in 0..w * h {
            out[i] = if aint[i] != ids::OCEAN && aint[i] != ids::DEEP_OCEAN {
                if aint1[i] == ids::RIVER {
                    if aint[i] == ids::ICE_PLAINS {
                        ids::FROZEN_RIVER
                    } else if aint[i] != ids::MUSHROOM_ISLAND && aint[i] != ids::MUSHROOM_ISLAND_SHORE {
                        aint1[i] & 255
                    } else {
                        ids::MUSHROOM_ISLAND_SHORE
                    }
                } else {
                    aint[i]
                }
            } else {
                aint[i]
            };
        }
        out
    }

    /// `GenLayerVoronoiZoom.getInts`: 4x zoom by jittered nearest cell.
    ///
    /// Vanilla's scratch grid is `((width >> 2) + 1) * 4` wide and the final copy starts at
    /// `(areaX - 2) & 3`, so a request where `((areaX - 2) & 3) + width % 4 > 4` (same for z) reads
    /// past the rows it filled and returns whatever `IntCache` left there. Vanilla only calls it
    /// with chunk-aligned 16x16 areas; such requests panic here instead.
    fn voronoi_zoom(&self, parent: &GenLayer, area_x: i32, area_y: i32, w: usize, h: usize) -> Vec<i32> {
        let area_x = area_x - 2;
        let area_y = area_y - 2;
        let i = area_x >> 2;
        let j = area_y >> 2;
        let k = (w >> 2) + 2;
        let l = (h >> 2) + 2;
        let aint = parent.get_ints(i, j, k, l);
        let i1 = (k - 1) << 2;
        let j1 = (l - 1) << 2;
        let ox = (area_x & 3) as usize;
        let oy = (area_y & 3) as usize;
        assert!(
            ox + w <= i1 && oy + h <= j1,
            "GenLayerVoronoiZoom request ({}, {}, {w}, {h}) reads outside its scratch grid",
            area_x + 2,
            area_y + 2
        );
        let mut aint1 = vec![0; i1 * j1];
        let jitter = |r: &mut LayerRng| (r.next_int(1024) as f64 / 1024.0 - 0.5) * 3.6;
        for k1 in 0..l - 1 {
            let mut l1 = 0;
            let mut i2 = aint[k1 * k];
            let mut j2 = aint[(k1 + 1) * k];
            while l1 < k - 1 {
                let cx = l1 as i32 + i;
                let cz = k1 as i32 + j;
                let mut r = self.rng(cx << 2, cz << 2);
                let d1 = jitter(&mut r);
                let d2 = jitter(&mut r);
                let mut r = self.rng((cx + 1) << 2, cz << 2);
                let d3 = jitter(&mut r) + 4.0;
                let d4 = jitter(&mut r);
                let mut r = self.rng(cx << 2, (cz + 1) << 2);
                let d5 = jitter(&mut r);
                let d6 = jitter(&mut r) + 4.0;
                let mut r = self.rng((cx + 1) << 2, (cz + 1) << 2);
                let d7 = jitter(&mut r) + 4.0;
                let d8 = jitter(&mut r) + 4.0;
                let k2 = aint[l1 + 1 + k1 * k] & 255;
                let l2 = aint[l1 + 1 + (k1 + 1) * k] & 255;
                for i3 in 0..4usize {
                    let mut j3 = ((k1 << 2) + i3) * i1 + (l1 << 2);
                    let fi = i3 as f64;
                    for k3 in 0..4usize {
                        let fk = k3 as f64;
                        let d9 = (fi - d2) * (fi - d2) + (fk - d1) * (fk - d1);
                        let d10 = (fi - d4) * (fi - d4) + (fk - d3) * (fk - d3);
                        let d11 = (fi - d6) * (fi - d6) + (fk - d5) * (fk - d5);
                        let d12 = (fi - d8) * (fi - d8) + (fk - d7) * (fk - d7);
                        aint1[j3] = if d9 < d10 && d9 < d11 && d9 < d12 {
                            i2
                        } else if d10 < d9 && d10 < d11 && d10 < d12 {
                            k2
                        } else if d11 < d9 && d11 < d10 && d11 < d12 {
                            j2
                        } else {
                            l2
                        };
                        j3 += 1;
                    }
                }
                i2 = k2;
                j2 = l2;
                l1 += 1;
            }
        }
        let mut out = vec![0; w * h];
        for l3 in 0..h {
            let src = (l3 + oy) * i1 + ox;
            out[l3 * w..l3 * w + w].copy_from_slice(&aint1[src..src + w]);
        }
        out
    }
}

/// `GenLayerBiomeEdge.replaceBiomeEdgeIfNecessary`: `Some(result)` when it handled the cell.
fn replace_biome_edge_if_necessary(k: i32, neigh: [i32; 4], from: i32, to: i32) -> Option<i32> {
    if !biomes_equal_or_mesa_plateau(k, from) {
        return None;
    }
    Some(if neigh.iter().all(|&n| can_biomes_be_neighbors(n, from)) { k } else { to })
}

/// `GenLayerBiomeEdge.replaceBiomeEdge`: `Some(result)` when it handled the cell.
fn replace_biome_edge(k: i32, neigh: [i32; 4], from: i32, to: i32) -> Option<i32> {
    if k != from {
        return None;
    }
    Some(if neigh.iter().all(|&n| biomes_equal_or_mesa_plateau(n, from)) { k } else { to })
}

/// `GenLayerBiomeEdge.canBiomesBeNeighbors`
fn can_biomes_be_neighbors(a: i32, b: i32) -> bool {
    if biomes_equal_or_mesa_plateau(a, b) {
        return true;
    }
    match (biome::get(a), biome::get(b)) {
        (Some(ba), Some(bb)) => {
            let (ta, tb) = (ba.temp_category(), bb.temp_category());
            ta == tb || ta == TempCategory::Medium || tb == TempCategory::Medium
        }
        _ => false,
    }
}

/// `GenLayerShore.replaceIfNeighborOcean`
fn replace_if_neighbor_ocean(k: i32, neigh: [i32; 4], replacement: i32) -> i32 {
    if is_oceanic(k) || !neigh.iter().any(|&n| is_oceanic(n)) {
        k
    } else {
        replacement
    }
}

/// `GenLayerShore.isJungleCompatible`
fn is_jungle_compatible(id: i32) -> bool {
    if biome::get(id).is_some_and(|b| b.biome_class() == biome::BiomeClass::Jungle) {
        return true;
    }
    id == ids::JUNGLE_EDGE || id == ids::JUNGLE || id == ids::JUNGLE_HILLS || id == ids::FOREST || id == ids::TAIGA || is_oceanic(id)
}

/// `GenLayerShore.isMesa`: `instanceof BiomeMesa`.
fn is_mesa(id: i32) -> bool {
    biome::get(id).is_some_and(|b| b.biome_class() == biome::BiomeClass::Mesa)
}

/// `GenLayerZoom.magnify(long, GenLayer, int)`
fn magnify(seed: i64, parent: Arc<GenLayer>, count: i32, world_seed: Option<i64>) -> Arc<GenLayer> {
    let mut layer = parent;
    for i in 0..count {
        layer = GenLayer::new(seed + i as i64, Kind::Zoom(layer), world_seed);
    }
    layer
}

/// The two roots `BiomeProvider` keeps from `GenLayer.initializeAllBiomeGenerators`.
pub struct BiomeLayers {
    /// `genBiomes` (the RiverMix layer): 1:4 biomes for terrain shaping.
    pub gen_biomes: Arc<GenLayer>,
    /// `biomeIndexLayer` (VoronoiZoom): 1:1 biomes.
    pub biome_index: Arc<GenLayer>,
}

impl BiomeLayers {
    /// `GenLayer.initializeAllBiomeGenerators(seed, WorldType.DEFAULT, null)`. Biome and river size
    /// are both 4, since `BiomeProvider` passes null settings for the default world type.
    ///
    /// `initWorldGenSeed` runs from the RiverMix and Voronoi roots and recurses only through
    /// `parent` (RiverMix overrides it to cover both of its chains). The two zooms in
    /// `GenLayerHills.riverLayer` hang off a field it never follows, so their `worldGenSeed` stays 0.
    pub fn new(seed: i64) -> Self {
        let s = Some(seed);
        let n = |base: i64, kind: Kind| GenLayer::new(base, kind, s);

        let mut genlayer = n(1, Kind::Island);
        genlayer = n(2000, Kind::FuzzyZoom(genlayer));
        let genlayeraddisland = n(1, Kind::AddIsland(genlayer));
        let genlayerzoom = n(2001, Kind::Zoom(genlayeraddisland));
        let mut genlayeraddisland1 = n(2, Kind::AddIsland(genlayerzoom));
        genlayeraddisland1 = n(50, Kind::AddIsland(genlayeraddisland1));
        genlayeraddisland1 = n(70, Kind::AddIsland(genlayeraddisland1));
        let genlayerremovetoomuchocean = n(2, Kind::RemoveTooMuchOcean(genlayeraddisland1));
        let genlayeraddsnow = n(2, Kind::AddSnow(genlayerremovetoomuchocean));
        let genlayeraddisland2 = n(3, Kind::AddIsland(genlayeraddsnow));
        let mut genlayeredge = n(2, Kind::Edge(genlayeraddisland2, EdgeMode::CoolWarm));
        genlayeredge = n(2, Kind::Edge(genlayeredge, EdgeMode::HeatIce));
        genlayeredge = n(3, Kind::Edge(genlayeredge, EdgeMode::Special));
        let mut genlayerzoom1 = n(2002, Kind::Zoom(genlayeredge));
        genlayerzoom1 = n(2003, Kind::Zoom(genlayerzoom1));
        let genlayeraddisland3 = n(4, Kind::AddIsland(genlayerzoom1));
        let genlayeraddmushroomisland = n(5, Kind::AddMushroomIsland(genlayeraddisland3));
        let genlayerdeepocean = n(4, Kind::DeepOcean(genlayeraddmushroomisland));
        let genlayer4 = magnify(1000, genlayerdeepocean, 0, s);
        let i = 4;
        let j = 4;
        let lvt_7_1_ = magnify(1000, genlayer4.clone(), 0, s);
        let genlayerriverinit = n(100, Kind::RiverInit(lvt_7_1_));
        let lvt_8_1_ = n(200, Kind::Biome(genlayer4));
        let genlayer6 = magnify(1000, lvt_8_1_, 2, s);
        let genlayerbiomeedge = n(1000, Kind::BiomeEdge(genlayer6));
        let lvt_9_1_ = magnify(1000, genlayerriverinit.clone(), 2, None);
        let mut genlayerhills = n(1000, Kind::Hills { parent: genlayerbiomeedge, river: lvt_9_1_ });
        let mut genlayer5 = magnify(1000, genlayerriverinit, 2, s);
        genlayer5 = magnify(1000, genlayer5, j, s);
        let genlayerriver = n(1, Kind::River(genlayer5));
        let genlayersmooth = n(1000, Kind::Smooth(genlayerriver));
        genlayerhills = n(1001, Kind::RareBiome(genlayerhills));
        for k in 0..i {
            genlayerhills = n(1000 + k as i64, Kind::Zoom(genlayerhills));
            if k == 0 {
                genlayerhills = n(3, Kind::AddIsland(genlayerhills));
            }
            if k == 1 || i == 1 {
                genlayerhills = n(1000, Kind::Shore(genlayerhills));
            }
        }
        let genlayersmooth1 = n(1000, Kind::Smooth(genlayerhills));
        let genlayerrivermix = n(100, Kind::RiverMix { biome: genlayersmooth1, river: genlayersmooth });
        let genlayer3 = n(10, Kind::VoronoiZoom(genlayerrivermix.clone()));
        BiomeLayers { gen_biomes: genlayerrivermix, biome_index: genlayer3 }
    }

    /// Every distinct node reachable from `biome_index`, in the harness's depth-first order
    /// (node, then children in [`GenLayer::children`] order). Shared nodes appear once.
    pub fn walk(&self) -> Vec<Arc<GenLayer>> {
        fn visit(l: &Arc<GenLayer>, out: &mut Vec<Arc<GenLayer>>) {
            if out.iter().any(|o| Arc::ptr_eq(o, l)) {
                return;
            }
            out.push(l.clone());
            for c in l.children() {
                visit(c, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.biome_index, &mut out);
        out
    }
}
