//! `EntityXPOrb` on `sim::entity::Body`: the `onUpdate` physics (gravity 0.03, the lava bounce, the
//! pull toward the closest player within 8 blocks, the slipperiness drag, the bounce off the
//! ground), `getXPSplit`, `getTextureByXP`, and the age 6000 despawn. The server owns the list and
//! picks the orbs up (`server/game/xp_orbs.rs`); the viewer ticks its copies between the server's
//! updates (`viewer/survival/xp_orbs.rs`).
//!
//! DROPPED: the `DataManager` (an orb has no data), Mending in `onCollideWithPlayer` (no
//! enchantments yet), `Entity.handleMaterialAcceleration`'s flow push on the motion (the port's
//! helper only reports the material), NBT, the `hasNoGravity` branch (always false), `ticksExisted`
//! (nothing in an orb reads it).

use glam::DVec3;
use worldgen::java_random::JavaRandom;

use crate::collision::{collides_with_any_block, collision_box, floor, handle_material_acceleration, slipperiness, BlockAccess};
use crate::entity::Body;
use crate::region::{props, Mat};

/// `age >= 6000` despawns.
pub const LIFESPAN: i32 = 6000;

/// A player an orb can chase: `World.getClosestPlayerToEntity` reads these.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub id: u32,
    /// `posX, posY, posZ`
    pub pos: [f64; 3],
    /// `getEyeHeight()`
    pub eye_height: f64,
    pub spectator: bool,
}

/// `net.minecraft.entity.item.EntityXPOrb`.
#[derive(Clone, Debug)]
pub struct XpOrb {
    pub body: Body,
    /// `xpValue`
    pub value: i32,
    /// `xpColor`: the tick counter the render pulse reads.
    pub color: i32,
    /// `xpOrbAge`
    pub age: i32,
    /// `delayBeforeCanPickup`
    pub delay_before_can_pickup: i32,
    /// `xpOrbHealth`
    pub health: i32,
    /// `xpTargetColor`
    target_color: i32,
    /// `closestPlayer`, as an id into the targets the tick is given.
    closest: Option<u32>,
    /// `Entity.isDead`
    pub dead: bool,
    /// `Entity.velocityChanged`: a hit set the motion.
    pub velocity_changed: bool,
    /// `Entity.isAirBorne`
    pub is_air_borne: bool,
    /// `Entity.fire`, in ticks.
    pub fire: i32,
    /// `Entity.rand`
    pub rand: JavaRandom,
    /// The lava burn sounds the tick asked for, as their pitch (volume 0.4).
    pub sounds: Vec<f32>,
}

impl XpOrb {
    /// `new EntityXPOrb(world, x, y, z, expValue)`. The five `Math.random()` draws come from `rand`.
    pub fn new(x: f64, y: f64, z: f64, value: i32, mut rand: JavaRandom) -> XpOrb {
        let mut body = Body::new(DVec3::new(x, y, z), 0.0, 0.0);
        // setSize(0.5F, 0.5F); stepHeight stays Entity's 0.
        body.width = 0.5;
        body.height = 0.5;
        body.step_height = 0.0;
        body.set_position(DVec3::new(x, y, z));
        body.yaw = (rand.next_double() * 360.0) as f32;
        body.prev_yaw = body.yaw;
        body.motion.x = ((rand.next_double() * 0.200_000_002_980_232_24 - 0.100_000_001_490_116_12) as f32 * 2.0) as f64;
        body.motion.y = ((rand.next_double() * 0.2) as f32 * 2.0) as f64;
        body.motion.z = ((rand.next_double() * 0.200_000_002_980_232_24 - 0.100_000_001_490_116_12) as f32 * 2.0) as f64;
        XpOrb {
            body,
            value,
            color: 0,
            age: 0,
            delay_before_can_pickup: 0,
            health: 5,
            target_color: 0,
            closest: None,
            dead: false,
            velocity_changed: false,
            is_air_borne: false,
            fire: 0,
            rand,
            sounds: Vec::new(),
        }
    }

    /// `Entity.isEntityAlive`
    pub fn is_alive(&self) -> bool {
        !self.dead
    }

    /// `EntityXPOrb.attackEntityFrom` for fire and lava: `markVelocityChanged`, then the health.
    pub fn attack_entity_from(&mut self, amount: f32) {
        self.velocity_changed = true;
        self.health = (self.health as f32 - amount) as i32;
        if self.health <= 0 {
            self.dead = true;
        }
    }

