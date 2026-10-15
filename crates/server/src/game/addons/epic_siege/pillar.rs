//! ESM-14, the pillaring: `ai/ESM_EntityAIPillarUp.java` and its addition
//! `ai/additions/AdditionPillaring.java`, ported line by line.
//!
//! A mob that cannot path to a target which is above it, or across a gap it cannot cross, builds a
//! pillar of the configured block under itself: every 15 ticks it steps up onto the block it just
//! placed and re-paths. The goal is not interruptible, so nothing else takes the mob out of the
//! climb. Everything is gated by the settings ESM reads (`Pillar Up`, `Pillar Block`).
//!
//! DROPPED: the live `Pillar Block` config reload (`ESM_EntityAIPillarUp.updatePillarBlock`,
//! `updateBlock`): the port reads the block id out of its const configuration once, at build time.
//! `Entity.setPositionAndUpdate` is a plain `Entity.setPosition` here (the tracker sends the move
//! on its own schedule). The step up and the block it stands on are one `Effect::StepUp`, applied
//! after the mob tick: see `update_task`.

use sim::collision::floor;
use sim::collision::BlockAccess;
use worldgen::populate::state::{self as st, Facing};

use crate::game::mobs::ai::{dist_sq, Ctx, Effect, Goal};
use super::config::Config;

/// `ESM_EntityAIPillarUp.placeSurface`: down, north, east, south, west.
const PLACE_SURFACE: [Facing; 5] =
    [Facing::Down, Facing::North, Facing::East, Facing::South, Facing::West];

/// `ESM_EntityAIPillarUp`, the addition at task priority 4. `mutexBits` is 0, as in the mod.
#[derive(Clone, Debug)]
pub struct PillarUp {
    /// `target`, the attack target the goal last saw.
    target: Option<u32>,
    place_delay: i32,
    /// `blockPos`: where the next block goes, found by `should_execute`.
    block_pos: Option<(i32, i32, i32)>,
    /// `Pillar Block`
    pillar_block: u16,
}

impl PillarUp {
    pub fn new(cfg: &Config) -> PillarUp {
        PillarUp { target: None, place_delay: 15, block_pos: None, pillar_block: cfg.pillar_block }
    }

    fn at(p: (i32, i32, i32), dx: i32, dy: i32, dz: i32) -> (i32, i32, i32) {
        (p.0 + dx, p.1 + dy, p.2 + dz)
    }

    /// `IBlockState.isFullBlock()`
    fn full(c: &Ctx<'_>, at: (i32, i32, i32)) -> bool {
        st::is_full_block(c.w.state(at.0, at.1, at.2))
    }

    /// `IBlockState.getMaterial().isReplaceable()`
    fn replaceable(c: &Ctx<'_>, at: (i32, i32, i32)) -> bool {
        st::mat_replaceable(c.w.state(at.0, at.1, at.2))
    }

    /// `IBlockState.getMaterial().blocksMovement()`
    fn blocks_movement(c: &Ctx<'_>, at: (i32, i32, i32)) -> bool {
        st::blocks_movement(c.w.state(at.0, at.1, at.2))
    }

    /// `Entity.getDistance(x, y, z)`
    fn dist_to(c: &Ctx<'_>, at: [f64; 3]) -> f64 {
        let m = c.mob.body.pos;
        dist_sq([m.x, m.y, m.z], at).sqrt()
    }
}

impl Goal for PillarUp {
    fn mutex_bits(&self) -> u32 {
        0
    }

    fn is_interruptible(&self) -> bool {
        false
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        self.target = c.mob.attack_target;
        let Some(t) = self.target.and_then(|id| c.player(id)) else { return false };
        if t.dead {
            return false;
        }
        let m = c.mob.body.pos;
        // noPath && ((within 4 blocks on the ground) || in lava || in water)
        let close = Self::dist_to(c, [t.pos[0], m.y, t.pos[2]]) < 4.0;
        if !(c.mob.navigator.no_path()
            && (close && c.mob.body.on_ground || c.mob.body.is_in_lava(c.w) || c.mob.body.in_water))
        {
            return false;
        }
        let org = (floor(m.x), floor(m.y), floor(m.z));
        let mut tmp = org;
        let x_off = (floor(t.pos[0]) - org.0).signum();
        let z_off = (floor(t.pos[2]) - org.2).signum();
        let mut can_place = false;
        for dir in PLACE_SURFACE {
            let o = dir.offset();
            if Self::full(c, Self::at(tmp, o.0, o.1, o.2)) {
                can_place = true;
                break;
            }
        }
        // Standing on solid ground: the surface block is the one at the target's side, when that
        // spot is free.
        if t.pos[1] - m.y < 16.0
            && Self::full(c, Self::at(tmp, 0, -2, 0))
            && Self::full(c, Self::at(tmp, 0, -1, 0))
        {
            if Self::replaceable(c, Self::at(tmp, x_off, -1, 0)) {
                tmp = Self::at(tmp, x_off, -1, 0);
            } else if Self::replaceable(c, Self::at(tmp, 0, -1, z_off)) {
                tmp = Self::at(tmp, 0, -1, z_off);
            } else if t.pos[1] <= m.y {
                return false;
            }
        } else if t.pos[1] <= m.y {
            return false;
        }
        if !can_place
            || Self::blocks_movement(c, Self::at(org, 0, 2, 0))
            || Self::blocks_movement(c, Self::at(tmp, 0, 2, 0))
        {
            return false;
        }
        self.block_pos = Some(tmp);
        true
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        self.should_execute(c)
    }

    fn start_executing(&mut self, _c: &mut Ctx<'_>) {
        self.place_delay = 15;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        if self.place_delay > 0 || self.target.is_none() {
            self.place_delay -= 1;
            return;
        }
        let Some(at) = self.block_pos else { return };
        let Some(t) = self.target.and_then(|id| c.player(id)) else { return };
        self.place_delay = 15;
        // `setPositionAndUpdate(x + 0.5, y + 1, z + 0.5)` and the `setBlockState(at, block)` under
        // it, as one step: the goal cannot write the world, and the mob's own `move` for this tick
        // has not run yet, so a block pushed on its own would land a phase too late and drop the
        // mob again. `Effect::StepUp` runs the pair after the mob tick, where both hold.
        if Self::replaceable(c, at) {
            let to = [at.0 as f64 + 0.5, at.1 as f64 + 1.0, at.2 as f64 + 0.5];
            c.fx.push(Effect::StepUp { mob: c.mob.id, at, state: self.pillar_block, to });
        }
        let speed = c.mob.move_helper.get_speed();
        let path = c.mob.get_path_to_entity_living(c.w, t.pos);
        c.mob.set_path(c.w, path, speed);
    }
}
