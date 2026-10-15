//! `EntityAIRestrictSun` and `EntityAIFleeSun`, for the skeleton (and any daylight burner).
//!
//! DROPPED: the head-slot checks (the head slot is empty, so a helmet never stops either goal),
//! and the `getBlockPathWeight(pos) < 0` test of `findPossibleShelter` (`EntityMob` reads the
//! light brightness, which needs the light cache the goals cannot borrow): any spot that cannot
//! see the sky counts as shelter.

use sim::collision::floor;

use super::{Ctx, Goal};
use crate::game::mobs::path::blocks;

/// `EntityAIRestrictSun`: by day the navigator avoids paths under open sky.
#[derive(Clone, Debug, Default)]
pub struct RestrictSun;

impl RestrictSun {
    pub fn new() -> RestrictSun {
        RestrictSun
    }
}

impl Goal for RestrictSun {
    fn mutex_bits(&self) -> u32 {
        0
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        c.is_daytime()
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        c.mob.navigator.set_avoid_sun(true);
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        c.mob.navigator.set_avoid_sun(false);
    }

    fn reset_on_remove(&mut self, mob: &mut crate::game::mobs::Mob) {
        mob.navigator.set_avoid_sun(false);
    }
}

/// `EntityAIFleeSun`
#[derive(Clone, Debug)]
pub struct FleeSun {
    speed: f64,
    shelter: [f64; 3],
}

impl FleeSun {
    pub fn new(speed: f64) -> FleeSun {
        FleeSun { speed, shelter: [0.0; 3] }
    }

    /// `findPossibleShelter`
    fn find_possible_shelter(&self, c: &mut Ctx<'_>) -> Option<[f64; 3]> {
        let (bx, by, bz) = (floor(c.mob.body.pos.x), floor(c.mob.body.bb.min_y), floor(c.mob.body.pos.z));
        for _ in 0..10 {
            let x = bx + c.rand.next_int_bounded(20) - 10;
            let y = by + c.rand.next_int_bounded(6) - 3;
            let z = bz + c.rand.next_int_bounded(20) - 10;
            if !blocks::can_see_sky(c.w, x, y, z) {
                return Some([x as f64, y as f64, z as f64]);
            }
        }
        None
    }
}

impl Goal for FleeSun {
    fn mutex_bits(&self) -> u32 {
        1
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if !c.is_daytime() || !c.mob.is_burning() {
            return false;
        }
        let b = &c.mob.body;
        if !blocks::can_see_sky(c.w, floor(b.pos.x), floor(b.bb.min_y), floor(b.pos.z)) {
            return false;
        }
        match self.find_possible_shelter(c) {
            Some(s) => {
                self.shelter = s;
                true
            }
            None => false,
        }
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        !c.mob.navigator.no_path()
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        let [x, y, z] = self.shelter;
        c.mob.try_move_to(c.w, x, y, z, self.speed);
    }
}
