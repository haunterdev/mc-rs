//! The items in a player model's hands: `LayerHeldItem.renderHeldItem` for
//! `THIRD_PERSON_RIGHT_HAND` on the right arm and `THIRD_PERSON_LEFT_HAND` on the left, on other
//! players and on the player's own model in third person. The main hand stack comes from
//! `PlayerState`'s `held_item` and `held_meta`, or from `held` (the block state) when a build
//! before those fields sent it; the off hand from `held_item_off` and `held_meta_off`. The mesh is
//! the item's model from item_render.rs, a child of the model's root, lit as vanilla lights
//! entities with the lightmap at the player's eyes and fogged (the item material's world mode).
//! DROPPED: the main/off hand choice by `getPrimaryHand()` (every model is right handed),
//! `ModelPlayer.smallArms`, the child scale, `ArmPose.BOW_AND_ARROW` and `BLOCK` (no item is used
//! over time here), and the enchantment glint.

use items::ItemStack;

use super::*;
use crate::hand::standard_lights;
use crate::item::model_key;
use crate::item_render::{ItemMeshes, ItemMaterials};
use crate::material::ItemMaterial;

/// What `PlayerState` carries for the held stack: the block state it places (0 for none), the
/// item id and the metadata.
pub fn held_fields(stack: &ItemStack) -> (u16, u16, u16) {
    if stack.is_empty() {
        return (0, 0, 0);
    }
    (crate::hotbar::held_state(stack), stack.get_item(), stack.get_metadata().clamp(0, u16::MAX as i32) as u16)
}

/// The stack a state says is in the main hand.
fn held_stack(s: &PlayerState) -> ItemStack {
    if s.held_item != 0 {
        ItemStack::new(s.held_item, 1, s.held_meta as i32)
    } else if s.held != 0 {
        crate::hotbar::block_stack(s.held >> 4, s.held & 15)
    } else {
        ItemStack::EMPTY
    }
}

/// The stack a state says is in the off hand.
fn held_stack_off(s: &PlayerState) -> ItemStack {
    if s.held_item_off != 0 {
        ItemStack::new(s.held_item_off, 1, s.held_meta_off as i32)
    } else {
        ItemStack::EMPTY
    }
}

/// The two held item meshes and their materials, on a model's root.
#[derive(Component)]
pub struct HeldModel {
    entity: Entity,
    material: Handle<ItemMaterial>,
    off_entity: Entity,
    off_material: Handle<ItemMaterial>,
}

/// The item's transform below the root (which carries the model's 1/16 scale):
/// `ModelPlayer.postRenderArm(0.0625, side)` (`ModelRenderer.postRender`: translate to the
/// pivot, rotate Z, Y, X), up 0.2 sneaking, `rotate(-90, x)`, `rotate(180, y)`,
/// `translate(+-1/16, 0.125, -0.625)`, then `RenderItem.renderItemModel`: the display transform
/// and `renderItem`'s `translate(-0.5)`. Scaled back up by 16 for the root's 1/16. `left` picks
/// the left arm and its negative x translation.
pub fn local_matrix(arm: (Vec3, Vec3), sneaking: bool, left: bool, display: Mat4) -> Mat4 {
    let (pivot, angles) = arm;
    let x = if left { -1.0 / 16.0 } else { 1.0 / 16.0 };
    let mut m = Mat4::from_scale(Vec3::splat(16.0))
        * Mat4::from_translation(pivot / 16.0)
        * Mat4::from_quat(Quat::from_euler(EulerRot::ZYX, angles.z, angles.y, angles.x));
    if sneaking {
        m *= Mat4::from_translation(Vec3::new(0.0, 0.2, 0.0));
    }
    m * Mat4::from_rotation_x((-90f32).to_radians())
        * Mat4::from_rotation_y(180f32.to_radians())
        * Mat4::from_translation(Vec3::new(x, 0.125, -0.625))
        * display
        * Mat4::from_translation(Vec3::splat(-0.5))
}

