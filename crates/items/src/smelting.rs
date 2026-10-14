//! Furnaces: the `FurnaceRecipes` table (with experience), `TileEntityFurnace.getItemBurnTime`,
//! the furnace's tick (`update`, `canSmelt`, `smeltItem`, `setInventorySlotContents`) and the
//! experience `SlotFurnaceOutput.onCrafting` pays out.

use worldgen::populate::props::material;
use worldgen::populate::state::mat;

use crate::crafting::WILDCARD;
use crate::ids::{block, item};
use crate::registry::{self, block_from_item, item_from_block, Kind};
use crate::stack::{are_item_stack_tags_equal, ItemStack};
use crate::tools::ToolMaterial;

/// One `FurnaceRecipes` entry: `smeltingList` input to output, `experienceList` output to xp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smelting {
    /// Metadata 32767 matches any.
    pub input: ItemStack,
    pub output: ItemStack,
    pub experience: f32,
}

const fn s(input: u16, meta: i32, output: u16, count: i32, out_meta: i32, experience: f32) -> Smelting {
    Smelting {
        input: ItemStack { item: input, count: 1, damage: meta },
        output: ItemStack { item: output, count, damage: out_meta },
        experience,
    }
}

const W: i32 = WILDCARD;

/// The `FurnaceRecipes()` constructor, in order. `addSmeltingRecipeForBlock` and `addSmelting`
/// use metadata 32767; `EnumDyeColor.GREEN.getDyeDamage()` is 2 and `BLUE` 4; the fish loop adds
/// cod (0) and salmon (1), the cookable `ItemFishFood.FishType`s.
#[rustfmt::skip]
pub static SMELTING: [Smelting; 67] = [
    s(block::IRON_ORE, W, item::IRON_INGOT, 1, 0, 0.7),
    s(block::GOLD_ORE, W, item::GOLD_INGOT, 1, 0, 1.0),
    s(block::DIAMOND_ORE, W, item::DIAMOND, 1, 0, 1.0),
    s(block::SAND, W, block::GLASS, 1, 0, 0.1),
    s(item::PORKCHOP, W, item::COOKED_PORKCHOP, 1, 0, 0.35),
    s(item::BEEF, W, item::COOKED_BEEF, 1, 0, 0.35),
    s(item::CHICKEN, W, item::COOKED_CHICKEN, 1, 0, 0.35),
    s(item::RABBIT, W, item::COOKED_RABBIT, 1, 0, 0.35),
    s(item::MUTTON, W, item::COOKED_MUTTON, 1, 0, 0.35),
    s(block::COBBLESTONE, W, block::STONE, 1, 0, 0.1),
    s(block::STONEBRICK, 0, block::STONEBRICK, 1, 2, 0.1),
    s(item::CLAY_BALL, W, item::BRICK, 1, 0, 0.3),
    s(block::CLAY, W, block::HARDENED_CLAY, 1, 0, 0.35),
    s(block::CACTUS, W, item::DYE, 1, 2, 0.2),
    s(block::LOG, W, item::COAL, 1, 1, 0.15),
    s(block::LOG2, W, item::COAL, 1, 1, 0.15),
    s(block::EMERALD_ORE, W, item::EMERALD, 1, 0, 1.0),
    s(item::POTATO, W, item::BAKED_POTATO, 1, 0, 0.35),
    s(block::NETHERRACK, W, item::NETHERBRICK, 1, 0, 0.1),
    s(block::SPONGE, 1, block::SPONGE, 1, 0, 0.15),
    s(item::CHORUS_FRUIT, W, item::CHORUS_FRUIT_POPPED, 1, 0, 0.1),
    s(item::FISH, 0, item::COOKED_FISH, 1, 0, 0.35),
    s(item::FISH, 1, item::COOKED_FISH, 1, 1, 0.35),
    s(block::COAL_ORE, W, item::COAL, 1, 0, 0.1),
    s(block::REDSTONE_ORE, W, item::REDSTONE, 1, 0, 0.7),
    s(block::LAPIS_ORE, W, item::DYE, 1, 4, 0.2),
    s(block::QUARTZ_ORE, W, item::QUARTZ, 1, 0, 0.2),
    s(item::CHAINMAIL_HELMET, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::CHAINMAIL_CHESTPLATE, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::CHAINMAIL_LEGGINGS, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::CHAINMAIL_BOOTS, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_PICKAXE, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_SHOVEL, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_AXE, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_HOE, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_SWORD, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_HELMET, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_CHESTPLATE, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_LEGGINGS, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_BOOTS, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::IRON_HORSE_ARMOR, W, item::IRON_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_PICKAXE, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_SHOVEL, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_AXE, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_HOE, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_SWORD, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_HELMET, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_CHESTPLATE, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_LEGGINGS, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_BOOTS, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(item::GOLDEN_HORSE_ARMOR, W, item::GOLD_NUGGET, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 0, block::WHITE_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 1, block::ORANGE_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 2, block::MAGENTA_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 3, block::LIGHT_BLUE_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 4, block::YELLOW_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 5, block::LIME_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 6, block::PINK_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 7, block::GRAY_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 8, block::SILVER_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 9, block::CYAN_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 10, block::PURPLE_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 11, block::BLUE_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 12, block::BROWN_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 13, block::GREEN_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 14, block::RED_GLAZED_TERRACOTTA, 1, 0, 0.1),
    s(block::STAINED_HARDENED_CLAY, 15, block::BLACK_GLAZED_TERRACOTTA, 1, 0, 0.1),
];

/// `FurnaceRecipes.compareItemStacks(stack1, stack2)`: `stack2`'s item, at its metadata or any.
fn compare_item_stacks(stack1: &ItemStack, stack2: &ItemStack) -> bool {
    stack2.get_item() == stack1.get_item() && (stack2.get_metadata() == WILDCARD || stack2.get_metadata() == stack1.get_metadata())
}

/// `FurnaceRecipes.getSmeltingResult(stack)`: empty when nothing smelts it.
pub fn get_smelting_result(stack: &ItemStack) -> ItemStack {
    SMELTING.iter().find(|r| compare_item_stacks(stack, &r.input)).map_or(ItemStack::EMPTY, |r| r.output)
}

/// `FurnaceRecipes.getSmeltingExperience(stack)`: the experience of a smelted output.
pub fn get_smelting_experience(stack: &ItemStack) -> f32 {
    SMELTING.iter().find(|r| compare_item_stacks(stack, &r.output)).map_or(0.0, |r| r.experience)
}

/// `TileEntityFurnace.getItemBurnTime(stack)`: ticks of fuel, 0 for anything that does not burn.
// Java's branch order, repeated values and all.
#[allow(clippy::if_same_then_else)]
pub fn get_item_burn_time(stack: &ItemStack) -> i32 {
    if stack.is_empty() {
        return 0;
    }
    let it = stack.get_item();
    let kind = registry::item(it).map(|d| d.kind);
    let wood = Some(ToolMaterial::Wood);
    if it == item_from_block(block::WOODEN_SLAB) {
        150
    } else if it == item_from_block(block::WOOL) {
        100
    } else if it == item_from_block(block::CARPET) {
        67
    } else if it == item_from_block(block::LADDER) {
        300
    } else if it == item_from_block(block::WOODEN_BUTTON) {
        100
    } else if mat(block_from_item(it) << 4) == material::WOOD {
        // Block.getBlockFromItem(item).getDefaultState().getMaterial() == Material.WOOD
        300
    } else if it == item_from_block(block::COAL_BLOCK) {
        16000
    } else if matches!(kind, Some(Kind::Tool(_, m)) if Some(m) == wood) {
        200
    } else if matches!(kind, Some(Kind::Sword(m)) if Some(m) == wood) {
        200
    } else if matches!(kind, Some(Kind::Hoe(m)) if Some(m) == wood) {
        200
    } else if it == item::STICK {
        100
    } else if it == item::BOW || it == item::FISHING_ROD {
        300
    } else if it == item::SIGN {
        200
    } else if it == item::COAL {
        1600
    } else if it == item::LAVA_BUCKET {
        20000
    } else if it == item_from_block(block::SAPLING) || it == item::BOWL {
        100
    } else if it == item::BLAZE_ROD {
        2400
    } else if matches!(kind, Some(Kind::Door(_))) && it != item::IRON_DOOR {
        200
    } else if matches!(kind, Some(Kind::Boat)) {
        400
    } else {
        0
    }
}

/// `TileEntityFurnace.isItemFuel(stack)`
pub fn is_item_fuel(stack: &ItemStack) -> bool {
    get_item_burn_time(stack) > 0
}

/// `EntityXPOrb.getXPSplit(expValue)`: the size of the next orb.
pub fn xp_split(exp: i32) -> i32 {
    match exp {
        e if e >= 2477 => 2477,
        e if e >= 1237 => 1237,
        e if e >= 617 => 617,
        e if e >= 307 => 307,
        e if e >= 149 => 149,
        e if e >= 73 => 73,
        e if e >= 37 => 37,
        e if e >= 17 => 17,
        e if e >= 7 => 7,
        e if e >= 3 => 3,
        _ => 1,
    }
}

/// `SlotFurnaceOutput.onCrafting(stack)`: the experience for taking `count` of a smelted
/// `output`. `math_random` stands for `Math.random()`, drawn only for a fractional share.
/// Experience of 1 or more per item pays `count` flat, as in Java.
pub fn furnace_output_xp(output: &ItemStack, count: i32, mut math_random: impl FnMut() -> f64) -> i32 {
    let mut i = count;
    let f = get_smelting_experience(output);
    if f == 0.0 {
        i = 0;
    } else if f < 1.0 {
        let x = i as f32 * f;
        let mut j = x.floor() as i32;
        if j < x.ceil() as i32 && math_random() < (x - j as f32) as f64 {
            j += 1;
        }
        i = j;
    }
    i
}

/// `TileEntityFurnace`: input (0), fuel (1) and output (2) slots and the four timers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Furnace {
    /// `furnaceItemStacks`
    pub items: [ItemStack; 3],
    /// `furnaceBurnTime`: ticks of fuel left
    pub burn_time: i32,
    /// `currentItemBurnTime`: the burn time of the last fuel, for the flame gauge
    pub current_item_burn_time: i32,
    /// `cookTime`
    pub cook_time: i32,
    /// `totalCookTime`
    pub total_cook_time: i32,
}

