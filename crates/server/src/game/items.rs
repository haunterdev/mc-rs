//! Item entities (`EntityItem`) on the server: spawn, the per-tick physics, merging, the age 6000
//! despawn, pickup into `sim::inventory`, the void, `V_DROP`, save and load, and the `Tracked`
//! view the entity tracker reads (item range 64, interval 20, velocity on, from `sim::kinds`).
//!
//! The entities live in two parallel lists, `entities` and `meta` (the id and the stack the
//! viewers were last told), so `sim::item_entity::search_for_other_items_nearby` can work on a
//! plain slice. Dead entities are removed at the end of the tick and the tracker sends their
//! `S_DESTROY` when it next runs.

use items::ItemStack;
use proto::{DropItem, SNEAKING};
use serde_json::{json, Value};
use sim::collision::{floor, Aabb, BlockAccess};
use sim::item_entity::{search_for_other_items_nearby, ItemEntity, ItemSound};
use sim::kinds::{Kind, ITEM};
use worldgen::java_random::JavaRandom;

use super::terrain::{self, Blocks};
use super::tracker::Tracked;
use super::{net, playerdata, players, Game};

pub struct State {
    /// Every item entity, in spawn order (`World.loadedEntityList` order for items).
    pub entities: Vec<ItemEntity>,
    /// Beside `entities`: the entity id and the stack the tracking viewers last had.
    pub meta: Vec<(u32, ItemStack)>,
    /// `World.rand`, for `Block.spawnAsEntity` and the seeds of each entity's own `rand`.
    pub rand: JavaRandom,
}

impl Default for State {
    fn default() -> State {
        State { entities: Vec::new(), meta: Vec::new(), rand: JavaRandom::new(crate::config::random_u64() as i64) }
    }
}

impl State {
    /// The id of the item entity at `index`.
    pub fn id_at(&self, index: usize) -> u32 {
        self.meta[index].0
    }
}

/// The wire form of a stack (`WireStack`), with the reduced tag as its three bytes and the
/// enchantment list.
pub fn wire_stack(s: &ItemStack) -> proto::WireStack {
    let tag = match s.tag {
        items::Tag::None => [0, 0, 0],
        items::Tag::Potion(p) => [1, p, 0],
        items::Tag::Entity(e) => [3, e, 0],
    };
    proto::WireStack { id: s.item as i16, count: s.count.clamp(0, 255) as u8, meta: s.damage.clamp(0, 65535) as u16, tag, ench: s.ench.to_bytes() }
}

/// `World.spawnEntity` for an item entity: gives it an id and keeps it. The tracker sends
/// `S_SPAWN_OBJECT` to the players in range on its next run. Returns the id.
pub fn spawn_stack(game: &mut Game, entity: ItemEntity) -> u32 {
    let id = net::new_entity_id(game);
    game.items.meta.push((id, entity.stack));
    game.items.entities.push(entity);
    game.stats.items = game.items.entities.len();
    id
}

/// The interface other tasks use: a stack at `pos`, with `motion` (`None` keeps the constructor's
/// random one) and a pickup delay in ticks.
pub fn spawn_stack_at(game: &mut Game, pos: [f64; 3], stack: ItemStack, motion: Option<[f64; 3]>, pickup_delay: i32) -> u32 {
    let seed = game.items.rand.next_long();
    let mut e = ItemEntity::new(pos[0], pos[1], pos[2], stack, JavaRandom::new(seed));
    if let Some(m) = motion {
        (e.body.motion.x, e.body.motion.y, e.body.motion.z) = (m[0], m[1], m[2]);
    }
    e.set_pickup_delay(pickup_delay);
    spawn_stack(game, e)
}

/// `Block.spawnAsEntity(world, pos, stack)`: inside the block at a random offset, the default
/// pickup delay, only while `doTileDrops` is on.
pub fn spawn_as_entity(game: &mut Game, at: (i32, i32, i32), stack: ItemStack) -> Option<u32> {
    if stack.is_empty() || !terrain::rule(game, "doTileDrops", true) {
        return None;
    }
    let r = &mut game.items.rand;
    let d0 = (r.next_float() * 0.5) as f64 + 0.25;
    let d1 = (r.next_float() * 0.5) as f64 + 0.25;
    let d2 = (r.next_float() * 0.5) as f64 + 0.25;
    let seed = r.next_long();
    let mut e = ItemEntity::new(at.0 as f64 + d0, at.1 as f64 + d1, at.2 as f64 + d2, stack, JavaRandom::new(seed));
    e.set_default_pickup_delay();
    Some(spawn_stack(game, e))
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

/// `V_DROP`: `NetHandlerPlayServer.processPlayerDigging` DROP_ITEM and DROP_ALL_ITEMS, then
/// `EntityPlayer.dropItem(boolean)`.
pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    if tag != proto::V_DROP {
        return false;
    }
    let Some(msg) = DropItem::decode(body) else { return true };
    if players::mode(game, id) == proto::GM_SPECTATOR {
        return true;
    }
    let Some(p) = game.players.players.get_mut(&id) else { return true };
    if p.dead {
        return true;
    }
    let held = p.inv.get_current_item();
    let count = if msg.all && !held.is_empty() { held.count } else { 1 };
    let cur = p.inv.current_item;
    let stack = p.inv.decr_stack_size(cur, count);
    if stack.is_empty() {
        return true;
    }
    players::drop_item(game, id, stack, false, true);
    players::send_slots(game, id);
    true
}

