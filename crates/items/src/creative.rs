//! The creative menu's item lists: `CreativeTabs` (indices, labels, icons, relevant enchantment
//! types) and `CreativeTabs.displayAllRelevantItems`, which walks `Item.REGISTRY` in id order
//! and calls each item's `getSubItems` (and through `ItemBlock` its block's `getSubBlocks`).
//!
//! The per-item tab and the potion, enchantment, spawn egg and fish registries are generated
//! from the server jar ([`crate::creative_table`]); the jar strips `getSubItems` and
//! `getSubBlocks` (client-only), so those are ported here by hand, one arm per override.

use crate::creative_table::{EGGS, ENCHANTMENTS, FISH, ITEM_TABS, POTIONS, TAB_ENCHANTS, TAB_LABELS};
use crate::ids::{block, item};
use crate::registry::{self, ItemDef, Kind};
use crate::stack::{ItemStack, Tag};

/// `CreativeTabs.CREATIVE_TAB_ARRAY`, by index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Tab {
    BuildingBlocks = 0,
    Decorations = 1,
    Redstone = 2,
    Transportation = 3,
    Hotbar = 4,
    Search = 5,
    Misc = 6,
    Food = 7,
    Tools = 8,
    Combat = 9,
    Brewing = 10,
    Inventory = 11,
}

impl Tab {
    pub const ALL: [Tab; 12] = [
        Tab::BuildingBlocks,
        Tab::Decorations,
        Tab::Redstone,
        Tab::Transportation,
        Tab::Hotbar,
        Tab::Search,
        Tab::Misc,
        Tab::Food,
        Tab::Tools,
        Tab::Combat,
        Tab::Brewing,
        Tab::Inventory,
    ];

    /// `getIndex()`
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Tab> {
        Tab::ALL.get(i).copied()
    }

    /// `getTabLabel()`
    pub fn label(self) -> &'static str {
        TAB_LABELS[self.index()]
    }

    /// `getTranslationKey()`: `"itemGroup." + label`.
    pub fn translation_key(self) -> String {
        format!("itemGroup.{}", self.label())
    }

    /// `getColumn()`: `index % 6`.
    pub fn column(self) -> usize {
        self.index() % 6
    }

    /// `isOnTopRow()`: `index < 6`.
    pub fn is_top_row(self) -> bool {
        self.index() < 6
    }

    /// `isAlignedRight()`: `getColumn() == 5`, and HOTBAR, which overrides it.
    pub fn is_aligned_right(self) -> bool {
        self.column() == 5 || self == Tab::Hotbar
    }

    /// `drawInForegroundOfTab()`: every tab but INVENTORY draws its title.
    pub fn draws_title(self) -> bool {
        self != Tab::Inventory
    }

    /// `hasScrollbar()`: every tab but INVENTORY.
    pub fn has_scrollbar(self) -> bool {
        self != Tab::Inventory
    }

    /// `getBackgroundImageName()`: `"item_search.png"` for SEARCH (the search box), `"inventory.png"`
    /// for INVENTORY, `"items.png"` for the rest (under `textures/gui/container/creative_inventory/tab_`).
    pub fn background(self) -> &'static str {
        match self {
            Tab::Search => "item_search.png",
            Tab::Inventory => "inventory.png",
            _ => "items.png",
        }
    }

    /// `getIcon()` (each tab's `getTabIconItem`).
    pub fn icon(self) -> ItemStack {
        match self {
            Tab::BuildingBlocks => ItemStack::of(block::BRICK_BLOCK),
            // BlockDoublePlant.EnumPlantType.PAEONIA
            Tab::Decorations => ItemStack::new(block::DOUBLE_PLANT, 1, 5),
            Tab::Redstone => ItemStack::of(item::REDSTONE),
            Tab::Transportation => ItemStack::of(block::GOLDEN_RAIL),
            Tab::Hotbar => ItemStack::of(block::BOOKSHELF),
            Tab::Search => ItemStack::of(item::COMPASS),
            Tab::Misc => ItemStack::of(item::LAVA_BUCKET),
            Tab::Food => ItemStack::of(item::APPLE),
            Tab::Tools => ItemStack::of(item::IRON_AXE),
            Tab::Combat => ItemStack::of(item::GOLDEN_SWORD),
            // PotionUtils.addPotionToItemStack(new ItemStack(Items.POTIONITEM), PotionTypes.WATER)
            Tab::Brewing => ItemStack::of(item::POTIONITEM).with_tag(Tag::Potion(potion_id("minecraft:water"))),
            Tab::Inventory => ItemStack::of(block::CHEST),
        }
    }

    /// `getRelevantEnchantmentTypes()`
    pub fn relevant_enchantment_types(self) -> &'static [EnchType] {
        TAB_ENCHANTS[self.index()]
    }

    /// `hasRelevantEnchantmentType(type)`
    pub fn has_relevant_enchantment_type(self, ty: Option<EnchType>) -> bool {
        ty.is_some_and(|t| self.relevant_enchantment_types().contains(&t))
    }
}

