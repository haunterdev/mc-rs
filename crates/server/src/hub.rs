//! The players online and their sessions. A browser that signs in gets one viewer process
//! (`viewer --stream`), which connects back over the local session socket; from then on the hub
//! relays the viewer's video, audio and focus to the browser, the browser's input to the viewer,
//! and every player's state and block edits to the other viewers, while the world keeps the edits
//! and positions on disk.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proto::PlayerState;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, Notify};

use crate::accounts::Accounts;
use crate::config::Config;
use crate::game::GameMsg;
use crate::world::World;

/// What goes out on a browser's socket.
pub enum ToBrowser {
    Binary(Vec<u8>),
    Text(String),
    Close(String),
}

/// Browser binary messages: a type byte, then the payload.
pub const B_VIDEO: u8 = 1;
pub const B_AUDIO: u8 = 2;

/// Video waiting for a slow browser past this much playing time is dropped until the next key
/// frame: queued frames are lag, and a picture a moment late beats one that falls further behind.
const BACKLOG_SECONDS: f64 = 0.1;

/// A dropped line is not a leave: a native viewer joins again with its token after waits of 1, 2,
/// 3, 4 and 5 s (`join.rs` `REJOINS`), so a leave is held this long, the whole retry chain, and
/// cancelled when the player comes back. A player who really left is announced once it runs out.
const REJOIN_GRACE: Duration = Duration::from_secs(15);

struct Online {
    name: String,
    to_viewer: mpsc::UnboundedSender<Vec<u8>>,
    to_browser: mpsc::UnboundedSender<ToBrowser>,
    state: Option<PlayerState>,
    kick: Arc<Notify>,
}

pub struct Hub {
    pub cfg: Config,
    /// Shared with the game's terrain, which writes the server's own block changes.
    pub world: Arc<Mutex<World>>,
    pub accounts: Mutex<Accounts>,
    online: Mutex<HashMap<u32, Online>>,
    /// Lowercase names whose leave is held for [`REJOIN_GRACE`]; a join that finds its name here
    /// was a dropped line coming back, and says nothing either.
    held: Mutex<std::collections::HashSet<String>>,
    /// Session tokens of viewers starting up, waiting for their connection.
    pending: Mutex<HashMap<String, oneshot::Sender<(TcpStream, Vec<u8>)>>>,
    pub session_addr: SocketAddr,
    next_id: AtomicU32,
    /// The data folder, for latency.log and server.json.
    pub dir: std::path::PathBuf,
    latency_log: Mutex<Option<std::fs::File>>,
    /// The operators (commands.rs), who may run every command whatever server.json "commands"
    /// says; starts as server.json "ops".
    pub ops: Mutex<Vec<String>>,
    /// `bans.json` (bans.rs).
    pub bans: Mutex<crate::bans::Bans>,
    /// Each name's address as last seen at a game socket (lowercase name), for `/ban <name> ip true`.
    pub ips: Mutex<HashMap<String, String>>,
    /// The game thread's inbox (game/mod.rs), set once it starts.
    game: std::sync::OnceLock<std::sync::mpsc::Sender<GameMsg>>,
    /// The backup thread's inbox (backup.rs), set once it starts.
    backups: std::sync::OnceLock<std::sync::mpsc::Sender<crate::backup::Request>>,
}

/// One session online, as commands.rs sees it.
pub struct Session {
    pub id: u32,
    pub name: String,
    pub state: Option<PlayerState>,
    pub to_viewer: mpsc::UnboundedSender<Vec<u8>>,
    pub to_browser: mpsc::UnboundedSender<ToBrowser>,
}

fn random_token() -> String {
    format!("{:016x}{:016x}", crate::config::random_u64(), crate::config::random_u64())
}

pub(crate) async fn read_frame(r: &mut (impl AsyncReadExt + Unpin)) -> std::io::Result<(u8, Vec<u8>)> {
    let len = r.read_u32_le().await? as usize;
    if len == 0 || len > proto::MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("frame length {len}")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    let tag = body.remove(0);
    Ok((tag, body))
}

/// A session's game connection: the viewer's local socket, or a native client's pipe (native.rs).
trait GameIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> GameIo for T {}

