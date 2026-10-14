//! Native clients: the launcher's Join (`viewer --join`), which renders the world on the
//! player's own PC, for friends whose line is too slow for the video. Only world and player data
//! cross the network.
//!
//! `GET /native` with `Authorization: Bearer <token>` (a token from `/api/login`, so the same
//! accounts and rules as the page) upgrades to a WebSocket on the web port, which the Cloudflare
//! tunnel carries like the page's own socket. Binary messages hold whole `proto` frames both ways.
//! The hub runs the session as it runs a streamed one, with a pipe in place of the viewer's
//! socket and no viewer started here: the client gets every frame a viewer gets (the welcome with
//! the seed, the saved edits, the other players, their states and edits, and whatever else the hub
//! sends its viewers), and every JSON message a page gets (chat, the player list, the reason it
//! was closed) as `S_TEXT`. From the client only `V_STATE`, `V_BLOCKS` and `V_PAINTING` (checked
//! here first), `V_MEDIA` (the hub keeps only a file whose name is its hash), `V_TEXT` (handled
//! as the page's JSON: chat, ping) and the survival frames, tags 60..99 under 64 bytes (the game
//! checks them) pass.
//!
//! Both ends ping: the server every [`PING`], so the tunnel always carries traffic toward
//! the client, and a client silent for [`SILENCE`] is gone, so its session leaves instead of
//! staying online after the tunnel dropped the line.

use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use proto::{BlockChange, PlayerState};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::hub::{Hub, ToBrowser};

/// Changed blocks one message may carry; one click changes a few.
const MAX_CHANGES: usize = 4096;
/// How far from where the player last stood an edit may land: the reach, plus what a broken
/// block's neighbours settle into (water flowing in, sand falling, the other half of a plant).
const EDIT_RANGE: f64 = 48.0;
/// Frames batched into one message for the client at most, in bytes.
const BATCH: usize = 64 << 10;
/// How often the server pings, and how long a client may stay silent (it pings every 15 s and
/// sends its state every tick).
const PING: std::time::Duration = std::time::Duration::from_secs(15);
const SILENCE: std::time::Duration = std::time::Duration::from_secs(45);