    /// `EntityXPOrb.getTextureByXP`: the frame of `experience_orb.png`, 0 to 10.
    pub fn texture_by_xp(&self) -> i32 {
        texture_by_xp(self.value)
    }

    /// `Entity.pushOutOfBlocks` (the plain one `EntityItem` also uses): a nudge toward the nearest
    /// open face while the box is in a block.
    fn push_out_of_blocks(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64) {
        let (bx, by, bz) = (floor(x), floor(y), floor(z));
        let d0 = x - bx as f64;
        let d1 = y - by as f64;
        let d2 = z - bz as f64;
        if !collides_with_any_block(w, &self.body.bb) {
            return;
        }
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
    }

    /// `EntityXPOrb.onUpdate`, with `Entity.onEntityUpdate` before it. `id` is `getEntityId()`,
    /// `targets` the players in the world, `remote` is `world.isRemote` (the fire damage is the
    /// server's).
    pub fn tick(&mut self, id: u32, w: &impl BlockAccess, targets: &[Target], remote: bool) {
        // Entity.onEntityUpdate
        self.body.prev_pos = self.body.pos;
        self.body.prev_yaw = self.body.yaw;
        self.body.prev_pitch = self.body.pitch;
        // EntityXPOrb.handleWaterMovement: the plain box.
        self.body.in_water = handle_material_acceleration(w, &self.body.bb, Mat::Water);
        if !remote && self.fire > 0 {
            if self.fire % 20 == 0 {
                self.attack_entity_from(1.0);
            }
            self.fire -= 1;
        }
        if self.body.is_in_lava(w) {
            // setOnFireFromLava: attackEntityFrom(LAVA, 4.0F), setFire(15).
            if !remote {
                self.attack_entity_from(4.0);
            }
            self.fire = self.fire.max(15 * 20);
        }
        if self.body.pos.y < -64.0 {
            self.dead = true;
        }

        if self.delay_before_can_pickup > 0 {
            self.delay_before_can_pickup -= 1;
        }
        self.body.prev_pos = self.body.pos;
        self.body.motion.y -= 0.029_999_999_329_447_746;
        let p = self.body.pos;
        if props(w.state(floor(p.x), floor(p.y), floor(p.z))).mat == Mat::Lava {
            self.body.motion.y = 0.200_000_002_980_232_24;
            self.body.motion.x = ((self.rand.next_float() - self.rand.next_float()) * 0.2) as f64;
            self.body.motion.z = ((self.rand.next_float() - self.rand.next_float()) * 0.2) as f64;
            self.sounds.push(2.0 + self.rand.next_float() * 0.4);
        }
        let mid = (self.body.bb.min_y + self.body.bb.max_y) / 2.0;
        self.push_out_of_blocks(w, self.body.pos.x, mid, self.body.pos.z);

        if self.target_color < self.color - 20 + (id % 100) as i32 {
            let keep = self.closest.and_then(|c| targets.iter().find(|t| t.id == c)).is_some_and(|t| dist_sq(t.pos, self.body.pos) <= 64.0);
            if !keep {
                self.closest = closest_player(targets, self.body.pos, 8.0);
            }
            self.target_color = self.color;
        }
        let target = self.closest.and_then(|c| targets.iter().find(|t| t.id == c)).filter(|t| !t.spectator).copied();
        if target.is_none() {
            self.closest = None;
        }
        if let Some(t) = target {
            let pos = self.body.pos;
            let d1 = (t.pos[0] - pos.x) / 8.0;
            let d2 = (t.pos[1] + t.eye_height / 2.0 - pos.y) / 8.0;
            let d3 = (t.pos[2] - pos.z) / 8.0;
            let d4 = (d1 * d1 + d2 * d2 + d3 * d3).sqrt();
            let mut d5 = 1.0 - d4;
            if d5 > 0.0 {
                d5 *= d5;
                self.body.motion.x += d1 / d4 * d5 * 0.1;
                self.body.motion.y += d2 / d4 * d5 * 0.1;
                self.body.motion.z += d3 / d4 * d5 * 0.1;
            }
        }

        let m = self.body.motion;
        self.body.move_entity(w, m.x, m.y, m.z, false, false);
        let mut f = 0.98f32;
        if self.body.on_ground {
            let p = self.body.pos;
            f = slipperiness(w.state(floor(p.x), floor(self.body.bb.min_y) - 1, floor(p.z))) * 0.98;
        }
        self.body.motion.x *= f as f64;
        self.body.motion.y *= 0.980_000_019_073_486_3;
        self.body.motion.z *= f as f64;
        if self.body.on_ground {
            self.body.motion.y *= -0.899_999_976_158_142_1;
        }
        self.color += 1;
        self.age += 1;
        if self.age >= LIFESPAN {
            self.dead = true;
        }
    }
}

