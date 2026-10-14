//! Item models: the model each stack shows (`RenderItem.registerItems` through
//! `ItemModelMesher`), the flat models `ItemModelGenerator` makes from layer sprites
//! (`generator.rs`), the display transforms (`ItemTransformVec3f`), and `ItemColors` for stacks
//! with no NBT.
//!
//! Every registered item gets a model: the block half of the table is `blocks.rs`, the item half
//! and the mesh definitions `item_table.rs`.
//!
//! DROPPED: item model `overrides` (the bow pulling, the clock and compass hands, the fishing
//! rod cast, the broken elytra, the blocking shield); each stack shows its base model.
//! DROPPED: the enchantment glint (`hasEffect`: enchanted books, the enchanted golden apple,
//! potions); there is no NBT to enchant a stack with and no glint pass.

mod blocks;
mod generator;
mod item_table;

use std::collections::HashMap;
use std::io;
use std::sync::OnceLock;

use bevy::math::{Mat4, Quat, Vec3};
use items::ids::item;
use items::ItemStack;
use serde_json::Value;
use worldgen::blocks::id;
use worldgen::math_helper::{cos, sin};

use crate::colors::Colors;
use crate::jar::Jar;
use crate::models::{bake_model, bake_quad, vec3, ModelBlock, ModelRotation, Quad};
use crate::textures::BlockTextures;
use generator::{generated_parts, LAYERS};

/// `ItemTransformVec3f`: one entry of a model's `display` block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub rotation: [f32; 3],
    pub translation: [f32; 3],
    pub scale: [f32; 3],
}

impl Display {
    /// `ItemTransformVec3f.DEFAULT`
    pub const DEFAULT: Display = Display { rotation: [0.0; 3], translation: [0.0; 3], scale: [1.0; 3] };

    /// `ItemTransformVec3f.Deserializer.deserialize`
    pub fn parse(v: &Value) -> Display {
        let get = |key: &str, default: [f32; 3]| v.get(key).and_then(vec3).unwrap_or(default);
        let translation = get("translation", [0.0; 3]).map(|c| (c * 0.0625).clamp(-5.0, 5.0));
        let scale = get("scale", [1.0; 3]).map(|c| c.clamp(-4.0, 4.0));
        Display { rotation: get("rotation", [0.0; 3]), translation, scale }
    }

    /// `ItemTransformVec3f.apply` for the right hand: translate, rotate, scale.
    pub fn matrix(&self) -> Mat4 {
        if *self == Display::DEFAULT {
            return Mat4::IDENTITY;
        }
        let [x, y, z] = self.rotation;
        Mat4::from_translation(Vec3::from_array(self.translation))
            * Mat4::from_quat(make_quaternion(x, y, z))
            * Mat4::from_scale(Vec3::from_array(self.scale))
    }
}

/// `ItemTransformVec3f.makeQuaternion`: degrees about x, then y, then z.
pub fn make_quaternion(x: f32, y: f32, z: f32) -> Quat {
    let (x, y, z) = (x * 0.017_453_292, y * 0.017_453_292, z * 0.017_453_292);
    let (f3, f4) = (sin(0.5 * x), cos(0.5 * x));
    let (f5, f6) = (sin(0.5 * y), cos(0.5 * y));
    let (f7, f8) = (sin(0.5 * z), cos(0.5 * z));
    Quat::from_xyzw(
        f3 * f6 * f8 + f4 * f5 * f7,
        f4 * f5 * f8 - f3 * f6 * f7,
        f3 * f5 * f8 + f4 * f6 * f7,
        f4 * f6 * f8 - f3 * f5 * f7,
    )
}

/// `ItemModelMesher.simpleShapes`: (item id, metadata) to model name, filled in
/// `registerItems` order (a later `register` of the same key replaces the earlier one).
fn registered() -> &'static HashMap<(u16, u16), &'static str> {
    static MAP: OnceLock<HashMap<(u16, u16), &'static str>> = OnceLock::new();
    MAP.get_or_init(|| blocks::BLOCK_ITEMS.iter().chain(item_table::ITEMS).map(|&(i, m, n)| ((i, m), n)).collect())
}

fn definition(item: u16) -> Option<&'static str> {
    item_table::DEFINITIONS.iter().find(|d| d.0 == item).map(|d| d.1)
}

