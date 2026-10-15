//! A survival player breaking a block: the non-world half of
//! `PlayerInteractionManager.tryHarvestBlock` (harvest check, tool wear, `Block.harvestBlock`),
//! with the `harvestBlock` overrides for shears, snow layers, ice and chorus flowers, and
//! `BlockDoublePlant.onBlockHarvested` for the upper half.
//!
//! The world keeps its own jobs: removing the block (`removeBlock`), the other half of doors and
//! beds (their `neighborChanged` drops the remaining half with [`drop_block_as_item`]), placing
//! water under broken ice, and spawning the entities in [`Drops`].

use worldgen::java_random::JavaRandom;

use crate::dig::can_harvest;
use crate::drops::{double_plant_variant, drop_block_as_item, drop_block_as_item_with_chance, spawn_as_entity, Drops, DOUBLE_FERN, DOUBLE_GRASS};
use crate::enchant::{self, id as ench_id};
use crate::ids::{block, item};
use crate::registry::{self, item_from_block};
use crate::stack::ItemStack;
use crate::tools::on_block_destroyed_damage;

/// What `harvestBlock` leaves for the world to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Harvest {
    /// Items and experience, in spawn order.
    pub drops: Drops,
    /// `player.addExhaustion(0.005F)` from `Block.harvestBlock` (and `BlockIce`,
    /// `BlockContainer`); the shears and snow paths add none.
    pub exhaustion: f32,
    /// `BlockIce.harvestBlock` without silk touch: unless water vaporizes in this dimension, the
    /// block becomes `FLOWING_WATER` when the material below blocks movement or is liquid.
    pub ice_melts: bool,
    /// Whether `harvestBlock` ran at all (`EntityPlayer.canHarvestBlock` held).
    pub harvested: bool,
    /// Whether the held stack broke from `onBlockDestroyed`.
    pub tool_broke: bool,
}

fn is_shears(stack: &ItemStack) -> bool {
    stack.get_item() == item::SHEARS
}

/// `Block.canSilkHarvest()` with every override: the default state is a full cube and the block
/// has no tile entity; glass, panes, stained glass, the sea lantern, cobweb and the ender chest
/// always; slabs never.
pub fn can_silk_harvest(state: u16) -> bool {
    let b = state >> 4;
    match b {
        block::GLASS | block::GLASS_PANE | block::STAINED_GLASS | block::STAINED_GLASS_PANE | block::SEA_LANTERN | block::WEB | block::ENDER_CHEST => true,
        block::DOUBLE_STONE_SLAB
        | block::STONE_SLAB
        | block::DOUBLE_WOODEN_SLAB
        | block::WOODEN_SLAB
        | block::DOUBLE_STONE_SLAB2
        | block::STONE_SLAB2
        | block::PURPUR_DOUBLE_SLAB
        | block::PURPUR_SLAB => false,
        // hasTileEntity on a full cube: furnaces, dispensers, droppers, note blocks, jukeboxes and
        // the command and structure blocks.
        block::FURNACE
        | block::LIT_FURNACE
        | block::DISPENSER
        | block::DROPPER
        | block::NOTEBLOCK
        | block::JUKEBOX
        | block::COMMAND_BLOCK
        | block::REPEATING_COMMAND_BLOCK
        | block::CHAIN_COMMAND_BLOCK
        | block::STRUCTURE_BLOCK => false,
        _ => worldgen::populate::state::is_full_cube(b << 4),
    }
}

