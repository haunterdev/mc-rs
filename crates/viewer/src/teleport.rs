//! What the server does to the session's own player (crates/server/src/commands.rs): moves it
//! (`/tp`, `/teleport`, `/spawn`, the respawn after `/kill`, a new player's first spawn) as
//! `NetHandlerPlayClient.handlePlayerPosLook` does, and sets its game mode as
//! `GameType.configurePlayerCapabilities` does. The viewer owns the player's physics, so the
//! server only says where; the session then reports where it stands like any other move.
//!
//! A moved player waits for the chunk it lands in (`Player::spawned`), then comes down as
//! [`Landing`] says: on the column's top block (`World.getTopSolidOrLiquidBlock`, a respawn at
//! the world spawn) and up out of any blocks (`EntityPlayerMP`'s constructor, any spawn).
//! DROPPED: survival and adventure play; spectator's no-clip.

use bevy::math::DVec3;
use proto::Teleport;
use worldgen::populate::props::material;
use worldgen::populate::state as st;

use crate::collision::{floor, get_collision_boxes, BlockAccess};
use crate::player::Player;

/// How the player comes down once the chunk under it has loaded.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Landing {
    /// Stand on the column's top block that blocks movement, ignoring y.
    pub on_top: bool,
    /// Rise a block at a time until nothing collides.
    pub rise: bool,
}

/// `World.getTopSolidOrLiquidBlock`: the block above the column's highest that blocks movement
/// and is not leaves (despite the name, water does not stop it).
pub fn top_solid_or_liquid(w: &impl BlockAccess, x: i32, z: i32) -> i32 {
    (0..256)
        .rev()
        .find(|&y| {
            let s = w.state(x, y, z);
            st::blocks_movement(s) && st::mat(s) != material::LEAVES
        })
        .map_or(0, |y| y + 1)
}

/// `NetHandlerPlayClient.handlePlayerPosLook`: a flagged axis is added to where the player is
/// and keeps its motion, any other is set and its motion stops; then `setPositionAndRotation`.
/// A respawn also ends flying and all motion, as the new `EntityPlayerSP` starts.
pub fn apply(p: &mut Player, tp: &Teleport) {
    let rel = |f: u8| tp.flags & f != 0;
    let mut at = DVec3::from_array(tp.pos);
    for (i, f) in [proto::TP_REL_X, proto::TP_REL_Y, proto::TP_REL_Z].into_iter().enumerate() {
        if rel(f) {
            at[i] += p.pos[i];
        } else {
            p.motion[i] = 0.0;
        }
    }
    let yaw = tp.yaw + if rel(proto::TP_REL_YAW) { p.yaw } else { 0.0 };
    let pitch = tp.pitch + if rel(proto::TP_REL_PITCH) { p.pitch } else { 0.0 };
    // Entity.setPositionAndRotation, then setRotation.
    at.x = at.x.clamp(-3.0e7, 3.0e7);
    at.z = at.z.clamp(-3.0e7, 3.0e7);
    p.set_position(at);
    p.prev_pos = at;
    let pitch = pitch.clamp(-90.0, 90.0);
    (p.yaw, p.pitch) = (yaw % 360.0, pitch % 360.0);
    p.prev_yaw = yaw;
    p.prev_pitch = pitch;
    if p.prev_yaw - yaw < -180.0 {
        p.prev_yaw += 360.0;
    }
    if p.prev_yaw - yaw >= 180.0 {
        p.prev_yaw -= 360.0;
    }
    if rel(proto::TP_RESPAWN) {
        p.motion = DVec3::ZERO;
        p.flying = p.game_mode == proto::GM_SPECTATOR;
        p.landing.rise = true;
    }
    if rel(proto::TP_SURFACE) {
        p.landing.on_top = true;
    }
    p.spawned = false;
}

/// `GameType.configurePlayerCapabilities`: creative keeps flying, spectator always flies, survival
/// and adventure stop.
pub fn set_game_mode(p: &mut Player, mode: u8) {
    p.game_mode = mode;
    p.flying = mode == proto::GM_SPECTATOR || (p.flying && p.allow_flying());
}

/// The chunk under the player has loaded: come down as the last teleport said.
pub fn land(p: &mut Player, w: &impl BlockAccess) {
    let l = std::mem::take(&mut p.landing);
    if l.on_top {
        let y = top_solid_or_liquid(w, floor(p.pos.x), floor(p.pos.z));
        let at = DVec3::new(p.pos.x, y as f64, p.pos.z);
        p.set_position(at);
        p.prev_pos = at;
    }
    if l.rise {
        // EntityPlayerMP(...): up out of whatever is at the spawn point.
        while !get_collision_boxes(w, &p.bb).is_empty() && p.pos.y < 255.0 {
            let at = p.pos;
            p.set_position(DVec3::new(at.x, at.y + 1.0, at.z));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::Blocks;
    use worldgen::blocks::STONE;

    #[test]
    fn relative_axes_add_and_keep_their_motion() {
        let mut p = Player::new(DVec3::new(10.0, 64.0, -3.0), 90.0, 10.0);
        p.motion = DVec3::new(0.5, 0.25, -0.5);
        p.spawned = true;
        let flags = proto::TP_REL_X | proto::TP_REL_Y | proto::TP_REL_YAW | proto::TP_REL_PITCH;
        apply(&mut p, &Teleport { pos: [0.0, 10.0, 5.5], yaw: 0.0, pitch: 0.0, flags });
        assert_eq!(p.pos, DVec3::new(10.0, 74.0, 5.5));
        assert_eq!(p.prev_pos, p.pos);
        assert_eq!(p.motion, DVec3::new(0.5, 0.25, 0.0));
        assert_eq!((p.yaw, p.pitch), (90.0, 10.0));
        assert!(!p.spawned, "it waits for the chunk it lands in");
        apply(&mut p, &Teleport { pos: [1.5, 70.0, 2.5], yaw: -90.0, pitch: 120.0, flags: 0 });
        assert_eq!((p.pos, p.motion, p.yaw, p.pitch), (DVec3::new(1.5, 70.0, 2.5), DVec3::ZERO, -90.0, 90.0));
        assert_eq!(p.landing, Landing { on_top: false, rise: true }, "a new player's spawn still rises");
    }

    #[test]
    fn a_respawn_stands_on_the_top_block_and_stops_flying() {
        let mut w = Blocks::default();
        for y in 0..70 {
            w.set(3, y, 4, STONE);
        }
        let mut p = Player::new(DVec3::new(0.5, 64.0, 0.5), 0.0, 0.0);
        p.landing = Landing::default();
        p.flying = true;
        apply(&mut p, &Teleport { pos: [3.5, 64.0, 4.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN | proto::TP_SURFACE });
        assert!(!p.flying);
        land(&mut p, &w);
        assert_eq!(p.pos, DVec3::new(3.5, 70.0, 4.5));
        assert_eq!(p.landing, Landing::default());
        assert_eq!(top_solid_or_liquid(&w, 9, 9), 0);
    }

    #[test]
    fn game_modes_set_who_may_fly() {
        let mut p = Player::new(DVec3::ZERO, 0.0, 0.0);
        p.flying = true;
        set_game_mode(&mut p, proto::GM_CREATIVE);
        assert!(p.flying && p.allow_flying());
        set_game_mode(&mut p, proto::GM_SURVIVAL);
        assert!(!p.flying && !p.allow_flying());
        set_game_mode(&mut p, proto::GM_SPECTATOR);
        assert!(p.flying && p.allow_flying());
        set_game_mode(&mut p, proto::GM_ADVENTURE);
        assert!(!p.flying && !p.allow_flying());
    }
}
