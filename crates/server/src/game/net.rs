//! Frames out of the game: to one session, to everyone, to everyone near a point, and the ids of
//! the server's own entities. Each goes into the session's `to_viewer` sender, which the hub's
//! writer task drains, so sending never blocks the tick.

use super::Game;

/// The first server entity id. Session (player) ids count from 1, so the two never meet.
pub const FIRST_ENTITY_ID: u32 = 1_000_000;

/// To one session. False when it is not online.
pub fn send(game: &Game, id: u32, tag: u8, body: &[u8]) -> bool {
    match game.sessions.get(&id) {
        Some(s) => s.to_viewer.send(proto::frame(tag, body)).is_ok(),
        None => false,
    }
}

/// To every session but `except`.
pub fn broadcast(game: &Game, except: Option<u32>, tag: u8, body: &[u8]) {
    let frame = proto::frame(tag, body);
    for (&id, s) in &game.sessions {
        if Some(id) != except {
            let _ = s.to_viewer.send(frame.clone());
        }
    }
}

/// To every session but `except` whose player stands closer than `range` to `pos`
/// (`PlayerList.sendToAllNearExcept`: squared distance under squared range). A session that has
/// not sent its state yet is nowhere, so it gets nothing.
pub fn send_near(game: &Game, except: Option<u32>, pos: [f64; 3], range: f64, tag: u8, body: &[u8]) {
    let frame = proto::frame(tag, body);
    for (&id, s) in &game.sessions {
        if Some(id) == except {
            continue;
        }
        let Some(st) = s.state else { continue };
        let d: f64 = (0..3).map(|i| (st.pos[i] - pos[i]).powi(2)).sum();
        if d < range * range {
            let _ = s.to_viewer.send(frame.clone());
        }
    }
}

/// A new id for a server entity (item, mob, arrow), from [`FIRST_ENTITY_ID`] up.
pub fn new_entity_id(game: &mut Game) -> u32 {
    let id = game.next_entity;
    game.next_entity += 1;
    id
}
