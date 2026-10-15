//! `RandomPositionGenerator`.
//!
//! DROPPED: the home position branches (`hasHome`, `getHomePosition`, `isWithinHomeDistanceFromPosition`;
//! no mob here has a home), and `findRandomTargetBlockTowards` and `findRandomTargetBlockAwayFrom`
//! (no goal calls them yet).

use sim::collision::{floor, BlockAccess};
use sim::region::Mat;
use worldgen::java_random::JavaRandom;

use crate::game::mobs::path::blocks::material;
use crate::game::mobs::path::PathNavigateGround;
use crate::game::mobs::Mob;

/// `findRandomTarget`
pub fn find_random_target(mob: &Mob, w: &impl BlockAccess, rand: &mut JavaRandom, xz: i32, y: i32) -> Option<[f64; 3]> {
    generate_random_pos(mob, w, rand, xz, y, true)
}

/// `getLandPos`
pub fn get_land_pos(mob: &Mob, w: &impl BlockAccess, rand: &mut JavaRandom, xz: i32, y: i32) -> Option<[f64; 3]> {
    generate_random_pos(mob, w, rand, xz, y, false)
}

/// `generateRandomPos` with no target vector: ten random offsets, the standable one with the
/// best `getBlockPathWeight` wins.
fn generate_random_pos(mob: &Mob, w: &impl BlockAccess, rand: &mut JavaRandom, xz: i32, y: i32, flag4: bool) -> Option<[f64; 3]> {
    let p = mob.body.pos;
    let mut found = false;
    let mut f = -99999.0f32;
    let (mut k1, mut i, mut j) = (0, 0, 0);
    for _ in 0..10 {
        let l = rand.next_int_bounded(2 * xz + 1) - xz;
        let i1 = rand.next_int_bounded(2 * y + 1) - y;
        let j1 = rand.next_int_bounded(2 * xz + 1) - xz;
        // new BlockPos(double, double, double) floors.
        let mut pos = (floor(l as f64 + p.x), floor(i1 as f64 + p.y), floor(j1 as f64 + p.z));
        if PathNavigateGround::can_entity_stand_on_pos(w, pos) {
            if !flag4 {
                pos = move_above_solid(w, pos);
                if is_water_destination(w, pos) {
                    continue;
                }
            }
            let f1 = mob.get_block_path_weight(w, pos);
            if f1 > f {
                f = f1;
                k1 = l;
                i = i1;
                j = j1;
                found = true;
            }
        }
    }
    found.then(|| [k1 as f64 + p.x, i as f64 + p.y, j as f64 + p.z])
}

/// `moveAboveSolid`
fn move_above_solid(w: &impl BlockAccess, pos: (i32, i32, i32)) -> (i32, i32, i32) {
    if !crate::game::mobs::path::blocks::is_solid(w.state(pos.0, pos.1, pos.2)) {
        return pos;
    }
    let mut b = (pos.0, pos.1 + 1, pos.2);
    while b.1 < 256 && crate::game::mobs::path::blocks::is_solid(w.state(b.0, b.1, b.2)) {
        b.1 += 1;
    }
    b
}

/// `isWaterDestination`
fn is_water_destination(w: &impl BlockAccess, pos: (i32, i32, i32)) -> bool {
    material(w.state(pos.0, pos.1, pos.2)) == Mat::Water
}
