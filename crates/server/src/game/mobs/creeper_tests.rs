//! The creeper (`EntityCreeper`), `game::explosion` and the Epic Siege swell goal over the slab
//! world: stone at y 200 under x, z 4..=13, the player at the west edge.

use proto::{PlayerState, ON_GROUND, S_EXPLOSION};
use sim::kinds::CREEPER;

use super::ai::Tasks;
use super::{def_of, summon, Mob};
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;
const NIGHT: i64 = 18000;

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    {
        let mut w = h.world.lock().unwrap();
        w.set_rule("doMobSpawning", "false");
        w.set_rule("doDaylightCycle", "false");
        w.set_time(NIGHT);
    }
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
    h.tick(1);
    h.frames_for(1);
    h
}

/// A creeper at `pos`, not powered; `esm` picks the swell goal; `targets` keeps the target goals.
fn creeper(h: &mut Harness, pos: [f64; 3], esm: bool, targets: bool) -> u32 {
    let id = summon(&mut h.game, &CREEPER, pos).unwrap();
    let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == id).unwrap();
    m.set_powered(false);
    m.tasks = Tasks::new();
    m.target_tasks = Tasks::new();
    m.init_creeper_ai();
    if esm {
        crate::game::addons::epic_siege::creeper::replace_swell(m, true);
    }
    if !targets {
        m.target_tasks = Tasks::new();
    }
    id
}

fn mob(h: &Harness, id: u32) -> Option<&Mob> {
    h.game.mobs.mobs.iter().find(|m| m.id == id)
}

fn health(h: &Harness) -> f32 {
    h.game.players.players[&1].health
}

fn explosions(frames: &[(u8, Vec<u8>)]) -> Vec<proto::Explosion> {
    frames.iter().filter(|f| f.0 == S_EXPLOSION).map(|f| proto::Explosion::decode(&f.1).expect("an explosion")).collect()
}

fn air(h: &Harness, at: (i32, i32, i32)) -> bool {
    terrain::state(&h.game, at) == 0
}

#[test]
fn a_creeper_has_the_vanilla_attributes_and_goal_lists() {
    let d = def_of(&CREEPER).unwrap();
    assert_eq!((d.max_health, d.speed, d.attack_damage), (20.0, 0.25, None));
    let c = Mob::new(1, d, [0.0, 0.0, 0.0], 0.0);
    // Swimming 1, swell 2, melee 4, wander 5, watch 6, look 6 (avoid ocelots, 3, is dropped).
    let pri: Vec<i32> = c.tasks.entries.iter().map(|e| e.priority).collect();
    assert_eq!(pri, vec![1, 2, 4, 5, 6, 6]);
    let pri: Vec<i32> = c.target_tasks.entries.iter().map(|e| e.priority).collect();
    assert_eq!(pri, vec![1, 2]);
    assert_eq!((c.creeper.fuse_time, c.creeper.explosion_radius, c.creeper_state()), (30, 3, -1));
    // The vanilla swell takes mutex bit 1; the Epic Siege one (an addon, applied in place) takes none
    // (walking fuse).
    let mut c = c;
    assert_eq!(c.tasks.entries[1].action.mutex_bits(), 1);
    crate::game::addons::epic_siege::creeper::replace_swell(&mut c, true);
    assert_eq!(c.tasks.entries[1].action.mutex_bits(), 0);
    assert_eq!(c.tasks.entries[1].priority, 2);
}

#[test]
fn a_creeper_next_to_a_player_ignites_and_explodes_after_30_ticks() {
    run(|| {
        let mut h = setup();
        let c = creeper(&mut h, [7.5, 201.0, 5.5], false, true);
        // Target found, then the swell starts.
        let mut first = None;
        for n in 0..200 {
            h.tick(1);
            match mob(&h, c) {
                Some(m) if m.creeper_state() == 1 && first.is_none() => first = Some(n),
                Some(_) => {}
                None => {
                    // 30 ticks of fuse from the first swell tick (the swell goal runs every tick
                    // after it starts, the creeper counts it the tick after).
                    let first = first.expect("swelled before it blew");
                    assert!(n - first >= 29 && n - first <= 34, "fuse ran {} ticks", n - first);
                    assert!(health(&h) < 20.0, "player health {}", health(&h));
                    let ex = explosions(&h.frames_for(1));
                    assert_eq!(ex.len(), 1);
                    assert_eq!(ex[0].strength, 3.0);
                    // The ground under the creeper is gone; a block far from the blast stays.
                    let at = (ex[0].pos[0].floor() as i32, 200, ex[0].pos[2].floor() as i32);
                    assert!(air(&h, at), "{at:?}");
                    assert!(!air(&h, (13, 200, 13)));
                    return;
                }
            }
        }
        panic!("the creeper never exploded");
    });
}

