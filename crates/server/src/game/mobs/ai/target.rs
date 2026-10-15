//! The target goals (the second `EntityAITasks`, `targetTasks`): `EntityAITarget` (the shared
//! state and checks, [`TargetBase`]), `EntityAINearestAttackableTarget` and `EntityAIHurtByTarget`.
//!
//! A target is an entity id kept in `EntityLiving.attackTarget` ([`Mob::attack_target`]). Only
//! players are targets here. DROPPED: the other target classes (`EntityVillager`, `EntityIronGolem`
//! and the mob-on-mob targets), the skull-head `0.5` factor of the player lookup (the head slot
//! is not kept), invisibility (no potions), teams (`isOnSameTeam`, `getTeam`), tamed owners
//! (`IEntityOwnable`), and `isWithinHomeDistanceFromPosition` (no mob has a home).

use sim::collision::floor;

use super::{dist_sq, Ctx, Goal, PlayerView};
use crate::game::mobs::Mob;

/// `EntityAITarget`: the fields and checks the concrete target goals share.
#[derive(Clone, Debug)]
pub struct TargetBase {
    should_check_sight: bool,
    nearby_only: bool,
    target_search_status: i32,
    target_search_delay: i32,
    target_unseen_ticks: i32,
    /// `target`, the entity id the goal chose.
    pub target: Option<u32>,
    /// `unseenMemoryTicks`
    pub unseen_memory_ticks: i32,
}

impl TargetBase {
    pub fn new(check_sight: bool, only_nearby: bool) -> TargetBase {
        TargetBase {
            should_check_sight: check_sight,
            nearby_only: only_nearby,
            target_search_status: 0,
            target_search_delay: 0,
            target_unseen_ticks: 0,
            target: None,
            unseen_memory_ticks: 60,
        }
    }

    /// `EntityAITarget.getTargetDistance`: the follow range attribute.
    pub fn get_target_distance(mob: &Mob) -> f64 {
        mob.def.follow_range as f64
    }

    /// `EntityAITarget.shouldContinueExecuting`
    pub fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        let Some(id) = c.mob.attack_target.or(self.target) else { return false };
        let Some(t) = c.player(id) else { return false };
        if t.dead {
            return false;
        }
        let d0 = Self::get_target_distance(c.mob);
        let m = c.mob.body.pos;
        if dist_sq([m.x, m.y, m.z], t.pos) > d0 * d0 {
            return false;
        }
        if self.should_check_sight {
            if c.can_see(&t) {
                self.target_unseen_ticks = 0;
            } else {
                self.target_unseen_ticks += 1;
                if self.target_unseen_ticks > self.unseen_memory_ticks {
                    return false;
                }
            }
        }
        if t.disable_damage {
            return false;
        }
        c.mob.attack_target = Some(id);
        true
    }

    /// `EntityAITarget.startExecuting`
    pub fn start_executing(&mut self) {
        self.target_search_status = 0;
        self.target_search_delay = 0;
        self.target_unseen_ticks = 0;
    }

    /// `EntityAITarget.resetTask`
    pub fn reset_task(&mut self, c: &mut Ctx<'_>) {
        c.mob.attack_target = None;
        self.target = None;
    }

    /// The static `EntityAITarget.isSuitableTarget(attacker, target, includeInvincibles,
    /// checkSight)`.
    pub fn is_suitable_target_for(c: &Ctx<'_>, target: &PlayerView, include_invincibles: bool, check_sight: bool) -> bool {
        if target.dead {
            return false;
        }
        if !include_invincibles && target.disable_damage {
            return false;
        }
        !check_sight || c.can_see(target)
    }

    /// `EntityAITarget.isSuitableTarget(target, includeInvincibles)`
    pub fn is_suitable_target(&mut self, c: &mut Ctx<'_>, target: &PlayerView, include_invincibles: bool) -> bool {
        if !Self::is_suitable_target_for(c, target, include_invincibles, self.should_check_sight) {
            return false;
        }
        if self.nearby_only {
            self.target_search_delay -= 1;
            if self.target_search_delay <= 0 {
                self.target_search_status = 0;
            }
            if self.target_search_status == 0 {
                self.target_search_status = if self.can_easily_reach(c, target) { 1 } else { 2 };
            }
            if self.target_search_status == 2 {
                return false;
            }
        }
        true
    }

    /// `EntityAITarget.canEasilyReach`
    fn can_easily_reach(&mut self, c: &mut Ctx<'_>, target: &PlayerView) -> bool {
        self.target_search_delay = 10 + c.rand.next_int_bounded(5);
        let Some(path) = c.mob.get_path_to_entity_living(c.w, target.pos) else { return false };
        let Some(p) = path.get_final_path_point() else { return false };
        let i = p.x - floor(target.pos[0]);
        let j = p.z - floor(target.pos[2]);
        (i * i + j * j) as f64 <= 2.25
    }
}

