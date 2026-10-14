//! Distant terrain past the render distance, after Distant Horizons and Voxy: chunks out to
//! `lodDistance` are generated on the worker pool, reduced to 2 x 2-column cells of top height
//! and color, and drawn as height-field sections of 8 x 8 chunks whose cells grow coarser with
//! distance (lod_mesh.rs). Fog and the far plane move out to the LOD distance with it.
//!
//! The LOD draws under the real terrain too: a 1024 x 1024 mask of the chunks whose full mesh is on
//! screen discards its fragments there (`shaders/lod.wgsl`), so a chunk still loading inside the
//! render distance shows its LOD instead of a hole.
//!
//! Not a port: vanilla has no distant terrain. The cells come from unpopulated chunks, so trees,
//! plants and lakes are DROPPED, and populate's ice and snow pass is approximated per column by
//! `Biome.getTemperature` at the top block.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, PrimitiveTopology, TexelCopyBufferLayout, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};
use crossbeam_channel::{unbounded, Receiver, Sender};
use worldgen::blocks::id;
use worldgen::Chunk;

use crate::colors::{biome_of, water_color, Colors};
use crate::env::Env;
use crate::lod_mesh::{mask_index, mesh_section, section_distance, section_far_distance, step_for, Cells, LodMesh, AROUND, MASK, SECTION};
use crate::material::LodMaterial;
use crate::models::{Facing, Models};
use crate::settings::Settings;
use crate::sky::{BiomeColors, View};
use crate::stream::{chunk_of, Pos, Streamer};
use crate::Atlas;

/// The tint a block's top takes, as `BlockColors` gives it.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Tint {
    #[default]
    None,
    Grass,
    Foliage,
    Water,
}

/// What a block shows from far away: the average color of its top texture, and its tint.
#[derive(Clone, Copy, Default, Debug)]
pub struct Swatch {
    pub rgb: [u8; 3],
    pub tint: Tint,
}

pub struct Palette {
    /// By block state.
    swatches: Vec<Swatch>,
    snow: Swatch,
    ice: Swatch,
}

/// The alpha-weighted average of an ARGB image.
fn average(argb: &[u32]) -> [u8; 3] {
    let (mut sum, mut weight) = ([0u64; 3], 0u64);
    for &p in argb {
        let a = (p >> 24) as u64;
        for (c, s) in sum.iter_mut().enumerate() {
            *s += ((p >> (16 - c * 8)) & 255) as u64 * a;
        }
        weight += a;
    }
    sum.map(|s| s.checked_div(weight).unwrap_or(0) as u8)
}

fn is_water(state: u16) -> bool {
    matches!(state >> 4, id::FLOWING_WATER | id::WATER)
}

fn is_liquid(state: u16) -> bool {
    matches!(state >> 4, id::FLOWING_WATER | id::WATER | id::FLOWING_LAVA | id::LAVA)
}

impl Palette {
    /// Every state's swatch from its first model variant's top quad (an upward quad, else any).
    pub fn new(models: &Models) -> Palette {
        let t = &models.textures;
        let by_layer: HashMap<u32, [u8; 3]> =
            t.sprites.iter().filter_map(|(name, s)| Some((t.index.get(name)?.layer, average(s.frames.first()?)))).collect();
        let swatch = |state: u16| -> Swatch {
            let rgb = |layer: u32| by_layer.get(&layer).copied().unwrap_or_default();
            match state >> 4 {
                id::FLOWING_WATER | id::WATER => return Swatch { rgb: rgb(models.water_still.layer), tint: Tint::Water },
                id::FLOWING_LAVA | id::LAVA => return Swatch { rgb: rgb(models.lava_still.layer), tint: Tint::None },
                _ => {}
            }
            let Some(m) = models.get(state, false).and_then(|b| b.variants.first()).map(|v| &v.0) else { return Swatch::default() };
            let up = m.face_quads[Facing::Up as usize].first().or_else(|| m.general.iter().find(|q| q.face == Facing::Up));
            let Some(q) = up.or_else(|| m.face_quads.iter().flatten().chain(&m.general).next()) else { return Swatch::default() };
            let tint = match (q.tint >= 0, state >> 4) {
                (false, _) => Tint::None,
                (true, id::GRASS | id::TALLGRASS | id::DOUBLE_PLANT) => Tint::Grass,
                (true, _) => Tint::Foliage,
            };
            Swatch { rgb: rgb(q.sprite.layer), tint }
        };
        let swatches: Vec<Swatch> = (0..=u16::MAX).map(swatch).collect();
        let (snow, ice) = (swatches[(id::SNOW_LAYER << 4) as usize], swatches[(id::ICE << 4) as usize]);
        Palette { swatches, snow, ice }
    }

