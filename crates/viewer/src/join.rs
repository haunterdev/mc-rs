//! A native session (`--join URL`, the launcher's Join): the viewer plays on a browser server
//! (crates/server) in its own window, rendering on this PC, for a line too slow for the video.
//! Only world and player data cross the network: the welcome (seed, where the player was, the
//! time), the saved and live block edits, the other players' states and names, chat. Sounds come
//! from the edits and the players' moves here, as in a streamed session.
//!
//! The connection is a WebSocket to the server's `/native` (wss through the Cloudflare tunnel,
//! ws on a local network), signed in with a token from the server's `/api/login`, which the
//! launcher fetches and hands over in `MCRS_JOIN_TOKEN`, never on the command line. Binary
//! messages carry whole `proto` frames both ways; one thread owns the socket and feeds a
//! [`Link`], so link.rs's `receive` and `send` run as they do for a streamed session. The JSON
//! the server's page would get (chat, player list, the reason it closed) arrives as `S_TEXT` and
//! goes to chat.rs.
//!
//! A line that drops (the tunnel resets a socket now and then) is joined again with the
//! same token up to [`REJOINS`] times; the second sign-in replaces the session the server still
//! holds, and the player keeps playing where they stand.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::{HandshakeError, Message, WebSocket};

use crate::link::{parse_welcome, Link};

/// The environment variable the sign-in token comes in.
pub const TOKEN_VAR: &str = "MCRS_JOIN_TOKEN";

/// Frames batched into one message to the server at most, in bytes.
const BATCH: usize = 64 << 10;
/// How long the socket thread waits for the server before it sends what is queued.
const POLL: Duration = Duration::from_millis(4);
/// A ping goes out this often, and the line counts as dead after this long without a word.
const PING: Duration = Duration::from_secs(15);
const SILENCE: Duration = Duration::from_secs(60);
/// Rejoins in a row before the game gives up; a session that lasted a minute starts the count
/// again.
const REJOINS: u32 = 5;

/// `--join URL`, with the token from [`TOKEN_VAR`].
#[derive(Clone, Debug)]
pub struct JoinArgs {
    pub url: String,
    pub token: String,
}

/// What a native session has besides its [`Link`]: the server's JSON for chat, and why the
/// server closed the connection.
#[derive(Resource)]
pub struct Joined {
    pub texts: Receiver<String>,
    pub ended: Arc<Mutex<Option<String>>>,
}

enum Conn {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Conn {
    fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Plain(s) => s,
            Conn::Tls(s) => s.get_ref(),
        }
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.write(buf),
            Conn::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Plain(s) => s.flush(),
            Conn::Tls(s) => s.flush(),
        }
    }
}

fn tls(host: &str, tcp: TcpStream) -> Result<Conn, String> {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| format!("{host}: {e}"))?;
    let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    Ok(Conn::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))))
}

fn would_block(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut))
}

/// The reason in a page's `{"t":"closed","text":...}`.
fn closed_reason(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    (v.get("t")?.as_str()? == "closed").then(|| v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string())
}

