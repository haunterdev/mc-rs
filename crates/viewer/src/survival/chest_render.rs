//! `TileEntityChestRenderer` with `ModelChest` and `ModelLargeChest`: the chest block is
//! invisible in the block mesh (it is a tile entity renderer's), so this draws it. The mesher
//! reports each chest it passes ([`crate::stream::Streamer::chests`]); a chest with a chest to
//! its north or west is the second half of a double and draws nothing, its partner draws the
//! 30 pixel wide large model.
//!
//! The lid is a `TileEntityChest`: `S_BLOCK_ACTION` (action 1) sets `numPlayersUsing`, and
//! `update` eases `lidAngle` by 0.1 a client tick.
//!
//! DROPPED: the Christmas textures, trapped chests (the server has none), and the destroy-stage
//! overlay (`destroyStage`, the crack shows on the block's selection instead).

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use worldgen::blocks::id;

use super::BlockAction;
use crate::audio::{Audio, Category};
use crate::hand::BoxMesh;
use crate::material::EntityMaterial;
use crate::players::Skin;
use crate::sky::{neighbor_light, Clock, View};
use crate::stream::Streamer;
use crate::target::BlockPos;

/// `TileEntity.getMaxRenderDistanceSquared`: 64 blocks.
const RENDER_DISTANCE_SQ: f32 = 4096.0;

/// `textures/entity/chest/normal.png` (64 x 64) and `normal_double.png` (128 x 64), as the world
/// draws them. Set up by main.rs.
#[derive(Resource)]
pub struct ChestImages {
    pub single: Handle<Image>,
    pub double: Handle<Image>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Chests>()
        .add_systems(Update, (block_actions, tick_lids, render).chain().after(crate::player::run_ticks).run_if(in_world))
        .add_systems(Update, demo.run_if(|a: Option<Res<crate::Args>>| a.is_some_and(|a| a.chests)));
}

/// `--chests`: a chest at 3 blocks left and a large chest in the middle, four blocks in front of
/// the `--at` eye (facing north, the latch toward the camera), the large one with its lid up, for a `--shot`.
fn demo(mut streamer: ResMut<Streamer>, mut chests: ResMut<Chests>, args: Res<crate::Args>, mut done: Local<bool>) {
    let Some([x, y, z, ..]) = args.at else { return };
    let (x, y, z) = (x.floor() as i32 - 1, y.floor() as i32 - 2, z.floor() as i32 + 4);
    if *done || streamer.state_at(x, y, z).is_none() {
        return;
    }
    *done = true;
    let chest = (id::CHEST << 4 | 2) as u16;
    let edit = |at: (i32, i32, i32)| sim::terrain::Edit { at, s: chest, flags: 3, place: true };
    streamer.edit(vec![edit((x - 3, y, z)), edit((x, y, z)), edit((x + 1, y, z))]);
    chests.lids.insert((x, y, z), Lid { angle: 0.7, prev: 0.7, using: 1 });
    chests.lids.insert((x + 1, y, z), Lid { angle: 0.7, prev: 0.7, using: 1 });
}

/// The resources the world systems read; a bare plugin (the frame tests) has none of them.
fn in_world(clock: Option<Res<Clock>>, streamer: Option<Res<Streamer>>, view: Option<Res<View>>) -> bool {
    clock.is_some() && streamer.is_some() && view.is_some()
}

/// Which of the four horizontal neighbors are chests: `adjacentChestZNeg`, `XPos`, `XNeg`, `ZPos`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Adjacent {
    pub z_neg: bool,
    pub x_pos: bool,
    pub x_neg: bool,
    pub z_pos: bool,
}

impl Adjacent {
    /// `TileEntityChest.checkForAdjacentChests`: `isChestAt` for west, east, north and south.
    pub fn of(pos: BlockPos, is_chest: impl Fn(BlockPos) -> bool) -> Adjacent {
        Adjacent {
            x_neg: is_chest((pos.0 - 1, pos.1, pos.2)),
            x_pos: is_chest((pos.0 + 1, pos.1, pos.2)),
            z_neg: is_chest((pos.0, pos.1, pos.2 - 1)),
            z_pos: is_chest((pos.0, pos.1, pos.2 + 1)),
        }
    }

