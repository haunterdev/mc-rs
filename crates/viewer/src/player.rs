//! The player: a creative `EntityPlayerSP` over a [`Body`] (`sim::entity`, the `Entity` and
//! `EntityLivingBase` movement). Here: the keys and `MovementInput`, the sprint and fly toggles of
//! `EntityPlayerSP.onLivingUpdate`, `EntityPlayer.travel`, auto-jump (auto_jump.rs), the sneaking
//! size, and the walk bobbing `EntityRenderer.applyBobbing` draws from.
//!
//! Positions and motion are doubles and the angles and speeds floats, as in the game, so the
//! arithmetic rounds the same way.

use std::ops::{Deref, DerefMut};

use bevy::math::DVec3;
use bevy::prelude::*;
use sim::entity::{travel_living, Body, Living};
use worldgen::math_helper::{cos, sin};

use crate::collision::{collides_with_any_block, floor, Aabb, BlockAccess};
use crate::hand::Hand;
use crate::hotbar::Inventory;
use crate::input::Focus;
use crate::interact::{self, Controller};
use crate::settings::{Menu, Settings};
use crate::sky::{self, Clock, View};
use crate::stream::Streamer;
use crate::teleport::{self, Landing};

pub use sim::entity::Sound as PlayerSound;

mod auto_jump;

/// `EntityPlayer.speedInAir`
const SPEED_IN_AIR: f32 = 0.02;
/// The `MOVEMENT_SPEED` attribute's base for a player.
const MOVEMENT_SPEED: f64 = 0.100_000_001_490_116_12;
/// `EntityLivingBase.SPRINTING_SPEED_BOOST`, operation 2 (multiply total).
const SPRINT_BOOST: f64 = 0.300_000_011_920_928_96;

/// The keys `MovementInputFromOptions` and `EntityPlayerSP` read (`KeyBinding.isKeyDown`).
#[derive(Clone, Copy, Default, Debug)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

/// `MovementInput`
#[derive(Clone, Copy, Default, Debug)]
pub struct MovementInput {
    pub strafe: f32,
    pub forward: f32,
    pub jump: bool,
    pub sneak: bool,
}

impl MovementInput {
    /// `MovementInputFromOptions.updatePlayerMoveState`
    fn update(&mut self, k: &Keys) {
        self.strafe = 0.0;
        self.forward = 0.0;
        if k.forward {
            self.forward += 1.0;
        }
        if k.back {
            self.forward -= 1.0;
        }
        if k.left {
            self.strafe += 1.0;
        }
        if k.right {
            self.strafe -= 1.0;
        }
        self.jump = k.jump;
        self.sneak = k.sneak;
        if self.sneak {
            self.strafe = (self.strafe as f64 * 0.3) as f32;
            self.forward = (self.forward as f64 * 0.3) as f32;
        }
    }
}

/// `Entity.getVectorForRotation` (and `Vec3d.fromPitchYaw`): the look vector, over the sine
/// table.
pub fn look(pitch: f32, yaw: f32) -> Vec3 {
    let f = cos(-yaw * 0.017_453_292 - 3.141_592_7);
    let f1 = sin(-yaw * 0.017_453_292 - 3.141_592_7);
    let f2 = -cos(-pitch * 0.017_453_292);
    let f3 = sin(-pitch * 0.017_453_292);
    Vec3::new(f1 * f2, f3, f * f2)
}

