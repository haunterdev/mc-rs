//! ESM-3, the targeting. `ai/ESM_EntityAITarget.java` and
//! `ai/ESM_EntityAINearestAttackableTarget.java`, as one goal: the mod keeps a list of target
//! classes per goal (player, villager, pet, every living thing), and players are the only target
//! class mc-rs has, so the list is the player list.
//!
//! The base class is `EntityAITarget` with the sight check going through the xray senses, a 60 tick
//! unseen allowance, and `getTargetDistance` read off the `FOLLOW_RANGE` attribute, which the join
//! raised to at least the awareness radius. The subclass searches every 10 ticks (`ticksExisted %
//! 10`), rolls the vanilla `targetChance`, and takes the nearest suitable target inside the owner's
//! box grown by `(follow range, 16, follow range)`. `isSuitableTarget` adds the attacker cap and
//! the visibility scale (a sneaking player is `0.8`).
//!
//! DROPPED: teams and `canAttackClass` (there is one target class), the ownable and pet rules, the
//! skull and carved-pumpkin visibility terms and the invisible-player term (no head slot and no
//! invisibility), the `EntitySelectors.IS_ALIVE` filter (the base class checks it), the mob targets
//! `Attack Everything` would add, and the villager targets `Attack Villagers` would.

use sim::collision::floor;

use crate::game::mobs::ai::{dist_sq, Ctx, Goal, PlayerView};
use super::config::Config;
use super::senses;

/// `ESM_EntityAITarget.field_188510_h`: unseen ticks allowed before the target is dropped.
const UNSEEN_LIMIT: i32 = 60;
/// `ESM_EntityAINearestAttackableTarget`'s default `targetChance`.
const TARGET_CHANCE: i32 = 10;

/// `ESM_EntityAINearestAttackableTarget` over `ESM_EntityAITarget`.
#[derive(Clone, Debug)]
pub struct EsNearestAttackableTarget {
    /// `shouldCheckSight`
    check_sight: bool,
    /// `nearbyOnly`
    nearby_only: bool,
    target_chance: i32,
    target_search_status: i32,
    target_search_delay: i32,
    target_unseen_ticks: i32,
    /// `field_188509_g`: the target `should_execute` found, until it is set on the mob.
    found: Option<u32>,
    xray: f64,
    target_cap: usize,
}

impl EsNearestAttackableTarget {
    /// The replacement `ModifierNearestAttackable` builds: the vanilla `targetChance` and sight
    /// flags are carried over, which for every ported kind are `(10, true, false)`.
    pub fn new(cfg: &Config) -> EsNearestAttackableTarget {
        EsNearestAttackableTarget {
            check_sight: true,
            nearby_only: false,
            target_chance: TARGET_CHANCE,
            target_search_status: 0,
            target_search_delay: 0,
            target_unseen_ticks: 0,
            found: None,
            xray: cfg.xray,
            target_cap: cfg.target_cap,
        }
    }

    /// `ESM_EntityAITarget.getTargetDistance`: the FOLLOW_RANGE attribute, not the `MOBS` entry.
    fn target_distance(c: &Ctx<'_>) -> f64 {
        c.mob.follow_range() as f64
    }

    /// `AttackerHandler.canAttack`: the mobs already on this player, against the pathing cap.
    /// The mod keeps a per-player set pruned every 20 ticks (`updateAttackers`); a mob is in that
    /// set while it targets the player, so counting the mobs that do is the same set.
    fn can_attack(c: &Ctx<'_>, player: u32, cap: usize) -> bool {
        let mut n = usize::from(c.mob.attack_target == Some(player));
        for m in c.others.iter() {
            if !m.dead && m.attack_target == Some(player) {
                n += 1;
            }
        }
        n < cap
    }

    /// `ESM_EntityAINearestAttackableTarget.getVisibility`: a sneaking target is seen at 0.8.
    fn visibility(p: &PlayerView) -> f64 {
        if p.sneaking {
            0.8
        } else {
            1.0
        }
    }

