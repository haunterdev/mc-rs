//! Tests for the game thread: the tick rate and catch-up, messages from the hub, net.rs.

use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proto::PlayerState;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use super::*;

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("mcrs-game-{}", crate::config::random_u64()))
}

/// A game on this thread, its inbox, and the latency.log lines it wrote.
fn game() -> (Game, Sender<GameMsg>, Arc<Mutex<Vec<Value>>>) {
    let (tx, rx) = channel();
    let lines = Arc::new(Mutex::new(Vec::new()));
    let l = lines.clone();
    let g = Game::new(Config::default(), temp_dir(), rx, Box::new(move |v| l.lock().unwrap().push(v)));
    (g, tx, lines)
}

#[test]
fn save_now_answers_once_the_files_are_written() {
    let (mut g, _tx, _) = game();
    let (done, wait) = channel();
    g.handle(GameMsg::Save { done });
    assert!(wait.try_recv().is_ok());
    assert!(!g.stopping);
}

fn join(tx: &Sender<GameMsg>, id: u32, name: &str) -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (to_viewer, rx) = mpsc::unbounded_channel();
    tx.send(GameMsg::Join { id, name: name.into(), to_viewer, mode: proto::GM_SURVIVAL, survival_client: false }).unwrap();
    rx
}

#[test]
fn the_thread_ticks_20_times_a_second() {
    let (tx, rx) = channel();
    let started = Instant::now();
    let thread = std::thread::Builder::new().name("game".into()).spawn(move || Game::new(Config::default(), temp_dir(), rx, Box::new(|_| {})).run()).unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    let elapsed = started.elapsed();
    tx.send(GameMsg::Shutdown).unwrap();
    let g = thread.join().unwrap();
    // Deadlines are absolute, so a busy machine makes the ticks late, never fewer.
    let expected = elapsed.as_millis() as i64 / 50 + 1;
    assert!((g.ticks as i64 - expected).abs() <= 3, "{} ticks in {elapsed:?}", g.ticks);
}

#[test]
fn late_ticks_catch_up_and_a_long_stall_is_dropped() {
    let t0 = Instant::now();
    let ms = |n: u64| t0 + Duration::from_millis(n);
    assert_eq!(step(ms(10), ms(50)), Err(Duration::from_millis(40)));
    assert_eq!(step(ms(50), ms(50)), Ok(ms(100)));
    // Runs a fake clock for `total` ms where tick `stall_at` takes `stall` ms and the rest none.
    let run = |total: u64, stall_at: u64, stall: u64| {
        let (mut now, mut next, mut ticks) = (0u64, 0u64, 0u64);
        while now < total {
            match step(ms(now), ms(next)) {
                Err(wait) => now += wait.as_millis() as u64,
                Ok(after) => {
                    next = after.duration_since(t0).as_millis() as u64;
                    if ticks == stall_at {
                        now += stall;
                    }
                    ticks += 1;
                }
            }
        }
        ticks
    };
    assert_eq!(run(1000, u64::MAX, 0), 20);
    // Half a second stalled: the ten missed ticks run back to back after it.
    assert_eq!(run(2000, 5, 500), 40);
    // Five seconds stalled: past the 2 s cap the backlog is dropped.
    assert!(run(7000, 5, 5000) < 140 - 40);
}

#[test]
fn join_frames_state_and_leave_reach_the_hooks() {
    let (mut g, tx, _) = game();
    let _a = join(&tx, 1, "xy");
    let _b = join(&tx, 2, "bob");
    g.tick();
    assert_eq!(g.sessions.len(), 2);
    assert!(!g.sessions[&1].survival_client);
    tx.send(GameMsg::Frame { id: 1, tag: proto::V_CLIENT_INFO, body: proto::ClientInfo { version: proto::SURVIVAL_PROTOCOL }.encode() }).unwrap();
    tx.send(GameMsg::Frame { id: 1, tag: proto::V_RESPAWN, body: Vec::new() }).unwrap();
    // A frame from no session is dropped.
    tx.send(GameMsg::Frame { id: 9, tag: proto::V_RESPAWN, body: Vec::new() }).unwrap();
    let s = PlayerState { pos: [1.0, 64.0, 2.0], ..Default::default() };
    tx.send(GameMsg::State { id: 2, state: s }).unwrap();
    tx.send(GameMsg::GameMode { id: 2, mode: proto::GM_CREATIVE }).unwrap();
    g.tick();
    assert!(g.sessions[&1].survival_client);
    // The client info is the game's own; nothing takes the respawn until T4.
    assert_eq!((g.stats.frames, g.stats.frames_unhandled), (2, 1));
    assert_eq!(g.sessions[&2].state.map(|s| s.pos), Some([1.0, 64.0, 2.0]));
    assert_eq!(g.sessions[&2].mode, proto::GM_CREATIVE);
    tx.send(GameMsg::Leave { id: 1 }).unwrap();
    g.tick();
    assert_eq!(g.sessions.keys().copied().collect::<Vec<_>>(), vec![2]);
    tx.send(GameMsg::Shutdown).unwrap();
    g.tick();
    assert!(g.stopping);
}

