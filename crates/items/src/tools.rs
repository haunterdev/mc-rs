//! Tools and armor: `Item.ToolMaterial`, `ItemArmor.ArmorMaterial`, and the per-class
//! `getDestroySpeed`, `canHarvestBlock`, `onBlockDestroyed`, `hitEntity` and attribute values of
//! `ItemTool` (`ItemPickaxe`, `ItemAxe`, `ItemSpade`), `ItemSword`, `ItemHoe` and `ItemShears`.
//!
//! Block states are `worldgen` state ids (`id << 4 | meta`); materials come from the generated
//! `worldgen::populate::state` table.

use worldgen::populate::props::material;
use worldgen::populate::state::{hardness, mat};

use crate::ids::{block, item};
use crate::registry::{self, Kind};

/// `Item.ToolMaterial`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolMaterial {
    Wood,
    Stone,
    Iron,
    Diamond,
    Gold,
}

impl ToolMaterial {
    pub const VALUES: [ToolMaterial; 5] =
        [ToolMaterial::Wood, ToolMaterial::Stone, ToolMaterial::Iron, ToolMaterial::Diamond, ToolMaterial::Gold];

    /// `(harvestLevel, maxUses, efficiency, attackDamage, enchantability)` from the enum constructor.
    const fn data(self) -> (i32, i32, f32, f32, i32) {
        match self {
            ToolMaterial::Wood => (0, 59, 2.0, 0.0, 15),
            ToolMaterial::Stone => (1, 131, 4.0, 1.0, 5),
            ToolMaterial::Iron => (2, 250, 6.0, 2.0, 14),
            ToolMaterial::Diamond => (3, 1561, 8.0, 3.0, 10),
            ToolMaterial::Gold => (0, 32, 12.0, 0.0, 22),
        }
    }
    /// `getHarvestLevel()`
    pub const fn harvest_level(self) -> i32 {
        self.data().0
    }
    /// `getMaxUses()`
    pub const fn max_uses(self) -> i32 {
        self.data().1
    }
    /// `getEfficiency()`
    pub const fn efficiency(self) -> f32 {
        self.data().2
    }
    /// `getAttackDamage()`
    pub const fn attack_damage(self) -> f32 {
        self.data().3
    }
    /// `getEnchantability()`
    pub const fn enchantability(self) -> i32 {
        self.data().4
    }
    /// `ordinal()`
    pub const fn ordinal(self) -> usize {
        self as usize
    }
    /// `toString()`, which `getToolMaterialName()` returns.
    pub const fn name(self) -> &'static str {
        match self {
            ToolMaterial::Wood => "WOOD",
            ToolMaterial::Stone => "STONE",
            ToolMaterial::Iron => "IRON",
            ToolMaterial::Diamond => "DIAMOND",
            ToolMaterial::Gold => "GOLD",
        }
    }
    /// `getRepairItem()`
    pub const fn repair_item(self) -> u16 {
        match self {
            ToolMaterial::Wood => block::PLANKS,
            ToolMaterial::Stone => block::COBBLESTONE,
            ToolMaterial::Gold => item::GOLD_INGOT,
            ToolMaterial::Iron => item::IRON_INGOT,
            ToolMaterial::Diamond => item::DIAMOND,
        }
    }
}

/// Which `ItemTool` subclass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolClass {
    Pickaxe,
    Axe,
    Spade,
}

/// `EntityEquipmentSlot` armor slots, in `getIndex()` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmorSlot {
    Feet,
    Legs,
    Chest,
    Head,
}

impl ArmorSlot {
    /// `EntityEquipmentSlot.getIndex()`
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// `ItemArmor.MAX_DAMAGE_ARRAY`, by slot index.
const MAX_DAMAGE_ARRAY: [i32; 4] = [13, 15, 16, 11];

/// `ItemArmor.ArmorMaterial`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmorMaterial {
    Leather,
    Chain,
    Iron,
    Gold,
    Diamond,
}

