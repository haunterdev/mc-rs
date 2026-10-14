//! How fast a player breaks a block: `Block.getBlockHardness`, `Material.isToolNotRequired`,
//! `EntityPlayer.getDigSpeed` and `canHarvestBlock`, `Block.getPlayerRelativeBlockHardness`,
//! and the per-tick accumulation of `PlayerControllerMP.onPlayerDamageBlock`. Forge's hooks are
//! not involved; this is vanilla 1.12.2.

use worldgen::populate::props::material;
use worldgen::populate::state::{hardness, mat};

use crate::stack::ItemStack;

/// `IBlockState.getBlockHardness(world, pos)`: `Block.blockHardness`, from `setHardness` in
/// `Block.registerBlocks` and the constructors (no block overrides it in 1.12.2). -1 means
/// unbreakable (`setBlockUnbreakable`). Read from the generated `worldgen` property table.
pub fn block_hardness(state: u16) -> f32 {
    hardness(state)
}

/// `Material.isToolNotRequired()`: false for the materials built with `setRequiresTool()`:
/// ROCK, IRON, ANVIL, SNOW, CRAFTED_SNOW, WEB and BARRIER.
pub fn is_tool_not_required(material_index: u8) -> bool {
    !matches!(
        material_index,
        material::ROCK
            | material::IRON
            | material::ANVIL
            | material::SNOW
            | material::CRAFTED_SNOW
            | material::WEB
            | material::BARRIER
    )
}

/// The player conditions `EntityPlayer.getDigSpeed` reads besides the held item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DigContext {
    /// `onGround`; off the ground digging is 5 times slower.
    pub on_ground: bool,
    /// `isInsideOfMaterial(Material.WATER)`: the eyes are in water.
    pub eyes_in_water: bool,
    /// `EnchantmentHelper.getAquaAffinityModifier`
    pub aqua_affinity: bool,
    /// `EnchantmentHelper.getEfficiencyModifier`: the held tool's Efficiency level.
    pub efficiency: i32,
    /// Haste amplifier, if the effect is active.
    pub haste: Option<i32>,
    /// Mining Fatigue amplifier, if the effect is active.
    pub mining_fatigue: Option<i32>,
}

impl Default for DigContext {
    /// Standing on the ground, dry, no enchantments, no effects.
    fn default() -> Self {
        DigContext { on_ground: true, eyes_in_water: false, aqua_affinity: false, efficiency: 0, haste: None, mining_fatigue: None }
    }
}

/// `InventoryPlayer.getDestroySpeed(state)` for a held stack.
fn inventory_destroy_speed(held: &ItemStack, state: u16) -> f32 {
    let mut f = 1.0f32;
    if !held.is_empty() {
        f *= held.get_destroy_speed(state);
    }
    f
}

/// `EntityPlayer.getDigSpeed(state)` with `held` in the main hand.
pub fn dig_speed(state: u16, held: &ItemStack, ctx: &DigContext) -> f32 {
    let mut f = inventory_destroy_speed(held, state);
    if f > 1.0 {
        let i = ctx.efficiency;
        if i > 0 && !held.is_empty() {
            f += (i * i + 1) as f32;
        }
    }
    if let Some(amp) = ctx.haste {
        f *= 1.0 + (amp + 1) as f32 * 0.2;
    }
    if let Some(amp) = ctx.mining_fatigue {
        let f1 = match amp {
            0 => 0.3f32,
            1 => 0.09,
            2 => 0.0027,
            _ => 8.1E-4,
        };
        f *= f1;
    }
    if ctx.eyes_in_water && !ctx.aqua_affinity {
        f /= 5.0;
    }
    if !ctx.on_ground {
        f /= 5.0;
    }
    f
}

/// `EntityPlayer.canHarvestBlock(state)` (`InventoryPlayer.canHarvestBlock`) with `held` in the
/// main hand: whether breaking the block drops anything.
pub fn can_harvest(state: u16, held: &ItemStack) -> bool {
    if is_tool_not_required(mat(state)) {
        true
    } else {
        !held.is_empty() && held.can_harvest_block(state)
    }
}

/// `Block.getPlayerRelativeBlockHardness(state, player, world, pos)`: the fraction of the block
/// broken per tick. 0 for unbreakable blocks.
pub fn player_relative_block_hardness(state: u16, held: &ItemStack, ctx: &DigContext) -> f32 {
    let f = block_hardness(state);
    if f < 0.0 {
        0.0
    } else if !can_harvest(state, held) {
        dig_speed(state, held, ctx) / f / 100.0
    } else {
        dig_speed(state, held, ctx) / f / 30.0
    }
}

/// Ticks of holding attack until the block breaks, as `PlayerControllerMP` counts them: 0 when
/// `clickBlock` breaks it at once (relative hardness >= 1), else the number of
/// `onPlayerDamageBlock` ticks whose float sum `curBlockDamageMP` reaches 1. `None` when it
/// never does (relative hardness 0).
pub fn ticks_to_break(relative_hardness: f32) -> Option<u32> {
    if relative_hardness >= 1.0 {
        return Some(0);
    }
    if relative_hardness <= 0.0 {
        return None;
    }
    let mut cur = 0.0f32;
    let mut ticks = 0u32;
    while cur < 1.0 {
        cur += relative_hardness;
        ticks += 1;
        if ticks == u32::MAX {
            return None;
        }
    }
    Some(ticks)
}
