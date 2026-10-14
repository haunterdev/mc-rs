//! What a block drops: `Block.getItemDropped`, `quantityDropped`, `quantityDroppedWithBonus`,
//! `damageDropped`, `dropBlockAsItemWithChance` and `spawnAsEntity` with every override in
//! 1.12.2, drawing from a `java.util.Random` in Java's order. The player path
//! (`harvestBlock`, shears, snow layers, ice) is in [`crate::harvest`].
//!
//! Silk touch is not modelled (`canSilkHarvest`/`getSilkTouchDrop` are never reached with it
//! off). Fortune is ported where the Java takes it; the player path passes 0.

use worldgen::java_random::JavaRandom;

pub use crate::drop_rules::{damage_dropped, get_item_dropped, quantity_dropped, quantity_dropped_with_bonus};
pub(crate) use crate::drop_rules::{double_plant_variant, DOUBLE_FERN, DOUBLE_GRASS};
use crate::drop_rules::{block_of, by_meta, crops_max_age, crops_seed_and_crop, is_crops, is_ore, meta_of};
use crate::ids::{block, item};
use crate::registry::item_from_block;
use crate::stack::ItemStack;

/// One `EntityItem` that `Block.spawnAsEntity` would spawn: the stack and its offset inside the
/// block (`world.rand.nextFloat() * 0.5F + 0.25D` on each axis).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drop {
    pub stack: ItemStack,
    pub offset: [f64; 3],
}

/// Everything breaking a block produces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drops {
    /// Item entities, in spawn order.
    pub items: Vec<Drop>,
    /// Experience from `dropXpOnBlockBreak` (ores, redstone ore, spawners). The caller splits it
    /// into orbs with `EntityXPOrb.getXPSplit`.
    pub xp: i32,
    /// `BlockSilverfish.dropBlockAsItemWithChance` spawns a silverfish (when `doTileDrops`).
    pub silverfish: bool,
}

impl Drops {
    /// The dropped stacks without their offsets.
    pub fn stacks(&self) -> Vec<ItemStack> {
        self.items.iter().map(|d| d.stack).collect()
    }
}

/// `MathHelper.getInt(random, min, max)`
fn get_int(rand: &mut JavaRandom, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rand.next_int_bounded(max - min + 1) + min
    }
}

/// `Block.spawnAsEntity(world, pos, stack)`: three offset draws for any non-empty stack.
/// (`doTileDrops` is assumed on.)
pub fn spawn_as_entity(out: &mut Drops, stack: ItemStack, rand: &mut JavaRandom) {
    if !stack.is_empty() {
        let d0 = (rand.next_float() * 0.5) as f64 + 0.25;
        let d1 = (rand.next_float() * 0.5) as f64 + 0.25;
        let d2 = (rand.next_float() * 0.5) as f64 + 0.25;
        out.items.push(Drop { stack, offset: [d0, d1, d2] });
    }
}

/// The generic `Block.dropBlockAsItemWithChance` body.
fn default_drop(out: &mut Drops, state: u16, chance: f32, fortune: i32, rand: &mut JavaRandom) {
    let i = quantity_dropped_with_bonus(state, fortune, rand);
    for _ in 0..i {
        if rand.next_float() <= chance {
            let it = get_item_dropped(state, rand, fortune);
            if it != item::AIR {
                spawn_as_entity(out, ItemStack::new(it, 1, damage_dropped(state)), rand);
            }
        }
    }
}

