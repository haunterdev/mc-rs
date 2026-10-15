//! Taking damage on the session's own player: the client-side health bookkeeping of
//! `EntityPlayerSP.setPlayerSPHealth` and `EntityLivingBase.handleStatusUpdate` ([`Vitals`]:
//! `hurtTime`, `hurtResistantTime`, `deathTime`), the camera tilt of
//! `EntityRenderer.hurtCameraEffect`, the hurt and death sounds, `S_VELOCITY` knockback, and
//! the food level the sprint rule reads.
//!
//! The frames reach here as the typed messages of survival/mod.rs, so a native and a streamed
//! session behave the same.
//!
//! DROPPED: the hand's own tilt (`renderHand` applies `hurtCameraEffect` to the arm too), the
//! limb swing a hit sets (`limbSwingAmount`, 1.5), and the other entities' hurt flashes.

use bevy::prelude::*;
use worldgen::math_helper::sin;

use super::{Death, EntityEvent, Health, SurvivalState, Velocity};
use crate::audio::{Audio, Category};
use crate::link::Link;
use crate::player::Player;

pub fn plugin(app: &mut App) {
    app.init_resource::<Vitals>().add_systems(
        Update,
        (vitals_in, sounds, player_in).before(crate::player::run_ticks).run_if(resource_exists::<Player>),
    );
}

/// `EntityLivingBase.maxHurtResistantTime` of a player.
const MAX_HURT_RESISTANT_TIME: i32 = 20;

/// The client entity's hurt and death counters.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Vitals {
    pub health: f32,
    /// `EntityPlayerSP.hasValidHealth`: the first `S_HEALTH` only sets the health.
    has_valid_health: bool,
    pub hurt_time: i32,
    pub max_hurt_time: i32,
    pub hurt_resistant_time: i32,
    pub death_time: i32,
    pub attacked_at_yaw: f32,
}

impl Default for Vitals {
    fn default() -> Vitals {
        Vitals { health: 20.0, has_valid_health: false, hurt_time: 0, max_hurt_time: 0, hurt_resistant_time: 0, death_time: 0, attacked_at_yaw: 0.0 }
    }
}

impl Vitals {
    /// `EntityPlayerSP.setPlayerSPHealth`: a drop starts the hurt tilt and the resistance window,
    /// a rise halves the window. DROPPED: `lastDamage` and the armor math of `damageEntity`,
    /// which only reach the new health already set.
    pub fn set_health(&mut self, health: f32) {
        if self.has_valid_health {
            let f = self.health - health;
            if f <= 0.0 {
                self.health = health;
                if f < 0.0 {
                    self.hurt_resistant_time = MAX_HURT_RESISTANT_TIME / 2;
                }
            } else {
                self.health = health;
                self.hurt_resistant_time = MAX_HURT_RESISTANT_TIME;
                self.max_hurt_time = 10;
                self.hurt_time = self.max_hurt_time;
            }
        } else {
            self.health = health;
            self.has_valid_health = true;
        }
    }

    /// `handleStatusUpdate` 2, 33, 36 and 37.
    pub fn hurt(&mut self) {
        self.hurt_resistant_time = MAX_HURT_RESISTANT_TIME;
        self.max_hurt_time = 10;
        self.hurt_time = self.max_hurt_time;
        self.attacked_at_yaw = 0.0;
    }

    /// One client tick: `EntityLivingBase.onLivingUpdate` counts `hurtTime` down, `onEntityUpdate`
    /// the resistance window, and `onDeathUpdate` counts `deathTime` to 20, where the entity is
    /// removed and the count stops.
    pub fn tick(&mut self) {
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.hurt_resistant_time > 0 {
            self.hurt_resistant_time -= 1;
        }
        if self.health <= 0.0 && self.death_time < 20 {
            self.death_time += 1;
        }
    }
}

/// `EntityRenderer.hurtCameraEffect` as a matrix on the modelview: the death roll, then the
/// hurt roll about the view's Z, turned by `attackedAtYaw`. Identity when nothing applies.
pub fn camera_effect(v: &Vitals, partial: f32) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    let mut f = v.hurt_time as f32 - partial;
    if v.health <= 0.0 {
        let f1 = v.death_time as f32 + partial;
        m *= Mat4::from_rotation_z((40.0 - 8000.0 / (f1 + 200.0)).to_radians());
    }
    // DROPPED: Java divides by maxHurtTime even at 0 (NaN at an exact partial 0); nothing is
    // hurt then.
    if f < 0.0 || v.max_hurt_time == 0 {
        return m;
    }
    f /= v.max_hurt_time as f32;
    f = sin(f * f * f * f * std::f32::consts::PI);
    let yaw = v.attacked_at_yaw;
    m * Mat4::from_rotation_y((-yaw).to_radians()) * Mat4::from_rotation_z((-f * 14.0).to_radians()) * Mat4::from_rotation_y(yaw.to_radians())
}

/// The session player's entity id.
fn own_id(link: Option<&Link>) -> Option<u32> {
    link.map(|l| l.welcome.id)
}