/// `EnumEnchantmentType`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnchType {
    All,
    Armor,
    ArmorFeet,
    ArmorLegs,
    ArmorChest,
    ArmorHead,
    Weapon,
    Digger,
    FishingRod,
    Breakable,
    Bow,
    Wearable,
}

/// One `PotionEffect` of a `PotionType`.
#[derive(Clone, Copy, Debug)]
pub struct Effect {
    /// `Potion.getName()`, e.g. `effect.moveSpeed`.
    pub key: &'static str,
    pub amplifier: i32,
    /// Ticks.
    pub duration: i32,
    /// `Potion.isBadEffect()`
    pub bad: bool,
    /// `Potion.isInstant()`
    pub instant: bool,
}

/// A `PotionType`.
#[derive(Clone, Copy, Debug)]
pub struct PotionDef {
    pub name: &'static str,
    /// `PotionUtils.getPotionColorFromEffectList(getEffects())`, RGB.
    pub color: u32,
    /// `getNamePrefixed("")`: the lang key suffix after `potion.effect.` and the like.
    pub base: &'static str,
    pub effects: &'static [Effect],
}

/// An `Enchantment`.
#[derive(Clone, Copy, Debug)]
pub struct EnchDef {
    pub id: u8,
    pub name: &'static str,
    /// `getName()`, the lang key, e.g. `enchantment.protect.all`.
    pub key: &'static str,
    pub ty: Option<EnchType>,
    pub min: i32,
    pub max: i32,
}

/// An `EntityList.EntityEggInfo`.
#[derive(Clone, Copy, Debug)]
pub struct EggDef {
    pub entity: &'static str,
    /// `EntityList.getTranslationName`: the lang key is `entity.<this>.name`.
    pub translation: &'static str,
    pub primary: u32,
    pub secondary: u32,
}

/// The `PotionType` registry id of a name, or 0 (`empty`, the registry default).
pub fn potion_id(name: &str) -> u8 {
    POTIONS.iter().position(|p| p.name == name).unwrap_or(0) as u8
}

/// `PotionType.REGISTRY.getObjectById(id)` with the `empty` default.
pub fn potion(id: u8) -> &'static PotionDef {
    POTIONS.get(id as usize).unwrap_or(&POTIONS[0])
}

pub fn enchantment(id: u8) -> Option<&'static EnchDef> {
    ENCHANTMENTS.iter().find(|e| e.id == id)
}

pub fn egg(index: u8) -> Option<&'static EggDef> {
    EGGS.get(index as usize)
}

/// `Item.getCreativeTab()` with the `ItemBlock` and `ItemBanner` overrides.
pub fn tab_of(item_id: u16) -> Option<Tab> {
    ITEM_TABS.binary_search_by_key(&item_id, |t| t.0).ok().and_then(|i| Tab::from_index(ITEM_TABS[i].1 as usize))
}

/// `Item.isInCreativeTab(target)`
fn is_in_creative_tab(item_id: u16, target: Tab) -> bool {
    tab_of(item_id).is_some_and(|t| target == Tab::Search || target == t)
}

/// `CreativeTabs.displayAllRelevantItems(list)` for `tab`, which is also what
/// `GuiContainerCreative.updateCreativeSearch` lists for SEARCH with an empty query. HOTBAR and
/// INVENTORY list nothing (Java throws for HOTBAR; the GUI shows saved toolbars and the player
/// inventory instead).
pub fn display_all_relevant_items(tab: Tab) -> Vec<ItemStack> {
    let mut out = Vec::new();
    if matches!(tab, Tab::Hotbar | Tab::Inventory) {
        return out;
    }
    for d in registry::all_items() {
        get_sub_items(d, tab, &mut out);
    }
    out
}

