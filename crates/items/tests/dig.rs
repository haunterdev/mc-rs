//! Hardness, destroy speed and harvesting, with the expected values worked out by hand from
//! `Block.getPlayerRelativeBlockHardness`, `EntityPlayer.getDigSpeed`, the tool classes and
//! `PlayerControllerMP`'s float accumulation (each step rounded to float, as Java does).

use items::dig::{block_hardness, can_harvest, dig_speed, player_relative_block_hardness, ticks_to_break};
use items::ids::{block, item};
use items::tools::{can_harvest_block, destroy_speed, on_block_destroyed_damage};
use items::{DigContext, ItemStack};

const fn st(id: u16, meta: u16) -> u16 {
    id << 4 | meta
}

fn rel(state: u16, held: u16) -> f32 {
    let h = if held == 0 { ItemStack::EMPTY } else { ItemStack::of(held) };
    player_relative_block_hardness(state, &h, &DigContext::default())
}

#[test]
fn hardness_table() {
    assert_eq!(block_hardness(st(block::STONE, 0)), 1.5);
    assert_eq!(block_hardness(st(block::OBSIDIAN, 0)), 50.0);
    assert_eq!(block_hardness(st(block::BEDROCK, 0)), -1.0);
    assert_eq!(block_hardness(st(block::DIRT, 2)), 0.5);
    assert_eq!(block_hardness(st(block::LOG, 0)), 2.0);
    assert_eq!(block_hardness(st(block::LEAVES, 0)), 0.2);
    assert_eq!(block_hardness(st(block::TALLGRASS, 1)), 0.0);
    assert_eq!(block_hardness(st(block::WEB, 0)), 4.0);
    assert_eq!(block_hardness(st(block::ENDER_CHEST, 0)), 22.5);
}

