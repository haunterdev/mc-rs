//! Join a server: plays on a friend's mc-rs browser server (crates/server) with the game
//! rendering on this PC, for a line too slow for the website's video. Only the world's changes
//! and the players cross the network.
//!
//! The panel takes the server's address (its website link, `host:port`, or an `mcrs://join` link
//! from the website, which fills it in), the name and password used on that website, and Join; or
//! As guest, which needs no account (`/api/guest` with an empty name, so the server picks a
//! `Guest` name, as the website's guest button does).
//! The launcher signs in at the server's `/api/login` itself, so a wrong password shows here at
//! once, then starts the viewer with `--join <wss://host/native>` and the sign-in token in
//! `MCRS_JOIN_TOKEN` (never on the command line); a guest's token is signed out when the game
//! ends, an account's is kept (30 days, as the website's).
//! The address, name and sign-in token are kept in `%APPDATA%\mc-rs\launcher.json`; the password
//! never is. On start a saved token is checked with `/api/me`, so a returning player joins with
//! one click and Sign out ends the sign-in.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Button, Color32, CornerRadius, Key, RichText, TextEdit, Vec2};

use crate::ui::{Handle, Phase};
use crate::uri::{self, Link, Server};
use crate::{game, net};

/// The viewer reads the sign-in token from this variable.
pub const TOKEN_VAR: &str = "MCRS_JOIN_TOKEN";
/// mc-rs-server's port when it is reached on a local network without the tunnel.
pub const SERVER_PORT: u16 = 25580;
const GREEN: Color32 = Color32::from_rgb(60, 133, 39);

/// Where a server answers: its website (for the sign-in) and its game socket.
#[derive(Clone, Debug, PartialEq)]
pub struct Address {
    /// `https://host` or `http://host:port`.
    pub http: String,
    /// `wss://host/native` or `ws://host:port/native`.
    pub ws: String,
    /// `host` or `host:port`, for the window.
    pub shown: String,
}

/// Reads what the player typed or pasted as a server address. A scheme picks TLS; without one a
/// name with no port is a tunnel (`https`, 443), and an IP address or `localhost` with no port is
/// a server on the local network (`http`, 25580); any other port is plain `http`.
pub fn address(text: &str) -> Result<Address, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("Enter the server's address: the link to its website, or host:port.".into());
    }
    let (scheme, rest) = match t.split_once("://") {
        Some((s, r)) => (Some(s.to_ascii_lowercase()), r),
        None => (None, t),
    };
    let (host, port, secure) = if scheme.as_deref() == Some(uri::SCHEME) {
        match uri::parse(t)? {
            Link::Join(Server { host, port }) => (host, port, None),
            Link::Play => return Err("mcrs://play is not a server address.".into()),
        }
    } else {
        let secure = match scheme.as_deref() {
            Some("https" | "wss") => Some(true),
            Some("http" | "ws") => Some(false),
            None => None,
            Some(s) => return Err(format!("{s}:// is not a server address; use the link to the server's website.")),
        };
        // The website's link may carry a path; only the host and port matter.
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, Some(uri::parse_port(p)?)),
            None => (authority, None),
        };
        (uri::parse_host(host)?, port, secure)
    };
    let local = host == "localhost" || host.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    let secure = secure.unwrap_or(match port {
        Some(p) => p == 443,
        None => !local,
    });
    let port = port.unwrap_or(match (secure, local) {
        (true, _) => 443,
        (false, true) => SERVER_PORT,
        (false, false) => 80,
    });
    let shown = if port == if secure { 443 } else { 80 } { host } else { format!("{host}:{port}") };
    let (http, ws) = if secure { ("https", "wss") } else { ("http", "ws") };
    Ok(Address { http: format!("{http}://{shown}"), ws: format!("{ws}://{shown}/native"), shown })
}

