//! `EntityTracker` and `EntityTrackerEntry`: who sees which server entity, by the range and update
//! interval `sim::kinds` gives each kind, and the frames that keep their copies current:
//! `S_SPAWN_OBJECT` or `S_SPAWN_MOB` on entering range, one `S_ENTITY_MOVE` batch per player per
//! tick, `S_VELOCITY` for the kinds that send velocity, and `S_DESTROY` on leaving range or
//! removal.
//!
//! Items and mobs implement [`Tracked`] and hand the tracker their entities each tick; an entity
//! missing from the list has been removed. Players are not tracked here: the hub relays player
//! states to the viewers.
//!
//! One frame type stands for vanilla's four movement packets: `S_ENTITY_MOVE` carries the
//! position and rotation the client ends up with after `S15PacketEntityRelMove`,
//! `S16PacketEntityLook`, `S17PacketEntityLookMove`, `SPacketEntityTeleport` or
//! `SPacketEntityHeadLook`, so the decisions (when, which, the 128 and 400 thresholds) stay
//! vanilla's.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use proto::EntityMove;
use serde_json::Value;
use sim::kinds::Kind;

use super::{net, Game};

/// What the tracker reads off an entity.
pub trait Tracked {
    fn id(&self) -> u32;
    fn kind(&self) -> &'static Kind;
    /// `posX, posY, posZ`.
    fn pos(&self) -> [f64; 3];
    /// `rotationYaw, rotationPitch, getRotationYawHead()` in degrees.
    fn rotation(&self) -> [f32; 3];
    fn on_ground(&self) -> bool;
    /// `motionX, motionY, motionZ`.
    fn motion(&self) -> [f64; 3];
    /// The spawn frame's body: `proto::SpawnObject` or `proto::SpawnMob`, as [`spawn_tag`] picks.
    fn spawn_body(&self) -> Vec<u8>;
    /// `Entity.isAirBorne`: moved by something other than its own motion (a knockback), so the
    /// next tick sends whatever the interval says. The owner clears it after the tracker's tick.
    fn air_borne(&self) -> bool {
        false
    }
    /// `Entity.velocityChanged`: a hit set its motion; it goes out at once. The owner clears it
    /// after the tracker's tick.
    fn velocity_changed(&self) -> bool {
        false
    }
}

/// `EntityTrackerEntry.createSpawnPacket`: `SPacketSpawnMob` for an `IAnimals` (every mob in
/// `sim::kinds`), `SPacketSpawnObject` for items, arrows and falling blocks.
pub fn spawn_tag(kind: &Kind) -> u8 {
    match kind.name {
        "item" | "arrow" | "falling_block" => proto::S_SPAWN_OBJECT,
        _ => proto::S_SPAWN_MOB,
    }
}

/// `EntityTracker.getPositionLong`: `MathHelper.lfloor(value * 4096.0D)`.
fn position_long(v: f64) -> i64 {
    (v * 4096.0).floor() as i64
}

/// `MathHelper.floor(rotation * 256.0F / 360.0F)`.
fn angle(r: f32) -> i32 {
    (r * 256.0 / 360.0).floor() as i32
}

/// The byte angle back to degrees.
fn degrees(a: i32) -> f32 {
    a as f32 * 360.0 / 256.0
}

fn dist_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

/// `SPacketEntityVelocity`: motion clamped to 3.9 a tick.
fn velocity(id: u32, m: [f64; 3]) -> (u8, Vec<u8>) {
    let c = |v: f64| v.clamp(-3.9, 3.9) as f32;
    (proto::S_VELOCITY, proto::Velocity { id, motion: [c(m[0]), c(m[1]), c(m[2])] }.encode())
}

/// One player's frames this tick: in order, then the moves as one batch, then the destroys.
#[derive(Default)]
struct Out {
    frames: Vec<(u8, Vec<u8>)>,
    moves: Vec<EntityMove>,
    destroy: Vec<u32>,
}

/// `EntityTrackerEntry`.
struct Entry {
    kind: &'static Kind,
    /// `min(range, maxRange)`: `EntityTracker.track` clamps the kind's range to the view distance.
    range: i32,
    encoded: [i64; 3],
    yaw: i32,
    pitch: i32,
    head: i32,
    last_motion: [f64; 3],
    counter: i32,
    last_tracked: [f64; 3],
    updated_visibility: bool,
    since_teleport: i32,
    on_ground: bool,
    trackers: BTreeSet<u32>,
}