impl ArmorMaterial {
    /// `(name, maxDamageFactor, damageReductionAmountArray, enchantability, toughness)`
    const fn data(self) -> (&'static str, i32, [i32; 4], i32, f32) {
        match self {
            ArmorMaterial::Leather => ("leather", 5, [1, 2, 3, 1], 15, 0.0),
            ArmorMaterial::Chain => ("chainmail", 15, [1, 4, 5, 2], 12, 0.0),
            ArmorMaterial::Iron => ("iron", 15, [2, 5, 6, 2], 9, 0.0),
            ArmorMaterial::Gold => ("gold", 7, [1, 3, 5, 2], 25, 0.0),
            ArmorMaterial::Diamond => ("diamond", 33, [3, 6, 8, 3], 10, 2.0),
        }
    }
    /// `getName()`
    pub const fn name(self) -> &'static str {
        self.data().0
    }
    /// `getDurability(slot)`
    pub const fn durability(self, slot: ArmorSlot) -> i32 {
        MAX_DAMAGE_ARRAY[slot.index()] * self.data().1
    }
    /// `getDamageReductionAmount(slot)`
    pub const fn damage_reduction_amount(self, slot: ArmorSlot) -> i32 {
        self.data().2[slot.index()]
    }
    /// `getEnchantability()`
    pub const fn enchantability(self) -> i32 {
        self.data().3
    }
    /// `getToughness()`
    pub const fn toughness(self) -> f32 {
        self.data().4
    }
    /// `getRepairItem()`
    pub const fn repair_item(self) -> u16 {
        match self {
            ArmorMaterial::Leather => item::LEATHER,
            ArmorMaterial::Chain | ArmorMaterial::Iron => item::IRON_INGOT,
            ArmorMaterial::Gold => item::GOLD_INGOT,
            ArmorMaterial::Diamond => item::DIAMOND,
        }
    }
}

#[inline]
fn block_of(state: u16) -> u16 {
    state >> 4
}

/// `ItemPickaxe.EFFECTIVE_ON`
pub const PICKAXE_EFFECTIVE_ON: [u16; 27] = [
    block::ACTIVATOR_RAIL,
    block::COAL_ORE,
    block::COBBLESTONE,
    block::DETECTOR_RAIL,
    block::DIAMOND_BLOCK,
    block::DIAMOND_ORE,
    block::DOUBLE_STONE_SLAB,
    block::GOLDEN_RAIL,
    block::GOLD_BLOCK,
    block::GOLD_ORE,
    block::ICE,
    block::IRON_BLOCK,
    block::IRON_ORE,
    block::LAPIS_BLOCK,
    block::LAPIS_ORE,
    block::LIT_REDSTONE_ORE,
    block::MOSSY_COBBLESTONE,
    block::NETHERRACK,
    block::PACKED_ICE,
    block::RAIL,
    block::REDSTONE_ORE,
    block::SANDSTONE,
    block::RED_SANDSTONE,
    block::STONE,
    block::STONE_SLAB,
    block::STONE_BUTTON,
    block::STONE_PRESSURE_PLATE,
];

/// `ItemAxe.EFFECTIVE_ON`
pub const AXE_EFFECTIVE_ON: [u16; 11] = [
    block::PLANKS,
    block::BOOKSHELF,
    block::LOG,
    block::LOG2,
    block::CHEST,
    block::PUMPKIN,
    block::LIT_PUMPKIN,
    block::MELON_BLOCK,
    block::LADDER,
    block::WOODEN_BUTTON,
    block::WOODEN_PRESSURE_PLATE,
];

/// `ItemSpade.EFFECTIVE_ON`
pub const SPADE_EFFECTIVE_ON: [u16; 12] = [
    block::CLAY,
    block::DIRT,
    block::FARMLAND,
    block::GRASS,
    block::GRAVEL,
    block::MYCELIUM,
    block::SAND,
    block::SNOW,
    block::SNOW_LAYER,
    block::SOUL_SAND,
    block::GRASS_PATH,
    block::CONCRETE_POWDER,
];

impl ToolClass {
    /// `ItemTool.effectiveBlocks`
    pub fn effective_blocks(self) -> &'static [u16] {
        match self {
            ToolClass::Pickaxe => &PICKAXE_EFFECTIVE_ON,
            ToolClass::Axe => &AXE_EFFECTIVE_ON,
            ToolClass::Spade => &SPADE_EFFECTIVE_ON,
        }
    }
}

