//! Mob AI (T9g) over the slab world of tests.rs: stone at y 200 under x, z 4..=13, the player at
//! the west edge.

use items::ids::item;
use items::ItemStack;
use proto::{PlayerState, EV_LOVE, ON_GROUND};
use sim::kinds::COW;

use super::combat::hurt_mob;
use super::interact::process_mob_interact;
use super::summon;
use crate::game::damage::{Attacker, DamageSource};
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;

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

fn hold(h: &mut Harness, stack: ItemStack) {
    h.game.players.players.get_mut(&1).unwrap().inv.main[0] = stack;
}

fn cow_at(h: &mut Harness, x: f64, z: f64) -> u32 {
    let id = summon(&mut h.game, &COW, [x, 201.0, z]).expect("loaded");
    h.tick(3);
    id
}

fn mob(h: &Harness, id: u32) -> &super::Mob {
    h.game.mobs.mobs.iter().find(|m| m.id == id).expect("the mob")
}

#[test]
fn a_cow_has_the_vanilla_goal_list() {
    let m = super::Mob::new(1, super::def_of(&COW).unwrap(), [0.0; 3], 0.0);
    let pri: Vec<i32> = m.tasks.entries.iter().map(|e| e.priority).collect();
    let mutex: Vec<u32> = m.tasks.entries.iter().map(|e| e.action.mutex_bits()).collect();
    assert_eq!(pri, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(mutex, [4, 1, 3, 3, 0, 1, 2, 3]);
    assert!(m.navigator.get_can_swim());
}

#[test]
fn wander_picks_reachable_spots() {
    run(|| {
        let mut h = setup();
        let id = cow_at(&mut h, 9.5, 9.5);
        let start = mob(&h, id).body.pos;
        let mut moved = false;
        for _ in 0..6000 {
            h.tick(1);
            let m = mob(&h, id);
            let p = m.body.pos;
            // Every spot is on the slab: the walk never steps off the stone.
            assert!(p.x > 3.9 && p.x < 14.1 && p.z > 3.9 && p.z < 14.1, "left the slab at {p:?}");
            assert!(p.y >= 200.99 && p.y < 202.0, "fell or climbed to {p:?}");
            if (p.x - start.x).abs() > 0.5 || (p.z - start.z).abs() > 0.5 {
                moved = true;
            }
        }
        assert!(moved, "the cow never wandered");
    });
}

#[test]
fn a_hurt_cow_panics() {
    run(|| {
        let mut h = setup();
        let id = cow_at(&mut h, 9.5, 9.5);
        let src = DamageSource::player(Attacker { name: "Xy".into(), pos: [5.5, 201.0, 5.5], player: true, id: Some(1) });
        assert!(hurt_mob(&mut h.game, id, &src, 1.0));
        assert_eq!(mob(&h, id).revenge_target, Some(1));
        let start = mob(&h, id).body.pos;
        let mut panicked = false;
        for _ in 0..12 {
            h.tick(1);
            panicked |= mob(&h, id).tasks.is_executing(1);
        }
        assert!(panicked, "EntityAIPanic never started");
        let p = mob(&h, id).body.pos;
        assert!((p.x - start.x).hypot(p.z - start.z) > 1.0, "the cow stayed put");
    });
}

#[test]
fn a_cow_follows_wheat() {
    run(|| {
        let mut h = setup();
        hold(&mut h, ItemStack::new(item::WHEAT, 5, 0));
        let id = cow_at(&mut h, 11.5, 5.5);
        let d0 = (mob(&h, id).body.pos.x - 5.5).abs();
        let mut tempted = false;
        for _ in 0..120 {
            h.tick(1);
            tempted |= mob(&h, id).tasks.is_executing(3);
        }
        let d1 = (mob(&h, id).body.pos.x - 5.5).abs();
        assert!(tempted, "EntityAITempt never started");
        assert!(d1 < d0 - 2.0, "the cow did not come closer: {d0} -> {d1}");
        // It stops 2.5 blocks short.
        assert!(d1 > 1.5, "walked into the player: {d1}");
    });
}

#[test]
fn two_cows_in_love_make_a_baby_that_grows_up() {
    run(|| {
        let mut h = setup();
        let a = cow_at(&mut h, 10.5, 10.5);
        let b = cow_at(&mut h, 11.5, 10.5);
        h.frames_for(1);
        for id in [a, b] {
            hold(&mut h, ItemStack::new(item::WHEAT, 2, 0));
            assert!(process_mob_interact(&mut h.game, 1, id, 0));
            assert_eq!(mob(&h, id).in_love, 600);
        }
        h.tick(1);
        let love: Vec<_> = h
            .frames_for(1)
            .iter()
            .filter(|f| f.0 == proto::S_ENTITY_EVENT)
            .filter_map(|f| proto::EntityEvent::decode(&f.1))
            .filter(|e| e.status == EV_LOVE)
            .map(|e| e.id)
            .collect();
        assert!(love.contains(&a) && love.contains(&b), "love status not sent: {love:?}");
        for _ in 0..200 {
            h.tick(1);
            if h.game.mobs.mobs.len() == 3 {
                break;
            }
        }
        assert_eq!(h.game.mobs.mobs.len(), 3, "no baby");
        let baby = h.game.mobs.mobs.last().unwrap().id;
        let m = mob(&h, baby);
        assert!(m.is_child());
        assert_ne!(m.meta.flags & proto::MM_CHILD, 0);
        assert_eq!(m.body.height, 0.7);
        for id in [a, b] {
            let p = mob(&h, id);
            assert!(!p.is_in_love());
            assert!(p.growing_age > 5000, "parent cooldown {}", p.growing_age);
        }
        // 24000 ticks from the birth.
        let born = m.growing_age;
        assert_eq!(born, -24000);
        h.tick(23999);
        assert!(mob(&h, baby).is_child());
        h.tick(1);
        let m = mob(&h, baby);
        assert!(!m.is_child());
        assert_eq!(m.growing_age, 0);
        assert_eq!(m.meta.flags & proto::MM_CHILD, 0);
        assert_eq!(m.body.height, 1.4);
    });
}

#[test]
fn feeding_a_calf_speeds_its_growth() {
    run(|| {
        let mut h = setup();
        let id = cow_at(&mut h, 10.5, 10.5);
        h.game.mobs.mobs[0].set_growing_age(-24000);
        hold(&mut h, ItemStack::new(item::WHEAT, 1, 0));
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        // ageUp((int)(24000 / 20 * 0.1)) = 120 seconds.
        assert_eq!(mob(&h, id).growing_age, -24000 + 120 * 20);
    });
}

#[test]
fn a_bucket_on_a_cow_gives_milk() {
    run(|| {
        let mut h = setup();
        let id = cow_at(&mut h, 6.5, 5.5);
        hold(&mut h, ItemStack::new(item::BUCKET, 2, 0));
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        let inv = &h.game.players.players[&1].inv;
        assert_eq!(inv.main[0].get_item(), item::BUCKET);
        assert_eq!(inv.main[0].get_count(), 1);
        assert!(inv.main.iter().any(|s| !s.is_empty() && s.get_item() == item::MILK_BUCKET));
        hold(&mut h, ItemStack::new(item::BUCKET, 1, 0));
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(h.game.players.players[&1].inv.main[0].get_item(), item::MILK_BUCKET);
        // A calf gives none.
        h.game.mobs.mobs[0].set_growing_age(-100);
        hold(&mut h, ItemStack::new(item::BUCKET, 1, 0));
        assert!(!process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(h.game.players.players[&1].inv.main[0].get_item(), item::BUCKET);
    });
}
