//! T5: the `EntityItem.onUpdate` physics on `sim::entity::Body`, so the server and viewer compute the
//! same arc. Also `searchForOtherItemsNearby` / `combineItems`, the age and pickup-delay fields, the
//! `onCollideWithPlayer` conditions and the `EntityItem(world, x, y, z)` constructor.
//!
//! Everything is a pure function of the entity and a [`BlockAccess`]. The caller (the server's
//! `game/items.rs`) owns the entity list: it calls [`ItemEntity::tick`], and when that returns
//! `true` it calls [`search_for_other_items_nearby`] over its item list. The viewer calls `tick`
//! with `remote = true`, which is the client branch of `onUpdate`.

use glam::DVec3;
use items::ItemStack;
use worldgen::java_random::JavaRandom;

use crate::collision::{collides_with_any_block, collision_box, floor, slipperiness, Aabb, BlockAccess};
use crate::entity::Body;
use crate::region::{props, Mat};

/// `pickupDelay` of an item that can never be picked up (`setInfinitePickupDelay`).
pub const INFINITE_PICKUP_DELAY: i32 = 32767;
/// `age` of an item that never despawns (`lifespan` is never the cap).
pub const INFINITE_AGE: i32 = -32768;
/// The despawn age (`age >= 6000`).
pub const LIFESPAN: i32 = 6000;
/// `setDefaultPickupDelay`
pub const DEFAULT_PICKUP_DELAY: i32 = 10;
/// `setNoDespawn`
pub const NO_DESPAWN_AGE: i32 = -6000;
/// `setAgeToCreativeDespawnTime`
pub const CREATIVE_DESPAWN_AGE: i32 = 4800;

/// A sound the tick asks for, played by whoever owns the entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemSound {
    /// `SoundEvents.ENTITY_GENERIC_BURN` at volume 0.4 and this pitch.
    Burn(f32),
}

/// `net.minecraft.entity.item.EntityItem`.
/// DROPPED: the `DataManager` (the stack is a plain field), `hoverStart` (the viewer's render
/// phase), the custom name, the thrower name as a UUID lookup, `attackEntityFrom` sources other
/// than fire and lava (explosions, `NETHER_STAR` immunity needs the source), NBT, and
/// `changeDimension`.
#[derive(Clone, Debug)]
pub struct ItemEntity {
    pub body: Body,
    /// `getItem()`
    pub stack: ItemStack,
    pub age: i32,
    pub pickup_delay: i32,
    pub health: i32,
    /// `Entity.ticksExisted`, bumped by `World.updateEntityWithOptionalForce` before `onUpdate`.
    pub ticks_existed: i32,
    /// `Entity.fire`, in ticks.
    pub fire: i32,
    /// `Entity.noClip`
    pub no_clip: bool,
    /// `Entity.firstUpdate`
    pub first_update: bool,
    /// `Entity.isDead`
    pub dead: bool,
    /// `Entity.isAirBorne`: the server resends the velocity.
    pub is_air_borne: bool,
    /// `EntityItem.owner`: the only player who may pick it up until the last 200 ticks.
    pub owner: Option<String>,
    pub thrower: Option<String>,
    /// `Entity.rand`
    pub rand: JavaRandom,
    pub sounds: Vec<ItemSound>,
}

impl ItemEntity {
    /// `new EntityItem(world, x, y, z, stack)`. The three `Math.random()` draws come from `rand`.
    pub fn new(x: f64, y: f64, z: f64, stack: ItemStack, mut rand: JavaRandom) -> ItemEntity {
        let mut body = Body::new(DVec3::new(x, y, z), 0.0, 0.0);
        // setSize(0.25F, 0.25F); stepHeight stays Entity's 0.
        body.width = 0.25;
        body.height = 0.25;
        body.step_height = 0.0;
        body.set_position(DVec3::new(x, y, z));
        // hoverStart = Math.random() * pi * 2 is DROPPED (render only), so the draw order is:
        body.yaw = (rand.next_double() * 360.0) as f32;
        body.prev_yaw = body.yaw;
        body.motion.x = (rand.next_double() * 0.200_000_002_980_232_24 - 0.100_000_001_490_116_12) as f32 as f64;
        body.motion.y = 0.200_000_002_980_232_24;
        body.motion.z = (rand.next_double() * 0.200_000_002_980_232_24 - 0.100_000_001_490_116_12) as f32 as f64;
        ItemEntity {
            body,
            stack,
            age: 0,
            pickup_delay: 0,
            health: 5,
            ticks_existed: 0,
            fire: 0,
            no_clip: false,
            first_update: true,
            dead: false,
            is_air_borne: false,
            owner: None,
            thrower: None,
            rand,
            sounds: Vec::new(),
        }
    }