/// Gives each model its held item mesh once, then shows, places and lights it every frame.
/// Runs after `pose_others`, which sets the root and the arm's pose and light for this frame.
#[allow(clippy::too_many_arguments)]
pub fn pose_held(
    mut commands: Commands,
    clock: Res<Clock>,
    items: Option<Res<ItemMeshes>>,
    item_mats: Option<Res<ItemMaterials>>,
    mut item_assets: ResMut<Assets<ItemMaterial>>,
    entity_mats: Res<Assets<EntityMaterial>>,
    players: Query<(Entity, &Other, Option<&HeldModel>)>,
    mut meshes: Query<(&mut Mesh3d, &mut Transform, &mut Visibility)>,
) {
    let (Some(items), Some(item_mats)) = (items, item_mats) else { return };
    for (root, o, held) in &players {
        let Some(held) = held else {
            let Some(mut m) = item_assets.get(&item_mats.hand).cloned() else { continue };
            let [l0, l1] = standard_lights();
            m.item.w0 = l0.extend(0.0);
            m.item.w1 = l1.extend(0.0);
            m.item.flags = UVec4::new(1, 0, 0, 1);
            let material = item_assets.add(m.clone());
            let off_material = item_assets.add(m);
            let entity = commands
                .spawn((Mesh3d::default(), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster, ChildOf(root)))
                .id();
            let off_entity = commands
                .spawn((Mesh3d::default(), MeshMaterial3d(off_material.clone()), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster, ChildOf(root)))
                .id();
            commands.entity(root).insert(HeldModel { entity, material, off_entity, off_material });
            continue;
        };
        let poses = pose(o, clock.partial);
        let sneak = o.sneaking();
        let main = model_key(&held_stack(&o.target)).and_then(|key| items.0.get(&key));
        let off = model_key(&held_stack_off(&o.target)).and_then(|key| items.0.get(&key));
        // `LayerHeldItem.doRenderLayer`: the right hand item under `THIRD_PERSON_RIGHT_HAND` on the
        // right arm, then the left hand item under `THIRD_PERSON_LEFT_HAND` on the left arm.
        let hands = [(held.entity, main, poses[2], false, held.material.clone()), (held.off_entity, off, poses[3], true, held.off_material.clone())];
        for (entity, item, arm, left, material) in hands {
            let Ok((mut mesh, mut transform, mut visibility)) = meshes.get_mut(entity) else { continue };
            let Some(item) = item else {
                if *visibility != Visibility::Hidden {
                    *visibility = Visibility::Hidden;
                }
                continue;
            };
            if mesh.0 != item.mesh {
                mesh.0 = item.mesh.clone();
            }
            let display = if left { item.third_person_left } else { item.third_person };
            *transform = Transform::from_matrix(local_matrix(arm, sneak, left, display));
            if *visibility != Visibility::Inherited {
                *visibility = Visibility::Inherited;
            }
            // The lightmap coordinates pose_others found at the eyes.
            if let Some(light) = entity_mats.get(&o.material).map(|m| m.light) {
                if item_assets.get(&material).is_some_and(|m| m.item.w0.w != light.x || m.item.w1.w != light.y) {
                    if let Some(mut m) = item_assets.get_mut(&material) {
                        m.item.w0.w = light.x;
                        m.item.w1.w = light.y;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::ids::item;
    use worldgen::blocks::id;

    #[test]
    fn fields_round_trip_through_the_state() {
        for stack in [ItemStack::new(item::DIAMOND_PICKAXE, 1, 12), ItemStack::new(item::DYE, 1, 4), crate::hotbar::block_stack(id::STONE, 2)] {
            let (held, held_item, held_meta) = held_fields(&stack);
            let s = PlayerState { held, held_item, held_meta, ..default() };
            let back = held_stack(&s);
            assert_eq!((back.get_item(), back.get_metadata()), (stack.get_item(), stack.get_metadata()));
        }
        assert_eq!(held_fields(&ItemStack::EMPTY), (0, 0, 0));
        assert!(held_stack(&PlayerState::default()).is_empty());
        // A state from an older build: the block state alone.
        let s = PlayerState { held: id::STONE << 4 | 2, ..default() };
        assert_eq!((held_stack(&s).get_item(), held_stack(&s).get_metadata()), (crate::hotbar::block_stack(id::STONE, 2).get_item(), 2));
    }

    /// Whatever a player holds reaches the other viewers as `PlayerState`'s held
    /// fields and resolves to a drawn item model, for every registered item and every block the
    /// hotbar can place.
    /// DROPPED: the `builtin/entity` items (chests, banners, beds, shulker boxes, skulls, the
    /// shield) and items whose model file is missing, which `item::load_sources` skips too.
    #[test]
    fn every_held_item_resolves_to_a_model() {
        let Ok(jar) = crate::jar::Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = crate::models::Models::load(&jar).expect("models");
        let mut missing = Vec::new();
        for (it, meta) in crate::item::item_keys().into_iter().chain(crate::item::block_keys()) {
            let stack = ItemStack::new(it, 1, meta as i32);
            let (held, held_item, held_meta) = held_fields(&stack);
            let back = held_stack(&PlayerState { held, held_item, held_meta, ..default() });
            if (back.get_item(), back.get_metadata()) != (stack.get_item(), stack.get_metadata()) {
                missing.push(format!("{it}:{meta} changed on the wire"));
                continue;
            }
            if model_key(&back).and_then(|k| models.items.get(&k)).is_none() {
                missing.push(format!("{it}:{meta}"));
            }
        }
        eprintln!("HELDMISSING {} {missing:?}", missing.len());
        // Chests (54, 130, 146), banner, bed, skulls, shield and shulker boxes are builtin/entity
        // models; 44:2, 83, 204 and 219 to 234 are not obtainable as held items.
        let allowed = |k: &str| {
            let it: u16 = k.split(':').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            matches!(it, 44 | 54 | 83 | 130 | 146 | 204 | 219..=234 | 355 | 397 | 425 | 442)
        };
        let unexpected: Vec<_> = missing.iter().filter(|k| !allowed(k)).collect();
        assert!(unexpected.is_empty(), "{unexpected:?}");
    }

    #[test]
    fn a_block_sits_in_front_of_the_hand() {
        // block/block's thirdperson_righthand.
        let d = crate::item::Display { rotation: [75.0, 45.0, 0.0], translation: [0.0, 2.5 / 16.0, 0.0], scale: [0.375; 3] };
        let mut o = Other::new(None, [Entity::PLACEHOLDER; 6], Handle::default());
        o.set_target(PlayerState { pos: [0.0, 64.0, 0.0], held: 1 << 4, ..default() });
        let root = body_matrix(&o, 1.0) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
        let world = root * local_matrix(pose(&o, 1.0)[2], false, false, d.matrix());
        let c = world.transform_point3(Vec3::splat(0.5));
        // Facing south (yaw 0), the right hand is on the west side (-x), the block ahead of the
        // body (+z), about hip height.
        assert!(c.x < -0.2 && c.x > -0.6, "{c}");
        assert!(c.z > 0.0 && c.z < 0.8, "{c}");
        assert!(c.y > 64.4 && c.y < 65.2, "{c}");
        assert!(world.determinant() > 0.0, "keeps the winding");
    }
}