/// A player's own entry, for the one thing it does here: after moving more than 4 blocks it has
/// every entity check it again (`EntityTracker.tick`'s second loop).
struct Watcher {
    pos: [f64; 3],
    last_tracked: [f64; 3],
    updated: bool,
}

pub struct EntityTracker {
    entries: BTreeMap<u32, Entry>,
    players: BTreeMap<u32, Watcher>,
    /// `PlayerList.getEntityViewDistance`: `(viewDistance - 1) * 16` blocks.
    max_range: i32,
    /// The view distance in chunks, for `isPlayerWatchingThisChunk`.
    view: i32,
}

impl EntityTracker {
    /// The server's view distance is `sim_distance`: the chunks a player's surroundings are
    /// simulated in.
    pub fn new(view_distance: i32) -> EntityTracker {
        EntityTracker { entries: BTreeMap::new(), players: BTreeMap::new(), max_range: (view_distance - 1) * 16, view: view_distance }
    }

    /// The players tracking entity `id`.
    pub fn tracking(&self, id: u32) -> Vec<u32> {
        self.entries.get(&id).map(|e| e.trackers.iter().copied().collect()).unwrap_or_default()
    }

    /// `EntityTracker.removePlayerFromTrackers`: the player left; nothing is sent to it.
    pub fn remove_player(&mut self, id: u32) {
        self.players.remove(&id);
        for e in self.entries.values_mut() {
            e.trackers.remove(&id);
        }
    }

    /// One tick of `EntityTracker.tick` over the entities alive now and the players with a
    /// position. Entities new to the list are tracked first (`EntityTracker.track`), the ones gone
    /// from it untracked (`untrack`); returns each player's frames, in order.
    pub fn tick(&mut self, entities: &[&dyn Tracked], players: &[(u32, [f64; 3])]) -> BTreeMap<u32, Vec<(u8, Vec<u8>)>> {
        let mut outs: BTreeMap<u32, Out> = BTreeMap::new();
        let gone: Vec<u32> = self.players.keys().copied().filter(|id| !players.iter().any(|p| p.0 == *id)).collect();
        for id in gone {
            self.remove_player(id);
        }
        // The players' own entries run in the same loop in vanilla; their visibility pass only
        // decides who checks every entity again below.
        let mut moved_players = Vec::new();
        for &(id, pos) in players {
            let w = self.players.entry(id).or_insert(Watcher { pos, last_tracked: pos, updated: false });
            w.pos = pos;
            if !w.updated || dist_sq(pos, w.last_tracked) > 16.0 {
                w.last_tracked = pos;
                w.updated = true;
                moved_players.push(id);
            }
            // DROPPED: resetPlayerVisibility on a player's teleport; the hub moves players.
        }
        let alive: HashMap<u32, &dyn Tracked> = entities.iter().map(|&e| (e.id(), e)).collect();
        // untrack: the destroy goes to everyone tracking it.
        let removed: Vec<u32> = self.entries.keys().copied().filter(|id| !alive.contains_key(id)).collect();
        for id in removed {
            if let Some(e) = self.entries.remove(&id) {
                for p in e.trackers {
                    outs.entry(p).or_default().destroy.push(id);
                }
            }
        }
        // track: a new entry checks every player at once.
        for &ent in entities {
            if self.entries.contains_key(&ent.id()) {
                continue;
            }
            let kind = ent.kind();
            let (pos, rot) = (ent.pos(), ent.rotation());
            let mut e = Entry {
                kind,
                range: kind.tracker_range.min(self.max_range),
                encoded: pos.map(position_long),
                yaw: angle(rot[0]),
                pitch: angle(rot[1]),
                head: angle(rot[2]),
                last_motion: [0.0; 3],
                counter: 0,
                last_tracked: [0.0; 3],
                updated_visibility: false,
                since_teleport: 0,
                on_ground: ent.on_ground(),
                trackers: BTreeSet::new(),
            };
            for (&pid, w) in &self.players {
                update_player_entity(&mut e, ent, pid, w.pos, self.view, &mut outs);
            }
            self.entries.insert(ent.id(), e);
        }
        for (&id, e) in &mut self.entries {
            update_player_list(e, alive[&id], &self.players, self.view, &mut outs);
        }
        for pid in moved_players {
            let pos = self.players[&pid].pos;
            for (&id, e) in &mut self.entries {
                update_player_entity(e, alive[&id], pid, pos, self.view, &mut outs);
            }
        }
        outs.into_iter()
            .map(|(pid, mut o)| {
                if !o.moves.is_empty() {
                    o.frames.push((proto::S_ENTITY_MOVE, proto::encode_entity_moves(&o.moves)));
                }
                // EntityPlayerMP.onUpdate sends its entityRemoveQueue as one packet.
                if !o.destroy.is_empty() {
                    o.frames.push((proto::S_DESTROY, proto::encode_destroy(&o.destroy)));
                }
                (pid, o.frames)
            })
            .filter(|(_, f)| !f.is_empty())
            .collect()
    }
}