    #[cfg(test)]
    pub fn from_fn(f: impl Fn(u16) -> Swatch) -> Palette {
        let swatches: Vec<Swatch> = (0..=u16::MAX).map(f).collect();
        let (snow, ice) = (swatches[(id::SNOW_LAYER << 4) as usize], swatches[(id::ICE << 4) as usize]);
        Palette { swatches, snow, ice }
    }
}

/// A generated chunk as LOD cells: each column's top block, its swatch tinted for the column's
/// biome, then 2 x 2 columns to a cell (average color, highest top).
pub fn reduce(chunk: &Chunk, palette: &Palette, colors: &Colors) -> Cells {
    let mut cols = [(0u32, [0u32; 3]); 256];
    for z in 0..16 {
        for x in 0..16 {
            let Some((top, state)) = (0..256).rev().map(|y| (y, chunk.blocks.get(x, y, z))).find(|&(_, s)| s != 0) else { continue };
            let b = biome_of(chunk.biomes[(x + z * 16) as usize]);
            let (wx, wz) = (chunk.x * 16 + x, chunk.z * 16 + z);
            // WorldServer.canBlockFreezeBody and canSnowAtBody's temperature test; the light and
            // edge-water conditions are DROPPED.
            let cold = |y: i32| colors.temperature(b, wx, y, wz) < 0.15;
            let ice = matches!(state >> 4, id::ICE | id::PACKED_ICE);
            let sw = if is_water(state) && cold(top) {
                palette.ice
            } else if !is_liquid(state) && !ice && cold(top + 1) {
                palette.snow
            } else {
                palette.swatches[state as usize]
            };
            let tint = match sw.tint {
                Tint::None => 0xFFFFFF,
                Tint::Grass => colors.biome_grass(b, wx, top, wz),
                Tint::Foliage => colors.biome_foliage(b, wx, top, wz),
                Tint::Water => water_color(b),
            };
            let rgb: [u32; 3] = std::array::from_fn(|c| sw.rgb[c] as u32 * ((tint >> (16 - c * 8)) & 255) as u32 / 255);
            cols[(x + z * 16) as usize] = (top as u32, rgb);
        }
    }
    std::array::from_fn(|i| {
        let (cx, cz) = ((i % 8) * 2, (i / 8) * 2);
        let four = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(a, b)| cols[cx + a + (cz + b) * 16]);
        let top = four.iter().map(|c| c.0).max().unwrap_or(0);
        let rgb: [u32; 3] = std::array::from_fn(|c| four.iter().map(|f| f.1[c]).sum::<u32>() / 4);
        rgb[0] | rgb[1] << 8 | rgb[2] << 16 | top << 24
    })
}

/// The shown-chunk mask the LOD shader reads, `MASK` x `MASK` `R8Unorm`, texel `mask_index(chunk)`.
#[derive(Resource, Clone, ExtractResource)]
pub struct LodMask {
    pub image: Handle<Image>,
    data: Arc<Vec<u8>>,
    generation: u64,
}

pub struct LodPlugin;

impl Plugin for LodPlugin {
    fn build(&self, app: &mut App) {
        let mut image = Image::new(
            Extent3d { width: MASK as u32, height: MASK as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            vec![0; (MASK * MASK) as usize],
            TextureFormat::R8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(LodMask { image: handle, data: Arc::new(vec![0; (MASK * MASK) as usize]), generation: 0 })
            .add_plugins(ExtractResourcePlugin::<LodMask>::default());
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(Render, upload_mask.in_set(RenderSystems::PrepareResources));
        }
    }
}

fn upload_mask(mask: Option<Res<LodMask>>, images: Res<RenderAssets<GpuImage>>, queue: Res<RenderQueue>, mut last: Local<Option<u64>>) {
    let Some(mask) = mask else { return };
    if *last == Some(mask.generation) {
        return;
    }
    let Some(gpu) = images.get(&mask.image) else { return };
    queue.write_texture(
        gpu.texture.as_image_copy(),
        &mask.data,
        TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(MASK as u32), rows_per_image: Some(MASK as u32) },
        Extent3d { width: MASK as u32, height: MASK as u32, depth_or_array_layers: 1 },
    );
    *last = Some(mask.generation);
}

/// The step and the number of chunks with data a section mesh was built from.
type Key = (i32, usize);

