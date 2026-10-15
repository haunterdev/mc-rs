//! Experience over the slab world of tiles_t8_tests.rs (stone at y 200 under x, z 4..=13,
//! the player at (5.5, 201, 5.5), a furnace at (7, 201, 6)): the level table, orbs from a mob, an
//! ore and a furnace take, the pickup into the bar, and what a death drops.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{Dig, PlayerState, DIG_FINISH, DIG_START, ON_GROUND};
use sim::kinds::ZOMBIE;

use crate::game::damage::{self, Attacker, DamageSource};
use crate::game::mobs::{combat::hurt_mob, summon};
use crate::game::players::Survival;
use crate::game::testing::{run, Harness};
use crate::game::terrain;

const STONE: u16 = block::STONE << 4;
const FURNACE_AT: (i32, i32, i32) = (7, 201, 6);
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

fn orb_total(h: &Harness) -> i32 {
    h.game.xp_orbs.orbs.iter().map(|o| o.value).sum()
}

fn player_src() -> DamageSource {
    DamageSource::player(Attacker { name: "Xy".into(), pos: [5.5, 201.0, 5.5], player: true, id: Some(1) })
}

#[test]
fn the_level_table_is_the_vanilla_one() {
    let mut p = Survival::new();
    p.add_experience(315);
    assert_eq!((p.xp.level, p.xp.total), (15, 315));
    assert_eq!(p.xp.bar, 0.0);
    p.add_experience(1395 - 315);
    assert_eq!((p.xp.level, p.xp.total), (30, 1395));
    // One point at a time reaches the same levels at the same totals.
    let mut q = Survival::new();
    let mut at_level = Vec::new();
    for t in 1..=1395 {
        let before = q.xp.level;
        q.add_experience(1);
        if q.xp.level != before {
            at_level.push((q.xp.level, t));
        }
    }
    // The f32 bar accumulates rounding like the Java float does, so the level can come a point early or late.
    assert!(at_level[14].0 == 15 && (314..=316).contains(&at_level[14].1), "{:?}", at_level[14]);
    assert!(at_level[29].0 == 30 && (1393..=1397).contains(&at_level[29].1), "{:?}", at_level[29]);
    // xpBarCap steps: 7 + 2 * level, 37 + 5 * (level - 15), 112 + 9 * (level - 30).
    let cap = |level: i32| {
        let mut s = Survival::new();
        s.xp.level = level;
        s.xp_bar_cap()
    };
    assert_eq!((cap(0), cap(14), cap(15), cap(29), cap(30), cap(31)), (7, 35, 37, 107, 112, 121));
    // A level taken away below zero clamps everything.
    let mut r = Survival::new();
    r.add_experience(20);
    r.add_experience_level(-50);
    assert_eq!((r.xp.level, r.xp.total, r.xp.bar), (0, 0, 0.0));
}

#[test]
fn killing_a_zombie_drops_orbs_worth_five_that_fill_the_bar() {
    run(|| {
        let mut h = setup();
        let id = summon(&mut h.game, &ZOMBIE, [7.5, 201.0, 6.5]).expect("loaded");
        h.tick(3);
        assert!(hurt_mob(&mut h.game, id, &player_src(), 100.0));
        h.tick(19);
        assert_eq!(orb_total(&h), 0, "the orbs wait for tick 20 of the death");
        h.tick(2);
        assert_eq!(orb_total(&h), 5);
        assert!(h.game.xp_orbs.orbs.iter().all(|o| o.value == 3 || o.value == 1), "getXPSplit(5) is 3, 1, 1");
        h.frames_for(1);
        // The orbs fly to the player and are taken.
        h.tick(120);
        assert_eq!(orb_total(&h), 0, "all picked up");
        let p = h.game.players.players[&1].xp;
        let cap = h.game.players.players[&1].xp_bar_cap();
        assert_eq!((p.level, p.total), (0, 5));
        assert!((p.bar - 5.0 / cap as f32).abs() < 1e-6, "bar {}", p.bar);
        // The client was told: the last S_XP carries the bar and the total.
        let last = h.frames_for(1).into_iter().filter(|f| f.0 == proto::S_XP).last().expect("an S_XP");
        assert_eq!(proto::Xp::decode(&last.1).unwrap(), p);
    });
}

#[test]
fn a_mob_nobody_hit_drops_no_experience() {
    run(|| {
        let mut h = setup();
        let id = summon(&mut h.game, &ZOMBIE, [7.5, 201.0, 6.5]).expect("loaded");
        h.tick(3);
        let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == id).unwrap();
        m.health = 0.0;
        h.tick(25);
        assert_eq!(orb_total(&h), 0);
    });
}

