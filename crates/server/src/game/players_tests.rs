//! Health, hunger, damage, death and respawn (T4), on a game over a seeded world. The player
//! stands in a column of air at y 200 to 250 above a stone slab, so a fall is whatever the test
//! makes it.

use items::ItemStack;
use proto::{PlayerState, ON_GROUND, SPRINTING};

use super::damage::{self, DamageSource};
use super::testing::{run, Harness};
use super::{players, terrain, GameMsg};

const STONE: u16 = 1 << 4;
const WATER: u16 = 9 << 4;
const LAVA: u16 = 11 << 4;
const LADDER: u16 = (65 << 4) | 2;

/// Stone top at y 201 (feet land at 201.0) over x, z 4..=13.
fn setup(name: &str) -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, name, [8.5, 230.0, 8.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    place(&mut h, [8.5, 201.0, 8.5], ON_GROUND);
    // The 60 ticks of respawn protection every new player has.
    h.tick(61);
    quiet(&mut h);
    h.frames_for(1);
    h
}

/// Nothing regenerates or starves while a test reads the health.
fn quiet(h: &mut Harness) {
    let p = h.game.players.players.get_mut(&1).unwrap();
    p.food.level = 10;
    p.food.saturation = 0.0;
}

/// The session's state becomes `pos` without the movement from where it was.
fn place(h: &mut Harness, pos: [f64; 3], flags: u8) {
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos, flags, ..Default::default() });
}

fn state(h: &mut Harness, pos: [f64; 3], flags: u8) {
    h.send(GameMsg::State { id: 1, state: PlayerState { pos, flags, ..Default::default() } });
    h.tick(1);
}

fn health(h: &Harness) -> f32 {
    h.game.players.players[&1].health
}

/// The player is placed at `start`, falls through each of `ys`, and lands at feet y 201.
fn fall(h: &mut Harness, x: f64, start: f64, ys: &[f64]) {
    place(h, [x, start, 8.5], 0);
    h.tick(1);
    for &y in ys {
        state(h, [x, y, 8.5], 0);
    }
    state(h, [x, 201.0, 8.5], ON_GROUND);
}

fn straight(from: i32, to: i32) -> Vec<f64> {
    (to..from).rev().map(|y| y as f64).collect()
}

fn tags(frames: &[(u8, Vec<u8>)], tag: u8) -> Vec<&Vec<u8>> {
    frames.iter().filter(|(t, _)| *t == tag).map(|(_, b)| b).collect()
}

#[test]
fn a_fall_of_23_blocks_kills_and_3_and_a_half_costs_one() {
    run(|| {
        let mut h = setup("Xy");
        // 23 blocks: ceil(23 - 3) = 20.
        fall(&mut h, 8.5, 225.0, &straight(225, 202));
        assert_eq!(health(&h), 0.0);
        let frames = h.frames_for(1);
        let death = proto::Death::decode(tags(&frames, proto::S_DEATH)[0]).unwrap();
        assert_eq!(death.message, "Xy fell from a high place");
        assert!(h.game.players.players[&1].dead);

        let mut h = setup("Xy");
        // 3.5 blocks: ceil(0.5) = 1.
        fall(&mut h, 8.5, 205.0, &[204.0, 203.0, 202.0, 201.5]);
        assert_eq!(health(&h), 19.0);
        assert_eq!(h.game.players.players[&1].fall_distance, 0.0);
        // 3 blocks costs nothing.
        let mut h = setup("Xy");
        fall(&mut h, 8.5, 205.0, &[204.0, 203.0, 202.0]);
        assert_eq!(health(&h), 20.0);
    });
}

#[test]
fn water_and_ladders_end_a_fall() {
    run(|| {
        let mut h = setup("Xy");
        for y in 201..=206 {
            terrain::set_block(&mut h.game, (8, y, 8), WATER);
        }
        fall(&mut h, 8.5, 225.0, &straight(225, 202));
        assert_eq!(health(&h), 20.0);

        let mut h = setup("Xy");
        for y in 201..=206 {
            terrain::set_block(&mut h.game, (8, y, 8), LADDER);
        }
        fall(&mut h, 8.5, 225.0, &straight(225, 202));
        assert_eq!(health(&h), 20.0);
    });
}

#[test]
fn creative_takes_no_damage_but_kill_still_works() {
    run(|| {
        let mut h = setup("Xy");
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        h.tick(1);
        fall(&mut h, 8.5, 225.0, &straight(225, 202));
        assert_eq!(health(&h), 20.0);
        assert!(!damage::attack_entity_from(&mut h.game, 1, &DamageSource::generic(), 5.0));
        assert_eq!(health(&h), 20.0);
        h.send(GameMsg::Kill { id: 1 });
        h.tick(1);
        assert_eq!(health(&h), 0.0);
        let frames = h.frames_for(1);
        assert_eq!(tags(&frames, proto::S_DEATH).len(), 1);
    });
}