/// `Block.getSilkTouchDrop(state)` with every override: the block's own item, with the metadata
/// when the item has subtypes; logs and leaves keep only their variant bits, quartz pillars
/// become the vertical one, lit redstone ore and lamps their unlit form, infested blocks the stone
/// they imitate, and the chorus flower nothing.
pub fn silk_touch_drop(state: u16) -> ItemStack {
    let b = state >> 4;
    let meta = (state & 15) as i32;
    let of = |blk: u16, m: i32| ItemStack::new(item_from_block(blk), 1, m);
    match b {
        block::LOG | block::LOG2 | block::LEAVES | block::LEAVES2 => of(b, meta & 3),
        block::QUARTZ_BLOCK => of(b, if meta == 3 || meta == 4 { 2 } else { meta }),
        block::LIT_REDSTONE_ORE => of(block::REDSTONE_ORE, 0),
        block::LIT_REDSTONE_LAMP => of(block::REDSTONE_LAMP, 0),
        block::MONSTER_EGG => match meta {
            1 => of(block::COBBLESTONE, 0),
            2 => of(block::STONEBRICK, 0),
            3 => of(block::STONEBRICK, 1),
            4 => of(block::STONEBRICK, 2),
            5 => of(block::STONEBRICK, 3),
            _ => of(block::STONE, 0),
        },
        block::CHORUS_FLOWER => ItemStack::EMPTY,
        // BlockRotatedPillar (hay bale, bone block) drops its item at meta 0.
        _ => {
            let it = item_from_block(b);
            let subtypes = registry::item(it).is_some_and(|d| d.has_subtypes);
            let pillar = matches!(registry::item(it).map(|d| d.name), Some("hay_block" | "bone_block"));
            ItemStack::new(it, 1, if subtypes && !pillar { meta } else { 0 })
        }
    }
}

/// `Block.harvestBlock(world, player, pos, state, te, stack)` with every vanilla override,
/// `stack` being the copy of the held stack taken before it took damage. Silk Touch and Fortune
/// are read from its enchantments.
pub fn harvest_block(state: u16, stack: &ItemStack, rand: &mut JavaRandom) -> Harvest {
    let b = state >> 4;
    let meta = (state & 15) as i32;
    let mut h = Harvest { harvested: true, ..Harvest::default() };
    let silk = can_silk_harvest(state) && enchant::get_enchantment_level(ench_id::SILK_TOUCH, stack) > 0;
    let fortune = enchant::get_enchantment_level(ench_id::FORTUNE, stack);
    let base = |h: &mut Harvest, rand: &mut JavaRandom| {
        // Block.harvestBlock: addStat, addExhaustion(0.005F), then the silk touch drop, or
        // dropBlockAsItem with the Fortune level.
        h.exhaustion += 0.005;
        if silk {
            spawn_as_entity(&mut h.drops, silk_touch_drop(state), rand);
            return;
        }
        let d = drop_block_as_item_with_chance(state, 1.0, fortune, rand);
        h.drops.items.extend(d.items);
        h.drops.xp += d.xp;
        h.drops.silverfish |= d.silverfish;
    };
    match b {
        // BlockDeadBush, BlockTallGrass, BlockVine, BlockWeb, BlockOldLeaf, BlockNewLeaf: shears
        // take the block itself
        block::DEADBUSH if is_shears(stack) => {
            spawn_as_entity(&mut h.drops, ItemStack::new(item_from_block(block::DEADBUSH), 1, 0), rand)
        }
        block::TALLGRASS if is_shears(stack) => {
            // BlockTallGrass.EnumType.byMetadata(meta).getMeta()
            let t = if meta < 3 { meta } else { 0 };
            spawn_as_entity(&mut h.drops, ItemStack::new(item_from_block(block::TALLGRASS), 1, t), rand)
        }
        block::VINE if is_shears(stack) => spawn_as_entity(&mut h.drops, ItemStack::new(item_from_block(block::VINE), 1, 0), rand),
        block::WEB if is_shears(stack) => spawn_as_entity(&mut h.drops, ItemStack::new(item_from_block(block::WEB), 1, 0), rand),
        block::LEAVES if is_shears(stack) => {
            spawn_as_entity(&mut h.drops, ItemStack::new(item_from_block(block::LEAVES), 1, meta & 3), rand)
        }
        block::LEAVES2 if is_shears(stack) => {
            let variant = if meta & 3 < 2 { (meta & 3) + 4 } else { 0 };
            spawn_as_entity(&mut h.drops, ItemStack::new(item_from_block(block::LEAVES2), 1, variant - 4), rand)
        }
        // BlockDoublePlant: shears on the lower half of double grass or a large fern give two
        // tall grass or ferns (onHarvest)
        block::DOUBLE_PLANT
            if is_shears(stack) && meta & 8 == 0 && matches!(double_plant_variant(meta), DOUBLE_GRASS | DOUBLE_FERN) =>
        {
            let t = if double_plant_variant(meta) == DOUBLE_GRASS { 1 } else { 2 };
            spawn_as_entity(&mut h.drops, ItemStack::new(item_from_block(block::TALLGRASS), 2, t), rand);
        }
        // BlockSnow: layers + 1 snowballs, and the block goes to air
        block::SNOW_LAYER => {
            let layers = (meta & 7) + 1;
            spawn_as_entity(&mut h.drops, ItemStack::new(item::SNOWBALL, layers + 1, 0), rand);
        }
        // BlockIce (and BlockFrostedIce): drops nothing, then may melt
        block::ICE | block::FROSTED_ICE => {
            base(&mut h, rand);
            // BlockIce.harvestBlock: silk touch takes the block itself and it does not melt.
            h.ice_melts = !silk;
        }
        // BlockChorusFlower: the normal (empty) drop, then the flower itself
        block::CHORUS_FLOWER => {
            base(&mut h, rand);
            spawn_as_entity(&mut h.drops, ItemStack::of(item_from_block(block::CHORUS_FLOWER)), rand);
        }
        // DROPPED: BlockBed and BlockBanner take their item from the tile entity, and
        // BlockContainer names the drop after a renamed tile; without tile entities they take
        // the plain path below.
        _ => base(&mut h, rand),
    }
    h
}

