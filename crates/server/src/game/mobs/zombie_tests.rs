//! The zombie (T9j) and the target goals over the slab world: stone at y 200 under x, z 4..=13,
//! the player at the west edge.

use items::ids::item;
use proto::{PlayerState, MM_BURNING, ON_GROUND};
use sim::kinds::ZOMBIE;
use worldgen::java_random::JavaRandom;

use super::ai::target::HurtByTarget;
use super::ai::Tasks;
use super::combat::hurt_mob;
use super::{def_of, summon, zombie, Mob};
use crate::game::damage::{Attacker, DamageSource};
use crate::game::terrain;
use crate::game::testing::{run, Harness};
use crate::game::GameMsg;

const STONE: u16 = 1 << 4;
const NIGHT: i64 = 18000;
const DAY: i64 = 6000;

/// The slab world at `time` of day, the clock stopped, natural spawning off.
fn setup(time: i64) -> Harness {
    let mut h = Harness::new(7, 2);
    {
        let mut w = h.world.lock().unwrap();
        w.set_rule("doMobSpawning", "false");
        w.set_rule("doDaylightCycle", "false");
        w.set_time(time);
    }
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

fn mob(h: &Harness, id: u32) -> &Mob {
    h.game.mobs.mobs.iter().find(|m| m.id == id).expect("the mob")
}

fn player_src() -> DamageSource {
    DamageSource::player(Attacker { name: "Xy".into(), pos: [5.5, 201.0, 5.5], player: true, id: Some(1) })
}

fn health(h: &Harness) -> f32 {
    h.game.players.players[&1].health
}

fn dist_to_player(h: &Harness, id: u32) -> f64 {
    let p = mob(h, id).body.pos;
    ((p.x - 5.5).powi(2) + (p.z - 5.5).powi(2)).sqrt()
}

#[test]
fn a_zombie_has_the_vanilla_attributes_and_goal_lists() {
    let d = def_of(&ZOMBIE).unwrap();
    assert_eq!((d.max_health, d.follow_range, d.speed, d.attack_damage, d.armor), (20.0, 35.0, 0.23, Some(3.0), 2.0));
    let z = Mob::new(1, d, [0.0, 0.0, 0.0], 0.0);
    // Swimming 0, ZombieAttack 2, WanderAvoidWater 7, WatchClosest 8, LookIdle 8.
    let pri: Vec<i32> = z.tasks.entries.iter().map(|e| e.priority).collect();
    assert_eq!(pri, vec![0, 2, 7, 8, 8]);
    // HurtByTarget 1, NearestAttackableTarget (players) 2.
    let pri: Vec<i32> = z.target_tasks.entries.iter().map(|e| e.priority).collect();
    assert_eq!(pri, vec![1, 2]);
    assert_eq!(z.target_tasks.entries[0].action.mutex_bits(), 1);
    assert_eq!(z.tasks.entries[1].action.mutex_bits(), 3);
}

#[test]
fn a_zombie_targets_walks_to_and_hurts_a_survival_player() {
    run(|| {
        let mut h = setup(NIGHT);
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        let z = summon(&mut h.game, &ZOMBIE, [12.5, 201.0, 5.5]).unwrap();
        h.tick(3);
        let start = dist_to_player(&h, z);
        assert!(start > 6.0);
        let mut targeted = false;
        let mut hit = None;
        for n in 0..400 {
            h.tick(1);
            targeted |= mob(&h, z).attack_target == Some(1);
            if hit.is_none() && health(&h) < 20.0 {
                hit = Some(n);
            }
        }
        assert!(targeted, "never took the player as its target");
        assert!(hit.is_some(), "never hurt the player");
        assert!(dist_to_player(&h, z) < 3.0, "walked to {}", dist_to_player(&h, z));
        // Attack damage 3, scaled by the difficulty, once per 20 ticks: more than one hit lands.
        assert!(health(&h) < 18.0, "{}", health(&h));
    });
}

#[test]
fn a_zombie_ignores_a_creative_player() {
    run(|| {
        let mut h = setup(NIGHT);
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        h.tick(1);
        let z = summon(&mut h.game, &ZOMBIE, [9.5, 201.0, 5.5]).unwrap();
        for _ in 0..300 {
            h.tick(1);
            assert_eq!(mob(&h, z).attack_target, None);
        }
        assert_eq!(health(&h), 20.0);
        // A hit does not make a creative player a revenge target either.
        let before = mob(&h, z).revenge_timer;
        assert!(hurt_mob(&mut h.game, z, &player_src(), 1.0));
        assert_eq!(mob(&h, z).revenge_target, Some(1));
        assert_ne!(mob(&h, z).revenge_timer, before - 1);
        h.tick(30);
        assert_eq!(mob(&h, z).attack_target, None);
    });
}

#[test]
fn a_spectator_is_not_a_target() {
    run(|| {
        let mut h = setup(NIGHT);
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_SPECTATOR });
        h.tick(1);
        let z = summon(&mut h.game, &ZOMBIE, [9.5, 201.0, 5.5]).unwrap();
        h.tick(200);
        assert_eq!(mob(&h, z).attack_target, None);
    });
}