/// The item model `ItemModelMesher.getItemModel` finds for an item and model metadata
/// ([`model_key`]): the `simpleShapes` entry, else the item's `ItemMeshDefinition`. The model is
/// `item/<name>`. `None` where vanilla shows the missing model.
pub fn item_model_name(item: u16, meta: u16) -> Option<&'static str> {
    registered().get(&(item, meta)).copied().or_else(|| definition(item))
}

/// The key a stack's model is kept under: the item and `ItemModelMesher.getMetadata(stack)` (0
/// for a damageable item). An item with an `ItemMeshDefinition` has one model, kept at meta 0.
pub fn model_key(stack: &ItemStack) -> Option<(u16, u16)> {
    if stack.is_empty() {
        return None;
    }
    let it = stack.get_item();
    if definition(it).is_some() {
        return Some((it, 0));
    }
    let meta = if stack.get_max_damage() > 0 { 0 } else { stack.get_metadata() };
    u16::try_from(meta).ok().map(|m| (it, m))
}

/// Every key `registerItems` gives a model: each registered (item, meta) and each mesh
/// definition's item at meta 0.
pub fn item_keys() -> Vec<(u16, u16)> {
    let mut keys: Vec<(u16, u16)> = registered().keys().copied().chain(item_table::DEFINITIONS.iter().map(|d| (d.0, 0))).collect();
    keys.sort_unstable();
    keys
}

/// The block keys the creative hotbar reaches: the start set, and what the middle click picks
/// from each generated block. A double plant's upper half reads the half below it, so only lower
/// halves are asked. The petrified oak slab (stone slab meta 2) has no item model in the jar.
#[cfg(test)]
pub fn block_keys() -> Vec<(u16, u16)> {
    use crate::blockstates::generated_states;
    let mut keys: Vec<(u16, u16)> = crate::hotbar::START.iter().map(|&b| (b, 0)).collect();
    for s in generated_states() {
        if s >> 4 == id::DOUBLE_PLANT && s & 8 != 0 {
            continue;
        }
        keys.extend(crate::interact::picked_item(s));
    }
    keys.retain(|&k| k != (id::STONE_SLAB, 2));
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// `ModelBakery.loadItemModels` for [`item_keys`].
/// DROPPED: `builtin/entity` models (chests, shulker boxes, banners, beds, the shield, skulls),
/// drawn by `TileEntityItemStackRenderer`; and a name with no model file (`old_wood_slab`),
/// where vanilla shows the missing model. Both draw nothing.
pub fn load_sources(jar: &Jar) -> io::Result<Vec<((u16, u16), ModelBlock)>> {
    let mut out = Vec::new();
    for key in item_keys() {
        let Some(name) = item_model_name(key.0, key.1) else { continue };
        if !jar.contains(&format!("assets/minecraft/models/item/{name}.json")) {
            continue;
        }
        let model = ModelBlock::load(jar, &format!("item/{name}"))?;
        if model.builtin.as_deref() == Some("builtin/entity") {
            continue;
        }
        out.push((key, model));
    }
    Ok(out)
}

fn generated(model: &ModelBlock) -> bool {
    model.builtin.as_deref() == Some("builtin/generated")
}

/// `ItemModelGenerator.makeItemModel`'s layer textures: `layer0` up to the first one missing.
fn layer_textures(model: &ModelBlock) -> Vec<String> {
    (0..LAYERS).map(|i| format!("layer{i}")).take_while(|k| model.is_texture_present(k)).map(|k| model.resolve_texture(&k)).collect()
}

/// The sprites the item models need, for the block texture array.
pub fn sprite_names(sources: &[((u16, u16), ModelBlock)]) -> Vec<String> {
    let mut names = Vec::new();
    for (_, m) in sources {
        if generated(m) {
            names.extend(layer_textures(m));
        } else {
            names.extend(m.elements().iter().flat_map(|p| p.faces.iter().map(|(_, f)| m.resolve_texture(&f.texture))));
        }
    }
    names
}

/// A baked item: `RenderItem.renderModel`'s quads in its order (each side's, then the general
/// ones), and the transforms `ItemCameraTransforms` keeps for the GUI and the right hand.
#[derive(Clone, Debug)]
pub struct ItemModel {
    pub quads: Vec<Quad>,
    pub gui: Display,
    pub first_person: Display,
    /// `IBakedModel.isGui3d`: block models are lit in the GUI, generated ones are not.
    pub gui3d: bool,
}

/// `ModelBakery.bakeItemModels`, with `ModelRotation.X0_Y0` and no uv lock. Generated models
/// (`item/generated`, `item/handheld` and their children) go through `ItemModelGenerator`.
pub fn bake_items(sources: &[((u16, u16), ModelBlock)], textures: &BlockTextures) -> io::Result<HashMap<(u16, u16), ItemModel>> {
    let info = |n: &str| textures.get(n).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("no sprite {n}")));
    let rot = ModelRotation::new(0, 0);
    let mut out = HashMap::new();
    for (key, m) in sources {
        // ModelBlock.getAllTransforms: the nearest model in the chain that sets each one.
        let gui = m.display("gui").unwrap_or(Display::DEFAULT);
        let first_person = m.display("firstperson_righthand").unwrap_or(Display::DEFAULT);
        let (quads, gui3d) = if generated(m) {
            let mut quads = Vec::new();
            for (layer, name) in layer_textures(m).iter().enumerate() {
                let s = info(name)?;
                let sprite = &textures.sprites.iter().find(|(n, _)| n == name).expect("indexed sprite is kept").1;
                for part in generated_parts(layer as i32, name, sprite) {
                    for (f, pf) in &part.faces {
                        quads.push(bake_quad(&part, *f, pf, s, &rot));
                    }
                }
            }
            (quads, false)
        } else {
            let b = bake_model(m, &rot, &info)?;
            (b.face_quads.into_iter().flatten().chain(b.general).collect(), true)
        };
        out.insert(*key, ItemModel { quads, gui, first_person, gui3d });
    }
    Ok(out)
}

