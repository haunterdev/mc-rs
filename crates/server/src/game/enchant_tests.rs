//! Enchantments through the server paths: the protection total in the damage path, Efficiency
//! and Silk Touch on a dig. Over the slab world of xp_tests.rs.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{Dig, PlayerState, DIG_FINISH, DIG_START, ON_GROUND};

use crate::game::damage::{self, DamageSource};
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = block::STONE << 4;
const ORE_AT: (i32, i32, i32) = (7, 201, 5);

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.world.lock().unwrap().set_rule("doMobSpawning", "false");
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.tick(1);
    h.frames_for(1);
    h
}

fn enchanted(it: u16, e: u8, level: i32) -> ItemStack {
    let mut s = ItemStack::of(it);
    s.add_enchantment(e, level);
    s
}

fn drop_hit(armor: [ItemStack; 4], kind: DamageSource, amount: f32) -> f32 {
    let (tx, rx) = std::sync::mpsc::channel();
    run(move || {
        let mut h = setup();
        h.game.players.players.get_mut(&1).unwrap().inv.armor = armor;
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        let before = h.game.players.players[&1].health;
        damage::attack_entity_from(&mut h.game, 1, &kind, amount);
        tx.send(before - h.game.players.players[&1].health).unwrap();
    });
    rx.recv().unwrap()
}

#[test]
fn protection_lowers_the_damage_a_player_takes() {
    let none = [ItemStack::EMPTY; 4];
    let plain = [ItemStack::of(item::DIAMOND_BOOTS), ItemStack::EMPTY, ItemStack::EMPTY, ItemStack::EMPTY];
    let prot = [enchanted(item::DIAMOND_BOOTS, ::items::enchant::id::PROTECTION, 4), ItemStack::EMPTY, ItemStack::EMPTY, ItemStack::EMPTY];
    let a = drop_hit(none, DamageSource::generic(), 10.0);
    let b = drop_hit(plain, DamageSource::generic(), 10.0);
    let c = drop_hit(prot, DamageSource::generic(), 10.0);
    assert_eq!(a, 10.0);
    // generic bypasses armor; protection still counts: 10 * (1 - 4 / 25)
    assert_eq!(b, 10.0);
    assert!((c - 8.4).abs() < 1e-4, "{c}");
    // Feather Falling III on fall damage: 9 points, 9 * (1 - 9 / 25)
    let ff = [enchanted(item::DIAMOND_BOOTS, ::items::enchant::id::FEATHER_FALLING, 3), ItemStack::EMPTY, ItemStack::EMPTY, ItemStack::EMPTY];
    let d = drop_hit(ff, DamageSource::fall(), 10.0);
    assert!(d < 10.0 * (1.0 - 8.0 / 25.0) && d > 0.0, "{d}");
}

fn break_ore(h: &mut Harness, state: u16, tool: ItemStack) -> (u32, Vec<ItemStack>) {
    h.game.players.players.get_mut(&1).unwrap().inv.main[0] = tool;
    let held = h.game.players.players[&1].inv.get_current_item();
    let ctx = crate::game::mining::dig_context(&h.game, 1);
    let ticks = sim::mining::break_ticks(state, &held, &ctx).expect("breakable");
    terrain::set_block(&mut h.game, ORE_AT, state);
    h.tick(1);
    let before = h.game.items.entities.len();
    let dig = |h: &mut Harness, action: u8| {
        h.push_frame(1, proto::V_DIG, &Dig { action, pos: [ORE_AT.0, ORE_AT.1, ORE_AT.2], face: 1, state }.encode());
    };
    dig(h, DIG_START);
    h.tick(ticks.max(1));
    dig(h, DIG_FINISH);
    h.tick(1);
    (ticks, h.game.items.entities[before..].iter().map(|e| e.stack).collect())
}