/// `Block.dropBlockAsItemWithChance(world, pos, state, chance, fortune)` with every override,
/// drawing from `rand` (`World.rand`) as the server does.
pub fn drop_block_as_item_with_chance(state: u16, chance: f32, fortune: i32, rand: &mut JavaRandom) -> Drops {
    let mut out = Drops::default();
    let b = block_of(state);
    let meta = meta_of(state);
    match b {
        // BlockAir, BlockBarrier, BlockStructureVoid: nothing
        block::AIR | block::BARRIER | block::STRUCTURE_VOID => {}
        // BlockSkull: nothing here; the skull drops from onBlockHarvested/breakBlock.
        // DROPPED: the skull item with its TileEntitySkull type; tile entities are not ported.
        block::SKULL => {}
        // BlockShulkerBox: nothing here; breakBlock drops the box with its contents.
        // DROPPED: that drop needs the TileEntityShulkerBox.
        b if (block::WHITE_SHULKER_BOX..=block::BLACK_SHULKER_BOX).contains(&b) => {}
        // BlockPistonMoving: drops the moving block's state from the TileEntityPiston.
        // DROPPED: no tile entity, so nothing (what Java does when the tile is gone).
        block::PISTON_EXTENSION => {}
        // BlockBed: only the head drops, a bed of the TileEntityBed's color.
        block::BED => {
            if meta & 8 != 0 {
                // DROPPED: the TileEntityBed color; without the tile Java uses EnumDyeColor.RED.
                spawn_as_entity(&mut out, ItemStack::new(item::BED, 1, 14), rand);
            }
        }
        // BlockCocoa: 3 cocoa beans when grown (age >= 2), else 1; no chance roll.
        block::COCOA => {
            let n = if (meta & 15) >> 2 >= 2 { 3 } else { 1 };
            for _ in 0..n {
                spawn_as_entity(&mut out, ItemStack::new(item::DYE, 1, 3), rand);
            }
        }
        // BlockCrops (and BlockPotato's extra roll)
        b if is_crops(b) => {
            default_drop(&mut out, state, chance, 0, rand);
            let max = crops_max_age(b);
            if meta >= max {
                let j = 3 + fortune;
                let (seed, _) = crops_seed_and_crop(b);
                for _ in 0..j {
                    if rand.next_int_bounded(2 * max) <= meta {
                        spawn_as_entity(&mut out, ItemStack::of(seed), rand);
                    }
                }
            }
            // BlockPotato.dropBlockAsItemWithChance
            if b == block::POTATOES && meta >= max && rand.next_int_bounded(50) == 0 {
                spawn_as_entity(&mut out, ItemStack::of(item::POISONOUS_POTATO), rand);
            }
        }
        // BlockJukebox: fortune 0. DROPPED: the record, which breakBlock ejects from the tile.
        block::JUKEBOX => default_drop(&mut out, state, chance, 0, rand),
        // BlockLeaves
        block::LEAVES | block::LEAVES2 => {
            let old = b == block::LEAVES;
            let variant = if old { meta & 3 } else { by_meta((meta & 3) + 4, 6) };
            // getSaplingDropChance: BlockOldLeaf gives jungle 40, the rest 20
            let mut i = if old && variant == 3 { 40 } else { 20 };
            if fortune > 0 {
                i -= 2 << fortune;
                if i < 10 {
                    i = 10;
                }
            }
            if rand.next_int_bounded(i) == 0 {
                let it = get_item_dropped(state, rand, fortune);
                spawn_as_entity(&mut out, ItemStack::new(it, 1, damage_dropped(state)), rand);
            }
            let mut i = 200;
            if fortune > 0 {
                i -= 10 << fortune;
                if i < 40 {
                    i = 40;
                }
            }
            // dropApple: BlockOldLeaf for OAK, BlockNewLeaf for DARK_OAK
            let apple = if old { variant == 0 } else { variant == 5 };
            if apple && rand.next_int_bounded(i) == 0 {
                spawn_as_entity(&mut out, ItemStack::of(item::APPLE), rand);
            }
        }
        // BlockMobSpawner: nothing, then 15 to 43 experience
        block::MOB_SPAWNER => {
            default_drop(&mut out, state, chance, fortune, rand);
            out.xp += 15 + rand.next_int_bounded(15) + rand.next_int_bounded(15);
        }
        // BlockNetherWart
        block::NETHER_WART => {
            let mut i = 1;
            if meta >= 3 {
                i = 2 + rand.next_int_bounded(3);
                if fortune > 0 {
                    i += rand.next_int_bounded(fortune + 1);
                }
            }
            for _ in 0..i {
                spawn_as_entity(&mut out, ItemStack::of(item::NETHER_WART), rand);
            }
        }
        // BlockOre: experience when the drop is not the ore itself
        b if is_ore(b) => {
            default_drop(&mut out, state, chance, fortune, rand);
            if get_item_dropped(state, rand, fortune) != item_from_block(b) {
                out.xp += match b {
                    block::COAL_ORE => get_int(rand, 0, 2),
                    block::DIAMOND_ORE | block::EMERALD_ORE => get_int(rand, 3, 7),
                    block::LAPIS_ORE | block::QUARTZ_ORE => get_int(rand, 2, 5),
                    _ => 0,
                };
            }
        }
        // BlockRedstoneOre: 1 to 5 experience
        block::REDSTONE_ORE | block::LIT_REDSTONE_ORE => {
            default_drop(&mut out, state, chance, fortune, rand);
            if get_item_dropped(state, rand, fortune) != item_from_block(b) {
                out.xp += 1 + rand.next_int_bounded(5);
            }
        }
        // BlockSilverfish: a silverfish instead of an item
        block::MONSTER_EGG => out.silverfish = true,
        // BlockStem: up to 3 seeds by age
        block::PUMPKIN_STEM | block::MELON_STEM => {
            default_drop(&mut out, state, chance, fortune, rand);
            let seed = if b == block::PUMPKIN_STEM { item::PUMPKIN_SEEDS } else { item::MELON_SEEDS };
            for _ in 0..3 {
                if rand.next_int_bounded(15) <= meta {
                    spawn_as_entity(&mut out, ItemStack::of(seed), rand);
                }
            }
        }
        // BlockBanner: the tile's banner, else the plain drop. DROPPED: the tile's color and
        // patterns, so a broken banner drops item damage 0 here.
        // DROPPED: container contents (chests, furnaces, dispensers, hoppers, brewing stands)
        // and flower pot plants come out in breakBlock from their tile entities.
        _ => default_drop(&mut out, state, chance, fortune, rand),
    }
    out
}

/// `Block.dropBlockAsItem(world, pos, state, 0)`: what a block drops when the world breaks it
/// (`World.destroyBlock`, a failed `canBlockStay`, an explosion at chance 1).
pub fn drop_block_as_item(state: u16, rand: &mut JavaRandom) -> Drops {
    drop_block_as_item_with_chance(state, 1.0, 0, rand)
}
