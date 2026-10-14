//! What the crosshair is on: `EntityRenderer.getMouseOver` for blocks over `World.rayTraceBlocks`
//! (in `sim::raytrace`), and the outline
//! `RenderGlobal.drawSelectionBox` draws around the block.

pub use sim::raytrace::*;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

use crate::celestial::Fluid;
use crate::collision::BlockAccess;
use crate::hand::HideGui;
use crate::player::{look, Player};
use crate::region::{props, Mat};
use crate::sky::{Clock, View};
use crate::stream::Streamer;

/// `EntityRenderer.getMouseOver` for blocks: `Entity.rayTrace(reach, partial)` from the eye along
/// the look, keeping the last block passed when nothing is hit.
/// DROPPED: the entities the ray could point at; the world has none.
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
        mouse_over(&*streamer, &player, clock.partial).filter(|h| h.kind == Kind::Block).and_then(|h| {
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
