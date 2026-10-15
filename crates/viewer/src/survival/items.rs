//! Dropped items in a server session (T5, viewer half): the item entities the server tracks, drawn
//! as `RenderEntityItem` draws them, moved between updates by `sim::item_entity` (the client
//! branch of `EntityItem.onUpdate`), flown into the collector by `ParticleItemPickup`, and the
//! drop keys that send `V_DROP` (`CPacketPlayerDigging` `DROP_ITEM` and `DROP_ALL_ITEMS`).
//!
//! Frames: `S_SPAWN_OBJECT` (`handleSpawnObject`, an `EntityItem` with the packet's motion),
//! `S_ENTITY_MOVE` (`handleEntityTeleport`: `setPosition`, `onGround`), `S_VELOCITY`
//! (`setVelocity`), `S_ENTITY_META` (the stack, `handleEntityMetadata`), `S_DESTROY`, `S_COLLECT`
//! (`handleCollectItem`: the pickup sound at the item, the count, the particle, the entity
//! removed) and `S_SLOTS` for the player inventory window. Native and streamed sessions alike.
//!
//! The draw is a pool of mesh entities on the item materials in their world mode (held.rs), one
//! per model of a stack, lit with the lightmap at the item as `Entity.getBrightnessForRender`.
//! DROPPED: the shadow blob (`shadowSize` 0.15), the outline mode, the `renderOutlines` team
//! colours, the NBT of a stack, `Entity.setPositionAndRotationDirect`'s increments (an
//! `Entity` snaps), and the frustum test (`isInRangeToRenderDist` stays: 16 blocks).

use std::collections::HashMap;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::prelude::*;
use items::{ItemStack, Tag};
use proto::{Collect, EntityMove, ObjectData, SpawnObject, Velocity};
use sim::item_entity::ItemEntity;
use worldgen::math_helper::sin;
use worldgen::JavaRandom;

use super::{Destroy, EntityMeta, EntityMoves, Held, Slots};
use super::{Collect as CollectMsg, SpawnObject as SpawnMsg, Velocity as VelocityMsg};
use crate::audio::{Audio, Category};
use crate::collision::{floor, BlockAccess};
use crate::hand::standard_lights;
use crate::hotbar::Inventory;
use crate::item::{model_key, Display};
use crate::item_render::{ItemMaterials, ItemMeshes};
use crate::link::Link;
use crate::material::ItemMaterial;
use crate::player::Player;
use crate::players::{Other, Others};
use crate::settings::Menu;
use crate::sky::{neighbor_light, Clock, View};
use crate::stream::Streamer;

pub fn plugin(app: &mut App) {
    app.init_resource::<Items>().add_systems(
        Update,
        (
            read_frames,
            tick.after(crate::player::run_ticks).run_if(in_world),
            collect_effects.after(read_frames),
            slots_in,
            held_sync.after(crate::player::run_ticks).run_if(resource_exists::<Link>),
            (drop_keys, swap_hands_keys).after(held_sync).run_if(resource_exists::<Link>),
            draw.after(crate::sky::advance_clock).after(read_frames).after(tick).run_if(in_world),
        ),
    );
    app.add_systems(Update, preview.after(crate::sky::advance_clock).run_if(in_world).run_if(|a: Option<Res<crate::Args>>| a.is_some_and(|a| a.items)));
}

/// The resources the world systems read; a bare plugin (the frame tests) has none of them.
fn in_world(clock: Option<Res<Clock>>, streamer: Option<Res<Streamer>>, view: Option<Res<View>>, player: Option<Res<Player>>) -> bool {
    clock.is_some() && streamer.is_some() && view.is_some() && player.is_some()
}

/// `RenderEntityItem.getModelCount`: how many copies of the model a stack draws.
pub fn model_count(count: i32) -> usize {
    if count > 48 {
        5
    } else if count > 32 {
        4
    } else if count > 16 {
        3
    } else if count > 1 {
        2
    } else {
        1
    }
}

/// The bob of `transformModelCount`: `MathHelper.sin((age + partial) / 10 + hoverStart) * 0.1 + 0.1`.
pub fn bob(age: i32, partial: f32, hover: f32) -> f32 {
    sin((age as f32 + partial) / 10.0 + hover) * 0.1 + 0.1
}

/// The spin of `transformModelCount`, in degrees: `((age + partial) / 20 + hoverStart) * 57.295776`.
pub fn spin_degrees(age: i32, partial: f32, hover: f32) -> f32 {
    ((age as f32 + partial) / 20.0 + hover) * 57.295_776
}

