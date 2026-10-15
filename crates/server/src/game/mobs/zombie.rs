//! `EntityZombie` and the parts of `EntityMob` a hostile kind shares (T9j), ported 1:1 from
//! 1.12.2: `initEntityAI`, `applyEntityAI`, the daylight burn of `onLivingUpdate`, and the
//! `EntityMob.onLivingUpdate` idle bonus in bright light. The attributes (health 20, follow range
//! 35, speed 0.23, attack 3, armor 2) are the `MOBS` entry; `attackEntityAsMob` is
//! `combat::attack_entity_as_mob`; the loot is `loot_tables/entities/zombie.json`; the light-level
//! spawning is `spawner::can_spawn_here`.
//!
//! The goal lists keep vanilla's shape (`tasks` and `targetTasks` by priority) so a port that swaps
//! tasks by class (Epic Siege) edits the same lists.
//!
//! DROPPED: baby zombies (`IS_CHILD`, the speed boost and size, the chicken jockey), villager
//! zombies and `EntityZombieVillager` conversion, `onKillEntity`, reinforcements
//! (`SPAWN_REINFORCEMENTS_CHANCE` and the call in `attackEntityFrom`), husks, `onInitialSpawn`
//! (equipment, enchantments, pick-up-loot, the follow range and knockback resistance bonuses, the
//! leader bonus, the break-doors roll), `EntityAIBreakDoor`, `EntityAIMoveThroughVillage` (no
//! villages) and `EntityAIMoveTowardsRestriction` (no mob has a home), the target goals for
//! villagers and iron golems, the fire a burning zombie spreads in `attackEntityAsMob` (it needs
//! `DifficultyInstance.getAdditionalDifficulty`), the helmet that stops the burn (the head slot is
//! empty), skull drops, the zombie `getYOffset` and sounds.

use items::ItemStack;
use sim::collision::floor;
use worldgen::java_random::JavaRandom;

use super::ai::target::{HurtByTarget, NearestAttackableTarget};
use super::ai::{goals, melee};
use super::{spawn_lists, spawner, Mob};
use crate::game::{terrain, Game};

impl Mob {
    /// `EntityZombie.initEntityAI`
    pub(super) fn init_zombie_ai(&mut self) {
        let swimming = goals::Swimming::new(self);
        self.tasks.add_task(0, swimming);
        self.tasks.add_task(2, melee::ZombieAttack::new(1.0, false));
        // 5: EntityAIMoveTowardsRestriction is DROPPED (no home position).
        self.tasks.add_task(7, goals::Wander::avoid_water(1.0));
        self.tasks.add_task(8, goals::WatchClosest::new(8.0));
        self.tasks.add_task(8, goals::LookIdle::new());
        self.apply_zombie_entity_ai();
    }

    /// `EntityZombie.applyEntityAI`
    fn apply_zombie_entity_ai(&mut self) {
        // tasks 6: EntityAIMoveThroughVillage is DROPPED.
        self.target_tasks.add_task(1, HurtByTarget::new(true));
        self.target_tasks.add_task(2, NearestAttackableTarget::new(true));
        // targetTasks 3: the villager and iron golem targets are DROPPED.
    }
}

/// `World.isDaytime` on the server: `skylightSubtracted < 4`.
pub fn is_daytime(sky_subtracted: i32) -> bool {
    sky_subtracted < 4
}

/// `World.canSeeSky(pos)`: the block is at or above the column's height value. False in a column
/// that is not held.
fn can_see_sky(game: &Game, x: i32, y: i32, z: i32) -> bool {
    terrain::column(game, x, z).is_some_and(|(h, _)| y >= h)
}

/// `World.isRainingAt` for the feet and the head of a mob in column (`x`, `z`): raining, with
/// the block at or above the column's height value.
fn raining_on(game: &Game, x: i32, feet_y: i32, head_y: i32, z: i32) -> bool {
    let raining = terrain::world(game).is_some_and(|w| {
        let w = w.lock().unwrap();
        w.weather.raining || w.weather.thundering
    });
    raining && (can_see_sky(game, x, feet_y, z) || can_see_sky(game, x, head_y, z))
}

/// The start of `EntityZombie.onLivingUpdate` and `EntityMob.onLivingUpdate`, which read the light
/// (so they need the game, not just the mob) and run before the living update: a zombie in
/// daylight under open sky catches fire for 8 seconds, and a hostile mob in bright light ages its
/// despawn clock twice as fast. `sky` is `World.skylightSubtracted`.
pub fn pre_update(game: &mut Game, m: &mut Mob, rand: &mut JavaRandom, sky: i32) {
    if !spawn_lists::is_hostile(m.def) {
        return;
    }
    let (x, z) = (floor(m.body.pos.x), floor(m.body.pos.z));
    let y = floor(m.body.pos.y + m.eye_height() as f64);
    // EntitySkeleton.onLivingUpdate: the sky position is the rounded feet, the light the eyes'.
    let sky_y = floor(m.body.pos.y + 0.5);
    // Entity.getBrightness: 0 when the column is not loaded.
    let f = spawner::light_from_neighbors(game, x, y, z, sky).map_or(0.0, spawner::brightness);
    let zombie = m.def.kind.name == "zombie" && !m.is_child();
    let skeleton = m.def.kind.name == "skeleton";
    if (zombie || skeleton) && is_daytime(sky) && f > 0.5 {
        if rand.next_float() * 30.0 < (f - 0.4) * 2.0 && can_see_sky(game, x, if skeleton { sky_y } else { y }, z) {
            // A helmet takes the sun instead: `setItemDamage(damage + nextInt(2))`, gone at
            // `getMaxDamage`. DROPPED: `renderBrokenItemStack` (the break sound and particles).
            let mut burn = true;
            if !m.head.is_empty() {
                if m.head.is_item_stack_damageable() {
                    let damage = m.head.get_metadata() + rand.next_int_bounded(2);
                    m.head.set_item_damage(damage);
                    if damage >= m.head.get_max_damage() {
                        m.head = ItemStack::EMPTY;
                    }
                }
                burn = false;
            }
            if burn {
                m.set_fire(8);
            }
        }
    }
    // Entity.move: `isWet` (in water, or rain falling on the feet or the head) puts a burning
    // mob out. DROPPED: the biome checks of `isRainingAt` (snow biomes, `canRain`).
    if m.fire > 0 && raining_on(game, x, floor(m.body.pos.y), floor(m.body.pos.y + m.body.height as f64), z) {
        m.fire = 0;
    }
    // EntityMob.onLivingUpdate
    if f > 0.5 {
        m.idle_time += 2;
    }
}