/// The player. Its [`Body`] (position, box, motion, rotation, ground and water state, step
/// sounds) is reached through `Deref`, so `player.pos` and `player.motion` read the body.
#[derive(Resource, Clone, Debug)]
pub struct Player {
    pub body: Body,
    pub camera_yaw: f32,
    pub prev_camera_yaw: f32,
    pub camera_pitch: f32,
    pub prev_camera_pitch: f32,
    jump_ticks: i32,
    is_jumping: bool,
    move_strafing: f32,
    move_forward: f32,
    jump_movement_factor: f32,
    pub sprinting: bool,
    /// `PlayerCapabilities.isFlying`, `flySpeed`, `walkSpeed`; `allowFlying` comes from the game
    /// mode ([`Player::allow_flying`]).
    pub flying: bool,
    pub fly_speed: f32,
    walk_speed: f32,
    fly_toggle_timer: i32,
    sprint_toggle_timer: i32,
    sprinting_ticks_left: i32,
    auto_jump_time: i32,
    /// `GameSettings.autoJump`
    pub auto_jump: bool,
    pub input: MovementInput,
    /// Whether the chunk under the spawn point has loaded; until then the player does not tick
    /// (it stands in for `GuiDownloadTerrain`). A teleport from the server clears it again.
    pub spawned: bool,
    /// How the player comes down once that chunk has loaded (teleport.rs).
    pub landing: Landing,
    /// `PlayerControllerMP.currentGameType`, as the server sets it: `proto::GM_*`.
    pub game_mode: u8,
    /// `WorldInfo.difficulty` id, as `S_DIFFICULTY` last sent it.
    pub difficulty: u8,
    /// `FoodStats.getFoodLevel`, as the server last sent it (`S_HEALTH`); sprinting needs more
    /// than 6 unless the game mode allows flying.
    pub food_level: i32,
    /// `EntityPlayerSP.isHandActive`: a bow is being drawn (survival/bow.rs).
    pub using_item: bool,
    /// `EntityLivingBase.activePotionsMap`, as the server last said (survival/effects.rs).
    pub effects: items::potion::Effects,
}

impl Deref for Player {
    type Target = Body;
    fn deref(&self) -> &Body {
        &self.body
    }
}

impl DerefMut for Player {
    fn deref_mut(&mut self) -> &mut Body {
        &mut self.body
    }
}

impl Living for Player {
    fn body(&mut self) -> &mut Body {
        &mut self.body
    }

    fn move_self(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64) {
        self.move_entity(w, x, y, z);
    }
}

impl Player {
    pub fn new(feet: DVec3, yaw: f32, pitch: f32) -> Player {
        Player {
            body: Body::new(feet, yaw, pitch),
            camera_yaw: 0.0,
            prev_camera_yaw: 0.0,
            camera_pitch: 0.0,
            prev_camera_pitch: 0.0,
            jump_ticks: 0,
            is_jumping: false,
            move_strafing: 0.0,
            move_forward: 0.0,
            jump_movement_factor: SPEED_IN_AIR,
            sprinting: false,
            flying: false,
            fly_speed: 0.05,
            walk_speed: 0.1,
            fly_toggle_timer: 0,
            sprint_toggle_timer: 0,
            sprinting_ticks_left: 0,
            auto_jump_time: 0,
            auto_jump: true,
            input: MovementInput::default(),
            spawned: false,
            landing: Landing { on_top: false, rise: true },
            game_mode: proto::GM_CREATIVE,
            difficulty: 2,
            food_level: 20,
            using_item: false,
            effects: items::potion::Effects::new(),
        }
    }

    /// `NetHandlerPlayClient.handleEntityVelocity` for the session's own player
    /// (`Entity.setVelocity`): the motion becomes what the server sent, blocks a tick.
    pub fn set_velocity(&mut self, motion: [f32; 3]) {
        self.body.motion = DVec3::new(motion[0] as f64, motion[1] as f64, motion[2] as f64);
    }

    /// `EntityPlayerSP.onLivingUpdate`'s `flag4`: enough food to sprint, or a game mode that
    /// allows flying.
    pub fn can_sprint(&self) -> bool {
        self.food_level as f32 > 6.0 || self.allow_flying()
    }

    /// Moves the player without interpolation or momentum (the smoke run's camera path).
    pub fn teleport(&mut self, feet: DVec3, yaw: f32, pitch: f32) {
        self.body.set_position(feet);
        self.body.prev_pos = feet;
        self.body.motion = DVec3::ZERO;
        let b = &mut self.body;
        (b.yaw, b.prev_yaw, b.pitch, b.prev_pitch) = (yaw, yaw, pitch, pitch);
    }