/// `EntityTrackerEntry.isVisibleTo`: a square of `min(range, maxRange)` around the position the
/// clients were last sent.
fn visible(e: &Entry, player: [f64; 3]) -> bool {
    let d0 = player[0] - e.encoded[0] as f64 / 4096.0;
    let d1 = player[2] - e.encoded[2] as f64 / 4096.0;
    let i = e.range as f64;
    // DROPPED: isSpectatedByPlayer (spectators who are not this entity's camera).
    d0 >= -i && d0 <= i && d1 >= -i && d1 <= i
}

/// `isPlayerWatchingThisChunk`: the entity's chunk is in the player's view square.
fn watching(ent: &dyn Tracked, player: [f64; 3], view: i32) -> bool {
    let p = ent.pos();
    let (cx, cz) = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
    let (px, pz) = ((player[0].floor() as i32) >> 4, (player[2].floor() as i32) >> 4);
    (cx - px).abs() <= view && (cz - pz).abs() <= view
}

/// `EntityTrackerEntry.updatePlayerEntity`.
fn update_player_entity(e: &mut Entry, ent: &dyn Tracked, pid: u32, player: [f64; 3], view: i32, outs: &mut BTreeMap<u32, Out>) {
    if pid == ent.id() {
        return;
    }
    if visible(e, player) {
        // DROPPED: forceSpawn (no tracked kind sets it).
        if !e.trackers.contains(&pid) && watching(ent, player, view) {
            e.trackers.insert(pid);
            let out = outs.entry(pid).or_default();
            let tag = spawn_tag(e.kind);
            if tag == proto::S_SPAWN_MOB {
                e.head = angle(ent.rotation()[2]);
            }
            out.frames.push((tag, ent.spawn_body()));
            // DROPPED: SPacketEntityMetadata and SPacketEntityProperties after the spawn, the
            // elytra velocity case, SPacketEntityEquipment, SPacketUseBed, SPacketEntityEffect and
            // SPacketSetPassengers: tasks that add metadata, equipment or riding send their own.
            e.last_motion = ent.motion();
            if e.kind.tracker_velocity && tag != proto::S_SPAWN_MOB {
                out.frames.push(velocity(ent.id(), ent.motion()));
            }
        }
    } else if e.trackers.remove(&pid) {
        outs.entry(pid).or_default().destroy.push(ent.id());
    }
}

