//! `EntityArrow` (T9k), ported 1:1 from 1.12.2 as a server entity: `shoot`, `onUpdate` (flight,
//! gravity 0.05, drag 0.99 and 0.6 in water, sticking in a block, the 1200 tick despawn),
//! `findEntityOnPath`, `onHit` (entity damage `ceil(speed * damage)`, crits, fire, the bounce off
//! a refused hit; block hit) and `onCollideWithPlayer` (pickup).
//!
//! The tracker shows an arrow as `S_SPAWN_OBJECT` kind arrow ([`ArrowView`]): the spawn frame
//! carries the motion, the viewer flies it with gravity 0.05 and drag 0.99 (0.6 in water), and an
//! `S_ENTITY_MOVE` every 20 ticks corrects it, with `on_ground` set once it sticks in a block.
//!
//! DROPPED: `EntityTippedArrow` potion effects and the colour metadata, `setArrowCountInEntity` and the thorns calls, the shield disable (`EntityArrow`
//! does none), `doBlockCollisions`, the enderman exemptions, `getIsCritical` on the wire, the
//! particles and sounds, `isWet`/`extinguish` beyond the fire counter, the NBT save (an arrow
//! does not survive a restart), and player-shot arrows (no bow item yet; the pickup and the
//! `Pickup` states are ported for them).

use glam::DVec3;
use items::ids::item;
use items::ItemStack;
use sim::collision::{ceil, collision_box, collides_with_any_block, floor, Aabb, BlockAccess};
use sim::entity::Body;
use sim::kinds::{Kind, ARROW};
use sim::raytrace::{calculate_intercept, ray_trace_blocks};
use worldgen::java_random::JavaRandom;

use super::path::math::atan2;
use super::combat;
use crate::game::damage::{self, Attacker, DamageSource};
use crate::game::terrain::Blocks;
use crate::game::tracker::Tracked;
use crate::game::{items as game_items, net, players, tracker, Game};

/// `EntityArrow.PickupStatus`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pickup {
    Disallowed,
    Allowed,
    CreativeOnly,
}

/// `EntityArrow`
#[derive(Clone, Debug)]
pub struct Arrow {
    pub id: u32,
    pub body: Body,
    pub dead: bool,
    /// `xTile, yTile, zTile` and the state there (`inTile`, `inData`).
    pub tile: (i32, i32, i32),
    pub in_tile: u16,
    pub in_ground: bool,
    pub pickup: Pickup,
    pub arrow_shake: i32,
    /// `shootingEntity`: an entity id (a session id for a player).
    pub shooter: Option<u32>,
    pub ticks_in_ground: i32,
    pub ticks_in_air: i32,
    /// `damage`, 2.0 by default.
    pub damage: f64,
    pub knockback: i32,
    pub critical: bool,
    /// `Entity.fire`, in ticks.
    pub fire: i32,
}

impl Arrow {
    /// `new EntityArrow(world, shooter)`: size 0.5, at the shooter's eyes less 0.1.
    pub fn new(shooter: Option<u32>, pos: [f64; 3]) -> Arrow {
        let mut body = Body::new(DVec3::new(pos[0], pos[1], pos[2]), 0.0, 0.0);
        body.width = ARROW.width;
        body.height = ARROW.height;
        body.set_position(body.pos);
        Arrow {
            id: 0,
            body,
            dead: false,
            tile: (-1, -1, -1),
            in_tile: 0,
            in_ground: false,
            pickup: Pickup::Disallowed,
            arrow_shake: 0,
            shooter,
            ticks_in_ground: 0,
            ticks_in_air: 0,
            damage: 2.0,
            knockback: 0,
            critical: false,
            fire: 0,
        }
    }

