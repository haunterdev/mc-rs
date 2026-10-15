//! Ores, smelting and tool tiers end to end over a seeded world. Generation is read from
//! the chunks the terrain made; mining, the furnace and crafting go through the real server hooks.
//!
//! The experience orbs these make are tested in xp_tests.rs; the tests here pin the xp tables.

use ::items::ids::{block, item};
use ::items::{CraftingGrid, DigContext, ItemStack, Recipes};
use proto::{Dig, PlayerState, DIG_FINISH, DIG_START, ON_GROUND};

use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = block::STONE << 4;
const AT: (i32, i32, i32) = (7, 201, 5);

fn setup(tool: ItemStack) -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.join(2, "Other", [6.5, 230.0, 6.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    for id in 1..=2 {
        h.game.sessions.get_mut(&id).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    }
    h.game.players.players.get_mut(&1).unwrap().inv.main[0] = tool;
    h.tick(1);
    h.frames_for(1);
    h.frames_for(2);
    h
}

fn dig(h: &mut Harness, action: u8, state: u16) {
    let d = Dig { action, pos: [AT.0, AT.1, AT.2], face: 1, state };
    h.push_frame(1, proto::V_DIG, &d.encode());
}

/// Holds attack on `state` at AT for exactly the ticks the held tool needs, then finishes.
fn mine(h: &mut Harness, state: u16) {
    let held = h.game.players.players[&1].inv.get_current_item();
    let ticks = sim::mining::break_ticks(state, &held, &DigContext::default()).expect("breakable");
    terrain::set_block(&mut h.game, AT, state);
    h.tick(1);
    h.frames_for(1);
    dig(h, DIG_START, state);
    h.tick(1);
    if ticks > 1 {
        h.tick(ticks - 1);
    }
    dig(h, DIG_FINISH, state);
    h.tick(1);
}

fn dropped(h: &Harness, it: u16) -> i32 {
    h.game.items.entities.iter().filter(|e| e.stack.get_item() == it).map(|e| e.stack.get_count()).sum()
}

fn total_dropped(h: &Harness) -> i32 {
    h.game.items.entities.iter().map(|e| e.stack.get_count()).sum()
}

fn mined(tool: u16, ore: u16) -> Harness {
    let mut h = setup(ItemStack::of(tool));
    mine(&mut h, ore << 4);
    h
}

#[test]
fn ores_generate_at_the_decorator_heights() {
    run(|| {
        let mut h = Harness::new(7, 3);
        h.join(1, "Xy", [0.5, 230.0, 0.5]);
        h.settle();
        // (block, min y, max y reached by a vein: the generator's max plus the vein radius)
        let ores = [
            (block::COAL_ORE, 0, 132),
            (block::IRON_ORE, 0, 68),
            (block::GOLD_ORE, 0, 36),
            (block::REDSTONE_ORE, 0, 20),
            (block::DIAMOND_ORE, 0, 20),
            (block::LAPIS_ORE, 0, 36),
        ];
        let mut found = [0usize; 6];
        let mut emerald = 0;
        for x in -40..40 {
            for z in -40..40 {
                for y in 0..140 {
                    let b = terrain::state(&h.game, (x, y, z)) >> 4;
                    if let Some(i) = ores.iter().position(|o| o.0 == b) {
                        found[i] += 1;
                        assert!(y >= ores[i].1 && y <= ores[i].2, "block {b} at y {y}");
                    } else if b == block::EMERALD_ORE {
                        emerald += 1;
                        let biome = terrain::biome(&h.game, x, z).expect("a held chunk");
                        assert!(matches!(biome.kind, worldgen::biome::Kind::Hills(_)), "emerald in {}", biome.name);
                    }
                }
            }
        }
        for (i, o) in ores.iter().enumerate() {
            assert!(found[i] > 0, "no ore {} in the generated chunks", o.0);
        }
        // coal is the most common, diamond the rarest of the veins
        assert!(found[0] > found[1] && found[1] > found[3] && found[3] > found[4], "{found:?} emerald {emerald}");
    });
}

#[test]
fn harvest_tiers_follow_the_pickaxe_material() {
    let can = |tool: u16, b: u16| ItemStack::of(tool).can_harvest_block(b << 4);
    assert!(!can(item::WOODEN_PICKAXE, block::IRON_ORE));
    assert!(can(item::STONE_PICKAXE, block::IRON_ORE));
    assert!(can(item::STONE_PICKAXE, block::LAPIS_ORE));
    for b in [block::GOLD_ORE, block::DIAMOND_ORE, block::REDSTONE_ORE, block::EMERALD_ORE] {
        assert!(!can(item::STONE_PICKAXE, b), "stone on {b}");
        assert!(can(item::IRON_PICKAXE, b), "iron on {b}");
    }
    assert!(!can(item::IRON_PICKAXE, block::OBSIDIAN));
    assert!(can(item::DIAMOND_PICKAXE, block::OBSIDIAN));
    assert!(can(item::WOODEN_PICKAXE, block::STONE));
    assert!(can(item::WOODEN_PICKAXE, block::COAL_ORE));
    assert!(!ItemStack::EMPTY.can_harvest_block(block::STONE << 4));
}

#[test]
fn too_low_a_tier_digs_a_hundred_over_instead_of_thirty() {
    let ctx = DigContext::default();
    let iron = ItemStack::of(item::IRON_PICKAXE);
    let wood = ItemStack::of(item::WOODEN_PICKAXE);
    let s = block::DIAMOND_ORE << 4;
    let f = sim::mining::block_hardness(s);
    assert_eq!(sim::mining::player_relative_block_hardness(s, &iron, &ctx), sim::mining::dig_speed(s, &iron, &ctx) / f / 30.0);
    assert_eq!(sim::mining::player_relative_block_hardness(s, &wood, &ctx), sim::mining::dig_speed(s, &wood, &ctx) / f / 100.0);
}

#[test]
fn too_low_a_tier_drops_nothing() {
    run(|| {
        let h = mined(item::WOODEN_PICKAXE, block::IRON_ORE);
        assert_eq!(terrain::state(&h.game, AT), 0);
        assert_eq!(total_dropped(&h), 0);
        let h = mined(item::STONE_PICKAXE, block::DIAMOND_ORE);
        assert_eq!(terrain::state(&h.game, AT), 0);
        assert_eq!(total_dropped(&h), 0);
    });
}

#[test]
fn ore_drops() {
    run(|| {
        assert_eq!(dropped(&mined(item::WOODEN_PICKAXE, block::COAL_ORE), item::COAL), 1);
        assert_eq!(dropped(&mined(item::STONE_PICKAXE, block::IRON_ORE), block::IRON_ORE), 1);
        assert_eq!(dropped(&mined(item::IRON_PICKAXE, block::GOLD_ORE), block::GOLD_ORE), 1);
        assert_eq!(dropped(&mined(item::IRON_PICKAXE, block::DIAMOND_ORE), item::DIAMOND), 1);
        let r = dropped(&mined(item::IRON_PICKAXE, block::REDSTONE_ORE), item::REDSTONE);
        assert!((4..=5).contains(&r), "redstone {r}");
        let l = dropped(&mined(item::STONE_PICKAXE, block::LAPIS_ORE), item::DYE);
        assert!((4..=8).contains(&l), "lapis dye {l}");
    });
}

#[test]
fn ore_experience_is_in_the_drops() {
    let mut rand = ::items::JavaRandom::new(1);
    for (ore, lo, hi) in [(block::COAL_ORE, 0, 2), (block::DIAMOND_ORE, 3, 7), (block::LAPIS_ORE, 2, 5), (block::REDSTONE_ORE, 1, 5)] {
        let h = ::items::harvest::harvest_block(ore << 4, &ItemStack::EMPTY, &mut rand);
        assert!((lo..=hi).contains(&h.drops.xp), "ore {ore} xp {}", h.drops.xp);
    }
}

#[test]
fn smelting_one_ore_takes_200_ticks_and_coal_burns_1600() {
    run(|| {
        assert_eq!(::items::smelting::get_item_burn_time(&ItemStack::of(item::COAL)), 1600);
        assert_eq!(::items::smelting::get_smelting_experience(&ItemStack::of(item::IRON_INGOT)), 0.7);
        assert_eq!(::items::smelting::furnace_output_xp(&ItemStack::of(item::IRON_INGOT), 10, || 0.99), 7);
        let mut h = setup(ItemStack::EMPTY);
        let at = (7, 201, 6);
        terrain::set_block(&mut h.game, at, block::FURNACE << 4 | 2);
        let f = h.game.tiles.furnaces.get_mut(&at).unwrap();
        f.set_inventory_slot_contents(0, ItemStack::new(block::IRON_ORE, 2, 0));
        f.set_inventory_slot_contents(1, ItemStack::of(item::COAL));
        h.tick(100);
        assert!(h.game.tiles.furnaces[&at].items[2].is_empty());
        h.tick(120);
        let f = &h.game.tiles.furnaces[&at];
        assert_eq!(f.items[2], ItemStack::new(item::IRON_INGOT, 1, 0), "one ingot by tick 220");
        assert_eq!(f.items[0].get_count(), 1);
        assert!(f.burn_time > 1000, "the coal is still burning: {}", f.burn_time);
    });
}

#[test]
fn a_full_output_slot_stops_the_smelt() {
    let mut f = ::items::Furnace::default();
    f.set_inventory_slot_contents(0, ItemStack::new(block::IRON_ORE, 1, 0));
    f.set_inventory_slot_contents(2, ItemStack::new(item::IRON_INGOT, 64, 0));
    assert!(!f.can_smelt());
    f.set_inventory_slot_contents(2, ItemStack::new(item::IRON_INGOT, 63, 0));
    assert!(f.can_smelt());
}

fn pickaxe_recipes() -> Recipes {
    let mut files = Vec::new();
    for (name, mat, res) in [("w", "planks", "wooden_pickaxe"), ("s", "cobblestone", "stone_pickaxe"), ("i", "iron_ingot", "iron_pickaxe"), ("d", "diamond", "diamond_pickaxe")] {
        let data = if mat == "planks" { r#","data":32767"# } else { "" };
        let json = format!(
            r##"{{"type":"crafting_shaped","pattern":["XXX"," # "," # "],"key":{{"X":{{"item":"minecraft:{mat}"{data}}},"#":{{"item":"minecraft:stick"}}}},"result":{{"item":"minecraft:{res}"}}}}"##
        );
        files.push((name.to_string(), json));
    }
    Recipes::from_files(files.iter().map(|(n, j)| (n.as_str(), j.as_bytes()))).unwrap()
}

#[test]
fn tier_pickaxes_craft_with_their_durability() {
    let r = pickaxe_recipes();
    let stick = ItemStack::of(item::STICK);
    for (head, want, uses) in [
        (ItemStack::of(block::PLANKS), item::WOODEN_PICKAXE, 59),
        (ItemStack::of(block::COBBLESTONE), item::STONE_PICKAXE, 131),
        (ItemStack::of(item::IRON_INGOT), item::IRON_PICKAXE, 250),
        (ItemStack::of(item::DIAMOND), item::DIAMOND_PICKAXE, 1561),
    ] {
        let e = ItemStack::EMPTY;
        let grid = CraftingGrid::from_rows(3, &[&[head, head, head], &[e, stick, e], &[e, stick, e]]);
        let out = r.find_matching_result(&grid);
        assert_eq!(out.get_item(), want);
        assert_eq!(out.get_max_damage(), uses);
    }
}

#[test]
fn a_pickaxe_loses_one_per_block_and_breaks_at_the_limit_with_the_sound() {
    run(|| {
        let mut h = setup(ItemStack::of(item::WOODEN_PICKAXE));
        mine(&mut h, STONE);
        assert_eq!(h.game.players.players[&1].inv.main[0].get_metadata(), 1);
        h.game.players.players.get_mut(&1).unwrap().inv.main[0] = ItemStack::new(item::WOODEN_PICKAXE, 1, 59);
        h.tick(1);
        h.frames_for(2);
        mine(&mut h, STONE);
        assert!(h.game.players.players[&1].inv.main[0].is_empty(), "the pickaxe broke");
        let sounds: Vec<_> = h.frames_for(2).into_iter().filter(|f| f.0 == proto::S_SOUND).map(|f| proto::Sound::decode(&f.1).unwrap().name).collect();
        assert!(sounds.iter().any(|n| n == "entity.item.break"), "{sounds:?}");
    });
}
