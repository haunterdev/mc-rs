//! ESM-13, the digging: `ai/ESM_EntityAIDigging.java` with the block strength of
//! `ai/utils/AiUtils.java` and the addition `ai/additions/AdditionDigger.java`, ported line by
//! line. This is the system that makes a zombie come through a wall.
//!
//! The goal runs only while the navigator has no path to the target (vanilla's own goals have
//! given up). It then scans one ray a tick from its own body toward the target, keeps the first
//! harvestable block the ray hits, stands still for 20 ticks (so it does not dig while walking)
//! and works that block down at `getBlockStrength * (digTick + 1)`, sending the ordinary break
//! progress every fifth tick. A break destroys the block with `destroyBlock(pos, false)` and runs
//! `harvestBlock` beside it through the mining drop path, then re-paths toward the target.
//!
//! DROPPED: the break particles and sound of `World.playEvent(2001, ...)`, the block sound the mod
//! plays every fifth tick, `swingArm` (a mob has no swing packet) and the fake player's
//! `addStat`/`addExhaustion(0.005F)`. `DIG_BL` and `DIG_BL_INV` are a list of block registry names
//! in the mod; the port matches the same names against the `items` registry path.

use glam::DVec3;
use sim::collision::{floor, BlockAccess};
use sim::raytrace::{ray_trace_blocks, Kind as RayKind};
use worldgen::populate::state as st;

use crate::game::mobs::path::navigate::PathNavigateGround;

use crate::game::mobs::ai::{dist_sq, Ctx, Effect, Goal, PlayerView};
use super::config::{block_name, strip, Config};

/// The block a mob broke and the drops the caller asked for.
pub const AIR: u16 = 0;

/// `ESM_EntityAIDigging`, the addition at task priority 4. `mutexBits` is 0: the mod leaves it at
/// `EntityAIBase`'s default, so digging runs beside the wander goal.
#[derive(Clone, Debug)]
pub struct Digging {
    /// `target`, the attack target the goal last saw.
    target: Option<u32>,
    /// `curBlock`
    cur_block: Option<(i32, i32, i32)>,
    scan_tick: i32,
    dig_tick: i32,
    /// `obsPos` and `obsTick`: the block the mob has stood still on.
    obs_pos: Option<(i32, i32, i32)>,
    obs_tick: i32,
    /// `Dig Blacklist` and `Invert Blacklist`
    blacklist: &'static [&'static str],
    blacklist_inverted: bool,
    /// `Dig Requires Tools`
    requires_tools: bool,
}

impl Digging {
    pub fn new(cfg: &Config) -> Digging {
        Digging {
            target: None,
            cur_block: None,
            scan_tick: 0,
            dig_tick: 0,
            obs_pos: None,
            obs_tick: 0,
            blacklist: cfg.dig_blacklist,
            blacklist_inverted: cfg.dig_blacklist_inverted,
            requires_tools: cfg.dig_requires_tools,
        }
    }

    /// `Entity.getDistanceSq(BlockPos)`: to the middle of the block.
    fn dist_sq_to_block(c: &Ctx<'_>, at: (i32, i32, i32)) -> f64 {
        let m = c.mob.body.pos;
        dist_sq([m.x, m.y, m.z], [at.0 as f64 + 0.5, at.1 as f64 + 0.5, at.2 as f64 + 0.5])
    }

    /// `EntityLiving.getHorizontalFaceSpeed` / `getVerticalFaceSpeed`, which the look helper is
    /// given.
    fn look_at_target(&self, c: &mut Ctx<'_>, t: &PlayerView) {
        let _ = self;
        c.mob.look_helper.set_look_position_with_entity(t.eye_pos(), 10.0, 40.0);
    }

