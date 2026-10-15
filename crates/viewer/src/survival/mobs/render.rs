//! Mobs in a server session: the entities `S_SPAWN_MOB` creates, kept current by `S_ENTITY_MOVE`
//! (with the head yaw), `S_VELOCITY`, `S_ENTITY_META` and `S_DESTROY`, ticked as
//! `EntityLivingBase`, and drawn as `RenderLiving` / `RenderLivingBase.doRender` draws them.
//!
//! The draw is the players/ pattern: a root entity per mob with one child per model part on an
//! `EntityMaterial` (the entity lighting, the lightmap at the mob and the fog of
//! shaders/entity.wgsl), the root carrying `doRender`'s transform and each part
//! `ModelRenderer.render`'s. Which parts, texture and pose a kind has is `models::MOBS`.
//!
//! The hurt and death look is `RenderLivingBase`'s: `setBrightness` puts red at 0.3 over the model
//! while `hurtTime` or `deathTime` is above 0 (the material's `light.z`, entity.wgsl), and
//! `applyRotations` lays the mob over on its side by `deathTime`. `S_ENTITY_EVENT` 2, 36 and 37 are
//! `handleStatusUpdate`'s hurt, 3 its death; the mob's own hurt and death sounds play with them.
//!
//! The layers (`RenderLiving.layerRenderers`, `models::LayerDef`) are one more set of part entities
//! per mob on their own material, posed with the base's frame: `LayerSaddle` while the mob's flags
//! say saddled, `LayerSheepWool` while they say not sheared, on the texture variant tinted for the
//! fleece color. `anim.rs` holds the sheep and chicken client state their frames read.
//!
//! A burning mob (`MM_BURNING`) renders at full lightmap (`RenderManager.renderEntity`) and gets
//! `fire.rs`' sprites; a creeper swells and flashes by its client side fuse (`RenderCreeper`),
//! the skeleton's bow comes from `S_EQUIPMENT` (`held.rs`) and `S_ANIMATION` 0 swings the arm.
//!
//! DROPPED: the shadow blob (`shadowSize` 0.7 for the cow, 0.3 for the chicken), the name tag
//! (`canRenderName`), the leash, the outline mode and the frustum test;
//! `isInRangeToRenderDist` stays. The hurt and death sounds are the server's `playHurtSound`
//! and `onDeath` in vanilla (`S_SOUND` here once it sends them); the client's own
//! `playSound(null player)` is silent on a `WorldClient`, so these play at the status instead,
//! at `getSoundPitch`.

use std::collections::HashMap;

use bevy::math::DVec3;
use bevy::prelude::*;

use super::anim::{Anim, EAT_GRASS_TICKS};
use super::fire::{self, FireImages};
use super::held::stack_of;
use super::hostile::{creeper_flash, creeper_scale};
use super::living::{death_rotation, hurt_overlay, Frame, Living};
use super::models::{def_of, layer_transforms, part_mesh, part_transforms, LayerKind, MobDef};
use crate::audio::Audio;
use crate::material::EntityMaterial;
use crate::players::Skin;
use crate::sky::{neighbor_light, Clock, View};
use crate::stream::Streamer;
use crate::survival::sound::category_of;
use crate::survival::{Animation as AnimationMsg, Destroy, EntityEvent as EventMsg, EntityMeta, EntityMoves, Equipment as EquipmentMsg, SpawnMob as SpawnMsg, Velocity as VelocityMsg};

/// The textures of `models::MOBS`, in that order, in the blocks' gamma space.
#[derive(Resource)]
pub struct MobImages(pub Vec<Handle<Image>>);

/// The layer textures of `models::MOBS`: per kind, per layer, one image per variant (the wool
/// layer has 16, tinted by `EnumDyeColor`; the saddle 1).
#[derive(Resource)]
pub struct LayerImages(pub Vec<Vec<Vec<Handle<Image>>>>);