    /// `setDefaultPickupDelay`
    pub fn set_default_pickup_delay(&mut self) {
        self.pickup_delay = DEFAULT_PICKUP_DELAY;
    }
    /// `setPickupDelay`
    pub fn set_pickup_delay(&mut self, ticks: i32) {
        self.pickup_delay = ticks;
    }
    /// `setNoPickupDelay`
    pub fn set_no_pickup_delay(&mut self) {
        self.pickup_delay = 0;
    }
    /// `setInfinitePickupDelay`
    pub fn set_infinite_pickup_delay(&mut self) {
        self.pickup_delay = INFINITE_PICKUP_DELAY;
    }
    /// `setNoDespawn`
    pub fn set_no_despawn(&mut self) {
        self.age = NO_DESPAWN_AGE;
    }
    /// `setAgeToCreativeDespawnTime`
    pub fn set_age_to_creative_despawn_time(&mut self) {
        self.age = CREATIVE_DESPAWN_AGE;
    }
    /// `Entity.isEntityAlive`
    pub fn is_alive(&self) -> bool {
        !self.dead
    }

    /// `EntityItem.attackEntityFrom` for damage that reaches the health counter (fire, lava).
    /// DROPPED: `isEntityInvulnerable` (items are never invulnerable here), the nether star
    /// explosion immunity and `markVelocityChanged`.
    pub fn attack_entity_from(&mut self, amount: f32) {
        self.health = (self.health as f32 - amount) as i32;
        if self.health <= 0 {
            self.dead = true;
        }
    }

    /// `Entity.onEntityUpdate`, the parts that touch an item: `handleWaterMovement`, lava, the
    /// void. DROPPED: riding, portals, running particles, the glowing and fire flags.
    fn on_entity_update(&mut self, w: &impl BlockAccess, remote: bool) {
        self.body.prev_pos = self.body.pos;
        self.body.prev_yaw = self.body.yaw;
        self.body.prev_pitch = self.body.pitch;
        self.handle_water_movement(w);
        if !remote && self.fire > 0 {
            // isImmuneToFire is false for items.
            if self.fire % 20 == 0 {
                self.attack_entity_from(1.0);
            }
            self.fire -= 1;
        }
        if self.body.is_in_lava(w) {
            // setOnFireFromLava: attackEntityFrom(LAVA, 4.0F), setFire(15).
            self.attack_entity_from(4.0);
            self.fire = self.fire.max(15 * 20);
        }
        if self.body.pos.y < -64.0 {
            // outOfWorld
            self.dead = true;
        }
        self.first_update = false;
    }

    /// `EntityItem.handleWaterMovement`: no splash on the first update.
    fn handle_water_movement(&mut self, w: &impl BlockAccess) {
        let n = self.body.sounds.len();
        self.body.handle_water_movement(w);
        if self.first_update {
            self.body.sounds.truncate(n);
        }
    }

    /// `Entity.pushOutOfBlocks` (the plain one, not `EntityPlayerSP`'s): true while the box is in a
    /// block, with a push toward the nearest open face.
    fn push_out_of_blocks(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64) -> bool {
        let (bx, by, bz) = (floor(x), floor(y), floor(z));
        let d0 = x - bx as f64;
        let d1 = y - by as f64;
        let d2 = z - bz as f64;
        if !collides_with_any_block(w, &self.body.bb) {
            return false;
        }
        // World.isBlockFullCube
        let full = |x: i32, y: i32, z: i32| collision_box(w.state(x, y, z)).is_some_and(|b| (b.max_x - b.min_x + b.max_y - b.min_y + b.max_z - b.min_z) / 3.0 >= 1.0);
        // 0 west, 1 east, 2 north, 3 south, 4 up
        let mut face = 4;
        let mut d3 = f64::MAX;
        if !full(bx - 1, by, bz) && d0 < d3 {
            d3 = d0;
            face = 0;
        }
        if !full(bx + 1, by, bz) && 1.0 - d0 < d3 {
            d3 = 1.0 - d0;
            face = 1;
        }
        if !full(bx, by, bz - 1) && d2 < d3 {
            d3 = d2;
            face = 2;
        }
        if !full(bx, by, bz + 1) && 1.0 - d2 < d3 {
            d3 = 1.0 - d2;
            face = 3;
        }
        if !full(bx, by + 1, bz) && 1.0 - d1 < d3 {
            face = 4;
        }
        let f = self.rand.next_float() * 0.2 + 0.1;
        let f1 = if face == 0 || face == 2 { -1.0f32 } else { 1.0 };
        let m = &mut self.body.motion;
        match face {
            0 | 1 => {
                m.x = (f1 * f) as f64;
                m.y *= 0.75;
                m.z *= 0.75;
            }
            4 => {
                m.x *= 0.75;
                m.y = (f1 * f) as f64;
                m.z *= 0.75;
            }
            _ => {
                m.x *= 0.75;
                m.y *= 0.75;
                m.z = (f1 * f) as f64;
            }
        }
        true
    }