enum Msg {
    Reduced(Pos, Cells),
    Meshed(Pos, Key, LodMesh),
}

#[derive(Default)]
struct Section {
    shown: Option<Key>,
    building: Option<Key>,
    entity: Option<Entity>,
    quads: usize,
}

/// Chunk data is kept this many chunks past the LOD radius, so turning back costs nothing.
const MARGIN: i32 = 4;
/// Section meshes started per frame at most.
const MESHES_PER_FRAME: usize = 16;

#[derive(Resource)]
pub struct Lod {
    palette: Arc<Palette>,
    colors: Arc<Colors>,
    material: Handle<LodMaterial>,
    /// Chunks; 0 is off.
    radius: i32,
    center: Option<Pos>,
    /// Offsets within the radius, nearest first, and how far generation has walked them.
    order: Vec<Pos>,
    cursor: usize,
    data: HashMap<Pos, Arc<Cells>>,
    asked: HashSet<Pos>,
    /// Generation and mesh tasks on the workers.
    in_flight: usize,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    sections: HashMap<Pos, Section>,
    dirty: HashSet<Pos>,
    /// `Streamer::shown_rev` the mask was built at.
    mask_rev: Option<u64>,
    pub quads: u64,
}

impl Lod {
    /// Chunks with LOD data, sections on screen.
    pub fn counts(&self) -> (usize, usize) {
        (self.data.len(), self.sections.values().filter(|s| s.entity.is_some()).count())
    }

    fn in_range(&self, p: Pos, center: Pos, r: i32) -> bool {
        let (dx, dz) = (p.0 - center.0, p.1 - center.1);
        dx * dx + dz * dz <= r * r
    }

    fn set_radius(&mut self, r: i32, commands: &mut Commands) {
        self.radius = r;
        self.center = None;
        self.dirty.clear();
        let mut order: Vec<Pos> = (-r..=r).flat_map(|x| (-r..=r).map(move |z| (x, z))).filter(|&(x, z)| x * x + z * z <= r * r).collect();
        order.sort_by_key(|&(x, z)| x * x + z * z);
        self.order = order;
        if r == 0 {
            self.data.clear();
            for (_, s) in self.sections.drain() {
                if let Some(e) = s.entity {
                    commands.entity(e).despawn();
                }
            }
            self.quads = 0;
        }
    }

    /// Marks the sections whose mesh reads chunk `p`.
    fn touch(&mut self, p: Pos) {
        for dx in [-1, 0, 1] {
            for dz in [-1, 0, 1] {
                self.dirty.insert(((p.0 + dx).div_euclid(SECTION), (p.1 + dz).div_euclid(SECTION)));
            }
        }
    }

    /// Starts the mesh of section `s` when every chunk it reads inside the radius has data and
    /// the mesh on screen was built from less. True when a task started.
    fn build(&mut self, s: Pos, center: Pos, render_distance: i32, streamer: &Streamer) -> bool {
        let d = section_distance(s, center);
        if d > self.radius {
            return false;
        }
        // A section the render distance covers whole only fills holes while chunks load.
        let step = if section_far_distance(s, center) < render_distance - 1 { step_for(d).max(8) } else { step_for(d) };
        let mut chunks = Vec::with_capacity((AROUND * AROUND) as usize);
        let mut present = 0;
        for i in 0..AROUND * AROUND {
            let p = (s.0 * SECTION + i % AROUND - 1, s.1 * SECTION + i / AROUND - 1);
            let c = self.data.get(&p).cloned();
            if c.is_none() && self.in_range(p, center, self.radius) {
                return false;
            }
            present += c.is_some() as usize;
            chunks.push(c);
        }
        let key = (step, present);
        let sec = self.sections.entry(s).or_default();
        if sec.shown == Some(key) || sec.building == Some(key) {
            return false;
        }
        sec.building = Some(key);
        self.in_flight += 1;
        let tx = self.tx.clone();
        streamer.spawn_task(move || {
            let _ = tx.send(Msg::Meshed(s, key, mesh_section(&chunks, step, (s.0 * SECTION, s.1 * SECTION))));
        });
        true
    }
}