#[test]
fn efficiency_speeds_the_dig() {
    run(|| {
        let mut h = setup();
        h.game.players.players.get_mut(&1).unwrap().inv.main[0] = enchanted(item::IRON_PICKAXE, ::items::enchant::id::EFFICIENCY, 5);
        let fast = crate::game::mining::dig_context(&h.game, 1);
        assert_eq!(fast.efficiency, 5);
        let held = h.game.players.players[&1].inv.get_current_item();
        let plain = ItemStack::of(item::IRON_PICKAXE);
        let s = block::STONE << 4;
        let a = sim::mining::break_ticks(s, &held, &fast).unwrap();
        let b = sim::mining::break_ticks(s, &plain, &::items::DigContext::default()).unwrap();
        assert!(a < b, "{a} vs {b}");
    });
}

#[test]
fn silk_touch_breaks_an_ore_into_the_ore_block() {
    run(|| {
        let mut h = setup();
        let state = block::COAL_ORE << 4;
        let (_, drops) = break_ore(&mut h, state, enchanted(item::IRON_PICKAXE, ::items::enchant::id::SILK_TOUCH, 1));
        assert_eq!(drops.len(), 1, "{drops:?}");
        assert_eq!(drops[0].item, ::items::registry::item_from_block(block::COAL_ORE));
        assert_eq!(h.game.xp_orbs.orbs.iter().map(|o| o.value).sum::<i32>(), 0);
    });
}

// ---- the enchanting table window ----

const TABLE: [i32; 3] = [7, 201, 6];

fn setup_table() -> Harness {
    let mut h = setup();
    terrain::set_block(&mut h.game, (TABLE[0], TABLE[1], TABLE[2]), block::ENCHANTING_TABLE << 4);
    // The ring two blocks out, both layers: more than the 15 shelves that cap the offers.
    let ring = [(-2, -2), (-2, -1), (-2, 0), (-2, 1), (-2, 2), (2, -2), (2, -1), (2, 0), (2, 1), (2, 2), (-1, -2), (0, -2), (1, -2), (-1, 2), (0, 2), (1, 2)];
    for (x, z) in ring {
        for y in 0..2 {
            terrain::set_block(&mut h.game, (TABLE[0] + x, TABLE[1] + y, TABLE[2] + z), block::BOOKSHELF << 4);
        }
    }
    h.tick(1);
    h.frames_for(1);
    h
}

fn frames_of(frames: &[(u8, Vec<u8>)], tag: u8) -> Vec<&[u8]> {
    frames.iter().filter(|f| f.0 == tag).map(|f| f.1.as_slice()).collect()
}

fn quick_move(h: &mut Harness, window: u8, slot: i16, action: u16) {
    let m = proto::Click { window, slot, button: 0, mode: sim::container::ClickType::QuickMove.ordinal(), action };
    h.push_frame(1, proto::V_CLICK, &m.encode());
}

