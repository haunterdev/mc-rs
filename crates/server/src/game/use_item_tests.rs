//! `V_USE_ITEM` over the slab world of use_on_tests.rs: stone at y 200 under x 4..=13 and
//! z 4..=13, the player at (5.5, 201, 5.5). Eating, milk, the bow and the buckets.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND};
use sim::kinds::ZOMBIE;

use crate::game::mobs::summon;
use crate::game::terrain;
use crate::game::testing::{run, Harness};

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    {
        let mut w = h.world.lock().unwrap();
        w.set_rule("doMobSpawning", "false");
        w.set_rule("doDaylightCycle", "false");
        w.set_time(18000);
    }
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=24 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), block::STONE << 4);
        }
    }
    look(&mut h, 0.0, 0.0);
    h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
    h.tick(1);
    h.frames_for(1);
    h
}

/// The player stands at (5.5, 201, 5.5) facing `yaw`, `pitch`.
fn look(h: &mut Harness, yaw: f32, pitch: f32) {
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], yaw, pitch, flags: ON_GROUND, ..Default::default() });
}

fn hold(h: &mut Harness, s: ItemStack) {
    let p = h.game.players.players.get_mut(&1).unwrap();
    let c = p.inv.current_item;
    p.inv.main[c] = s;
}

fn in_hand(h: &Harness) -> ItemStack {
    let p = &h.game.players.players[&1];
    p.inv.main[p.inv.current_item]
}

fn use_item(h: &mut Harness, phase: u8) {
    h.push_frame(1, proto::V_USE_ITEM, &proto::UseItem { hand: proto::HAND_MAIN, phase }.encode());
}

fn use_item_hand(h: &mut Harness, hand: u8, phase: u8) {
    h.push_frame(1, proto::V_USE_ITEM, &proto::UseItem { hand, phase }.encode());
}

fn hold_off(h: &mut Harness, s: ItemStack) {
    h.game.players.players.get_mut(&1).unwrap().inv.off_hand[0] = s;
}

fn hungry(h: &mut Harness, level: i32) {
    let f = &mut h.game.players.players.get_mut(&1).unwrap().food;
    f.level = level;
    f.saturation = 0.0;
}

fn food(h: &Harness) -> (i32, f32) {
    let f = &h.game.players.players[&1].food;
    (f.level, f.saturation)
}

fn count_of(h: &Harness, it: u16) -> i32 {
    let inv = &h.game.players.players[&1].inv;
    (0..inv.get_size_inventory()).map(|i| inv.get_stack_in_slot(i)).filter(|s| s.get_item() == it).map(|s| s.count).sum()
}

fn arrows(h: &Harness) -> usize {
    h.game.mobs.arrows.len()
}

fn give_bow(h: &mut Harness, n: i32) {
    hold(h, ItemStack::of(item::BOW));
    let mut a = ItemStack::of(item::ARROW);
    a.count = n;
    h.game.players.players.get_mut(&1).unwrap().inv.main[9] = a;
}

#[test]
fn a_hungry_player_eats_bread_over_32_ticks() {
    run(|| {
        let mut h = setup();
        hungry(&mut h, 10);
        hold(&mut h, ItemStack::of(item::BREAD));
        use_item(&mut h, proto::USE_START);
        h.tick(31);
        assert_eq!(food(&h), (10, 0.0), "not finished at tick 31");
        assert_eq!(in_hand(&h).get_item(), item::BREAD);
        h.tick(1);
        // addStats(5, 0.6): food 15, saturation 5 * 0.6 * 2 = 6.
        assert_eq!(food(&h), (15, 6.0));
        assert!(in_hand(&h).is_empty(), "the bread is used up");
        assert!(h.game.use_item.active.is_empty());
        let sounds = h.frames_for(1).iter().filter(|(t, _)| *t == proto::S_SOUND).count();
        assert!(sounds >= 6, "eat sounds and the burp: {sounds}");
    });
}

/// A `V_USE_ITEM` naming the off hand eats from slot 45 (`InventoryPlayer.offHandInventory`).
#[test]
fn a_use_packet_with_the_off_hand_eats_from_slot_45() {
    run(|| {
        let mut h = setup();
        hungry(&mut h, 10);
        hold_off(&mut h, ItemStack::of(item::BREAD));
        use_item_hand(&mut h, proto::HAND_OFF, proto::USE_START);
        h.tick(31);
        assert_eq!(food(&h), (10, 0.0), "not finished at tick 31");
        h.tick(1);
        assert_eq!(food(&h), (15, 6.0));
        assert!(h.game.players.players[&1].inv.off_hand[0].is_empty(), "the bread came from the off hand");
        assert!(in_hand(&h).is_empty(), "the main hand stays empty");
        assert!(h.game.use_item.active.is_empty());
    });
}