pub async fn socket(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let token = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
    let Some(name) = hub.accounts.lock().unwrap().check(token.trim()) else {
        // A wrong guess costs a moment, as at the login.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    };
    let ip = crate::bans::client_ip(peer, &headers);
    if let Some(why) = hub.refused(&name, &ip) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    hub.seen(&name, &ip);
    // A message may carry a painting's media file, up to one frame.
    ws.max_message_size(proto::MAX_FRAME + (64 << 10)).on_upgrade(move |socket| play(hub, name, socket))
}

/// The edits a client may make: inside the world's height, near where it last said it stood,
/// and real block ids, by a player whose game mode changes blocks (`GameType.hasLimitedInteractions`:
/// not adventure or spectator). Everything else is dropped.
/// A painting change a native client may make: as an edit, in reach and not in adventure or
/// spectator mode.
pub fn checked_painting(body: &[u8], at: Option<[f64; 3]>, game_mode: u8) -> bool {
    let Some(p) = proto::Painting::decode(body) else { return false };
    if p.gone {
        return at.is_some() && !matches!(game_mode, proto::GM_ADVENTURE | proto::GM_SPECTATOR);
    }
    let edit = proto::encode_blocks(&[((p.pos[0], p.pos[1], p.pos[2]), 16)]);
    !checked_edits(&edit, at, game_mode).is_empty()
}

pub fn checked_edits(body: &[u8], at: Option<[f64; 3]>, game_mode: u8) -> Vec<BlockChange> {
    let Some(at) = at.filter(|_| !matches!(game_mode, proto::GM_ADVENTURE | proto::GM_SPECTATOR)) else { return Vec::new() };
    let mut changes = proto::decode_blocks(body);
    changes.truncate(MAX_CHANGES);
    changes.retain(|&((x, y, z), s)| {
        let d = [x as f64 + 0.5 - at[0], y as f64 + 0.5 - at[1], z as f64 + 0.5 - at[2]];
        (0..256).contains(&y) && s >> 4 < 256 && d.iter().all(|v| v.abs() <= EDIT_RANGE)
    });
    changes
}

/// A survival frame (tags 60..99) the game checks itself; every one is short but a sign's lines.
pub fn passes_survival(tag: u8, body: &[u8]) -> bool {
    (60..100).contains(&tag) && (body.len() < 64 || (tag == proto::V_SIGN && body.len() <= proto::SignText::MAX_SIZE))
}

/// A state the world can keep: no NaN or infinite position or angle.
fn finite(s: &PlayerState) -> bool {
    s.pos.iter().all(|v| v.is_finite()) && s.yaw.is_finite() && s.pitch.is_finite()
}

async fn play(hub: Arc<Hub>, name: String, socket: WebSocket) {
    use futures_util::{SinkExt, StreamExt};
    println!("{name} is joining from the launcher");
    let (mut sink, mut stream) = socket.split();
    let (to_browser, mut out) = mpsc::unbounded_channel::<ToBrowser>();
    let (in_tx, incoming) = mpsc::unbounded_channel::<Result<Vec<u8>, String>>();
    let (hub_end, our_end) = tokio::io::duplex(1 << 20);
    let (mut from_hub, mut to_hub) = tokio::io::split(our_end);

    // What the hub writes for the viewer, frame by frame.
    let (frames_tx, mut frames) = mpsc::unbounded_channel::<Vec<u8>>();
    let pump = tokio::spawn(async move {
        while let Ok((tag, body)) = crate::hub::read_frame(&mut from_hub).await {
            if frames_tx.send(proto::frame(tag, &body)).is_err() {
                break;
            }
        }
    });
    let writer = tokio::spawn(async move {
        // The page's messages first, so a closing reason sent as the session ends still goes out
        // after the frames stop.
        let mut frames_done = false;
        let mut ping = tokio::time::interval(PING);
        loop {
            let mut msg = tokio::select! {
                biased;
                m = out.recv() => match m {
                    Some(ToBrowser::Text(t)) => proto::frame(proto::S_TEXT, t.as_bytes()),
                    // No video or sound comes from a session without a viewer here.
                    Some(ToBrowser::Binary(_)) => continue,
                    Some(ToBrowser::Close(reason)) => {
                        let closed = json!({"t": "closed", "text": reason}).to_string();
                        let _ = sink.send(Message::Binary(proto::frame(proto::S_TEXT, closed.as_bytes()).into())).await;
                        let _ = sink.close().await;
                        break;
                    }
                    None => break,
                },
                f = frames.recv(), if !frames_done => match f {
                    Some(f) => f,
                    None => {
                        frames_done = true;
                        continue;
                    }
                },
                _ = ping.tick() => {
                    if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                        break;
                    }
                    continue;
                }
            };
            while msg.len() < BATCH {
                match frames.try_recv() {
                    Ok(f) => msg.extend(f),
                    Err(_) => break,
                }
            }
            if sink.send(Message::Binary(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // What the client sends: its state and edits to the hub as a viewer's, its JSON as a page's.
    let who = name.clone();
    let world = hub.clone();
    let reader = tokio::spawn(async move {
        let mut at: Option<[f64; 3]> = None;
        loop {
            let m = match tokio::time::timeout(SILENCE, stream.next()).await {
                Ok(Some(Ok(m))) => m,
                Ok(_) => break,
                Err(_) => {
                    println!("[{who}] sent nothing for {} s; closing", SILENCE.as_secs());
                    break;
                }
            };
            let bytes = match m {
                Message::Binary(b) => b,
                Message::Close(_) => break,
                _ => continue,
            };
            let Some(list) = proto::split_frames(&bytes) else {
                println!("[{who}] sent a broken frame; closing");
                break;
            };
            for (tag, body) in list {
                let ok = match tag {
                    proto::V_STATE => match PlayerState::decode(body).filter(finite) {
                        Some(s) => {
                            at = Some(s.pos);
                            to_hub.write_all(&proto::frame(proto::V_STATE, &s.encode())).await.is_ok()
                        }
                        None => true,
                    },
                    proto::V_BLOCKS => {
                        let mode = world.world.lock().unwrap().game_mode(&who);
                        let changes = checked_edits(body, at, mode);
                        let dropped = body.len() / 14 - changes.len();
                        if dropped > 0 {
                            println!("[{who}] {dropped} block changes out of reach or not allowed dropped");
                        }
                        changes.is_empty() || to_hub.write_all(&proto::frame(proto::V_BLOCKS, &proto::encode_blocks(&changes))).await.is_ok()
                    }
                    proto::V_MEDIA => to_hub.write_all(&proto::frame(proto::V_MEDIA, body)).await.is_ok(),
                    proto::V_PAINTING => {
                        let mode = world.world.lock().unwrap().game_mode(&who);
                        !checked_painting(body, at, mode) || to_hub.write_all(&proto::frame(proto::V_PAINTING, body)).await.is_ok()
                    }
                    proto::V_TEXT => in_tx.send(Err(String::from_utf8_lossy(body).into_owned())).is_ok(),
                    t if passes_survival(t, body) => to_hub.write_all(&proto::frame(t, body)).await.is_ok(),
                    _ => true,
                };
                if !ok {
                    return;
                }
            }
        }
    });

    let picture = hub.cfg.picture(None, None, None);
    hub.run_session(name, picture, to_browser, incoming, Arc::new(AtomicUsize::new(0)), Some(hub_end)).await;
    reader.abort();
    pump.abort();
    // Let a closing message go out.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), writer).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_far_away_out_of_the_world_before_a_state_or_without_building_are_dropped() {
        let near = ((10, 64, -5), 1 << 4);
        let body = proto::encode_blocks(&[near, ((10, 256, -5), 16), ((200, 64, -5), 16), ((10, -1, -5), 16), ((11, 64, -5), 300 << 4 | 1)]);
        let at = Some([8.5, 65.0, -3.5]);
        assert_eq!(checked_edits(&body, at, proto::GM_CREATIVE), vec![near]);
        assert_eq!(checked_edits(&body, at, proto::GM_SURVIVAL), vec![near]);
        assert!(checked_edits(&body, None, proto::GM_CREATIVE).is_empty());
        assert!(checked_edits(&body, at, proto::GM_ADVENTURE).is_empty());
        assert!(checked_edits(&body, at, proto::GM_SPECTATOR).is_empty());
        let many = proto::encode_blocks(&vec![near; MAX_CHANGES + 10]);
        assert_eq!(checked_edits(&many, at, proto::GM_CREATIVE).len(), MAX_CHANGES);
    }

    #[test]
    fn survival_frames_pass_when_short() {
        assert!(passes_survival(proto::V_CLIENT_INFO, &[1, 0]));
        assert!(passes_survival(99, &[0; 63]));
        assert!(!passes_survival(99, &[0; 64]));
        assert!(!passes_survival(59, &[]));
        assert!(!passes_survival(100, &[]));
        assert!(!passes_survival(proto::V_TEXT, b"hi"));
        assert!(passes_survival(proto::V_SIGN, &[0; proto::SignText::MAX_SIZE]));
        assert!(!passes_survival(proto::V_SIGN, &[0; proto::SignText::MAX_SIZE + 1]));
    }

    #[test]
    fn states_with_nan_or_infinity_are_refused() {
        let s = PlayerState { pos: [1.0, 2.0, 3.0], ..Default::default() };
        assert!(finite(&s));
        assert!(!finite(&PlayerState { pos: [f64::NAN, 2.0, 3.0], ..s }));
        assert!(!finite(&PlayerState { yaw: f32::INFINITY, ..s }));
    }
}
