//! `AbstractSkeleton` and `EntitySkeleton` (T9k), ported 1:1 from 1.12.2: `initEntityAI`,
//! `setCombatTask` (the bow goal with the bow in hand, melee without), `setItemStackToSlot`,
//! `onInitialSpawn`'s bow, `attackEntityWithRangedAttack`, and the daylight burn (shared with
//! the zombie in `zombie::pre_update`). The arrow is `arrow.rs`; the goals are `ai/ranged.rs`,
//! `ai/sun.rs` and `ai/melee.rs`; the loot is `loot_tables/entities/skeleton.json`.
//!
//! The goal lists keep vanilla's shape so a port that swaps tasks by class (Epic Siege) edits the
//! same lists.
//!
//! DROPPED: the wolf `EntityAIAvoidEntity` (tasks 3) and the iron golem target (targetTasks 3),
//! `onInitialSpawn`'s armor roll, enchantments, pick-up-loot and halloween pumpkin, the stray,
//! wither skeleton and skeleton horse variants, `getSkeletonType`, the helmet that stops the
//! daylight burn (the head slot is empty), Power, Punch and Flame (`setEnchantmentEffectsFromEntity`
//! keeps the difficulty damage only), `ItemBow` use sounds and the swing animation packet.

use items::ids::item;
use items::ItemStack;

use super::ai::ranged::AttackRangedBow;
use super::ai::sun::{FleeSun, RestrictSun};
use super::ai::target::{HurtByTarget, NearestAttackableTarget};
use super::ai::{goals, melee, Ctx, Effect, PlayerView};
use super::arrow::Arrow;
use super::Mob;

impl Mob {
    /// `AbstractSkeleton.initEntityAI`; `setCombatTask` runs from the constructor.
    pub(super) fn init_skeleton_ai(&mut self) {
        let swimming = goals::Swimming::new(self);
        self.tasks.add_task(1, swimming);
        self.tasks.add_task(2, RestrictSun::new());
        self.tasks.add_task(3, FleeSun::new(1.0));
        // 3: EntityAIAvoidEntity for wolves is DROPPED.
        self.tasks.add_task(5, goals::Wander::avoid_water(1.0));
        self.tasks.add_task(6, goals::WatchClosest::new(8.0));
        self.tasks.add_task(6, goals::LookIdle::new());
        self.target_tasks.add_task(1, HurtByTarget::new(false));
        self.target_tasks.add_task(2, NearestAttackableTarget::new(true));
        // targetTasks 3: the iron golem target is DROPPED.
        self.set_combat_task();
    }

    /// `AbstractSkeleton.setCombatTask`
    pub fn set_combat_task(&mut self) {
        let mut tasks = std::mem::take(&mut self.tasks);
        tasks.remove_where(self, |g| g.is_combat_task());
        if self.main_hand.get_item() == item::BOW {
            // EnumDifficulty.HARD has id 3.
            let cooldown = if self.difficulty_id == 3 { 20 } else { 40 };
            let mut bow = AttackRangedBow::new(1.0, 20, 15.0);
            bow.set_attack_cooldown(cooldown);
            tasks.add_task(4, bow);
        } else {
            tasks.add_task(4, melee::SkeletonMelee::new(1.2, false));
        }
        self.tasks = tasks;
    }

    /// `EntityLivingBase.setItemStackToSlot(MAINHAND)` as `AbstractSkeleton` overrides it: the
    /// combat task follows the hand. The trackers hear of it from `Mob::update`.
    pub fn set_item_in_main_hand(&mut self, stack: ItemStack) {
        self.main_hand = stack;
        if self.def.kind.name == "skeleton" {
            self.set_combat_task();
        }
    }

    /// `AbstractSkeleton.onInitialSpawn` up to the bow: `setEquipmentBasedOnDifficulty`.
    pub(super) fn skeleton_initial_spawn(&mut self) {
        self.set_item_in_main_hand(ItemStack::of(item::BOW));
        // The spawn frame already carries the hand; the tick's diff has nothing to add.
        self.sent_main_hand = self.main_hand;
    }

    /// `EntityLivingBase.setActiveHand(MAIN_HAND)`: a held item starts being used.
    pub fn set_active_hand(&mut self) {
        if !self.main_hand.is_empty() && !self.hand_active {
            self.hand_active = true;
            // ItemStack.getMaxItemUseDuration: 72000 for a bow.
            self.item_in_use_count = 72000;
        }
    }

    /// `EntityLivingBase.resetActiveHand`
    pub fn reset_active_hand(&mut self) {
        self.hand_active = false;
        self.item_in_use_count = 0;
    }

    /// `getItemInUseMaxCount`: ticks the hand has been drawn.
    pub fn get_item_in_use_max_count(&self) -> i32 {
        if self.hand_active {
            72000 - self.item_in_use_count
        } else {
            0
        }
    }

    /// `EntityLivingBase.updateActiveHand`
    pub(super) fn update_active_hand(&mut self) {
        if self.hand_active {
            if self.main_hand.is_empty() {
                self.reset_active_hand();
            } else {
                // `--activeItemStackUseCount == 0` would finish the item; a bow's draw never gets there.
                self.item_in_use_count -= 1;
            }
        }
    }
}

/// `AbstractSkeleton.attackEntityWithRangedAttack` with `getArrow` and
/// `EntityArrow.setEnchantmentEffectsFromEntity`: the arrow joins the world on landing.
pub fn attack_entity_with_ranged_attack(c: &mut Ctx<'_>, target: &PlayerView, distance_factor: f32) {
    let b = c.mob.body.pos;
    let id = c.mob.id;
    let difficulty = c.mob.difficulty_id;
    let mut arrow = Arrow::new(Some(id), [b.x, b.y + c.mob.eye_height() as f64 - 0.100_000_001_490_116_12, b.z]);
    // setEnchantmentEffectsFromEntity: no Power, Punch or Flame (DROPPED).
    arrow.damage = (distance_factor * 2.0) as f64 + c.rand.next_gaussian() * 0.25 + (difficulty as f32 * 0.11) as f64;
    let d0 = target.pos[0] - b.x;
    let height = if target.sneaking { 1.65f32 } else { 1.8f32 };
    let d1 = target.pos[1] + (height / 3.0) as f64 - arrow.body.pos.y;
    let d2 = target.pos[2] - b.z;
    let d3 = (d0 * d0 + d2 * d2).sqrt() as f32 as f64;
    arrow.shoot(d0, d1 + d3 * 0.200_000_002_980_232_24, d2, 1.6, (14 - difficulty * 4) as f32, c.rand);
    // playSound(ENTITY_SKELETON_SHOOT, 1.0F, 1.0F / (getRNG().nextFloat() * 0.4F + 0.8F))
    let pitch = 1.0 / (c.rand.next_float() * 0.4 + 0.8);
    c.mob.sound(c.fx, "entity.skeleton.shoot", 1.0, pitch);
    c.fx.push(Effect::Arrow(arrow));
}
