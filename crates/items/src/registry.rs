//! `Item.REGISTRY` and `Block.REGISTRY` lookups over the tables in [`items_table`] and
//! [`blocks_table`]: ids, registry names, stack limits, durability, subtypes, container items,
//! `Item.getItemFromBlock` and `Block.getBlockFromItem`.
//!
//! [`items_table`]: crate::items_table
//! [`blocks_table`]: crate::blocks_table

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::blocks_table::BLOCKS;
use crate::ids::{block, item};
use crate::items_table::ITEMS;
use crate::tools::{ArmorMaterial, ArmorSlot, ToolClass, ToolMaterial};

/// One `Block.REGISTRY` entry.
#[derive(Debug)]
pub struct BlockDef {
    /// Registry path (`"stone"` for `minecraft:stone`); empty for an unused id.
    pub name: &'static str,
    /// `setTranslationKey` argument; `Block.getTranslationKey()` is `"tile." + key`.
    pub key: &'static str,
}

/// How a block item names its subtypes: `ItemMultiTexture.Mapper` and the other
/// `getTranslationKey(ItemStack)` overrides of the `ItemBlock` family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Names {
    /// `ItemBlock.getTranslationKey`: the block's key, no suffix.
    Plain,
    /// `EnumType.byMetadata(meta).getTranslationKey()`: a metadata out of range reads as 0.
    Multi(&'static [&'static str]),
    /// `BlockPlanks.EnumType.byMetadata(meta + 4)` (the `LOG2` mapper).
    Log2,
    /// `ItemLeaves`: `BlockOldLeaf.getWoodType` or `BlockNewLeaf.getWoodType`.
    Leaves { new: bool },
    /// The `SPONGE` mapper: `(meta & 1) == 1 ? "wet" : "dry"`.
    Sponge,
    /// `ItemCloth`: `EnumDyeColor.byMetadata(meta).getTranslationKey()`.
    Cloth,
    /// `ItemColored.setSubtypeNames`: out of range means no suffix at all.
    Colored(&'static [&'static str]),
}

/// The item class family, with the data the ported methods dispatch on. [`ItemDef::class`]
/// keeps the exact Java class name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `ItemAir`
    Air,
    /// plain `Item`
    Plain,
    /// `ItemBlock` and subclasses (`ItemMultiTexture`, `ItemCloth`, `ItemSlab`, `ItemBanner`, ...)
    Block { block: u16, names: Names },
    /// `ItemPickaxe`, `ItemAxe`, `ItemSpade`
    Tool(ToolClass, ToolMaterial),
    /// `ItemSword`
    Sword(ToolMaterial),
    /// `ItemHoe`
    Hoe(ToolMaterial),
    /// `ItemShears`
    Shears,
    /// `ItemArmor`
    Armor(ArmorMaterial, ArmorSlot),
    /// `ItemFood`, `ItemSoup`, `ItemAppleGold`, `ItemChorusFruit`
    Food,
    /// `ItemFishFood`
    Fish { cooked: bool },
    /// `ItemSeeds` and `ItemSeedFood`: plants `crop`
    Seeds { crop: u16 },
    /// `ItemBlockSpecial`: places the block but is not an `ItemBlock`
    Places(u16),
    /// `ItemDoor`
    Door(u16),
    /// `ItemBucket`: the block it holds (`AIR` for the empty bucket)
    Bucket(u16),
    /// `ItemBoat`
    Boat,
    /// `ItemCoal`
    Coal,
    /// `ItemDye`
    Dye,
    /// `ItemSkull`
    Skull,
    /// `ItemBed`
    Bed,
    /// every other class (see [`ItemDef::class`])
    Other,
}

/// One `Item.REGISTRY` entry.
#[derive(Debug)]
pub struct ItemDef {
    /// `Item.getIdFromItem`
    pub id: u16,
    /// Registry path (`"iron_pickaxe"` for `minecraft:iron_pickaxe`).
    pub name: &'static str,
    /// The Java class constructed in `registerItems`.
    pub class: &'static str,
    /// The translation key without its `item.`/`tile.` prefix (see [`ItemDef::translation_key`]).
    pub key: &'static str,
    /// `maxStackSize`
    pub max_stack: i32,
    /// `getMaxDamage()`
    pub max_damage: i32,
    /// `getHasSubtypes()`
    pub has_subtypes: bool,
    /// `getContainerItem()`, 0 for none.
    pub container: u16,
    pub kind: Kind,
}

impl ItemDef {
    /// `Item.getItemStackLimit()`
    pub fn item_stack_limit(&self) -> i32 {
        self.max_stack
    }

    /// `Item.isDamageable()`
    pub fn is_damageable(&self) -> bool {
        self.max_damage > 0 && (!self.has_subtypes || self.max_stack == 1)
    }