    /// `EntityArrow.shoot(x, y, z, velocity, inaccuracy)`
    pub fn shoot(&mut self, x: f64, y: f64, z: f64, velocity: f32, inaccuracy: f32, rand: &mut JavaRandom) {
        let f = (x * x + y * y + z * z).sqrt() as f32;
        let (mut x, mut y, mut z) = (x / f as f64, y / f as f64, z / f as f64);
        let k = 0.007_499_999_832_361_937 * inaccuracy as f64;
        x += rand.next_gaussian() * k;
        y += rand.next_gaussian() * k;
        z += rand.next_gaussian() * k;
        x *= velocity as f64;
        y *= velocity as f64;
        z *= velocity as f64;
        self.body.motion = DVec3::new(x, y, z);
        let f1 = (x * x + z * z).sqrt() as f32;
        self.body.yaw = (atan2(x, z) * 57.295_779_513_082_32) as f32;
        self.body.pitch = (atan2(y, f1 as f64) * 57.295_779_513_082_32) as f32;
        self.body.prev_yaw = self.body.yaw;
        self.body.prev_pitch = self.body.pitch;
        self.ticks_in_ground = 0;
    }

    pub fn is_burning(&self) -> bool {
        self.fire > 0
    }

    pub fn speed(&self) -> f64 {
        let m = self.body.motion;
        (m.x * m.x + m.y * m.y + m.z * m.z).sqrt() as f32 as f64
    }
}

/// A block read as air where it has no collision box: `ignoreBlockWithoutBoundingBox` of
/// `World.rayTraceBlocks`, which `sim::raytrace` does not take.
struct Solid<'a>(&'a Blocks<'a>);

impl BlockAccess for Solid<'_> {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        let s = self.0.state(x, y, z);
        if collision_box(s).is_none() {
            0
        } else {
            s
        }
    }
    fn loaded(&self, x: i32, z: i32) -> bool {
        self.0.loaded(x, z)
    }
}

/// What an arrow can hit: `ARROW_TARGETS` (alive and collidable) with the box `getEntityBoundingBox`.
struct Target {
    id: u32,
    bb: Aabb,
    player: bool,
}

fn targets(game: &Game, around: &Aabb) -> Vec<Target> {
    let mut v = Vec::new();
    for pv in super::ai::player_views(game) {
        if pv.spectator || pv.dead {
            continue;
        }
        let h = if pv.sneaking { 1.65 } else { 1.8 };
        let bb = Aabb::new(pv.pos[0] - 0.3, pv.pos[1], pv.pos[2] - 0.3, pv.pos[0] + 0.3, pv.pos[1] + h, pv.pos[2] + 0.3);
        if bb.intersects(around) {
            v.push(Target { id: pv.id, bb, player: true });
        }
    }
    for m in &game.mobs.mobs {
        if m.dead || m.health <= 0.0 || !m.body.bb.intersects(around) {
            continue;
        }
        v.push(Target { id: m.id, bb: m.body.bb, player: false });
    }
    v
}

/// `EntityArrow.findEntityOnPath`
fn find_entity_on_path(game: &Game, a: &Arrow, start: DVec3, end: DVec3) -> Option<(u32, bool)> {
    let m = a.body.motion;
    let around = a.body.bb.expand(m.x, m.y, m.z).grow(1.0, 1.0, 1.0);
    let mut best: Option<(u32, bool)> = None;
    let mut d0 = 0.0;
    for t in targets(game, &around) {
        if Some(t.id) == a.shooter && a.ticks_in_air < 5 {
            continue;
        }
        let bb = t.bb.grow(0.300_000_011_920_928_96, 0.300_000_011_920_928_96, 0.300_000_011_920_928_96);
        if let Some((hit, _)) = calculate_intercept(&bb, start, end) {
            let d1 = start.distance_squared(hit);
            if d1 < d0 || d0 == 0.0 {
                best = Some((t.id, t.player));
                d0 = d1;
            }
        }
    }
    best
}

/// `DamageSource.causeArrowDamage(arrow, shooter)`: the shooter is the attacker, else the arrow.
fn arrow_source(game: &Game, a: &Arrow) -> DamageSource {
    let attacker = match a.shooter {
        Some(id) => {
            if let Some(m) = game.mobs.mobs.iter().find(|m| m.id == id) {
                Some(combat::mob_attacker(m))
            } else if let Some(s) = game.sessions.get(&id) {
                let pos = s.state.map(|st| st.pos).unwrap_or([0.0; 3]);
                Some(Attacker { name: s.name.clone(), pos, player: true, id: Some(id) })
            } else {
                None
            }
        }
        None => None,
    };
    let attacker = attacker.unwrap_or(Attacker { name: "Arrow".into(), pos: [a.body.pos.x, a.body.pos.y, a.body.pos.z], player: false, id: None });
    DamageSource::arrow(Some(attacker))
}

