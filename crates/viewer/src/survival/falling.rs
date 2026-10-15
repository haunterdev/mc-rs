//! Falling blocks in a server session: the `S_SPAWN_OBJECT` blocks (`ObjectData::FallingBlock`),
//! flown on the client between the server's `S_ENTITY_MOVE` updates as `EntityFallingBlock.onUpdate`
//! moves them (`sim::falling_block`, the `world.isRemote` half: gravity, move, drag), and drawn as
//! `RenderFallingBlock.doRender` draws them: the block's model with its cell origin at
//! `(x - 0.5, y, z - 0.5)` of the interpolated feet, lit by the lightmap at
//! `BlockPos(posX, maxY, posZ)`. Hidden while the world already holds the same state at the
//! entity's block (`iblockstate != world.getBlockState(new BlockPos(entity))`).
//!
//! The model is the block item's mesh (the same quads the dropped item and the hotbar icon draw,
//! `ItemMeshes`) on the world-mode item material, so the face shading is the item lights' and
//! not `BlockModelRenderer`'s fixed per-face brightness.
//! DROPPED: the shadow, the outline mode, `getPositionRandom(origin)` for random-offset models
//! (the item model has one variant), non-`MODEL` render types.

use std::collections::HashMap;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::prelude::*;
use proto::{EntityMove, ObjectData, SpawnObject, Velocity};
use sim::falling_block::FallingBlock;

use super::{Destroy, EntityMoves, SpawnObject as SpawnMsg, Velocity as VelocityMsg};
use crate::collision::{floor, BlockAccess};
use crate::hand::standard_lights;
use crate::item_render::{ItemMaterials, ItemMeshes};
use crate::material::ItemMaterial;
use crate::sky::{neighbor_light, Clock, View};
use crate::stream::Streamer;

/// `Entity.isInRangeToRenderDist`: 64 times the box's average edge (0.98).
const RENDER_RANGE: f64 = 64.0 * 0.98;

/// The falling blocks of the session.
#[derive(Resource, Default)]
pub struct FallingBlocks {
    pub map: HashMap<u32, FallingBlock>,
    /// Entity and material per block id, for the draw.
    drawn: HashMap<u32, (Entity, Handle<ItemMaterial>)>,
}

/// The item key of the block's model: the block item and `damageDropped`.
pub fn model_key(state: u16) -> (u16, u16) {
    (state >> 4, items::drop_rules::damage_dropped(state).clamp(0, u16::MAX as i32) as u16)
}

/// `RenderFallingBlock.doRender`'s matrix: the block's cell origin at `(x - 0.5, y, z - 0.5)`.
pub fn block_matrix(feet: Vec3) -> Mat4 {
    Mat4::from_translation(feet - Vec3::new(0.5, 0.0, 0.5))
}

/// The interpolated feet, `lastTickPos` to `pos`.
pub fn feet(e: &FallingBlock, partial: f32) -> DVec3 {
    e.body.prev_pos + (e.body.pos - e.body.prev_pos) * partial as f64
}

impl FallingBlocks {
    /// `handleSpawnObject` for type 70: `new EntityFallingBlock(world, x, y, z, state)` with the
    /// packet's position and motion.
    pub fn spawn(&mut self, m: &SpawnObject) {
        let ObjectData::FallingBlock { state } = m.data else { return };
        let mut e = FallingBlock::new(m.pos[0], m.pos[1], m.pos[2], state);
        // The packet carries the entity's own position (already 0.01 up); the constructor adds it.
        e.body.set_position(DVec3::new(m.pos[0], m.pos[1], m.pos[2]));
        e.body.prev_pos = e.body.pos;
        e.body.motion = DVec3::new(m.motion[0] as f64, m.motion[1] as f64, m.motion[2] as f64);
        self.map.insert(m.id, e);
    }

    /// `handleEntityTeleport` for an `Entity`: position, `onGround`.
    pub fn moves(&mut self, moves: &[EntityMove]) {
        for m in moves {
            if let Some(e) = self.map.get_mut(&m.id) {
                e.body.set_position(DVec3::new(m.pos[0], m.pos[1], m.pos[2]));
                e.body.on_ground = m.on_ground;
            }
        }
    }

    /// `handleEntityVelocity`: `Entity.setVelocity`.
    pub fn velocity(&mut self, v: &Velocity) {
        if let Some(e) = self.map.get_mut(&v.id) {
            e.body.motion = DVec3::new(v.motion[0] as f64, v.motion[1] as f64, v.motion[2] as f64);
        }
    }

    /// `handleDestroyEntities`
    pub fn destroy(&mut self, ids: &[u32]) {
        for id in ids {
            self.map.remove(id);
        }
    }

    /// One client tick: `EntityFallingBlock.onUpdate` for every block in a loaded chunk.
    pub fn tick(&mut self, w: &impl BlockAccess) {
        for e in self.map.values_mut() {
            let p = e.body.pos;
            if !w.loaded(floor(p.x), floor(p.z)) {
                continue;
            }
            e.tick(w, false);
        }
    }
}

fn read_frames(
    mut blocks: ResMut<FallingBlocks>,
    mut spawns: MessageReader<SpawnMsg>,
    mut moves: MessageReader<EntityMoves>,
    mut velocity: MessageReader<VelocityMsg>,
    mut destroy: MessageReader<Destroy>,
) {
    for SpawnMsg(m) in spawns.read() {
        blocks.spawn(m);
    }
    for EntityMoves(m) in moves.read() {
        blocks.moves(m);
    }
    for VelocityMsg(v) in velocity.read() {
        blocks.velocity(v);
    }
    for Destroy(ids) in destroy.read() {
        blocks.destroy(ids);
    }
}

