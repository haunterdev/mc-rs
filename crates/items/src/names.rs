//! Translation keys per stack: `Item.getTranslationKey(ItemStack)` with every override, and the
//! `.name` key `getItemStackDisplayName` looks up, so a UI can read names from the jar's
//! `assets/minecraft/lang/en_us.lang` at runtime ([`parse_lang`]).

use std::collections::HashMap;

use crate::blocks_table::BLOCKS;
use crate::food::FishType;
use crate::ids::item;
use crate::items_table::WOOD;
use crate::registry::{Kind, Names};
use crate::stack::ItemStack;

/// `EnumDyeColor.getTranslationKey()`, by metadata.
pub const DYE_KEYS: [&str; 16] = [
    "white", "orange", "magenta", "lightBlue", "yellow", "lime", "pink", "gray", "silver", "cyan", "purple", "blue", "brown",
    "green", "red", "black",
];

/// `ItemSkull.SKULL_TYPES`
pub const SKULL_TYPES: [&str; 6] = ["skeleton", "wither", "zombie", "char", "creeper", "dragon"];

/// `EnumDyeColor.byMetadata(meta)`, as a metadata.
fn dye_by_metadata(meta: i32) -> usize {
    if (0..16).contains(&meta) {
        meta as usize
    } else {
        0
    }
}

/// `EnumDyeColor.byDyeDamage(damage)`, as a metadata (`15 - damage`).
fn dye_by_dye_damage(damage: i32) -> usize {
    let d = if (0..16).contains(&damage) { damage } else { 0 };
    (15 - d) as usize
}

fn suffix(names: Names, meta: i32) -> Option<&'static str> {
    let clamp = |m: i32, len: usize| if m >= 0 && (m as usize) < len { m as usize } else { 0 };
    match names {
        Names::Plain => None,
        Names::Multi(list) => Some(list[clamp(meta, list.len())]),
        // BlockPlanks.EnumType.byMetadata(meta + 4)
        Names::Log2 => Some(WOOD[clamp(meta + 4, 6)]),
        // BlockOldLeaf.getWoodType: byMetadata((meta & 3) % 4); BlockNewLeaf: byMetadata((meta & 3) + 4)
        Names::Leaves { new: false } => Some(WOOD[(meta & 3) as usize]),
        Names::Leaves { new: true } => Some(WOOD[clamp((meta & 3) + 4, 6)]),
        Names::Sponge => Some(if meta & 1 == 1 { "wet" } else { "dry" }),
        Names::Cloth => Some(DYE_KEYS[dye_by_metadata(meta)]),
        Names::Colored(list) => (meta >= 0 && (meta as usize) < list.len()).then(|| list[meta as usize]),
    }
}

/// `ItemStack.getTranslationKey()` (`Item.getTranslationKey(ItemStack)` with the overrides of
/// `ItemBlock`, `ItemMultiTexture`, `ItemCloth`, `ItemColored`, `ItemLeaves`, `ItemSlab`,
/// `ItemCoal`, `ItemDye`, `ItemFishFood`, `ItemSkull`, `ItemBed` and `ItemAir`), e.g.
/// `tile.stone.granite` or `item.dyePowder.black`.
pub fn translation_key(stack: &ItemStack) -> String {
    let d = stack.def();
    let meta = stack.get_metadata();
    match d.kind {
        Kind::Air => format!("tile.{}", d.key),
        Kind::Block { block: b, names } => {
            let base = format!("tile.{}", BLOCKS[b as usize].key);
            match suffix(names, meta) {
                Some(s) => format!("{base}.{s}"),
                None => base,
            }
        }
        // ItemCoal
        Kind::Coal => if meta == 1 { "item.charcoal" } else { "item.coal" }.to_string(),
        // ItemDye: EnumDyeColor.byDyeDamage(meta)
        Kind::Dye => format!("item.{}.{}", d.key, DYE_KEYS[dye_by_dye_damage(meta)]),
        // ItemFishFood
        Kind::Fish { cooked } => {
            let t = FishType::by_item_stack(stack);
            let state = if cooked && t.can_cook() { "cooked" } else { "raw" };
            format!("item.{}.{}.{}", d.key, t.translation_key(), state)
        }
        // ItemSkull
        Kind::Skull => {
            let i = if (0..6).contains(&meta) { meta as usize } else { 0 };
            format!("item.{}.{}", d.key, SKULL_TYPES[i])
        }
        // ItemBed: EnumDyeColor.byMetadata(meta)
        Kind::Bed => format!("item.{}.{}", d.key, DYE_KEYS[dye_by_metadata(meta)]),
        _ => format!("item.{}", d.key),
    }
}

/// The lang key `Item.getItemStackDisplayName(stack)` reads: [`translation_key`] + `".name"`,
/// except where a class builds it another way:
/// `ItemBanner` (`item.banner.<color>.name` by `byDyeDamage(meta & 15)`), and the potion items,
/// whose untagged stacks read the `empty` potion type (`potion.effect.empty` and the splash,
/// lingering and tipped arrow forms). DROPPED with NBT: potion types, spawn egg mob names,
/// player head owners, written book titles and shield banner colors; those stacks name as
/// their untagged forms.
pub fn name_key(stack: &ItemStack) -> String {
    let it = stack.get_item();
    match it {
        item::BANNER => format!("item.banner.{}.name", DYE_KEYS[dye_by_dye_damage(stack.get_metadata() & 15)]),
        item::POTIONITEM => "potion.effect.empty".into(),
        item::SPLASH_POTION => "splash_potion.effect.empty".into(),
        item::LINGERING_POTION => "lingering_potion.effect.empty".into(),
        item::TIPPED_ARROW => "tipped_arrow.effect.empty".into(),
        _ => format!("{}.name", translation_key(stack)),
    }
}

/// The name a `.lang` table gives a stack (`I18n.translateToLocal(...).trim()`), or the key
/// itself when the table lacks it, as `I18n` does.
pub fn display_name(stack: &ItemStack, lang: &HashMap<String, String>) -> String {
    let key = name_key(stack);
    lang.get(&key).map(|s| s.trim().to_string()).unwrap_or(key)
}

/// `LanguageMap.inject`: `key=value` lines of a `.lang` file (comments start with `#`; the
/// value keeps everything after the first `=`). DROPPED: the `%d`/`%f` to `%s` rewrite of
/// format arguments, which no item name uses.
pub fn parse_lang(bytes: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut map = HashMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}