/// The matrix of each model copy `RenderEntityItem.doRender` draws for a stack, in world space,
/// at the interpolated position `at`. `seed` is `Item.getIdFromItem + getMetadata` (187 for an
/// empty stack), which seeds the offsets between the copies.
pub fn model_matrices(at: Vec3, ground: &Display, gui3d: bool, count: i32, seed: i64, age: i32, partial: f32, hover: f32) -> Vec<Mat4> {
    let j = model_count(count);
    let mut rand = JavaRandom::new(seed ^ 0x5DEECE66D);
    // transformModelCount: translate(x, y + bob + 0.25 * ground.scale.y, z), rotate(spin, y).
    let mut m = Mat4::from_translation(at + Vec3::new(0.0, bob(age, partial, hover) + 0.25 * ground.scale[1], 0.0))
        * Mat4::from_rotation_y(spin_degrees(age, partial, hover).to_radians());
    let [_, _, f2] = ground.scale;
    if !gui3d {
        // -0.0 * (j - 1) * 0.5 * scale for x and y, then the z spread.
        m *= Mat4::from_translation(Vec3::new(0.0, 0.0, -0.093_75 * (j - 1) as f32 * 0.5 * f2));
    }
    let centre = Mat4::from_translation(Vec3::splat(-0.5));
    let mut out = Vec::with_capacity(j);
    for k in 0..j {
        let mut piece = m;
        if k > 0 {
            if gui3d {
                let x = (rand.next_float() * 2.0 - 1.0) * 0.15;
                let y = (rand.next_float() * 2.0 - 1.0) * 0.15;
                let z = (rand.next_float() * 2.0 - 1.0) * 0.15;
                piece *= Mat4::from_translation(Vec3::new(x, y, z));
            } else {
                let x = (rand.next_float() * 2.0 - 1.0) * 0.15 * 0.5;
                let y = (rand.next_float() * 2.0 - 1.0) * 0.15 * 0.5;
                piece *= Mat4::from_translation(Vec3::new(x, y, 0.0));
            }
        }
        out.push(piece * ground.matrix() * centre);
        if !gui3d {
            // translate(0, 0, 0.09375 * scale.z) stays on the stack for the next copy.
            m *= Mat4::from_translation(Vec3::new(0.0, 0.0, 0.093_75 * f2));
        }
    }
    out
}

/// `ParticleItemPickup.renderParticle`'s position: the item's own spot moves to the target
/// (plus `yOffset` up) by `f * f`, `f = (age + partial) / 3`.
pub fn pickup_position(item: DVec3, target: DVec3, y_offset: f64, age: i32, partial: f32) -> DVec3 {
    let f = (age as f32 + partial) / PICKUP_AGE as f32;
    let f = (f * f) as f64;
    let target = target + DVec3::new(0.0, y_offset, 0.0);
    item + (target - item) * f
}

/// `ParticleItemPickup.maxAge`
const PICKUP_AGE: i32 = 3;
/// `handleCollectItem`'s `yOffset` argument.
const PICKUP_Y_OFFSET: f64 = 0.5;

/// What `V_DROP` the keys ask for: Q drops one, Ctrl+Q the whole stack. Nothing while a screen
/// has the keys, after death, or as a spectator (`Minecraft.processKeyBinds`).
pub fn drop_request(q_pressed: bool, ctrl: bool, screen_open: bool, dead: bool, game_mode: u8) -> Option<proto::DropItem> {
    (q_pressed && !screen_open && !dead && game_mode != proto::GM_SPECTATOR).then_some(proto::DropItem { all: ctrl })
}

/// What `V_SWAP_HANDS` the keys ask for: F swaps the main and off hands. Nothing while a screen has
/// the keys, after death, or as a spectator (`Minecraft.processKeyBinds`, which runs under the
/// `currentScreen == null || allowUserInput` guard and sends only when `!isSpectator`).
pub fn swap_request(f_pressed: bool, screen_open: bool, dead: bool, game_mode: u8) -> bool {
    f_pressed && !screen_open && !dead && game_mode != proto::GM_SPECTATOR
}

/// A wire stack as an `ItemStack`.
pub fn stack_of(w: &proto::WireStack) -> ItemStack {
    if w.id < 0 || w.count == 0 {
        return ItemStack::EMPTY;
    }
    let tag = match w.tag {
        [1, p, _] => Tag::Potion(p),
        [3, e, _] => Tag::Entity(e),
        _ => Tag::None,
    };
    ItemStack::new(w.id as u16, w.count as i32, w.meta as i32).with_tag(tag).with_ench(items::EnchList::from_bytes(&w.ench))
}