/// What one [`Furnace::update`] changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FurnaceTick {
    /// `markDirty()` was due.
    pub dirty: bool,
    /// `BlockFurnace.setState`: the block should swap between `FURNACE` and `LIT_FURNACE`.
    pub lit_changed: bool,
    /// An item finished smelting.
    pub smelted: bool,
}

impl Furnace {
    /// `getInventoryStackLimit()`
    pub const STACK_LIMIT: i32 = 64;

    /// `getCookTime(stack)`
    pub fn get_cook_time(&self, _stack: &ItemStack) -> i32 {
        200
    }

    /// `isBurning()`
    pub fn is_burning(&self) -> bool {
        self.burn_time > 0
    }

    /// `setInventorySlotContents(index, stack)`: a new input restarts the cook timer.
    pub fn set_inventory_slot_contents(&mut self, index: usize, mut stack: ItemStack) {
        let old = self.items[index];
        let flag = !stack.is_empty() && stack.is_item_equal(&old) && are_item_stack_tags_equal(&stack, &old);
        if stack.get_count() > Self::STACK_LIMIT {
            stack.set_count(Self::STACK_LIMIT);
        }
        self.items[index] = stack;
        if index == 0 && !flag {
            self.total_cook_time = self.get_cook_time(&stack);
            self.cook_time = 0;
        }
    }