    /// `PlayerCapabilities.allowFlying` as `GameType.configurePlayerCapabilities` sets it.
    pub fn allow_flying(&self) -> bool {
        matches!(self.game_mode, proto::GM_CREATIVE | proto::GM_SPECTATOR)
    }

    /// `EntityPlayerSP.isSneaking`
    pub fn is_sneaking(&self) -> bool {
        self.input.sneak
    }

    /// `EntityPlayer.getEyeHeight`
    pub fn eye_height(&self) -> f32 {
        if self.is_sneaking() || self.body.height == 1.65 {
            1.62 - 0.08
        } else {
            1.62
        }
    }

    /// The `MOVEMENT_SPEED` attribute's value.
    fn speed_attribute(&self) -> f64 {
        // Speed and Slowness are operation 2 modifiers, like the sprint boost.
        let base = MOVEMENT_SPEED * self.effects.movement_speed_factor();
        if self.sprinting {
            base * (1.0 + SPRINT_BOOST)
        } else {
            base
        }
    }

    /// `EntityPlayer.getAIMoveSpeed`
    pub fn ai_move_speed(&self) -> f32 {
        self.speed_attribute() as f32
    }

    /// `AbstractClientPlayer.getFovModifier`
    /// DROPPED: the bow zoom; the hand is empty.
    pub fn fov_modifier(&self) -> f32 {
        let mut f = 1.0f32;
        if self.flying {
            f *= 1.1;
        }
        (f as f64 * ((self.speed_attribute() / self.walk_speed as f64 + 1.0) / 2.0)) as f32
    }

