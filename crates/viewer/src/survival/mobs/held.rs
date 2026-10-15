//! `LayerHeldItem` for mobs: the main hand item a mob's `S_EQUIPMENT` slot 0 says (a skeleton's
//! bow), drawn on its right arm with the item mesh and the third person display of
//! `players/held.rs`, which this reuses (`local_matrix`, the item material setup).
//!
//! DROPPED: the off hand and the left handed side (every mob is right handed, and `S_EQUIPMENT`
//! slot 1 is not drawn), the enchantment glint, and the item's pull-back models while a bow draws
//! (the bow model is the resting one).

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use items::ItemStack;

use super::models::part_transforms;
use super::render::Mob;
use crate::hand::standard_lights;
use crate::item::model_key;
use crate::item_render::{ItemMaterials, ItemMeshes};
use crate::material::{EntityMaterial, ItemMaterial};
use crate::players::local_matrix;
use crate::sky::Clock;

/// A mob's held item mesh entity and its material, on the mob's root.
#[derive(Component)]
pub struct HeldMob {
    entity: Entity,
    material: Handle<ItemMaterial>,
}

/// The item stack an equipment frame puts in a hand: the id and metadata, `None` for empty.
pub fn stack_of(id: i16, meta: u16) -> Option<(u16, u16)> {
    (id > 0).then_some((id as u16, meta))
}

/// `LayerHeldItem.doRenderLayer`'s extra transform for a child: translate(0, 0.75, 0), scale(0.5),
/// in blocks, ahead of the arm's.
pub fn child_pre() -> Mat4 {
    Mat4::from_translation(Vec3::new(0.0, 0.75, 0.0)) * Mat4::from_scale(Vec3::splat(0.5))
}

/// The item's matrix below the mob's root (model units): the arm part's transform with
/// `postRenderArm`'s extra x, then `LayerHeldItem.renderHeldItem` as `players/held.rs` has it.
pub fn item_matrix(arm: (Vec3, Vec3), dx: f32, child: bool, display: Mat4) -> Mat4 {
    let (pivot, angles) = arm;
    let inner = local_matrix((pivot + Vec3::new(dx, 0.0, 0.0), angles), false, false, display);
    if child {
        // local_matrix starts with the scale up by 16 from blocks to the root's units
        Mat4::from_scale(Vec3::splat(16.0)) * child_pre() * Mat4::from_scale(Vec3::splat(1.0 / 16.0)) * inner
    } else {
        inner
    }
}

/// Gives each armed mob its held item mesh once, then shows, places and lights it every frame.
/// Runs after `pose`, which sets the part transforms and the light for this frame.
#[allow(clippy::too_many_arguments)]
pub fn pose_held(
    mut commands: Commands,
    clock: Res<Clock>,
    items: Option<Res<ItemMeshes>>,
    item_mats: Option<Res<ItemMaterials>>,
    mut item_assets: ResMut<Assets<ItemMaterial>>,
    entity_mats: Res<Assets<EntityMaterial>>,
    mobs: Query<(Entity, &Mob, Option<&HeldMob>)>,
    mut meshes: Query<(&mut Mesh3d, &mut Transform, &mut Visibility), Without<Mob>>,
) {
    let (Some(items), Some(item_mats)) = (items, item_mats) else { return };
    for (root, mob, held) in &mobs {
        let Some(hand) = mob.def().hand.as_ref() else { continue };
        let Some(held) = held else {
            let Some(mut m) = item_assets.get(&item_mats.hand).cloned() else { continue };
            let [l0, l1] = standard_lights();
            m.item.w0 = l0.extend(0.0);
            m.item.w1 = l1.extend(0.0);
            m.item.flags = UVec4::new(1, 0, 0, 1);
            let material = item_assets.add(m);
            let entity = commands
                .spawn((Mesh3d::default(), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster, ChildOf(root)))
                .id();
            commands.entity(root).insert(HeldMob { entity, material });
            continue;
        };
        let Ok((mut mesh, mut transform, mut visibility)) = meshes.get_mut(held.entity) else { continue };
        let item = mob.main_hand.and_then(|(id, meta)| model_key(&ItemStack::new(id, 1, meta as i32))).and_then(|key| items.0.get(&key));
        let Some(item) = item else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        if mesh.0 != item.mesh {
            mesh.0 = item.mesh.clone();
        }
        let frame = mob.frame(clock.partial);
        let arm = part_transforms(mob.def(), &frame, false)[hand.part];
        let (z, y, x) = arm.rotation.to_euler(EulerRot::ZYX);
        *transform = Transform::from_matrix(item_matrix((arm.translation, Vec3::new(x, y, z)), hand.dx, mob.is_child(), item.third_person));
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
        // The lightmap coordinates pose found at the eyes.
        if let Some(light) = entity_mats.get(mob.material()).map(|m| m.light) {
            if item_assets.get(&held.material).is_some_and(|m| m.item.w0.w != light.x || m.item.w1.w != light.y) {
                if let Some(mut m) = item_assets.get_mut(&held.material) {
                    m.item.w0.w = light.x;
                    m.item.w1.w = light.y;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_slot_holds_nothing() {
        assert_eq!(stack_of(0, 0), None);
        assert_eq!(stack_of(-1, 0), None);
        assert_eq!(stack_of(261, 0), Some((261, 0)));
    }

    #[test]
    fn a_child_holds_the_item_half_size_three_quarters_up() {
        let arm = (Vec3::new(-5.0, 2.0, 0.0), Vec3::ZERO);
        let adult = item_matrix(arm, 0.0, false, Mat4::IDENTITY);
        let child = item_matrix(arm, 0.0, true, Mat4::IDENTITY);
        // the same point of the item, half as far from the origin and 0.75 blocks (12 units) up
        let p = adult.transform_point3(Vec3::ZERO);
        let c = child.transform_point3(Vec3::ZERO);
        assert!((c - (p * 0.5 + Vec3::new(0.0, 12.0, 0.0))).length() < 1e-4, "{p} {c}");
    }

    #[test]
    fn the_skeleton_arm_point_moves_a_unit_out() {
        let arm = (Vec3::new(-5.0, 2.0, 0.0), Vec3::ZERO);
        let a = item_matrix(arm, 0.0, false, Mat4::IDENTITY).transform_point3(Vec3::ZERO);
        let b = item_matrix(arm, 1.0, false, Mat4::IDENTITY).transform_point3(Vec3::ZERO);
        assert!((b - a - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-4);
    }
}