/// Signs in at the server's `/api/login`, as its website does (a new name makes the account
/// there too). Returns the token, or a sentence for the window.
pub fn login(addr: &Address, name: &str, password: &str) -> Result<String, String> {
    let body = serde_json::json!({"name": name.trim(), "password": password}).to_string();
    let mut resp = net::agent()
        .post(format!("{}/api/login", addr.http))
        .config()
        .http_status_as_error(false)
        .build()
        .header("Content-Type", "application/json")
        .send(&body)
        .map_err(|e| format!("Could not reach {} ({e}).", addr.shown))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(64 << 10).read_to_string().unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        if let Some(t) = v.get("token").and_then(|t| t.as_str()) {
            return Ok(t.to_string());
        }
    }
    match v.get("error").and_then(|e| e.as_str()) {
        Some("That server password is not right.") => {
            Err("This server asks new players for a server password: sign up once on its website, then join here.".into())
        }
        Some(e) => Err(e.to_string()),
        None => Err(format!("{} answered HTTP {status}; is it an mc-rs server?", addr.shown)),
    }
}

/// Joins as a guest at the server's `/api/guest`: `name`, or a name the server picks when it is
/// empty. Returns the token, or a sentence for the window.
pub fn login_guest(addr: &Address, name: &str) -> Result<String, String> {
    let body = serde_json::json!({"name": name.trim()}).to_string();
    let mut resp = net::agent()
        .post(format!("{}/api/guest", addr.http))
        .config()
        .http_status_as_error(false)
        .build()
        .header("Content-Type", "application/json")
        .send(&body)
        .map_err(|e| format!("Could not reach {} ({e}).", addr.shown))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(64 << 10).read_to_string().unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if let Some(t) = v.get("token").and_then(|t| t.as_str()).filter(|_| v.get("ok").and_then(|o| o.as_bool()) == Some(true)) {
        return Ok(t.to_string());
    }
    match v.get("error").and_then(|e| e.as_str()) {
        Some("That server password is not right.") => {
            Err("This server asks new players for a server password: sign up once on its website, then join here.".into())
        }
        Some(e) => Err(e.to_string()),
        // A server from before guests (0.3.0 and older) has no /api/guest.
        None if status == 404 || status == 405 => Err("This server is older than guest joining: sign in with a name and password.".into()),
        None => Err(format!("{} answered HTTP {status}; is it an mc-rs server?", addr.shown)),
    }
}

/// Ends the token once the game is over, so the server's token list does not grow with each join.
pub fn logout(addr: &Address, token: &str) {
    let body = serde_json::json!({"token": token}).to_string();
    let _ = net::agent().post(format!("{}/api/logout", addr.http)).header("Content-Type", "application/json").send(&body);
}

/// How a server answered the check of a saved token.
enum Me {
    /// The token is good, under this name.
    Name(String),
    /// The server refused the token; it is no longer valid.
    Rejected,
    /// The server did not answer (down or unreachable).
    Unreachable,
}

/// Asks the server whether a saved token still signs a player in (web.rs `me`).
fn me(addr: &Address, token: &str) -> Me {
    let url = format!("{}/api/me?token={token}", addr.http);
    let mut resp = match net::agent().get(&url).config().http_status_as_error(false).build().call() {
        Ok(r) => r,
        Err(_) => return Me::Unreachable,
    };
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(64 << 10).read_to_string().unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        if let Some(n) = v.get("name").and_then(|n| n.as_str()) {
            return Me::Name(n.to_string());
        }
    }
    if status == 401 || status == 403 {
        Me::Rejected
    } else {
        Me::Unreachable
    }
}

/// Why the viewer left the server, from its log: the connection error or the server's reason.
pub fn reason(log: &[String]) -> Option<String> {
    log.iter().rev().find_map(|l| {
        if let Some(r) = l.split_once("mc-rs viewer: disconnected: ").map(|(_, r)| r) {
            return Some(format!("Disconnected: {r}"));
        }
        let rest = l.split_once("mc-rs viewer: could not join ")?.1;
        Some(format!("Could not join: {}", rest.split_once(": ").map_or(rest, |(_, r)| r)))
    })
}