    /// `EntityPlayerSP.setSprinting`
    fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
        self.sprinting_ticks_left = 0;
    }

    /// `Entity.turn`: the mouse, per frame.
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        let b = &mut self.body;
        let f = b.pitch;
        let f1 = b.yaw;
        b.yaw = (b.yaw as f64 + yaw as f64 * 0.15) as f32;
        b.pitch = (b.pitch as f64 - pitch as f64 * 0.15) as f32;
        b.pitch = b.pitch.clamp(-90.0, 90.0);
        b.prev_pitch += b.pitch - f;
        b.prev_yaw += b.yaw - f1;
    }

    /// The feet between the last two ticks.
    pub fn interp_pos(&self, partial: f32) -> DVec3 {
        self.body.prev_pos + (self.body.pos - self.body.prev_pos) * partial as f64
    }

    pub fn interp_pitch(&self, partial: f32) -> f32 {
        self.body.prev_pitch + (self.body.pitch - self.body.prev_pitch) * partial
    }

    pub fn interp_yaw(&self, partial: f32) -> f32 {
        self.body.prev_yaw + (self.body.yaw - self.body.prev_yaw) * partial
    }

    /// `EntityRenderer.applyBobbing` as a matrix, applied before the camera's own rotation.
    pub fn bobbing(&self, partial: f32) -> Mat4 {
        let pi = 3.141_592_7f32;
        let f = self.body.distance_walked_modified - self.body.prev_distance_walked_modified;
        // Extrapolated from this tick, not interpolated from the last one, as vanilla does.
        let f1 = -(self.body.distance_walked_modified + f * partial);
        let f2 = self.prev_camera_yaw + (self.camera_yaw - self.prev_camera_yaw) * partial;
        let f3 = self.prev_camera_pitch + (self.camera_pitch - self.prev_camera_pitch) * partial;
        let rx = |d: f32| Mat4::from_rotation_x(d.to_radians());
        let rz = |d: f32| Mat4::from_rotation_z(d.to_radians());
        Mat4::from_translation(Vec3::new(sin(f1 * pi) * f2 * 0.5, -(cos(f1 * pi) * f2).abs(), 0.0))
            * rz(sin(f1 * pi) * f2 * 3.0)
            * rx((cos(f1 * pi - 0.2) * f2).abs() * 5.0)
            * rx(f3)
    }

    /// `EntityPlayerSP.move`: `Entity.move(MoverType.SELF, ...)` (`Body::move_entity`, which
    /// walks only when not flying), then `updateAutoJump`.
    pub fn move_entity(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64) {
        let (d10, d1) = (self.body.pos.x, self.body.pos.z);
        let sneaking = self.is_sneaking();
        self.body.move_entity(w, x, y, z, sneaking, !self.flying);
        self.update_auto_jump(w, (self.body.pos.x - d10) as f32, (self.body.pos.z - d1) as f32);
    }

    /// `EntityPlayer.travel`, then `EntityLivingBase.travel` ([`travel_living`]).
    fn travel(&mut self, w: &impl BlockAccess, strafe: f32, vertical: f32, forward: f32) {
        if self.flying {
            let d3 = self.body.motion.y;
            let f = self.jump_movement_factor;
            self.jump_movement_factor = self.fly_speed * if self.sprinting { 2.0 } else { 1.0 };
            let (speed, jump_factor) = (self.ai_move_speed(), self.jump_movement_factor);
            travel_living(self, w, strafe, vertical, forward, true, speed, jump_factor);
            self.body.motion.y = d3 * 0.6;
            self.jump_movement_factor = f;
        } else {
            let (speed, jump_factor) = (self.ai_move_speed(), self.jump_movement_factor);
            travel_living(self, w, strafe, vertical, forward, false, speed, jump_factor);
        }
        // DROPPED: addMovementStat.
    }

    /// `EntityLivingBase.jump`
    fn jump(&mut self) {
        // getJumpUpwardsMotion is the float 0.42.
        self.body.motion.y = 0.42f32 as f64;
        // Jump Boost adds (amplifier + 1) * 0.1.
        let boost = self.effects.jump_boost_level();
        if boost > 0.0 {
            self.body.motion.y += (boost * 0.1f32) as f64;
        }
        if self.sprinting {
            let f = self.body.yaw * 0.017_453_292;
            self.body.motion.x -= (sin(f) * 0.2) as f64;
            self.body.motion.z += (cos(f) * 0.2) as f64;
        }
    }

    /// `EntityPlayer.updateSize` and `Entity.setSize`: crouched 1.65 high, when the room is there.
    /// DROPPED: the elytra and sleeping sizes.
    fn update_size(&mut self, w: &impl BlockAccess) {
        let (f, f1) = if self.is_sneaking() { (0.6f32, 1.65f32) } else { (0.6, 1.8) };
        let body = &mut self.body;
        if f != body.width || f1 != body.height {
            let b = body.bb;
            let grown = Aabb::new(b.min_x, b.min_y, b.min_z, b.min_x + f as f64, b.min_y + f1 as f64, b.min_z + f as f64);
            if !collides_with_any_block(w, &grown) {
                let old = body.width;
                body.width = f;
                body.height = f1;
                if body.width < old {
                    let d0 = f as f64 / 2.0;
                    let p = body.pos;
                    body.bb = Aabb::new(p.x - d0, p.y, p.z - d0, p.x + d0, p.y + f1 as f64, p.z + d0);
                } else {
                    body.bb = Aabb::new(b.min_x, b.min_y, b.min_z, b.min_x + f as f64, b.min_y + f1 as f64, b.min_z + f as f64);
                }
            }
        }
    }

    /// One tick of `EntityPlayerSP.onUpdate`: `Entity.onEntityUpdate`, the `onLivingUpdate` chain
    /// of `EntityPlayerSP`, `EntityPlayer` and `EntityLivingBase`, then `updateSize`.
    /// DROPPED: the network updates, health, hunger, items and everything the hand already ticks.
    pub fn tick(&mut self, w: &impl BlockAccess, keys: &Keys) {
        // EntityLivingBase.updatePotionEffects: the durations count down.
        self.effects.tick();
        // Entity.onEntityUpdate
        self.body.prev_distance_walked_modified = self.body.distance_walked_modified;
        self.body.prev_pos = self.body.pos;
        self.body.prev_pitch = self.body.pitch;
        self.body.prev_yaw = self.body.yaw;
        self.body.handle_water_movement(w);
        // DROPPED: lava fire and the void below -64.
        // EntityLivingBase.onEntityUpdate
        self.prev_camera_pitch = self.camera_pitch;

        // EntityPlayerSP.onLivingUpdate
        self.sprinting_ticks_left += 1;
        if self.sprint_toggle_timer > 0 {
            self.sprint_toggle_timer -= 1;
        }
        // DROPPED: the portal overlay.
        let flag = self.input.jump;
        let flag1 = self.input.sneak;
        let flag2 = self.input.forward >= 0.8;
        self.input.update(keys);
        // DROPPED: the tutorial.
        if self.using_item {
            // isHandActive && !isRiding: the item-use slowdown.
            self.input.strafe *= 0.2;
            self.input.forward *= 0.2;
            self.sprint_toggle_timer = 0;
        }
        let mut flag3 = false;
        if self.auto_jump_time > 0 {
            self.auto_jump_time -= 1;
            flag3 = true;
            self.input.jump = true;
        }
        let bb = self.body.bb;
        let d = self.body.width as f64 * 0.35;
        let (x, y, z) = (self.body.pos.x, bb.min_y + 0.5, self.body.pos.z);
        self.body.push_out_of_blocks(w, x - d, y, z + d);
        self.body.push_out_of_blocks(w, x - d, y, z - d);
        self.body.push_out_of_blocks(w, x + d, y, z - d);
        self.body.push_out_of_blocks(w, x + d, y, z + d);
        let flag4 = self.can_sprint();
        if self.body.on_ground && !flag1 && !flag2 && self.input.forward >= 0.8 && !self.sprinting && flag4 {
            if self.sprint_toggle_timer <= 0 && !keys.sprint {
                self.sprint_toggle_timer = 7;
            } else {
                self.set_sprinting(true);
            }
        }
        if !self.sprinting && self.input.forward >= 0.8 && flag4 && keys.sprint {
            self.set_sprinting(true);
        }
        if self.sprinting && (self.input.forward < 0.8 || self.body.collided_horizontally || !flag4) {
            self.set_sprinting(false);
        }
        if self.allow_flying() {
            if self.game_mode == proto::GM_SPECTATOR {
                self.flying = true;
            } else if !flag && self.input.jump && !flag3 {
                if self.fly_toggle_timer == 0 {
                    self.fly_toggle_timer = 7;
                } else {
                    self.flying = !self.flying;
                    self.fly_toggle_timer = 0;
                }
            }
        }
        // DROPPED: elytra and horses.
        if self.flying {
            if self.input.sneak {
                self.input.strafe = (self.input.strafe as f64 / 0.3) as f32;
                self.input.forward = (self.input.forward as f64 / 0.3) as f32;
                self.body.motion.y -= (self.fly_speed * 3.0) as f64;
            }
            if self.input.jump {
                self.body.motion.y += (self.fly_speed * 3.0) as f64;
            }
        }

        // EntityPlayer.onLivingUpdate
        if self.fly_toggle_timer > 0 {
            self.fly_toggle_timer -= 1;
        }
        self.prev_camera_yaw = self.camera_yaw;

        // EntityLivingBase.onLivingUpdate. EntityPlayerSP is a server world entity here, so its
        // motion is not damped by 0.98.
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        for m in [&mut self.body.motion.x, &mut self.body.motion.y, &mut self.body.motion.z] {
            if m.abs() < 0.003 {
                *m = 0.0;
            }
        }
        // EntityPlayerSP.updateEntityActionState
        self.move_strafing = self.input.strafe;
        self.move_forward = self.input.forward;
        self.is_jumping = self.input.jump;
        if self.is_jumping {
            if self.body.in_water || self.body.is_in_lava(w) {
                // handleJumpWater, handleJumpLava
                self.body.motion.y += 0.039_999_999_105_930_33;
            } else if self.body.on_ground && self.jump_ticks == 0 {
                self.jump();
                self.jump_ticks = 10;
            }
        } else {
            self.jump_ticks = 0;
        }
        self.move_strafing *= 0.98;
        self.move_forward *= 0.98;
        self.travel(w, self.move_strafing, 0.0, self.move_forward);
        // DROPPED: collideWithNearbyEntities.

        // The rest of EntityPlayer.onLivingUpdate.
        self.jump_movement_factor = SPEED_IN_AIR;
        if self.sprinting {
            self.jump_movement_factor = (self.jump_movement_factor as f64 + SPEED_IN_AIR as f64 * 0.3) as f32;
        }
        let m = self.body.motion;
        let mut f = (m.x * m.x + m.z * m.z).sqrt() as f32;
        let mut f1 = ((-m.y * 0.200_000_002_980_232_24).atan() * 15.0) as f32;
        if f > 0.1 {
            f = 0.1;
        }
        if !self.body.on_ground {
            f = 0.0;
        }
        if self.body.on_ground {
            f1 = 0.0;
        }
        self.camera_yaw += (f - self.camera_yaw) * 0.4;
        self.camera_pitch += (f1 - self.camera_pitch) * 0.8;
        // DROPPED: picking up items and touching entities.

        // The rest of EntityPlayerSP.onLivingUpdate.
        if self.body.on_ground && self.flying && self.game_mode != proto::GM_SPECTATOR {
            self.flying = false;
        }

        // EntityPlayer.onUpdate
        self.update_size(w);
    }
}