/// `Item.getSubItems(tab, items)` with every override.
pub fn get_sub_items(d: &ItemDef, tab: Tab, out: &mut Vec<ItemStack>) {
    let id = d.id;
    // ItemEnchantedBook ignores isInCreativeTab: its tab is null.
    if id == item::ENCHANTED_BOOK {
        enchanted_books(tab, out);
        return;
    }
    if !is_in_creative_tab(id, tab) {
        return;
    }
    let mut metas = |range: std::ops::Range<i32>| out.extend(range.map(|m| ItemStack::new(id, 1, m)));
    match (d.kind, id) {
        // ItemBlock.getSubItems: block.getSubBlocks
        (Kind::Block { block: b, .. }, _) => get_sub_blocks(b, id, out),
        // ItemAppleGold, ItemCoal
        (_, item::GOLDEN_APPLE | item::COAL) => metas(0..2),
        // ItemBed, ItemDye
        (_, item::BED | item::DYE) => metas(0..16),
        // ItemSkull: SKULL_TYPES.length
        (_, item::SKULL) => metas(0..6),
        // ItemBanner: makeBanner(color, null) for EnumDyeColor.values(), damage = getDyeDamage()
        (_, item::BANNER) => out.extend((0..16).map(|m| ItemStack::new(id, 1, 15 - m))),
        // ItemFishFood: every FishType, cooked ones only where canCook()
        (Kind::Fish { cooked }, _) => {
            out.extend(FISH.iter().filter(|f| !cooked || f.1).map(|f| ItemStack::new(id, 1, f.0 as i32)));
        }
        // ItemMonsterPlacer: one egg per ENTITY_EGGS entry, applyEntityIdToItemStack
        (_, item::SPAWN_EGG) => out.extend((0..EGGS.len()).map(|i| ItemStack::of(id).with_tag(Tag::Entity(i as u8)))),
        // ItemPotion (and ItemSplashPotion, ItemLingeringPotion): every type but EMPTY
        (_, item::POTIONITEM | item::SPLASH_POTION | item::LINGERING_POTION) => {
            out.extend((1..POTIONS.len()).map(|p| ItemStack::of(id).with_tag(Tag::Potion(p as u8))));
        }
        // ItemTippedArrow: every type with effects
        (_, item::TIPPED_ARROW) => out.extend(
            POTIONS.iter().enumerate().filter(|(_, p)| !p.effects.is_empty()).map(|(p, _)| ItemStack::of(id).with_tag(Tag::Potion(p as u8))),
        ),
        // Item.getSubItems
        _ => out.push(ItemStack::of(id)),
    }
}

/// `ItemEnchantedBook.getSubItems`: on SEARCH every typed enchantment at every level, on a tab
/// with relevant types each matching enchantment at its max level.
fn enchanted_books(tab: Tab, out: &mut Vec<ItemStack>) {
    let book = |e: &EnchDef, level: i32| ItemStack::of(item::ENCHANTED_BOOK).with_tag(Tag::StoredEnchant { id: e.id, level: level as u8 });
    if tab == Tab::Search {
        for e in ENCHANTMENTS.iter().filter(|e| e.ty.is_some()) {
            out.extend((e.min..=e.max).map(|l| book(e, l)));
        }
    } else if !tab.relevant_enchantment_types().is_empty() {
        for e in ENCHANTMENTS.iter().filter(|e| tab.has_relevant_enchantment_type(e.ty)) {
            out.push(book(e, e.max));
        }
    }
}