/// Server entity id to the mob's root entity, and the part meshes made so far per kind and model
/// (0 the base, `1 + n` its layer `n`).
#[derive(Resource, Default)]
pub struct Mobs {
    by_id: HashMap<u32, Entity>,
    meshes: HashMap<(usize, usize), Vec<Handle<Mesh>>>,
    /// The two fire sprites' meshes per kind and adult or child
    fire_meshes: HashMap<(usize, bool), [Handle<Mesh>; 2]>,
    /// Main hand items that arrived in the frame their mob spawned: `Mob` is inserted by a
    /// deferred command, so the hand is set at the start of the next frame.
    pending_hands: HashMap<u32, Option<(u16, u16)>>,
}

impl Mobs {
    /// An equipment frame for mob `id`: `Some(hand)` when the mob exists to take it now, else it
    /// is kept for [`Mobs::take_pending`] when the id is a mob spawned this frame.
    fn queue_hand(&mut self, id: u32, hand: Option<(u16, u16)>, exists: bool) -> Option<Option<(u16, u16)>> {
        if exists {
            return Some(hand);
        }
        if self.by_id.contains_key(&id) {
            self.pending_hands.insert(id, hand);
        }
        None
    }

    /// The hands kept by [`Mobs::queue_hand`], to apply now.
    fn take_pending(&mut self) -> Vec<(u32, Option<(u16, u16)>)> {
        self.pending_hands.drain().collect()
    }
}

/// The fire of a burning mob: one part entity per sprite and their materials.
struct FireInst {
    parts: [Entity; 2],
    materials: [Handle<EntityMaterial>; 2],
}

/// One layer's part entities of a mob, and the material they share.
struct LayerInst {
    kind: LayerKind,
    /// Index into `MobDef::layers`
    index: usize,
    parts: Vec<Entity>,
    material: Handle<EntityMaterial>,
    images: Vec<Handle<Image>>,
}

/// One tracked mob.
#[derive(Component)]
pub struct Mob {
    /// The server's entity id
    pub id: u32,
    pub living: Living,
    def: &'static MobDef,
    /// `MobMeta` `MM_CHILD`, which `isChild()` reads
    child: bool,
    /// `MobMeta::flags` and the wool color (`EnumDyeColor` meta) as last sent
    flags: u8,
    wool: u8,
    /// The sheep's eat timer and the chicken's wings
    pub anim: Anim,
    parts: Vec<Entity>,
    material: Handle<EntityMaterial>,
    layers: Vec<LayerInst>,
    /// `S_EQUIPMENT` slot 0: the main hand item's id and metadata
    pub main_hand: Option<(u16, u16)>,
    fire: Option<FireInst>,
}

impl Mob {
    pub fn def(&self) -> &'static MobDef {
        self.def
    }

    pub fn is_child(&self) -> bool {
        self.child
    }

    pub fn material(&self) -> &Handle<EntityMaterial> {
        &self.material
    }

    /// The model's frame: `Living::frame` plus what the model reads off the entity (the sheep's head,
    /// the chicken's wings, the flags, the bow).
    pub fn frame(&self, partial: f32) -> Frame {
        let mut frame = self.living.frame(partial, self.child);
        match self.def.kind {
            proto::MOB_SHEEP => self.anim.sheep_frame(&mut frame, partial, self.living.pitch),
            // RenderChicken.handleRotationFloat is the model's ageInTicks
            proto::MOB_CHICKEN => frame.age = self.anim.wing_angle(partial),
            _ => {}
        }
        frame.flags = self.flags;
        frame.bow = self.main_hand.is_some_and(|(id, _)| id == items::ids::item::BOW);
        frame
    }

    /// `EntityCreeper.getCreeperFlashIntensity`, 0 for any other kind.
    fn flash(&self, partial: f32) -> f32 {
        if self.def.kind == proto::MOB_CREEPER {
            self.anim.flash_intensity(partial)
        } else {
            0.0
        }
    }

    /// `Entity.getEntityBoundingBox` from `setSize`: the adult width and height, halved for a
    /// child (`EntityAgeable.setScale`), `width / 2` kept as a float, centred on the feet.
    pub fn bb(&self) -> sim::collision::Aabb {
        let k = if self.child { 0.5 } else { 1.0 };
        let (w, h) = (self.def.size.0 * k, self.def.size.1 * k);
        let f = (w / 2.0) as f64;
        let p = self.living.pos;
        sim::collision::Aabb::new(p.x - f, p.y, p.z - f, p.x + f, p.y + h as f64, p.z + f)
    }
}

