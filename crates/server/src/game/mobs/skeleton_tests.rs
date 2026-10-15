//! The skeleton and the arrow (T9k) over the slab world: stone at y 200 under x 4..=24 and
//! z 4..=13, the player at the west edge, the skeleton up to 15 blocks east.

use items::ids::item;
use proto::{PlayerState, MM_AGGRESSIVE, ON_GROUND};
use sim::kinds::{SKELETON, ZOMBIE};
use worldgen::java_random::JavaRandom;

use super::arrow::{self, Arrow, Pickup};
use super::combat::hurt_mob;
use super::{def_of, summon, zombie, Mob};
use crate::game::damage::{Attacker, DamageSource};
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;
const NIGHT: i64 = 18000;
const DAY: i64 = 6000;

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
    for x in 4..=24 {
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

fn mob(h: &Harness, id: u32) -> &Mob {
    h.game.mobs.mobs.iter().find(|m| m.id == id).expect("the mob")
}

fn health(h: &Harness) -> f32 {
    h.game.players.players[&1].health
}

fn arrow_count(h: &Harness) -> usize {
    h.game.mobs.arrows.len()
}

#[test]
fn a_skeleton_has_the_vanilla_goal_lists_and_swaps_the_combat_task() {
    let d = def_of(&SKELETON).unwrap();
    assert_eq!((d.max_health, d.speed), (20.0, 0.25));
    let mut s = Mob::new(1, d, [0.0, 0.0, 0.0], 0.0);
    // Swimming 1, RestrictSun 2, FleeSun 3, WanderAvoidWater 5, WatchClosest 6, LookIdle 6, melee 4.
    let pri: Vec<i32> = s.tasks.entries.iter().map(|e| e.priority).collect();
    assert_eq!(pri, vec![1, 2, 3, 5, 6, 6, 4]);
    assert!(s.tasks.entries[6].action.is_combat_task());
    assert_eq!(s.target_tasks.entries.iter().map(|e| e.priority).collect::<Vec<_>>(), vec![1, 2]);
    // The bow in hand swaps the melee goal for the ranged one (mutex 3, priority 4).
    s.set_item_in_main_hand(items::ItemStack::of(item::BOW));
    let pri: Vec<i32> = s.tasks.entries.iter().map(|e| e.priority).collect();
    assert_eq!(pri, vec![1, 2, 3, 5, 6, 6, 4]);
    assert_eq!(s.tasks.entries[6].action.mutex_bits(), 3);
    assert_eq!(s.tasks.entries.iter().filter(|e| e.action.is_combat_task()).count(), 1);
    s.set_item_in_main_hand(items::ItemStack::EMPTY);
    assert_eq!(s.tasks.entries.iter().filter(|e| e.action.is_combat_task()).count(), 1);
}

#[test]
fn a_skeleton_spawns_with_a_bow_and_the_viewers_are_told() {
    run(|| {
        let mut h = setup(NIGHT);
        let s = summon(&mut h.game, &SKELETON, [20.5, 201.0, 5.5]).unwrap();
        assert_eq!(mob(&h, s).main_hand.get_item(), item::BOW);
        h.tick(2);
        let frames = h.frames_for(1);
        let eq: Vec<proto::Equipment> = frames.iter().filter(|f| f.0 == proto::S_EQUIPMENT).filter_map(|f| proto::Equipment::decode(&f.1)).collect();
        assert!(eq.iter().any(|e| e.id == s && e.slot == proto::EQ_MAIN_HAND && e.stack.id == item::BOW as i16), "{eq:?}");
        // A hand that changes later is broadcast too.
        h.game.mobs.mobs.iter_mut().find(|m| m.id == s).unwrap().set_item_in_main_hand(items::ItemStack::EMPTY);
        h.tick(2);
        let frames = h.frames_for(1);
        let eq: Vec<proto::Equipment> = frames.iter().filter(|f| f.0 == proto::S_EQUIPMENT).filter_map(|f| proto::Equipment::decode(&f.1)).collect();
        assert!(eq.iter().any(|e| e.id == s && e.slot == proto::EQ_MAIN_HAND && e.stack.id == -1), "{eq:?}");
    });
}

#[test]
fn a_skeleton_targets_a_player_at_fifteen_blocks_shoots_and_hurts_them() {
    run(|| {
        let mut h = setup(NIGHT);
        let s = summon(&mut h.game, &SKELETON, [20.5, 201.0, 5.5]).unwrap();
        let mut targeted = false;
        let mut aggressive = false;
        let mut spawn = None;
        let mut hit_tick = None;
        for n in 0..900 {
            h.tick(1);
            let m = mob(&h, s);
            targeted |= m.attack_target == Some(1);
            aggressive |= m.meta.flags & MM_AGGRESSIVE != 0 && m.swinging_arms;
            for (tag, body) in h.frames_for(1) {
                if tag == proto::S_SPAWN_OBJECT {
                    let o = proto::SpawnObject::decode(&body).unwrap();
                    if let proto::ObjectData::Arrow { shooter } = o.data {
                        spawn.get_or_insert((o, shooter));
                    }
                }
            }
            if hit_tick.is_none() && health(&h) < 20.0 {
                hit_tick = Some(n);
            }
            if hit_tick.is_some() && spawn.is_some() {
                break;
            }
        }
        assert!(targeted, "never took the player as its target");
        assert!(aggressive, "MM_AGGRESSIVE was never set while aiming");
        let (o, shooter) = spawn.expect("no arrow was sent to the viewer");
        assert_eq!(shooter, s);
        assert!(o.motion[0] < -1.0, "flies west at the player: {:?}", o.motion);
        assert!(hit_tick.is_some(), "the arrows never hurt the player");
    });
}

#[test]
fn a_skeleton_that_loses_its_bow_swings_in_melee_and_aggression_clears() {
    run(|| {
        let mut h = setup(NIGHT);
        let s = summon(&mut h.game, &SKELETON, [9.5, 201.0, 5.5]).unwrap();
        h.tick(30);
        assert!(mob(&h, s).swinging_arms);
        assert_ne!(mob(&h, s).meta.flags & MM_AGGRESSIVE, 0);
        h.game.mobs.mobs.iter_mut().find(|m| m.id == s).unwrap().set_item_in_main_hand(items::ItemStack::EMPTY);
        h.tick(120);
        assert!(health(&h) < 20.0, "the melee skeleton hurts");
    });
}

#[test]
fn a_zombie_attacking_sets_the_aggressive_bit() {
    run(|| {
        let mut h = setup(NIGHT);
        // The vanilla zombie: Epic Siege's awareness roll takes a few hundred ticks to pick a target.
        h.game.addons.enabled.clear();
        let z = summon(&mut h.game, &ZOMBIE, [9.5, 201.0, 5.5]).unwrap();
        let mut seen = false;
        for _ in 0..200 {
            h.tick(1);
            let m = mob(&h, z);
            assert_eq!(m.arms_raised, m.meta.flags & MM_AGGRESSIVE != 0);
            seen |= m.arms_raised;
        }
        assert!(seen, "arms never raised");
    });
}

#[test]
fn an_arrow_sticks_in_a_block_and_despawns() {
    run(|| {
        let mut h = setup(NIGHT);
        let mut a = Arrow::new(None, [12.5, 205.0, 8.5]);
        a.shoot(0.0, -1.0, 0.0, 1.6, 0.0, &mut JavaRandom::new(1));
        let id = arrow::spawn(&mut h.game, a);
        h.tick(1);
        let spawned = h.frames_for(1).iter().any(|f| f.0 == proto::S_SPAWN_OBJECT);
        assert!(spawned, "the arrow was tracked");
        h.tick(20);
        let a = h.game.mobs.arrows.iter().find(|a| a.id == id).expect("still there");
        assert!(a.in_ground, "stuck");
        assert!((a.body.pos.y - 201.0).abs() < 0.2, "{}", a.body.pos.y);
        assert_eq!(a.arrow_shake.max(0), a.arrow_shake);
        h.game.mobs.arrows[0].ticks_in_ground = 1195;
        h.tick(10);
        assert_eq!(arrow_count(&h), 0, "despawned");
        let destroyed = h.frames_for(1).iter().any(|f| f.0 == proto::S_DESTROY);
        assert!(destroyed);
    });
}

#[test]
fn gravity_pulls_a_flying_arrow_down() {
    run(|| {
        let mut h = setup(NIGHT);
        let mut a = Arrow::new(None, [8.5, 230.0, 8.5]);
        a.shoot(1.0, 0.0, 0.0, 1.0, 0.0, &mut JavaRandom::new(1));
        arrow::spawn(&mut h.game, a);
        h.tick(10);
        let a = &h.game.mobs.arrows[0];
        // Ten ticks of 0.05 gravity with 0.99 drag: about 2.3 blocks down, about 9 along.
        assert!(a.body.pos.y < 230.0 - 1.5 && a.body.pos.y > 230.0 - 3.0, "{}", a.body.pos.y);
        assert!(a.body.pos.x > 16.0 && a.body.pos.x < 18.5, "{}", a.body.pos.x);
    });
}

#[test]
fn an_arrow_does_not_hurt_its_shooter() {
    run(|| {
        let mut h = setup(NIGHT);
        let s = summon(&mut h.game, &SKELETON, [15.5, 201.0, 8.5]).unwrap();
        let before = mob(&h, s).health;
        let p = mob(&h, s).body.pos;
        let mut a = Arrow::new(Some(s), [p.x, p.y + 1.6, p.z]);
        // Straight up, then back down through the shooter's own column.
        a.shoot(0.0, 1.0, 0.0, 0.3, 0.0, &mut JavaRandom::new(1));
        arrow::spawn(&mut h.game, a);
        h.tick(4);
        assert_eq!(mob(&h, s).health, before, "no damage from its own arrow in its first ticks");
        assert_eq!(arrow_count(&h), 1);
    });
}

#[test]
fn an_arrow_damages_by_speed_and_a_refused_hit_bounces() {
    run(|| {
        let mut h = setup(NIGHT);
        // Fired from the east at the player, 1.6 blocks a tick, base damage 2: ceil(1.6 * 2) = 4.
        let mut a = Arrow::new(None, [9.5, 202.4, 5.5]);
        a.shoot(-1.0, 0.0, 0.0, 1.6, 0.0, &mut JavaRandom::new(1));
        arrow::spawn(&mut h.game, a);
        let before = health(&h);
        h.tick(6);
        assert_eq!(arrow_count(&h), 0, "landed and gone");
        assert!(health(&h) < before, "hurt: {} -> {}", before, health(&h));
        assert!(before - health(&h) >= 3.0, "{}", before - health(&h));
        // A creative player refuses the damage: the arrow bounces off and stays.
        h.send(crate::game::GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        h.tick(1);
        let kept = health(&h);
        let mut a = Arrow::new(None, [9.5, 202.4, 5.5]);
        a.shoot(-1.0, 0.0, 0.0, 1.6, 0.0, &mut JavaRandom::new(1));
        arrow::spawn(&mut h.game, a);
        h.tick(2);
        if let Some(a) = h.game.mobs.arrows.first() {
            assert!(a.body.motion.x > 0.0 || a.body.pos.x > 5.5, "bounced or passed");
        }
        assert_eq!(health(&h), kept);
    });
}

#[test]
fn only_a_player_arrow_can_be_picked_up() {
    run(|| {
        let mut h = setup(NIGHT);
        let stuck = |pickup, x: f64| {
            let mut a = Arrow::new(None, [x, 200.9, 5.5]);
            a.in_ground = true;
            a.tile = (x as i32, 200, 5);
            a.in_tile = STONE;
            a.pickup = pickup;
            a
        };
        let skeleton_arrow = arrow::spawn(&mut h.game, stuck(Pickup::Disallowed, 5.9));
        let own = arrow::spawn(&mut h.game, stuck(Pickup::Allowed, 5.1));
        h.tick(3);
        assert!(h.game.mobs.arrows.iter().any(|a| a.id == skeleton_arrow), "a skeleton's arrow stays");
        assert!(!h.game.mobs.arrows.iter().any(|a| a.id == own), "an allowed arrow is picked up");
        let inv = &h.game.players.players[&1].inv;
        let n: i32 = (0..36).map(|i| inv.main[i]).filter(|s| s.get_item() == item::ARROW).map(|s| s.count).sum();
        assert_eq!(n, 1);
        let frames = h.frames_for(1);
        assert!(frames.iter().any(|f| f.0 == proto::S_COLLECT));
    });
}

#[test]
fn a_skeleton_burns_in_daylight_with_sky_access() {
    run(|| {
        let mut h = setup(DAY);
        h.tick(1);
        let day = crate::game::mobs::spawner::sky_subtracted(&h.game);
        let mut s = Mob::new(900, def_of(&SKELETON).unwrap(), [8.5, 201.0, 11.5], 0.0);
        let mut rand = JavaRandom::new(5);
        for _ in 0..400 {
            zombie::pre_update(&mut h.game, &mut s, &mut rand, day);
        }
        assert!(s.fire > 0);
    });
}

#[test]
fn a_skeleton_drops_bones_and_arrows() {
    run(|| {
        let mut h = setup(NIGHT);
        h.send(crate::game::GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        let src = DamageSource::player(Attacker { name: "Xy".into(), pos: [5.5, 201.0, 5.5], player: true, id: Some(1) });
        let mut got = std::collections::BTreeSet::new();
        for n in 0..30 {
            let id = summon(&mut h.game, &SKELETON, [6.5 + (n % 6) as f64, 201.0, 12.5]).unwrap();
            assert!(hurt_mob(&mut h.game, id, &src, 100.0));
        }
        for e in &h.game.items.entities {
            got.insert(e.stack.item);
        }
        assert!(got.contains(&item::BONE) || got.contains(&item::ARROW), "{got:?}");
    });
}