/// One item entity as this viewer knows it.
#[derive(Clone, Debug)]
pub struct Client {
    pub e: ItemEntity,
    /// `EntityItem.hoverStart`
    pub hover: f32,
}

impl Client {
    /// The interpolated feet, `lastTickPos` to `pos`.
    pub fn feet(&self, partial: f32) -> DVec3 {
        let b = &self.e.body;
        b.prev_pos + (b.pos - b.prev_pos) * partial as f64
    }
}

/// A `ParticleItemPickup` in flight.
#[derive(Clone, Debug)]
pub struct Pickup {
    pub item: Client,
    pub target: u32,
    pub age: i32,
}

/// The item entities of the session and the pickups flying.
#[derive(Resource)]
pub struct Items {
    pub map: HashMap<u32, Client>,
    pub pickups: Vec<Pickup>,
    /// Where `S_COLLECT` took an item this frame, for `collect_effects` to sound.
    pub collected: Vec<DVec3>,
    rand: JavaRandom,
    pool: Vec<Slot>,
}

impl Default for Items {
    fn default() -> Items {
        Items { map: HashMap::new(), pickups: Vec::new(), collected: Vec::new(), rand: JavaRandom::new(0x1F2E3D4C), pool: Vec::new() }
    }
}

struct Slot {
    entity: Entity,
    material: Handle<ItemMaterial>,
}

impl Items {
    /// `handleSpawnObject` for type 2: `new EntityItem(world, x, y, z)`, then the packet's
    /// motion (`setVelocity`). The stack arrives in the same frame here.
    pub fn spawn(&mut self, m: &SpawnObject) {
        let ObjectData::Item(w) = m.data else { return };
        let seed = self.rand.next_long();
        let mut e = ItemEntity::new(m.pos[0], m.pos[1], m.pos[2], stack_of(&w), JavaRandom::new(seed));
        e.body.motion = DVec3::new(m.motion[0] as f64, m.motion[1] as f64, m.motion[2] as f64);
        e.body.yaw = m.yaw;
        e.body.prev_yaw = m.yaw;
        let hover = (self.rand.next_double() * std::f64::consts::PI * 2.0) as f32;
        self.map.insert(m.id, Client { e, hover });
    }

    /// `handleEntityTeleport` for an `Entity`: position, `onGround`.
    pub fn moves(&mut self, moves: &[EntityMove]) {
        for m in moves {
            if let Some(c) = self.map.get_mut(&m.id) {
                c.e.body.set_position(DVec3::new(m.pos[0], m.pos[1], m.pos[2]));
                c.e.body.on_ground = m.on_ground;
            }
        }
    }

    /// `handleEntityVelocity`: `Entity.setVelocity`.
    pub fn velocity(&mut self, v: &Velocity) {
        if let Some(c) = self.map.get_mut(&v.id) {
            c.e.body.motion = DVec3::new(v.motion[0] as f64, v.motion[1] as f64, v.motion[2] as f64);
        }
    }

    /// `handleEntityMetadata`: the item entity's stack.
    pub fn meta(&mut self, id: u32, stack: ItemStack) {
        if let Some(c) = self.map.get_mut(&id) {
            c.e.stack = stack;
        }
    }

    /// `handleDestroyEntities`
    pub fn destroy(&mut self, ids: &[u32]) {
        for id in ids {
            self.map.remove(id);
        }
    }

    /// `handleCollectItem`: the count the stack shows, a `ParticleItemPickup` toward the
    /// collector, the entity removed. Returns where the item was, for the pickup sound.
    pub fn collect(&mut self, c: &Collect) -> Option<DVec3> {
        let mut item = self.map.remove(&c.item)?;
        item.e.stack.count = c.count as i32;
        let at = item.e.body.pos;
        self.pickups.push(Pickup { item, target: c.collector, age: 0 });
        Some(at)
    }

    /// One client tick: `EntityItem.onUpdate` (client branch) for every item in a loaded
    /// chunk, then `ParticleItemPickup.onUpdate`.
    pub fn tick(&mut self, w: &impl BlockAccess) {
        for c in self.map.values_mut() {
            let p = c.e.body.pos;
            if !w.loaded(floor(p.x), floor(p.z)) {
                continue;
            }
            c.e.tick(w, true);
            // The burn sound is the server's (`Entity.playSound` is silent on a client).
            c.e.sounds.clear();
        }
        for p in &mut self.pickups {
            p.age += 1;
        }
        self.pickups.retain(|p| p.age < PICKUP_AGE);
    }
}