fn remembered_path() -> Option<PathBuf> {
    paths::data_dir().map(|d| d.join("launcher.json"))
}

/// The address, name and sign-in token in `path`, empty when it does not exist yet.
fn remembered_at(path: &Path) -> (String, String, String) {
    let v: serde_json::Value = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    (s("join_address"), s("join_name"), s("join_token"))
}

/// Writes the address into `path`, and the name and token too when given; As guest passes neither,
/// so the remembered name stays. A token belongs to one server, so moving to another address drops
/// it. The password is never written.
fn save_remembered_at(path: &Path, address: &str, name: Option<&str>, token: Option<&str>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut v: serde_json::Value = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({}));
    if let Some(o) = v.as_object_mut() {
        let same = o.get("join_address").and_then(|x| x.as_str()).map(str::trim) == Some(address.trim());
        o.insert("join_address".into(), address.trim().into());
        if let Some(n) = name {
            o.insert("join_name".into(), n.trim().into());
        }
        match token {
            Some(t) => {
                o.insert("join_token".into(), t.into());
            }
            None if !same => {
                o.remove("join_token");
            }
            None => {}
        }
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default());
}

/// Drops the saved token, keeping the address and name.
fn forget_token_at(path: &Path) {
    let mut v: serde_json::Value = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({}));
    if let Some(o) = v.as_object_mut() {
        o.remove("join_token");
    }
    let _ = std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default());
}

/// The last address, name and token joined with.
fn remembered() -> (String, String, String) {
    remembered_path().map_or_else(|| (String::new(), String::new(), String::new()), |p| remembered_at(&p))
}

fn save_remembered(address: &str, name: Option<&str>, token: Option<&str>) {
    if let Some(p) = remembered_path() {
        save_remembered_at(&p, address, name, token);
    }
}

fn forget_token() {
    if let Some(p) = remembered_path() {
        forget_token_at(&p);
    }
}

/// The Join a server panel's fields.
pub struct Form {
    pub address: String,
    pub name: String,
    password: String,
    /// The last press was As guest.
    pub guest: bool,
    /// The remembered address and the token saved for it.
    saved: Option<(String, String)>,
    /// The name `/api/me` gave for the saved token; a valid one means signed in.
    signed_in: Option<String>,
    /// The background `/api/me` answer for the saved address, for the window to read.
    checked: Arc<Mutex<Option<(String, Me)>>>,
    /// The saved token has been checked once.
    checked_once: bool,
    /// The check is running.
    checking: bool,
}

impl Form {
    /// The remembered address, name and token; an `mcrs://join` link's address wins. A saved token
    /// is used only for the remembered address, and checked with `/api/me` when the window draws.
    pub fn new(link: Option<&Server>) -> Form {
        let (remembered_address, name, token) = remembered();
        let address = link.map_or(remembered_address.clone(), |s| s.to_string());
        let saved = (!token.is_empty() && address.trim() == remembered_address.trim()).then(|| (address.trim().to_string(), token));
        Form { address, name, password: String::new(), guest: false, saved, signed_in: None, checked: Arc::new(Mutex::new(None)), checked_once: false, checking: false }
    }

    /// The name the server confirmed for the saved token, while the address still matches it.
    fn signed_in_name(&self) -> Option<String> {
        match (&self.saved, &self.signed_in) {
            (Some((addr, _)), Some(name)) if addr == self.address.trim() => Some(name.clone()),
            _ => None,
        }
    }