    /// `EntityItem.onUpdate`. Returns true when the server should now run
    /// [`search_for_other_items_nearby`] for this item (the `searchForOtherItemsNearby` call site,
    /// which needs the entity list). `remote` is `world.isRemote`.
    /// DROPPED: `hasNoGravity` (always false), the `onGround` lava sound's `playSound` position
    /// (the caller plays [`ItemSound::Burn`] at the entity).
    pub fn tick(&mut self, w: &impl BlockAccess, remote: bool) -> bool {
        if self.stack.is_empty() {
            self.dead = true;
            return false;
        }
        // World.updateEntityWithOptionalForce: ++ticksExisted before onUpdate.
        self.ticks_existed += 1;
        // super.onUpdate -> onEntityUpdate
        self.on_entity_update(w, remote);
        if self.pickup_delay > 0 && self.pickup_delay != INFINITE_PICKUP_DELAY {
            self.pickup_delay -= 1;
        }

        self.body.prev_pos = self.body.pos;
        let (d0, d1, d2) = (self.body.motion.x, self.body.motion.y, self.body.motion.z);
        self.body.motion.y -= 0.039_999_999_105_930_33;

        if remote {
            self.no_clip = false;
        } else {
            let mid = (self.body.bb.min_y + self.body.bb.max_y) / 2.0;
            self.no_clip = self.push_out_of_blocks(w, self.body.pos.x, mid, self.body.pos.z);
        }

        let m = self.body.motion;
        if self.no_clip {
            // Entity.move with noClip
            self.body.bb = self.body.bb.offset(m.x, m.y, m.z);
            self.body.reset_position_to_bb();
        } else {
            // canTriggerWalking is false for items, so no step sounds or walked distance.
            self.body.move_entity(w, m.x, m.y, m.z, false, false);
        }

        let p = self.body.pos;
        let q = self.body.prev_pos;
        let flag = q.x as i32 != p.x as i32 || q.y as i32 != p.y as i32 || q.z as i32 != p.z as i32;
        let mut search = false;
        if flag || self.ticks_existed % 25 == 0 {
            let here = w.state(floor(p.x), floor(p.y), floor(p.z));
            if props(here).mat == Mat::Lava {
                self.body.motion.y = 0.200_000_002_980_232_24;
                self.body.motion.x = ((self.rand.next_float() - self.rand.next_float()) * 0.2) as f64;
                self.body.motion.z = ((self.rand.next_float() - self.rand.next_float()) * 0.2) as f64;
                self.sounds.push(ItemSound::Burn(2.0 + self.rand.next_float() * 0.4));
            }
            if !remote {
                search = true;
            }
        }

        let mut f = 0.98f32;
        if self.body.on_ground {
            let under = w.state(floor(p.x), floor(self.body.bb.min_y) - 1, floor(p.z));
            f = slipperiness(under) * 0.98;
        }
        self.body.motion.x *= f as f64;
        self.body.motion.y *= 0.980_000_019_073_486_3;
        self.body.motion.z *= f as f64;
        if self.body.on_ground {
            self.body.motion.y *= -0.5;
        }

        if self.age != INFINITE_AGE {
            self.age += 1;
        }
        self.handle_water_movement(w);

        if !remote {
            let m = self.body.motion;
            let (d3, d4, d5) = (m.x - d0, m.y - d1, m.z - d2);
            if d3 * d3 + d4 * d4 + d5 * d5 > 0.01 {
                self.is_air_borne = true;
            }
        }
        if !remote && self.age >= LIFESPAN {
            self.dead = true;
        }
        search && self.is_alive()
    }

    /// The box `searchForOtherItemsNearby` queries: `getEntityBoundingBox().grow(0.5, 0, 0.5)`.
    pub fn merge_box(&self) -> Aabb {
        self.body.bb.grow(0.5, 0.0, 0.5)
    }

