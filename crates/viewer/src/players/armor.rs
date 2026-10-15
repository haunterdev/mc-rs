//! `LayerBipedArmor` for the player models (other players, and the player's own in third
//! person): each worn piece is a second, bigger `ModelBiped` drawn over the body parts with the
//! jar's `textures/models/armor/<material>_layer_1.png` (helmet, chestplate, boots, a
//! `ModelBiped(1.0)`) or `_layer_2.png` (leggings, a `ModelBiped(0.5)`), on a 64 x 32 texture.
//!
//! Which pieces a player wears is what `S_EQUIPMENT` says for slots 2 to 5 (feet to head); the
//! player's own model reads its inventory. The boxes are children of the model's part entities,
//! so they take the pose of the body part they cover. `setModelSlotVisible`: the head piece shows
//! the head, the chestplate the body and both arms, the leggings the body and both legs, the
//! boots both legs.
//!
//! DROPPED: the leather dye (`ItemArmor.getColor` is NBT the wire stack does not carry, and
//! `leather_layer_1_overlay.png`), the enchantment glint (`renderEnchantedGlint`), the elytra and
//! pumpkin head layers, and armor on mobs (the server equips none, and the mob table in
//! `survival/mobs` has no armor slots).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use items::{ArmorMaterial, ArmorSlot, ItemStack, Kind};

use super::{InventoryModel, Other, OwnModel, Others, Skin};
use crate::hand::BoxMesh;
use crate::hotbar::Inventory;
use crate::material::EntityMaterial;
use crate::survival::Equipment;

/// The materials in texture order.
pub const MATERIALS: [ArmorMaterial; 5] = [ArmorMaterial::Leather, ArmorMaterial::Chain, ArmorMaterial::Iron, ArmorMaterial::Gold, ArmorMaterial::Diamond];

/// `ItemArmor.getArmorMaterial().getName()` and the layer: `LayerBipedArmor.getArmorResource`.
pub fn texture_path(material: ArmorMaterial, legs: bool) -> String {
    format!("textures/models/armor/{}_layer_{}.png", material.name(), if legs { 2 } else { 1 })
}

/// The jar images of [`MATERIALS`], `material * 2 + (layer 2 as 1)`, loaded at startup.
#[derive(Resource)]
pub struct ArmorImages(pub Vec<Handle<Image>>);

/// The model part boxes in `ModelBiped` order (head, body, right arm, left arm, right leg, left
/// leg): texture offset, box origin, size, mirrored.
const BOXES: [((i32, i32), [f32; 3], [i32; 3], bool); 6] = [
    ((0, 0), [-4.0, -8.0, -4.0], [8, 8, 8], false),
    ((16, 16), [-4.0, 0.0, -2.0], [8, 12, 4], false),
    ((40, 16), [-3.0, -2.0, -2.0], [4, 12, 4], false),
    ((40, 16), [-1.0, -2.0, -2.0], [4, 12, 4], true),
    ((0, 16), [-2.0, 0.0, -2.0], [4, 12, 4], false),
    ((0, 16), [-2.0, 0.0, -2.0], [4, 12, 4], true),
];

/// Which armor slots (`ArmorSlot::index`: feet 0, legs 1, chest 2, head 3) cover each part, and
/// whether that slot's model is the 0.5 leggings one.
const COVER: [&[(usize, bool)]; 6] = [&[(3, false)], &[(2, false), (1, true)], &[(2, false)], &[(2, false)], &[(1, true), (0, false)], &[(1, true), (0, false)]];

/// `new ModelBiped(delta, 0, 64, 32)`'s box for `part`.
pub fn part_box(part: usize, delta: f32) -> BoxMesh {
    let ((u, v), [x, y, z], [dx, dy, dz], mirror) = BOXES[part];
    let mut b = BoxMesh::default();
    b.model_box_mirrored(64.0, 32.0, u, v, x, y, z, dx, dy, dz, delta, mirror);
    b
}

fn mesh_of(b: BoxMesh) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
        .with_inserted_indices(Indices::U32(b.idx))
}

/// The armor slot of an `S_EQUIPMENT` slot (`EQ_FEET` to `EQ_HEAD`).
fn armor_slot(eq_slot: u8) -> Option<usize> {
    (proto::EQ_FEET..=proto::EQ_HEAD).contains(&eq_slot).then(|| (eq_slot - proto::EQ_FEET) as usize)
}