/// Ticks from the swell state first reading 1 to the creeper gone, for a creeper beside a survival
/// player. A playtest saw them explode instantly.
fn ticks_from_ignition_to_blast(esm: bool) -> i32 {
    let mut h = setup();
    let c = creeper(&mut h, [6.5, 201.0, 5.5], esm, true);
    let mut lit_at = None;
    for n in 1..=200 {
        h.tick(1);
        match mob(&h, c) {
            Some(m) if m.creeper_state() == 1 && lit_at.is_none() => lit_at = Some(n),
            Some(m) => assert!(lit_at.is_none() || m.creeper.time_since_ignited < m.creeper.fuse_time),
            None => return n - lit_at.expect("lit before it blew"),
        }
    }
    panic!("the creeper never blew");
}

#[test]
fn a_vanilla_creeper_takes_30_ticks_from_ignition_to_blast() {
    run(|| assert_eq!(ticks_from_ignition_to_blast(false), 30));
}

#[test]
fn an_esm_creeper_takes_30_ticks_from_ignition_to_blast() {
    run(|| assert_eq!(ticks_from_ignition_to_blast(true), 30));
}

#[test]
fn an_esm_creeper_with_a_path_does_not_breach() {
    run(|| {
        let mut h = setup();
        // Open ground, 7 blocks of walk, the target kept by hand and the creeper already past the
        // 60 tick breach age: only the 3 block swell range may light it.
        let c = creeper(&mut h, [12.5, 201.0, 5.5], true, false);
        h.game.mobs.mobs.iter_mut().find(|m| m.id == c).unwrap().ticks_existed = 100;
        for n in 0..60 {
            if let Some(m) = h.game.mobs.mobs.iter_mut().find(|m| m.id == c) {
                m.attack_target = Some(1);
            }
            h.tick(1);
            let Some(m) = mob(&h, c) else { return };
            let d = ((m.body.pos.x - 5.5).powi(2) + (m.body.pos.z - 5.5).powi(2)).sqrt();
            let has_path = !m.navigator.no_path();
            assert!(m.creeper_state() != 1 || d < 3.5, "tick {n}: lit at {d} blocks, path {has_path}");
        }
    });
}

#[test]
fn griefing_off_keeps_the_blocks() {
    run(|| {
        let mut h = setup();
        h.world.lock().unwrap().set_rule("mobGriefing", "false");
        creeper(&mut h, [7.5, 201.0, 5.5], false, true);
        h.tick(120);
        assert!(!air(&h, (7, 200, 5)));
        assert!(health(&h) < 20.0);
    });
}

#[test]
fn a_vanilla_creeper_defuses_when_the_player_walks_off() {
    run(|| {
        let mut h = setup();
        let c = creeper(&mut h, [7.5, 201.0, 5.5], false, true);
        for _ in 0..200 {
            h.tick(1);
            if mob(&h, c).is_some_and(|m| m.creeper_state() == 1) {
                break;
            }
        }
        assert_eq!(mob(&h, c).unwrap().creeper_state(), 1);
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 12.5], flags: ON_GROUND, ..Default::default() });
        h.tick(12);
        let m = mob(&h, c).expect("it did not blow");
        assert_eq!(m.creeper_state(), -1);
    });
}

/// With the addon off, a creeper summoned the ordinary way runs the vanilla swell goal and defuses.
#[test]
fn addon_off_a_creeper_defuses_when_the_player_walks_out_of_range() {
    run(|| {
        let mut h = setup();
        h.game.addons.enabled.clear();
        let c = summon(&mut h.game, &CREEPER, [7.5, 201.0, 5.5]).unwrap();
        h.game.mobs.mobs.iter_mut().find(|m| m.id == c).unwrap().set_powered(false);
        assert_eq!(mob(&h, c).unwrap().tasks.entries[1].action.mutex_bits(), 1, "the vanilla swell goal");
        for _ in 0..200 {
            h.tick(1);
            if mob(&h, c).is_some_and(|m| m.creeper_state() == 1) {
                break;
            }
        }
        assert_eq!(mob(&h, c).unwrap().creeper_state(), 1);
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 12.5], flags: ON_GROUND, ..Default::default() });
        h.tick(12);
        let m = mob(&h, c).expect("it did not blow");
        assert_eq!(m.creeper_state(), -1);
    });
}