/// `ItemColors.colorMultiplier` for a stack with no NBT, as RGB; white where vanilla returns -1.
/// The handlers for grass, tall grass, vines, leaves and lily pads ask `BlockColors` with no
/// world, which takes their default colors. The NBT colors (dyed leather, potion effects,
/// firework charge colors, spawn egg mobs, map colors) are DROPPED with NBT: each stack takes
/// the color its handler gives an untagged stack.
pub fn item_color(it: u16, meta: u16, tint: i32, colors: &Colors) -> u32 {
    // RenderItem.renderQuads: only quads with a tint index ask ItemColors.
    if tint < 0 {
        return 0xFF_FFFF;
    }
    let grass = || colors.grass_colorizer(0.5, 1.0) as u32 & 0xFF_FFFF;
    let c: i32 = match it {
        // ItemArmor.getColor: the leather default.
        item::LEATHER_HELMET..=item::LEATHER_BOOTS => {
            if tint > 0 {
                -1
            } else {
                10_511_680
            }
        }
        // ItemColors: only the grass and fern types are tinted.
        id::DOUBLE_PLANT => {
            if meta == 2 || meta == 3 {
                grass() as i32
            } else {
                -1
            }
        }
        item::FIREWORK_CHARGE => {
            if tint != 1 {
                -1
            } else {
                9_079_434
            }
        }
        // PotionUtils.getColor: PotionTypes.EMPTY.
        item::POTIONITEM | item::SPLASH_POTION | item::LINGERING_POTION => {
            if tint > 0 {
                -1
            } else {
                16_253_176
            }
        }
        item::TIPPED_ARROW => {
            if tint == 0 {
                16_253_176
            } else {
                -1
            }
        }
        // ItemMap.getColor with no display tag.
        item::FILLED_MAP => {
            if tint == 0 {
                -1
            } else {
                -12_173_266
            }
        }
        // EntityList.ENTITY_EGGS has no entry for an egg with no entity tag.
        item::SPAWN_EGG => -1,
        id::GRASS | id::TALLGRASS => grass() as i32,
        // ColorizerFoliage.getFoliageColorPine, getFoliageColorBirch, getFoliageColorBasic.
        id::LEAVES => match meta & 3 {
            1 => 6_396_257,
            2 => 8_431_445,
            _ => 4_764_952,
        },
        id::LEAVES2 | id::VINE => 4_764_952,
        id::WATERLILY => 7_455_580,
        _ => -1,
    };
    c as u32 & 0xFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::{registry, ItemDef, Kind, Names};

    #[test]
    fn quaternion_is_x_then_y_then_z() {
        let (x, y, z) = (30.0f32, 225.0f32, -12.5f32);
        let q = make_quaternion(x, y, z);
        let r = Quat::from_rotation_x(x.to_radians()) * Quat::from_rotation_y(y.to_radians()) * Quat::from_rotation_z(z.to_radians());
        assert!(q.dot(r).abs() > 0.99999, "{q:?} vs {r:?}");
    }

    #[test]
    fn display_parses_and_clamps() {
        let v: Value = serde_json::from_str(r#"{"rotation":[0,45,0],"translation":[0,160,-1],"scale":[0.625,9,0.625]}"#).unwrap();
        let d = Display::parse(&v);
        assert_eq!(d.rotation, [0.0, 45.0, 0.0]);
        assert_eq!(d.translation, [0.0, 5.0, -0.0625]);
        assert_eq!(d.scale, [0.625, 4.0, 0.625]);
        assert_eq!(Display::parse(&serde_json::json!({})), Display::DEFAULT);
    }

    #[test]
    fn names_follow_register_items() {
        assert_eq!(item_model_name(id::STONE, 2), Some("granite_smooth"));
        assert_eq!(item_model_name(id::LOG2, 1), Some("dark_oak_log"));
        assert_eq!(item_model_name(id::LEAVES, 4), None);
        assert_eq!(item_model_name(id::STAINED_HARDENED_CLAY, 8), Some("silver_stained_hardened_clay"));
        assert_eq!(item_model_name(id::GLASS, 1), None);
        assert_eq!(item_model_name(item::DYE, 0), Some("dye_black"));
        assert_eq!(item_model_name(item::DYE, 15), Some("dye_white"));
        assert_eq!(item_model_name(item::COAL, 1), Some("charcoal"));
        assert_eq!(item_model_name(item::FISH, 3), Some("pufferfish"));
        assert_eq!(item_model_name(item::COOKED_FISH, 2), None);
        assert_eq!(item_model_name(item::GOLDEN_APPLE, 1), Some("golden_apple"));
        assert_eq!(item_model_name(item::SKULL, 4), Some("skull_creeper"));
        assert_eq!(item_model_name(item::POTIONITEM, 0), Some("bottle_drinkable"));
        assert_eq!(item_model_name(item::SPAWN_EGG, 90), Some("spawn_egg"));
        assert_eq!(item_model_name(item::BOAT, 0), Some("oak_boat"));
        assert_eq!(item_model_name(item::TOTEM_OF_UNDYING, 0), Some("totem"));
        assert_eq!(item_model_name(items::ids::block::CONCRETE, 3), Some("light_blue_concrete"));
        for b in crate::hotbar::START {
            assert!(item_model_name(b, 0).is_some());
        }
    }

    /// A damaged tool shows the undamaged model; a mesh definition item shows one model.
    #[test]
    fn model_key_follows_item_model_mesher() {
        assert_eq!(model_key(&ItemStack::new(item::DIAMOND_PICKAXE, 1, 37)), Some((item::DIAMOND_PICKAXE, 0)));
        assert_eq!(model_key(&ItemStack::new(item::DYE, 1, 4)), Some((item::DYE, 4)));
        assert_eq!(model_key(&ItemStack::new(item::SPAWN_EGG, 1, 50)), Some((item::SPAWN_EGG, 0)));
        assert_eq!(model_key(&ItemStack::new(item::REEDS, 1, 0)), Some((item::REEDS, 0)));
        assert_eq!(model_key(&ItemStack::new(item::STICK, 0, 0)), None);
    }

    /// No key is registered twice under two names, so splitting the table by half keeps the
    /// lookup `registerItems` order gives.
    #[test]
    fn no_key_has_two_names() {
        let mut seen: HashMap<(u16, u16), &str> = HashMap::new();
        for &(i, m, n) in blocks::BLOCK_ITEMS.iter().chain(item_table::ITEMS) {
            if let Some(old) = seen.insert((i, m), n) {
                assert_eq!(old, n, "{i}:{m}");
            }
        }
    }

    #[test]
    fn item_colors_without_nbt() {
        let c = Colors::from_buffers(vec![0; 65536], vec![0; 65536]);
        assert_eq!(item_color(item::LEATHER_CHESTPLATE, 0, 0, &c), 0xA0_6540);
        assert_eq!(item_color(item::LEATHER_CHESTPLATE, 0, 1, &c), 0xFF_FFFF);
        assert_eq!(item_color(item::POTIONITEM, 0, 0, &c), 0xF8_00F8);
        assert_eq!(item_color(item::FILLED_MAP, 0, 1, &c), 0x46_402E);
        assert_eq!(item_color(item::SPAWN_EGG, 0, 0, &c), 0xFF_FFFF);
        assert_eq!(item_color(id::LEAVES, 2, 0, &c), 8_431_445);
        assert_eq!(item_color(item::STICK, 0, -1, &c), 0xFF_FFFF);
    }

    /// The subtypes the registry knows an item has. A `Multi` or `Colored` name table can be
    /// longer than the item's subtypes (the log reads `BlockPlanks.EnumType`, six woods for four
    /// logs), so those items are asked at meta 0 only; `registerItems` names their subtypes, and
    /// [`item_keys`] covers them.
    fn registry_metas(d: &ItemDef) -> std::ops::Range<i32> {
        let n = match d.kind {
            Kind::Block { names: Names::Cloth, .. } => 16,
            Kind::Block { names: Names::Log2 | Names::Sponge | Names::Leaves { new: true }, .. } => 2,
            Kind::Block { names: Names::Leaves { new: false }, .. } => 4,
            Kind::Coal => 2,
            Kind::Dye | Kind::Bed => 16,
            Kind::Fish { cooked: false } => 4,
            Kind::Fish { cooked: true } => 2,
            Kind::Skull => 6,
            _ => 1,
        };
        0..n as i32
    }

    /// Every registered item and named subtype has a model name, and every model the jar holds
    /// bakes; the only keys left without one are the `builtin/entity` models and the names
    /// `registerItems` gives blocks with no obtainable item, which the jar has no model for.
    #[test]
    fn every_registered_item_bakes_from_the_jar() {
        let Ok(jar) = Jar::find() else { return };
        let models = crate::models::Models::load(&jar).unwrap();
        assert!(models.textures.layers <= crate::textures::MAX_LAYERS, "{} layers", models.textures.layers);
        let (mut missing, mut entity): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
        for key in item_keys() {
            if models.items.contains_key(&key) {
                continue;
            }
            let name = item_model_name(key.0, key.1).unwrap();
            if !jar.contains(&format!("assets/minecraft/models/item/{name}.json")) {
                missing.push(name);
                continue;
            }
            let m = ModelBlock::load(&jar, &format!("item/{name}")).unwrap();
            assert_eq!(m.builtin.as_deref(), Some("builtin/entity"), "{name} did not bake");
            entity.push(name);
        }
        for l in [&mut missing, &mut entity] {
            l.sort_unstable();
            l.dedup();
        }
        assert_eq!(missing, ["old_wood_slab", "purpur_double_slab"]);
        // TileEntityItemStackRenderer's set: 16 shulker boxes, 3 chests, 6 skulls, banner, bed, shield.
        assert_eq!(entity.len(), 28, "{entity:?}");
        let teisr = |n: &str| n.ends_with("shulker_box") || n.ends_with("chest") || n.starts_with("skull_") || ["banner", "bed", "shield"].contains(&n);
        assert!(entity.iter().all(|n| teisr(n)), "{entity:?}");
        let skipped: Vec<&str> = missing.iter().chain(&entity).copied().collect();
        for d in registry::all_items().iter().filter(|d| d.id != item::AIR) {
            for meta in registry_metas(d) {
                let key = model_key(&ItemStack::new(d.id, 1, meta)).unwrap();
                let name = item_model_name(key.0, key.1).unwrap_or_else(|| panic!("{} meta {meta} has no model", d.name));
                assert!(models.items.contains_key(&key) || skipped.contains(&name), "{} meta {meta}", d.name);
            }
        }
        let stone = &models.items[&(id::STONE, 0)];
        assert!(stone.gui3d);
        assert_eq!(stone.gui.rotation, [30.0, 225.0, 0.0]);
        let dandelion = &models.items[&(id::YELLOW_FLOWER, 0)];
        assert!(!dandelion.gui3d);
        assert!(dandelion.quads.len() > 2);
        for k in [item::STICK, item::DIAMOND_PICKAXE, item::APPLE] {
            let m = &models.items[&(k, 0)];
            assert!(!m.gui3d && m.quads.len() > 2, "{k}");
            assert_eq!(m.gui, Display::DEFAULT);
        }
        // item/handheld's right hand.
        assert_eq!(models.items[&(item::DIAMOND_PICKAXE, 0)].first_person.rotation, [0.0, -90.0, 25.0]);
        // Two layers: the spawn egg and its overlay, tint indices 0 and 1.
        let egg = &models.items[&(item::SPAWN_EGG, 0)];
        assert!(egg.quads.iter().any(|q| q.tint == 1));
        for m in models.items.values() {
            assert!(crate::item_render::gui_matrix(0, 0, m.gui.matrix()).determinant() > 0.0);
        }
    }
}
