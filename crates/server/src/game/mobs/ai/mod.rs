//! Mob AI (T9g), ported 1:1 from 1.12.2 `net.minecraft.entity.ai`.
//!
//! | File | Source |
//! |---|---|
//! | `mod.rs` | `EntityAIBase` (the [`Goal`] trait), `EntityAITasks` ([`Tasks`]), and what a goal reads of the world ([`Ctx`]) |
//! | `goals.rs` | `EntityAISwimming`, `EntityAIPanic`, `EntityAIWander`, `EntityAIWanderAvoidWater`, `EntityAIWatchClosest`, `EntityAILookIdle` |
//! | `animal_goals.rs` | `EntityAIMate`, `EntityAITempt`, `EntityAIFollowParent` |
//! | `random_pos.rs` | `RandomPositionGenerator` |
//! | `target.rs` | `EntityAITarget`, `EntityAINearestAttackableTarget`, `EntityAIHurtByTarget` |
//! | `melee.rs` | `EntityAIAttackMelee`, `EntityAIZombieAttack` |
//! | `ranged.rs` | `EntityAIAttackRangedBow` |
//! | `sun.rs` | `EntityAIRestrictSun`, `EntityAIFleeSun` |
//!
//! A goal gets a [`Ctx`]: its own mob, the other mobs, a snapshot of the players, the blocks, the
//! shared mob generator and the effects only the game can apply (a baby to spawn, a status to send).
//!
//! DROPPED as a whole: every goal no ported mob uses (avoid-entity and the rest, until their mobs
//! are ported), `EntityAITasks.removeTask`'s reset gets no `Ctx` (see [`Tasks::remove_where`]), `EntitySenses`' per-tick
//! sight cache (`canSee` traces each time), teams, home positions (`isWithinHomeDistance` is
//! always true), the target goals' non-player target classes (villagers, golems), the
//! profiler sections, and the per-entity `Entity.rand` (every mob draws from the shared generator).

pub mod animal_goals;
pub mod eat_grass;
pub mod goals;
pub mod melee;
pub mod random_pos;
pub mod ranged;
pub mod sun;
pub mod target;

use worldgen::java_random::JavaRandom;

use super::{Mob, MobDef};
use crate::game::players;
use crate::game::terrain::Blocks;
use crate::game::Game;

