//! Pathfinding over small hand-placed worlds, then a cow that walks a path in the game harness.

use std::collections::HashMap;

use glam::DVec3;
use proto::{PlayerState, ON_GROUND};
use sim::collision::{Aabb, BlockAccess};
use sim::kinds::COW;
use worldgen::blocks::{id, state, STONE};

use super::finder::PathFinder;
use super::node::{NavEntity, NodeProcessor, PathNodeType, Priorities};
use super::swim::SwimNodeProcessor;
use super::walk::WalkNodeProcessor;
use crate::game::mobs as gm;
use crate::game::terrain;
use crate::game::testing::{run, Harness};

#[derive(Default)]
struct World(HashMap<(i32, i32, i32), u16>);

impl BlockAccess for World {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        *self.0.get(&(x, y, z)).unwrap_or(&0)
    }
    fn loaded(&self, _: i32, _: i32) -> bool {
        true
    }
}

impl World {
    /// Stone at y 0 under -12..=12: the mobs stand on y 1.
    fn flat() -> World {
        let mut w = World::default();
        for x in -12..=12 {
            for z in -12..=12 {
                w.0.insert((x, 0, z), STONE);
            }
        }
        w
    }
    fn set(&mut self, x: i32, y: i32, z: i32, s: u16) {
        self.0.insert((x, y, z), s);
    }
}

fn entity(w: f32, h: f32, x: i32, y: i32, z: i32) -> NavEntity {
    let (cx, cz) = (x as f64 + 0.5, z as f64 + 0.5);
    let r = (w / 2.0) as f64;
    NavEntity {
        pos: DVec3::new(cx, y as f64, cz),
        bb: Aabb::new(cx - r, y as f64, cz - r, cx + r, y as f64 + h as f64, cz + r),
        width: w,
        height: h,
        eye_height: h * 0.9,
        step_height: 0.6,
        on_ground: true,
        in_water: false,
        in_lava: false,
        max_fall_height: 3,
        follow_range: 16.0,
        movement_speed: 0.2,
        priorities: Priorities::default(),
        yaw: 0.0,
        pitch: 0.0,
        head_yaw: 0.0,
        render_yaw_offset: 0.0,
        ai_move_speed: 0.0,
        move_forward: 0.0,
        move_strafing: 0.0,
        is_jumping: false,
    }
}

/// A cow standing at the cell.
fn cow(x: i32, y: i32, z: i32) -> NavEntity {
    entity(0.9, 1.4, x, y, z)
}

fn find(w: &World, e: &mut NavEntity, to: (i32, i32, i32), max: f32) -> Option<Vec<(i32, i32, i32)>> {
    let mut f = PathFinder::new(WalkNodeProcessor::new());
    f.find_path_to_pos(w, e, to, max).map(|p| p.cells())
}

#[test]
fn a_path_goes_around_a_wall() {
    let mut w = World::flat();
    for z in -3..=3 {
        for y in 1..=2 {
            w.set(5, y, z, STONE);
        }
    }
    let cells = find(&w, &mut cow(0, 1, 0), (10, 1, 0), 24.0).expect("a path");
    assert_eq!(cells.first(), Some(&(0, 1, 0)));
    assert_eq!(cells.last(), Some(&(10, 1, 0)));
    assert!(cells.iter().all(|c| !(c.0 == 5 && c.2.abs() <= 3)), "{cells:?}");
    assert!(cells.iter().any(|c| c.0 == 5 && c.2.abs() == 4), "it passes the wall's end: {cells:?}");
}

#[test]
fn a_path_steps_up_one_block() {
    let mut w = World::flat();
    for x in 3..=12 {
        for z in -12..=12 {
            w.set(x, 1, z, STONE);
        }
    }
    let cells = find(&w, &mut cow(0, 1, 0), (6, 2, 0), 16.0).expect("a path");
    assert_eq!(cells.last(), Some(&(6, 2, 0)));
    let up = cells.windows(2).find(|p| p[1].1 == 2 && p[0].1 == 1).expect("a rise");
    assert_eq!(up[1].0, 3, "the rise is onto the step's edge: {cells:?}");
}

#[test]
fn a_path_does_not_cross_a_two_high_wall() {
    let mut w = World::flat();
    for z in -12..=12 {
        for y in 1..=2 {
            w.set(5, y, z, STONE);
        }
    }
    let cells = find(&w, &mut cow(0, 1, 0), (10, 1, 0), 24.0);
    assert!(cells.map_or(true, |c| c.iter().all(|p| p.0 < 5)), "nothing is on or past the wall");
    // The same wall one block high is a step, and is crossed.
    let mut low = World::flat();
    for z in -12..=12 {
        low.set(5, 1, z, STONE);
    }
    let cells = find(&low, &mut cow(0, 1, 0), (10, 1, 0), 24.0).expect("a path over the step");
    assert_eq!(cells.last(), Some(&(10, 1, 0)));
}

