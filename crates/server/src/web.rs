//! The web side: the page (web/index.html), the login API (accounts and guests), the game socket,
//! and the two probes the page's settings tuner times (tune.js): `/api/ping` and `/api/probe`, a
//! block of incompressible bytes for the line's speed; `/api/media`, a picture or video the
//! player hangs in the world from the page; `/native` is the launcher's socket (native.rs).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use std::net::SocketAddr;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;

use crate::hub::{Hub, ToBrowser};

const INDEX: &str = include_str!("../web/index.html");
const CLIENT: &str = include_str!("../web/client.js");
const AUDIO: &str = include_str!("../web/audio-worklet.js");
const PREFS: &str = include_str!("../web/prefs.js");
const LAG: &str = include_str!("../web/lag.js");
const TOUCH: &str = include_str!("../web/touch.js");
const TUNE: &str = include_str!("../web/tune.js");
const PICTURE: &str = include_str!("../web/picture.js");
/// The largest probe the page may ask for.
const PROBE_MAX_KB: usize = 8192;

pub fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/client.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], CLIENT) }))
        .route("/audio-worklet.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], AUDIO) }))
        .route("/prefs.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], PREFS) }))
        .route("/lag.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], LAG) }))
        .route("/touch.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], TOUCH) }))
        .route("/tune.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], TUNE) }))
        .route("/picture.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], PICTURE) }))
        .route("/api/info", get(info))
        .route("/api/ping", get(|| async { ([(header::CACHE_CONTROL, "no-store")], "pong") }))
        .route("/api/probe", get(probe))
        .route("/api/login", post(login))
        .route("/api/guest", post(guest))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/media", post(media).layer(DefaultBodyLimit::max(proto::MAX_MEDIA + 1)))
        .route("/ws", get(socket))
        .route("/native", get(crate::native::socket))
        .with_state(hub)
}

async fn info(State(hub): State<Arc<Hub>>) -> impl IntoResponse {
    Json(json!({
        "players": hub.names(),
        "max": hub.cfg.max_players,
        "open_registration": hub.cfg.open_registration,
        "join_password": !hub.cfg.join_password.is_empty(),
        "guests": hub.cfg.guests && hub.cfg.open_registration,
        "width": hub.cfg.width,
        "height": hub.cfg.height,
        "fps": hub.cfg.fps,
        "max_bitrate_kbps": hub.cfg.max_bitrate_kbps,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Deserialize)]
struct LoginBody {
    name: String,
    password: String,
    #[serde(default)]
    join: String,
}

/// A banned name or address gets the ban's line in place of a token.
fn banned(hub: &Hub, name: &str, token: &str, ip: &str) -> Option<Response> {
    let why = hub.refused(name, ip)?;
    hub.accounts.lock().unwrap().logout(token);
    Some((StatusCode::FORBIDDEN, Json(json!({"ok": false, "error": why}))).into_response())
}

async fn login(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginBody>,
) -> Response {
    let ip = crate::bans::client_ip(peer, &headers);
    let h = hub.clone();
    // Argon2 takes tens of milliseconds; keep it off the async threads.
    let result = tokio::task::spawn_blocking(move || {
        let cfg = &h.cfg;
        h.accounts.lock().unwrap().login(&body.name, &body.password, cfg.open_registration, &cfg.join_password, &body.join)
    })
    .await;
    match result {
        Ok(Ok((name, token))) => match banned(&hub, &name, &token, &ip) {
            Some(r) => r,
            None => Json(json!({"ok": true, "name": name, "token": token})).into_response(),
        },
        Ok(Err(e)) => {
            // A wrong guess costs a moment.
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            (StatusCode::UNAUTHORIZED, Json(json!({"ok": false, "error": e.message()}))).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
struct GuestBody {
    #[serde(default)]
    name: String,
    #[serde(default)]
    join: String,
}

async fn guest(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<GuestBody>,
) -> Response {
    let ip = crate::bans::client_ip(peer, &headers);
    let cfg = &hub.cfg;
    let result = hub.accounts.lock().unwrap().guest(&body.name, cfg.guests && cfg.open_registration, &cfg.join_password, &body.join);
    match result {
        Ok((name, token)) => match banned(&hub, &name, &token, &ip) {
            Some(r) => r,
            None => Json(json!({"ok": true, "name": name, "token": token, "guest": true})).into_response(),
        },
        Err(e) => {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            (StatusCode::UNAUTHORIZED, Json(json!({"ok": false, "error": e.message()}))).into_response()
        }
    }
}

/// `kb` kilobytes of bytes no proxy can compress (the Cloudflare tunnel would shrink zeros), never
/// cached, so the time the page takes to read them is the line's speed from here to there.
async fn probe(Query(q): Query<HashMap<String, String>>) -> Response {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    let kb = q.get("kb").and_then(|v| v.parse::<usize>().ok()).unwrap_or(256).clamp(1, PROBE_MAX_KB);
    let all = BYTES.get_or_init(|| {
        // xorshift64*: noise, and the same bytes every start.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        (0..PROBE_MAX_KB * 1024 / 8)
            .flat_map(|_| {
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                x.wrapping_mul(0x2545_F491_4F6C_DD1D).to_le_bytes()
            })
            .collect()
    });
    (
        [(header::CONTENT_TYPE, "application/octet-stream"), (header::CACHE_CONTROL, "no-store")],
        all[..kb * 1024].to_vec(),
    )
        .into_response()
}

#[derive(Deserialize)]
struct TokenBody {
    token: String,
}

async fn logout(State(hub): State<Arc<Hub>>, Json(body): Json<TokenBody>) -> impl IntoResponse {
    hub.accounts.lock().unwrap().logout(&body.token);
    Json(json!({"ok": true}))
}

async fn me(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>) -> Response {
    let name = q.get("token").and_then(|t| hub.accounts.lock().unwrap().check(t));
    match name {
        Some(n) => {
            let guest = !hub.accounts.lock().unwrap().is_account(&n);
            Json(json!({"ok": true, "name": n, "guest": guest})).into_response()
        }
        None => (StatusCode::UNAUTHORIZED, Json(json!({"ok": false}))).into_response(),
    }
}

/// `POST /api/media?token=..&name=file.png`, the file as the body: the player's session hangs it
/// where its crosshair is (hub.rs `upload_media`).
async fn media(State(hub): State<Arc<Hub>>, Query(q): Query<HashMap<String, String>>, body: Bytes) -> Response {
    let Some(name) = q.get("token").and_then(|t| hub.accounts.lock().unwrap().check(t)) else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"ok": false, "error": "sign in first"}))).into_response();
    };
    let file = q.get("name").map_or("", |n| n.as_str());
    match hub.upload_media(&name, file, &body) {
        Ok(id) => Json(json!({"ok": true, "id": id})).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"ok": false, "error": e}))).into_response(),
    }
}

async fn socket(
    State(hub): State<Arc<Hub>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(name) = q.get("token").and_then(|t| hub.accounts.lock().unwrap().check(t)) else {
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    };
    let ip = crate::bans::client_ip(peer, &headers);
    if let Some(why) = hub.refused(&name, &ip) {
        return (StatusCode::FORBIDDEN, why).into_response();
    }
    hub.seen(&name, &ip);
    let num = |k: &str| q.get(k).and_then(|v| v.parse().ok());
    let picture = hub.cfg.picture(num("w"), num("fps"), num("br"));
    ws.max_message_size(1 << 20).on_upgrade(move |socket| play(hub, name, picture, socket))
}

async fn play(hub: Arc<Hub>, name: String, picture: crate::config::Picture, socket: WebSocket) {
    use futures_util::{SinkExt, StreamExt};
    let (mut sink, mut stream) = socket.split();
    let (to_browser, mut out) = mpsc::unbounded_channel::<ToBrowser>();
    let (in_tx, incoming) = mpsc::unbounded_channel::<Result<Vec<u8>, String>>();
    let backlog = Arc::new(AtomicUsize::new(0));
    let b = backlog.clone();
    let writer = tokio::spawn(async move {
        while let Some(m) = out.recv().await {
            let r = match m {
                ToBrowser::Binary(v) => {
                    let n = v.len();
                    let r = sink.send(Message::Binary(v.into())).await;
                    b.fetch_sub(n, Ordering::Relaxed);
                    r
                }
                ToBrowser::Text(t) => sink.send(Message::Text(t.into())).await,
                ToBrowser::Close(reason) => {
                    let _ = sink.send(Message::Text(json!({"t": "closed", "text": reason}).to_string().into())).await;
                    let _ = sink.close().await;
                    break;
                }
            };
            if r.is_err() {
                break;
            }
        }
    });
    let reader = tokio::spawn(async move {
        while let Some(Ok(m)) = stream.next().await {
            let item = match m {
                Message::Binary(b) => Ok(b.to_vec()),
                Message::Text(t) => Err(t.to_string()),
                Message::Close(_) => break,
                _ => continue,
            };
            if in_tx.send(item).is_err() {
                break;
            }
        }
    });
    hub.run_session(name, picture, to_browser, incoming, backlog, None).await;
    reader.abort();
    // Let a closing message go out.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), writer).await;
}