/// `EntityLivingBase.getSoundPitch`: `(rand - rand) * 0.2 + 1`, or `+ 1.5` for a child.
pub fn sound_pitch(child: bool, r1: f32, r2: f32) -> f32 {
    (r1 - r2) * 0.2 + if child { 1.5 } else { 1.0 }
}

/// Plays one of the mob's sounds at its feet: `Entity.playSound` in the neutral category.
fn play_own(audio: &Audio, mob: &Mob, name: &str) {
    let pitch = sound_pitch(mob.child, audio.next_float(), audio.next_float());
    let at = mob.living.pos.as_vec3();
    audio.play(name, category_of(6), Some(at), mob.def.sounds.volume, pitch);
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Mobs>().add_systems(
        Update,
        (
            read_frames.run_if(resource_exists::<MobImages>),
            tick.after(crate::player::run_ticks).after(read_frames).run_if(in_world),
            pose.after(crate::sky::advance_clock).after(tick).run_if(in_world),
            super::held::pose_held.after(pose).run_if(in_world),
        ),
    );
    app.add_systems(Update, super::preview::demo.before(read_frames).run_if(in_world).run_if(|a: Option<Res<crate::Args>>| a.is_some_and(|a| a.mobs)));
}

/// The resources the world systems read; a bare plugin (the frame tests) has none of them.
pub(super) fn in_world(clock: Option<Res<Clock>>, streamer: Option<Res<Streamer>>, view: Option<Res<View>>) -> bool {
    clock.is_some() && streamer.is_some() && view.is_some()
}