    /// `ESM_EntityAIDigging.canHarvest`: a solid, breakable block the mob's tool can take.
    fn can_harvest(&self, c: &Ctx<'_>, at: (i32, i32, i32)) -> bool {
        let s = c.w.state(at.0, at.1, at.2);
        if !st::mat_solid(s) || st::hardness(s) < 0.0 {
            return false;
        }
        if items::dig::is_tool_not_required(st::mat(s)) || !self.requires_tools {
            return true;
        }
        !c.mob.main_hand.is_empty() && items::dig::can_harvest(s, &c.mob.main_hand)
    }

    /// `getNextBlock`: one ray of the scan, from a point in the mob's own footprint toward the
    /// target. `dist` is how far the ray runs.
    fn get_next_block(&mut self, c: &mut Ctx<'_>, dist: f64) -> Option<(i32, i32, i32)> {
        let want = self.target.and_then(|id| c.player(id))?;
        let width = (c.mob.body.width.ceil() as i32).max(0);
        let height = (c.mob.body.height.ceil() as i32).max(0);
        let pass_max = width * width * height;
        if pass_max <= 0 {
            return None;
        }
        let y = self.scan_tick % height;
        let x = self.scan_tick % (width * height) / height;
        let z = self.scan_tick / (width * height);
        let m = c.mob.body.pos;
        let half = width as f64 / 2.0;
        let origin = DVec3::new(
            x as f64 + m.x.floor() + 0.5 - half,
            y as f64 + m.y.floor() + 0.5,
            z as f64 + m.z.floor() + 0.5 - half,
        );
        let t = want.pos;
        let mut offset = DVec3::new(t[0].floor() + 0.5, t[1].floor() + 0.5, t[2].floor() + 0.5);
        offset += DVec3::new(x as f64 - half, y as f64, z as f64 - half);
        let mut norm = (offset - origin).normalize();
        if norm.x.abs() == norm.z.abs() && norm.x != 0.0 {
            norm = DVec3::new(norm.x, norm.y, 0.0).normalize();
        }
        let mut end = origin + norm * dist;
        // A target straight up or straight down of the mob is dug at directly.
        let o = [m.x.floor(), m.y.floor(), m.z.floor()];
        let p2 = [t[0].floor(), t[1].floor(), t[2].floor()];
        let (dx, dz) = (p2[0] - o[0], p2[2] - o[2]);
        if ((dx * dx + dz * dz) as f64) < 4.0 {
            if (p2[1] as f64) - (o[1] as f64) > 2.0 {
                end = origin + DVec3::new(0.0, dist, 0.0);
            } else if (p2[1] as f64) - (o[1] as f64) < -2.0 {
                end = origin + DVec3::new(0.0, -dist, 0.0);
            }
        }
        let ray = ray_trace_blocks(c.w, origin, end, false, true);
        self.scan_tick = (self.scan_tick + 1) % pass_max;
        let hit = ray.filter(|h| h.kind == RayKind::Block)?;
        let (hx, hy, hz) = hit.pos;
        if !self.can_harvest(c, (hx, hy, hz)) {
            return None;
        }
        let s = c.w.state(hx, hy, hz);
        let listed = block_name(s).is_some_and(|n| self.blacklist.iter().any(|e| strip(e) == n));
        if listed != self.blacklist_inverted {
            return None;
        }
        Some((hx, hy, hz))
    }
}

/// `ESM_EntityAIDigging.getBlockStrength` (`AiUtils.getBlockStrength`): the fraction of the block
/// the mob breaks per tick, 0 for a block with no hardness.
pub(crate) fn block_strength(c: &Ctx<'_>, at: (i32, i32, i32)) -> f32 {
    let s = c.w.state(at.0, at.1, at.2);
    let hardness = st::hardness(s);
    if hardness <= 0.0 {
        return 0.0;
    }
    let held = &c.mob.main_hand;
    let speed = items::dig::dig_speed(s, held, &dig_context(c));
    speed / hardness / if items::dig::can_harvest(s, held) { 30.0 } else { 100.0 }
}