/// The box `EntityPlayer.onLivingUpdate` collects in: the player's box grown by 1, 0.5, 1.
pub(super) fn pickup_box(pos: [f64; 3], flags: u8) -> Aabb {
    let h = if flags & SNEAKING != 0 { 1.65 } else { 1.8 };
    Aabb::new(pos[0] - 0.3, pos[1], pos[2] - 0.3, pos[0] + 0.3, pos[1] + h, pos[2] + 0.3).grow(1.0, 0.5, 1.0)
}

/// Every player picks up what touches it (`EntityPlayer.onLivingUpdate`, `EntityItem.
/// onCollideWithPlayer`): the stack goes into `sim::inventory`, `S_COLLECT` to the viewers
/// tracking the item and to the collector, `S_SLOTS` to the collector.
fn collide_with_players(game: &mut Game) {
    let mut ids: Vec<u32> = game.sessions.keys().copied().collect();
    ids.sort();
    for pid in ids {
        let Some(s) = game.sessions.get(&pid) else { continue };
        let Some(st) = s.state else { continue };
        let (name, mode) = (s.name.clone(), s.mode);
        if mode == proto::GM_SPECTATOR {
            continue;
        }
        let Some(p) = game.players.players.get_mut(&pid) else { continue };
        if p.dead || p.health <= 0.0 {
            continue;
        }
        let area = pickup_box(st.pos, st.flags);
        let creative = mode == proto::GM_CREATIVE;
        let mut taken: Vec<(u32, i32)> = Vec::new();
        for (i, e) in game.items.entities.iter_mut().enumerate() {
            if e.dead || !e.body.bb.intersects(&area) {
                continue;
            }
            if let Some(n) = e.on_collide_with_player(&name, |s| p.inv.add_item_stack_to_inventory(s, creative)) {
                taken.push((game.items.meta[i].0, n));
            }
        }
        if taken.is_empty() {
            continue;
        }
        for (item, n) in taken {
            // EntityLivingBase.onItemPickup: SPacketCollectItem to the trackers, and the collector
            // (sendToTrackingAndSelf). The pickup sound is the viewer's, on this frame
            // (NetHandlerPlayClient.handleCollectItem).
            let msg = proto::Collect { item, collector: pid, count: n.clamp(0, 255) as u8 }.encode();
            super::tracker::send_to_tracking(game, item, proto::S_COLLECT, &msg);
            net::send(game, pid, proto::S_COLLECT, &msg);
        }
        // EntityPlayerMP.onItemPickup: openContainer.detectAndSendChanges.
        players::send_slots(game, pid);
    }
}

/// One tick of every item entity: `World.updateEntities` for `EntityItem`.
pub fn tick(game: &mut Game) {
    collide_with_players(game);
    let mut ents = std::mem::take(&mut game.items.entities);
    let mut sounds: Vec<([f64; 3], ItemSound)> = Vec::new();
    for i in 0..ents.len() {
        if ents[i].dead {
            continue;
        }
        let blocks = Blocks(game);
        let p = ents[i].body.pos;
        // Entities in unloaded chunks are not updated.
        if !blocks.loaded(floor(p.x), floor(p.z)) {
            continue;
        }
        let search = ents[i].tick(&blocks, false);
        let at = [ents[i].body.pos.x, ents[i].body.pos.y, ents[i].body.pos.z];
        for s in ents[i].sounds.drain(..) {
            sounds.push((at, s));
        }
        // DROPPED: the water splash sound of Entity.doWaterSplashEffect (Body::sounds).
        ents[i].body.sounds.clear();
        if search {
            search_for_other_items_nearby(&mut ents, i);
        }
    }
    game.items.entities = ents;
    for (pos, s) in sounds {
        let ItemSound::Burn(pitch) = s;
        let sound = proto::Sound { name: "entity.generic.burn".into(), category: 6, pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32], volume: 0.4, pitch };
        net::send_near(game, None, pos, 16.0, proto::S_SOUND, &sound.encode());
    }
    // DataManager: a changed stack (a merge, a partial pickup) goes to the viewers tracking it.
    for i in 0..game.items.entities.len() {
        let (id, sent) = game.items.meta[i];
        let now = game.items.entities[i].stack;
        if now != sent && !game.items.entities[i].dead {
            game.items.meta[i].1 = now;
            let msg = proto::EntityMeta { id, payload: proto::MetaPayload::Item(wire_stack(&now)) };
            super::tracker::send_to_tracking(game, id, proto::S_ENTITY_META, &msg.encode());
        }
    }
    // World.updateEntities removes the dead.
    let mut keep = game.items.entities.iter().map(|e| !e.dead);
    game.items.meta.retain(|_| keep.next().unwrap_or(true));
    game.items.entities.retain(|e| !e.dead);
    game.stats.items = game.items.entities.len();
}