fn read_frames(
    mut items: ResMut<Items>,
    mut spawns: MessageReader<SpawnMsg>,
    mut moves: MessageReader<EntityMoves>,
    mut velocity: MessageReader<VelocityMsg>,
    mut meta: MessageReader<EntityMeta>,
    mut collects: MessageReader<CollectMsg>,
    mut destroy: MessageReader<Destroy>,
) {
    for SpawnMsg(m) in spawns.read() {
        items.spawn(m);
    }
    for EntityMoves(m) in moves.read() {
        items.moves(m);
    }
    for VelocityMsg(v) in velocity.read() {
        items.velocity(v);
    }
    for EntityMeta(m) in meta.read() {
        if let proto::MetaPayload::Item(w) = m.payload {
            items.meta(m.id, stack_of(&w));
        }
    }
    // S_COLLECT before S_DESTROY, as the server sends them: a collect that arrives in the same
    // frame as the destroy still finds the item (and so still sounds).
    for CollectMsg(c) in collects.read() {
        if let Some(at) = items.collect(c) {
            items.collected.push(at);
        }
    }
    for Destroy(ids) in destroy.read() {
        items.destroy(ids);
    }
}

/// `S_COLLECT`: the particle and `SoundEvents.ENTITY_ITEM_PICKUP` (`PLAYERS`, volume 0.2, pitch
/// `(r - r) * 1.4 + 2`).
fn collect_effects(mut items: ResMut<Items>, audio: Option<Res<Audio>>) {
    for at in std::mem::take(&mut items.collected) {
        if let Some(audio) = &audio {
            let pitch = (audio.next_float() - audio.next_float()) * 1.4 + 2.0;
            audio.play("entity.item.pickup", Category::Players, Some(at.as_vec3()), 0.2, pitch);
        }
    }
}

fn tick(clock: Res<Clock>, streamer: Res<Streamer>, mut items: ResMut<Items>) {
    for _ in 0..clock.ticks_this_frame {
        items.tick(&*streamer);
    }
}

/// `S_SLOTS` for window 0 into the player inventory: `ContainerPlayer` slots 9 to 35 are the
/// main inventory and 36 to 44 the hotbar. The armor, crafting and offhand slots are the window
/// screens' (T7).
fn slots_in(mut slots: MessageReader<Slots>, inv: Option<ResMut<Inventory>>) {
    let Some(mut inv) = inv else {
        slots.clear();
        return;
    };
    for Slots(s) in slots.read() {
        if s.window != proto::WINDOW_INVENTORY {
            continue;
        }
        for (slot, w) in &s.slots {
            let main = match *slot {
                9..=35 => *slot as usize,
                36..=44 => *slot as usize - 36,
                // The armor slots, head (5) to feet (8), and the off hand.
                5..=8 => {
                    inv.player.set_inventory_slot_contents(44 - *slot as usize, stack_of(w));
                    continue;
                }
                45 => {
                    inv.player.set_inventory_slot_contents(40, stack_of(w));
                    continue;
                }
                _ => continue,
            };
            inv.main[main] = stack_of(w);
        }
    }
}

/// `PlayerControllerMP.syncCurrentPlayItem`: the hotbar slot the server should hold, when it
/// differs from the one last sent (`CPacketHeldItemChange`). `S_HELD` (`handleHeldItemChange`)
/// moves the selection and counts as known to the server.
pub fn held_to_send(sent: &mut Option<usize>, current: &mut usize, from_server: Option<u8>) -> Option<proto::HeldSlot> {
    if let Some(s) = from_server {
        *current = s as usize;
        *sent = Some(*current);
    }
    if *sent == Some(*current) {
        return None;
    }
    *sent = Some(*current);
    Some(proto::HeldSlot { slot: *current as u8 })
}

/// Sends the hotbar slot before the drop keys, digging and right clicks of the same frame reach
/// the server, which drops and uses the item in the slot it was last told.
pub fn held_sync(mut held: MessageReader<Held>, inv: Option<ResMut<Inventory>>, link: Res<Link>, mut sent: Local<Option<usize>>) {
    let Some(mut inv) = inv else {
        held.clear();
        return;
    };
    if link.is_added() {
        // A new connection: the server loaded its own slot from the player file.
        *sent = None;
    }
    let from_server = held.read().last().map(|Held(h)| h.slot);
    let mut current = inv.current_item;
    let out = held_to_send(&mut sent, &mut current, from_server);
    if inv.current_item != current {
        inv.current_item = current;
    }
    if let Some(m) = out {
        super::net::send_held(&link, &m);
    }
}