/// `handleSpawnMob` and the rest of the entity frames, for the kinds in the table.
#[allow(clippy::too_many_arguments)]
fn read_frames(
    mut commands: Commands,
    mut mobs: ResMut<Mobs>,
    images: Res<MobImages>,
    layer_images: Option<Res<LayerImages>>,
    skin: Option<Res<Skin>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut spawns: MessageReader<SpawnMsg>,
    mut moves: MessageReader<EntityMoves>,
    mut velocity: MessageReader<VelocityMsg>,
    mut meta: MessageReader<EntityMeta>,
    mut destroy: MessageReader<Destroy>,
    mut events: MessageReader<EventMsg>,
    (mut equipment, mut animations, fire_images): (MessageReader<EquipmentMsg>, MessageReader<AnimationMsg>, Option<Res<FireImages>>),
    audio: Option<Res<Audio>>,
    mut live: Query<&mut Mob>,
) {
    let Some(skin) = skin else { return };
    for (id, hand) in mobs.take_pending() {
        match mobs.by_id.get(&id).and_then(|&e| live.get_mut(e).ok()) {
            Some(mut mob) => mob.main_hand = hand,
            None if mobs.by_id.contains_key(&id) => {
                mobs.pending_hands.insert(id, hand);
            }
            None => {}
        }
    }
    for SpawnMsg(s) in spawns.read() {
        // A repeated spawn of an id replaces it, as `addEntityToWorld` does.
        if let Some(old) = mobs.by_id.remove(&s.id) {
            commands.entity(old).despawn();
        }
        let Some((index, def)) = def_of(s.kind) else { continue };
        let Some(image) = images.0.get(index) else { continue };
        let handles = mobs.meshes.entry((index, 0)).or_insert_with(|| def.parts.iter().enumerate().map(|(i, p)| meshes.add(part_mesh(def, p, def.mirrored.contains(&i)))).collect()).clone();
        let material = materials.add(EntityMaterial::new(skin.env.clone(), image.clone()));
        let root = commands.spawn((Name::new(def.name), Transform::default(), Visibility::Hidden)).id();
        let parts = handles
            .into_iter()
            .map(|h| commands.spawn((Mesh3d(h), MeshMaterial3d(material.clone()), Transform::default(), ChildOf(root))).id())
            .collect();
        let mut layers = Vec::new();
        for (n, layer) in def.layers.iter().enumerate() {
            let Some(variants) = layer_images.as_ref().and_then(|l| l.0.get(index)).and_then(|l| l.get(n)) else { continue };
            let Some(first) = variants.get(layer.kind.variant(s.meta.wool)) else { continue };
            let handles = mobs.meshes.entry((index, 1 + n)).or_insert_with(|| layer.parts.iter().map(|p| meshes.add(part_mesh(def, p, false))).collect()).clone();
            let material = materials.add(EntityMaterial { additive: layer.kind.additive(), fx: layer.kind.fx(0.0), ..EntityMaterial::new(skin.env.clone(), first.clone()) });
            let parts = handles
                .into_iter()
                .map(|h| commands.spawn((Mesh3d(h), MeshMaterial3d(material.clone()), Transform::default(), ChildOf(root))).id())
                .collect();
            layers.push(LayerInst { kind: layer.kind, index: n, parts, material, images: variants.clone() });
        }
        let child = s.meta.flags & proto::MM_CHILD != 0;
        // Render.renderEntityOnFire: one part per sprite, the quads sized by the entity's box.
        let fire = fire_images.as_ref().map(|images| {
            let k = if child { 0.5 } else { 1.0 };
            let (w, h) = (def.size.0 * k, def.size.1 * k);
            let handles = mobs.fire_meshes.entry((index, child)).or_insert_with(|| [0, 1].map(|n| meshes.add(fire::mesh(w, h, n, images.frames)))).clone();
            let mut make = |n: usize| {
                let material = materials.add(EntityMaterial { fx: Vec4::new(0.0, 0.0, 1.0, 1.0), ..EntityMaterial::new(skin.env.clone(), images.layers[n].clone()) });
                let part = commands.spawn((Mesh3d(handles[n].clone()), MeshMaterial3d(material.clone()), Transform::default(), Visibility::Hidden, ChildOf(root))).id();
                (part, material)
            };
            let (p0, m0) = make(0);
            let (p1, m1) = make(1);
            FireInst { parts: [p0, p1], materials: [m0, m1] }
        });
        commands.entity(root).insert(Mob { id: s.id, living: Living::spawned(s), def, child, flags: s.meta.flags, wool: s.meta.wool, anim: Anim::default(), parts, material, layers, main_hand: None, fire });
        mobs.by_id.insert(s.id, root);
    }
    for EntityMoves(list) in moves.read() {
        for m in list {
            if let Some(mut mob) = mobs.by_id.get(&m.id).and_then(|&e| live.get_mut(e).ok()) {
                mob.living.moved(m);
            }
        }
    }
    for VelocityMsg(v) in velocity.read() {
        if let Some(mut mob) = mobs.by_id.get(&v.id).and_then(|&e| live.get_mut(e).ok()) {
            mob.living.set_velocity(v.motion);
        }
    }
    for EntityMeta(m) in meta.read() {
        if let proto::MetaPayload::Mob(data) = m.payload {
            if let Some(mut mob) = mobs.by_id.get(&m.id).and_then(|&e| live.get_mut(e).ok()) {
                mob.child = data.flags & proto::MM_CHILD != 0;
                mob.flags = data.flags;
                mob.wool = data.wool;
            }
        }
    }
    // handleEntityEquipment: the main hand slot.
    for EquipmentMsg(e) in equipment.read() {
        if e.slot == proto::EQ_MAIN_HAND {
            let hand = stack_of(e.stack.id, e.stack.meta);
            let found = mobs.by_id.get(&e.id).and_then(|&en| live.get_mut(en).ok());
            let exists = found.is_some();
            if let Some(mut mob) = found {
                mob.main_hand = hand;
            } else {
                mobs.queue_hand(e.id, hand, exists);
            }
        }
    }
    // handleAnimation 0: swingArm.
    for AnimationMsg(a) in animations.read() {
        if a.animation == proto::ANIM_SWING_MAIN {
            if let Some(mut mob) = mobs.by_id.get(&a.id).and_then(|&en| live.get_mut(en).ok()) {
                mob.living.swing_arm();
            }
        }
    }
    // handleStatusUpdate: 2, 36 and 37 hurt, 3 dies.
    for EventMsg(e) in events.read() {
        let Some(mut mob) = mobs.by_id.get(&e.id).and_then(|&en| live.get_mut(en).ok()) else { continue };
        match e.status {
            proto::EV_HURT | proto::EV_HURT_DROWN | proto::EV_HURT_BURN => {
                mob.living.hurt();
                if let Some(audio) = &audio {
                    play_own(audio, &mob, mob.def.sounds.hurt);
                }
            }
            proto::EV_DEATH => {
                mob.living.die();
                if let Some(audio) = &audio {
                    play_own(audio, &mob, mob.def.sounds.death);
                }
            }
            // EntitySheep.handleStatusUpdate: sheepTimer = 40 (nothing else answers 10 here)
            proto::EV_EAT_GRASS if mob.def.kind == proto::MOB_SHEEP => mob.anim.sheep_timer = EAT_GRASS_TICKS,
            _ => {}
        }
    }
    for Destroy(ids) in destroy.read() {
        for id in ids {
            mobs.pending_hands.remove(id);
            if let Some(e) = mobs.by_id.remove(id) {
                commands.entity(e).despawn();
            }
        }
    }
}