/// `EntityAIBase`. `mutexBits` is [`Goal::mutex_bits`].
pub trait Goal: GoalClone + std::fmt::Debug + Send + Sync {
    fn mutex_bits(&self) -> u32;
    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool;
    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        self.should_execute(c)
    }
    fn is_interruptible(&self) -> bool {
        true
    }
    fn start_executing(&mut self, _c: &mut Ctx<'_>) {}
    fn reset_task(&mut self, _c: &mut Ctx<'_>) {}
    fn update_task(&mut self, _c: &mut Ctx<'_>) {}
    /// `resetTask` for a goal taken off the list while running (`removeTask`), which has the
    /// mob but no world.
    fn reset_on_remove(&mut self, _mob: &mut Mob) {}
    /// The goals `AbstractSkeleton.setCombatTask` swaps (`aiAttackOnCollide`, `aiArrowAttack`).
    fn is_combat_task(&self) -> bool {
        false
    }
}

/// `Clone` for `Box<dyn Goal>` (a `Mob` is cloned by the tests and the save round trip), and the
/// downcast an addon's task pass needs to find the goal it replaces by class (`as_any().is::<T>()`).
pub trait GoalClone {
    fn clone_box(&self) -> Box<dyn Goal>;
    fn as_any(&self) -> &dyn std::any::Any;
}

impl<T: 'static + Goal + Clone> GoalClone for T {
    fn clone_box(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Clone for Box<dyn Goal> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// `EntityAITasks.EntityAITaskEntry`
#[derive(Clone, Debug)]
pub struct Entry {
    pub action: Box<dyn Goal>,
    pub priority: i32,
    pub using: bool,
}

/// `EntityAITasks`: `taskEntries` in insertion order, `executingTaskEntries` as indices in the
/// order they started (a `LinkedHashSet`).
#[derive(Clone, Debug)]
pub struct Tasks {
    pub entries: Vec<Entry>,
    executing: Vec<usize>,
    tick_count: u32,
    tick_rate: u32,
    disabled_control_flags: u32,
}

impl Default for Tasks {
    fn default() -> Tasks {
        Tasks::new()
    }
}

impl Tasks {
    pub fn new() -> Tasks {
        Tasks { entries: Vec::new(), executing: Vec::new(), tick_count: 0, tick_rate: 3, disabled_control_flags: 0 }
    }

    /// `addTask`
    pub fn add_task(&mut self, priority: i32, task: impl Goal + 'static) {
        self.entries.push(Entry { action: Box::new(task), priority, using: false });
    }

    /// `EntityAITasks.removeTask` for every goal `pred` picks: a running one is reset first, the
    /// rest keep their order. DROPPED: the reset runs without a `Ctx` ([`Goal::reset_on_remove`]).
    pub fn remove_where(&mut self, mob: &mut Mob, pred: impl Fn(&dyn Goal) -> bool) {
        let mut i = 0;
        while i < self.entries.len() {
            if !pred(&*self.entries[i].action) {
                i += 1;
                continue;
            }
            if self.entries[i].using {
                self.entries[i].action.reset_on_remove(mob);
            }
            self.entries.remove(i);
            self.executing.retain(|&e| e != i);
            for e in &mut self.executing {
                if *e > i {
                    *e -= 1;
                }
            }
        }
    }

    /// Whether goal entry `i` is running.
    pub fn is_executing(&self, i: usize) -> bool {
        self.executing.contains(&i)
    }

    /// The indices of the running goals, in the order they started.
    pub fn executing(&self) -> &[usize] {
        &self.executing
    }

    /// `onUpdateTasks`
    pub fn on_update_tasks(&mut self, c: &mut Ctx<'_>) {
        let n = self.tick_count;
        self.tick_count = self.tick_count.wrapping_add(1);
        if n % self.tick_rate == 0 {
            for i in 0..self.entries.len() {
                if self.entries[i].using {
                    if !self.can_use(i) || !self.can_continue(i, c) {
                        self.entries[i].using = false;
                        self.entries[i].action.reset_task(c);
                        self.executing.retain(|&e| e != i);
                    }
                } else if self.can_use(i) && self.entries[i].action.should_execute(c) {
                    self.entries[i].using = true;
                    self.entries[i].action.start_executing(c);
                    self.executing.push(i);
                }
            }
        } else {
            let mut k = 0;
            while k < self.executing.len() {
                let i = self.executing[k];
                if !self.can_continue(i, c) {
                    self.entries[i].using = false;
                    self.entries[i].action.reset_task(c);
                    self.executing.remove(k);
                } else {
                    k += 1;
                }
            }
        }
        for k in 0..self.executing.len() {
            let i = self.executing[k];
            self.entries[i].action.update_task(c);
        }
    }

    fn can_continue(&mut self, i: usize, c: &mut Ctx<'_>) -> bool {
        self.entries[i].action.should_continue_executing(c)
    }

    /// `canUse`: no running goal of an equal or higher priority shares a mutex bit, and no
    /// running goal of a lower priority refuses to be interrupted.
    fn can_use(&self, i: usize) -> bool {
        if self.executing.is_empty() {
            return true;
        }
        let e = &self.entries[i];
        if self.is_control_flag_disabled(e.action.mutex_bits()) {
            return false;
        }
        for &j in &self.executing {
            if j != i {
                let other = &self.entries[j];
                if e.priority >= other.priority {
                    // areTasksCompatible
                    if e.action.mutex_bits() & other.action.mutex_bits() != 0 {
                        return false;
                    }
                } else if !other.action.is_interruptible() {
                    return false;
                }
            }
        }
        true
    }

    pub fn is_control_flag_disabled(&self, flag: u32) -> bool {
        self.disabled_control_flags & flag > 0
    }

    pub fn disable_control_flag(&mut self, flag: u32) {
        self.disabled_control_flags |= flag;
    }

    pub fn enable_control_flag(&mut self, flag: u32) {
        self.disabled_control_flags &= !flag;
    }

    pub fn set_control_flag(&mut self, flag: u32, enabled: bool) {
        if enabled {
            self.enable_control_flag(flag);
        } else {
            self.disable_control_flag(flag);
        }
    }
}

/// What a player looks like to a goal (`World.playerEntities`).
#[derive(Clone, Copy, Debug)]
pub struct PlayerView {
    /// The session id, which is also the entity id.
    pub id: u32,
    pub pos: [f64; 3],
    pub eye_height: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub spectator: bool,
    /// Item ids in `getHeldItemMainhand` and `getHeldItemOffhand`, 0 for an empty hand.
    pub held: [u16; 2],
    /// `isSneaking`
    pub sneaking: bool,
    /// `capabilities.disableDamage`: creative and spectator.
    pub disable_damage: bool,
    /// Dead until the respawn (`!isEntityAlive`).
    pub dead: bool,
}

/// `EntityPlayer.width`
pub const PLAYER_WIDTH: f32 = 0.6;

impl PlayerView {
    pub fn eye_pos(&self) -> [f64; 3] {
        [self.pos[0], self.pos[1] + self.eye_height as f64, self.pos[2]]
    }
}

/// Every player in the world, by session id.
pub fn player_views(game: &Game) -> Vec<PlayerView> {
    let mut ids: Vec<u32> = game.sessions.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter()
        .filter_map(|id| {
            let s = &game.sessions[&id];
            let st = s.state?;
            let mut held = [0u16; 2];
            if let Some(p) = game.players.players.get(&id) {
                let (main, off) = (p.inv.get_current_item(), p.inv.off_hand[0]);
                held = [if main.is_empty() { 0 } else { main.get_item() }, if off.is_empty() { 0 } else { off.get_item() }];
            }
            Some(PlayerView {
                id,
                pos: st.pos,
                eye_height: players::eye_height(st.flags & proto::SNEAKING != 0),
                yaw: st.yaw,
                pitch: st.pitch,
                spectator: s.mode == proto::GM_SPECTATOR,
                held,
                sneaking: st.flags & proto::SNEAKING != 0,
                disable_damage: players::disable_damage(s.mode),
                dead: game.players.players.get(&id).is_some_and(|p| p.dead),
            })
        })
        .collect()
}

/// The mobs other than the one being updated, in list order: the slice before it and the slice
/// after.
pub struct Others<'a> {
    pub before: &'a mut [Mob],
    pub after: &'a mut [Mob],
}

impl<'a> Others<'a> {
    pub fn reborrow(&mut self) -> Others<'_> {
        Others { before: &mut *self.before, after: &mut *self.after }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Mob> {
        self.before.iter().chain(self.after.iter())
    }

    pub fn get(&self, id: u32) -> Option<&Mob> {
        self.iter().find(|m| m.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Mob> {
        self.before.iter_mut().chain(self.after.iter_mut()).find(|m| m.id == id)
    }
}

/// What only the game can do, collected during the mob tick and applied after it.
#[derive(Clone, Debug)]
pub enum Effect {
    /// `World.spawnEntity` of a baby made by `EntityAIMate.spawnBaby`.
    /// `dyes` are the parents' `getDyeDamage` for a sheep (`getDyeColorMixFromParents`).
    Child { def: &'static MobDef, pos: [f64; 3], dyes: Option<(i32, i32)> },
    /// `Entity.entityDropItem` / `dropItem` of one stack at `pos`.
    Drop { pos: [f64; 3], stack: items::ItemStack },
    /// `World.setBlockState` by a mob.
    SetBlock { at: (i32, i32, i32), state: u16 },
    /// The pillar's step: the block goes in at `at` and mob `mob` is placed on top of it at `to`
    /// (`Entity.setPositionAndUpdate`). One effect because the two happen together in the mod and a
    /// goal cannot write the world: applied after the mob tick, so the block is under the mob
    /// before its next `move`.
    StepUp { mob: u32, at: (i32, i32, i32), state: u16, to: [f64; 3] },
    /// `World.destroyBlock(pos, drop)` by a mob: the block goes with its neighbour updates, and
    /// `drop` asks for the block's own drops.
    Break { at: (i32, i32, i32), state: u16, drop: bool },
    /// `Block.harvestBlock` beside a mob's dig, with the stack it held.
    Harvest { at: (i32, i32, i32), state: u16, stack: items::ItemStack },
    /// `World.sendBlockBreakProgress(breaker, pos, stage)`.
    BreakAnim { breaker: u32, at: (i32, i32, i32), stage: i32 },
    /// `World.setEntityState`
    Event { id: u32, status: u8 },
    /// `EntityMob.attackEntityAsMob` on a player (the attribute damage through `causeMobDamage`).
    Attack { mob: u32, target: u32 },
    /// `attackEntityFrom` on a mob by a world source (fire, lava).
    Hurt { id: u32, source: crate::game::damage::DamageSource, amount: f32 },
    /// `World.spawnEntity(arrow)` from `attackEntityWithRangedAttack`; the id is given on landing.
    Arrow(crate::game::mobs::arrow::Arrow),
    /// `World.playSound(null, ...)` at `pos` (`Entity.playSound`).
    Sound { pos: [f64; 3], name: String, category: u8, volume: f32, pitch: f32 },
    /// `SPacketEntityEquipment` to the trackers of mob `id` (`EntityLivingBase.onUpdate`).
    Equipment { id: u32, slot: u8, stack: items::ItemStack },
}

/// What the mob tick hands `Mob::update` besides the blocks.
pub struct Env<'a> {
    pub others: Others<'a>,
    pub players: &'a [PlayerView],
    pub rand: &'a mut JavaRandom,
    pub fx: &'a mut Vec<Effect>,
    /// `World.skylightSubtracted` (`isDaytime` is `< 4`).
    pub sky_subtracted: i32,
}

impl Env<'_> {
    /// `Entity.isEntityAlive` for a player or a mob by entity id.
    pub fn entity_alive(&self, id: u32) -> bool {
        if let Some(m) = self.others.get(id) {
            return !m.dead && m.health > 0.0;
        }
        self.players.iter().any(|p| p.id == id && !p.dead)
    }
}

/// The world a goal sees.
pub struct Ctx<'a> {
    pub w: &'a Blocks<'a>,
    pub mob: &'a mut Mob,
    pub others: Others<'a>,
    pub players: &'a [PlayerView],
    pub rand: &'a mut JavaRandom,
    pub fx: &'a mut Vec<Effect>,
    pub sky_subtracted: i32,
}

impl Ctx<'_> {
    /// `World.isDaytime`
    pub fn is_daytime(&self) -> bool {
        self.sky_subtracted < 4
    }

    /// `World.getClosestPlayer(x, y, z, distance, NOT_SPECTATING)`: the nearest by feet position
    /// strictly inside `distance` (any distance when negative).
    pub fn closest_player(&self, pos: [f64; 3], distance: f64) -> Option<PlayerView> {
        let mut best: Option<(f64, PlayerView)> = None;
        for p in self.players.iter().filter(|p| !p.spectator) {
            let d1 = dist_sq(p.pos, pos);
            if (distance < 0.0 || d1 < distance * distance) && best.is_none_or(|(d0, _)| d1 < d0) {
                best = Some((d1, *p));
            }
        }
        best.map(|b| b.1)
    }

    pub fn player(&self, id: u32) -> Option<PlayerView> {
        self.players.iter().find(|p| p.id == id).copied()
    }

    /// `EntitySenses.canSee` from this mob's eyes to a player's. DROPPED: the per-tick cache.
    pub fn can_see(&self, target: &PlayerView) -> bool {
        use glam::DVec3;
        let m = self.mob.body.pos;
        let from = DVec3::new(m.x, m.y + self.mob.eye_height() as f64, m.z);
        let e = target.eye_pos();
        sim::raytrace::ray_trace_blocks(self.w, from, DVec3::new(e[0], e[1], e[2]), false, false).is_none()
    }
}

/// `Entity.getDistanceSq(x, y, z)` between two feet positions.
pub fn dist_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy + dz * dz
}

/// `EntitySheep.getDyeColorMixFromParents`: the two parents' dye damages in a 2 by 1 crafting
/// grid of dye; a dye that comes out is the child's colour, else one parent's at random. Returns
/// the colour metadata. DROPPED: the grid is rebuilt each call instead of held per sheep.
pub fn sheep_dye_mix(game: &mut Game, i: i32, j: i32) -> u8 {
    use items::crafting::CraftingGrid;
    use items::ids::item;
    use items::ItemStack;
    let recipes = crate::game::windows::recipes(game);
    let mut grid = CraftingGrid::new(2, 1);
    grid.set_inventory_slot_contents(0, ItemStack::new(item::DYE, 1, i));
    grid.set_inventory_slot_contents(1, ItemStack::new(item::DYE, 1, j));
    let res = recipes.find_matching_result(&grid);
    let k = if !res.is_empty() && res.get_item() == item::DYE {
        res.get_metadata()
    } else if game.mobs.rand.next_boolean() {
        i
    } else {
        j
    };
    super::animals::meta_of_dye_damage(k)
}