#[test]
fn tool_destroy_speed_and_harvest() {
    let stone = st(block::STONE, 0);
    // ItemPickaxe: efficiency on ROCK/IRON/ANVIL
    assert_eq!(destroy_speed(item::WOODEN_PICKAXE, stone), 2.0);
    assert_eq!(destroy_speed(item::GOLDEN_PICKAXE, stone), 12.0);
    assert_eq!(destroy_speed(item::DIAMOND_PICKAXE, st(block::IRON_BLOCK, 0)), 8.0);
    assert_eq!(destroy_speed(item::DIAMOND_PICKAXE, st(block::DIRT, 0)), 1.0);
    // ItemPickaxe EFFECTIVE_ON includes ice (material ICE)
    assert_eq!(destroy_speed(item::STONE_PICKAXE, st(block::ICE, 0)), 4.0);
    // ItemAxe: WOOD/PLANTS/VINE materials and its effective set
    assert_eq!(destroy_speed(item::IRON_AXE, st(block::LOG, 0)), 6.0);
    assert_eq!(destroy_speed(item::IRON_AXE, st(block::MELON_BLOCK, 0)), 6.0);
    assert_eq!(destroy_speed(item::IRON_AXE, st(block::STONE, 0)), 1.0);
    // ItemSpade: only its effective set
    assert_eq!(destroy_speed(item::DIAMOND_SHOVEL, st(block::GRASS, 0)), 8.0);
    assert_eq!(destroy_speed(item::DIAMOND_SHOVEL, st(block::CLAY, 0)), 8.0);
    assert_eq!(destroy_speed(item::DIAMOND_SHOVEL, st(block::SANDSTONE, 0)), 1.0);
    // ItemSword: web 15, plants/vine/coral/leaves/gourd 1.5
    assert_eq!(destroy_speed(item::WOODEN_SWORD, st(block::WEB, 0)), 15.0);
    assert_eq!(destroy_speed(item::WOODEN_SWORD, st(block::LEAVES, 0)), 1.5);
    assert_eq!(destroy_speed(item::WOODEN_SWORD, st(block::PUMPKIN, 0)), 1.5);
    assert_eq!(destroy_speed(item::WOODEN_SWORD, st(block::STONE, 0)), 1.0);
    // ItemShears: web and leaves 15, wool 5
    assert_eq!(destroy_speed(item::SHEARS, st(block::LEAVES2, 1)), 15.0);
    assert_eq!(destroy_speed(item::SHEARS, st(block::WOOL, 4)), 5.0);
    assert_eq!(destroy_speed(item::STICK, stone), 1.0);

    // ItemPickaxe.canHarvestBlock harvest levels
    assert!(can_harvest_block(item::WOODEN_PICKAXE, stone));
    assert!(!can_harvest_block(item::WOODEN_PICKAXE, st(block::IRON_ORE, 0)));
    assert!(can_harvest_block(item::STONE_PICKAXE, st(block::IRON_ORE, 0)));
    assert!(!can_harvest_block(item::STONE_PICKAXE, st(block::DIAMOND_ORE, 0)));
    assert!(can_harvest_block(item::IRON_PICKAXE, st(block::DIAMOND_ORE, 0)));
    assert!(can_harvest_block(item::IRON_PICKAXE, st(block::REDSTONE_ORE, 0)));
    assert!(!can_harvest_block(item::IRON_PICKAXE, st(block::OBSIDIAN, 0)));
    assert!(can_harvest_block(item::DIAMOND_PICKAXE, st(block::OBSIDIAN, 0)));
    assert!(!can_harvest_block(item::GOLDEN_PICKAXE, st(block::GOLD_ORE, 0)), "gold is level 0");
    // ItemSpade only harvests snow; the axe harvests nothing by itself
    assert!(can_harvest_block(item::WOODEN_SHOVEL, st(block::SNOW_LAYER, 0)));
    assert!(!can_harvest_block(item::WOODEN_SHOVEL, st(block::DIRT, 0)));
    assert!(!can_harvest_block(item::DIAMOND_AXE, st(block::LOG, 0)));
    assert!(can_harvest_block(item::STONE_SWORD, st(block::WEB, 0)));
    assert!(can_harvest_block(item::SHEARS, st(block::REDSTONE_WIRE, 0)));

    // EntityPlayer.canHarvestBlock: tool-free materials always, else the held item
    assert!(can_harvest(st(block::DIRT, 0), &ItemStack::EMPTY));
    assert!(can_harvest(st(block::LOG, 0), &ItemStack::EMPTY));
    assert!(!can_harvest(stone, &ItemStack::EMPTY));
    assert!(!can_harvest(st(block::SNOW_LAYER, 0), &ItemStack::EMPTY), "SNOW requires a tool");
    assert!(!can_harvest(st(block::WEB, 0), &ItemStack::of(item::DIAMOND_PICKAXE)));

    // onBlockDestroyed wear
    assert_eq!(on_block_destroyed_damage(item::IRON_PICKAXE, stone), 1);
    assert_eq!(on_block_destroyed_damage(item::IRON_PICKAXE, st(block::TALLGRASS, 1)), 0);
    assert_eq!(on_block_destroyed_damage(item::IRON_SWORD, stone), 2);
    assert_eq!(on_block_destroyed_damage(item::SHEARS, st(block::TALLGRASS, 1)), 1);
    assert_eq!(on_block_destroyed_damage(item::STICK, stone), 0);
}

