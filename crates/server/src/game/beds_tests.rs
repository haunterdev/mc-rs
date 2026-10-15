//! Beds over the slab world of use_on_tests.rs: stone at y 200 under x, z 4..=13, the player at
//! (5.5, 201, 5.5) facing south, the bed foot at (8, 201, 8) and its head at (8, 201, 9).

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND, SNEAKING};
use sim::kinds::ZOMBIE;

use crate::game::testing::{run, Harness};
use crate::game::{damage, mining, mobs, terrain};

const BED: u16 = block::BED << 4;
const FOOT: (i32, i32, i32) = (8, 201, 8);
const HEAD: (i32, i32, i32) = (8, 201, 9);

fn setup(time: i64) -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    {
        let mut w = h.world.lock().unwrap();
        w.set_rule("doMobSpawning", "false");
        w.set_time(time);
    }
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), block::STONE << 4);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
    h.tick(1);
    h.frames_for(1);
    h
}

fn place_bed(h: &mut Harness) {
    let p = h.game.players.players.get_mut(&1).unwrap();
    let c = p.inv.current_item;
    p.inv.main[c] = ItemStack::new(item::BED, 2, 14);
    h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos: [8, 200, 8], face: 1, hit: [0.5; 3], hand: 0, state: 0 }.encode());
    h.tick(1);
}

fn click_bed(h: &mut Harness) {
    h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos: [HEAD.0, HEAD.1, HEAD.2], face: 1, hit: [0.5; 3], hand: 0, state: 0 }.encode());
    h.tick(1);
    // The viewer answers the teleport onto the bed with its next state.
    if h.game.beds.sleeping.contains_key(&1) {
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [8.5, 201.69, 9.9], flags: ON_GROUND, ..Default::default() });
        h.tick(1);
    }
}

fn chat_lines(h: &mut Harness) -> Vec<String> {
    h.frames_for(1).into_iter().filter(|f| f.0 == proto::S_TEXT).map(|f| String::from_utf8_lossy(&f.1).into_owned()).collect()
}

#[test]
fn a_placed_bed_has_a_foot_and_a_head() {
    run(|| {
        let mut h = setup(18000);
        place_bed(&mut h);
        assert_eq!(terrain::state(&h.game, FOOT), BED);
        assert_eq!(terrain::state(&h.game, HEAD), BED | 8);
        let p = &h.game.players.players[&1];
        assert_eq!(p.inv.main[p.inv.current_item].count, 1);
    });
}

#[test]
fn a_bed_needs_a_solid_top_under_both_halves() {
    run(|| {
        let mut h = setup(18000);
        terrain::set_block(&mut h.game, (8, 200, 9), 0);
        place_bed(&mut h);
        assert_eq!(terrain::state(&h.game, FOOT), 0);
        assert_eq!(terrain::state(&h.game, HEAD), 0);
    });
}

#[test]
fn every_player_in_bed_skips_the_night_and_sets_the_respawn_point() {
    run(|| {
        let mut h = setup(18000);
        place_bed(&mut h);
        click_bed(&mut h);
        assert!(h.game.beds.sleeping.contains_key(&1), "{:?}", chat_lines(&mut h));
        assert_eq!(terrain::state(&h.game, HEAD), BED | 8 | 4, "the head is occupied");
        h.frames_for(1);
        h.tick(105);
        assert!(h.game.beds.sleeping.is_empty());
        let t = h.world.lock().unwrap().time();
        assert!((24000..24100).contains(&t), "morning: {t}");
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [7.5, 201.1, 9.5], flags: ON_GROUND, ..Default::default() });
        h.tick(1);
        assert_eq!(terrain::state(&h.game, HEAD), BED | 8);
        assert_eq!(h.world.lock().unwrap().player("Xy").unwrap().spawn, Some([HEAD.0, HEAD.1, HEAD.2]));
        assert!(h.frames_for(1).iter().any(|f| f.0 == proto::S_TIME), "the viewers' clocks follow");
    });
}