    /// `Item.hasContainerItem()`
    pub fn has_container_item(&self) -> bool {
        self.container != 0
    }

    /// `Item.getTranslationKey()`: `"tile." + block key` for `ItemBlock` and `ItemAir`, else
    /// `"item." + key`. The per-stack form is [`crate::names::translation_key`].
    pub fn translation_key(&self) -> String {
        match self.kind {
            Kind::Air | Kind::Block { .. } => format!("tile.{}", self.key),
            _ => format!("item.{}", self.key),
        }
    }

    /// `Item.getMetadata(damage)`: the block metadata an `ItemBlock` places for an item damage.
    pub fn metadata(&self, damage: i32) -> i32 {
        match self.class {
            // ItemMultiTexture, ItemCloth, ItemColored, ItemLilyPad, ItemSlab, ItemSnow
            "ItemMultiTexture" | "ItemCloth" | "ItemColored" | "ItemLilyPad" | "ItemSlab" | "ItemSnow" => damage,
            // ItemLeaves.getMetadata: damage | 4 (no decay check)
            "ItemLeaves" => damage | 4,
            // ItemPiston.getMetadata
            "ItemPiston" => 7,
            // ItemAnvilBlock.getMetadata
            "ItemAnvilBlock" => damage << 2,
            _ => 0,
        }
    }

    /// `ItemTool`/`ItemSword`/`ItemHoe`/`ItemArmor.getIsRepairable(toRepair, repair)`: whether
    /// `repair` is the material's repair item. Every other item answers false.
    pub fn is_repairable_with(&self, repair: u16) -> bool {
        let r = match self.kind {
            Kind::Tool(_, m) | Kind::Sword(m) => Some(m.repair_item()),
            Kind::Armor(m, _) => Some(m.repair_item()),
            // DROPPED: ItemElytra and ItemShield repair with leather and planks in anvils; the
            // anvil is not ported here.
            _ => None,
        };
        r == Some(repair)
    }
}

/// `Item.getItemById(id)`; `None` where Java returns null.
pub fn item(id: u16) -> Option<&'static ItemDef> {
    ITEMS.binary_search_by_key(&id, |d| d.id).ok().map(|i| &ITEMS[i])
}

/// The definition of a registered item, panicking otherwise. For ids taken from [`crate::ids`].
pub fn def(id: u16) -> &'static ItemDef {
    item(id).unwrap_or_else(|| panic!("item id {id} is not registered"))
}

/// Every registered item, sorted by id.
pub fn all_items() -> &'static [ItemDef] {
    ITEMS
}

fn names() -> &'static HashMap<&'static str, u16> {
    static MAP: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
    MAP.get_or_init(|| ITEMS.iter().map(|d| (d.name, d.id)).collect())
}

/// `new ResourceLocation(s)` as the registries read it: a `minecraft:` path, lower-cased. Any
/// other namespace has no vanilla entry.
pub fn resource_path(s: &str) -> Option<String> {
    let lower = s.to_lowercase();
    match lower.split_once(':') {
        Some(("minecraft", p)) => Some(p.to_string()),
        Some(_) => None,
        None => Some(lower),
    }
}

/// `Item.REGISTRY.getObject(new ResourceLocation(name))`
pub fn by_name(name: &str) -> Option<&'static ItemDef> {
    let p = resource_path(name)?;
    names().get(p.as_str()).and_then(|&id| item(id))
}

/// `Item.getByNameOrId(id)`
pub fn by_name_or_id(s: &str) -> Option<&'static ItemDef> {
    by_name(s).or_else(|| s.parse::<i32>().ok().and_then(|n| u16::try_from(n).ok()).and_then(item))
}

/// `Item.getItemFromBlock(block)`: the block's item, or `Items.AIR` (0) when it has none.
pub fn item_from_block(block_id: u16) -> u16 {
    match item(block_id) {
        Some(ItemDef { kind: Kind::Block { block: b, .. }, .. }) if *b == block_id => block_id,
        _ => item::AIR,
    }
}

/// `Block.getBlockFromItem(item)`: the block of an `ItemBlock`, else `Blocks.AIR`.
pub fn block_from_item(item_id: u16) -> u16 {
    match item(item_id) {
        Some(ItemDef { kind: Kind::Block { block: b, .. }, .. }) => *b,
        _ => block::AIR,
    }
}

/// `Block.REGISTRY` entry by id.
pub fn block_def(id: u16) -> &'static BlockDef {
    &BLOCKS[(id & 255) as usize]
}

/// `Block.getTranslationKey()`: `"tile." + key`.
pub fn block_translation_key(id: u16) -> String {
    format!("tile.{}", block_def(id).key)
}