#[test]
fn a_path_refuses_lava() {
    let mut w = World::flat();
    for x in 3..=5 {
        for z in -2..=2 {
            w.set(x, 0, z, state(id::LAVA, 0));
        }
    }
    let cells = find(&w, &mut cow(0, 1, 0), (9, 1, 0), 24.0).expect("a path round");
    assert_eq!(cells.last(), Some(&(9, 1, 0)));
    assert!(cells.iter().all(|c| !((3..=5).contains(&c.0) && c.2.abs() <= 2)), "{cells:?}");
    let p = WalkNodeProcessor::new();
    assert_eq!(p.get_path_node_type(&w, 4, 0, 0), PathNodeType::Lava);
    assert_eq!(PathNodeType::Lava.priority(), -1.0);
}

#[test]
fn a_path_avoids_fire_and_cactus() {
    let mut w = World::flat();
    w.set(4, 1, 0, state(id::FIRE, 0));
    w.set(4, 1, 2, state(id::CACTUS, 0));
    let p = WalkNodeProcessor::new();
    assert_eq!(p.get_path_node_type(&w, 4, 1, 0), PathNodeType::DamageFire);
    assert_eq!(p.get_path_node_type(&w, 3, 1, 0), PathNodeType::DangerFire);
    assert_eq!(p.get_path_node_type(&w, 4, 1, 2), PathNodeType::DamageCactus);
    assert_eq!(p.get_path_node_type(&w, 4, 1, 3), PathNodeType::DangerCactus);
    let cells = find(&w, &mut cow(0, 1, 0), (8, 1, 0), 24.0).expect("a path");
    assert_eq!(cells.last(), Some(&(8, 1, 0)));
    assert!(!cells.contains(&(4, 1, 0)), "{cells:?}");
    assert!(!cells.contains(&(4, 1, 2)), "{cells:?}");
    assert!(!cells.contains(&(3, 1, 0)) && !cells.contains(&(5, 1, 0)), "the cells beside fire cost 8 each: {cells:?}");
}

#[test]
fn a_cactus_corridor_is_closed() {
    let mut w = World::flat();
    for z in -12..=12 {
        w.set(4, 1, z, state(id::CACTUS, 0));
    }
    let cells = find(&w, &mut cow(0, 1, 0), (8, 1, 0), 24.0);
    assert!(cells.map_or(true, |c| c.iter().all(|p| p.0 < 4)));
}

#[test]
fn water_costs_a_cow_but_not_a_chicken() {
    let mut w = World::flat();
    for x in 3..=5 {
        for z in -1..=1 {
            w.set(x, 1, z, state(id::WATER, 0));
        }
    }
    let in_pond = |c: &(i32, i32, i32)| (3..=5).contains(&c.0) && c.2.abs() <= 1;
    // A cow (water 8) walks round.
    let cells = find(&w, &mut cow(0, 1, 0), (8, 1, 0), 24.0).expect("a path");
    assert_eq!(cells.last(), Some(&(8, 1, 0)));
    assert!(!cells.iter().any(in_pond), "{cells:?}");
    // A chicken (EntityChicken sets water to 0) goes straight through.
    let mut chicken = entity(0.4, 0.7, 0, 1, 0);
    chicken.priorities.set(PathNodeType::Water, 0.0);
    let cells = find(&w, &mut chicken, (8, 1, 0), 24.0).expect("a path");
    assert_eq!(cells.last(), Some(&(8, 1, 0)));
    assert!(cells.iter().filter(|c| in_pond(c)).count() >= 3, "{cells:?}");
    // postProcess puts the water priority back.
    assert_eq!(chicken.priorities.get(PathNodeType::Water), 0.0);
    assert_eq!(cow(0, 1, 0).priorities.get(PathNodeType::Water), 8.0);
}

#[test]
fn a_mob_will_not_drop_more_than_its_fall_height() {
    // The floor is at y 20; a pit across the way has its floor at y 12 (8 down), then one at
    // y 18 (2 down), which a cow takes (getMaxFallHeight is 3).
    let build = |pit_floor: i32| {
        let mut w = World::default();
        for x in -12..=12 {
            for z in -12..=12 {
                w.set(x, 20, z, STONE);
                if (3..=5).contains(&x) {
                    w.0.remove(&(x, 20, z));
                    w.set(x, pit_floor, z, STONE);
                }
            }
        }
        w
    };
    let deep = build(12);
    let cells = find(&deep, &mut cow(0, 21, 0), (9, 21, 0), 24.0);
    assert!(cells.map_or(true, |c| c.iter().all(|p| p.0 < 3)), "a cow does not walk into an 8 block pit");
    let shallow = build(18);
    let cells = find(&shallow, &mut cow(0, 21, 0), (9, 21, 0), 24.0).expect("a path through the shallow pit");
    assert!(cells.iter().any(|c| c.1 == 19 && (3..=5).contains(&c.0)), "{cells:?}");
}

