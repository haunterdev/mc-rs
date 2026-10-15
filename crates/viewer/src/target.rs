//! What the crosshair is on: `EntityRenderer.getMouseOver` for blocks over `World.rayTraceBlocks`
//! (in `sim::raytrace`) and for entities ([`pick_entity`]: the nearest box the look ray crosses
//! before the block), and the outline `RenderGlobal.drawSelectionBox` draws around the block.

pub use sim::raytrace::*;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use crate::celestial::Fluid;
use crate::collision::{Aabb, BlockAccess};
use crate::hand::HideGui;
use crate::player::{look, Player};
use crate::region::{props, Mat};
use crate::sky::{Clock, View};
use crate::stream::Streamer;

/// `EntityRenderer.getMouseOver` for blocks: `Entity.rayTrace(reach, partial)` from the eye along
/// the look, keeping the last block passed when nothing is hit.
/// The entities it could point at are [`pick_entity`]'s.
pub fn mouse_over(w: &impl BlockAccess, player: &Player, partial: f32) -> Option<Hit> {
    let eye = DVec3::Y * player.eye_height() as f64;
    // Entity.getPositionEyes and EntityLivingBase.getLook: the current values at partial 1.
    let (start, l) = if partial == 1.0 {
        (player.pos + eye, look(player.pitch, player.yaw))
    } else {
        (player.interp_pos(partial) + eye, look(player.interp_pitch(partial), player.interp_yaw(partial)))
    };
    let l = l.as_dvec3();
    ray_trace_blocks(w, start, start + DVec3::new(l.x * REACH, l.y * REACH, l.z * REACH), false, true)
}

/// The entity `RayTraceResult` of `getMouseOver`: `entityHit` (the server's id) and `hitVec`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityHit {
    pub id: u32,
    pub vec: DVec3,
}

/// The entities part of `EntityRenderer.getMouseOver`, given the block result it starts from
/// (`objectMouseOver`), the eye, the look at partial tick 1, the player's own box and the boxes
/// of every entity that `canBeCollidedWith` (`getEntitiesInAABBexcluding`'s candidates, each
/// `getEntityBoundingBox`). Returns the new `objectMouseOver`'s block part and the entity it
/// points at: an entity nearer than the block replaces the block result, and in survival one
/// beyond 3 blocks turns the whole result into a MISS.
/// DROPPED: `getCollisionBorderSize` (0 for every entity here), `getLowestRidingEntity` (nothing
/// rides), `EntitySelectors.NOT_SPECTATING` and the item frame check (`mc.pointedEntity`).
pub fn pick_entity(block: Option<Hit>, start: DVec3, look: DVec3, creative: bool, own: &Aabb, candidates: &[(u32, Aabb)]) -> (Option<Hit>, Option<EntityHit>) {
    // getBlockReachDistance is 5 in creative and 4.5 otherwise; extendedReach (creative) makes
    // the entity reach and the starting block distance 6.
    let (mut d1, d0, flag) = if creative { (6.0, 6.0, false) } else { (4.5, 4.5, true) };
    if let Some(h) = &block {
        d1 = h.vec.distance(start);
    }
    let end = start + DVec3::new(look.x * d0, look.y * d0, look.z * d0);
    let search = own.expand(look.x * d0, look.y * d0, look.z * d0).grow(1.0, 1.0, 1.0);
    let mut pointed: Option<(u32, DVec3)> = None;
    let mut d2 = d1;
    for (id, bb) in candidates.iter().filter(|(_, bb)| bb.intersects(&search)) {
        let hit = calculate_intercept(bb, start, end);
        let inside = bb.min_x < start.x && start.x < bb.max_x && bb.min_y < start.y && start.y < bb.max_y && bb.min_z < start.z && start.z < bb.max_z;
        if inside {
            if d2 >= 0.0 {
                pointed = Some((*id, hit.map_or(start, |(v, _)| v)));
                d2 = 0.0;
            }
        } else if let Some((v, _)) = hit {
            let d3 = start.distance(v);
            if d3 < d2 || d2 == 0.0 {
                pointed = Some((*id, v));
                d2 = d3;
            }
        }
    }
    if let Some((_, v)) = pointed {
        if flag && start.distance(v) > 3.0 {
            let pos = (v.x.floor() as i32, v.y.floor() as i32, v.z.floor() as i32);
            return (Some(Hit { kind: Kind::Miss, vec: v, face: worldgen::populate::Facing::Up, pos }), None);
        }
    }
    match pointed {
        Some((id, vec)) if d2 < d1 || block.is_none() => (None, Some(EntityHit { id, vec })),
        _ => (block, None),
    }
}

/// `getMouseOver` for blocks and entities at `partial`: [`mouse_over`], then [`pick_entity`] over
/// `candidates`. The entity ray uses `getLook(1.0F)` whatever the partial tick.
pub fn mouse_over_entities(w: &impl BlockAccess, player: &Player, partial: f32, candidates: &[(u32, Aabb)]) -> (Option<Hit>, Option<EntityHit>) {
    let block = mouse_over(w, player, partial);
    if candidates.is_empty() {
        return (block, None);
    }
    let eye = DVec3::Y * player.eye_height() as f64;
    let start = if partial == 1.0 { player.pos + eye } else { player.interp_pos(partial) + eye };
    let l = look(player.pitch, player.yaw).as_dvec3();
    pick_entity(block, start, l, player.game_mode == proto::GM_CREATIVE, &player.bb, candidates)
}

/// The outline around the block the crosshair is on.
#[derive(Component)]
pub struct Outline;