pub fn setup(mut commands: Commands, atlas: Res<Atlas>, colors: Res<BiomeColors>, env: Res<Env>, mask: Res<LodMask>, mut materials: ResMut<Assets<LodMaterial>>) {
    let (tx, rx) = unbounded();
    commands.insert_resource(Lod {
        palette: Arc::new(Palette::new(&atlas.0)),
        colors: colors.0.clone(),
        material: materials.add(LodMaterial { env: env.image.clone(), mask: mask.image.clone() }),
        radius: 0,
        center: None,
        order: Vec::new(),
        cursor: 0,
        data: HashMap::new(),
        asked: HashSet::new(),
        in_flight: 0,
        tx,
        rx,
        sections: HashMap::new(),
        dirty: HashSet::new(),
        mask_rev: None,
        quads: 0,
    });
}

#[derive(Component)]
pub struct LodMarker;

fn to_mesh(m: LodMesh) -> Mesh {
    let indices = if m.positions.len() <= u16::MAX as usize + 1 {
        Indices::U16(m.indices.iter().map(|&i| i as u16).collect())
    } else {
        Indices::U32(m.indices)
    };
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.positions)
        .with_inserted_attribute(crate::material::ATTRIBUTE_LOD, m.packed)
        .with_inserted_indices(indices)
}