/// `S_HEALTH` into the hurt counters, `EV_HURT*` into the hurt animation.
fn vitals_in(mut vitals: ResMut<Vitals>, mut health: MessageReader<Health>, mut events: MessageReader<EntityEvent>, link: Option<Res<Link>>) {
    for Health(h) in health.read() {
        // A respawn is a new `EntityPlayerSP`: nothing carries over.
        if vitals.has_valid_health && vitals.health <= 0.0 && h.health > 0.0 {
            *vitals = Vitals::default();
        }
        vitals.set_health(h.health);
    }
    let Some(me) = own_id(link.as_deref()) else { return };
    for EntityEvent(e) in events.read() {
        if e.id == me && matches!(e.status, proto::EV_HURT | proto::EV_HURT_DROWN | proto::EV_HURT_BURN) {
            vitals.hurt();
        }
    }
}

/// The sound a status plays: `EntityPlayer.getHurtSound` and `getDeathSound`.
pub fn sound_of(status: u8) -> Option<&'static str> {
    match status {
        proto::EV_HURT => Some("entity.player.hurt"),
        proto::EV_HURT_DROWN => Some("entity.player.hurt_drown"),
        proto::EV_HURT_BURN => Some("entity.player.hurt_on_fire"),
        proto::EV_DEATH => Some("entity.player.death"),
        _ => None,
    }
}

/// Plays the hurt sound for the player's own statuses, and the death sound on `S_DEATH` (the
/// server sends the dying player no status 3). Volume 1, pitch `(r - r) * 0.2 + 1`.
fn sounds(mut events: MessageReader<EntityEvent>, mut deaths: MessageReader<Death>, link: Option<Res<Link>>, audio: Option<Res<Audio>>, player: Res<Player>) {
    let Some(audio) = audio else {
        events.clear();
        deaths.clear();
        return;
    };
    let me = own_id(link.as_deref());
    let at = Some(player.pos.as_vec3());
    let pitch = |a: &Audio| (a.next_float() - a.next_float()) * 0.2 + 1.0;
    for EntityEvent(e) in events.read() {
        if Some(e.id) == me && e.status != proto::EV_DEATH {
            if let Some(name) = sound_of(e.status) {
                audio.play(name, Category::Players, at, 1.0, pitch(&audio));
            }
        }
    }
    if deaths.read().next().is_some() {
        audio.play("entity.player.death", Category::Players, at, 1.0, pitch(&audio));
    }
}

