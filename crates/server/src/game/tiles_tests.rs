//! Tests for tiles.rs: a sign comes and goes with its block, only its placer fills it, every
//! viewer hears the lines, and the lines outlive a restart in `tiles.json`.

use std::sync::mpsc::{channel, Sender};

use proto::SignText;
use tokio::sync::mpsc;

use super::*;

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("mcrs-tiles-{}", crate::config::random_u64()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn game_in(dir: &PathBuf) -> (Game, Sender<GameMsg>) {
    let (tx, rx) = channel();
    (Game::new(Config::default(), dir.clone(), rx, Box::new(|_| {})), tx)
}

fn join(tx: &Sender<GameMsg>, id: u32, name: &str) -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (to_viewer, rx) = mpsc::unbounded_channel();
    tx.send(GameMsg::Join { id, name: name.into(), to_viewer, mode: proto::GM_CREATIVE, survival_client: true }).unwrap();
    rx
}

/// The `S_SIGN` bodies a session got.
fn signs(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<SignText> {
    let mut out = Vec::new();
    while let Ok(bytes) = rx.try_recv() {
        for (tag, body) in proto::split_frames(&bytes).unwrap() {
            if tag == proto::S_SIGN {
                out.push(SignText::decode(body).unwrap());
            }
        }
    }
    out
}

fn lines(a: &str, b: &str) -> [String; 4] {
    [a.into(), b.into(), String::new(), String::new()]
}

const AT: (i32, i32, i32) = (3, 70, -2);
const STANDING: u16 = 63 << 4 | 8;
const WALL: u16 = 68 << 4 | 2;

#[test]
fn the_placer_fills_a_new_sign_and_everyone_hears_it() {
    let d = dir();
    let (mut g, tx) = game_in(&d);
    let mut a = join(&tx, 1, "xy");
    let mut b = join(&tx, 2, "guest");
    tx.send(GameMsg::Blocks { id: 1, changes: vec![(AT, STANDING)] }).unwrap();
    g.tick();
    assert_eq!(g.tiles.signs[&AT], tiles::Sign { block: 63, lines: Default::default(), editor: Some(1) });
    // Someone else's lines are refused.
    let text = SignText { pos: [AT.0, AT.1, AT.2], lines: lines("not", "mine") };
    tx.send(GameMsg::Frame { id: 2, tag: proto::V_SIGN, body: text.encode() }).unwrap();
    g.tick();
    assert_eq!(g.tiles.signs[&AT].lines, <[String; 4]>::default());
    // The placer's are kept, without formatting codes, and sent to both viewers.
    let text = SignText { pos: [AT.0, AT.1, AT.2], lines: lines("\u{a7}cHello", "x\u{a7}zy") };
    tx.send(GameMsg::Frame { id: 1, tag: proto::V_SIGN, body: text.encode() }).unwrap();
    g.tick();
    let want = SignText { pos: [AT.0, AT.1, AT.2], lines: lines("Hello", "x\u{a7}zy") };
    assert_eq!(signs(&mut a), vec![want.clone()]);
    assert_eq!(signs(&mut b), vec![want.clone()]);
    // A late joiner gets the sign on join.
    let mut c = join(&tx, 3, "late");
    g.tick();
    assert_eq!(signs(&mut c), vec![want]);
    // Turned into a wall sign, it is a new empty sign; taken away, it is gone.
    tx.send(GameMsg::Blocks { id: 2, changes: vec![(AT, WALL)] }).unwrap();
    g.tick();
    assert_eq!(g.tiles.signs[&AT], tiles::Sign { block: 68, lines: Default::default(), editor: Some(2) });
    tx.send(GameMsg::Blocks { id: 2, changes: vec![(AT, 0)] }).unwrap();
    g.tick();
    assert!(g.tiles.signs.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn signs_outlive_a_restart_and_are_not_editable_after() {
    let d = dir();
    {
        let (mut g, tx) = game_in(&d);
        let _a = join(&tx, 1, "xy");
        tx.send(GameMsg::Blocks { id: 1, changes: vec![(AT, WALL)] }).unwrap();
        let text = SignText { pos: [AT.0, AT.1, AT.2], lines: lines("kept", "") };
        tx.send(GameMsg::Frame { id: 1, tag: proto::V_SIGN, body: text.encode() }).unwrap();
        tx.send(GameMsg::Shutdown).unwrap();
        g.tick();
        assert!(g.stopping);
    }
    let (mut g, tx) = game_in(&d);
    assert_eq!(g.tiles.signs[&AT], tiles::Sign { block: 68, lines: lines("kept", ""), editor: None });
    let _a = join(&tx, 1, "xy");
    let text = SignText { pos: [AT.0, AT.1, AT.2], lines: lines("changed", "") };
    tx.send(GameMsg::Frame { id: 1, tag: proto::V_SIGN, body: text.encode() }).unwrap();
    g.tick();
    assert_eq!(g.tiles.signs[&AT].lines, lines("kept", ""));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn formatting_codes_come_out() {
    assert_eq!(tiles::strip_formatting("\u{a7}4red\u{a7}R \u{a7}lbold\u{a7}"), "red bold\u{a7}");
    assert_eq!(tiles::strip_formatting("a\u{a7}gb"), "a\u{a7}gb");
}