#[test]
fn an_esm_creeper_does_not_defuse_when_the_player_walks_off() {
    run(|| {
        let mut h = setup();
        let c = creeper(&mut h, [7.5, 201.0, 5.5], true, true);
        for _ in 0..200 {
            h.tick(1);
            if mob(&h, c).is_some_and(|m| m.creeper_state() == 1) {
                break;
            }
        }
        assert_eq!(mob(&h, c).unwrap().creeper_state(), 1);
        // Out of the blast radius and out of the 7 block swell range, the fuse stays lit.
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 13.5], flags: ON_GROUND, ..Default::default() });
        h.frames_for(1);
        for _ in 0..60 {
            h.tick(1);
            match mob(&h, c) {
                Some(m) => assert_eq!(m.creeper_state(), 1, "the locked fuse cancelled"),
                None => {
                    assert_eq!(explosions(&h.frames_for(1)).len(), 1);
                    return;
                }
            }
        }
        panic!("the locked fuse never went off");
    });
}

#[test]
fn an_esm_creeper_breaches_a_wall_after_60_ticks() {
    run(|| {
        let mut h = setup();
        // A wall across the slab between the creeper and the player.
        for z in 4..=13 {
            for y in 201..=204 {
                terrain::set_block(&mut h.game, (9, y, z), STONE);
            }
        }
        let c = creeper(&mut h, [12.5, 201.0, 5.5], true, false);
        let mut lit_at = None;
        for _ in 0..300 {
            // The target is kept by hand: it is behind the wall and out of sight.
            if let Some(m) = h.game.mobs.mobs.iter_mut().find(|m| m.id == c) {
                m.attack_target = Some(1);
            }
            h.tick(1);
            match mob(&h, c) {
                Some(m) if m.creeper_state() == 1 && lit_at.is_none() => lit_at = Some(m.ticks_existed),
                Some(_) => {}
                None => {
                    let t = lit_at.expect("lit before it blew");
                    // Not before 60 ticks alive, and the player was never within 3 blocks.
                    assert!(t > 60, "breached at {t}");
                    assert_eq!(explosions(&h.frames_for(1)).len(), 1);
                    return;
                }
            }
        }
        panic!("the creeper never breached");
    });
}

#[test]
fn an_esm_creeper_does_not_breach_before_60_ticks() {
    run(|| {
        let mut h = setup();
        for z in 4..=13 {
            for y in 201..=204 {
                terrain::set_block(&mut h.game, (9, y, z), STONE);
            }
        }
        let c = creeper(&mut h, [12.5, 201.0, 5.5], true, false);
        for _ in 0..50 {
            h.game.mobs.mobs.iter_mut().find(|m| m.id == c).unwrap().attack_target = Some(1);
            h.tick(1);
            assert_eq!(mob(&h, c).unwrap().creeper_state(), -1);
        }
    });
}

#[test]
fn a_powered_creeper_blasts_with_radius_6() {
    run(|| {
        let mut h = setup();
        let c = creeper(&mut h, [7.5, 201.0, 5.5], false, true);
        h.game.mobs.mobs.iter_mut().find(|m| m.id == c).unwrap().set_powered(true);
        assert_eq!(mob(&h, c).unwrap().explosion_size(), 6.0);
        for _ in 0..200 {
            h.tick(1);
            if mob(&h, c).is_none() {
                let ex = explosions(&h.frames_for(1));
                assert_eq!(ex.len(), 1);
                assert_eq!(ex[0].strength, 6.0);
                let at = (ex[0].pos[0].floor() as i32, 200, ex[0].pos[2].floor() as i32);
                assert!(air(&h, at), "{at:?}");
                return;
            }
        }
        panic!("the creeper never exploded");
    });
}

#[test]
fn the_spawn_roll_powers_about_a_tenth_of_creepers() {
    let mut r = worldgen::java_random::JavaRandom::new(5);
    let d = def_of(&CREEPER).unwrap();
    let mut n = 0;
    for _ in 0..2000 {
        let mut m = Mob::new(1, d, [0.0; 3], 0.0);
        crate::game::addons::epic_siege::creeper::on_spawn(&mut m, &mut r, 0.1);
        n += m.get_powered() as i32;
    }
    assert!((140..260).contains(&n), "{n} of 2000 powered");
}

#[test]
fn block_resistances_follow_the_registry() {
    use crate::game::explosion::block_resistance;
    assert_eq!(block_resistance(STONE), 6.0);
    assert_eq!(block_resistance(49 << 4), 1200.0);
    assert_eq!(block_resistance(3 << 4), 0.5);
    assert!(block_resistance(7 << 4) > 1.0e6);
}