/// `EntityArrow.onHit` for an entity.
fn on_hit_entity(game: &mut Game, a: &mut Arrow, target: u32, is_player: bool) {
    let f = a.speed();
    let mut i = ceil(f * a.damage);
    if a.critical {
        i += game.mobs.rand.next_int_bounded(i / 2 + 2);
    }
    let src = arrow_source(game, a);
    // isBurning: entity.setFire(5), before the damage.
    if a.is_burning() {
        if is_player {
            players::set_fire(game, target, 5);
        } else if let Some(m) = game.mobs.mobs.iter_mut().find(|m| m.id == target) {
            m.set_fire(5);
        }
    }
    let landed = if is_player { damage::attack_entity_from(game, target, &src, i as f32) } else { combat::hurt_mob(game, target, &src, i as f32) };
    if landed {
        // knockbackStrength > 0 (Punch): addVelocity on a mob. DROPPED for a player (the server
        // keeps no motion for one), thorns and arrowHit.
        if a.knockback > 0 && !is_player {
            let m = a.body.motion;
            let f1 = (m.x * m.x + m.z * m.z).sqrt() as f32;
            if f1 > 0.0 {
                if let Some(t) = game.mobs.mobs.iter_mut().find(|t| t.id == target) {
                    let k = a.knockback as f64 * 0.600_000_023_841_857_9 / f1 as f64;
                    t.body.motion.x += m.x * k;
                    t.body.motion.y += 0.1;
                    t.body.motion.z += m.z * k;
                    t.velocity_changed = true;
                }
            }
        }
        arrow_hit_sound(game, a);
        a.dead = true;
    } else {
        let m = &mut a.body.motion;
        *m *= -0.100_000_001_490_116_12;
        a.body.yaw += 180.0;
        a.body.prev_yaw += 180.0;
        a.ticks_in_air = 0;
        if a.body.motion.length_squared() < 0.001_000_000_047_497_493_6 {
            if a.pickup == Pickup::Allowed {
                let p = a.body.pos;
                game_items::spawn_stack_at(game, [p.x, p.y + 0.1, p.z], ItemStack::of(item::ARROW), None, sim::item_entity::DEFAULT_PICKUP_DELAY);
            }
            a.dead = true;
        }
    }
}

/// `EntityArrow.onHit` for a block.
fn on_hit_block(game: &mut Game, a: &mut Arrow, hit: sim::raytrace::Hit) {
    a.tile = hit.pos;
    a.in_tile = Blocks(game).state(hit.pos.0, hit.pos.1, hit.pos.2);
    let p = a.body.pos;
    a.body.motion = DVec3::new((hit.vec.x - p.x) as f32 as f64, (hit.vec.y - p.y) as f32 as f64, (hit.vec.z - p.z) as f32 as f64);
    let m = a.body.motion;
    let f2 = (m.x * m.x + m.y * m.y + m.z * m.z).sqrt() as f32 as f64;
    a.body.pos -= m / f2 * 0.050_000_000_745_058_06;
    a.in_ground = true;
    a.arrow_shake = 7;
    a.critical = false;
    arrow_hit_sound(game, a);
}

/// `playSound(ENTITY_ARROW_HIT, 1.0F, 1.2F / (rand.nextFloat() * 0.2F + 0.9F))`; an arrow is NEUTRAL.
fn arrow_hit_sound(game: &mut Game, a: &Arrow) {
    let pitch = 1.2 / (game.mobs.rand.next_float() * 0.2 + 0.9);
    let p = a.body.pos;
    super::sounds::play_sound(game, [p.x, p.y, p.z], "entity.arrow.hit", super::sounds::NEUTRAL, 1.0, pitch);
}