/// `EntityTrackerEntry.updatePlayerList`, for an entity that rides nothing.
fn update_player_list(e: &mut Entry, ent: &dyn Tracked, players: &BTreeMap<u32, Watcher>, view: i32, outs: &mut BTreeMap<u32, Out>) {
    let id = ent.id();
    let pos = ent.pos();
    if !e.updated_visibility || dist_sq(pos, e.last_tracked) > 16.0 {
        e.last_tracked = pos;
        e.updated_visibility = true;
        for (&pid, w) in players {
            update_player_entity(e, ent, pid, w.pos, view, outs);
        }
    }
    // DROPPED: SPacketSetPassengers on a passenger change and the item frame's map data.
    // DROPPED: getDataManager().isDirty() as a reason to run: no metadata is tracked yet.
    if e.counter % e.kind.tracker_interval == 0 || ent.air_borne() {
        // DROPPED: the riding branch (S16PacketEntityLook only): nothing rides yet.
        e.since_teleport += 1;
        let now = pos.map(position_long);
        let rot = ent.rotation();
        let (yaw, pitch) = (angle(rot[0]), angle(rot[1]));
        let d = [now[0] - e.encoded[0], now[1] - e.encoded[1], now[2] - e.encoded[2]];
        let moved = d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= 128 || e.counter % 60 == 0;
        let turned = (yaw - e.yaw).abs() >= 1 || (pitch - e.pitch).abs() >= 1;
        let arrow = e.kind.name == "arrow";
        let at = |p: [i64; 3]| p.map(|v| v as f64 / 4096.0);
        let mv = |pos: [f64; 3], yaw: i32, pitch: i32| EntityMove { id, pos, yaw: degrees(yaw), pitch: degrees(pitch), head_yaw: 0.0, on_ground: ent.on_ground() };
        let mut packet = None;
        if e.counter > 0 || arrow {
            if d.iter().all(|v| (-32768..32768).contains(v)) && e.since_teleport <= 400 && e.on_ground == ent.on_ground() {
                if (!moved || !turned) && !arrow {
                    if moved {
                        // S15PacketEntityRelMove
                        packet = Some(mv(at(now), e.yaw, e.pitch));
                    } else if turned {
                        // S16PacketEntityLook
                        packet = Some(mv(at(e.encoded), yaw, pitch));
                    }
                } else {
                    // S17PacketEntityLookMove
                    packet = Some(mv(at(now), yaw, pitch));
                }
            } else {
                e.on_ground = ent.on_ground();
                e.since_teleport = 0;
                // resetPlayerVisibility
                e.updated_visibility = false;
                // SPacketEntityTeleport
                packet = Some(mv(pos, yaw, pitch));
            }
        }
        // DROPPED: isElytraFlying turning velocity on.
        if e.kind.tracker_velocity && e.counter > 0 {
            let m = ent.motion();
            let dm = [m[0] - e.last_motion[0], m[1] - e.last_motion[1], m[2] - e.last_motion[2]];
            let d4 = dm[0] * dm[0] + dm[1] * dm[1] + dm[2] * dm[2];
            if d4 > 4.0e-4 || d4 > 0.0 && m == [0.0; 3] {
                e.last_motion = m;
                let v = velocity(id, m);
                for &p in &e.trackers {
                    outs.entry(p).or_default().frames.push(v.clone());
                }
            }
        }
        // DROPPED: sendMetadata (SPacketEntityMetadata, SPacketEntityProperties).
        if moved {
            e.encoded = now;
        }
        if turned {
            e.yaw = yaw;
            e.pitch = pitch;
        }
        let head = angle(rot[2]);
        if (head - e.head).abs() >= 1 {
            // SPacketEntityHeadLook, folded into this tick's move.
            e.head = head;
            packet.get_or_insert_with(|| mv(at(e.encoded), e.yaw, e.pitch));
        }
        if let Some(mut m) = packet {
            m.head_yaw = degrees(e.head);
            for &p in &e.trackers {
                outs.entry(p).or_default().moves.push(m);
            }
        }
    }
    e.counter += 1;
    if ent.velocity_changed() {
        // sendToTrackingAndSelf: the entity is no player, so the trackers only.
        let v = velocity(id, ent.motion());
        for &p in &e.trackers {
            outs.entry(p).or_default().frames.push(v.clone());
        }
    }
}

#[derive(Default)]
pub struct State {
    pub tracker: Option<EntityTracker>,
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(game: &mut Game, id: u32) {
    if let Some(t) = &mut game.tracker.tracker {
        t.remove_player(id);
    }
}

pub fn on_frame(_game: &mut Game, _id: u32, _tag: u8, _body: &[u8]) -> bool {
    false
}

pub fn tick(game: &mut Game) {
    let players: Vec<(u32, [f64; 3])> = game.sessions.iter().filter_map(|(&id, s)| s.state.map(|st| (id, st.pos))).collect();
    // Items (T5) and mobs (T9a) put their entities here.
    let entities: Vec<&dyn Tracked> = Vec::new();
    let view = game.cfg.sim_distance;
    let tracker = game.tracker.tracker.get_or_insert_with(|| EntityTracker::new(view));
    let out = tracker.tick(&entities, &players);
    for (id, frames) in out {
        for (tag, body) in frames {
            net::send(game, id, tag, &body);
        }
    }
}

/// `EntityTracker.sendToTracking`: a frame about entity `entity` to every player tracking it.
pub fn send_to_tracking(game: &Game, entity: u32, tag: u8, body: &[u8]) {
    if let Some(t) = &game.tracker.tracker {
        for p in t.tracking(entity) {
            net::send(game, p, tag, body);
        }
    }
}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

#[cfg(test)]
#[path = "tracker_tests.rs"]
mod tests;