    /// `EntityItem.onCollideWithPlayer`'s server branch. `add` is
    /// `inventory.addItemStackToInventory` on the stack (it shrinks it in place and says whether
    /// anything was taken). Returns the count picked up (`entityIn.onItemPickup(this, i)`), or
    /// `None` when the player could not take it. The item dies when the stack is emptied.
    /// DROPPED: the stat.
    pub fn on_collide_with_player(&mut self, player_name: &str, add: impl FnOnce(&mut ItemStack) -> bool) -> Option<i32> {
        let i = self.stack.count;
        let owner_ok = self.owner.as_deref().is_none_or(|o| LIFESPAN - self.age <= 200 || o == player_name);
        if self.pickup_delay == 0 && owner_ok && add(&mut self.stack) {
            if self.stack.is_empty() {
                self.dead = true;
                self.stack.count = i;
            }
            return Some(i);
        }
        None
    }
}

/// `EntityItem.combineItems(other)`: `this` is absorbed into `other`, or the reverse when `other`
/// holds fewer. True when `this` was absorbed or absorbed `other` through the swap.
/// The `other == this` check is the caller's (two `&mut` cannot alias).
pub fn combine_items(this: &mut ItemEntity, other: &mut ItemEntity) -> bool {
    if !(other.is_alive() && this.is_alive()) {
        return false;
    }
    if this.pickup_delay == INFINITE_PICKUP_DELAY || other.pickup_delay == INFINITE_PICKUP_DELAY {
        return false;
    }
    if this.age == INFINITE_AGE || other.age == INFINITE_AGE {
        return false;
    }
    let (s, s1) = (this.stack, other.stack);
    if s1.item != s.item {
        return false;
    }
    if s1.has_tag_compound() ^ s.has_tag_compound() {
        return false;
    }
    if s1.has_tag_compound() && (s1.tag != s.tag || s1.ench != s.ench) {
        return false;
    }
    if s1.get_has_subtypes() && s1.get_metadata() != s.get_metadata() {
        return false;
    }
    if s1.count < s.count {
        return combine_items(other, this);
    }
    if s1.count + s.count > s1.get_max_stack_size() {
        return false;
    }
    other.stack.count += s.count;
    other.pickup_delay = other.pickup_delay.max(this.pickup_delay);
    other.age = other.age.min(this.age);
    this.dead = true;
    true
}