#[test]
fn the_enchanting_table_window_offers_and_enchants_and_gives_the_items_back() {
    run(|| {
        let mut h = setup_table();
        h.game.players.players.get_mut(&1).unwrap().xp.level = 30;
        h.game.players.players.get_mut(&1).unwrap().inv.main[0] = ItemStack::of(item::DIAMOND_SWORD);
        h.game.players.players.get_mut(&1).unwrap().inv.main[1] = ItemStack::new(item::DYE, 5, 4);
        h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos: TABLE, face: 1, hit: [0.5; 3], hand: 0, state: 0 }.encode());
        h.tick(1);
        let frames = h.frames_for(1);
        let o = proto::OpenWindow::decode(frames_of(&frames, proto::S_OPEN_WINDOW)[0]).unwrap();
        assert_eq!((o.window, o.kind, o.pos, o.title.as_str()), (1, proto::WK_ENCHANTING, TABLE, "container.enchant"));
        // 2 table slots, 27 main, 9 hotbar, then the cursor
        assert_eq!(proto::WindowSlots::decode(frames_of(&frames, proto::S_SLOTS)[0]).unwrap().slots.len(), 39);
        // The sword (hotbar slot 29) and the lapis (slot 30) go in by shift-click.
        quick_move(&mut h, 1, 29, 0);
        quick_move(&mut h, 1, 30, 1);
        h.tick(1);
        let frames = h.frames_for(1);
        let props: Vec<proto::WindowProp> = frames_of(&frames, proto::S_WINDOW_PROP).iter().map(|b| proto::WindowProp::decode(b).unwrap()).collect();
        assert!(props.iter().any(|p| p.window == 1 && p.property == 2 && p.value == 30), "15 bookshelves: a level 30 bottom offer {props:?}");
        // The third button costs 3 levels and 3 lapis.
        h.push_frame(1, proto::V_ENCHANT, &proto::EnchantItem { window: 1, button: 2 }.encode());
        h.tick(1);
        let frames = h.frames_for(1);
        assert_eq!(h.game.players.players[&1].xp.level, 27);
        assert!(frames_of(&frames, proto::S_XP).len() >= 1, "the new level goes out");
        let t = h.game.windows.players[&1].open.as_ref().unwrap().enchant_ref().clone();
        assert!(t.items[0].is_item_enchanted());
        assert_eq!(t.items[1].get_count(), 2);
        // Another button with too few levels left (27 < 30 would refuse the third again).
        h.push_frame(1, proto::V_ENCHANT, &proto::EnchantItem { window: 1, button: 2 }.encode());
        h.tick(1);
        h.frames_for(1);
        assert_eq!(h.game.players.players[&1].xp.level, 27, "an enchanted item has no offers");
        // Closing returns the sword and the lapis to the inventory.
        h.push_frame(1, proto::V_CLOSE, &proto::CloseWindow { window: 1 }.encode());
        h.tick(1);
        h.frames_for(1);
        let inv = &h.game.players.players[&1].inv;
        let all: Vec<ItemStack> = inv.main.iter().copied().collect();
        assert!(all.iter().any(|s| s.get_item() == item::DIAMOND_SWORD && s.is_item_enchanted()), "{:?} {:?}", all.iter().filter(|s| !s.is_empty()).collect::<Vec<_>>(), h.game.windows.players[&1].open.is_some());
        assert!(all.iter().any(|s| s.get_item() == item::DYE && s.get_count() == 2));
    });
}

#[test]
fn too_few_levels_refuse_an_offer_and_the_seed_is_saved_with_the_player() {
    run(|| {
        let mut h = setup_table();
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            p.xp.level = 29;
            p.inv.main[0] = ItemStack::of(item::DIAMOND_SWORD);
            p.inv.main[1] = ItemStack::new(item::DYE, 5, 4);
        }
        h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos: TABLE, face: 1, hit: [0.5; 3], hand: 0, state: 0 }.encode());
        h.tick(1);
        h.frames_for(1);
        quick_move(&mut h, 1, 29, 0);
        quick_move(&mut h, 1, 30, 1);
        h.tick(1);
        h.frames_for(1);
        h.push_frame(1, proto::V_ENCHANT, &proto::EnchantItem { window: 1, button: 2 }.encode());
        h.tick(1);
        h.frames_for(1);
        assert_eq!(h.game.players.players[&1].xp.level, 29);
        assert!(!h.game.windows.players[&1].open.as_ref().unwrap().enchant_ref().items[0].is_item_enchanted());
        // The offers for one seed repeat: reopening shows the same bottom offer.
        let seed = h.game.players.players[&1].xp_seed;
        let json = crate::game::playerdata::to_json(&h.game.players.players[&1]);
        assert_eq!(json["xp_seed"].as_i64(), Some(seed as i64));
        let p = h.game.players.players.get_mut(&1).unwrap();
        p.xp_seed = seed.wrapping_add(1);
        crate::game::playerdata::from_json(p, &json);
        assert_eq!(p.xp_seed, seed);
    });
}