/// The survival half of `PlayerInteractionManager.tryHarvestBlock(pos)` once the block is
/// removed: `canHarvestBlock` on the held stack, `held.onBlockDestroyed` (tool wear, in place;
/// a broken stack is left empty), then `harvestBlock` with the pre-damage copy when harvestable.
pub fn player_break_block(state: u16, held: &mut ItemStack, rand: &mut JavaRandom) -> Harvest {
    let copy = if held.is_empty() { ItemStack::EMPTY } else { held.copy() };
    let flag = can_harvest(state, held);
    let mut broke = false;
    if !held.is_empty() {
        let dmg = on_block_destroyed_damage(held.get_item(), state);
        if dmg > 0 {
            broke = held.damage_item(dmg, false, rand);
        }
    }
    let mut h = if flag { harvest_block(state, &copy, rand) } else { Harvest::default() };
    h.tool_broke = broke;
    h
}

/// `BlockDoublePlant.onBlockHarvested` for a survival player breaking the upper half, which runs
/// before the upper half's own harvest: the lower half (`lower_state`) drops as if destroyed,
/// except double grass and ferns under shears, which give two tall grass or ferns. Returns the
/// lower half's drops; the world then removes the lower half.
pub fn double_plant_upper_harvested(lower_state: u16, held: &ItemStack, rand: &mut JavaRandom) -> Drops {
    if lower_state >> 4 != block::DOUBLE_PLANT {
        return Drops::default();
    }
    let v = double_plant_variant((lower_state & 15) as i32);
    if v != DOUBLE_FERN && v != DOUBLE_GRASS {
        // worldIn.destroyBlock(pos.down(), true)
        drop_block_as_item(lower_state, rand)
    } else if !held.is_empty() && is_shears(held) {
        // onHarvest(worldIn, pos, lowerState, player)
        let mut out = Drops::default();
        let t = if v == DOUBLE_GRASS { 1 } else { 2 };
        spawn_as_entity(&mut out, ItemStack::new(item_from_block(block::TALLGRASS), 2, t), rand);
        out
    } else {
        drop_block_as_item(lower_state, rand)
    }
}