/// This frame's client ticks for every mob. One that has died for 20 ticks is `setDead`: taken
/// out of the world.
fn tick(mut commands: Commands, mut by_id: ResMut<Mobs>, clock: Res<Clock>, mut mobs: Query<(Entity, &mut Mob)>) {
    for _ in 0..clock.ticks_this_frame {
        for (e, mut mob) in &mut mobs {
            if mob.living.dead {
                continue;
            }
            mob.living.tick();
            // onLivingUpdate's client branches of EntitySheep and EntityChicken
            match mob.def.kind {
                proto::MOB_SHEEP => mob.anim.tick_sheep(),
                proto::MOB_CHICKEN => {
                    let on_ground = mob.living.on_ground;
                    mob.anim.tick_chicken(on_ground);
                }
                // EntityCreeper.onUpdate
                proto::MOB_CREEPER => {
                    let ignited = mob.flags & proto::MM_IGNITED != 0;
                    mob.anim.tick_creeper(ignited);
                }
                _ => {}
            }
            if mob.living.dead {
                by_id.by_id.remove(&mob.id);
                commands.entity(e).despawn();
            }
        }
    }
}

/// `RenderLivingBase.doRender`'s transform down to the model's space (before the 1/16 units):
/// translate to the interpolated feet, rotate `180 - renderYawOffset`, `applyRotations`' death
/// lie-down about Z (`death_tilt` degrees), `prepareScale`'s `scale(-1, -1, 1)` and the 1.501 drop.
#[cfg(test)]
pub fn root_matrix(feet: Vec3, body_yaw: f32, death_tilt: f32) -> Mat4 {
    root_matrix_scaled(feet, body_yaw, death_tilt, Vec3::ONE)
}

/// [`root_matrix`] with `preRenderCallback`'s `scale` between the flip and the drop (a creeper's swell).
pub fn root_matrix_scaled(feet: Vec3, body_yaw: f32, death_tilt: f32, pre_scale: Vec3) -> Mat4 {
    Mat4::from_translation(feet)
        * Mat4::from_rotation_y((180.0 - body_yaw).to_radians())
        * Mat4::from_rotation_z(death_tilt.to_radians())
        * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0))
        * Mat4::from_scale(pre_scale)
        * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0))
}

/// `Entity.isInRangeToRenderDist` for a box of this width and height at `renderDistanceWeight` 1.
pub fn in_render_range(distance_sq: f64, size: (f32, f32)) -> bool {
    let d = (size.0 + size.1 + size.0) as f64 / 3.0 * 64.0;
    distance_sq < d * d
}

