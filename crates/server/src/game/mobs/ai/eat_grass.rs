//! `EntityAIEatGrass`.
//!
//! DROPPED: `playEvent(2001)` (the block break particles and sound) when grass turns to dirt or
//! tall grass is destroyed, and `setBlockState`'s flag 2 (the change goes through the world's
//! normal neighbour updates, flags 3).

use worldgen::blocks::id;

use super::{Ctx, Effect, Goal};
use crate::game::terrain;

/// `BlockTallGrass` state of the `GRASS` type (`IS_TALL_GRASS`): block 31, metadata 1.
const TALL_GRASS: u16 = id::TALLGRASS << 4 | 1;
const DIRT: u16 = id::DIRT << 4;

/// `EntityAIEatGrass` (mutex 7).
#[derive(Clone, Debug)]
pub struct EatGrass {
    eating_grass_timer: i32,
}

impl EatGrass {
    pub fn new() -> EatGrass {
        EatGrass { eating_grass_timer: 0 }
    }

    pub fn get_eating_grass_timer(&self) -> i32 {
        self.eating_grass_timer
    }
}

impl Default for EatGrass {
    fn default() -> EatGrass {
        EatGrass::new()
    }
}

fn block_pos(c: &Ctx<'_>) -> (i32, i32, i32) {
    let p = c.mob.body.pos;
    (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
}

impl Goal for EatGrass {
    fn mutex_bits(&self) -> u32 {
        7
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        let bound = if c.mob.is_child() { 50 } else { 1000 };
        if c.rand.next_int_bounded(bound) != 0 {
            return false;
        }
        let (x, y, z) = block_pos(c);
        if terrain::state(c.w.0, (x, y, z)) == TALL_GRASS {
            return true;
        }
        terrain::state(c.w.0, (x, y - 1, z)) >> 4 == id::GRASS
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        self.eating_grass_timer = 40;
        c.fx.push(Effect::Event { id: c.mob.id, status: proto::EV_EAT_GRASS });
        c.mob.navigator.clear_path();
    }

    fn reset_task(&mut self, _c: &mut Ctx<'_>) {
        self.eating_grass_timer = 0;
    }

    fn should_continue_executing(&mut self, _c: &mut Ctx<'_>) -> bool {
        self.eating_grass_timer > 0
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        self.eating_grass_timer = (self.eating_grass_timer - 1).max(0);
        if self.eating_grass_timer == 4 {
            let (x, y, z) = block_pos(c);
            let griefing = terrain::rule(c.w.0, "mobGriefing", true);
            if terrain::state(c.w.0, (x, y, z)) == TALL_GRASS {
                if griefing {
                    // destroyBlock(pos, false): no drops.
                    c.fx.push(Effect::SetBlock { at: (x, y, z), state: worldgen::blocks::AIR });
                }
                c.mob.eat_grass_bonus();
            } else if terrain::state(c.w.0, (x, y - 1, z)) >> 4 == id::GRASS {
                if griefing {
                    c.fx.push(Effect::SetBlock { at: (x, y - 1, z), state: DIRT });
                }
                c.mob.eat_grass_bonus();
            }
        }
    }
}