/// Runs this frame's client ticks in `Minecraft.runTick` order: the mouse and keys on the world
/// (`interact::tick`), `EntityRenderer.updateRenderer`, then the player and its hand.
#[allow(clippy::too_many_arguments)]
pub fn run_ticks(
    clock: Res<Clock>,
    settings: Res<Settings>,
    mut streamer: ResMut<Streamer>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    focus: Res<Focus>,
    menu: Res<Menu>,
    mut hand: ResMut<Hand>,
    mut ctl: ResMut<Controller>,
    mut inv: ResMut<Inventory>,
    mut view: ResMut<View>,
    mut player: ResMut<Player>,
) {
    hand.start(player.pitch, player.yaw);
    // An open screen takes the keys (setIngameNotInFocus releases them all).
    let active = !menu.open;
    let k = Keys {
        forward: active && keys.pressed(KeyCode::KeyW),
        back: active && keys.pressed(KeyCode::KeyS),
        left: active && keys.pressed(KeyCode::KeyA),
        right: active && keys.pressed(KeyCode::KeyD),
        jump: active && keys.pressed(KeyCode::Space),
        sneak: active && keys.pressed(KeyCode::ShiftLeft),
        sprint: active && keys.pressed(KeyCode::ControlLeft),
    };
    // sendClickBlockToController's argument.
    let attack_held = active && focus.in_game && mouse.pressed(MouseButton::Left);
    let use_held = active && mouse.pressed(MouseButton::Right);
    player.auto_jump = settings.auto_jump;
    for _ in 0..clock.ticks_this_frame {
        if player.spawned {
            interact::tick(&mut *streamer, &mut ctl, &mut hand, &mut inv, &player, attack_held, use_held);
        }
        streamer.tick_blocks();
        sky::renderer_tick(&mut view, &player, &streamer, &clock, &settings);
        if !player.spawned {
            player.spawned = streamer.info_at(floor(player.pos.x), floor(player.pos.z)).is_some();
            if player.spawned {
                teleport::land(&mut player, &*streamer);
            }
        }
        if player.spawned {
            player.tick(&*streamer, &k);
        }
        hand.end_tick(player.pitch, player.yaw);
    }
}

#[cfg(test)]
mod tests;