/// `EntityAINearestAttackableTarget<EntityPlayer>`
#[derive(Clone, Debug)]
pub struct NearestAttackableTarget {
    base: TargetBase,
    target_chance: i32,
    target_entity: Option<u32>,
}

impl NearestAttackableTarget {
    /// `(creature, EntityPlayer.class, checkSight)`: chance 10, not nearby only.
    pub fn new(check_sight: bool) -> NearestAttackableTarget {
        NearestAttackableTarget::with(10, check_sight, false)
    }

    /// `(creature, EntityPlayer.class, chance, checkSight, onlyNearby, null)`
    pub fn with(chance: i32, check_sight: bool, only_nearby: bool) -> NearestAttackableTarget {
        NearestAttackableTarget { base: TargetBase::new(check_sight, only_nearby), target_chance: chance, target_entity: None }
    }

    /// `World.getNearestAttackablePlayer(x, y + eye, z, d, d, function, predicate)` with the goal's
    /// `targetEntitySelector` as the predicate. `max_xz` is the horizontal range and `max_y` the
    /// vertical one, which vanilla squares.
    fn nearest_attackable_player(&mut self, c: &mut Ctx<'_>, max_xz: f64, max_y: f64) -> Option<u32> {
        let m = c.mob.body.pos;
        let pos_y = m.y + c.mob.eye_height() as f64;
        let mut d0 = -1.0f64;
        let mut best = None;
        let players: Vec<PlayerView> = c.players.to_vec();
        for p in players {
            // !disableDamage && isEntityAlive && !isSpectator, then the predicate
            // (NOT_SPECTATING and isSuitableTarget(player, false)).
            if p.disable_damage || p.dead || p.spectator || !self.base.is_suitable_target(c, &p, false) {
                continue;
            }
            let (dx, dz) = (p.pos[0] - m.x, p.pos[2] - m.z);
            let d1 = dx * dx + dz * dz;
            let mut d2 = max_xz;
            if p.sneaking {
                d2 *= 0.800000011920929;
            }
            if (max_y < 0.0 || (p.pos[1] - pos_y).abs() < max_y * max_y) && (max_xz < 0.0 || d1 < d2 * d2) && (d0 == -1.0 || d1 < d0) {
                d0 = d1;
                best = Some(p.id);
            }
        }
        best
    }
}

impl Goal for NearestAttackableTarget {
    fn mutex_bits(&self) -> u32 {
        1
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if self.target_chance > 0 && c.rand.next_int_bounded(self.target_chance) != 0 {
            return false;
        }
        let d = TargetBase::get_target_distance(c.mob);
        self.target_entity = self.nearest_attackable_player(c, d, d);
        self.target_entity.is_some()
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        self.base.should_continue_executing(c)
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        c.mob.attack_target = self.target_entity;
        self.base.start_executing();
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        self.base.reset_task(c);
    }
}

/// `EntityAIHurtByTarget`. DROPPED: `excludedReinforcementTypes` (only `EntityPigZombie` is ever
/// passed and it is not ported).
#[derive(Clone, Debug)]
pub struct HurtByTarget {
    base: TargetBase,
    entity_calls_for_help: bool,
    revenge_timer_old: i32,
}

impl HurtByTarget {
    pub fn new(entity_calls_for_help: bool) -> HurtByTarget {
        HurtByTarget { base: TargetBase::new(true, false), entity_calls_for_help, revenge_timer_old: 0 }
    }

    /// `alertOthers`: the same class within the follow range (10 blocks up and down) with no
    /// target takes this mob's revenge target.
    fn alert_others(&self, c: &mut Ctx<'_>) {
        let d0 = TargetBase::get_target_distance(c.mob);
        let m = c.mob.body.pos;
        let area = sim::collision::Aabb::new(m.x, m.y, m.z, m.x + 1.0, m.y + 1.0, m.z + 1.0).grow(d0, 10.0, d0);
        let name = c.mob.def.kind.name;
        let revenge = c.mob.revenge_target;
        let owner = c.mob.id;
        for o in c.others.before.iter_mut().chain(c.others.after.iter_mut()) {
            if o.id != owner && !o.dead && o.def.kind.name == name && o.attack_target.is_none() && o.body.bb.intersects(&area) {
                // setEntityAttackTarget
                o.attack_target = revenge;
            }
        }
    }
}

impl Goal for HurtByTarget {
    fn mutex_bits(&self) -> u32 {
        1
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        let i = c.mob.revenge_timer;
        let Some(t) = c.mob.revenge_target else { return false };
        if i == self.revenge_timer_old {
            return false;
        }
        match c.player(t) {
            Some(p) => self.base.is_suitable_target(c, &p, false),
            None => false,
        }
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        self.base.should_continue_executing(c)
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        c.mob.attack_target = c.mob.revenge_target;
        self.base.target = c.mob.attack_target;
        self.revenge_timer_old = c.mob.revenge_timer;
        self.base.unseen_memory_ticks = 300;
        if self.entity_calls_for_help {
            self.alert_others(c);
        }
        self.base.start_executing();
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        self.base.reset_task(c);
    }
}