/// Collects reduced chunks and section meshes, follows the player, rebuilds the mask, and starts
/// generation (fewer tasks while the chunk streamer has work queued) and section meshes.
pub fn update_lod(
    mut commands: Commands,
    mut lod: ResMut<Lod>,
    mut mask: ResMut<LodMask>,
    streamer: Res<Streamer>,
    settings: Res<Settings>,
    view: Res<View>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let l = &mut *lod;
    let radius = settings.lod_chunks();
    if radius != l.radius {
        l.set_radius(radius, &mut commands);
    }
    let center = chunk_of(view.feet);

    // Results.
    while let Ok(msg) = l.rx.try_recv() {
        l.in_flight -= 1;
        match msg {
            Msg::Reduced(p, cells) => {
                l.asked.remove(&p);
                if l.radius > 0 && l.in_range(p, center, l.radius + MARGIN) {
                    l.data.insert(p, Arc::new(cells));
                    l.touch(p);
                }
            }
            Msg::Meshed(s, key, m) => {
                let Some(sec) = l.sections.get_mut(&s).filter(|sec| sec.building == Some(key)) else { continue };
                sec.building = None;
                if let Some(e) = sec.entity.take() {
                    commands.entity(e).despawn();
                }
                l.quads -= sec.quads as u64;
                sec.quads = m.quads();
                l.quads += sec.quads as u64;
                if sec.quads > 0 {
                    let origin = Transform::from_xyz((s.0 * SECTION * 16) as f32, 0.0, (s.1 * SECTION * 16) as f32);
                    sec.entity = Some(commands.spawn((Mesh3d(meshes.add(to_mesh(m))), MeshMaterial3d(l.material.clone()), origin, LodMarker, NotShadowCaster)).id());
                }
                sec.shown = Some(key);
                l.dirty.insert(s);
            }
        }
    }
    if l.radius == 0 {
        return;
    }

    // Following the player: drop what fell out of range, and recheck the sections near the edge
    // and those whose step changes.
    if l.center != Some(center) {
        let first = l.center.is_none();
        l.center = Some(center);
        l.cursor = 0;
        let keep = l.radius + MARGIN;
        l.data.retain(|&(x, z), _| (x - center.0).pow(2) + (z - center.1).pow(2) <= keep * keep);
        let r = l.radius;
        let mut gone = Vec::new();
        for (&s, sec) in &l.sections {
            if section_distance(s, center) > r {
                gone.push(s);
            } else if first || section_distance(s, center) + 2 * SECTION >= r || sec.shown.is_some_and(|k| k.0 != step_for(section_distance(s, center))) {
                l.dirty.insert(s);
            }
        }
        for s in gone {
            if let Some(sec) = l.sections.remove(&s) {
                l.quads -= sec.quads as u64;
                if let Some(e) = sec.entity {
                    commands.entity(e).despawn();
                }
            }
        }
        let (lo, hi) = ((center.0 - r).div_euclid(SECTION), (center.0 + r).div_euclid(SECTION));
        let (lo_z, hi_z) = ((center.1 - r).div_euclid(SECTION), (center.1 + r).div_euclid(SECTION));
        for sx in lo..=hi {
            for sz in lo_z..=hi_z {
                if section_distance((sx, sz), center) <= r && !l.sections.contains_key(&(sx, sz)) {
                    l.dirty.insert((sx, sz));
                }
            }
        }
    }

    // The mask, when the set of shown chunks changed.
    if l.mask_rev != Some(streamer.shown_rev) {
        l.mask_rev = Some(streamer.shown_rev);
        let mut data = vec![0u8; (MASK * MASK) as usize];
        for p in streamer.shown() {
            data[mask_index(p)] = 255;
        }
        let m = &mut *mask;
        m.data = Arc::new(data);
        m.generation += 1;
    }

    // Section meshes, nearest first.
    let mut dirty: Vec<Pos> = l.dirty.drain().collect();
    dirty.sort_by_key(|&s| section_distance(s, center));
    let mut started = 0;
    for (i, &s) in dirty.iter().enumerate() {
        if started == MESHES_PER_FRAME {
            l.dirty.extend(&dirty[i..]);
            break;
        }
        started += l.build(s, center, settings.render_distance, &streamer) as usize;
    }

    // Generation, nearest first. The memory brake stops it.
    let cap = if streamer.brake.on {
        0
    } else if streamer.busy {
        (streamer.workers / 4).max(1)
    } else {
        streamer.workers * 2
    };
    while l.in_flight < cap && l.cursor < l.order.len() {
        let d = l.order[l.cursor];
        l.cursor += 1;
        let p = (center.0 + d.0, center.1 + d.1);
        if l.data.contains_key(&p) || !l.asked.insert(p) {
            continue;
        }
        l.in_flight += 1;
        let (g, palette, colors, tx) = (streamer.generator.clone(), l.palette.clone(), l.colors.clone(), l.tx.clone());
        streamer.spawn_task(move || {
            let chunk = g.generate(p.0, p.1);
            let _ = tx.send(Msg::Reduced(p, reduce(&chunk, &palette, &colors)));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::tests::chunk;
    use worldgen::blocks::{state, GRASS, WATER};

    fn palette() -> Palette {
        Palette::from_fn(|s| match s >> 4 {
            id::STONE => Swatch { rgb: [100, 100, 100], tint: Tint::None },
            id::GRASS => Swatch { rgb: [200, 200, 200], tint: Tint::Grass },
            id::WATER => Swatch { rgb: [60, 90, 255], tint: Tint::Water },
            id::SNOW_LAYER => Swatch { rgb: [250, 250, 250], tint: Tint::None },
            id::ICE => Swatch { rgb: [120, 160, 250], tint: Tint::None },
            _ => Swatch::default(),
        })
    }

    #[test]
    fn columns_reduce_to_cells() {
        let colors = Colors::from_buffers(vec![0xFF00FF00; 65536], vec![0xFF0000FF; 65536]);
        // Stone up to 63; one grass column at (3, 5); a pillar at (10, 10) up to 80.
        let c = chunk(0, 0, 64, |p| {
            p.set(3, 64, 5, GRASS);
            for y in 64..=80 {
                p.set(10, y, 10, state(id::STONE, 0));
            }
        });
        let cells = reduce(&c, &palette(), &colors);
        let unpack = |v: u32| ((v & 255, v >> 8 & 255, v >> 16 & 255), v >> 24);
        assert_eq!(unpack(cells[0]), ((100, 100, 100), 63));
        // Cell (1, 2): three stone columns and the grass, green from the plains colormap.
        assert_eq!(unpack(cells[1 + 2 * 8]), ((75, 125, 75), 64));
        assert_eq!(unpack(cells[5 + 5 * 8]).1, 80);
    }

    #[test]
    fn cold_water_freezes_and_cold_ground_snows() {
        let colors = Colors::from_buffers(vec![0xFFFFFFFF; 65536], vec![0xFFFFFFFF; 65536]);
        let mut c = chunk(0, 0, 60, |p| {
            for x in 0..8 {
                for z in 0..16 {
                    p.set(x, 60, z, WATER);
                }
            }
        });
        // Ice plains, temperature 0.0.
        c.biomes = [12; 256];
        let cells = reduce(&c, &palette(), &colors);
        assert_eq!(cells[0] & 0xFFFFFF, 120 | 160 << 8 | 250 << 16);
        assert_eq!(cells[7] & 0xFFFFFF, 250 | 250 << 8 | 250 << 16);
        // Plains stay as they are.
        c.biomes = [1; 256];
        let cells = reduce(&c, &palette(), &colors);
        assert_eq!(cells[0] & 0xFFFFFF, 60 | 90 << 8 | 255 << 16);
    }

    #[test]
    fn average_weights_by_alpha() {
        assert_eq!(average(&[0xFF_FF0000, 0x00_00FF00, 0xFF_0000FF]), [127, 0, 127]);
        assert_eq!(average(&[0]), [0, 0, 0]);
    }
}