/// What the tracker reads off one item entity.
pub struct View<'a> {
    pub id: u32,
    pub e: &'a ItemEntity,
}

pub fn views(state: &State) -> Vec<View<'_>> {
    state.entities.iter().enumerate().map(|(i, e)| View { id: state.meta[i].0, e }).collect()
}

impl Tracked for View<'_> {
    fn id(&self) -> u32 {
        self.id
    }
    fn kind(&self) -> &'static Kind {
        &ITEM
    }
    fn pos(&self) -> [f64; 3] {
        let p = self.e.body.pos;
        [p.x, p.y, p.z]
    }
    fn rotation(&self) -> [f32; 3] {
        [self.e.body.yaw, self.e.body.pitch, 0.0]
    }
    fn on_ground(&self) -> bool {
        self.e.body.on_ground
    }
    fn motion(&self) -> [f64; 3] {
        let m = self.e.body.motion;
        [m.x, m.y, m.z]
    }
    fn spawn_body(&self) -> Vec<u8> {
        let p = self.e.body.pos;
        let m = self.e.body.motion;
        proto::SpawnObject {
            id: self.id,
            pos: [p.x, p.y, p.z],
            yaw: self.e.body.yaw,
            pitch: self.e.body.pitch,
            motion: [m.x as f32, m.y as f32, m.z as f32],
            data: proto::ObjectData::Item(wire_stack(&self.e.stack)),
        }
        .encode()
    }
    fn air_borne(&self) -> bool {
        self.e.is_air_borne
    }
}

/// The tracker has run: `Entity.isAirBorne` is cleared for every item.
pub fn after_tracker(game: &mut Game) {
    for e in &mut game.items.entities {
        e.is_air_borne = false;
    }
}

/// `items.json`: every item entity (`EntityItem.writeEntityToNBT`: position, motion, Age, Health,
/// PickupDelay, Owner, Thrower, Item). Entity ids are not saved; they are handed out again on load.
pub fn save(game: &Game) -> Value {
    let list: Vec<Value> = game
        .items
        .entities
        .iter()
        .filter(|e| !e.dead)
        .filter_map(|e| {
            let stack = playerdata::stack_json(0, &e.stack)?;
            let (p, m) = (e.body.pos, e.body.motion);
            Some(json!({
                "pos": [p.x, p.y, p.z],
                "motion": [m.x, m.y, m.z],
                "yaw": e.body.yaw,
                "age": e.age,
                "health": e.health,
                "pickup_delay": e.pickup_delay,
                "owner": e.owner,
                "thrower": e.thrower,
                "item": stack,
            }))
        })
        .collect();
    json!({"version": 1, "items": list})
}

pub fn load(game: &mut Game, v: Value) {
    let Some(list) = v.get("items").and_then(Value::as_array) else { return };
    for it in list {
        let Some((_, stack)) = it.get("item").and_then(playerdata::stack_from) else { continue };
        let f = |key: &str, i: usize| it.get(key).and_then(Value::as_array).and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(0.0);
        let seed = game.items.rand.next_long();
        let mut e = ItemEntity::new(f("pos", 0), f("pos", 1), f("pos", 2), stack, JavaRandom::new(seed));
        (e.body.motion.x, e.body.motion.y, e.body.motion.z) = (f("motion", 0), f("motion", 1), f("motion", 2));
        e.body.yaw = it.get("yaw").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        e.body.prev_yaw = e.body.yaw;
        let int = |key: &str, d: i64| it.get(key).and_then(Value::as_i64).unwrap_or(d) as i32;
        e.age = int("age", 0);
        e.health = int("health", 5);
        e.pickup_delay = int("pickup_delay", 0);
        e.owner = it.get("owner").and_then(Value::as_str).map(String::from);
        e.thrower = it.get("thrower").and_then(Value::as_str).map(String::from);
        spawn_stack(game, e);
    }
}

#[cfg(test)]
#[path = "items_tests.rs"]
mod tests;