#[test]
fn sprinting_100_blocks_costs_10_exhaustion_and_a_jump_costs_more_sprinting() {
    run(|| {
        let mut h = setup("Xy");
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            p.food.level = 20;
            p.food.saturation = 20.0;
        }
        h.move_to(1, [4.5, 201.0, 8.5]);
        h.tick(1);
        let before = h.game.players.players[&1].food;
        for i in 1..=400 {
            // Handled without a tick between, as the player's states come faster than the tick.
            h.game.handle(GameMsg::State { id: 1, state: PlayerState { pos: [4.5 + 0.25 * i as f64, 201.0, 8.5], flags: ON_GROUND | SPRINTING, ..Default::default() } });
        }
        // FoodStats drains one 4.0 per tick.
        h.game.tick();
        h.game.tick();
        let f = h.game.players.players[&1].food;
        // The tick only drains: 10.0 exhaustion becomes 2 saturation points and 2.0 left over.
        assert!((f.exhaustion - 2.0).abs() < 0.01, "{f:?}");
        assert!((before.saturation - f.saturation - 2.0).abs() < 0.01, "{f:?}");
        // A sprinting jump 0.2, a plain one 0.05.
        for (flags, cost) in [(SPRINTING, 0.2f32), (0, 0.05)] {
            let mut h = setup("Xy");
            h.game.handle(GameMsg::State { id: 1, state: PlayerState { pos: [8.5, 201.0, 8.5], flags: ON_GROUND | flags, ..Default::default() } });
            let e0 = h.game.players.players[&1].food.exhaustion;
            h.game.handle(GameMsg::State { id: 1, state: PlayerState { pos: [8.5, 201.4, 8.5], flags, ..Default::default() } });
            let e1 = h.game.players.players[&1].food.exhaustion;
            assert!((e1 - e0 - cost).abs() < 1e-6, "{flags}: {e0} {e1}");
        }
    });
}

#[test]
fn food_regenerates_and_starves_in_the_game() {
    run(|| {
        let mut h = setup("Xy");
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            p.food.level = 18;
            p.food.saturation = 0.0;
            p.health = 10.0;
        }
        h.tick(80);
        assert_eq!(health(&h), 11.0);
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            p.food.level = 0;
            p.food.saturation = 0.0;
            p.food.timer = 0;
            p.food.exhaustion = 0.0;
            p.health = 20.0;
        }
        h.tick(80);
        assert_eq!(health(&h), 19.0);
    });
}

#[test]
fn drowning_takes_two_hearts_after_air_runs_out() {
    run(|| {
        let mut h = setup("Xy");
        for y in 201..=206 {
            for x in 6..=10 {
                for z in 6..=10 {
                    terrain::set_block(&mut h.game, (x, y, z), WATER);
                }
            }
        }
        h.move_to(1, [8.5, 201.0, 8.5]);
        h.tick(1);
        quiet(&mut h);
        h.tick(300);
        assert_eq!(health(&h), 20.0);
        // Air is -20 on tick 321 (it counts from 300 on the first wet tick).
        h.tick(25);
        assert_eq!(health(&h), 18.0);
        let frames = h.frames_for(1);
        let last = proto::Health::decode(tags(&frames, proto::S_HEALTH).last().unwrap()).unwrap();
        assert_eq!(last.health, 18.0);
    });
}

#[test]
fn lava_burns_and_the_void_hurts() {
    run(|| {
        let mut h = setup("Xy");
        for x in 6..=10 {
            for z in 6..=10 {
                terrain::set_block(&mut h.game, (x, 201, z), LAVA);
            }
        }
        h.move_to(1, [8.5, 201.0, 8.5]);
        h.tick(2);
        quiet(&mut h);
        assert!(health(&h) <= 16.0, "{}", health(&h));
        assert!(h.game.players.players[&1].fire > 0);
        let frames = h.frames_for(1);
        let last = proto::Health::decode(tags(&frames, proto::S_HEALTH).last().unwrap()).unwrap();
        assert!(last.flags & proto::HEALTH_BURNING != 0);

        let mut h = setup("Xy");
        h.move_to(1, [8.5, -70.0, 8.5]);
        h.tick(2);
        assert_eq!(health(&h), 16.0);
    });
}

#[test]
fn a_stronger_hit_inside_the_hurt_window_counts_the_difference() {
    run(|| {
        let mut h = setup("Xy");
        let src = DamageSource::generic();
        assert!(damage::attack_entity_from(&mut h.game, 1, &src, 5.0));
        assert_eq!(health(&h), 15.0);
        assert!(!damage::attack_entity_from(&mut h.game, 1, &src, 3.0));
        assert_eq!(health(&h), 15.0);
        assert!(damage::attack_entity_from(&mut h.game, 1, &src, 8.0));
        assert_eq!(health(&h), 12.0);
    });
}