#[test]
fn a_full_player_cannot_eat_bread_but_can_eat_a_golden_apple() {
    run(|| {
        let mut h = setup();
        let mut bread = ItemStack::of(item::BREAD);
        bread.count = 2;
        hold(&mut h, bread);
        use_item(&mut h, proto::USE_START);
        h.tick(40);
        assert_eq!(in_hand(&h).count, 2);
        assert!(h.game.use_item.active.is_empty());
        let mut apple = ItemStack::of(item::GOLDEN_APPLE);
        apple.count = 2;
        hold(&mut h, apple);
        use_item(&mut h, proto::USE_START);
        h.tick(1);
        assert!(h.game.use_item.active.contains_key(&1));
        h.tick(31);
        assert_eq!(in_hand(&h).count, 1, "one golden apple eaten");
        assert_eq!(food(&h).0, 20);
    });
}

#[test]
fn a_creative_player_eats_a_golden_apple_at_full_hunger() {
    run(|| {
        let mut h = setup();
        h.send(crate::game::GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        h.tick(1);
        h.frames_for(1);
        // The default food bar is full, so a survival player could not eat plain food at all.
        assert_eq!(food(&h), (20, 5.0));
        hold(&mut h, ItemStack::of(item::GOLDEN_APPLE));
        use_item(&mut h, proto::USE_START);
        h.tick(1);
        // CHANGED: creative eats at any hunger, by request.
        assert!(h.game.use_item.active.contains_key(&1), "creative starts the use");
        h.tick(31);
        assert!(in_hand(&h).is_empty(), "the apple is eaten");
        // The hidden food and saturation numbers stay where they were.
        assert_eq!(food(&h), (20, 5.0));
        // The food's effects still land.
        let p = &h.game.players.players[&1];
        assert!(p.effects.get(::items::potion::Effect::Regeneration).is_some(), "regeneration");
        assert!(p.effects.get(::items::potion::Effect::Absorption).is_some(), "absorption");
    });
}

#[test]
fn releasing_early_eats_nothing() {
    run(|| {
        let mut h = setup();
        hungry(&mut h, 10);
        hold(&mut h, ItemStack::of(item::BREAD));
        use_item(&mut h, proto::USE_START);
        h.tick(10);
        use_item(&mut h, proto::USE_RELEASE);
        h.tick(40);
        assert_eq!(food(&h), (10, 0.0));
        assert_eq!(in_hand(&h).get_item(), item::BREAD);
    });
}

#[test]
fn mushroom_stew_leaves_a_bowl() {
    run(|| {
        let mut h = setup();
        hungry(&mut h, 10);
        hold(&mut h, ItemStack::of(item::MUSHROOM_STEW));
        use_item(&mut h, proto::USE_START);
        h.tick(32);
        assert_eq!(in_hand(&h).get_item(), item::BOWL);
        assert_eq!(food(&h).0, 16);
    });
}

#[test]
fn milk_is_drunk_over_32_ticks_and_leaves_a_bucket() {
    run(|| {
        let mut h = setup();
        hold(&mut h, ItemStack::of(item::MILK_BUCKET));
        use_item(&mut h, proto::USE_START);
        h.tick(31);
        assert_eq!(in_hand(&h).get_item(), item::MILK_BUCKET);
        h.tick(1);
        assert_eq!(in_hand(&h).get_item(), item::BUCKET);
    });
}

#[test]
fn swapping_the_item_cancels_the_use() {
    run(|| {
        let mut h = setup();
        hungry(&mut h, 10);
        hold(&mut h, ItemStack::of(item::BREAD));
        use_item(&mut h, proto::USE_START);
        h.tick(5);
        hold(&mut h, ItemStack::of(item::APPLE));
        h.tick(40);
        assert_eq!(food(&h), (10, 0.0));
        assert!(h.game.use_item.active.is_empty());
    });
}

#[test]
fn a_full_draw_shoots_one_arrow_and_uses_one_arrow_and_one_durability() {
    run(|| {
        let mut h = setup();
        give_bow(&mut h, 5);
        use_item(&mut h, proto::USE_START);
        h.tick(22);
        assert_eq!(arrows(&h), 0);
        use_item(&mut h, proto::USE_RELEASE);
        h.tick(1);
        assert_eq!(arrows(&h), 1);
        let a = &h.game.mobs.arrows[0];
        assert!(a.critical, "full charge");
        assert_eq!(a.shooter, Some(1));
        assert_eq!(a.pickup, crate::game::mobs::arrow::Pickup::Allowed);
        assert_eq!(count_of(&h, item::ARROW), 4);
        assert_eq!(in_hand(&h).damage, 1);
        assert!(h.game.use_item.active.is_empty());
    });
}

#[test]
fn a_quick_tap_shoots_nothing() {
    run(|| {
        let mut h = setup();
        give_bow(&mut h, 5);
        use_item(&mut h, proto::USE_START);
        use_item(&mut h, proto::USE_RELEASE);
        h.tick(1);
        use_item(&mut h, proto::USE_START);
        h.tick(1);
        use_item(&mut h, proto::USE_RELEASE);
        h.tick(1);
        assert_eq!(arrows(&h), 0);
        assert_eq!(count_of(&h, item::ARROW), 5);
        assert_eq!(in_hand(&h).damage, 0);
    });
}

#[test]
fn without_arrows_a_survival_player_cannot_draw() {
    run(|| {
        let mut h = setup();
        hold(&mut h, ItemStack::of(item::BOW));
        use_item(&mut h, proto::USE_START);
        h.tick(25);
        assert!(h.game.use_item.active.is_empty());
        use_item(&mut h, proto::USE_RELEASE);
        h.tick(1);
        assert_eq!(arrows(&h), 0);
        assert_eq!(in_hand(&h).damage, 0);
    });
}

#[test]
fn an_arrow_hurts_a_zombie_ten_blocks_away() {
    run(|| {
        let mut h = setup();
        let z = summon(&mut h.game, &ZOMBIE, [15.5, 201.0, 5.5]).unwrap();
        let before = h.game.mobs.mobs.iter().find(|m| m.id == z).unwrap().health;
        give_bow(&mut h, 5);
        // Facing east: -sin(yaw) = 1.
        look(&mut h, -90.0, 0.0);
        use_item(&mut h, proto::USE_START);
        h.tick(22);
        use_item(&mut h, proto::USE_RELEASE);
        h.tick(8);
        let after = h.game.mobs.mobs.iter().find(|m| m.id == z).map_or(0.0, |m| m.health);
        assert!(after < before, "{before} -> {after}");
        let me = h.game.players.players[&1].health;
        assert_eq!(me, 20.0, "the shooter is not hurt by its own arrow");
    });
}

#[test]
fn a_stuck_arrow_can_be_picked_up_again() {
    run(|| {
        let mut h = setup();
        give_bow(&mut h, 5);
        // Straight down at the feet.
        look(&mut h, 0.0, 90.0);
        use_item(&mut h, proto::USE_START);
        h.tick(22);
        use_item(&mut h, proto::USE_RELEASE);
        h.tick(1);
        assert_eq!(count_of(&h, item::ARROW), 4);
        h.tick(20);
        assert_eq!(arrows(&h), 0, "picked up");
        assert_eq!(count_of(&h, item::ARROW), 5);
    });
}

#[test]
fn an_empty_bucket_takes_a_still_water_source() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (7, 202, 5), worldgen::blocks::WATER);
        hold(&mut h, ItemStack::of(item::BUCKET));
        look(&mut h, -90.0, 0.0);
        use_item(&mut h, proto::USE_START);
        h.tick(1);
        assert_eq!(in_hand(&h).get_item(), item::WATER_BUCKET);
        assert_eq!(terrain::state(&h.game, (7, 202, 5)) >> 4, 0, "the source is gone");
    });
}