#[test]
fn relative_hardness_and_ticks() {
    // bare hand on stone: 1 / 1.5 / 100, and 151 ticks of float accumulation (not 150)
    assert_eq!(rel(st(block::STONE, 0), 0), 0.006_666_667);
    assert_eq!(ticks_to_break(rel(st(block::STONE, 0), 0)), Some(151));
    // wooden pickaxe on stone: 2 / 1.5 / 30
    assert_eq!(rel(st(block::STONE, 0), item::WOODEN_PICKAXE), 0.044_444_446);
    assert_eq!(ticks_to_break(rel(st(block::STONE, 0), item::WOODEN_PICKAXE)), Some(23));
    // diamond pickaxe on obsidian: 8 / 50 / 30 -> 188 ticks
    assert_eq!(ticks_to_break(rel(st(block::OBSIDIAN, 0), item::DIAMOND_PICKAXE)), Some(188));
    // iron pickaxe on obsidian cannot harvest: 6 / 50 / 100 -> 834 ticks
    assert_eq!(rel(st(block::OBSIDIAN, 0), item::IRON_PICKAXE), 0.001_199_999_9);
    assert_eq!(ticks_to_break(rel(st(block::OBSIDIAN, 0), item::IRON_PICKAXE)), Some(834));
    // diamond axe on a log: 8 / 2 / 30 -> 8 ticks
    assert_eq!(ticks_to_break(rel(st(block::LOG, 0), item::DIAMOND_AXE)), Some(8));
    // sword on web: 15 / 4 / 30 = 0.125 -> 8 ticks
    assert_eq!(rel(st(block::WEB, 0), item::IRON_SWORD), 0.125);
    // hand on dirt 15 ticks, on grass (hardness 0.6F) 18 ticks
    assert_eq!(ticks_to_break(rel(st(block::DIRT, 0), 0)), Some(15));
    assert_eq!(ticks_to_break(rel(st(block::GRASS, 0), 0)), Some(18));
    // diamond shovel on dirt: 8 / 0.5 / 30 -> 2 ticks
    assert_eq!(ticks_to_break(rel(st(block::DIRT, 0), item::DIAMOND_SHOVEL)), Some(2));
    // shears on leaves: 15 / 0.2 / 30 >= 1, broken on the click
    assert_eq!(ticks_to_break(rel(st(block::LEAVES, 0), item::SHEARS)), Some(0));
    // zero hardness breaks at once (infinity), bedrock never
    assert_eq!(ticks_to_break(rel(st(block::TALLGRASS, 1), 0)), Some(0));
    assert_eq!(rel(st(block::BEDROCK, 0), item::DIAMOND_PICKAXE), 0.0);
    assert_eq!(ticks_to_break(0.0), None);
    // snow layer by hand: SNOW needs a tool, so / 100: 1 / 0.1 / 100 -> 10 ticks
    assert_eq!(ticks_to_break(rel(st(block::SNOW_LAYER, 0), 0)), Some(10));
}

#[test]
fn dig_speed_conditions() {
    let stone = st(block::STONE, 0);
    let dirt = st(block::DIRT, 0);
    let pick = ItemStack::of(item::DIAMOND_PICKAXE);
    let hand = ItemStack::EMPTY;
    // off the ground and with eyes in water: / 5 each
    let air_water = DigContext { on_ground: false, eyes_in_water: true, ..DigContext::default() };
    assert_eq!(dig_speed(dirt, &hand, &air_water), 0.04);
    let aqua = DigContext { aqua_affinity: true, ..air_water };
    assert_eq!(dig_speed(dirt, &hand, &aqua), 0.2);
    // Efficiency V: 8 + 5 * 5 + 1 = 34; only when the tool speed is above 1
    let eff = DigContext { efficiency: 5, ..DigContext::default() };
    assert_eq!(dig_speed(stone, &pick, &eff), 34.0);
    assert_eq!(player_relative_block_hardness(stone, &pick, &eff), 0.755_555_5);
    assert_eq!(dig_speed(dirt, &pick, &eff), 1.0);
    // Haste II (amplifier 1): * (1 + 2 * 0.2)
    let haste = DigContext { haste: Some(1), ..DigContext::default() };
    assert_eq!(dig_speed(stone, &pick, &haste), 8.0 * 1.4f32);
    // Mining Fatigue I: * 0.3
    let fatigue = DigContext { mining_fatigue: Some(0), ..DigContext::default() };
    assert_eq!(dig_speed(stone, &pick, &fatigue), 8.0 * 0.3f32);
}