/// The twelve edges of the unit cube as a line list.
fn outline_mesh() -> Mesh {
    let corner = |i: u32| Vec3::new((i & 1) as f32, (i >> 1 & 1) as f32, (i >> 2 & 1) as f32);
    let mut positions = Vec::with_capacity(24);
    for a in 0..8u32 {
        for bit in [1, 2, 4] {
            if a & bit == 0 {
                positions.push(corner(a));
                positions.push(corner(a | bit));
            }
        }
    }
    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
}

/// `RenderGlobal.drawSelectionBox`'s lines: black at alpha 0.4.
/// DROPPED: the line width of 2; wgpu draws lines one pixel wide.
pub fn spawn_outline(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn((
        Outline,
        Mesh3d(meshes.add(outline_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, 0.4),
            unlit: true,
            fog_enabled: false,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        NotShadowCaster,
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// `EntityRenderer.renderWorldPass`: the outline over the pointed block, from this frame's
/// `getMouseOver(partialTicks)`, unless the GUI is hidden or the eye is in water.
/// DROPPED: the second outline pass drawn after the water for an eye in water.
pub fn update_outline(
    ctl: Res<crate::interact::Controller>,
    clock: Res<Clock>,
    streamer: Res<Streamer>,
    player: Res<Player>,
    view: Res<View>,
    hide: Res<HideGui>,
    outline: Single<(&mut Transform, &mut Visibility), With<Outline>>,
) {
    let (mut transform, mut visibility) = outline.into_inner();
    let shown = if hide.0 || view.fluid == Fluid::Water {
        None
    } else {
        mouse_over_entities(&*streamer, &player, clock.partial, &ctl.entities).0.filter(|h| h.kind == Kind::Block).and_then(|h| {
            let s = streamer.state(h.pos.0, h.pos.1, h.pos.2);
            (props(s).mat != Mat::Air).then(|| selected_box(&*streamer, s, h.pos).grow(0.002, 0.002, 0.002))
        })
    };
    match shown {
        Some(b) => {
            *transform = Transform::from_xyz(b.min_x as f32, b.min_y as f32, b.min_z as f32)
                .with_scale(Vec3::new((b.max_x - b.min_x) as f32, (b.max_y - b.min_y) as f32, (b.max_z - b.min_z) as f32));
            *visibility = Visibility::Inherited;
        }
        None => *visibility = Visibility::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen::populate::Facing;

    fn block_at(dist: f64) -> Hit {
        Hit { kind: Kind::Block, vec: DVec3::new(0.0, 65.0, dist), face: Facing::North, pos: (0, 65, dist.floor() as i32) }
    }

    /// A cow-sized box standing `z` blocks south of the origin, on the ground at y 64 (the test eye is at 65, level with its back).
    fn cow(id: u32, z: f64) -> (u32, Aabb) {
        (id, Aabb::new(-0.45, 64.0, z - 0.45, 0.45, 65.4, z + 0.45))
    }

    const EYE: DVec3 = DVec3::new(0.0, 65.0, 0.0);
    const SOUTH: DVec3 = DVec3::Z;

    fn own() -> Aabb {
        Aabb::new(-0.3, 64.0, -0.3, 0.3, 65.8, 0.3)
    }

    #[test]
    fn a_mob_in_front_of_a_block_is_what_the_crosshair_is_on() {
        let (over, entity) = pick_entity(Some(block_at(4.0)), EYE, SOUTH, false, &own(), &[cow(7, 2.5)]);
        let e = entity.expect("the cow is nearer than the block");
        assert_eq!(e.id, 7);
        assert!((e.vec.z - 2.05).abs() < 1e-9, "the front face: {}", e.vec.z);
        assert!(over.is_none(), "the entity replaces the block result");
    }

    #[test]
    fn a_mob_behind_a_block_is_not() {
        let b = block_at(2.0);
        let (over, entity) = pick_entity(Some(b), EYE, SOUTH, false, &own(), &[cow(7, 3.5)]);
        assert!(entity.is_none());
        assert_eq!(over, Some(b));
    }

    #[test]
    fn with_no_block_in_reach_a_mob_in_reach_is_picked() {
        let (over, entity) = pick_entity(None, EYE, SOUTH, false, &own(), &[cow(3, 2.0)]);
        assert_eq!(entity.map(|e| e.id), Some(3));
        assert!(over.is_none());
    }

    #[test]
    fn the_nearest_of_two_mobs_wins_and_a_mob_to_the_side_does_not_count() {
        let sideways = (9, Aabb::new(3.0, 64.0, 1.0, 3.9, 65.4, 1.9));
        let (_, entity) = pick_entity(None, EYE, SOUTH, false, &own(), &[cow(1, 4.0), sideways, cow(2, 2.0)]);
        assert_eq!(entity.map(|e| e.id), Some(2));
    }

    #[test]
    fn in_survival_a_mob_past_three_blocks_turns_the_result_into_a_miss() {
        let b = block_at(4.4);
        let (over, entity) = pick_entity(Some(b), EYE, SOUTH, false, &own(), &[cow(7, 3.9)]);
        assert!(entity.is_none());
        assert_eq!(over.map(|h| h.kind), Some(Kind::Miss), "the block behind it is hidden too");
        // Creative reaches six blocks for entities and keeps no such cut.
        let (_, entity) = pick_entity(Some(block_at(5.9)), EYE, SOUTH, true, &own(), &[cow(7, 4.4)]);
        assert_eq!(entity.map(|e| e.id), Some(7));
    }

    #[test]
    fn a_mob_out_of_reach_is_not_picked() {
        let (over, entity) = pick_entity(None, EYE, SOUTH, false, &own(), &[cow(7, 5.2)]);
        assert!(entity.is_none() && over.is_none());
    }
}