#[test]
fn a_new_player_is_protected_for_60_ticks() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [8.5, 230.0, 8.5]);
        h.tick(1);
        assert!(!damage::attack_entity_from(&mut h.game, 1, &DamageSource::generic(), 5.0));
        // The void goes through.
        assert!(damage::attack_entity_from(&mut h.game, 1, &DamageSource::out_of_world(), 4.0));
        assert_eq!(health(&h), 16.0);
    });
}

fn stacks(h: &mut Harness) {
    let inv = &mut h.game.players.players.get_mut(&1).unwrap().inv;
    inv.set_inventory_slot_contents(0, ItemStack::new(1, 5, 0));
    inv.set_inventory_slot_contents(1, ItemStack::new(4, 64, 0));
}

#[test]
fn death_drops_the_inventory_and_respawn_brings_the_player_back() {
    run(|| {
        let mut h = setup("Xy");
        stacks(&mut h);
        h.send(GameMsg::Kill { id: 1 });
        h.tick(1);
        let frames = h.frames_for(1);
        let spawned: Vec<_> = tags(&frames, proto::S_SPAWN_OBJECT).iter().map(|b| proto::SpawnObject::decode(b).unwrap()).collect();
        assert_eq!(spawned.len(), 2);
        let mut counts: Vec<u8> = spawned.iter().map(|s| match s.data { proto::ObjectData::Item(w) => w.count, _ => 0 }).collect();
        counts.sort();
        assert_eq!(counts, vec![5, 64]);
        assert_eq!(h.game.items.entities.len(), 2);
        assert!(h.game.players.players.get_mut(&1).unwrap().inv.drop_all_items().is_empty());
        assert!(h.game.players.players[&1].dead);
        // A dead player takes no more damage, and a living one cannot "respawn".
        h.push_frame(1, proto::V_RESPAWN, &[]);
        h.tick(1);
        let p = &h.game.players.players[&1];
        assert!(!p.dead);
        assert_eq!(p.health, 20.0);
        assert_eq!(p.respawn_invulnerable, 60 - 0);
        let frames = h.frames_for(1);
        let tp = proto::Teleport::decode(tags(&frames, proto::S_TELEPORT)[0]).unwrap();
        assert!(tp.flags & proto::TP_RESPAWN != 0);
        assert_eq!(tags(&frames, proto::S_HEALTH).len(), 1);
        assert!(h.game.sessions[&1].state.is_none());
        // Pressing respawn alive does nothing.
        h.push_frame(1, proto::V_RESPAWN, &[]);
        h.tick(1);
        assert!(h.frames_for(1).iter().all(|(t, _)| *t != proto::S_TELEPORT));
    });
}

#[test]
fn keep_inventory_keeps_the_stacks() {
    run(|| {
        let mut h = setup("Xy");
        h.world.lock().unwrap().set_rule("keepInventory", "true");
        stacks(&mut h);
        h.send(GameMsg::Kill { id: 1 });
        h.tick(1);
        assert!(h.game.items.entities.is_empty());
        h.push_frame(1, proto::V_RESPAWN, &[]);
        h.tick(1);
        let inv = &mut h.game.players.players.get_mut(&1).unwrap().inv;
        assert_eq!(inv.drop_all_items().len(), 2);
    });
}

#[test]
fn damage_after_absorb_with_no_armor_is_the_damage() {
    assert_eq!(damage::damage_after_absorb(7.0, 0.0, 0.0), 7.0);
    // 10 armor halves a 4 point hit by 40%: 4 * (1 - 8 / 25) after the 2 + t/4 divisor.
    let d = damage::damage_after_absorb(4.0, 10.0, 0.0);
    assert!((d - 4.0 * (1.0 - (10.0f32 - 2.0).clamp(2.0, 20.0) / 25.0)).abs() < 1e-6);
}

#[allow(dead_code)]
fn unused(_: &players::Survival) {}

fn slots_in(frames: &[(u8, Vec<u8>)]) -> Vec<proto::WindowSlots> {
    tags(frames, proto::S_SLOTS).iter().map(|b| proto::WindowSlots::decode(b).unwrap()).collect()
}

