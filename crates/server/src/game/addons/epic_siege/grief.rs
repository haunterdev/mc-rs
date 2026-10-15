//! ESM-14, the griefing: `ai/ESM_EntityAIGrief.java` and its addition
//! `ai/additions/AdditionGrief.java`, ported line by line.
//!
//! Every tick a mob with this goal has a one in four chance of picking a random block up to 16
//! blocks away, 8 up or down: a listed grief target, or anything that gives off light. It walks
//! there, works the block down at the same strength a dig does, and breaks it with the drops
//! `destroyBlock(pos, canHarvest)` gives. This is the goal that eats a player's crops, glass and
//! chests while the zombie is on its way in.
//!
//! DROPPED: the mob's block sound and `swingArm` every fifth tick, and the break particles and
//! sound of `World.destroyBlock` (`playEvent(2001, ...)`). The mod's `getBlockStrength` is the dig
//! goal's, shared through [`super::dig::block_strength`].

use sim::collision::{floor, BlockAccess};
use worldgen::populate::state as st;

use crate::game::mobs::ai::{dist_sq, Ctx, Effect, Goal};
use super::config::{block_name, strip, Config};
use super::dig::block_strength;

/// `ESM_EntityAIGrief`, the addition at task priority 4. `setMutexBits(5)` is MOVE and JUMP: the
/// goal owns the walk while it is running.
#[derive(Clone, Debug)]
pub struct Grief {
    /// `markedLoc`
    marked: Option<(i32, i32, i32)>,
    dig_tick: i32,
    /// `Grief Targets`
    targets: &'static [&'static str],
    /// `Requires Tools`
    requires_tools: bool,
}

impl Grief {
    pub fn new(cfg: &Config) -> Grief {
        Grief { marked: None, dig_tick: 0, targets: cfg.grief_blocks, requires_tools: cfg.dig_requires_tools }
    }

    /// `GRIEF_BLOCKS.contains(regName)`
    fn listed(&self, s: u16) -> bool {
        block_name(s).is_some_and(|n| self.targets.iter().any(|e| strip(e) == n))
    }

    /// `!DIG_TOOLS || !item.isEmpty() && item.canHarvestBlock(state, item)
    /// || state.getMaterial().isToolNotRequired()`
    fn tool_ok(&self, c: &Ctx<'_>, s: u16) -> bool {
        !self.requires_tools
            || (!c.mob.main_hand.is_empty() && items::dig::can_harvest(s, &c.mob.main_hand))
            || items::dig::is_tool_not_required(st::mat(s))
    }

    /// `Entity.getDistance(x, y, z)` from the mob's feet to the block's corner.
    fn dist_to(c: &Ctx<'_>, at: (i32, i32, i32)) -> f64 {
        let m = c.mob.body.pos;
        dist_sq([m.x, m.y, m.z], [at.0 as f64, at.1 as f64, at.2 as f64]).sqrt()
    }
}

impl Goal for Grief {
    fn mutex_bits(&self) -> u32 {
        5
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if c.rand.next_int_bounded(4) != 0 {
            return false;
        }
        let m = c.mob.body.pos;
        let cur = (floor(m.x), floor(m.y), floor(m.z));
        let (x, y, z) = (
            cur.0 + c.rand.next_int_bounded(32) - 16,
            cur.1 + c.rand.next_int_bounded(16) - 8,
            cur.2 + c.rand.next_int_bounded(32) - 16,
        );
        let s = c.w.state(x, y, z);
        let want = (self.listed(s) || st::light_value(s) > 0)
            && st::hardness(s) >= 0.0
            && !st::mat_liquid(s)
            && self.tool_ok(c, s);
        if !want {
            return false;
        }
        self.marked = Some((x, y, z));
        let speed = 1.0;
        c.mob.try_move_to(c.w, x as f64, y as f64, z as f64, speed);
        self.dig_tick = 0;
        true
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        if self.marked.is_none() || c.mob.health <= 0.0 {
            self.marked = None;
            return false;
        }
        let at = self.marked.unwrap_or_default();
        let s = c.w.state(at.0, at.1, at.2);
        if s >> 4 == 0 || (!self.listed(s) && st::light_value(s) <= 0) {
            self.marked = None;
            return false;
        }
        self.tool_ok(c, s)
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        let Some(at) = self.marked else { return };
        if !self.should_continue_executing(c) {
            self.dig_tick = 0;
            return;
        }
        if Self::dist_to(c, at) >= 3.0 {
            if c.mob.navigator.no_path() {
                c.mob.try_move_to(c.w, at.0 as f64, at.1 as f64, at.2 as f64, 1.0);
            }
            self.dig_tick = 0;
            return;
        }
        let state = c.w.state(at.0, at.1, at.2);
        self.dig_tick += 1;
        let str = block_strength(c, at) * (self.dig_tick as f32 + 1.0);
        if str >= 1.0 {
            self.dig_tick = 0;
            let drop = items::dig::is_tool_not_required(st::mat(state))
                || (!c.mob.main_hand.is_empty() && items::dig::can_harvest(state, &c.mob.main_hand));
            c.fx.push(Effect::Break { at, state, drop });
            self.marked = None;
        }
        // DROPPED: the block sound and `swingArm` on every fifth tick (`digTick % 5 == 0`).
    }
}