/// Connects, signs in and waits for the welcome; whatever follows it in the same message goes to
/// `in_tx` and `text_tx`. Returns the socket, reading with the [`POLL`] timeout, and the welcome;
/// the error is a sentence for the player.
fn open(args: &JoinArgs, in_tx: &Sender<(u8, Vec<u8>)>, text_tx: &Sender<String>) -> Result<(WebSocket<Conn>, Vec<u8>), String> {
    if args.token.is_empty() {
        return Err(format!("no sign-in token in {TOKEN_VAR}; join from the launcher's Join a server"));
    }
    let mut req = args.url.as_str().into_client_request().map_err(|e| format!("{} is not a server address ({e})", args.url))?;
    let bearer = HeaderValue::from_str(&format!("Bearer {}", args.token)).map_err(|_| "the sign-in token is malformed".to_string())?;
    req.headers_mut().insert("Authorization", bearer);
    let uri = req.uri().clone();
    let secure = match uri.scheme_str() {
        Some("wss") => true,
        Some("ws") => false,
        _ => return Err(format!("{} is not a ws:// or wss:// address", args.url)),
    };
    let host = uri.host().unwrap_or("").trim_start_matches('[').trim_end_matches(']').to_string();
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let unreachable = |e: &dyn std::fmt::Display| format!("could not reach {host}:{port} ({e})");
    let addrs: Vec<_> = (host.as_str(), port).to_socket_addrs().map_err(|e| unreachable(&e))?.collect();
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no address");
    let tcp = addrs
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, Duration::from_secs(10)).map_err(|e| last = e).ok())
        .ok_or_else(|| unreachable(&last))?;
    let _ = tcp.set_nodelay(true);
    tcp.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e| e.to_string())?;
    let conn = if secure { tls(&host, tcp)? } else { Conn::Plain(tcp) };
    let mut ws = match tungstenite::client(req, conn) {
        Ok((ws, _)) => ws,
        Err(HandshakeError::Failure(tungstenite::Error::Http(r))) if r.status() == 401 => {
            return Err("the server did not accept the sign-in; sign in again".into());
        }
        Err(HandshakeError::Failure(tungstenite::Error::Http(r))) => {
            return Err(format!("the server answered HTTP {} (is this an mc-rs server new enough for the launcher?)", r.status()));
        }
        Err(HandshakeError::Failure(e)) => return Err(unreachable(&e)),
        Err(HandshakeError::Interrupted(_)) => return Err("the server did not answer in time".into()),
    };

    // The welcome comes first; whatever follows it in the same message waits for the inbox.
    let mut welcome = None;
    while welcome.is_none() {
        let msg = match ws.read() {
            Ok(m) => m,
            Err(e) if would_block(&e) => return Err("the server did not let this player in within 30 seconds".into()),
            Err(e) => return Err(format!("the connection broke while joining ({e})")),
        };
        let bytes = match msg {
            Message::Binary(b) => b,
            Message::Close(_) => return Err("the server closed the connection".into()),
            _ => continue,
        };
        for (tag, body) in proto::split_frames(&bytes).ok_or("the server sent a broken frame")? {
            match tag {
                proto::S_WELCOME if welcome.is_none() => welcome = Some(body.to_vec()),
                proto::S_TEXT => {
                    let text = String::from_utf8_lossy(body).into_owned();
                    if let Some(reason) = closed_reason(&text) {
                        return Err(reason);
                    }
                    let _ = text_tx.send(text);
                }
                _ => {
                    let _ = in_tx.send((tag, body.to_vec()));
                }
            }
        }
    }
    // The survival protocol this viewer reads (survival/net.rs), once per connection.
    ws.send(Message::Binary(crate::survival::net::client_info_frame().into())).map_err(|e| format!("the connection broke while joining ({e})"))?;
    ws.get_ref().tcp().set_read_timeout(Some(POLL)).map_err(|e| e.to_string())?;
    Ok((ws, welcome.unwrap_or_default()))
}

/// Connects, signs in and waits for the welcome. Returns the link, the world seed and what
/// chat.rs reads; the error is a sentence for the player.
pub fn connect(args: &JoinArgs) -> Result<(Link, i64, Joined), String> {
    let (in_tx, inbox) = unbounded::<(u8, Vec<u8>)>();
    let (text_tx, texts) = unbounded::<String>();
    let (mut ws, body) = open(args, &in_tx, &text_tx)?;
    let w = parse_welcome(&body).map_err(|e| format!("the server's welcome did not read ({e})"))?;
    let seed = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("seed").and_then(|s| s.as_i64()))
        .ok_or("the server sent no world seed (it needs updating for the launcher)")?;
    eprintln!("mc-rs viewer: joined {} as {} (player {})", args.url, w.name, w.id);

    let closed = Arc::new(AtomicBool::new(false));
    let ended = Arc::new(Mutex::new(None));
    let (out, out_rx) = unbounded::<Vec<u8>>();
    let (c, e, again) = (closed.clone(), ended.clone(), args.clone());
    thread::Builder::new()
        .name("join-net".into())
        .spawn(move || {
            let why = run(&again, &mut ws, &out_rx, &in_tx, &text_tx);
            if let Ok(mut e) = e.lock() {
                e.get_or_insert(why);
            }
            let _ = ws.close(None);
            let _ = ws.flush();
            c.store(true, Ordering::Relaxed);
        })
        .map_err(|e| e.to_string())?;
    Ok((Link::over(out, inbox, closed, w), seed, Joined { texts, ended }))
}

/// How a connection ended: for good, or with a dropped line worth joining again.
enum End {
    Final(String),
    Dropped(String),
}