/// The texture image index a piece in `slot` shows, if the item is armor for that slot
/// (`LayerBipedArmor.renderArmorLayer`: `instanceof ItemArmor` and `getEquipmentSlot() == slot`).
pub fn image_index(item: u16, slot: usize) -> Option<usize> {
    if item == 0 {
        return None;
    }
    let Kind::Armor(material, aslot) = ItemStack::new(item, 1, 0).def().kind else { return None };
    if aslot.index() != slot {
        return None;
    }
    let m = MATERIALS.iter().position(|&x| x == material)?;
    Some(m * 2 + (aslot == ArmorSlot::Legs) as usize)
}

/// What a model wears and the boxes that show it.
#[derive(Component)]
pub struct Worn {
    /// The item id in each armor slot, 0 for none.
    items: [u16; 4],
    /// What the boxes show now.
    shown: [u16; 4],
    /// One material per slot, its texture the piece's.
    mats: [Handle<EntityMaterial>; 4],
    /// Every box entity and the slot it belongs to.
    boxes: Vec<(Entity, usize)>,
}

/// Gives every new player model its armor boxes, takes the server's equipment frames and the
/// inventory for the player's own model, and shows what changed.
#[allow(clippy::too_many_arguments)]
pub fn update_armor(
    mut commands: Commands,
    images: Option<Res<ArmorImages>>,
    skin: Res<Skin>,
    others: Res<Others>,
    inv: Option<Res<Inventory>>,
    mut equipment: MessageReader<Equipment>,
    mut said: Local<HashMap<u32, [u16; 4]>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut cache: Local<Option<[[Handle<Mesh>; 6]; 2]>>,
    fresh: Query<(Entity, &Other), Without<Worn>>,
    mut models: Query<(Entity, &Other, &mut Worn, Has<OwnModel>, Has<InventoryModel>)>,
    mut visibility: Query<&mut Visibility>,
) {
    let Some(images) = images else { return };
    for Equipment(e) in equipment.read() {
        if let Some(slot) = armor_slot(e.slot) {
            said.entry(e.id).or_default()[slot] = if e.stack.id > 0 { e.stack.id as u16 } else { 0 };
        }
    }
    let [full, legs] = cache.get_or_insert_with(|| {
        let mut make = |delta: f32| std::array::from_fn(|i| meshes.add(mesh_of(part_box(i, delta))));
        [make(1.0), make(0.5)]
    });
    // ModelBiped(1.0) for every slot but the leggings, whose model is the 0.5 one.
    let (full, legs) = (full.clone(), legs.clone());
    for (root, o) in &fresh {
        let mats: [Handle<EntityMaterial>; 4] = std::array::from_fn(|_| materials.add(EntityMaterial::new(skin.env.clone(), skin.skin.clone())));
        let mut boxes = Vec::new();
        for (part, cover) in COVER.iter().enumerate() {
            for &(slot, is_legs) in *cover {
                let mesh = if is_legs { legs[part].clone() } else { full[part].clone() };
                let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mats[slot].clone()), Transform::default(), Visibility::Hidden, ChildOf(o.parts[part]))).id();
                boxes.push((e, slot));
            }
        }
        commands.entity(root).insert(Worn { items: [0; 4], shown: [0; 4], mats, boxes });
    }
    let own_items = inv.as_deref().map(|i| std::array::from_fn::<u16, 4, _>(|s| if i.armor[s].is_empty() { 0 } else { i.armor[s].get_item() }));
    let ids: HashMap<Entity, u32> = others.all().map(|(id, e)| (e, id)).collect();
    for (e, o, mut w, own, preview) in &mut models {
        let own = own || preview;
        let want = if own { own_items.unwrap_or([0; 4]) } else { ids.get(&e).and_then(|id| said.get(id)).copied().unwrap_or([0; 4]) };
        if w.items != want {
            w.items = want;
        }
        // getBrightnessForRender: the model's own light.
        if let Some(light) = materials.get(&o.material).map(|m| m.light) {
            for h in &w.mats {
                if materials.get(h).is_some_and(|m| m.light != light) {
                    if let Some(mut m) = materials.get_mut(h) {
                        m.light = light;
                    }
                }
            }
        }
        if w.shown == w.items {
            continue;
        }
        w.shown = w.items;
        for slot in 0..4 {
            if let Some(i) = image_index(w.items[slot], slot) {
                if let Some(mut m) = materials.get_mut(&w.mats[slot]) {
                    m.skin = images.0[i].clone();
                }
            }
        }
        for &(b, slot) in &w.boxes {
            if let Ok(mut v) = visibility.get_mut(b) {
                *v = if image_index(w.items[slot], slot).is_some() { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(b: &BoxMesh) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &b.pos {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    #[test]
    fn the_helmet_box_is_the_head_grown_by_one_and_the_leggings_by_a_half() {
        let (lo, hi) = bounds(&part_box(0, 1.0));
        assert_eq!((lo, hi), ([-5.0, -9.0, -5.0], [5.0, 1.0, 5.0]));
        let (lo, hi) = bounds(&part_box(1, 0.5));
        assert_eq!((lo, hi), ([-4.5, -0.5, -2.5], [4.5, 12.5, 2.5]));
    }

    #[test]
    fn the_armor_texture_is_sixty_four_by_thirty_two() {
        for part in 0..6 {
            let b = part_box(part, 1.0);
            assert_eq!(b.pos.len(), 24, "six quads");
            assert!(b.uv.iter().all(|uv| (0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1])), "part {part} stays on the sheet");
        }
        // The head's top: u 8 to 16 over 64, v 0 to 8 over 32.
        let b = part_box(0, 0.0);
        assert!(b.uv.iter().any(|uv| (uv[0] - 16.0 / 64.0).abs() < 1e-6 && (uv[1] - 8.0 / 32.0).abs() < 1e-6));
    }

    #[test]
    fn the_left_limbs_are_mirrored_boxes_on_the_right_ones_uvs() {
        let (r, l) = (part_box(2, 1.0), part_box(3, 1.0));
        let (rlo, rhi) = bounds(&r);
        let (llo, lhi) = bounds(&l);
        assert_eq!((rlo[0], rhi[0]), (-4.0, 2.0));
        assert_eq!((llo[0], lhi[0]), (-2.0, 4.0));
        let mut ru: Vec<_> = r.uv.iter().map(|u| (u[0].to_bits(), u[1].to_bits())).collect();
        let mut lu: Vec<_> = l.uv.iter().map(|u| (u[0].to_bits(), u[1].to_bits())).collect();
        ru.sort();
        lu.sort();
        assert_eq!(ru, lu);
    }

    #[test]
    fn the_jar_has_both_layers_of_every_material() {
        let Ok(jar) = jar::Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        for m in MATERIALS {
            for legs in [false, true] {
                let path = texture_path(m, legs);
                let png = jar.asset(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
                let img = crate::textures::decode_png(&png).unwrap();
                assert_eq!((img.width, img.height), (64, 32), "{path}");
            }
        }
    }

    #[test]
    fn each_slot_covers_the_parts_vanilla_shows() {
        let covered = |slot: usize| (0..6).filter(|&p| COVER[p].iter().any(|&(s, _)| s == slot)).collect::<Vec<_>>();
        assert_eq!(covered(3), vec![0]);
        assert_eq!(covered(2), vec![1, 2, 3]);
        assert_eq!(covered(1), vec![1, 4, 5]);
        assert_eq!(covered(0), vec![4, 5]);
    }

    #[test]
    fn only_a_piece_for_its_own_slot_picks_a_texture() {
        let iron_helmet = items::ids::item::IRON_HELMET;
        let iron_leggings = items::ids::item::IRON_LEGGINGS;
        assert_eq!(image_index(iron_helmet, 3), Some(2 * 2));
        assert_eq!(image_index(iron_helmet, 2), None, "a helmet in the chest slot is not drawn");
        assert_eq!(image_index(iron_leggings, 1), Some(2 * 2 + 1), "leggings use layer 2");
        assert_eq!(image_index(0, 3), None);
        assert_eq!(image_index(items::ids::item::BOW, 3), None);
        assert_eq!(texture_path(ArmorMaterial::Chain, true), "textures/models/armor/chainmail_layer_2.png");
    }
}