/// `S_VELOCITY` for the player sets its motion, and the food level feeds the sprint rule.
fn player_in(mut velocity: MessageReader<Velocity>, link: Option<Res<Link>>, state: Res<SurvivalState>, mut player: ResMut<Player>) {
    if player.food_level != state.food as i32 {
        player.food_level = state.food as i32;
    }
    let Some(me) = own_id(link.as_deref()) else { return };
    for Velocity(v) in velocity.read() {
        if v.id == me {
            player.set_velocity(v.motion);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_health_only_sets_it_and_a_drop_starts_the_hurt() {
        let mut v = Vitals::default();
        v.set_health(20.0);
        assert_eq!((v.health, v.hurt_time, v.hurt_resistant_time), (20.0, 0, 0));
        v.set_health(17.0);
        assert_eq!((v.health, v.hurt_time, v.max_hurt_time, v.hurt_resistant_time), (17.0, 10, 10, 20));
        // Healing opens half a window and no hurt.
        let mut v = Vitals::default();
        v.set_health(10.0);
        v.set_health(12.0);
        assert_eq!((v.health, v.hurt_time, v.hurt_resistant_time), (12.0, 0, 10));
        v.set_health(12.0);
        assert_eq!(v.hurt_resistant_time, 10);
    }

    #[test]
    fn the_counters_run_down_one_a_tick_and_death_stops_at_twenty() {
        let mut v = Vitals::default();
        v.set_health(20.0);
        v.set_health(15.0);
        for _ in 0..4 {
            v.tick();
        }
        assert_eq!((v.hurt_time, v.hurt_resistant_time), (6, 16));
        for _ in 0..20 {
            v.tick();
        }
        assert_eq!((v.hurt_time, v.hurt_resistant_time, v.death_time), (0, 0, 0));
        v.set_health(0.0);
        for _ in 0..30 {
            v.tick();
        }
        assert_eq!(v.death_time, 20);
    }

    #[test]
    fn the_hurt_tilt_peaks_a_little_after_the_hit_and_dies_away() {
        let mut v = Vitals::default();
        assert_eq!(camera_effect(&v, 0.5), Mat4::IDENTITY);
        v.hurt();
        let roll = |v: &Vitals, partial: f32| {
            let m = camera_effect(v, partial);
            // The roll about Z: atan2 of the rotated X axis.
            m.transform_vector3(Vec3::X).y.atan2(m.transform_vector3(Vec3::X).x).to_degrees()
        };
        // f = (hurtTime - partial) / 10; sin(f^4 * pi) * -14 degrees.
        let at = |t: f32| -14.0 * sin((t / 10.0).powi(4) * std::f32::consts::PI);
        assert!((roll(&v, 0.0) - at(10.0)).abs() < 1e-3, "{}", roll(&v, 0.0));
        // Right after the hit the tilt is small, near the end of the swing it is at its largest.
        let mut samples = Vec::new();
        for _ in 0..10 {
            samples.push(roll(&v, 0.0).abs());
            v.tick();
        }
        let max = samples.iter().cloned().fold(0.0, f32::max);
        assert!(max > 13.0 && max <= 14.0 + 1e-3, "{samples:?}");
        assert!(samples[9] < max * 0.01 + 0.01, "settles at the end: {samples:?}");
        assert_eq!(camera_effect(&v, 0.0), Mat4::IDENTITY, "nothing left once hurtTime is 0");
    }

    #[test]
    fn death_rolls_the_view_toward_forty_degrees() {
        let mut v = Vitals::default();
        v.set_health(20.0);
        v.set_health(0.0);
        let roll = |m: Mat4| m.transform_vector3(Vec3::X).y.atan2(m.transform_vector3(Vec3::X).x).to_degrees();
        for _ in 0..30 {
            v.tick();
        }
        // deathTime 20: 40 - 8000 / 220 = 3.636 degrees, plus the last of the hurt.
        v.hurt_time = 0;
        let r = roll(camera_effect(&v, 0.0));
        assert!((r - (40.0 - 8000.0 / 220.0)).abs() < 1e-3, "{r}");
        let later = Vitals { death_time: 20, ..v.clone() };
        assert!(roll(camera_effect(&later, 1.0)) > r);
    }

    #[test]
    fn a_respawn_starts_clean_and_the_status_ids_map_to_sounds() {
        assert_eq!(sound_of(proto::EV_HURT), Some("entity.player.hurt"));
        assert_eq!(sound_of(proto::EV_HURT_DROWN), Some("entity.player.hurt_drown"));
        assert_eq!(sound_of(proto::EV_HURT_BURN), Some("entity.player.hurt_on_fire"));
        assert_eq!(sound_of(proto::EV_DEATH), Some("entity.player.death"));
        assert_eq!(sound_of(proto::EV_EAT_GRASS), None);
    }

    /// The frames, through the real systems: health, a hurt status for this player and not for
    /// another, knockback, and the food level reaching the player.
    #[test]
    fn frames_reach_the_vitals_and_the_player() {
        use crate::link::Welcome;
        use crate::survival::{Inbox, SurvivalPlugin};
        use bevy::math::DVec3;
        use crossbeam_channel::unbounded;
        use std::sync::{atomic::AtomicBool, Arc};
        let (out, _rx) = unbounded();
        let (_tx, inbox) = unbounded();
        let link = Link::over(out, inbox, Arc::new(AtomicBool::new(false)), Welcome { id: 3, ..Default::default() });
        let mut app = App::new();
        app.add_plugins(SurvivalPlugin).insert_resource(link).insert_resource(Player::new(DVec3::ZERO, 0.0, 0.0));
        let health = |h: f32, food: u8| proto::Health { health: h, food, saturation: 5.0, air: 300, flags: 0, absorption: 0.0 };
        app.world_mut().resource_mut::<Inbox>().0.push((proto::S_HEALTH, health(20.0, 20).encode()));
        app.update();
        app.world_mut().resource_mut::<Inbox>().0.extend([
            (proto::S_HEALTH, health(16.0, 4).encode()),
            (proto::S_ENTITY_EVENT, proto::EntityEvent { id: 9, status: proto::EV_HURT }.encode()),
            (proto::S_VELOCITY, proto::Velocity { id: 9, motion: [1.0, 1.0, 1.0] }.encode()),
            (proto::S_VELOCITY, proto::Velocity { id: 3, motion: [0.25, 0.5, -0.25] }.encode()),
        ]);
        app.update();
        let v = app.world().resource::<Vitals>();
        assert_eq!((v.health, v.hurt_time, v.hurt_resistant_time), (16.0, 10, 20));
        let p = app.world().resource::<Player>();
        assert_eq!(p.motion, DVec3::new(0.25, 0.5, -0.25));
        assert_eq!(p.food_level, 4);
        // Death, then the respawn's health wipes the counters.
        app.world_mut().resource_mut::<Inbox>().0.extend([
            (proto::S_HEALTH, health(0.0, 4).encode()),
            (proto::S_DEATH, proto::Death { message: "x".into() }.encode()),
        ]);
        app.update();
        assert!(app.world().resource::<SurvivalState>().dead);
        app.world_mut().resource_mut::<Inbox>().0.push((proto::S_ENTITY_EVENT, proto::EntityEvent { id: 3, status: proto::EV_HURT_DROWN }.encode()));
        app.update();
        assert_eq!(app.world().resource::<Vitals>().hurt_time, 10);
        app.world_mut().resource_mut::<Inbox>().0.push((proto::S_HEALTH, health(20.0, 20).encode()));
        app.update();
        let v = app.world().resource::<Vitals>();
        assert_eq!((v.health, v.hurt_time, v.death_time), (20.0, 0, 0));
    }
}