    /// The renderer's `te.adjacentChestZNeg == null && te.adjacentChestXNeg == null`: this chest
    /// draws, alone or as the north or west half of a double.
    pub fn draws(&self) -> bool {
        !self.z_neg && !self.x_neg
    }

    /// `adjacentChestXPos != null || adjacentChestZPos != null`: the large model.
    pub fn is_double(&self) -> bool {
        self.x_pos || self.z_pos
    }
}

/// The renderer's matrix from model pixels (times 1/16) to the world: `translate(x, y + 1, z + 1)`,
/// `scale(1, -1, -1)`, `translate(0.5)`, the double chest's shift, `rotate(j)` about Y,
/// `translate(-0.5)`. `meta` is the block's facing index (2 north, 3 south, 4 west, 5 east).
pub fn chest_matrix(pos: BlockPos, meta: u16, adj: Adjacent) -> Mat4 {
    let mut m = Mat4::from_translation(Vec3::new(pos.0 as f32, pos.1 as f32 + 1.0, pos.2 as f32 + 1.0))
        * Mat4::from_scale(Vec3::new(1.0, -1.0, -1.0))
        * Mat4::from_translation(Vec3::splat(0.5));
    let j = match meta {
        2 => 180.0,
        4 => 90.0,
        5 => -90.0,
        _ => 0.0,
    };
    if meta == 2 && adj.x_pos {
        m *= Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0));
    }
    if meta == 5 && adj.z_pos {
        m *= Mat4::from_translation(Vec3::new(0.0, 0.0, -1.0));
    }
    m * Mat4::from_rotation_y(f32::to_radians(j)) * Mat4::from_translation(Vec3::splat(-0.5))
}

/// The lid's `rotateAngleX`: `f = prev + (lid - prev) * partial`, eased `1 - (1 - f)^3`, times
/// -pi/2 (`1.5707964F`).
pub fn lid_rotation(prev: f32, lid: f32, partial: f32) -> f32 {
    let mut f = prev + (lid - prev) * partial;
    f = 1.0 - f;
    f = 1.0 - f * f * f;
    -(f * 1.5707964)
}

/// A `TileEntityChest`'s lid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lid {
    pub angle: f32,
    pub prev: f32,
    /// `numPlayersUsing`
    pub using: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LidSound {
    Open,
    Close,
}

impl Lid {
    /// The lid half of `TileEntityChest.update`. `draws` is `adjacentChestZNeg == null &&
    /// adjacentChestXNeg == null`, the half that plays the sounds.
    pub fn tick(&mut self, draws: bool) -> Option<LidSound> {
        self.prev = self.angle;
        let mut sound = None;
        if self.using > 0 && self.angle == 0.0 && draws {
            sound = Some(LidSound::Open);
        }
        if self.using == 0 && self.angle > 0.0 || self.using > 0 && self.angle < 1.0 {
            let before = self.angle;
            if self.using > 0 {
                self.angle += 0.1;
            } else {
                self.angle -= 0.1;
            }
            if self.angle > 1.0 {
                self.angle = 1.0;
            }
            if self.angle < 0.5 && before >= 0.5 && draws {
                sound = Some(LidSound::Close);
            }
            if self.angle < 0.0 {
                self.angle = 0.0;
            }
        }
        sound
    }

    fn at_rest(&self) -> bool {
        self.using <= 0 && self.angle == 0.0 && self.prev == 0.0
    }
}

/// `ModelChest` (single) or `ModelLargeChest`, as the base and the lid with its knob.
struct ChestMeshes {
    base: Handle<Mesh>,
    lid: Handle<Mesh>,
    big_base: Handle<Mesh>,
    big_lid: Handle<Mesh>,
}