#[test]
fn playerdata_round_trips_through_json() {
    let mut p = players::Survival::new();
    p.health = 7.5;
    p.food.level = 13;
    p.food.saturation = 2.25;
    p.food.exhaustion = 1.5;
    p.food.timer = 40;
    p.air = 120;
    p.fall_distance = 4.5;
    p.xp = proto::Xp { bar: 0.5, level: 3, total: 40 };
    p.inv.set_inventory_slot_contents(2, ItemStack::new(1, 64, 0));
    p.inv.set_inventory_slot_contents(30, ItemStack::new(4, 3, 0));
    p.inv.current_item = 2;
    let v = super::playerdata::to_json(&p);
    assert_eq!(v["inventory"][0]["id"], "minecraft:stone");
    assert_eq!(v["inventory"][0]["slot"], 0 + 2);
    let mut q = players::Survival::new();
    assert!(super::playerdata::from_json(&mut q, &v));
    assert_eq!((q.health, q.food.level, q.food.saturation, q.food.exhaustion, q.food.timer), (7.5, 13, 2.25, 1.5, 40));
    assert_eq!((q.air, q.fall_distance, q.xp, q.inv.current_item), (120, 4.5, p.xp, 2));
    assert_eq!(q.inv.to_save(), p.inv.to_save());
    // A dead player's file, or one of another version, is not applied.
    let mut dead = v.clone();
    dead["dead"] = true.into();
    assert!(!super::playerdata::from_json(&mut players::Survival::new(), &dead));
    let mut other = v.clone();
    other["version"] = 2.into();
    assert!(!super::playerdata::from_json(&mut players::Survival::new(), &other));
}

#[test]
fn a_rejoining_player_gets_their_state_back() {
    run(|| {
        let mut h = setup("Xy");
        stacks(&mut h);
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            p.health = 11.0;
            p.food.level = 9;
            p.food.saturation = 1.0;
            p.air = 200;
            p.xp = proto::Xp { bar: 0.25, level: 2, total: 17 };
        }
        h.leave(1);
        h.tick(1);
        assert!(h.dir.join("playerdata").join("xy.json").is_file());
        h.join(2, "XY", [8.5, 201.0, 8.5]);
        h.tick(1);
        let p = &h.game.players.players[&2];
        // Air refills out of water in the tick after the join, so it is not compared here.
        assert_eq!((p.health, p.food.level, p.food.saturation), (11.0, 9, 1.0));
        assert_eq!(p.xp.level, 2);
        assert_eq!(p.inv.main[0], ItemStack::new(1, 5, 0));
        assert_eq!(p.inv.main[1], ItemStack::new(4, 64, 0));
        // A player with no file starts fresh.
        h.join(3, "Fresh", [8.5, 201.0, 8.5]);
        h.tick(1);
        assert_eq!(h.game.players.players[&3].health, 20.0);
        assert!(h.game.players.players[&3].inv.to_save().is_empty());
    });
}

#[test]
fn playerdata_is_written_on_save_and_at_the_interval() {
    run(|| {
        let mut h = setup("Xy");
        let path = h.dir.join("playerdata").join("xy.json");
        let _ = std::fs::remove_file(&path);
        h.game.players.players.get_mut(&1).unwrap().health = 5.0;
        h.tick(100);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["health"], 5.0);
        assert_eq!(v["version"], 1);
    });
}

#[test]
fn respawn_sends_the_inventory_slots() {
    run(|| {
        let mut h = setup("Xy");
        h.world.lock().unwrap().set_rule("keepInventory", "true");
        stacks(&mut h);
        h.send(GameMsg::Kill { id: 1 });
        h.tick(1);
        h.frames_for(1);
        h.push_frame(1, proto::V_RESPAWN, &[]);
        h.tick(1);
        let slots = slots_in(&h.frames_for(1));
        assert_eq!(slots.len(), 1);
        let s = &slots[0];
        assert_eq!(s.window, 0);
        // Hotbar slot 0 is window slot 36.
        let at = |n: i16| s.slots.iter().find(|(i, _)| *i == n).unwrap().1;
        assert_eq!((at(36).id, at(36).count), (1, 5));
        assert_eq!((at(37).id, at(37).count), (4, 64));
        assert!(at(9).is_empty());
    });
}

#[test]
fn a_teleport_does_not_deal_fall_damage() {
    run(|| {
        let mut h = setup("Xy");
        place(&mut h, [8.5, 230.0, 8.5], 0);
        h.tick(1);
        state(&mut h, [8.5, 220.0, 8.5], 0);
        assert!(h.game.players.players[&1].fall_distance > 5.0);
        // /tp puts the player on the ground 19 blocks lower.
        h.send(GameMsg::Teleport { id: 1 });
        h.tick(1);
        assert_eq!(h.game.players.players[&1].fall_distance, 0.0);
        state(&mut h, [8.5, 201.0, 8.5], ON_GROUND);
        state(&mut h, [8.5, 201.0, 8.5], ON_GROUND);
        assert_eq!(health(&h), 20.0);
    });
}