#[test]
fn hurt_by_target_retargets_the_attacker_and_calls_for_help() {
    run(|| {
        let mut h = setup(NIGHT);
        // Only the revenge goal, so the nearest-player lookup cannot be what retargets.
        let a = summon(&mut h.game, &ZOMBIE, [12.5, 201.0, 5.5]).unwrap();
        let b = summon(&mut h.game, &ZOMBIE, [12.5, 201.0, 8.5]).unwrap();
        for m in h.game.mobs.mobs.iter_mut() {
            m.target_tasks = Tasks::new();
            m.target_tasks.add_task(1, HurtByTarget::new(true));
        }
        h.tick(3);
        assert_eq!(mob(&h, a).attack_target, None);
        assert!(hurt_mob(&mut h.game, a, &player_src(), 1.0));
        h.tick(4);
        assert_eq!(mob(&h, a).attack_target, Some(1));
        assert_eq!(mob(&h, b).attack_target, Some(1), "the zombie next to it was alerted");
        // The goal runs again for a fresh revenge timer after the target is lost.
        h.game.mobs.mobs.iter_mut().find(|m| m.id == a).unwrap().attack_target = None;
        h.tick(60);
        assert!(hurt_mob(&mut h.game, a, &player_src(), 5.0), "the hurt window is over");
        h.tick(4);
        assert_eq!(mob(&h, a).attack_target, Some(1));
    });
}

/// Calls `pre_update` for `n` ticks on a zombie standing at `pos`; true when it caught fire.
fn burns(h: &mut Harness, pos: [f64; 3], sky: i32, n: usize) -> bool {
    let mut z = Mob::new(900, def_of(&ZOMBIE).unwrap(), pos, 0.0);
    let mut rand = JavaRandom::new(5);
    for _ in 0..n {
        zombie::pre_update(&mut h.game, &mut z, &mut rand, sky);
    }
    z.fire > 0
}

#[test]
fn a_zombie_burns_in_daylight_with_sky_access_and_not_under_a_roof() {
    run(|| {
        let mut h = setup(DAY);
        for x in 8..=12 {
            for z in 8..=12 {
                terrain::set_block(&mut h.game, (x, 204, z), STONE);
            }
        }
        h.tick(1);
        let day = crate::game::mobs::spawner::sky_subtracted(&h.game);
        assert!(zombie::is_daytime(day));
        assert!(burns(&mut h, [5.5, 201.0, 11.5], day, 400), "open sky in daylight");
        assert!(!burns(&mut h, [10.5, 201.0, 10.5], day, 400), "under a roof");
        let night = 11;
        assert!(!zombie::is_daytime(night));
        assert!(!burns(&mut h, [5.5, 201.0, 11.5], night, 400), "at night");
    });
}

#[test]
fn daylight_sets_a_bare_headed_zombie_alight_within_a_seeded_window() {
    run(|| {
        let mut h = setup(DAY);
        h.tick(1);
        let day = crate::game::mobs::spawner::sky_subtracted(&h.game);
        // 0.04 a tick (full brightness): alight inside 200 ticks for this seed, not before the first draw.
        assert!(!burns(&mut h, [5.5, 201.0, 11.5], day, 0));
        assert!(burns(&mut h, [5.5, 201.0, 11.5], day, 200));
        // A baby zombie does not burn.
        let mut baby = Mob::new(901, def_of(&ZOMBIE).unwrap(), [5.5, 201.0, 11.5], 0.0);
        baby.set_growing_age(-24000);
        let mut rand = JavaRandom::new(5);
        for _ in 0..400 {
            zombie::pre_update(&mut h.game, &mut baby, &mut rand, day);
        }
        assert_eq!(baby.fire, 0);
    });
}

#[test]
fn a_helmet_takes_the_sun_instead_of_the_zombie_catching_fire() {
    run(|| {
        let mut h = setup(DAY);
        h.tick(1);
        let day = crate::game::mobs::spawner::sky_subtracted(&h.game);
        let mut z = Mob::new(902, def_of(&ZOMBIE).unwrap(), [5.5, 201.0, 11.5], 0.0);
        z.head = items::ItemStack::of(item::IRON_HELMET);
        let mut rand = JavaRandom::new(5);
        for _ in 0..400 {
            zombie::pre_update(&mut h.game, &mut z, &mut rand, day);
        }
        assert_eq!(z.fire, 0, "the helmet absorbed every burn");
        assert!(z.head.get_metadata() > 0, "worn: {}", z.head.get_metadata());
        // A helmet at the edge of breaking is gone after the next burn that rolls a point of damage.
        z.head.set_item_damage(z.head.get_max_damage() - 1);
        for _ in 0..400 {
            zombie::pre_update(&mut h.game, &mut z, &mut rand, day);
            if z.head.is_empty() {
                break;
            }
        }
        assert!(z.head.is_empty(), "broke");
        assert_eq!(z.fire, 0, "the tick it broke it still did not burn");
        for _ in 0..400 {
            zombie::pre_update(&mut h.game, &mut z, &mut rand, day);
        }
        assert!(z.fire > 0, "bare headed it burns");
    });
}