/// `ItemTool.getDestroySpeed`: the material's efficiency on an effective block, else 1.
fn item_tool_destroy_speed(class: ToolClass, m: ToolMaterial, state: u16) -> f32 {
    if class.effective_blocks().contains(&block_of(state)) {
        m.efficiency()
    } else {
        1.0
    }
}

/// `Item.getDestroySpeed(stack, state)`, dispatched on the item class: `ItemPickaxe`, `ItemAxe`,
/// `ItemTool` (spade), `ItemSword`, `ItemShears`, else the base 1.0.
pub fn destroy_speed(item_id: u16, state: u16) -> f32 {
    let Some(d) = registry::item(item_id) else { return 1.0 };
    let m = mat(state);
    match d.kind {
        // ItemPickaxe.getDestroySpeed
        Kind::Tool(ToolClass::Pickaxe, tm) => {
            if m != material::IRON && m != material::ANVIL && m != material::ROCK {
                item_tool_destroy_speed(ToolClass::Pickaxe, tm, state)
            } else {
                tm.efficiency()
            }
        }
        // ItemAxe.getDestroySpeed
        Kind::Tool(ToolClass::Axe, tm) => {
            if m != material::WOOD && m != material::PLANTS && m != material::VINE {
                item_tool_destroy_speed(ToolClass::Axe, tm, state)
            } else {
                tm.efficiency()
            }
        }
        // ItemSpade inherits ItemTool.getDestroySpeed
        Kind::Tool(ToolClass::Spade, tm) => item_tool_destroy_speed(ToolClass::Spade, tm, state),
        // ItemSword.getDestroySpeed
        Kind::Sword(_) => {
            if block_of(state) == block::WEB {
                15.0
            } else if m != material::PLANTS
                && m != material::VINE
                && m != material::CORAL
                && m != material::LEAVES
                && m != material::GOURD
            {
                1.0
            } else {
                1.5
            }
        }
        // ItemShears.getDestroySpeed
        Kind::Shears => {
            let b = block_of(state);
            if b != block::WEB && m != material::LEAVES {
                if b == block::WOOL {
                    5.0
                } else {
                    1.0
                }
            } else {
                15.0
            }
        }
        _ => 1.0,
    }
}

// The pickaxe branches keep Java's order, repeated values and all.
#[allow(clippy::if_same_then_else)]
/// `Item.canHarvestBlock(state)`, dispatched: `ItemPickaxe`, `ItemSpade`, `ItemSword`,
/// `ItemShears`; every other item (the axe included) answers false.
pub fn can_harvest_block(item_id: u16, state: u16) -> bool {
    let Some(d) = registry::item(item_id) else { return false };
    let b = block_of(state);
    match d.kind {
        // ItemPickaxe.canHarvestBlock
        Kind::Tool(ToolClass::Pickaxe, tm) => {
            let level = tm.harvest_level();
            if b == block::OBSIDIAN {
                level == 3
            } else if b == block::DIAMOND_BLOCK || b == block::DIAMOND_ORE {
                level >= 2
            } else if b == block::EMERALD_ORE || b == block::EMERALD_BLOCK {
                level >= 2
            } else if b == block::GOLD_BLOCK || b == block::GOLD_ORE {
                level >= 2
            } else if b == block::IRON_BLOCK || b == block::IRON_ORE {
                level >= 1
            } else if b == block::LAPIS_BLOCK || b == block::LAPIS_ORE {
                level >= 1
            } else if b == block::REDSTONE_ORE || b == block::LIT_REDSTONE_ORE {
                level >= 2
            } else {
                let m = mat(state);
                m == material::ROCK || m == material::IRON || m == material::ANVIL
            }
        }
        // ItemSpade.canHarvestBlock
        Kind::Tool(ToolClass::Spade, _) => b == block::SNOW_LAYER || b == block::SNOW,
        // ItemSword.canHarvestBlock
        Kind::Sword(_) => b == block::WEB,
        // ItemShears.canHarvestBlock
        Kind::Shears => b == block::WEB || b == block::REDSTONE_WIRE || b == block::TRIPWIRE,
        _ => false,
    }
}