    /// `canEasilyReach`: the path the mob would take ends within 1.5 blocks of the target.
    fn can_easily_reach(&mut self, c: &mut Ctx<'_>, p: &PlayerView) -> bool {
        let m = c.mob.body.pos;
        let dist = dist_sq([m.x, m.y, m.z], p.pos).sqrt() as f32;
        self.target_search_delay = (10 + c.rand.next_int_bounded(5)).max(dist as i32 - 16);
        let Some(path) = c.mob.get_path_to_entity_living(c.w, p.pos) else { return false };
        let Some(pt) = path.get_final_path_point() else { return false };
        let (i, j) = (pt.x - floor(p.pos[0]), pt.z - floor(p.pos[2]));
        f64::from(i * i + j * j) <= 2.25
    }

    /// `ESM_EntityAITarget.isSuitableTarget` then the subclass's additions.
    fn is_suitable(&mut self, c: &mut Ctx<'_>, p: &PlayerView, include_invincibles: bool) -> bool {
        if p.dead {
            return false;
        }
        if !include_invincibles && p.disable_damage {
            return false;
        }
        if self.check_sight && !senses::can_see(c, p, self.xray) {
            return false;
        }
        if !Self::can_attack(c, p.id, self.target_cap) {
            return false;
        }
        let m = c.mob.body.pos;
        let dist = dist_sq([m.x, m.y, m.z], p.pos).sqrt();
        if dist > Self::target_distance(c) * Self::visibility(p) {
            return false;
        }
        // The accumulated target checks and `Attack Pets` are DROPPED: the only target class here
        // is the player, which every ported kind already lists.
        if self.nearby_only {
            self.target_search_delay -= 1;
            if self.target_search_delay <= 0 {
                self.target_search_status = 0;
            }
            if self.target_search_status == 0 {
                self.target_search_status = if self.can_easily_reach(c, p) { 1 } else { 2 };
            }
            return self.target_search_status != 2;
        }
        true
    }
}

impl Goal for EsNearestAttackableTarget {
    fn mutex_bits(&self) -> u32 {
        1
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if c.mob.ticks_existed % 10 != 0 {
            return false;
        }
        if self.target_chance > 0 && c.rand.next_int_bounded(self.target_chance) != 0 {
            return false;
        }
        let reach = Self::target_distance(c);
        let m = c.mob.body.pos;
        let mob = [m.x, m.y, m.z];
        // World.getEntitiesWithinAABB of the owner's box grown by (follow range, 16, follow range).
        let players: &[PlayerView] = c.players;
        let mut best: Option<(f64, u32)> = None;
        for p in players {
            if (p.pos[0] - mob[0]).abs() > reach || (p.pos[2] - mob[2]).abs() > reach {
                continue;
            }
            if (p.pos[1] - mob[1]).abs() > 16.0 {
                continue;
            }
            if !self.is_suitable(c, p, false) {
                continue;
            }
            let d = dist_sq(p.pos, mob);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, p.id));
            }
        }
        match best {
            Some((_, id)) => {
                self.found = Some(id);
                true
            }
            None => false,
        }
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        let Some(id) = c.mob.attack_target.or(self.found) else { return false };
        // A target that is not a player is DROPPED: no mob targets another mob yet.
        let Some(p) = c.player(id) else { return false };
        if p.dead {
            return false;
        }
        let m = c.mob.body.pos;
        let d = Self::target_distance(c);
        if dist_sq([m.x, m.y, m.z], p.pos) > d * d {
            return false;
        }
        if self.check_sight {
            if senses::can_see(c, &p, self.xray) {
                self.target_unseen_ticks = 0;
            } else {
                self.target_unseen_ticks += 1;
                if self.target_unseen_ticks > UNSEEN_LIMIT {
                    return false;
                }
            }
        }
        if p.disable_damage {
            return false;
        }
        c.mob.attack_target = Some(id);
        true
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        if let Some(id) = self.found {
            c.mob.attack_target = Some(id);
        }
        self.target_search_status = 0;
        self.target_search_delay = 0;
        self.target_unseen_ticks = 0;
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        c.mob.attack_target = None;
        self.found = None;
    }
}