#[test]
fn the_search_stops_after_200_nodes() {
    let w = World::flat();
    let mut e = cow(-11, 1, -11);
    // Reachable on a flat floor, but too far for a 200 node search: the path is the best node
    // reached, which is short of the target.
    let cells = find(&w, &mut e, (11, 1, 11), 200.0).expect("a partial path");
    assert_ne!(cells.last(), Some(&(11, 1, 11)));
    assert!(cells.len() > 3);
}

#[test]
fn a_swimmer_finds_a_path_through_water_only() {
    let mut w = World::default();
    for x in 0..=6 {
        for y in 1..=3 {
            w.set(x, y, 0, state(id::WATER, 0));
        }
    }
    let mut e = entity(0.5, 0.5, 0, 2, 0);
    e.bb = Aabb::new(0.0, 2.0, 0.0, 0.5, 2.5, 0.5);
    let mut f = PathFinder::new(SwimNodeProcessor::new());
    let cells = f.find_path(&w, &mut e, 6.25, 2.0, 0.25, 32.0).expect("a path").cells();
    assert_eq!(cells.last(), Some(&(6, 2, 0)));
    assert!(cells.iter().all(|c| w.state(c.0, c.1, c.2) == state(id::WATER, 0)));
}

#[test]
fn heap_orders_by_distance_to_target() {
    use super::heap::PathHeap;
    use super::point::PathPoint;
    let mut arena: Vec<PathPoint> = (0..20).map(|i| PathPoint::new(i, 0, 0)).collect();
    let order = [7, 3, 19, 0, 12, 5, 8, 1, 16, 2];
    let mut h = PathHeap::default();
    for &i in &order {
        arena[i].distance_to_target = ((i * 37) % 23) as f32;
        h.add_point(&mut arena, i);
    }
    h.change_distance(&mut arena, 19, -1.0);
    h.change_distance(&mut arena, 0, 100.0);
    let mut out = Vec::new();
    while !h.is_path_empty() {
        let i = h.dequeue(&mut arena);
        out.push(arena[i].distance_to_target);
    }
    assert_eq!(out.len(), order.len());
    assert!(out.windows(2).all(|p| p[0] <= p[1]), "{out:?}");
    assert_eq!(out[0], -1.0);
    assert_eq!(*out.last().unwrap(), 100.0);
}

#[test]
fn atan2_matches_the_table_version_closely() {
    for (y, x) in [(1.0f64, 1.0f64), (-2.0, 0.5), (0.3, -4.0), (-1.0, -1.0), (0.0, 1.0)] {
        assert!((super::math::atan2(y, x) - y.atan2(x)).abs() < 1e-4, "{y} {x}");
    }
}

const STONE_ID: u16 = 1 << 4;

#[test]
fn a_cow_walks_to_a_target_over_ticks() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.world.lock().unwrap().set_rule("doMobSpawning", "false");
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        for x in 4..=13 {
            for z in 4..=13 {
                terrain::set_block(&mut h.game, (x, 200, z), STONE_ID);
            }
        }
        // A one block step on the way: x 8..=9 is raised.
        for x in 8..=9 {
            for z in 4..=13 {
                terrain::set_block(&mut h.game, (x, 201, z), STONE_ID);
            }
        }
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 202.0, 5.5], flags: ON_GROUND, ..Default::default() });
        h.tick(1);
        gm::summon(&mut h.game, &COW, [12.5, 203.0, 12.5]).expect("loaded");
        h.tick(40);
        assert!(h.game.mobs.mobs[0].body.on_ground);
        assert_eq!(h.game.mobs.mobs[0].body.pos.y, 201.0);

        let mut m = h.game.mobs.mobs[0].clone();
        let ok = m.try_move_to(&terrain::Blocks(&h.game), 6.5, 201.0, 12.5, 1.0);
        h.game.mobs.mobs[0] = m;
        assert!(ok, "a path to the target");
        let mut jumped = false;
        for _ in 0..240 {
            h.tick(1);
            jumped |= h.game.mobs.mobs[0].body.pos.y > 201.9;
        }
        let m = &h.game.mobs.mobs[0];
        assert!((m.body.pos.x - 6.5).abs() < 1.0 && (m.body.pos.z - 12.5).abs() < 1.0, "the cow is at {:?}", m.body.pos);
        assert!(m.navigator.no_path(), "the path is finished");
        assert!(jumped || m.body.pos.y >= 201.0);
    });
}