#[test]
fn every_100_ticks_a_tick_line_goes_to_latency_log() {
    let (mut g, _tx, lines) = game();
    for _ in 0..250 {
        g.tick();
    }
    let lines = lines.lock().unwrap();
    assert_eq!(lines.len(), 2);
    let l = &lines[0];
    assert_eq!(l["event"], "tick");
    assert_eq!(l["ticks"], 100);
    assert!(l["tick_p50_ms"].as_f64().unwrap() <= l["tick_p99_ms"].as_f64().unwrap());
    assert!(l["tick_p99_ms"].as_f64().unwrap() <= l["tick_max_ms"].as_f64().unwrap());
    for k in ["entities", "items", "mobs", "loaded_chunks", "players"] {
        assert_eq!(l[k], 0, "{k}");
    }
}

#[test]
fn net_sends_to_one_everyone_and_those_near() {
    let (mut g, tx, _) = game();
    let mut a = join(&tx, 1, "xy");
    let mut b = join(&tx, 2, "bob");
    let mut c = join(&tx, 3, "cat");
    g.tick();
    let at = |pos| PlayerState { pos, ..Default::default() };
    g.sessions.get_mut(&1).unwrap().state = Some(at([0.0, 64.0, 0.0]));
    g.sessions.get_mut(&2).unwrap().state = Some(at([10.0, 64.0, 0.0]));
    // Player 3 has sent no state: it is nowhere.
    let got = |r: &mut mpsc::UnboundedReceiver<Vec<u8>>| std::iter::from_fn(|| r.try_recv().ok()).collect::<Vec<_>>();
    assert!(net::send(&g, 2, proto::S_HEALTH, &[1]));
    assert!(!net::send(&g, 7, proto::S_HEALTH, &[1]));
    assert_eq!(got(&mut b), vec![proto::frame(proto::S_HEALTH, &[1])]);
    net::broadcast(&g, Some(1), proto::S_SOUND, &[2]);
    assert!(got(&mut a).is_empty() && got(&mut b).len() == 1 && got(&mut c).len() == 1);
    net::send_near(&g, None, [1.0, 64.0, 0.0], 9.0, proto::S_SOUND, &[3]);
    assert!(got(&mut a).len() == 1 && got(&mut b).is_empty() && got(&mut c).is_empty());
    net::send_near(&g, Some(1), [1.0, 64.0, 0.0], 9.5, proto::S_SOUND, &[3]);
    assert!(got(&mut a).is_empty() && got(&mut b).len() == 1);
    assert_eq!(net::new_entity_id(&mut g), 1_000_000);
    assert_eq!(net::new_entity_id(&mut g), 1_000_001);
}

/// A native session through the hub: its join, its survival frames, its state and edits, and its
/// leave reach the game.
#[tokio::test]
async fn a_hub_session_reaches_the_game() {
    let dir = temp_dir();
    let cfg = Config::default();
    let world = crate::world::World::load(&dir).unwrap();
    let accounts = crate::accounts::Accounts::load(&dir);
    let (hub, _listener) = crate::hub::Hub::new(cfg.clone(), world, accounts, dir.clone()).await.unwrap();
    let (tx, rx) = channel();
    hub.set_game(tx);
    let mut g = Game::new(cfg.clone(), dir.clone(), rx, Box::new(|_| {}));

    let (hub_end, our_end) = tokio::io::duplex(1 << 20);
    let (to_browser, _browser) = mpsc::unbounded_channel();
    let (in_tx, incoming) = mpsc::unbounded_channel();
    let session = tokio::spawn(hub.clone().run_session("xy".into(), cfg.picture(None, None, None), to_browser, incoming, Arc::new(AtomicUsize::new(0)), Some(hub_end)));
    let (mut from_hub, mut to_hub) = tokio::io::split(our_end);
    tokio::spawn(async move {
        let mut buf = vec![0u8; 1 << 16];
        while matches!(from_hub.read(&mut buf).await, Ok(n) if n > 0) {}
    });
    let s = PlayerState { pos: [0.5, 64.0, 0.5], ..Default::default() };
    to_hub.write_all(&proto::frame(proto::V_STATE, &s.encode())).await.unwrap();
    to_hub.write_all(&proto::frame(proto::V_CLIENT_INFO, &proto::ClientInfo { version: proto::SURVIVAL_PROTOCOL }.encode())).await.unwrap();
    to_hub.write_all(&proto::frame(proto::V_RESPAWN, &[])).await.unwrap();
    to_hub.write_all(&proto::frame(proto::V_BLOCKS, &proto::encode_blocks(&[((1, 64, 1), 1 << 4)]))).await.unwrap();

    let mut waited = 0;
    while !(g.stats.frames == 2 && g.sessions.values().any(|s| s.state.is_some())) {
        assert!(waited < 500, "the game saw {:?} and {} sessions", g.stats, g.sessions.len());
        tokio::time::sleep(Duration::from_millis(10)).await;
        g.tick();
        waited += 1;
    }
    let me = g.sessions.values().next().unwrap();
    assert_eq!((me.name.as_str(), me.survival_client, me.mode), ("xy", true, crate::world::DEFAULT_GAME_MODE));
    assert_eq!(g.stats.frames_unhandled, 1);

    drop(in_tx);
    session.await.unwrap();
    g.tick();
    assert!(g.sessions.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