#[test]
fn a_burning_zombie_takes_fire_damage_and_shows_the_flag() {
    run(|| {
        let mut h = setup(NIGHT);
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        let z = summon(&mut h.game, &ZOMBIE, [9.5, 201.0, 9.5]).unwrap();
        h.tick(3);
        h.game.mobs.mobs.iter_mut().find(|m| m.id == z).unwrap().set_fire(8);
        h.tick(1);
        assert!(mob(&h, z).meta.flags & MM_BURNING != 0);
        h.tick(40);
        // A burn at fire 160, 140 and 120 (each 1.0 less the armor 2 reduction, 0.94).
        let hp = mob(&h, z).health;
        assert!((hp - (20.0 - 3.0 * 0.94)).abs() < 0.01, "{hp}");
        h.tick(200);
        assert_eq!(mob(&h, z).fire, 0);
        assert_eq!(mob(&h, z).meta.flags & MM_BURNING, 0);
        // Water puts it out.
        h.game.mobs.mobs.iter_mut().find(|m| m.id == z).unwrap().set_fire(8);
        terrain::set_block(&mut h.game, (9, 201, 9), 9 << 4);
        h.tick(3);
        assert_eq!(mob(&h, z).fire, 0);
    });
}

#[test]
fn armor_trims_damage_and_the_loot_is_rotten_flesh() {
    run(|| {
        let mut h = setup(NIGHT);
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        let z = summon(&mut h.game, &ZOMBIE, [9.5, 201.0, 9.5]).unwrap();
        assert!(hurt_mob(&mut h.game, z, &player_src(), 10.0));
        // damage 10 vs armor 2: 10 * (1 - 0.4 / 25)
        assert!((mob(&h, z).health - (20.0 - 9.84)).abs() < 1e-4, "{}", mob(&h, z).health);
        let mut flesh = Vec::new();
        for n in 0..40 {
            let id = summon(&mut h.game, &ZOMBIE, [6.5 + (n % 6) as f64, 201.0, 12.5]).unwrap();
            let before: i32 = h.game.items.entities.iter().filter(|e| e.stack.item == item::ROTTEN_FLESH).map(|e| e.stack.count).sum();
            assert!(hurt_mob(&mut h.game, id, &player_src(), 100.0));
            let after: i32 = h.game.items.entities.iter().filter(|e| e.stack.item == item::ROTTEN_FLESH).map(|e| e.stack.count).sum();
            flesh.push(after - before);
        }
        assert!(flesh.iter().all(|&f| (0..=2).contains(&f)) && flesh.contains(&0) && flesh.contains(&2), "{flesh:?}");
    });
}

/// A zombie knocked back by the player keeps the player as its target, and once it lands its
/// next path ends at the player instead of the goal standing idle on a failed airborne re-path.
#[test]
fn a_zombie_hit_by_the_player_keeps_its_target_and_paths_back_to_the_player() {
    run(|| {
        let mut h = setup(NIGHT);
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 1000;
        let z = summon(&mut h.game, &ZOMBIE, [9.5, 201.0, 5.5]).unwrap();
        // Only the revenge goal, so the hit is what gives it a target.
        for m in h.game.mobs.mobs.iter_mut() {
            m.target_tasks = Tasks::new();
            m.target_tasks.add_task(1, HurtByTarget::new(true));
        }
        h.tick(20);
        let p = h.game.sessions.get(&1).unwrap().state.unwrap().pos;
        let src = DamageSource::player(Attacker { name: "Xy".into(), pos: p, player: true, id: Some(1) });
        assert!(hurt_mob(&mut h.game, z, &src, 1.0));
        let (mut landed, mut pathed) = (None, None);
        for n in 0..40 {
            h.tick(1);
            let m = mob(&h, z);
            if n >= 2 {
                assert_eq!(m.attack_target, Some(1), "tick {n}: the attacker stays the target");
            }
            if landed.is_none() && m.body.on_ground && n > 2 {
                landed = Some(n);
            }
            if pathed.is_none() {
                if let Some(f) = m.navigator.get_path().and_then(|p| p.get_final_path_point().map(|f| (f.x, f.z))) {
                    let (dx, dz) = (f64::from(f.0) + 0.5 - p[0], f64::from(f.1) + 0.5 - p[2]);
                    assert!((dx * dx + dz * dz).sqrt() <= 2.0, "path ends at {f:?}, the player is at {p:?}");
                    pathed = Some(n);
                }
            }
        }
        let (landed, pathed) = (landed.expect("landed"), pathed.expect("never pathed back"));
        assert!(pathed <= landed + 12, "landed on tick {landed}, pathed on {pathed}");
    });
}
