//! Tests for the join and leave announcements: a dropped line that joins again must
//! not announce a leave, a session replaced by a newer one for the same name must stay silent,
//! and a player who really left is announced exactly once.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Notify};

use super::*;
use crate::accounts::Accounts;
use crate::config::Config;
use crate::world::World;

/// A hub in a throwaway folder, with fake sessions inserted straight into `online`.
struct T {
    hub: Arc<Hub>,
    dir: PathBuf,
}

impl T {
    async fn new() -> T {
        let dir = crate::game::testing::temp_dir();
        let cfg = Config { seed: 0, ..Config::default() };
        let world = World::load(&dir).unwrap();
        let accounts = Accounts::load(&dir);
        let (hub, _listener) = Hub::new(cfg, world, accounts, dir.clone()).await.unwrap();
        T { hub, dir }
    }

    /// Puts session `id` named `name` online and returns its browser's messages.
    fn add(&self, id: u32, name: &str) -> mpsc::UnboundedReceiver<ToBrowser> {
        let (to_viewer, _viewer) = mpsc::unbounded_channel();
        let (to_browser, browser) = mpsc::unbounded_channel();
        self.hub.online.lock().unwrap().insert(id, Online { name: name.into(), to_viewer, to_browser, state: None, kick: Arc::new(Notify::new()) });
        browser
    }
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Drains the browser `Text` messages received so far.
fn texts(rx: &mut mpsc::UnboundedReceiver<ToBrowser>) -> Vec<String> {
    let mut v = Vec::new();
    while let Ok(m) = rx.try_recv() {
        if let ToBrowser::Text(t) = m {
            v.push(t);
        }
    }
    v
}

#[tokio::test]
async fn a_rejoin_within_the_grace_announces_nothing() {
    let t = T::new().await;
    let mut leaver = t.add(1, "Xy");
    t.hub.leave_held(1, "Xy", "left", Duration::from_millis(300));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut again = t.add(2, "Xy");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(t.hub.count(), 1);
    assert!(texts(&mut leaver).iter().all(|m| !m.contains("left the game")), "the replaced session spoke");
    assert!(texts(&mut again).iter().all(|m| !m.contains("left the game")), "a rejoin announced a leave");
}

#[tokio::test]
async fn a_rejoin_announces_no_join_either() {
    let t = T::new().await;
    let mut other = t.add(3, "Bob");
    // The old line closed first: its leave is held, and the rejoin ends it quietly.
    let _leaver = t.add(1, "Xy");
    t.hub.leave_held(1, "Xy", "left", Duration::from_millis(300));
    let _again = t.add(2, "Xy");
    t.hub.announce_join("Xy", false);
    // The rejoin beat the old line's close: the old session is still online.
    let _third = t.add(4, "Xy");
    t.hub.announce_join("Xy", true);
    // A fresh join is announced.
    t.hub.announce_join("Amy", false);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let lines = texts(&mut other);
    assert!(lines.iter().all(|m| !m.contains("Xy joined") && !m.contains("Xy left")), "{lines:?}");
    assert_eq!(lines.iter().filter(|m| m.contains("Amy joined the game")).count(), 1, "{lines:?}");
}

#[tokio::test]
async fn a_real_leave_announces_once() {
    let t = T::new().await;
    let _leaver = t.add(1, "Xy");
    let mut other = t.add(2, "Bob");
    t.hub.leave_held(1, "Xy", "left", Duration::from_millis(50));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(t.hub.count(), 1);
    let lines = texts(&mut other);
    assert_eq!(lines.iter().filter(|m| m.contains("Xy left the game")).count(), 1, "{lines:?}");
}

#[tokio::test]
async fn a_duplicate_login_kicks_the_old_session_with_no_leave_line() {
    let t = T::new().await;
    let mut old = t.add(1, "Xy");
    t.hub.kick("Xy", "Signed in somewhere else.");
    assert!(matches!(old.try_recv(), Ok(ToBrowser::Close(_))), "the old session was not kicked");
    let mut new = t.add(2, "Xy");
    // The newer session for the same name is online before the old one closes.
    t.hub.leave(1, "Xy", "closed by the server");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(t.hub.count(), 1);
    assert!(texts(&mut new).iter().all(|m| !m.contains("left the game")), "a duplicate login announced a leave");
}