/// `Block.getSubBlocks(tab, items)` with every override, for the item `item_id` of block `b`.
fn get_sub_blocks(b: u16, item_id: u16, out: &mut Vec<ItemStack>) {
    let metas: &[i32] = match b {
        // BlockStone, BlockSilverfish: every EnumType
        block::STONE => &[0, 1, 2, 3, 4, 5, 6],
        block::MONSTER_EGG => &[0, 1, 2, 3, 4, 5],
        // BlockDirt: DIRT, COARSE_DIRT, PODZOL
        block::DIRT => &[0, 1, 2],
        // BlockPlanks, BlockSapling, BlockWoodSlab: BlockPlanks.EnumType.values()
        block::PLANKS | block::SAPLING | block::WOODEN_SLAB => &[0, 1, 2, 3, 4, 5],
        // BlockSand
        block::SAND => &[0, 1],
        // BlockOldLog, BlockOldLeaf: OAK, SPRUCE, BIRCH, JUNGLE
        block::LOG | block::LEAVES => &[0, 1, 2, 3],
        // BlockNewLog (ACACIA - 4, DARK_OAK - 4), BlockNewLeaf
        block::LOG2 | block::LEAVES2 => &[0, 1],
        // BlockSponge
        block::SPONGE => &[0, 1],
        // BlockSandStone, BlockRedSandstone, BlockPrismarine, BlockQuartz (DEFAULT, CHISELED, LINES_Y), BlockAnvil
        block::SANDSTONE | block::RED_SANDSTONE | block::PRISMARINE | block::QUARTZ_BLOCK | block::ANVIL => &[0, 1, 2],
        // BlockTallGrass: 1 and 2
        block::TALLGRASS => &[1, 2],
        // BlockColored (wool, stained hardened clay, concrete), BlockConcretePowder, BlockCarpet,
        // BlockStainedGlass, BlockStainedGlassPane
        block::WOOL
        | block::STAINED_HARDENED_CLAY
        | block::CONCRETE
        | block::CONCRETE_POWDER
        | block::CARPET
        | block::STAINED_GLASS
        | block::STAINED_GLASS_PANE => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        // BlockFlower: EnumFlowerType.getTypes(color)
        block::YELLOW_FLOWER => &[0],
        block::RED_FLOWER => &[0, 1, 2, 3, 4, 5, 6, 7, 8],
        // BlockStoneSlab: every EnumType but WOOD (2)
        block::STONE_SLAB => &[0, 1, 3, 4, 5, 6, 7],
        // BlockStoneSlabNew
        block::STONE_SLAB2 => &[0],
        // BlockStoneBrick
        block::STONEBRICK => &[0, 1, 2, 3],
        // BlockWall: NORMAL, MOSSY
        block::COBBLESTONE_WALL => &[0, 1],
        // BlockDoublePlant: EnumPlantType.values()
        block::DOUBLE_PLANT => &[0, 1, 2, 3, 4, 5],
        // BlockDaylightDetector: only the non-inverted one (the inverted has no item)
        block::DAYLIGHT_DETECTOR => &[0],
        // Block.getSubBlocks
        _ => &[0],
    };
    out.extend(metas.iter().map(|&m| ItemStack::new(item_id, 1, m)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(list: &[ItemStack], id: u16, meta: i32) -> bool {
        list.iter().any(|s| s.item == id && s.damage == meta)
    }

    #[test]
    fn tabs_hold_their_items() {
        let bb = display_all_relevant_items(Tab::BuildingBlocks);
        assert_eq!(bb[0], ItemStack::new(block::STONE, 1, 0));
        assert!(has(&bb, block::STONE, 6) && has(&bb, block::PLANKS, 5) && has(&bb, block::WOOL, 15));
        assert!(!has(&bb, block::STONE_SLAB, 2), "the wood stone-slab is skipped");
        let food = display_all_relevant_items(Tab::Food);
        assert!(has(&food, item::FISH, 3) && has(&food, item::COOKED_FISH, 1) && !has(&food, item::COOKED_FISH, 2));
        assert!(display_all_relevant_items(Tab::Inventory).is_empty());
        // Tab-less blocks stay out of creative, as in vanilla.
        let search = display_all_relevant_items(Tab::Search);
        assert!(!search.iter().any(|s| s.item == block::BARRIER || s.item == block::COMMAND_BLOCK));
    }

    #[test]
    fn tagged_stacks() {
        let brewing = display_all_relevant_items(Tab::Brewing);
        let potions = brewing.iter().filter(|s| s.item == item::POTIONITEM).count();
        assert_eq!(potions, POTIONS.len() - 1);
        assert!(brewing.iter().all(|s| s.item != item::POTIONITEM || s.tag != Tag::Potion(0)));
        let misc = display_all_relevant_items(Tab::Misc);
        assert_eq!(misc.iter().filter(|s| s.item == item::SPAWN_EGG).count(), EGGS.len());
        // Enchanted books: one per relevant enchantment on TOOLS, every level on SEARCH.
        let tools = display_all_relevant_items(Tab::Tools);
        let books = tools.iter().filter(|s| s.item == item::ENCHANTED_BOOK).count();
        assert!(books > 0);
        let search = display_all_relevant_items(Tab::Search);
        let all_levels: i32 = ENCHANTMENTS.iter().filter(|e| e.ty.is_some()).map(|e| e.max - e.min + 1).sum();
        assert_eq!(search.iter().filter(|s| s.item == item::ENCHANTED_BOOK).count() as i32, all_levels);
        // Two potions of different types do not stack.
        let a = ItemStack::of(item::POTIONITEM).with_tag(Tag::Potion(1));
        let b = ItemStack::of(item::POTIONITEM).with_tag(Tag::Potion(2));
        assert!(!crate::stack::are_item_stack_tags_equal(&a, &b));
        assert!(crate::stack::are_item_stack_tags_equal(&a, &a.copy()));
    }

    #[test]
    fn tab_layout_and_icons() {
        assert_eq!(Tab::Inventory.column(), 5);
        assert!(!Tab::Food.is_top_row());
        assert_eq!(Tab::Brewing.icon().tag, Tag::Potion(potion_id("minecraft:water")));
        assert_eq!(Tab::Combat.relevant_enchantment_types().len(), 10);
        assert_eq!(tab_of(item::BANNER), Some(Tab::Decorations));
        assert_eq!(tab_of(item::ENCHANTED_BOOK), None);
    }
}