/// A mesh from model boxes, each offset by its `rotationPoint` (pixels) and scaled by 1/16
/// (`ModelRenderer.render(0.0625)`).
fn model_mesh(parts: &[(f32, f32, i32, i32, [f32; 3], i32, i32, i32, [f32; 3])]) -> Mesh {
    let mut b = BoxMesh::default();
    for &(tw, th, u, v, o, dx, dy, dz, point) in parts {
        let from = b.pos.len();
        b.model_box_on(tw, th, u, v, o[0], o[1], o[2], dx, dy, dz, 0.0);
        for p in &mut b.pos[from..] {
            *p = [(p[0] + point[0]) * 0.0625, (p[1] + point[1]) * 0.0625, (p[2] + point[2]) * 0.0625];
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
        .with_inserted_indices(Indices::U32(b.idx))
}

/// The lid and knob turn about the lid's `rotationPoint` (y 7, z 15), so their meshes are
/// relative to it, and only x (1 for the lid, 8 or 16 for the knob) stays in.
const PIVOT: Vec3 = Vec3::new(0.0, 7.0 * 0.0625, 15.0 * 0.0625);

fn chest_meshes(meshes: &mut Assets<Mesh>) -> ChestMeshes {
    // ModelChest: chestLid 14 x 5 x 14, chestKnob 2 x 4 x 1, chestBelow 14 x 10 x 14, 64 x 64.
    let base = model_mesh(&[(64.0, 64.0, 0, 19, [0.0, 0.0, 0.0], 14, 10, 14, [1.0, 6.0, 1.0])]);
    let lid = model_mesh(&[
        (64.0, 64.0, 0, 0, [0.0, -5.0, -14.0], 14, 5, 14, [1.0, 0.0, 0.0]),
        (64.0, 64.0, 0, 0, [-1.0, -2.0, -15.0], 2, 4, 1, [8.0, 0.0, 0.0]),
    ]);
    // ModelLargeChest: the same on 128 x 64 with 30 wide boxes and the knob at x 16.
    let big_base = model_mesh(&[(128.0, 64.0, 0, 19, [0.0, 0.0, 0.0], 30, 10, 14, [1.0, 6.0, 1.0])]);
    let big_lid = model_mesh(&[
        (128.0, 64.0, 0, 0, [0.0, -5.0, -14.0], 30, 5, 14, [1.0, 0.0, 0.0]),
        (128.0, 64.0, 0, 0, [-1.0, -2.0, -15.0], 2, 4, 1, [16.0, 0.0, 0.0]),
    ]);
    ChestMeshes { base: meshes.add(base), lid: meshes.add(lid), big_base: meshes.add(big_base), big_lid: meshes.add(big_lid) }
}

/// The entities drawing one chest.
struct Drawn {
    base: Entity,
    lid: Entity,
    material: Handle<EntityMaterial>,
    double: bool,
}

#[derive(Resource, Default)]
pub struct Chests {
    pub lids: HashMap<BlockPos, Lid>,
    drawn: HashMap<BlockPos, Drawn>,
}

/// `TileEntityChest.receiveClientEvent`: id 1 sets `numPlayersUsing`.
fn block_actions(mut actions: MessageReader<BlockAction>, mut chests: ResMut<Chests>) {
    for BlockAction(a) in actions.read() {
        if a.block == id::CHEST && a.a == 1 {
            chests.lids.entry((a.pos[0], a.pos[1], a.pos[2])).or_default().using = a.b as i32;
        }
    }
}

fn is_chest(streamer: &Streamer, p: BlockPos) -> bool {
    streamer.state_at(p.0, p.1, p.2).is_some_and(|s| s >> 4 == id::CHEST)
}

/// The client ticks of every moving lid, and the sounds `update` plays.
fn tick_lids(mut chests: ResMut<Chests>, clock: Res<Clock>, streamer: Res<Streamer>, audio: Option<Res<Audio>>, mut seed: Local<u32>) {
    if chests.lids.is_empty() || clock.ticks_this_frame == 0 {
        return;
    }
    for _ in 0..clock.ticks_this_frame {
        for (&pos, lid) in chests.lids.iter_mut() {
            let adj = Adjacent::of(pos, |p| is_chest(&streamer, p));
            if let Some(sound) = lid.tick(adj.draws()) {
                let at = Vec3::new(pos.0 as f32 + 0.5 + if adj.x_pos { 0.5 } else { 0.0 }, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5 + if adj.z_pos { 0.5 } else { 0.0 });
                // rand.nextFloat() * 0.1 + 0.9
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let pitch = (*seed >> 8) as f32 / (1u32 << 24) as f32 * 0.1 + 0.9;
                let event = if sound == LidSound::Open { "block.chest.open" } else { "block.chest.close" };
                if let Some(audio) = &audio {
                    audio.play(event, Category::Blocks, Some(at), 0.5, pitch);
                }
            }
        }
    }
    chests.lids.retain(|_, l| !l.at_rest());
}

/// `TileEntityChestRenderer.render` for every chest the mesher found within 64 blocks.
#[allow(clippy::too_many_arguments)]
fn render(
    mut commands: Commands,
    mut chests: ResMut<Chests>,
    have: Option<Res<ChestMeshesRes>>,
    streamer: Res<Streamer>,
    view: Res<View>,
    clock: Res<Clock>,
    skin: Option<Res<Skin>>,
    images: Option<Res<ChestImages>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut parts: Query<(&mut Transform, &mut Visibility, &mut Mesh3d)>,
) {
    let (Some(skin), Some(images)) = (skin, images) else { return };
    let found = streamer.chests();
    if found.is_empty() && chests.drawn.is_empty() {
        return;
    }
    let shared = match have {
        Some(m) => m.0.clone(),
        None => {
            let m = std::sync::Arc::new(chest_meshes(&mut meshes));
            commands.insert_resource(ChestMeshesRes(m.clone()));
            m
        }
    };
    let chests = &mut *chests;
    let mut seen = HashSet::new();
    for pos in found {
        let Some(state) = streamer.state_at(pos.0, pos.1, pos.2).filter(|&s| s >> 4 == id::CHEST) else { continue };
        let adj = Adjacent::of(pos, |p| is_chest(&streamer, p));
        let near = Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5).distance_squared(view.feet) < RENDER_DISTANCE_SQ;
        if !adj.draws() {
            continue;
        }
        seen.insert(pos);
        let double = adj.is_double();
        if chests.drawn.get(&pos).is_some_and(|d| d.double != double) {
            if let Some(d) = chests.drawn.remove(&pos) {
                commands.entity(d.base).despawn();
                commands.entity(d.lid).despawn();
            }
        }
        let d = chests.drawn.entry(pos).or_insert_with(|| {
            let image = if double { images.double.clone() } else { images.single.clone() };
            let material = materials.add(EntityMaterial::new(skin.env.clone(), image));
            let (base, lid) = if double { (shared.big_base.clone(), shared.big_lid.clone()) } else { (shared.base.clone(), shared.lid.clone()) };
            let spawn = |commands: &mut Commands, mesh: Handle<Mesh>| {
                commands.spawn((Mesh3d(mesh), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster)).id()
            };
            Drawn { base: spawn(&mut commands, base), lid: spawn(&mut commands, lid), material: material.clone(), double }
        });
        let m = chest_matrix(pos, state & 15, adj);
        let lid = chests.lids.get(&pos).copied().unwrap_or_default();
        let angle = lid_rotation(lid.prev, lid.angle, clock.partial);
        let lid_m = m * Mat4::from_translation(PIVOT) * Mat4::from_rotation_x(angle);
        for (e, matrix) in [(d.base, m), (d.lid, lid_m)] {
            if let Ok((mut t, mut v, _)) = parts.get_mut(e) {
                let want = Transform::from_matrix(matrix);
                if *t != want {
                    *t = want;
                }
                let vis = if near { Visibility::Inherited } else { Visibility::Hidden };
                if *v != vis {
                    *v = vis;
                }
            }
        }
        // getCombinedLight(pos, 0)
        let (sky, block) = neighbor_light(&streamer, pos.0, pos.1, pos.2);
        let light = Vec4::new(block as f32 * 16.0, sky as f32 * 16.0, 0.0, 0.0);
        if materials.get(&d.material).is_some_and(|m| m.light != light) {
            if let Some(mut m) = materials.get_mut(&d.material) {
                m.light = light;
            }
        }
    }
    chests.drawn.retain(|pos, d| {
        let keep = seen.contains(pos);
        if !keep {
            commands.entity(d.base).despawn();
            commands.entity(d.lid).despawn();
        }
        keep
    });
}

#[derive(Resource)]
struct ChestMeshesRes(std::sync::Arc<ChestMeshes>);

#[cfg(test)]
mod tests {
    use super::*;

    fn set(list: &[BlockPos]) -> impl Fn(BlockPos) -> bool + '_ {
        move |p| list.contains(&p)
    }

    #[test]
    fn a_double_chest_draws_once_from_its_north_or_west_half() {
        let chests = [(10, 64, 10), (11, 64, 10)];
        let west = Adjacent::of(chests[0], set(&chests));
        let east = Adjacent::of(chests[1], set(&chests));
        assert!(west.x_pos && !west.x_neg && west.draws() && west.is_double());
        assert!(east.x_neg && !east.draws());
        let north_south = [(0, 5, 0), (0, 5, 1)];
        assert!(Adjacent::of(north_south[0], set(&north_south)).draws());
        assert!(!Adjacent::of(north_south[1], set(&north_south)).draws());
        // Any chest next to it pairs, whatever it faces; a diagonal one does not.
        let lone = Adjacent::of((0, 0, 0), set(&[(1, 0, 1), (0, 1, 0)]));
        assert!(lone.draws() && !lone.is_double());
    }

    /// The latch: the knob's middle in model pixels, x 8, y 7, z 0.5 (the box runs z -15 to -14
    /// from the lid's pivot at z 15).
    fn latch(pos: BlockPos, meta: u16, adj: Adjacent) -> Vec3 {
        chest_matrix(pos, meta, adj).transform_point3(Vec3::new(8.0, 7.0, 0.5) * 0.0625)
    }

    #[test]
    fn the_latch_faces_the_blocks_facing() {
        let a = Adjacent::default();
        let c = |meta| latch((0, 64, 0), meta, a);
        assert!((c(3) - Vec3::new(0.5, 64.5625, 0.96875)).length() < 1e-4, "{}", c(3));
        assert!((c(2) - Vec3::new(0.5, 64.5625, 0.03125)).length() < 1e-4, "{}", c(2));
        assert!((c(4) - Vec3::new(0.03125, 64.5625, 0.5)).length() < 1e-4, "{}", c(4));
        assert!((c(5) - Vec3::new(0.96875, 64.5625, 0.5)).length() < 1e-4, "{}", c(5));
    }

    #[test]
    fn a_double_chests_shift_keeps_it_over_both_blocks() {
        // Facing north with a chest to the east: the 30 wide model runs from x pos to pos + 2.
        let adj = Adjacent { x_pos: true, ..Default::default() };
        let m = chest_matrix((0, 64, 0), 2, adj);
        let lo = m.transform_point3(Vec3::new(1.0, 0.0, 1.0) * 0.0625).x;
        let hi = m.transform_point3(Vec3::new(31.0, 0.0, 1.0) * 0.0625).x;
        assert!((lo - 1.9375).abs() < 1e-4 && (hi - 0.0625).abs() < 1e-4, "{lo} {hi}");
        // Facing east with a chest to the south shifts back one block.
        let m = chest_matrix((0, 64, 0), 5, Adjacent { z_pos: true, ..Default::default() });
        let z = [1.0, 31.0].map(|x| m.transform_point3(Vec3::new(x, 0.0, 1.0) * 0.0625).z);
        assert!((z[0] - 1.9375).abs() < 1e-4 && (z[1] - 0.0625).abs() < 1e-4, "{z:?}");
    }

    #[test]
    fn the_lid_eases_open_and_shut() {
        assert_eq!(lid_rotation(0.0, 0.0, 0.5), -0.0);
        assert!((lid_rotation(1.0, 1.0, 0.5) + std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // Halfway: 1 - 0.5^3 = 0.875 of a quarter turn.
        assert!((lid_rotation(0.5, 0.5, 0.0) + 0.875 * std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        // Between ticks it interpolates the angle first.
        assert_eq!(lid_rotation(0.2, 0.4, 0.5), lid_rotation(0.3, 0.3, 0.0));
    }

    #[test]
    fn the_lid_moves_a_tenth_a_tick_and_plays_its_sounds_once() {
        let mut lid = Lid { using: 1, ..Default::default() };
        assert_eq!(lid.tick(true), Some(LidSound::Open));
        assert!((lid.angle - 0.1).abs() < 1e-6 && lid.prev == 0.0);
        let mut sounds = 0;
        for _ in 0..20 {
            sounds += lid.tick(true).is_some() as i32;
        }
        assert_eq!(lid.angle, 1.0);
        assert_eq!(sounds, 0);
        lid.using = 0;
        let mut ticks = 0;
        let mut close = 0;
        while lid.angle > 0.0 {
            close += (lid.tick(true) == Some(LidSound::Close)) as i32;
            ticks += 1;
        }
        assert_eq!((ticks, close), (10, 1));
        assert_eq!(lid.angle, 0.0);
        // The second half of a double plays nothing.
        let mut lid = Lid { using: 1, ..Default::default() };
        assert_eq!(lid.tick(false), None);
    }
}