/// `Item.onBlockDestroyed(stack, world, state, pos, entity)` as the durability it costs:
/// `ItemTool` 1 and `ItemSword` 2 when the block's hardness is not 0, `ItemShears` 1 always.
/// Apply it with [`crate::ItemStack::damage_item`].
pub fn on_block_destroyed_damage(item_id: u16, state: u16) -> i32 {
    let Some(d) = registry::item(item_id) else { return 0 };
    match d.kind {
        // ItemTool.onBlockDestroyed: (double)state.getBlockHardness(worldIn, pos) != 0.0D
        Kind::Tool(..) => i32::from(hardness(state) as f64 != 0.0),
        // ItemSword.onBlockDestroyed
        Kind::Sword(_) => 2 * i32::from(hardness(state) as f64 != 0.0),
        // ItemShears.onBlockDestroyed: damageItem(1) before anything else, then Item's false.
        Kind::Shears => 1,
        _ => 0,
    }
}

/// `Item.hitEntity(stack, target, attacker)` as the durability it costs: `ItemTool` 2,
/// `ItemSword` and `ItemHoe` 1.
pub fn hit_entity_damage(item_id: u16) -> i32 {
    match registry::item(item_id).map(|d| d.kind) {
        Some(Kind::Tool(..)) => 2,
        Some(Kind::Sword(_)) | Some(Kind::Hoe(_)) => 1,
        _ => 0,
    }
}

/// `ItemAxe.ATTACK_DAMAGES` and `ATTACK_SPEEDS`, by material ordinal.
const AXE_ATTACK_DAMAGES: [f32; 5] = [6.0, 8.0, 8.0, 8.0, 6.0];
const AXE_ATTACK_SPEEDS: [f32; 5] = [-3.2, -3.2, -3.1, -3.0, -3.0];

/// `getItemAttributeModifiers(MAINHAND)`: the `generic.attackDamage` and
/// `generic.attackSpeed` modifier amounts (operation 0, added to the player's base 1.0 and
/// 4.0). `None` for items without them.
pub fn attack_modifiers(item_id: u16) -> Option<(f64, f64)> {
    let d = registry::item(item_id)?;
    match d.kind {
        Kind::Tool(class, m) => {
            // ItemTool(attackDamageIn, attackSpeedIn, material): attackDamageIn + material damage;
            // ItemAxe replaces both from its arrays.
            let (damage, speed) = match class {
                ToolClass::Pickaxe => (1.0 + m.attack_damage(), -2.8f32),
                ToolClass::Spade => (1.5 + m.attack_damage(), -3.0f32),
                ToolClass::Axe => (AXE_ATTACK_DAMAGES[m.ordinal()], AXE_ATTACK_SPEEDS[m.ordinal()]),
            };
            Some((damage as f64, speed as f64))
        }
        // ItemSword: 3.0F + material damage, speed -2.4000000953674316D
        Kind::Sword(m) => Some(((3.0 + m.attack_damage()) as f64, -2.4000000953674316)),
        // ItemHoe: 0 damage, speed (material damage + 1.0F) - 4.0F
        Kind::Hoe(m) => Some((0.0, ((m.attack_damage() + 1.0) - 4.0) as f64)),
        _ => None,
    }
}

/// `Item.getItemEnchantability()`: `ItemTool`, `ItemSword` and `ItemArmor` from their material,
/// `ItemBow`, `ItemFishingRod` and `ItemBook` 1, the rest (`ItemHoe` included) 0.
pub fn item_enchantability(item_id: u16) -> i32 {
    match registry::item(item_id).map(|d| d.kind) {
        Some(Kind::Tool(_, m)) | Some(Kind::Sword(m)) => m.enchantability(),
        Some(Kind::Armor(m, _)) => m.enchantability(),
        _ if matches!(item_id, item::BOW | item::FISHING_ROD | item::BOOK) => 1,
        _ => 0,
    }
}