    /// Starts the `/api/me` check for the saved token once, and reads its answer when it arrives.
    fn poll(&mut self, ui: &mut egui::Ui) {
        if !self.checked_once {
            if let Some((addr_text, token)) = self.saved.clone() {
                if addr_text == self.address.trim() {
                    if let Ok(addr) = address(&addr_text) {
                        self.checked_once = true;
                        self.checking = true;
                        let slot = self.checked.clone();
                        std::thread::spawn(move || {
                            let answer = me(&addr, &token);
                            *slot.lock().unwrap() = Some((addr_text, answer));
                        });
                    }
                }
            }
        }
        if let Some((addr_text, answer)) = self.checked.lock().unwrap().take() {
            self.checking = false;
            if addr_text == self.address.trim() {
                match answer {
                    Me::Name(n) => self.signed_in = Some(n),
                    // A token the server refused is gone; the name and password fields come back.
                    Me::Rejected => {
                        self.saved = None;
                        forget_token();
                    }
                    // The server was down: keep the token for the next launch, sign in again now.
                    Me::Unreachable => self.saved = None,
                }
            }
        }
        if self.checking {
            ui.ctx().request_repaint();
        }
    }

    /// Ends the saved sign-in: forgets the token (in the file too) and brings the fields back.
    fn sign_out(&mut self) {
        if let Some((addr_text, token)) = self.saved.take() {
            if let Ok(addr) = address(&addr_text) {
                std::thread::spawn(move || logout(&addr, &token));
            }
        }
        self.signed_in = None;
        self.checking = false;
        forget_token();
    }