/// `EntityLivingBase.getDigSpeed`'s conditions for a mob. A mob carries no enchantments and no
/// effects, so efficiency, haste, fatigue and aqua affinity stay empty.
pub(crate) fn dig_context(c: &Ctx<'_>) -> items::DigContext {
    let m = c.mob.body.pos;
    let eye = c.mob.eye_height() as f64;
    let eyes_in_water = st::mat_liquid(c.w.state(floor(m.x), floor(m.y + eye), floor(m.z)));
    items::DigContext { on_ground: c.mob.body.on_ground, eyes_in_water, ..items::DigContext::default() }
}

impl Goal for Digging {
    fn mutex_bits(&self) -> u32 {
        0
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        self.target = c.mob.attack_target;
        let Some(t) = self.target.and_then(|id| c.player(id)) else { return false };
        if t.dead || !c.mob.navigator.no_path() {
            return false;
        }
        // `dist` is a squared distance compared against 1.0, as in the mod.
        let dist = dist_sq(
            { let m = c.mob.body.pos; [m.x, m.y, m.z] },
            t.pos,
        );
        let nav = PathNavigateGround::get_path_search_range(&c.mob.nav_entity(c.w, false)) as f64;
        if dist < 1.0 || dist > nav * nav {
            return false;
        }
        let here = block_pos(c);
        match self.obs_pos {
            None => self.obs_pos = Some(here),
            Some(p) if p != here => {
                self.obs_tick = 0;
                self.obs_pos = None;
                return false;
            }
            Some(_) => {}
        }
        self.obs_tick += 1;
        if self.obs_tick < 20 {
            return false;
        }
        let keep = self.cur_block.filter(|&p| Self::dist_sq_to_block(c, p) <= 16.0 && self.can_harvest(c, p));
        self.cur_block = match keep {
            Some(p) => Some(p),
            None => self.get_next_block(c, 2.0),
        };
        self.cur_block.is_some()
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        let Some(t) = self.target.and_then(|id| c.player(id)) else { return false };
        let _ = t;
        match self.cur_block {
            Some(p) => Self::dist_sq_to_block(c, p) <= 16.0 && self.can_harvest(c, p),
            None => false,
        }
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        c.mob.navigator.clear_path();
        self.obs_tick = 0;
        self.obs_pos = None;
    }

    fn reset_task(&mut self, _c: &mut Ctx<'_>) {
        self.cur_block = None;
        self.dig_tick = 0;
        self.obs_tick = 0;
        self.obs_pos = None;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        let Some(t) = self.target.and_then(|id| c.player(id)) else { return };
        let Some(at) = self.cur_block else { return };
        self.look_at_target(c, &t);
        c.mob.navigator.clear_path();
        self.dig_tick += 1;
        let str = block_strength(c, at) * (self.dig_tick as f32 + 1.0);
        let state = c.w.state(at.0, at.1, at.2);
        if st::is_mat_air(state) {
            self.reset_task(c);
            return;
        }
        if str >= 1.0 {
            // `state.getMaterial().isToolNotRequired() || !held.isEmpty() && canHarvestBlock`.
            let harvest = items::dig::can_harvest(state, &c.mob.main_hand);
            c.fx.push(Effect::Break { at, state, drop: false });
            if harvest {
                c.fx.push(Effect::Harvest { at, state, stack: c.mob.main_hand });
            }
            let speed = c.mob.move_helper.get_speed();
            let path = c.player(self.target.unwrap_or(0)).and_then(|t| c.mob.get_path_to_entity_living(c.w, t.pos));
            c.mob.set_path(c.w, path, speed);
            self.reset_task(c);
        } else if self.dig_tick % 5 == 0 {
            c.fx.push(Effect::BreakAnim { breaker: c.mob.id, at, stage: (str * 10.0) as i32 });
        }
    }
}

/// The feet block of this mob, `Entity.getPosition`.
fn block_pos(c: &Ctx<'_>) -> (i32, i32, i32) {
    let m = c.mob.body.pos;
    (floor(m.x), floor(m.y), floor(m.z))
}