fn dist_sq(a: [f64; 3], b: DVec3) -> f64 {
    let (dx, dy, dz) = (a[0] - b.x, a[1] - b.y, a[2] - b.z);
    dx * dx + dy * dy + dz * dz
}

/// `World.getClosestPlayer(x, y, z, distance, NOT_SPECTATING)`: the nearest within `distance`.
fn closest_player(targets: &[Target], at: DVec3, distance: f64) -> Option<u32> {
    let mut best = -1.0f64;
    let mut found = None;
    for t in targets.iter().filter(|t| !t.spectator) {
        let d = dist_sq(t.pos, at);
        if d < distance * distance && (best == -1.0 || d < best) {
            best = d;
            found = Some(t.id);
        }
    }
    found
}

/// `EntityLiving.experienceValue` as the constructors leave it: 5 for an `EntityMob` (zombie,
/// skeleton, creeper, spider), 0 for the animals, which override `getExperiencePoints` instead
/// (1 to 3, `EntityAnimal`).
pub fn experience_value(kind: &str) -> i32 {
    match kind {
        "zombie" | "skeleton" | "creeper" | "spider" => 5,
        _ => 0,
    }
}

/// `EntityXPOrb.getTextureByXP`
pub fn texture_by_xp(xp: i32) -> i32 {
    match xp {
        2477.. => 10,
        1237.. => 9,
        617.. => 8,
        307.. => 7,
        149.. => 6,
        73.. => 5,
        37.. => 4,
        17.. => 3,
        7.. => 2,
        3.. => 1,
        _ => 0,
    }
}

/// `EntityXPOrb.getXPSplit`: the biggest orb value that fits in `xp`.
pub fn xp_split(xp: i32) -> i32 {
    match xp {
        2477.. => 2477,
        1237.. => 1237,
        617.. => 617,
        307.. => 307,
        149.. => 149,
        73.. => 73,
        37.. => 37,
        17.. => 17,
        7.. => 7,
        3.. => 3,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Void;
    impl BlockAccess for Void {
        fn state(&self, _: i32, _: i32, _: i32) -> u16 {
            0
        }
        fn loaded(&self, _: i32, _: i32) -> bool {
            true
        }
    }

    #[test]
    fn splits_follow_the_vanilla_steps() {
        let mut left = 2500;
        let mut parts = Vec::new();
        while left > 0 {
            let k = xp_split(left);
            left -= k;
            parts.push(k);
        }
        assert_eq!(parts, vec![2477, 17, 3, 3]);
        assert_eq!((xp_split(1), xp_split(2), xp_split(5), xp_split(6)), (1, 1, 3, 3));
        assert_eq!((texture_by_xp(2), texture_by_xp(3), texture_by_xp(7), texture_by_xp(2477)), (0, 1, 2, 10));
    }

    #[test]
    fn an_orb_falls_and_is_pulled_toward_a_player() {
        let mut o = XpOrb::new(0.5, 100.0, 0.5, 5, JavaRandom::new(1));
        o.body.motion = DVec3::ZERO;
        o.tick(1, &Void, &[], false);
        assert!(o.body.motion.y < 0.0);
        let t = [Target { id: 9, pos: [4.5, 100.0, 0.5], eye_height: 1.62, spectator: false }];
        let mut o = XpOrb::new(0.5, 100.0, 0.5, 5, JavaRandom::new(1));
        o.body.motion = DVec3::ZERO;
        // The target is looked for once xpColor passes xpTargetColor - 20 + id % 100.
        for _ in 0..30 {
            o.tick(1, &Void, &t, false);
        }
        assert!(o.body.motion.x > 0.0, "pulled toward +x: {:?}", o.body.motion);
        let far = [Target { pos: [20.5, 100.0, 0.5], ..t[0] }];
        let mut o = XpOrb::new(0.5, 100.0, 0.5, 5, JavaRandom::new(1));
        o.body.motion = DVec3::ZERO;
        for _ in 0..30 {
            o.tick(1, &Void, &far, false);
        }
        assert_eq!(o.body.motion.x, 0.0);
    }

    #[test]
    fn an_orb_despawns_at_6000_ticks() {
        let mut o = XpOrb::new(0.5, 100.0, 0.5, 1, JavaRandom::new(1));
        o.age = LIFESPAN - 1;
        o.tick(1, &Void, &[], false);
        assert!(o.dead);
    }
}