#[test]
fn a_second_player_awake_keeps_the_night() {
    run(|| {
        let mut h = setup(18000);
        h.join(2, "Ab", [6.5, 230.0, 5.5]);
        h.tick(1);
        place_bed(&mut h);
        click_bed(&mut h);
        h.tick(105);
        assert!(h.game.beds.sleeping.contains_key(&1), "still waiting for the other player");
        assert!(h.world.lock().unwrap().time() < 24000);
    });
}

#[test]
fn a_zombie_four_blocks_away_refuses_the_bed() {
    run(|| {
        let mut h = setup(18000);
        place_bed(&mut h);
        mobs::summon(&mut h.game, &ZOMBIE, [12.5, 201.0, 9.5]).unwrap();
        h.frames_for(1);
        click_bed(&mut h);
        assert!(h.game.beds.sleeping.is_empty());
        let lines = chat_lines(&mut h);
        assert!(lines.iter().any(|l| l.contains("You may not rest now, there are monsters nearby")), "{lines:?}");
    });
}

#[test]
fn the_bed_refuses_in_daylight() {
    run(|| {
        let mut h = setup(6000);
        place_bed(&mut h);
        h.frames_for(1);
        click_bed(&mut h);
        assert!(h.game.beds.sleeping.is_empty());
        let lines = chat_lines(&mut h);
        assert!(lines.iter().any(|l| l.contains("You can only sleep at night")), "{lines:?}");
    });
}

#[test]
fn sneaking_leaves_the_bed() {
    run(|| {
        let mut h = setup(18000);
        place_bed(&mut h);
        click_bed(&mut h);
        assert!(h.game.beds.sleeping.contains_key(&1));
        h.tick(3);
        let mut st = h.game.sessions[&1].state.unwrap_or_default();
        st.flags |= SNEAKING;
        h.game.sessions.get_mut(&1).unwrap().state = Some(st);
        h.tick(1);
        assert!(h.game.beds.sleeping.is_empty());
        assert_eq!(terrain::state(&h.game, HEAD), BED | 8);
    });
}

#[test]
fn a_death_after_sleeping_respawns_beside_the_bed_and_a_broken_bed_falls_back() {
    run(|| {
        let mut h = setup(18000);
        place_bed(&mut h);
        click_bed(&mut h);
        h.tick(105);
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [9.5, 201.1, 9.5], flags: ON_GROUND, ..Default::default() });
        h.tick(1);
        damage::attack_entity_from(&mut h.game, 1, &damage::DamageSource::fall(), 100.0);
        assert!(h.game.players.players[&1].dead);
        h.frames_for(1);
        damage::respawn(&mut h.game, 1);
        let tp = h.frames_for(1).into_iter().filter(|f| f.0 == proto::S_TELEPORT).find_map(|f| proto::Teleport::decode(&f.1)).expect("a teleport");
        assert!((tp.pos[0] - 8.5).abs() <= 2.0 && (tp.pos[2] - 9.5).abs() <= 2.0 && (tp.pos[1] - 201.1).abs() < 0.01, "{:?}", tp.pos);
        // With the bed gone the world spawn is used.
        terrain::set_block(&mut h.game, FOOT, 0);
        terrain::set_block(&mut h.game, HEAD, 0);
        damage::attack_entity_from(&mut h.game, 1, &damage::DamageSource::fall(), 100.0);
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        h.frames_for(1);
        damage::respawn(&mut h.game, 1);
        let tp = h.frames_for(1).into_iter().filter(|f| f.0 == proto::S_TELEPORT).find_map(|f| proto::Teleport::decode(&f.1)).expect("a teleport");
        assert_ne!(tp.flags & proto::TP_SURFACE, 0, "the world spawn, on the top block");
    });
}

#[test]
fn breaking_either_half_removes_both_and_drops_one_bed() {
    run(|| {
        for half in [FOOT, HEAD] {
            let mut h = setup(18000);
            place_bed(&mut h);
            let state = terrain::state(&h.game, half);
            h.game.items.entities.clear();
            assert!(mining::try_harvest(&mut h.game, 1, half, state));
            assert_eq!(terrain::state(&h.game, FOOT), 0, "{half:?}");
            assert_eq!(terrain::state(&h.game, HEAD), 0, "{half:?}");
            assert_eq!(h.game.items.entities.len(), 1, "{half:?}");
        }
    });
}