/// `Minecraft.processKeyBinds`: Q is `keyBindDrop`; `isCtrlKeyDown` makes it the whole stack.
fn drop_keys(keys: Option<Res<ButtonInput<KeyCode>>>, menu: Option<Res<Menu>>, state: Res<super::SurvivalState>, player: Res<Player>, link: Res<Link>) {
    let (Some(keys), Some(menu)) = (keys, menu) else { return };
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if let Some(d) = drop_request(keys.just_pressed(KeyCode::KeyQ), ctrl, menu.open, state.dead, player.game_mode) {
        super::net::send_drop(&link, &d);
    }
}

/// `Minecraft.processKeyBinds`: F is `keyBindSwapHands` (`GameSettings.keyBindSwapHands`, key code
/// 33), which sends `CPacketPlayerDigging(SWAP_HELD_ITEMS)` as `V_SWAP_HANDS`.
fn swap_hands_keys(keys: Option<Res<ButtonInput<KeyCode>>>, menu: Option<Res<Menu>>, state: Res<super::SurvivalState>, player: Res<Player>, link: Res<Link>) {
    let (Some(keys), Some(menu)) = (keys, menu) else { return };
    if swap_request(keys.just_pressed(KeyCode::KeyF), menu.open, state.dead, player.game_mode) {
        super::net::send_swap_hands(&link);
    }
}