/// Places and lights every mob for this frame.
#[allow(clippy::too_many_arguments)]
fn pose(
    clock: Res<Clock>,
    view: Res<View>,
    streamer: Res<Streamer>,
    fire_images: Option<Res<FireImages>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut roots: Query<(&Mob, &mut Transform, &mut Visibility)>,
    mut part_tf: Query<&mut Transform, Without<Mob>>,
    mut part_vis: Query<&mut Visibility, Without<Mob>>,
) {
    let partial = clock.partial;
    for (mob, mut t, mut vis) in &mut roots {
        let feet = mob.living.interp_pos(partial);
        let shown = in_render_range((feet - view.eye.as_dvec3()).length_squared(), mob.def.size);
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if !shown {
            continue;
        }
        let frame = mob.frame(partial);
        // RenderCreeper.preRenderCallback and getColorMultiplier
        let flash = mob.flash(partial);
        let (pre_scale, white) = if mob.def.kind == proto::MOB_CREEPER { (creeper_scale(flash), creeper_flash(flash)) } else { (Vec3::ONE, 0.0) };
        // applyRotations lies the mob over by deathTime, up to the kind's getDeathMaxRotation
        let tilt = death_rotation(mob.living.death_time, partial) * mob.def.death_max / 90.0;
        let root = root_matrix_scaled(feet.as_vec3(), frame.body_yaw, tilt, pre_scale) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
        *t = Transform::from_matrix(root);
        for (e, local) in mob.parts.iter().zip(part_transforms(mob.def, &frame, mob.child)) {
            if let Ok(mut pt) = part_tf.get_mut(*e) {
                *pt = local;
            }
        }
        // getBrightnessForRender: the lightmap coordinates at the mob's eyes, and in z the
        // setBrightness overlay strength; RenderManager.renderEntity lights a burning entity at (15, 15).
        let burning = mob.flags & proto::MM_BURNING != 0;
        let eye = feet + DVec3::new(0.0, (if mob.child { mob.def.eye_height.1 } else { mob.def.eye_height.0 }) as f64, 0.0);
        let (sky, block) = if burning { (15, 15) } else { neighbor_light(&streamer, eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32) };
        let light = Vec4::new(block as f32 * 16.0, sky as f32 * 16.0, hurt_overlay(mob.living.hurt_time, mob.living.death_time), white);
        set_light(&mut materials, &mob.material, light, None);
        for l in &mob.layers {
            let show = l.kind.shown(mob.flags);
            for (e, local) in l.parts.iter().zip(layer_transforms(mob.def, mob.def.layers[l.index].parts, &frame, mob.child)) {
                if let Ok(mut pt) = part_tf.get_mut(*e) {
                    *pt = local;
                }
                if let Ok(mut v) = part_vis.get_mut(*e) {
                    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
                    if *v != want {
                        *v = want;
                    }
                }
            }
            // LayerSheepWool's fleece color, as the texture variant tinted for it.
            if let Some(img) = l.images.get(l.kind.variant(mob.wool)) {
                if materials.get(&l.material).is_some_and(|m| m.skin != *img) {
                    if let Some(mut m) = materials.get_mut(&l.material) {
                        m.skin = img.clone();
                    }
                }
            }
            // A layer that does not combine textures gets no overlay, and one may set its own lightmap.
            let mut layer_light = if l.kind.combines() { light } else { Vec4::new(light.x, light.y, 0.0, 0.0) };
            if let Some((b, s)) = l.kind.fixed_light() {
                layer_light.x = b;
                layer_light.y = s;
            }
            set_light(&mut materials, &l.material, layer_light, Some(l.kind.fx(frame.age)));
        }
        // Render.renderEntityOnFire: face the camera at the feet, one frame of each sprite's strip.
        if let (Some(fire), Some(images)) = (&mob.fire, &fire_images) {
            let on = burning && !mob.living.dead;
            let local = Transform::from_matrix(root.inverse() * fire::world_matrix(feet.as_vec3(), view.yaw));
            let frame_v = images.frame(clock.ticks) as f32 / images.frames.max(1) as f32;
            for (e, m) in fire.parts.iter().zip(&fire.materials) {
                if let Ok(mut v) = part_vis.get_mut(*e) {
                    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
                    if *v != want {
                        *v = want;
                    }
                }
                if !on {
                    continue;
                }
                if let Ok(mut pt) = part_tf.get_mut(*e) {
                    *pt = local;
                }
                set_light(&mut materials, m, Vec4::new(240.0, 240.0, 0.0, 0.0), Some(Vec4::new(0.0, frame_v, 1.0, 1.0)));
            }
        }
    }
}