/// `EntityArrow.onUpdate`
fn update(game: &mut Game, a: &mut Arrow) {
    // Entity.onUpdate: the previous position and angles, water, the fire counter, the void.
    let b = &mut a.body;
    b.prev_pos = b.pos;
    b.prev_pitch = b.pitch;
    b.prev_yaw = b.yaw;
    b.handle_water_movement(&Blocks(game));
    b.sounds.clear();
    if b.in_water {
        a.fire = 0;
    } else if a.fire > 0 {
        a.fire -= 1;
    }
    if a.body.pos.y < -64.0 {
        a.dead = true;
        return;
    }
    if a.body.prev_pitch == 0.0 && a.body.prev_yaw == 0.0 {
        let m = a.body.motion;
        let f = (m.x * m.x + m.z * m.z).sqrt() as f32;
        a.body.yaw = (atan2(m.x, m.z) * 57.295_779_513_082_32) as f32;
        a.body.pitch = (atan2(m.y, f as f64) * 57.295_779_513_082_32) as f32;
        a.body.prev_yaw = a.body.yaw;
        a.body.prev_pitch = a.body.pitch;
    }
    // The block the arrow is stuck in still holds it.
    let (tx, ty, tz) = a.tile;
    let s = Blocks(game).state(tx, ty, tz);
    if s != 0 {
        if let Some(bb) = collision_box(s) {
            let o = DVec3::new(tx as f64, ty as f64, tz as f64);
            let p = a.body.pos - o;
            if p.x >= bb.min_x && p.x <= bb.max_x && p.y >= bb.min_y && p.y <= bb.max_y && p.z >= bb.min_z && p.z <= bb.max_z {
                a.in_ground = true;
            }
        }
    }
    if a.arrow_shake > 0 {
        a.arrow_shake -= 1;
    }
    if a.in_ground {
        if s != a.in_tile && !collides_with_any_block(&Blocks(game), &a.body.bb.grow(0.05, 0.05, 0.05)) {
            a.in_ground = false;
            a.body.motion.x *= (game.mobs.rand.next_float() * 0.2) as f64;
            a.body.motion.y *= (game.mobs.rand.next_float() * 0.2) as f64;
            a.body.motion.z *= (game.mobs.rand.next_float() * 0.2) as f64;
            a.ticks_in_ground = 0;
            a.ticks_in_air = 0;
        } else {
            a.ticks_in_ground += 1;
            if a.ticks_in_ground >= 1200 {
                a.dead = true;
            }
        }
        return;
    }
    a.ticks_in_air += 1;
    let from = a.body.pos;
    let mut to = from + a.body.motion;
    let mut block_hit = ray_trace_blocks(&Solid(&Blocks(game)), from, to, false, false);
    if let Some(h) = block_hit {
        to = h.vec;
    }
    let mut entity_hit = find_entity_on_path(game, a, from, to);
    if let Some((id, true)) = entity_hit {
        // canAttackPlayer: a player's arrow does not hurt another player with pvp off.
        let shooter_is_player = a.shooter.is_some_and(|s| game.sessions.contains_key(&s));
        if shooter_is_player && !game.cfg.pvp && a.shooter != Some(id) {
            entity_hit = None;
            block_hit = None;
        }
    }
    if let Some((id, is_player)) = entity_hit {
        on_hit_entity(game, a, id, is_player);
    } else if let Some(h) = block_hit {
        on_hit_block(game, a, h);
    }
    let m = a.body.motion;
    a.body.pos += m;
    let f4 = (m.x * m.x + m.z * m.z).sqrt() as f32;
    let m = a.body.motion;
    a.body.yaw = (atan2(m.x, m.z) * 57.295_779_513_082_32) as f32;
    a.body.pitch = (atan2(m.y, f4 as f64) * 57.295_779_513_082_32) as f32;
    while a.body.pitch - a.body.prev_pitch < -180.0 {
        a.body.prev_pitch -= 360.0;
    }
    while a.body.pitch - a.body.prev_pitch >= 180.0 {
        a.body.prev_pitch += 360.0;
    }
    while a.body.yaw - a.body.prev_yaw < -180.0 {
        a.body.prev_yaw -= 360.0;
    }
    while a.body.yaw - a.body.prev_yaw >= 180.0 {
        a.body.prev_yaw += 360.0;
    }
    a.body.pitch = a.body.prev_pitch + (a.body.pitch - a.body.prev_pitch) * 0.2;
    a.body.yaw = a.body.prev_yaw + (a.body.yaw - a.body.prev_yaw) * 0.2;
    let mut f1 = 0.99f32;
    let f2 = 0.05f32;
    if a.body.in_water {
        f1 = 0.6;
    }
    a.body.motion *= f1 as f64;
    a.body.motion.y -= f2 as f64;
    a.body.set_position(a.body.pos);
}