#[test]
fn an_ore_breaks_into_orbs_in_its_range() {
    run(|| {
        let mut h = setup();
        h.game.players.players.get_mut(&1).unwrap().inv.main[0] = ItemStack::of(item::IRON_PICKAXE);
        let state = block::DIAMOND_ORE << 4;
        let held = h.game.players.players[&1].inv.get_current_item();
        let ticks = sim::mining::break_ticks(state, &held, &::items::DigContext::default()).expect("breakable");
        terrain::set_block(&mut h.game, ORE_AT, state);
        h.tick(1);
        let dig = |h: &mut Harness, action: u8| {
            h.push_frame(1, proto::V_DIG, &Dig { action, pos: [ORE_AT.0, ORE_AT.1, ORE_AT.2], face: 1, state }.encode());
        };
        dig(&mut h, DIG_START);
        h.tick(ticks.max(1));
        dig(&mut h, DIG_FINISH);
        h.tick(1);
        assert!((3..=7).contains(&orb_total(&h)), "diamond ore xp {}", orb_total(&h));
        // The orbs start at the middle of the block.
        let o = &h.game.xp_orbs.orbs[0];
        assert!((o.body.pos.x - 7.5).abs() < 1.0 && (o.body.pos.y - 201.5).abs() < 1.0);
    });
}

#[test]
fn taking_iron_from_a_furnace_pays_point_seven_each() {
    run(|| {
        assert_eq!(::items::smelting::get_smelting_experience(&ItemStack::of(item::IRON_INGOT)), 0.7);
        let mut h = setup();
        terrain::set_block(&mut h.game, FURNACE_AT, block::FURNACE << 4 | 2);
        h.game.tiles.furnaces.get_mut(&FURNACE_AT).unwrap().set_inventory_slot_contents(2, ItemStack::new(item::IRON_INGOT, 10, 0));
        let m = proto::UseOn { pos: [FURNACE_AT.0, FURNACE_AT.1, FURNACE_AT.2], face: 1, hit: [0.5; 3], hand: 0, state: 0 };
        h.push_frame(1, proto::V_USE_ON, &m.encode());
        h.tick(1);
        let frames = h.frames_for(1);
        let open = frames.iter().find(|f| f.0 == proto::S_OPEN_WINDOW).map(|f| proto::OpenWindow::decode(&f.1).unwrap()).expect("the furnace opened");
        assert_eq!(orb_total(&h), 0);
        // Shift-click the output (slot 2): the ten ingots go to the inventory, 10 * 0.7 = 7 points.
        h.push_frame(1, proto::V_CLICK, &proto::Click { window: open.window, slot: 2, button: 0, mode: 1, action: 1 }.encode());
        h.tick(1);
        assert!(h.game.tiles.furnaces[&FURNACE_AT].items[2].is_empty());
        // The orbs appear at the player's feet and are taken at once.
        h.tick(2);
        let p = h.game.players.players[&1].xp;
        assert_eq!((p.level, p.total), (1, 7), "7 points fill level 0 exactly");
    });
}

#[test]
fn a_death_drops_seven_per_level_up_to_a_hundred() {
    run(|| {
        for (level, want) in [(10, 70), (20, 100), (0, 0)] {
            let mut h = setup();
            h.game.players.players.get_mut(&1).unwrap().xp.level = level;
            h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
            assert!(damage::attack_entity_from(&mut h.game, 1, &DamageSource::lava(), 1000.0));
            assert!(h.game.players.players[&1].dead);
            h.tick(19);
            assert_eq!(orb_total(&h), 0);
            h.tick(2);
            assert_eq!(orb_total(&h), want, "level {level}");
        }
    });
}

#[test]
fn keep_inventory_keeps_the_levels_and_drops_no_orbs() {
    run(|| {
        let mut h = setup();
        h.world.lock().unwrap().set_rule("keepInventory", "true");
        h.game.players.players.get_mut(&1).unwrap().xp.level = 10;
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        assert!(damage::attack_entity_from(&mut h.game, 1, &DamageSource::lava(), 1000.0));
        h.tick(25);
        assert_eq!(orb_total(&h), 0);
        damage::respawn(&mut h.game, 1);
        assert_eq!(h.game.players.players[&1].xp.level, 10);
    });
}

#[test]
fn an_orb_despawns_at_six_thousand_ticks() {
    run(|| {
        let mut h = setup();
        crate::game::xp_orbs::spawn_orb(&mut h.game, [12.5, 205.0, 12.5], 3);
        h.game.xp_orbs.orbs[0].age = sim::xp_orb::LIFESPAN - 2;
        h.tick(1);
        assert_eq!(h.game.xp_orbs.orbs.len(), 1);
        h.tick(2);
        assert!(h.game.xp_orbs.orbs.is_empty());
    });
}