/// `EntityItem.searchForOtherItemsNearby` for `items[i]`: `combineItems` against every item whose
/// box meets [`ItemEntity::merge_box`], in list order (itself skipped).
pub fn search_for_other_items_nearby(items: &mut [ItemEntity], i: usize) {
    let area = items[i].merge_box();
    for j in 0..items.len() {
        if j == i || !items[j].body.bb.intersects(&area) {
            continue;
        }
        let (a, b) = if i < j {
            let (l, r) = items.split_at_mut(j);
            (&mut l[i], &mut r[0])
        } else {
            let (l, r) = items.split_at_mut(i);
            (&mut r[0], &mut l[j])
        };
        combine_items(a, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Blocks;

    fn stack(n: i32) -> ItemStack {
        // cobblestone: 64 per stack, no subtypes
        ItemStack::new(4, n, 0)
    }

    fn item_at(y: f64, n: i32) -> ItemEntity {
        let mut e = ItemEntity::new(0.5, y, 0.5, stack(n), JavaRandom::new(1));
        e.body.motion = DVec3::ZERO;
        e
    }

    #[test]
    fn rests_on_stone_at_the_vanilla_height_and_tick() {
        // Stone fills y = 0, so the surface is y = 1. Dropped 3 above, at rest.
        let w = Blocks::floor_at(0, 3);
        let mut e = item_at(4.0, 1);
        // The same recurrence by hand: motionY = (motionY - 0.04) * 0.98, y += motionY, until the
        // move would pass y = 1.
        let (mut y, mut vy, mut expect) = (4.0f64, 0.0f64, 0);
        loop {
            expect += 1;
            vy -= 0.039_999_999_105_930_33;
            if y + vy <= 1.0 {
                break;
            }
            y += vy;
            vy *= 0.980_000_019_073_486_3;
        }
        let mut landed = 0;
        for t in 1..=200 {
            e.tick(&w, false);
            if e.body.on_ground && landed == 0 {
                landed = t;
            }
        }
        assert_eq!(landed, expect, "first tick on the ground");
        assert_eq!(e.body.pos.y, 1.0);
        assert!(e.body.on_ground && e.is_alive());
    }

    #[test]
    fn merging() {
        let mut v = vec![item_at(1.0, 40), item_at(1.0, 40)];
        search_for_other_items_nearby(&mut v, 0);
        assert!(v[0].is_alive() && v[1].is_alive(), "40 + 40 > 64 stays two stacks");

        let mut v = vec![item_at(1.0, 20), item_at(1.0, 40)];
        v[0].set_pickup_delay(7);
        v[1].age = 100;
        search_for_other_items_nearby(&mut v, 0);
        assert!(v[0].dead && v[1].stack.count == 60, "the smaller stack joins the larger");
        assert_eq!((v[1].pickup_delay, v[1].age), (7, 0));

        let mut v = vec![item_at(1.0, 30), item_at(1.0, 30)];
        search_for_other_items_nearby(&mut v, 1);
        assert!(v[1].dead && v[0].stack.count == 60);

        // different items and out-of-range items do not merge
        let mut v = vec![item_at(1.0, 5), item_at(1.0, 5)];
        v[1].stack.item = 5;
        search_for_other_items_nearby(&mut v, 0);
        assert!(v[0].is_alive() && v[1].is_alive());
        let mut v = vec![item_at(1.0, 5), item_at(1.0, 5)];
        v[1].body.set_position(DVec3::new(3.0, 1.0, 0.5));
        search_for_other_items_nearby(&mut v, 0);
        assert!(v[0].is_alive() && v[1].is_alive());
    }

    #[test]
    fn tick_asks_for_the_search_every_25_ticks() {
        let w = Blocks::floor_at(0, 3);
        let mut e = item_at(1.0, 1);
        let mut asks = Vec::new();
        for _ in 0..50 {
            if e.tick(&w, false) {
                asks.push(e.ticks_existed);
            }
        }
        assert_eq!(asks, vec![25, 50]);
        let mut c = item_at(1.0, 1);
        assert!(!(1..=50).any(|_| c.tick(&w, true)), "the client never searches");
    }

    #[test]
    fn pickup_delay() {
        let w = Blocks::floor_at(0, 3);
        let mut e = item_at(1.0, 3);
        e.set_default_pickup_delay();
        for _ in 0..9 {
            e.tick(&w, false);
            assert!(e.on_collide_with_player("xy", |_| true).is_none());
        }
        assert_eq!(e.pickup_delay, 1);
        e.tick(&w, false);
        assert_eq!(e.pickup_delay, 0);
        assert_eq!(e.on_collide_with_player("xy", |s| { s.count = 0; true }), Some(3));
        assert!(e.dead);

        let mut f = item_at(1.0, 3);
        f.set_infinite_pickup_delay();
        f.tick(&w, false);
        assert_eq!(f.pickup_delay, INFINITE_PICKUP_DELAY);

        // a full inventory leaves the item
        let mut g = item_at(1.0, 3);
        assert_eq!(g.on_collide_with_player("xy", |_| false), None);
        assert!(g.is_alive());
        // an owned item waits for its owner until the last 200 ticks
        g.owner = Some("a".into());
        assert_eq!(g.on_collide_with_player("b", |_| true), None);
        assert_eq!(g.on_collide_with_player("a", |_| true), Some(3));
        let mut h = item_at(1.0, 3);
        h.owner = Some("a".into());
        h.age = 5799;
        assert_eq!(h.on_collide_with_player("b", |_| true), None);
        h.age = 5800;
        assert_eq!(h.on_collide_with_player("b", |_| true), Some(3));
    }

    #[test]
    fn despawns_at_6000() {
        let w = Blocks::floor_at(0, 3);
        let mut e = item_at(1.0, 1);
        for _ in 0..5999 {
            e.tick(&w, false);
        }
        assert!(e.is_alive() && e.age == 5999);
        e.tick(&w, false);
        assert!(e.dead);
        // the client does not despawn
        let mut c = item_at(1.0, 1);
        c.age = 5999;
        c.tick(&w, true);
        assert!(c.is_alive());
        // the infinite age never counts up
        let mut n = item_at(1.0, 1);
        n.age = INFINITE_AGE;
        n.tick(&w, false);
        assert_eq!(n.age, INFINITE_AGE);
    }

    #[test]
    fn void_and_empty_stack() {
        let w = Blocks::default();
        let mut e = item_at(-70.0, 1);
        e.tick(&w, false);
        assert!(e.dead);
        let mut z = item_at(10.0, 0);
        z.tick(&w, false);
        assert!(z.dead);
    }
}