/// `EntityArrow.onCollideWithPlayer` for every player whose pickup box touches a stuck arrow.
fn collide_with_players(game: &mut Game, a: &mut Arrow) {
    if !a.in_ground || a.arrow_shake > 0 || a.dead {
        return;
    }
    let mut ids: Vec<u32> = game.sessions.keys().copied().collect();
    ids.sort_unstable();
    for pid in ids {
        let Some(s) = game.sessions.get(&pid) else { continue };
        let Some(st) = s.state else { continue };
        let mode = s.mode;
        if mode == proto::GM_SPECTATOR {
            continue;
        }
        let creative = mode == proto::GM_CREATIVE;
        let Some(p) = game.players.players.get_mut(&pid) else { continue };
        if p.dead || p.health <= 0.0 || !a.body.bb.intersects(&game_items::pickup_box(st.pos, st.flags)) {
            continue;
        }
        let mut flag = a.pickup == Pickup::Allowed || (a.pickup == Pickup::CreativeOnly && creative);
        if a.pickup == Pickup::Allowed {
            let mut stack = ItemStack::of(item::ARROW);
            if !p.inv.add_item_stack_to_inventory(&mut stack, creative) {
                flag = false;
            }
        }
        if flag {
            let msg = proto::Collect { item: a.id, collector: pid, count: 1 }.encode();
            tracker::send_to_tracking(game, a.id, proto::S_COLLECT, &msg);
            net::send(game, pid, proto::S_COLLECT, &msg);
            players::send_slots(game, pid);
            a.dead = true;
            return;
        }
    }
}

/// `World.updateEntities` for the arrows: each one in a loaded chunk ticks, the dead go.
pub fn tick(game: &mut Game) {
    let mut arrows = std::mem::take(&mut game.mobs.arrows);
    for a in &mut arrows {
        if a.dead || !Blocks(game).loaded(floor(a.body.pos.x), floor(a.body.pos.z)) {
            continue;
        }
        update(game, a);
        collide_with_players(game, a);
    }
    arrows.retain(|a| !a.dead);
    // Arrows shot during this tick's hits (none) would have been pushed to the live list.
    arrows.append(&mut game.mobs.arrows);
    game.mobs.arrows = arrows;
}

/// What the tracker reads off one arrow.
pub struct ArrowView<'a>(pub &'a Arrow);

pub fn views(arrows: &[Arrow]) -> Vec<ArrowView<'_>> {
    arrows.iter().filter(|a| !a.dead).map(ArrowView).collect()
}

impl Tracked for ArrowView<'_> {
    fn id(&self) -> u32 {
        self.0.id
    }
    fn kind(&self) -> &'static Kind {
        &ARROW
    }
    fn pos(&self) -> [f64; 3] {
        let p = self.0.body.pos;
        [p.x, p.y, p.z]
    }
    fn rotation(&self) -> [f32; 3] {
        [self.0.body.yaw, self.0.body.pitch, 0.0]
    }
    fn on_ground(&self) -> bool {
        self.0.in_ground
    }
    fn motion(&self) -> [f64; 3] {
        let m = self.0.body.motion;
        [m.x, m.y, m.z]
    }
    fn spawn_body(&self) -> Vec<u8> {
        let (b, m) = (&self.0.body, self.0.body.motion);
        proto::SpawnObject {
            id: self.0.id,
            pos: [b.pos.x, b.pos.y, b.pos.z],
            yaw: b.yaw,
            pitch: b.pitch,
            motion: [m.x, m.y, m.z].map(|v| v.clamp(-3.9, 3.9) as f32),
            // SPacketSpawnObject: the shooter's id, else the arrow's own.
            data: proto::ObjectData::Arrow { shooter: self.0.shooter.unwrap_or(self.0.id) },
        }
        .encode()
    }
}

/// The skeleton's shot: `AbstractSkeleton.attackEntityWithRangedAttack`'s arrow, built in
/// `skeleton.rs` and handed over as an [`super::ai::Effect::Arrow`]; this gives it an id.
pub fn spawn(game: &mut Game, mut a: Arrow) -> u32 {
    a.id = net::new_entity_id(game);
    let id = a.id;
    game.mobs.arrows.push(a);
    id
}
