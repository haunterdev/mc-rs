//! T6 (sim half): what the server asks when a player digs. The port itself lives in `items`
//! (`items::dig`: `Block.getPlayerRelativeBlockHardness`, `EntityPlayer.getDigSpeed` with the
//! water and airborne divisors, efficiency, haste and fatigue, `canHarvestBlock`;
//! `items::harvest`/`drops`/`drop_rules`: `Block.harvestBlock`, `getItemDropped`,
//! `quantityDropped`); this file gives them the names the server and viewer call and the tick
//! counts `PlayerControllerMP` produces.
//!
//! Tick counts are the float accumulation of `PlayerControllerMP.onPlayerDamageBlock`. Stone by
//! hand is 151 ticks, not the design's 150: `1/1.5/100` summed in f32 stays under 1.0 after 150
//! additions. The source wins.
//!
//! DROPPED: Forge's `ForgeHooks.canHarvestBlock` and `getBreakSpeed` events; the creative branch
//! of `PlayerInteractionManager` (instant break, no drops), which the server handles by game
//! mode; `ItemStack.getDestroySpeed` NBT-enchantment reads (no tag compound in `items`).

pub use items::dig::{block_hardness, can_harvest, dig_speed, player_relative_block_hardness, ticks_to_break};
pub use items::harvest::{harvest_block, player_break_block, Harvest};
pub use items::{DigContext, ItemStack};

/// Ticks of holding attack on `state` with `held` until it breaks (`None`: never, 0: at once).
pub fn break_ticks(state: u16, held: &ItemStack, ctx: &DigContext) -> Option<u32> {
    ticks_to_break(player_relative_block_hardness(state, held, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::ids::{block, item};
    use worldgen::java_random::JavaRandom;

    const fn st(id: u16, meta: u16) -> u16 {
        id << 4 | meta
    }
    fn held(id: u16) -> ItemStack {
        if id == 0 { ItemStack::EMPTY } else { ItemStack::of(id) }
    }
    fn ticks(s: u16, tool: u16, ctx: &DigContext) -> Option<u32> {
        break_ticks(s, &held(tool), ctx)
    }
    fn drops(s: u16, tool: u16) -> Vec<(u16, i32)> {
        let mut h = held(tool);
        let mut r = JavaRandom::new(1);
        player_break_block(s, &mut h, &mut r).drops.stacks().iter().map(|x| (x.get_item(), x.get_count())).collect()
    }
    const WATER: DigContext = DigContext { eyes_in_water: true, ..ctx_default() };
    const AIR: DigContext = DigContext { on_ground: false, ..ctx_default() };
    const fn ctx_default() -> DigContext {
        DigContext { on_ground: true, eyes_in_water: false, aqua_affinity: false, efficiency: 0, haste: None, mining_fatigue: None }
    }

    #[test]
    fn times() {
        let d = ctx_default();
        assert_eq!(ticks(st(block::STONE, 0), 0, &d), Some(151));
        assert_eq!(ticks(st(block::DIRT, 0), 0, &d), Some(15));
        assert_eq!(ticks(st(block::STONE, 0), item::WOODEN_PICKAXE, &d), Some(23));
        assert_eq!(ticks(st(block::OBSIDIAN, 0), item::DIAMOND_PICKAXE, &d), Some(188));
        assert_eq!(ticks(st(block::BEDROCK, 0), item::DIAMOND_PICKAXE, &d), None);
    }

    #[test]
    fn water_and_air_are_five_times_slower() {
        let base = player_relative_block_hardness(st(block::DIRT, 0), &held(0), &ctx_default());
        for c in [&WATER, &AIR] {
            let r = player_relative_block_hardness(st(block::DIRT, 0), &held(0), c);
            assert!((r - base / 5.0).abs() < 1e-6);
        }
        assert_eq!(ticks(st(block::DIRT, 0), 0, &WATER), Some(76));
        assert_eq!(ticks(st(block::DIRT, 0), 0, &AIR), Some(76));
        let aqua = DigContext { aqua_affinity: true, ..WATER };
        assert_eq!(ticks(st(block::DIRT, 0), 0, &aqua), Some(15));
    }

    #[test]
    fn haste_fatigue_efficiency() {
        let haste = DigContext { haste: Some(1), ..ctx_default() };
        assert!(ticks(st(block::DIRT, 0), 0, &haste).unwrap() < 15);
        let fatigue = DigContext { mining_fatigue: Some(0), ..ctx_default() };
        assert!(ticks(st(block::DIRT, 0), 0, &fatigue).unwrap() > 15);
        let eff = DigContext { efficiency: 3, ..ctx_default() };
        assert!(ticks(st(block::STONE, 0), item::WOODEN_PICKAXE, &eff).unwrap() < 23);
        // efficiency only applies once the tool is faster than 1
        assert_eq!(ticks(st(block::STONE, 0), 0, &eff), Some(151));
    }

    #[test]
    fn harvestability() {
        assert!(!can_harvest(st(block::STONE, 0), &held(0)));
        assert!(can_harvest(st(block::STONE, 0), &held(item::WOODEN_PICKAXE)));
        assert!(!can_harvest(st(block::OBSIDIAN, 0), &held(item::IRON_PICKAXE)));
        assert!(can_harvest(st(block::DIRT, 0), &held(0)));
    }

    #[test]
    fn drop_rules() {
        assert!(drops(st(block::STONE, 0), 0).is_empty());
        assert_eq!(drops(st(block::STONE, 0), item::WOODEN_PICKAXE), vec![(block::COBBLESTONE, 1)]);
        assert_eq!(drops(st(block::GRASS, 0), 0), vec![(block::DIRT, 1)]);
        assert!(drops(st(block::GLASS, 0), 0).is_empty());
    }

    #[test]
    fn tool_wears() {
        let mut h = held(item::WOODEN_PICKAXE);
        let mut r = JavaRandom::new(1);
        player_break_block(st(block::STONE, 0), &mut h, &mut r);
        assert_eq!(h.get_metadata(), 1);
    }
}
