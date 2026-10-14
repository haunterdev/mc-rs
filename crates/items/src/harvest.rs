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
use crate::drops::{double_plant_variant, drop_block_as_item, spawn_as_entity, Drops, DOUBLE_FERN, DOUBLE_GRASS};
use crate::ids::{block, item};
use crate::registry::item_from_block;
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

/// `Block.harvestBlock(world, player, pos, state, te, stack)` with every vanilla override,
/// `stack` being the copy of the held stack taken before it took damage. Silk touch is off and
/// fortune 0.
pub fn harvest_block(state: u16, stack: &ItemStack, rand: &mut JavaRandom) -> Harvest {
    let b = state >> 4;
    let meta = (state & 15) as i32;
    let mut h = Harvest { harvested: true, ..Harvest::default() };
    let base = |h: &mut Harvest, rand: &mut JavaRandom| {
        // Block.harvestBlock: addStat, addExhaustion(0.005F), then dropBlockAsItem(fortune 0).
        h.exhaustion += 0.005;
        let d = drop_block_as_item(state, rand);
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
            h.ice_melts = true;
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
            broke = held.damage_item(dmg, false);
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