    /// `isItemValidForSlot(index, stack)`: fuel or an empty bucket (when the slot holds none)
    /// in slot 1, anything in slot 0, nothing in the output.
    pub fn is_item_valid_for_slot(&self, index: usize, stack: &ItemStack) -> bool {
        match index {
            2 => false,
            1 => is_item_fuel(stack) || (stack.get_item() == item::BUCKET && self.items[1].get_item() != item::BUCKET),
            _ => true,
        }
    }

    /// `canSmelt()`
    pub fn can_smelt(&self) -> bool {
        if self.items[0].is_empty() {
            return false;
        }
        let result = get_smelting_result(&self.items[0]);
        if result.is_empty() {
            return false;
        }
        let out = self.items[2];
        if out.is_empty() {
            true
        } else if !out.is_item_equal(&result) {
            false
        } else if out.get_count() < Self::STACK_LIMIT && out.get_count() < out.get_max_stack_size() {
            true
        } else {
            out.get_count() < result.get_max_stack_size()
        }
    }

    /// `smeltItem()`: one input into the output; wet sponge also fills an empty bucket in the
    /// fuel slot.
    pub fn smelt_item(&mut self) {
        if !self.can_smelt() {
            return;
        }
        let input = self.items[0];
        let result = get_smelting_result(&input);
        if self.items[2].is_empty() {
            self.items[2] = result.copy();
        } else if self.items[2].get_item() == result.get_item() {
            self.items[2].grow(1);
        }
        if input.get_item() == item_from_block(block::SPONGE)
            && input.get_metadata() == 1
            && !self.items[1].is_empty()
            && self.items[1].get_item() == item::BUCKET
        {
            self.items[1] = ItemStack::of(item::WATER_BUCKET);
        }
        self.items[0].shrink(1);
        if self.items[0].is_empty() {
            self.items[0] = ItemStack::EMPTY;
        }
    }

    /// `update()`: one server tick.
    pub fn update(&mut self) -> FurnaceTick {
        let mut t = FurnaceTick::default();
        let flag = self.is_burning();
        if self.is_burning() {
            self.burn_time -= 1;
        }
        let fuel = self.items[1];
        if self.is_burning() || (!fuel.is_empty() && !self.items[0].is_empty()) {
            if !self.is_burning() && self.can_smelt() {
                self.burn_time = get_item_burn_time(&fuel);
                self.current_item_burn_time = self.burn_time;
                if self.is_burning() {
                    t.dirty = true;
                    if !fuel.is_empty() {
                        let it = fuel.get_item();
                        self.items[1].shrink(1);
                        if self.items[1].is_empty() {
                            let c = registry::def(it).container;
                            self.items[1] = if c == 0 { ItemStack::EMPTY } else { ItemStack::of(c) };
                        }
                    }
                }
            }
            if self.is_burning() && self.can_smelt() {
                self.cook_time += 1;
                if self.cook_time == self.total_cook_time {
                    self.cook_time = 0;
                    self.total_cook_time = self.get_cook_time(&self.items[0]);
                    self.smelt_item();
                    t.smelted = true;
                    t.dirty = true;
                }
            } else {
                self.cook_time = 0;
            }
        } else if !self.is_burning() && self.cook_time > 0 {
            self.cook_time = (self.cook_time - 2).clamp(0, self.total_cook_time);
        }
        if flag != self.is_burning() {
            t.dirty = true;
            t.lit_changed = true;
        }
        t
    }
}