/// The socket thread: [`session`] after session, joining again after a dropped line. Returns why
/// it ended.
fn run(args: &JoinArgs, ws: &mut WebSocket<Conn>, out: &Receiver<Vec<u8>>, inbox: &Sender<(u8, Vec<u8>)>, texts: &Sender<String>) -> String {
    let mut tries = 0;
    loop {
        let start = Instant::now();
        let why = match session(ws, out, inbox, texts) {
            End::Final(why) => return why,
            End::Dropped(why) => why,
        };
        if start.elapsed() > Duration::from_secs(60) {
            tries = 0;
        }
        eprintln!("mc-rs viewer: {why}; joining again");
        loop {
            if tries == REJOINS {
                return why;
            }
            tries += 1;
            thread::sleep(Duration::from_secs(tries as u64));
            match open(args, inbox, texts) {
                Ok((again, _welcome)) => {
                    *ws = again;
                    eprintln!("mc-rs viewer: joined again");
                    let line = serde_json::json!({"t": "chat", "text": "The connection dropped; joined again", "color": "gray"});
                    let _ = texts.send(line.to_string());
                    break;
                }
                Err(e) => eprintln!("mc-rs viewer: joining again failed: {e}"),
            }
        }
    }
}

/// One connection: queued frames out, the server's messages in, until either side ends.
fn session(ws: &mut WebSocket<Conn>, out: &Receiver<Vec<u8>>, inbox: &Sender<(u8, Vec<u8>)>, texts: &Sender<String>) -> End {
    let mut last_heard = Instant::now();
    let mut last_ping = Instant::now();
    loop {
        // The server answers a ping even when nothing else happens; a long silence is a dead line.
        if last_ping.elapsed() > PING {
            last_ping = Instant::now();
            if let Err(e) = ws.send(Message::Ping(Vec::new().into())) {
                return End::Dropped(format!("the connection broke ({e})"));
            }
        }
        let mut batch = Vec::new();
        loop {
            match out.try_recv() {
                Ok(f) => {
                    batch.extend(f);
                    if batch.len() >= BATCH {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return End::Final("the game closed".into()),
            }
        }
        if !batch.is_empty() {
            if let Err(e) = ws.send(Message::Binary(batch.into())) {
                return End::Dropped(format!("the connection broke ({e})"));
            }
        }
        match ws.read() {
            Ok(Message::Binary(bytes)) => {
                last_heard = Instant::now();
                let Some(frames) = proto::split_frames(&bytes) else { return End::Final("the server sent a broken frame".into()) };
                for (tag, body) in frames {
                    if tag == proto::S_TEXT {
                        let text = String::from_utf8_lossy(body).into_owned();
                        if let Some(reason) = closed_reason(&text) {
                            return End::Final(reason);
                        }
                        let _ = texts.send(text);
                    } else if inbox.send((tag, body.to_vec())).is_err() {
                        return End::Final("the game closed".into());
                    }
                }
            }
            Ok(Message::Close(_)) => return End::Final("the server closed the connection".into()),
            Ok(_) => last_heard = Instant::now(),
            Err(e) if would_block(&e) => {
                if last_heard.elapsed() > SILENCE {
                    return End::Dropped("the server stopped answering".into());
                }
            }
            Err(e) => return End::Dropped(format!("the connection broke ({e})")),
        }
    }
}

/// Ends the game when the server closed the connection, with the reason on stderr (the
/// launcher's log, which it shows) and exit code 3; runs before link.rs's `receive`, whose
/// plain exit then loses to this one.
pub fn watch(joined: Res<Joined>, mut exit: MessageWriter<AppExit>, mut told: Local<bool>) {
    let Ok(ended) = joined.ended.lock() else { return };
    let Some(why) = ended.as_ref().filter(|_| !*told) else { return };
    if why == "the game closed" {
        return;
    }
    *told = true;
    eprintln!("mc-rs viewer: disconnected: {why}");
    exit.write(AppExit::from_code(3));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_message_gives_its_reason() {
        assert_eq!(closed_reason(r#"{"t":"closed","text":"The server is full (4 players)."}"#).as_deref(), Some("The server is full (4 players)."));
        assert_eq!(closed_reason(r#"{"t":"chat","from":"a","text":"closed"}"#), None);
        assert_eq!(closed_reason("not json"), None);
    }

    /// A stand-in server on a local socket: checks the token, sends the welcome with a chat line,
    /// reads the client's state, then closes the session with a reason.
    #[test]
    fn joins_trades_frames_and_hears_why_it_was_closed() {
        use tungstenite::handshake::server::{ErrorResponse, Request, Response};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for wanted in [false, true] {
                let (tcp, _) = listener.accept().unwrap();
                let check = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                    match req.headers().get("Authorization").and_then(|v| v.to_str().ok()) {
                        Some("Bearer good") => Ok(resp),
                        _ => Err(tungstenite::http::Response::builder().status(401).body(None).unwrap()),
                    }
                };
                let Ok(mut ws) = tungstenite::accept_hdr(tcp, check) else {
                    assert!(!wanted, "the good token was refused");
                    continue;
                };
                let welcome = br#"{"id":3,"name":"friend","pos":[1.5,70,2.5],"time":6000,"seed":-42}"#;
                let mut msg = proto::frame(proto::S_WELCOME, welcome);
                msg.extend(proto::frame(proto::S_TEXT, br#"{"t":"chat","from":"","text":"friend joined the game"}"#));
                msg.extend(proto::frame(proto::S_JOIN, b"\x07\0\0\0xy"));
                ws.send(Message::Binary(msg.into())).unwrap();
                // V_CLIENT_INFO once, straight after the welcome, then the state.
                let mut tags = Vec::new();
                while !tags.contains(&proto::V_STATE) {
                    if let Message::Binary(b) = ws.read().unwrap() {
                        for (tag, body) in proto::split_frames(&b).unwrap() {
                            if tag == proto::V_CLIENT_INFO {
                                assert_eq!(proto::ClientInfo::decode(body), Some(proto::ClientInfo { version: 1 }));
                            }
                            tags.push(tag);
                        }
                    }
                }
                assert_eq!(tags, [proto::V_CLIENT_INFO, proto::V_STATE]);
                let closed = br#"{"t":"closed","text":"Signed in somewhere else."}"#;
                ws.send(Message::Binary(proto::frame(proto::S_TEXT, closed).into())).unwrap();
                let _ = ws.close(None);
                while ws.read().is_ok() {}
            }
        });
        let url = format!("ws://127.0.0.1:{port}/native");
        let err = connect(&JoinArgs { url: url.clone(), token: "bad".into() }).err().unwrap();
        assert!(err.contains("did not accept the sign-in"), "{err}");
        let (link, seed, joined) = connect(&JoinArgs { url, token: "good".into() }).unwrap();
        assert_eq!((seed, link.welcome.id, link.welcome.name.as_str(), link.welcome.world_time), (-42, 3, "friend", 6000));
        assert!(joined.texts.recv_timeout(Duration::from_secs(5)).unwrap().contains("friend joined the game"));
        link.send(proto::V_STATE, &proto::PlayerState::default().encode());
        let stop = Instant::now() + Duration::from_secs(10);
        while joined.ended.lock().unwrap().is_none() && Instant::now() < stop {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(joined.ended.lock().unwrap().as_deref(), Some("Signed in somewhere else."));
        server.join().unwrap();
    }

    /// Against a running mc-rs-server: `MCRS_JOIN_URL=ws://127.0.0.1:25580/native` and a token
    /// from its `/api/login` in `MCRS_JOIN_TOKEN`, then `cargo test -p viewer -- --ignored
    /// joins_a_running_server`. Joins, stands somewhere, places a block and asks for the seed.
    #[test]
    #[ignore]
    fn joins_a_running_server() {
        let url = std::env::var("MCRS_JOIN_URL").expect("MCRS_JOIN_URL");
        let token = std::env::var(TOKEN_VAR).expect("MCRS_JOIN_TOKEN");
        let (link, seed, joined) = connect(&JoinArgs { url, token }).unwrap();
        let s = proto::PlayerState { pos: [8.5, 80.0, 8.5], flags: proto::ON_GROUND, ..Default::default() };
        link.send(proto::V_STATE, &s.encode());
        link.send(proto::V_BLOCKS, &proto::encode_blocks(&[((8, 81, 9), 1 << 4)]));
        link.send(proto::V_TEXT, br#"{"t":"chat","text":"/seed"}"#);
        let want = format!("Seed: {seed}");
        let stop = Instant::now() + Duration::from_secs(10);
        let mut heard = Vec::new();
        while Instant::now() < stop && !heard.iter().any(|t: &String| t.contains(&want)) {
            if let Ok(t) = joined.texts.recv_timeout(Duration::from_millis(100)) {
                eprintln!("{t}");
                heard.push(t);
            }
        }
        assert!(heard.iter().any(|t| t.contains(&want)), "no {want:?} in {heard:?}");
    }

    #[test]
    fn a_bad_address_is_a_sentence() {
        let args = |url: &str| JoinArgs { url: url.into(), token: "t".into() };
        let err = |url: &str| connect(&args(url)).err().unwrap();
        assert!(err("https://example.com/native").contains("not a ws:// or wss://"), "{}", err("https://example.com/native"));
        assert!(err("not a url at all").contains("not a server address"));
    }
}