/// Writes a material's light (and look) when they changed.
fn set_light(materials: &mut Assets<EntityMaterial>, handle: &Handle<EntityMaterial>, light: Vec4, fx: Option<Vec4>) {
    if materials.get(handle).is_some_and(|m| m.light != light || fx.is_some_and(|f| m.fx != f)) {
        if let Some(mut m) = materials.get_mut(handle) {
            m.light = light;
            if let Some(f) = fx {
                m.fx = f;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_turns_the_cow_to_its_body_yaw() {
        // Body yaw 0 faces south (+z): the model's front (-z in model space, after the flip) ends up +z.
        let m = root_matrix(Vec3::ZERO, 0.0, 0.0);
        let front = m.transform_vector3(Vec3::new(0.0, 0.0, -1.0));
        assert!((front - Vec3::Z).length() < 1e-5, "{front}");
        let m = root_matrix(Vec3::ZERO, 90.0, 0.0);
        let front = m.transform_vector3(Vec3::new(0.0, 0.0, -1.0));
        assert!((front - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5, "yaw 90 faces west: {front}");
        // The model's feet (y = 24 units) land a thousandth over the ground (the 1.501 drop).
        let foot = m.transform_point3(Vec3::new(0.0, 24.0 / 16.0, 0.0));
        assert!((foot.y - 0.001).abs() < 1e-5, "{foot}");
    }

    #[test]
    fn a_dead_cow_lies_on_its_side_a_quarter_turn_about_z() {
        // Fully fallen (90 degrees): the model's vertical axis lies along the world's x axis.
        let m = root_matrix(Vec3::ZERO, 0.0, 90.0);
        let down = m.transform_vector3(Vec3::Y);
        assert!(down.y.abs() < 1e-5 && down.x.abs() > 0.99, "the model's down axis lies flat: {down}");
        let upright = root_matrix(Vec3::ZERO, 0.0, 0.0).transform_vector3(Vec3::Y);
        assert!((upright - Vec3::new(0.0, -1.0, 0.0)).length() < 1e-5, "alive, model +y is world down (the flip): {upright}");
    }

    #[test]
    fn sound_pitch_spreads_around_one_and_a_half_for_a_child() {
        assert_eq!(sound_pitch(false, 0.5, 0.5), 1.0);
        assert!((sound_pitch(false, 1.0, 0.0) - 1.2).abs() < 1e-6);
        assert!((sound_pitch(true, 0.0, 1.0) - 1.3).abs() < 1e-6);
        assert_eq!(sound_pitch(true, 0.25, 0.25), 1.5);
    }

    #[test]
    fn the_hit_box_is_the_set_size_box_halved_for_a_child() {
        let def = &super::super::models::MOBS[0];
        let spawn = proto::SpawnMob { pos: [10.0, 64.0, -3.0], ..Default::default() };
        let mut mob = Mob { id: 1, living: Living::spawned(&spawn), def, child: false, flags: 0, wool: 0, anim: Anim::default(), parts: vec![], material: Handle::default(), layers: vec![], main_hand: None, fire: None };
        let b = mob.bb();
        assert_eq!((b.min_x, b.max_x, b.min_y, b.max_y), (10.0 - 0.45f32 as f64, 10.0 + 0.45f32 as f64, 64.0, 64.0 + 1.4f32 as f64));
        mob.child = true;
        let b = mob.bb();
        assert!((b.max_y - b.min_y - 0.7).abs() < 1e-6 && (b.max_z - b.min_z - 0.45).abs() < 1e-6);
    }

    #[test]
    fn a_hand_sent_with_the_spawn_waits_for_the_mob_component() {
        // The spawn and the bow arrive in one frame, before the deferred insert of `Mob`.
        let mut mobs = Mobs::default();
        let bow = Some((261, 0));
        assert_eq!(mobs.queue_hand(7, bow, false), None, "unknown id: nothing kept");
        assert!(mobs.take_pending().is_empty());
        mobs.by_id.insert(7, Entity::PLACEHOLDER);
        assert_eq!(mobs.queue_hand(7, bow, false), None);
        assert_eq!(mobs.take_pending(), vec![(7, bow)]);
        assert_eq!(mobs.queue_hand(7, bow, true), Some(bow));
    }

    #[test]
    fn render_range_is_sixty_four_average_edges() {
        let size = (0.9, 1.4);
        assert!(in_render_range(68.0 * 68.0, size));
        assert!(!in_render_range(69.0 * 69.0, size));
    }
}