/// Until the session's viewer exits; never for a native client, which has none here.
async fn wait_child(child: &mut Option<tokio::process::Child>) {
    match child {
        Some(c) => {
            let _ = c.wait().await;
        }
        None => std::future::pending().await,
    }
}

/// The saved paintings for a joining viewer: each media file once, then the paintings.
fn painting_frames(w: &crate::world::World) -> Vec<u8> {
    let paintings = w.paintings();
    let mut out = Vec::new();
    let mut sent: Vec<&str> = Vec::new();
    for p in &paintings {
        if sent.contains(&p.media.as_str()) {
            continue;
        }
        sent.push(&p.media);
        if let Some(bytes) = w.media(&p.media) {
            out.extend(proto::frame(proto::S_MEDIA, &proto::encode_media(&p.media, &bytes)));
        }
    }
    for p in &paintings {
        out.extend(proto::frame(proto::S_PAINTING, &p.encode()));
    }
    out
}

impl Hub {
    pub async fn new(cfg: Config, world: World, accounts: Accounts, dir: std::path::PathBuf) -> std::io::Result<(Arc<Hub>, TcpListener)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let ops = Mutex::new(cfg.ops.clone());
        let bans = Mutex::new(crate::bans::Bans::load(&dir));
        let hub = Arc::new(Hub {
            cfg,
            world: Arc::new(Mutex::new(world)),
            accounts: Mutex::new(accounts),
            online: Mutex::new(HashMap::new()),
            held: Mutex::new(std::collections::HashSet::new()),
            pending: Mutex::new(HashMap::new()),
            session_addr: listener.local_addr()?,
            next_id: AtomicU32::new(1),
            dir,
            latency_log: Mutex::new(None),
            ops,
            bans,
            ips: Mutex::new(HashMap::new()),
            game: std::sync::OnceLock::new(),
            backups: std::sync::OnceLock::new(),
        });
        Ok((hub, listener))
    }

    /// Connects the game thread; every session from then on reaches it.
    pub fn set_game(&self, tx: std::sync::mpsc::Sender<GameMsg>) {
        let _ = self.game.set(tx);
    }

    pub(crate) fn has_game(&self) -> bool {
        self.game.get().is_some()
    }

    pub(crate) fn to_game(&self, m: GameMsg) {
        if let Some(tx) = self.game.get() {
            let _ = tx.send(m);
        }
    }

    /// Has the game thread write its module files now and waits up to `timeout` for it; true
    /// when it answered, or when there is no game thread.
    pub fn save_game(&self, timeout: std::time::Duration) -> bool {
        let Some(tx) = self.game.get() else { return true };
        let (done, wait) = std::sync::mpsc::channel();
        if tx.send(GameMsg::Save { done }).is_err() {
            return false;
        }
        wait.recv_timeout(timeout).is_ok()
    }

    /// Connects the backup thread (backup.rs).
    pub fn set_backups(&self, tx: std::sync::mpsc::Sender<crate::backup::Request>) {
        let _ = self.backups.set(tx);
    }

    /// Asks the backup thread for a backup; false when there is none.
    pub fn request_backup(&self, from: Option<u32>) -> bool {
        self.backups.get().is_some_and(|tx| tx.send(crate::backup::Request { from }).is_ok())
    }

    /// Accepts viewer connections and hands each to the browser session that started it.
    pub async fn accept_sessions(self: Arc<Hub>, listener: TcpListener) {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { continue };
            let hub = self.clone();
            tokio::spawn(async move {
                let _ = stream.set_nodelay(true);
                let hello = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut stream)).await;
                let Ok(Ok((proto::V_HELLO, token))) = hello else { return };
                let token = String::from_utf8_lossy(&token).into_owned();
                let waiting = hub.pending.lock().unwrap().remove(&token);
                if let Some(tx) = waiting {
                    let _ = tx.send((stream, Vec::new()));
                }
            });
        }
    }

    /// Appends one JSON line to latency.log: who, what reported it, and the numbers.
    pub fn log_latency(&self, name: &str, source: &str, mut v: serde_json::Value) {
        use std::io::Write;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        if let Some(o) = v.as_object_mut() {
            o.insert("time".into(), json!(now));
            o.insert("player".into(), json!(name));
            o.insert("source".into(), json!(source));
        }
        let Ok(mut f) = self.latency_log.lock() else { return };
        if f.is_none() {
            *f = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("latency.log")).ok();
        }
        if let Some(file) = f.as_mut() {
            let _ = writeln!(file, "{v}");
        }
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.online.lock().unwrap().values().map(|o| o.name.clone()).collect();
        v.sort_by_key(|n| n.to_ascii_lowercase());
        v
    }

    /// Everyone online, sorted by name.
    pub fn sessions(&self) -> Vec<Session> {
        let mut v: Vec<Session> = self
            .online
            .lock()
            .unwrap()
            .iter()
            .map(|(&id, o)| Session { id, name: o.name.clone(), state: o.state, to_viewer: o.to_viewer.clone(), to_browser: o.to_browser.clone() })
            .collect();
        v.sort_by_key(|s| s.name.to_ascii_lowercase());
        v
    }

    pub fn count(&self) -> usize {
        self.online.lock().unwrap().len()
    }

    /// Sends a text message to every browser.
    pub fn tell_all(&self, msg: &serde_json::Value) {
        let text = msg.to_string();
        for o in self.online.lock().unwrap().values() {
            let _ = o.to_browser.send(ToBrowser::Text(text.clone()));
        }
    }

    fn announce_players(&self) {
        self.tell_all(&json!({"t": "players", "list": self.names()}));
    }

    pub fn chat(&self, from: &str, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).take(256).collect();
        if text.trim().is_empty() {
            return;
        }
        if from.is_empty() {
            println!("* {text}");
        } else {
            println!("<{from}> {text}");
        }
        self.tell_all(&json!({"t": "chat", "from": from, "text": text}));
    }

    /// To every viewer but `except`'s.
    /// A file the session player uploaded from the page: kept, sent to every viewer, and hung by
    /// the player's own where its crosshair is. The error is for the page.
    pub fn upload_media(&self, name: &str, file_name: &str, bytes: &[u8]) -> Result<String, String> {
        if bytes.len() > proto::MAX_MEDIA {
            return Err(format!("That file is {} MB; the limit is {} MB.", bytes.len() >> 20, proto::MAX_MEDIA >> 20));
        }
        let ext = file_name.rsplit_once('.').map_or("", |e| e.1);
        let id = proto::media_id(bytes, ext);
        if !proto::valid_media_id(&id) || !self.world.lock().unwrap().store_media(&id, bytes) {
            return Err("The server could not keep that file.".into());
        }
        let online = self.online.lock().unwrap();
        let Some(me) = online.values().find(|o| o.name == name) else { return Err("Join the game first.".into()) };
        let media = proto::frame(proto::S_MEDIA, &proto::encode_media(&id, bytes));
        for o in online.values() {
            let _ = o.to_viewer.send(media.clone());
        }
        let _ = me.to_viewer.send(proto::frame(proto::S_HANG, id.as_bytes()));
        println!("[{name}] uploaded {file_name} ({} KB) as {id}", bytes.len() >> 10);
        Ok(id)
    }

    fn to_viewers(&self, except: u32, frame: Vec<u8>) {
        for (&id, o) in self.online.lock().unwrap().iter() {
            if id != except {
                let _ = o.to_viewer.send(frame.clone());
            }
        }
    }

    /// Ends whatever session `name` has open, with `reason` on their screen: a second sign-in
    /// replaces the first, and `/kick` and `/ban` end one.
    pub fn kick(&self, name: &str, reason: &str) {
        for o in self.online.lock().unwrap().values() {
            if o.name.eq_ignore_ascii_case(name) {
                let _ = o.to_browser.send(ToBrowser::Close(reason.into()));
                o.kick.notify_one();
            }
        }
    }

    /// Why `name` from `ip` may not play (bans.rs), or None.
    pub fn refused(&self, name: &str, ip: &str) -> Option<String> {
        self.bans.lock().unwrap().refusal(name, Some(ip))
    }

    /// Notes where `name` connects from, for `/ban <name> ip true` and `/ban-ip <name>`.
    pub fn seen(&self, name: &str, ip: &str) {
        self.ips.lock().unwrap().insert(name.to_ascii_lowercase(), ip.to_string());
    }

    /// Starts `viewer --stream` for a browser player and waits for it to connect back.
    async fn start_viewer(
        &self,
        name: &str,
        picture: crate::config::Picture,
        to_browser: &mpsc::UnboundedSender<ToBrowser>,
    ) -> Option<(TcpStream, tokio::process::Child)> {
        let Some(viewer) = self.cfg.viewer_path() else {
            let _ = to_browser.send(ToBrowser::Close("The host has no viewer exe set up (server.json \"viewer\").".into()));
            eprintln!("no viewer exe found; set \"viewer\" in server.json");
            return None;
        };
        let (w, h, fps) = (picture.width, picture.height, picture.fps);
        let token = random_token();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(token.clone(), tx);
        let mut cmd = tokio::process::Command::new(&viewer);
        cmd.args([
            "--stream".to_string(),
            self.session_addr.to_string(),
            "--session".into(),
            token.clone(),
            "--seed".into(),
            self.cfg.seed.to_string(),
            "--size".into(),
            format!("{w}x{h}"),
            "--fps".into(),
            fps.to_string(),
            "--encoder".into(),
            self.cfg.encoder.clone(),
            "--bitrate".into(),
            picture.bitrate_kbps.to_string(),
            "--radius".into(),
            self.cfg.render_distance.to_string(),
            "--workers".into(),
            self.cfg.workers.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.pending.lock().unwrap().remove(&token);
                eprintln!("could not start {}: {e}", viewer.display());
                let _ = to_browser.send(ToBrowser::Close("The host could not start a game session.".into()));
                return None;
            }
        };
        for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|p| Box::new(p) as _)]
            .into_iter()
            .flatten()
        {
            let who = name.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(pipe).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.trim().is_empty() {
                        println!("[{who}] {line}");
                    }
                }
            });
        }
        let _ = to_browser.send(ToBrowser::Text(json!({"t": "status", "text": "Starting your game..."}).to_string()));
        let stream = tokio::select! {
            r = tokio::time::timeout(Duration::from_secs(90), rx) => match r {
                Ok(Ok((s, _))) => s,
                _ => {
                    self.pending.lock().unwrap().remove(&token);
                    let _ = to_browser.send(ToBrowser::Close("The game session did not start in time.".into()));
                    return None;
                }
            },
            _ = child.wait() => {
                self.pending.lock().unwrap().remove(&token);
                let _ = to_browser.send(ToBrowser::Close("The game session stopped while starting; the host log says why.".into()));
                return None;
            }
        };
        Some((stream, child))
    }

    /// Runs one player's session for as long as the browser and the viewer both live. `incoming`
    /// carries what the browser sends: binary input or JSON text. A native client (native.rs)
    /// brings its own end of the game connection in `native`, and no viewer starts on this PC.
    pub async fn run_session(
        self: Arc<Hub>,
        name: String,
        picture: crate::config::Picture,
        to_browser: mpsc::UnboundedSender<ToBrowser>,
        mut incoming: mpsc::UnboundedReceiver<Result<Vec<u8>, String>>,
        backlog: Arc<AtomicUsize>,
        native: Option<tokio::io::DuplexStream>,
    ) {
        let say = |msg: serde_json::Value| {
            let _ = to_browser.send(ToBrowser::Text(msg.to_string()));
        };
        self.kick(&name, "Signed in somewhere else.");
        if self.count() >= self.cfg.max_players {
            let _ = to_browser.send(ToBrowser::Close(format!("The server is full ({} players).", self.cfg.max_players)));
            return;
        }
        let (w, h, fps) = (picture.width, picture.height, picture.fps);
        let backlog_limit = ((picture.bitrate_kbps as f64 * 1000.0 / 8.0 * BACKLOG_SECONDS) as usize).max(48 << 10);
        let backlog_drops = Arc::new(AtomicU32::new(0));
        let backlog_peak = Arc::new(AtomicUsize::new(0));
        let (stream, mut child): (Box<dyn GameIo>, _) = match native {
            Some(s) => (Box::new(s), None),
            None => match self.start_viewer(&name, picture, &to_browser).await {
                Some((s, c)) => (Box::new(s), Some(c)),
                None => return,
            },
        };
        let (mut reader, mut writer) = tokio::io::split(stream);

        // Welcome, the saved edits and who is here, before this player joins the relay.
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (saved, edits, time, paintings) = {
            let w = self.world.lock().unwrap();
            (w.player(&name), w.all_edits(), w.time(), painting_frames(&w))
        };
        let welcome = json!({
            "id": id,
            "name": name,
            "pos": saved.and_then(|s| s.pos),
            "yaw": saved.map_or(0.0, |s| s.yaw),
            "pitch": saved.map_or(0.0, |s| s.pitch),
            "flying": saved.is_some_and(|s| s.flying),
            "time": time,
            // A native client generates the terrain itself.
            "seed": self.cfg.seed,
        });
        let mut first = proto::frame(proto::S_WELCOME, welcome.to_string().as_bytes());
        for chunk in edits.chunks(50_000) {
            first.extend(proto::frame(proto::S_BLOCKS, &proto::encode_blocks(chunk)));
        }
        first.extend(paintings);
        let (to_viewer, mut viewer_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let kick = Arc::new(Notify::new());
        // The same name still online is a session this one replaces (a rejoin that beat the old
        // line's close): it leaves without a word (leave), so this join says nothing either.
        let replacing;
        {
            let mut online = self.online.lock().unwrap();
            replacing = online.values().any(|o| o.name.eq_ignore_ascii_case(&name));
            for (&other, o) in online.iter() {
                let mut join = other.to_le_bytes().to_vec();
                join.extend_from_slice(o.name.as_bytes());
                first.extend(proto::frame(proto::S_JOIN, &join));
                if let Some(s) = o.state {
                    let mut st = other.to_le_bytes().to_vec();
                    st.extend_from_slice(&s.encode());
                    first.extend(proto::frame(proto::S_STATE, &st));
                }
                let mut join = id.to_le_bytes().to_vec();
                join.extend_from_slice(name.as_bytes());
                let _ = o.to_viewer.send(proto::frame(proto::S_JOIN, &join));
            }
            online.insert(id, Online { name: name.clone(), to_viewer: to_viewer.clone(), to_browser: to_browser.clone(), state: None, kick: kick.clone() });
        }
        let mode = self.world.lock().unwrap().game_mode(&name);
        self.to_game(GameMsg::Join { id, name: name.clone(), to_viewer: to_viewer.clone(), mode, survival_client: false });
        first.extend(crate::commands::join_frames(&self, &name, saved.is_some_and(|s| s.pos.is_some())));
        let _ = to_viewer.send(first);
        println!("{name} joined ({} online)", self.count());
        say(json!({"t": "welcome", "name": name, "width": w, "height": h, "fps": fps, "bitrate_kbps": picture.bitrate_kbps}));
        self.log_latency(&name, "server", json!({"event": "join", "width": w, "height": h, "fps": fps, "bitrate_kbps": picture.bitrate_kbps, "backlog_limit": backlog_limit}));
        self.announce_join(&name, replacing);

        let writer_task = tokio::spawn(async move {
            while let Some(f) = viewer_rx.recv().await {
                if writer.write_all(&f).await.is_err() {
                    break;
                }
            }
        });

        let hub = self.clone();
        let tb = to_browser.clone();
        let who = name.clone();
        let backlog_v = backlog.clone();
        let (drops_v, peak_v) = (backlog_drops.clone(), backlog_peak.clone());
        let viewer_task = tokio::spawn(async move {
            let mut waiting_key = false;
            loop {
                let Ok((tag, body)) = read_frame(&mut reader).await else { break };
                match tag {
                    proto::V_VIDEO => {
                        let key = body.first().is_some_and(|f| f & proto::VIDEO_KEY != 0);
                        let queued = backlog_v.load(Ordering::Relaxed);
                        peak_v.fetch_max(queued, Ordering::Relaxed);
                        if queued > backlog_limit && !key {
                            waiting_key = true;
                        }
                        if waiting_key && !key {
                            drops_v.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        waiting_key = false;
                        let mut msg = Vec::with_capacity(body.len() + 1);
                        msg.push(B_VIDEO);
                        msg.extend_from_slice(&body);
                        backlog_v.fetch_add(msg.len(), Ordering::Relaxed);
                        let _ = tb.send(ToBrowser::Binary(msg));
                    }
                    proto::V_AUDIO => {
                        if backlog_v.load(Ordering::Relaxed) > backlog_limit * 2 {
                            continue;
                        }
                        let mut msg = Vec::with_capacity(body.len() + 1);
                        msg.push(B_AUDIO);
                        msg.extend_from_slice(&body);
                        backlog_v.fetch_add(msg.len(), Ordering::Relaxed);
                        let _ = tb.send(ToBrowser::Binary(msg));
                    }
                    proto::V_STATE => {
                        let Some(s) = PlayerState::decode(&body) else { continue };
                        if let Some(o) = hub.online.lock().unwrap().get_mut(&id) {
                            o.state = Some(s);
                        }
                        hub.world.lock().unwrap().remember(&who, &s);
                        let mut st = id.to_le_bytes().to_vec();
                        st.extend_from_slice(&body);
                        hub.to_viewers(id, proto::frame(proto::S_STATE, &st));
                        hub.to_game(GameMsg::State { id, state: s });
                    }
                    proto::V_BLOCKS => {
                        let changes = proto::decode_blocks(&body);
                        if changes.is_empty() {
                            continue;
                        }
                        let gone = hub.world.lock().unwrap().apply(&changes);
                        hub.to_viewers(id, proto::frame(proto::S_BLOCKS, &body));
                        for p in gone {
                            hub.to_viewers(u32::MAX, proto::frame(proto::S_PAINTING, &p.encode()));
                        }
                        hub.to_game(GameMsg::Blocks { id, changes });
                    }
                    // Survival frames (proto survival.rs) are the game's.
                    60..=99 => hub.to_game(GameMsg::Frame { id, tag, body }),
                    proto::V_MEDIA => {
                        let Some((mid, bytes)) = proto::decode_media(&body) else { continue };
                        if hub.world.lock().unwrap().store_media(&mid, bytes) {
                            hub.to_viewers(id, proto::frame(proto::S_MEDIA, &body));
                        } else {
                            println!("[{who}] sent media {mid} that does not match its name or is too large");
                        }
                    }
                    proto::V_PAINTING => {
                        let Some(p) = proto::Painting::decode(&body) else { continue };
                        if hub.world.lock().unwrap().paint(p) {
                            hub.to_viewers(id, proto::frame(proto::S_PAINTING, &body));
                        }
                    }
                    proto::V_FOCUS => {
                        let f = body.first().copied().unwrap_or(0);
                        let _ = tb.send(ToBrowser::Text(json!({"t": "focus", "state": f}).to_string()));
                    }
                    proto::V_LOG => {
                        let line = String::from_utf8_lossy(&body);
                        match line.strip_prefix("stats ").and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok()) {
                            Some(v) => hub.log_latency(&who, "viewer", v),
                            None => println!("[{who}] {line}"),
                        }
                    }
                    _ => {}
                }
            }
        });

        // The browser side: input to the viewer, chat and pings answered here.
        let reason = loop {
            tokio::select! {
                m = incoming.recv() => match m {
                    Some(Ok(bin)) => {
                        let _ = to_viewer.send(proto::frame(proto::S_INPUT, &bin));
                    }
                    Some(Err(text)) => {
                        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
                        match v.get("t").and_then(|t| t.as_str()) {
                            Some("chat") => crate::commands::chat(&self, id, &name, v.get("text").and_then(|t| t.as_str()).unwrap_or("")),
                            Some("ping") => say(json!({"t": "pong", "ts": v.get("ts")})),
                            // The tuner's new picture, within the host's limits as on join; the
                            // session restarts its encoder at it and the page follows.
                            Some("picture") => {
                                let num = |k: &str| v.get(k).and_then(|x| x.as_u64()).map(|x| x as u32);
                                let p = self.cfg.picture(num("w"), num("fps"), num("br"));
                                let _ = to_viewer.send(proto::frame(proto::S_PICTURE, &proto::encode_picture([p.width, p.height, p.fps, p.bitrate_kbps])));
                                say(json!({"t": "picture", "width": p.width, "height": p.height, "fps": p.fps, "bitrate_kbps": p.bitrate_kbps}));
                                self.log_latency(&name, "server", json!({"event": "picture", "picture": format!("{}x{}@{} {}k", p.width, p.height, p.fps, p.bitrate_kbps)}));
                            }
                            Some("stats") => {
                                let mut v = v.clone();
                                if let Some(o) = v.as_object_mut() {
                                    o.remove("t");
                                    o.insert("server_backlog_drops".into(), json!(backlog_drops.swap(0, Ordering::Relaxed)));
                                    o.insert("server_backlog_peak".into(), json!(backlog_peak.swap(0, Ordering::Relaxed)));
                                    o.insert("server_backlog_limit".into(), json!(backlog_limit));
                                }
                                self.log_latency(&name, "browser", v);
                            }
                            _ => {}
                        }
                    }
                    None => break "left",
                },
                _ = wait_child(&mut child) => break "game session ended",
                _ = kick.notified() => break "closed by the server",
            }
        };
        viewer_task.abort();
        writer_task.abort();
        if let Some(c) = child.as_mut() {
            let _ = c.kill().await;
        }
        if reason == "game session ended" {
            let _ = to_browser.send(ToBrowser::Close("Your game session ended; the host log says why.".into()));
        }
        self.leave(id, &name, reason);
    }

    /// A session ended: drop it, keep its state, free its player model and tell everyone `name`
    /// left (`NetHandlerPlayServer.onDisconnect`'s `multiplayer.player.left` line, then
    /// `PlayerList.playerLoggedOut`'s player-list removal). A session whose name already has a
    /// newer session (a duplicate login, or a rejoin that beat this close) was replaced and says
    /// nothing. Any other leave is held for [`REJOIN_GRACE`] first, so a line that only dropped
    /// and joined again announces nothing; a player who really left is announced once it is over.
    fn leave(self: &Arc<Hub>, id: u32, name: &str, reason: &str) {
        self.leave_held(id, name, reason, REJOIN_GRACE);
    }

    /// "`name` joined the game" (`PlayerList.initializeConnectionToPlayer`), unless the join only
    /// replaces a session of the same name (`replacing`) or ends a held leave: a dropped line
    /// that came back says nothing on either side.
    fn announce_join(&self, name: &str, replacing: bool) {
        if !self.held.lock().unwrap().remove(&name.to_ascii_lowercase()) && !replacing {
            self.chat("", &format!("{name} joined the game"));
        }
        self.announce_players();
    }

    /// [`leave`](Hub::leave) with the length of the hold passed in, so a test can use a short one.
    fn leave_held(self: &Arc<Hub>, id: u32, name: &str, reason: &str, grace: Duration) {
        let gone = self.online.lock().unwrap().remove(&id);
        self.to_game(GameMsg::Leave { id });
        if let Some(s) = gone.and_then(|o| o.state) {
            let mut w = self.world.lock().unwrap();
            w.remember(name, &s);
            w.save();
        }
        self.to_viewers(id, proto::frame(proto::S_LEAVE, &id.to_le_bytes()));
        println!("{name} left: {reason} ({} online)", self.count());
        self.log_latency(name, "server", json!({"event": "leave", "reason": reason}));
        if self.online.lock().unwrap().iter().any(|(&other, o)| other > id && o.name.eq_ignore_ascii_case(name)) {
            return;
        }
        let hub = self.clone();
        let who = name.to_string();
        hub.held.lock().unwrap().insert(who.to_ascii_lowercase());
        tokio::spawn(async move {
            tokio::time::sleep(grace).await;
            // A rejoin took the name out of `held` (and kept quiet); otherwise the leave stands.
            if !hub.held.lock().unwrap().remove(&who.to_ascii_lowercase()) {
                return;
            }
            if hub.online.lock().unwrap().values().any(|o| o.name.eq_ignore_ascii_case(&who)) {
                return;
            }
            hub.chat("", &format!("{who} left the game"));
            hub.announce_players();
        });
    }
}

#[cfg(test)]
#[path = "hub_tests.rs"]
mod tests;