/// `Entity.isInRangeToRenderDist` for an item: `64 * 0.25` blocks (the box's average edge).
const RENDER_RANGE: f64 = 16.0;
/// The most mesh entities the pool grows to.
const MAX_DRAWN: usize = 1024;

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut items: ResMut<Items>,
    clock: Res<Clock>,
    view: Res<View>,
    streamer: Res<Streamer>,
    meshes: Option<Res<ItemMeshes>>,
    materials: Option<Res<ItemMaterials>>,
    mut assets: ResMut<Assets<ItemMaterial>>,
    player: Res<Player>,
    link: Option<Res<Link>>,
    others: Res<Others>,
    other_q: Query<&Other>,
    mut shown: Query<(&mut Mesh3d, &mut Transform, &mut Visibility)>,
) {
    let (Some(meshes), Some(materials)) = (meshes, materials) else { return };
    let partial = clock.partial;
    let eye = view.eye.as_dvec3();
    let me = link.as_ref().map(|l| l.welcome.id);
    let target_pos = |id: u32| -> DVec3 {
        if Some(id) != me {
            if let Some(o) = others.entity_of(id).and_then(|e| other_q.get(e).ok()) {
                return o.interp_feet(partial);
            }
        }
        player.interp_pos(partial)
    };

    // (mesh, world matrix, lightmap block and sky)
    let mut list: Vec<(Handle<Mesh>, Mat4, (f32, f32))> = Vec::new();
    let add = |c: &Client, at: DVec3, list: &mut Vec<(Handle<Mesh>, Mat4, (f32, f32))>| {
        if at.distance_squared(eye) >= RENDER_RANGE * RENDER_RANGE {
            return;
        }
        let Some(mesh) = model_key(&c.e.stack).and_then(|k| meshes.0.get(&k)) else { return };
        let seed = if c.e.stack.is_empty() { 187 } else { c.e.stack.get_item() as i64 + c.e.stack.get_metadata() as i64 };
        // getBrightnessForRender: the block at the entity's eye height.
        let y = (c.e.body.pos.y + c.e.body.height as f64 / 2.0).floor() as i32;
        let (sky, block) = neighbor_light(&streamer, floor(at.x), y, floor(at.z));
        let light = (block as f32 * 16.0, sky as f32 * 16.0);
        for m in model_matrices(at.as_vec3(), &mesh.ground, mesh.gui3d, c.e.stack.count, seed, c.e.age, partial, c.hover) {
            list.push((mesh.mesh.clone(), m, light));
        }
    };
    for c in items.map.values() {
        add(c, c.feet(partial), &mut list);
    }
    for p in &items.pickups {
        let at = pickup_position(p.item.e.body.pos, target_pos(p.target), PICKUP_Y_OFFSET, p.age, partial);
        add(&p.item, at, &mut list);
    }
    list.truncate(MAX_DRAWN);

    while items.pool.len() < list.len() {
        let Some(mut m) = assets.get(&materials.hand).cloned() else { return };
        let [l0, l1] = standard_lights();
        m.item.w0 = l0.extend(0.0);
        m.item.w1 = l1.extend(0.0);
        m.item.flags = UVec4::new(1, 0, 0, 1);
        let material = assets.add(m);
        let entity = commands
            .spawn((Mesh3d::default(), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster))
            .id();
        items.pool.push(Slot { entity, material });
    }
    for (i, slot) in items.pool.iter().enumerate() {
        let Ok((mut mesh, mut transform, mut visibility)) = shown.get_mut(slot.entity) else { continue };
        let Some((handle, matrix, light)) = list.get(i) else {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        if mesh.0 != *handle {
            mesh.0 = handle.clone();
        }
        *transform = Transform::from_matrix(*matrix);
        if *visibility != Visibility::Inherited {
            *visibility = Visibility::Inherited;
        }
        if assets.get(&slot.material).is_some_and(|m| m.item.w0.w != light.0 || m.item.w1.w != light.1) {
            if let Some(mut m) = assets.get_mut(&slot.material) {
                m.item.w0.w = light.0;
                m.item.w1.w = light.1;
            }
        }
    }
}

/// `--items`: once the terrain ahead has loaded, drops a few stacks in front of the camera, as
/// `S_SPAWN_OBJECT` would, so a `--shot` shows the render.
fn preview(view: Res<View>, streamer: Res<Streamer>, mut items: ResMut<Items>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let fwd = Vec3::new(-view.yaw.to_radians().sin(), 0.0, view.yaw.to_radians().cos()).normalize_or_zero();
    let right = Vec3::new(-fwd.z, 0.0, fwd.x);
    let stacks = [
        ItemStack::new(items::ids::block::COBBLESTONE, 64, 0),
        ItemStack::new(items::ids::item::DIAMOND, 1, 0),
        ItemStack::new(items::ids::item::STICK, 20, 0),
        ItemStack::new(items::ids::item::IRON_PICKAXE, 1, 0),
        ItemStack::new(items::ids::block::PLANKS, 40, 0),
    ];
    let mut spawns = Vec::new();
    for (i, s) in stacks.iter().enumerate() {
        let p = view.eye + fwd * 2.5 + right * (i as f32 - 2.0) * 0.8;
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        if streamer.state_at(x, 0, z).is_none() {
            return;
        }
        let ground = (0..256).rev().find(|&y| {
            let st = streamer.state_at(x, y, z).unwrap_or(0);
            crate::region::props(st).mat == crate::region::Mat::Solid
        });
        let Some(g) = ground else { return };
        spawns.push(SpawnObject {
            id: 9000 + i as u32,
            pos: [p.x as f64, g as f64 + 1.0, p.z as f64],
            data: ObjectData::Item(crate::survival::net::wire_stack(s)),
            ..Default::default()
        });
    }
    for s in &spawns {
        items.spawn(s);
    }
    *done = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::Blocks;
    use items::ids::item;

    #[test]
    fn a_stack_draws_one_to_five_models() {
        let counts: Vec<(i32, usize)> = [0, 1, 2, 16, 17, 32, 33, 48, 49, 64].iter().map(|&c| (c, model_count(c))).collect();
        assert_eq!(counts, [(0, 1), (1, 1), (2, 2), (16, 2), (17, 3), (32, 3), (33, 4), (48, 4), (49, 5), (64, 5)]);
    }

    #[test]
    fn bob_and_spin_follow_the_age() {
        // age 0, partial 0, hover 0: sin(0) * 0.1 + 0.1.
        assert_eq!(bob(0, 0.0, 0.0), 0.1);
        assert_eq!(spin_degrees(0, 0.0, 0.0), 0.0);
        // A full 20 ticks of age turns one radian.
        assert!((spin_degrees(20, 0.0, 0.0) - 57.295_776).abs() < 1.0e-4);
        // Halfway between ticks 10 and 11, hover 0.5: sin(1.05 + 0.5) through the table.
        let want = sin(10.5 / 10.0 + 0.5) * 0.1 + 0.1;
        assert_eq!(bob(10, 0.5, 0.5), want);
        assert!((want - ((1.55f32).sin() * 0.1 + 0.1)).abs() < 1.0e-3);
        // The bob stays between 0 and 0.2.
        for age in 0..200 {
            let b = bob(age, 0.3, 1.0);
            assert!((-1.0e-3..=0.2 + 1.0e-3).contains(&b), "{b}");
        }
    }

    #[test]
    fn model_copies_spread_by_kind() {
        // A flat item (stick: ground scale 0.5): 3 copies stacked 0.09375 * 0.5 apart in z.
        let ground = Display { rotation: [0.0; 3], translation: [0.0; 3], scale: [0.5; 3] };
        let flat = model_matrices(Vec3::ZERO, &ground, false, 20, 280, 0, 0.0, 0.0);
        assert_eq!(flat.len(), 3);
        // The origin of the model space (the -0.5 centre) lands 0.25 up, spin 0: copies differ
        // only by the small jitter in x and y and the step in z.
        let at = |m: &Mat4| m.transform_point3(Vec3::splat(0.5));
        let dz = at(&flat[1]).z - at(&flat[0]).z;
        assert!((dz - 0.093_75 * 0.5).abs() < 0.08, "{dz}");
        assert!(at(&flat[2]).z > at(&flat[0]).z);
        // A block (gui3d): 5 copies jittered within 0.15, same y base.
        let g3 = Display { rotation: [0.0; 3], translation: [0.0, 3.0 / 16.0, 0.0], scale: [0.25; 3] };
        let blocks = model_matrices(Vec3::new(1.0, 64.0, 2.0), &g3, true, 64, 4, 0, 0.0, 0.0);
        assert_eq!(blocks.len(), 5);
        let c0 = at(&blocks[0]);
        assert!((c0.x - 1.0).abs() < 1.0e-5 && (c0.z - 2.0).abs() < 1.0e-5, "{c0}");
        for m in &blocks[1..] {
            let d = at(m) - c0;
            assert!(d.abs().max_element() <= 0.15 + 1.0e-5, "{d}");
        }
        // The same stack and age give the same copies (the random is seeded by item and meta).
        assert_eq!(blocks, model_matrices(Vec3::new(1.0, 64.0, 2.0), &g3, true, 64, 4, 0, 0.0, 0.0));
        // A single block draws one copy at the bobbing height: 0.1 bob + 0.25 * 0.25 scale.
        let one = model_matrices(Vec3::ZERO, &g3, true, 1, 4, 0, 0.0, 0.0);
        assert!((at(&one[0]).y - (0.1 + 0.0625 + 3.0 / 16.0)).abs() < 1.0e-5);
    }

    #[test]
    fn the_pickup_flies_to_the_collector() {
        let (item, target) = (DVec3::new(0.0, 64.0, 0.0), DVec3::new(2.0, 64.0, 0.0));
        let p = |age, partial| pickup_position(item, target, 0.5, age, partial);
        assert_eq!(p(0, 0.0), item);
        // f = (1.5 / 3)^2 = 0.25 of the way.
        let mid = p(1, 0.5);
        assert!((mid.x - 0.5).abs() < 1.0e-6 && (mid.y - 64.125).abs() < 1.0e-6, "{mid}");
        assert!((p(3, 0.0) - (target + DVec3::new(0.0, 0.5, 0.0))).length() < 1.0e-6);
    }

    #[test]
    fn q_drops_one_and_ctrl_q_the_stack() {
        let survival = proto::GM_SURVIVAL;
        assert_eq!(drop_request(true, false, false, false, survival), Some(proto::DropItem { all: false }));
        assert_eq!(drop_request(true, true, false, false, survival), Some(proto::DropItem { all: true }));
        assert_eq!(drop_request(false, true, false, false, survival), None);
        assert_eq!(drop_request(true, false, true, false, survival), None, "a screen has the keys");
        assert_eq!(drop_request(true, false, false, true, survival), None, "dead");
        assert_eq!(drop_request(true, false, false, false, proto::GM_SPECTATOR), None);
        // The frames: DROP_ITEM and DROP_ALL_ITEMS.
        assert_eq!(proto::DropItem { all: false }.encode(), [0]);
        assert_eq!(proto::DropItem { all: true }.encode(), [1]);
    }

    #[test]
    fn f_swaps_the_hands_and_a_spectator_does_not() {
        let survival = proto::GM_SURVIVAL;
        assert!(swap_request(true, false, false, survival));
        assert!(!swap_request(false, false, false, survival));
        assert!(!swap_request(true, true, false, survival), "a screen has the keys");
        assert!(!swap_request(true, false, true, survival), "dead");
        assert!(!swap_request(true, false, false, proto::GM_SPECTATOR));
        // The frame carries no body.
        assert_eq!(proto::encode_empty(), Vec::<u8>::new());
    }

    #[test]
    fn the_hotbar_slot_reaches_the_server_once_per_change() {
        let (mut sent, mut cur) = (None, 0usize);
        // The first frame of a connection tells the server, then nothing until it changes.
        assert_eq!(held_to_send(&mut sent, &mut cur, None), Some(proto::HeldSlot { slot: 0 }));
        assert_eq!(held_to_send(&mut sent, &mut cur, None), None);
        cur = 1;
        assert_eq!(held_to_send(&mut sent, &mut cur, None), Some(proto::HeldSlot { slot: 1 }));
        assert_eq!(held_to_send(&mut sent, &mut cur, None), None);
        // S_HELD moves the selection and is not echoed back.
        assert_eq!(held_to_send(&mut sent, &mut cur, Some(6)), None);
        assert_eq!(cur, 6);
    }

    fn spawn_msg(id: u32, pos: [f64; 3], stack: ItemStack) -> SpawnObject {
        SpawnObject { id, pos, motion: [0.0, 0.2, 0.0], data: ObjectData::Item(crate::survival::net::wire_stack(&stack)), ..Default::default() }
    }

    #[test]
    fn frames_move_an_item_and_a_collect_flies_it_off() {
        let mut items = Items::default();
        let w = Blocks::floor_at(63, 8);
        let diamond = ItemStack::new(item::DIAMOND, 3, 0);
        items.spawn(&spawn_msg(7, [0.5, 66.0, 0.5], diamond));
        assert_eq!(items.map[&7].e.stack.count, 3);
        // It falls onto the stone and comes to rest on top (y 64).
        for _ in 0..80 {
            items.tick(&w);
        }
        let c = &items.map[&7];
        assert!(c.e.body.on_ground && (c.e.body.pos.y - 64.0).abs() < 1.0e-3, "{:?}", c.e.body.pos);
        assert_eq!(c.e.age, 80);
        // A move frame places it, a velocity frame sets its motion, a meta frame the stack.
        items.moves(&[EntityMove { id: 7, pos: [2.0, 65.0, 2.0], on_ground: false, ..Default::default() }]);
        assert_eq!(items.map[&7].e.body.pos, DVec3::new(2.0, 65.0, 2.0));
        items.velocity(&Velocity { id: 7, motion: [0.1, 0.0, 0.0] });
        assert_eq!(items.map[&7].e.body.motion.x, 0.1f32 as f64);
        items.meta(7, ItemStack::new(item::DIAMOND, 5, 0));
        assert_eq!(items.map[&7].e.stack.count, 5);
        // An unknown id changes nothing.
        items.moves(&[EntityMove { id: 8, ..Default::default() }]);
        // S_COLLECT: the item leaves the world, a pickup carries it for three ticks.
        let at = items.collect(&Collect { item: 7, collector: 1, count: 2 }).unwrap();
        assert_eq!(at, DVec3::new(2.0, 65.0, 2.0));
        assert!(items.map.is_empty());
        assert_eq!(items.pickups[0].item.e.stack.count, 2);
        for _ in 0..2 {
            items.tick(&w);
            assert_eq!(items.pickups.len(), 1);
        }
        items.tick(&w);
        assert!(items.pickups.is_empty());
        assert!(items.collect(&Collect { item: 7, collector: 1, count: 2 }).is_none());
        items.spawn(&spawn_msg(9, [0.5, 66.0, 0.5], diamond));
        items.destroy(&[9]);
        assert!(items.map.is_empty());
    }

    #[test]
    fn a_collect_sounds_even_when_the_destroy_follows_in_the_same_frame() {
        let mut items = Items::default();
        items.spawn(&spawn_msg(7, [0.5, 66.0, 0.5], ItemStack::new(item::DIAMOND, 3, 0)));
        // read_frames order: collect, then destroy.
        if let Some(at) = items.collect(&Collect { item: 7, collector: 1, count: 3 }) {
            items.collected.push(at);
        }
        items.destroy(&[7]);
        assert_eq!(items.collected.len(), 1);
    }

    #[test]
    fn an_item_in_an_unloaded_chunk_waits() {
        struct Unloaded;
        impl BlockAccess for Unloaded {
            fn state(&self, _: i32, _: i32, _: i32) -> u16 {
                0
            }
            fn loaded(&self, _: i32, _: i32) -> bool {
                false
            }
        }
        let mut items = Items::default();
        items.spawn(&spawn_msg(1, [0.5, 66.0, 0.5], ItemStack::new(item::STICK, 1, 0)));
        items.tick(&Unloaded);
        assert_eq!(items.map[&1].e.age, 0);
    }

    #[test]
    fn wire_stacks_read_back() {
        let s = ItemStack::new(item::DIAMOND_PICKAXE, 1, 12);
        let back = stack_of(&crate::survival::net::wire_stack(&s));
        assert_eq!((back.get_item(), back.count, back.get_metadata()), (s.get_item(), 1, 12));
        assert!(stack_of(&proto::WireStack::EMPTY).is_empty());
    }
}