#[test]
fn a_water_bucket_places_water_and_returns_an_empty_bucket() {
    run(|| {
        let mut h = setup();
        hold(&mut h, ItemStack::of(item::WATER_BUCKET));
        // Down onto the stone at the feet: the water goes into the cell above it.
        look(&mut h, 0.0, 90.0);
        use_item(&mut h, proto::USE_START);
        h.tick(1);
        assert_eq!(in_hand(&h).get_item(), item::BUCKET);
        let s = terrain::state(&h.game, (5, 201, 5)) >> 4;
        assert!(s == 8 || s == 9, "water at the feet: {s}");
        h.tick(12);
        // The water runs toward the nearest edge of the slab (BlockDynamicLiquid.getPossibleFlowDirections).
        let flowed = [(4, 201, 5), (5, 201, 4)].iter().any(|&c| terrain::state(&h.game, c) >> 4 == 8);
        assert!(flowed, "the water flowed");
    });
}

#[test]
fn a_creative_player_keeps_the_water_bucket() {
    run(|| {
        let mut h = setup();
        h.send(crate::game::GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        h.tick(1);
        hold(&mut h, ItemStack::of(item::WATER_BUCKET));
        look(&mut h, 0.0, 90.0);
        use_item(&mut h, proto::USE_START);
        h.tick(1);
        assert_eq!(in_hand(&h).get_item(), item::WATER_BUCKET);
        assert_eq!(terrain::state(&h.game, (5, 201, 5)) >> 4, 8);
    });
}