    /// The fields, Join and As guest; true when either was pressed (or Enter in a field, which is
    /// Join), with `guest` saying which. `hint` adds the line under the fields, left out while the
    /// phases below need the room.
    pub fn draw(&mut self, ui: &mut egui::Ui, busy: bool, label: &str, hint: bool) -> bool {
        self.poll(ui);
        let mut enter = false;
        ui.label(RichText::new("Server address").strong());
        let r = ui.add_enabled(!busy, TextEdit::singleline(&mut self.address).hint_text("the server's link, or host:port").desired_width(f32::INFINITY));
        enter |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if let Some(name) = self.signed_in_name() {
            let mut out = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Signed in as {name}.")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = ui.link("Sign out");
                    out = !busy && r.clicked();
                });
            });
            if out {
                self.sign_out();
            }
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Name").strong());
                let r = ui.add_enabled(!busy, TextEdit::singleline(&mut self.name).desired_width(130.0));
                enter |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.label(RichText::new("Password").strong());
                let r = ui.add_enabled(!busy, TextEdit::singleline(&mut self.password).password(true).desired_width(f32::INFINITY));
                enter |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            });
        }
        if hint {
            ui.label(
                RichText::new("Your account on the server's website (a new name signs up), or As guest with no account, which joins under a Guest name. The game runs on this PC.")
                    .small()
                    .weak(),
            );
        }
        ui.add_space(6.0);
        let (mut join, mut guest) = (false, false);
        ui.horizontal(|ui| {
            let guest_width = 120.0;
            join = ui
                .add_enabled(
                    !busy,
                    Button::new(RichText::new(label).size(21.0).strong().color(Color32::WHITE))
                        .fill(GREEN)
                        .corner_radius(CornerRadius::same(6))
                        .min_size(Vec2::new(ui.available_width() - guest_width - ui.spacing().item_spacing.x, 50.0)),
                )
                .clicked();
            guest = ui
                .add_enabled(
                    !busy,
                    Button::new(RichText::new("As guest").size(16.0)).corner_radius(CornerRadius::same(6)).min_size(Vec2::new(guest_width, 50.0)),
                )
                .on_hover_text("Play without an account, under a Guest name the server picks")
                .clicked();
        });
        if guest {
            self.guest = true;
        } else if join || (enter && !busy) {
            self.guest = false;
        }
        guest || join || (enter && !busy)
    }

    /// Install, client jar, sign-in, then the viewer in join mode, on a worker thread.
    /// `download` is the player's yes to fetching the jar.
    pub fn start(&self, h: Handle, download: bool) {
        let addr = match address(&self.address) {
            Ok(a) => a,
            Err(e) => return h.phase(Phase::Failed { message: e, log: Vec::new() }),
        };
        let guest = self.guest;
        // A saved token the server confirmed skips the password; otherwise the fields are needed.
        let saved_token = match (&self.saved, self.signed_in_name()) {
            (Some((_, token)), Some(_)) if !guest => Some(token.clone()),
            _ => None,
        };
        if !guest && saved_token.is_none() && (self.name.trim().is_empty() || self.password.is_empty()) {
            return h.phase(Phase::Failed {
                message: "Enter your name and password for this server, or join As guest.".into(),
                log: Vec::new(),
            });
        }
        match (guest, saved_token.is_some()) {
            // As guest keeps the remembered name and token; only the address is saved.
            (true, _) => save_remembered(&self.address, None, None),
            // Sign in with the fields; the worker saves the name and the token the server returns.
            (false, false) => save_remembered(&self.address, Some(&self.name), None),
            // The saved token is already remembered.
            (false, true) => {}
        }
        let (name, password, raw_address) = (self.name.clone(), self.password.clone(), self.address.clone());
        h.phase(Phase::Working { what: "Preparing".into(), done: 0, total: None });
        std::thread::spawn(move || {
            let Some((viewer, jar)) = crate::ui::prepare(&h, download) else { return };
            h.phase(Phase::Working { what: format!("Signing in to {}", addr.shown), done: 0, total: None });
            let signed_in = match saved_token {
                // A confirmed saved token: no password needed.
                Some(t) => Ok(t),
                None if guest => login_guest(&addr, ""),
                None => match login(&addr, &name, &password) {
                    Ok(t) => {
                        // The server knows the name; keep it with the token for the next launch.
                        save_remembered(&raw_address, Some(&name), Some(&t));
                        Ok(t)
                    }
                    Err(message) => Err(message),
                },
            };
            let token = match signed_in {
                Ok(t) => t,
                Err(message) => return h.phase(Phase::Failed { message, log: Vec::new() }),
            };
            let child = game::start(&viewer, &["--join", &addr.ws], &[(TOKEN_VAR, &token)], &jar);
            let mut child = match child {
                Ok(c) => c,
                Err(e) => return h.phase(Phase::Failed { message: format!("mc-rs could not start: {e}"), log: Vec::new() }),
            };
            h.phase(Phase::Joined { server: addr.shown.clone() });
            let status = child.wait();
            // A guest's token is not remembered, so it is signed out here; an account's token stays
            // (30 days, as the website's) so the next launch is signed in with one click.
            if guest {
                logout(&addr, &token);
            }
            h.phase(match status {
                Ok(s) if s.success() => Phase::Left { server: addr.shown },
                Ok(s) => {
                    let log = game::log_tail(14);
                    match reason(&log) {
                        Some(message) => Phase::Failed { message, log },
                        None => crate::ui::exit_failure(s, log),
                    }
                }
                Err(e) => Phase::Failed { message: e.to_string(), log: Vec::new() },
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(text: &str) -> (String, String) {
        let a = address(text).unwrap();
        (a.http, a.ws)
    }

    /// The remembered file keeps the token, and As guest and Sign out do not touch the name.
    #[test]
    fn the_remembered_file_keeps_the_token() {
        let path = std::env::temp_dir().join(format!("mc-rs-launcher-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(remembered_at(&path), (String::new(), String::new(), String::new()));
        save_remembered_at(&path, "red-fox.trycloudflare.com", Some("Steve"), Some("abc123"));
        let (address, name, token) = remembered_at(&path);
        assert_eq!((address.as_str(), name.as_str(), token.as_str()), ("red-fox.trycloudflare.com", "Steve", "abc123"));
        // As guest on the same server: the remembered name and token stay.
        save_remembered_at(&path, "red-fox.trycloudflare.com", None, None);
        let (_, name, token) = remembered_at(&path);
        assert_eq!((name.as_str(), token.as_str()), ("Steve", "abc123"));
        // Another server: the token goes (it belongs to the first), the name stays.
        save_remembered_at(&path, "10.0.0.2:25580", None, None);
        let (address, name, token) = remembered_at(&path);
        assert_eq!((address.as_str(), name.as_str()), ("10.0.0.2:25580", "Steve"));
        assert!(token.is_empty(), "the token belonged to the other server");
        // Sign out drops the token and keeps the rest.
        save_remembered_at(&path, "10.0.0.2:25580", None, Some("def456"));
        forget_token_at(&path);
        let (_, name, token) = remembered_at(&path);
        assert_eq!(name, "Steve");
        assert!(token.is_empty(), "the token is gone after a sign out");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn addresses_pick_the_scheme_and_port() {
        let tunnel = ("https://red-fox.trycloudflare.com".to_string(), "wss://red-fox.trycloudflare.com/native".to_string());
        assert_eq!(a("https://red-fox.trycloudflare.com/"), tunnel);
        assert_eq!(a("red-fox.trycloudflare.com"), tunnel);
        assert_eq!(a("  Red-Fox.trycloudflare.com  "), tunnel);
        assert_eq!(a("mcrs://join/red-fox.trycloudflare.com"), tunnel);
        assert_eq!(a("https://red-fox.trycloudflare.com/?x=1#play"), tunnel);
        assert_eq!(a("http://127.0.0.1:25580"), ("http://127.0.0.1:25580".into(), "ws://127.0.0.1:25580/native".into()));
        assert_eq!(a("192.168.1.20"), ("http://192.168.1.20:25580".into(), "ws://192.168.1.20:25580/native".into()));
        assert_eq!(a("localhost"), ("http://localhost:25580".into(), "ws://localhost:25580/native".into()));
        assert_eq!(a("mcrs://join/pc.example.net:25580"), ("http://pc.example.net:25580".into(), "ws://pc.example.net:25580/native".into()));
        assert_eq!(a("example.net:443"), tunnel_of("example.net"));
        assert_eq!(address("red-fox.trycloudflare.com").unwrap().shown, "red-fox.trycloudflare.com");
        assert_eq!(address("10.0.0.2").unwrap().shown, "10.0.0.2:25580");
    }

    fn tunnel_of(host: &str) -> (String, String) {
        (format!("https://{host}"), format!("wss://{host}/native"))
    }

    #[test]
    fn bad_addresses_say_why() {
        for bad in ["", "   ", "ftp://example.com", "example.com:0", "exa mple.com", "user@example.com", "mcrs://play", "a\"b.com:80"] {
            assert!(address(bad).is_err(), "{bad:?} parsed");
        }
    }

    /// Against a running mc-rs-server: `MCRS_TEST_SERVER=127.0.0.1:25580 cargo test -p launcher
    /// -- --ignored signs_in_at_a_running_server`. Signs up, refuses a wrong password, signs out.
    #[test]
    #[ignore]
    fn signs_in_at_a_running_server() {
        let addr = address(&std::env::var("MCRS_TEST_SERVER").expect("MCRS_TEST_SERVER")).unwrap();
        let token = login(&addr, "join_test", "secret1").unwrap();
        assert!(!token.is_empty());
        assert_eq!(login(&addr, "join_test", "wrong").unwrap_err(), "Wrong name or password.");
        logout(&addr, &token);
        let guest = login_guest(&addr, "").unwrap();
        assert!(!guest.is_empty());
        assert_eq!(login_guest(&addr, "join_test").unwrap_err(), "That name is taken here: sign in with its password, or pick another.");
        logout(&addr, &guest);
        let me = net::get_json(&format!("{}/api/me?token={token}", addr.http));
        assert!(me.is_err(), "the token still works after the sign-out: {me:?}");
    }

    #[test]
    fn the_viewers_reason_comes_from_its_log() {
        let log = |l: &str| vec!["INFO something".to_string(), l.to_string(), "WARN after".to_string()];
        assert_eq!(reason(&log("mc-rs viewer: disconnected: Signed in somewhere else.")).as_deref(), Some("Disconnected: Signed in somewhere else."));
        assert_eq!(
            reason(&log("mc-rs viewer: could not join wss://a.b/native: The server is full (4 players).")).as_deref(),
            Some("Could not join: The server is full (4 players).")
        );
        assert_eq!(reason(&log("ERROR viewer: panicked")), None);
    }
}