fn tick(clock: Res<Clock>, streamer: Res<Streamer>, mut blocks: ResMut<FallingBlocks>) {
    for _ in 0..clock.ticks_this_frame {
        blocks.tick(&*streamer);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut blocks: ResMut<FallingBlocks>,
    clock: Res<Clock>,
    view: Res<View>,
    streamer: Res<Streamer>,
    meshes: Option<Res<ItemMeshes>>,
    materials: Option<Res<ItemMaterials>>,
    mut assets: ResMut<Assets<ItemMaterial>>,
    mut shown: Query<(&mut Mesh3d, &mut Transform, &mut Visibility)>,
) {
    let (Some(meshes), Some(materials)) = (meshes, materials) else { return };
    let partial = clock.partial;
    let eye = view.eye.as_dvec3();
    let blocks = &mut *blocks;
    let gone: Vec<u32> = blocks.drawn.keys().filter(|id| !blocks.map.contains_key(id)).copied().collect();
    for id in gone {
        if let Some((e, m)) = blocks.drawn.remove(&id) {
            commands.entity(e).despawn();
            assets.remove(&m);
        }
    }
    for (id, e) in &blocks.map {
        let at = feet(e, partial);
        let Some(mesh) = meshes.0.get(&model_key(e.state)) else { continue };
        let p = e.body.pos;
        // The world already holds this block where the entity is: it is not drawn.
        let same = streamer.state(floor(p.x), floor(p.y), floor(p.z)) == e.state;
        let visible = !same && at.distance_squared(eye) <= RENDER_RANGE * RENDER_RANGE;
        // getBrightnessForRender at BlockPos(posX, maxY, posZ).
        let (sky, block) = neighbor_light(&streamer, floor(p.x), floor(e.body.bb.max_y), floor(p.z));
        let light = (block as f32 * 16.0, sky as f32 * 16.0);
        let matrix = Transform::from_matrix(block_matrix(at.as_vec3()));
        match blocks.drawn.get(id) {
            Some((entity, material)) => {
                if let Ok((mut m, mut t, mut v)) = shown.get_mut(*entity) {
                    if m.0 != mesh.mesh {
                        m.0 = mesh.mesh.clone();
                    }
                    *t = matrix;
                    let want = if visible { Visibility::Inherited } else { Visibility::Hidden };
                    if *v != want {
                        *v = want;
                    }
                }
                if let Some(mut m) = assets.get_mut(material) {
                    m.item.w0.w = light.0;
                    m.item.w1.w = light.1;
                }
            }
            None => {
                let Some(mut m) = assets.get(&materials.hand).cloned() else { return };
                let [l0, l1] = standard_lights();
                m.item.w0 = l0.extend(light.0);
                m.item.w1 = l1.extend(light.1);
                m.item.flags = UVec4::new(1, 0, 0, 1);
                let material = assets.add(m);
                let entity = commands
                    .spawn((Mesh3d(mesh.mesh.clone()), MeshMaterial3d(material.clone()), matrix, Visibility::Hidden, NoFrustumCulling, NotShadowCaster))
                    .id();
                blocks.drawn.insert(*id, (entity, material));
            }
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<FallingBlocks>().add_systems(
        Update,
        (
            read_frames,
            tick.after(crate::player::run_ticks).after(read_frames).run_if(resource_exists::<Streamer>).run_if(resource_exists::<Clock>),
            draw.after(crate::sky::advance_clock).after(read_frames).after(tick).run_if(resource_exists::<Streamer>).run_if(resource_exists::<Clock>).run_if(resource_exists::<View>),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat;
    impl BlockAccess for Flat {
        fn state(&self, _x: i32, y: i32, _z: i32) -> u16 {
            if y <= 64 {
                1 << 4
            } else {
                0
            }
        }
        fn loaded(&self, _x: i32, _z: i32) -> bool {
            true
        }
    }

    fn spawn_msg(id: u32, y: f64, state: u16) -> SpawnObject {
        SpawnObject { id, pos: [0.5, y, 0.5], data: ObjectData::FallingBlock { state }, ..Default::default() }
    }

    #[test]
    fn a_spawned_block_falls_on_the_client_and_waits_for_the_destroy() {
        let sand = worldgen::blocks::SAND;
        let mut b = FallingBlocks::default();
        b.spawn(&spawn_msg(3, 70.01, sand));
        b.spawn(&SpawnObject { id: 4, data: ObjectData::Arrow { shooter: 1 }, ..Default::default() });
        assert_eq!(b.map.len(), 1);
        for _ in 0..40 {
            b.tick(&Flat);
        }
        // It rests on the stone; the client never places it, the server's destroy removes it.
        let e = &b.map[&3];
        assert!(e.body.on_ground && !e.dead && (e.body.pos.y - 65.0).abs() < 0.02, "{:?}", e.body.pos);
        b.moves(&[EntityMove { id: 3, pos: [0.5, 80.0, 0.5], on_ground: false, ..Default::default() }]);
        assert_eq!(b.map[&3].body.pos.y, 80.0);
        b.destroy(&[3]);
        assert!(b.map.is_empty());
    }

    #[test]
    fn the_model_is_the_block_item_and_sits_half_a_block_back() {
        assert_eq!(model_key(worldgen::blocks::state(12, 1)), (12, 1));
        let m = block_matrix(Vec3::new(10.5, 70.01, 4.5));
        assert_eq!(m.transform_point3(Vec3::ZERO), Vec3::new(10.0, 70.01, 4.0));
    }
}
