//! A streamed session (`--stream`): the viewer runs on the host for one browser player, started
//! by the server (crates/server). It has no OS window; the cameras draw into an image that is read
//! back every frame and encoded (encode.rs), the browser's keys and mouse arrive as Bevy input
//! messages, and the player's state and block edits go to the server, which relays the other
//! players and their edits back (players.rs, `Streamer::remote`).
//!
//! The server connection carries `proto` frames; a reader and a writer thread keep the frame
//! loop from ever waiting on the socket.
//!
//! The server also keeps the world's clock (`S_TIME`, followed by `Clock::sync`) and moves the
//! player or sets its game mode for commands (`S_TELEPORT`, `S_GAMEMODE`, teleport.rs).
//!
//! A native session (`--join`, join.rs) runs `receive` and `send` too, over a WebSocket to the
//! server, in its own window and with no video; the server sends it no browser input.

use std::collections::{HashSet, VecDeque};
use std::io::{self, BufReader, BufWriter, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::input::ButtonState;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use crossbeam_channel::{unbounded, Receiver, Sender};
use proto::{Input, PlayerState};

use crate::encode::{Encoder, VideoConfig};
use crate::hand::Hand;
use crate::hotbar::Inventory;
use crate::input::Focus;
use crate::player::Player;
use crate::players::{OtherEvent, Others};
use crate::settings::Menu;
use crate::sky::Clock;
use crate::stream::Streamer;

/// `--stream ADDR --session TOKEN [--size WxH] [--fps N] [--encoder E] [--bitrate KBPS]`
#[derive(Clone, Debug)]
pub struct StreamArgs {
    pub addr: String,
    pub session: String,
    pub video: VideoConfig,
}

/// What the server knows about this player when the session starts.
#[derive(Clone, Debug, Default)]
pub struct Welcome {
    pub id: u32,
    pub name: String,
    /// The saved feet position, yaw and pitch; `None` for a new player.
    pub at: Option<(DVec3, f32, f32)>,
    pub flying: bool,
    /// The shared world's time of day.
    pub world_time: i64,
}

#[derive(Resource)]
pub struct Link {
    out: Sender<Vec<u8>>,
    inbox: Receiver<(u8, Vec<u8>)>,
    /// Frames taken off `inbox` while [`pace`] watched for browser input, for `receive`.
    held: VecDeque<(u8, Vec<u8>)>,
    closed: Arc<AtomicBool>,
    pub welcome: Welcome,
    last_state: Option<PlayerState>,
    /// Ticks since the state was last sent; it goes at least once a second while unchanged.
    idle_ticks: u32,
    focus_sent: Option<u8>,
    /// The newest input sequence the browser sent (`Input::Seq`).
    pub seq: u32,
    /// Keys and buttons the browser holds, released on [`Input::ReleaseAll`].
    keys_down: HashSet<KeyCode>,
    buttons_down: HashSet<MouseButton>,
}

impl Link {
    /// Whether the browser holds a shift key, for the text a key types.
    pub fn shift_held(&self) -> bool {
        self.keys_down.contains(&KeyCode::ShiftLeft) || self.keys_down.contains(&KeyCode::ShiftRight)
    }

    /// Connects, says hello and waits for the server's welcome.
    pub fn connect(args: &StreamArgs) -> io::Result<Link> {
        let stream = TcpStream::connect(&args.addr)?;
        stream.set_nodelay(true)?;
        let mut writer = BufWriter::new(stream.try_clone()?);
        proto::write_frame(&mut writer, proto::V_HELLO, args.session.as_bytes())?;
        writer.flush()?;
        let mut reader = BufReader::new(stream.try_clone()?);
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        let (tag, body) = proto::read_frame(&mut reader)?;
        if tag != proto::S_WELCOME {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("expected a welcome, got tag {tag}")));
        }
        stream.set_read_timeout(None)?;
        let welcome = parse_welcome(&body)?;
        eprintln!("mc-rs viewer: joined as {} (player {})", welcome.name, welcome.id);
        let closed = Arc::new(AtomicBool::new(false));
        let (out, out_rx) = unbounded::<Vec<u8>>();
        let c = closed.clone();
        thread::Builder::new().name("link-out".into()).spawn(move || {
            while let Ok(f) = out_rx.recv() {
                if writer.write_all(&f).is_err() {
                    break;
                }
                // Write what is queued, then flush once.
                if out_rx.is_empty() && writer.flush().is_err() {
                    break;
                }
            }
            c.store(true, Ordering::Relaxed);
        })?;
        let (in_tx, inbox) = unbounded();
        let c = closed.clone();
        thread::Builder::new().name("link-in".into()).spawn(move || {
            while let Ok(f) = proto::read_frame(&mut reader) {
                if in_tx.send(f).is_err() {
                    break;
                }
            }
            c.store(true, Ordering::Relaxed);
        })?;
        let link = Link::over(out, inbox, closed, welcome);
        crate::survival::net::send_client_info(&link);
        Ok(link)
    }

    /// A link over any transport: frames to send go to `out`, frames received come from `inbox`,
    /// and `closed` is set when the connection ends. A native session's (join.rs) is a WebSocket.
    pub fn over(out: Sender<Vec<u8>>, inbox: Receiver<(u8, Vec<u8>)>, closed: Arc<AtomicBool>, welcome: Welcome) -> Link {
        Link {
            out,
            inbox,
            held: VecDeque::new(),
            closed,
            welcome,
            last_state: None,
            idle_ticks: 0,
            focus_sent: None,
            seq: 0,
            keys_down: HashSet::new(),
            buttons_down: HashSet::new(),
        }
    }

    pub fn send(&self, tag: u8, body: &[u8]) {
        let _ = self.out.send(proto::frame(tag, body));
    }

    /// The next frame from the server: the held ones first, in the order they came.
    fn next_frame(&mut self) -> Option<(u8, Vec<u8>)> {
        self.held.pop_front().or_else(|| self.inbox.try_recv().ok())
    }

    /// Holds what has arrived; true once browser input is among the held frames.
    fn input_waiting(&mut self) -> bool {
        while let Ok(f) = self.inbox.try_recv() {
            self.held.push_back(f);
        }
        self.held.iter().any(|f| f.0 == proto::S_INPUT)
    }

    pub fn sender(&self) -> Sender<Vec<u8>> {
        self.out.clone()
    }
}

pub(crate) fn parse_welcome(body: &[u8]) -> io::Result<Welcome> {
    let v: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let f = |k: &str| v.get(k).and_then(|x| x.as_f64());
    let at = v.get("pos").and_then(|p| p.as_array()).and_then(|p| {
        let c: Vec<f64> = p.iter().filter_map(|x| x.as_f64()).collect();
        (c.len() == 3).then(|| (DVec3::new(c[0], c[1], c[2]), f("yaw").unwrap_or(0.0) as f32, f("pitch").unwrap_or(0.0) as f32))
    });
    Ok(Welcome {
        id: f("id").unwrap_or(0.0) as u32,
        name: v.get("name").and_then(|n| n.as_str()).unwrap_or("player").to_string(),
        at,
        flying: v.get("flying").and_then(|x| x.as_bool()).unwrap_or(false),
        world_time: f("time").unwrap_or(0.0) as i64,
    })
}

/// `KeyboardEvent.code` to the key Bevy names the same way.
pub fn key_code(code: &str) -> Option<KeyCode> {
    use KeyCode::*;
    if let Some(c) = code.strip_prefix("Key").filter(|c| c.len() == 1) {
        const LETTERS: [KeyCode; 26] = [
            KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU,
            KeyV, KeyW, KeyX, KeyY, KeyZ,
        ];
        let b = c.as_bytes()[0];
        return b.is_ascii_uppercase().then(|| LETTERS[(b - b'A') as usize]);
    }
    if let Some(d) = code.strip_prefix("Digit").and_then(|d| d.parse::<usize>().ok()) {
        const DIGITS: [KeyCode; 10] = [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];
        return DIGITS.get(d).copied();
    }
    if let Some(n) = code.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
        const FKEYS: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
        return n.checked_sub(1).and_then(|i| FKEYS.get(i)).copied();
    }
    Some(match code {
        "Space" => Space,
        "ShiftLeft" => ShiftLeft,
        "ShiftRight" => ShiftRight,
        "ControlLeft" => ControlLeft,
        "ControlRight" => ControlRight,
        "AltLeft" => AltLeft,
        "AltRight" => AltRight,
        "Escape" => Escape,
        "Enter" => Enter,
        "Tab" => Tab,
        "Backspace" => Backspace,
        "ArrowUp" => ArrowUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        "Minus" => Minus,
        "Equal" => Equal,
        "NumpadAdd" => NumpadAdd,
        "NumpadSubtract" => NumpadSubtract,
        "Backquote" => Backquote,
        "Slash" => Slash,
        "Period" => Period,
        "Comma" => Comma,
        "Semicolon" => Semicolon,
        "Quote" => Quote,
        "Delete" => Delete,
        "BracketLeft" => BracketLeft,
        "BracketRight" => BracketRight,
        "Backslash" => Backslash,
        _ => return None,
    })
}

fn mouse_button(b: u8) -> Option<MouseButton> {
    match b {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Middle),
        2 => Some(MouseButton::Right),
        _ => None,
    }
}

/// The free cursor the browser reports, in the picture's pixels, for the menu buttons.
#[derive(Resource, Default)]
pub struct StreamCursor(pub Option<Vec2>);

/// Server messages: input becomes Bevy input messages for this frame, the rest goes to the
/// world and the other players.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut link: ResMut<Link>,
    mut streamer: ResMut<Streamer>,
    mut others: ResMut<Others>,
    mut cursor: ResMut<StreamCursor>,
    mut clock: ResMut<Clock>,
    mut player: ResMut<Player>,
    mut media: ResMut<crate::media::Inbox>,
    mut new_picture: ResMut<NewPicture>,
    mut survival: ResMut<crate::survival::Inbox>,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut motion: MessageWriter<MouseMotion>,
    mut wheel: MessageWriter<MouseWheel>,
    mut exit: MessageWriter<AppExit>,
) {
    let (window, win) = window.into_inner();
    let size = Vec2::new(win.width(), win.height());
    let key = |key_code: KeyCode, state: ButtonState| KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window,
    };
    while let Some((tag, body)) = link.next_frame() {
        match tag {
            proto::S_INPUT => {
                for e in proto::decode_inputs(&body) {
                    match e {
                        Input::KeyDown(ref c) | Input::KeyUp(ref c) => {
                            let down = matches!(e, Input::KeyDown(_));
                            let Some(k) = key_code(c) else { continue };
                            if down {
                                link.keys_down.insert(k);
                            } else {
                                link.keys_down.remove(&k);
                            }
                            keys.write(key(k, if down { ButtonState::Pressed } else { ButtonState::Released }));
                        }
                        Input::MouseMove(x, y) => {
                            motion.write(MouseMotion { delta: Vec2::new(x, y) });
                        }
                        Input::MouseDown(b) | Input::MouseUp(b) => {
                            let down = matches!(e, Input::MouseDown(_));
                            let Some(button) = mouse_button(b) else { continue };
                            if down {
                                link.buttons_down.insert(button);
                            } else {
                                link.buttons_down.remove(&button);
                            }
                            let state = if down { ButtonState::Pressed } else { ButtonState::Released };
                            buttons.write(MouseButtonInput { button, state, window });
                        }
                        Input::Wheel(lines) => {
                            wheel.write(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y: lines, window, phase: TouchPhase::Moved });
                        }
                        Input::Cursor(x, y) => {
                            cursor.0 = Some(Vec2::new(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)) * size);
                        }
                        Input::Seq(n) => link.seq = n,
                        Input::ReleaseAll => {
                            for k in std::mem::take(&mut link.keys_down) {
                                keys.write(key(k, ButtonState::Released));
                            }
                            for button in std::mem::take(&mut link.buttons_down) {
                                buttons.write(MouseButtonInput { button, state: ButtonState::Released, window });
                            }
                        }
                    }
                }
            }
            proto::S_JOIN if body.len() >= 4 => {
                let id = u32::from_le_bytes(body[..4].try_into().unwrap());
                others.events.push(OtherEvent::Join(id, String::from_utf8_lossy(&body[4..]).into_owned()));
            }
            proto::S_LEAVE if body.len() >= 4 => {
                others.events.push(OtherEvent::Leave(u32::from_le_bytes(body[..4].try_into().unwrap())));
            }
            proto::S_STATE if body.len() >= 4 => {
                let id = u32::from_le_bytes(body[..4].try_into().unwrap());
                if let Some(s) = PlayerState::decode(&body[4..]) {
                    others.events.push(OtherEvent::State(id, s));
                }
            }
            proto::S_BLOCKS => streamer.remote(proto::decode_blocks(&body)),
            // The shared clock and what commands do to this player (teleport.rs).
            proto::S_TIME => {
                if let Some(t) = proto::TimeSync::decode(&body) {
                    clock.sync(t);
                }
            }
            proto::S_TELEPORT => {
                if let Some(tp) = proto::Teleport::decode(&body) {
                    crate::teleport::apply(&mut player, &tp);
                }
            }
            proto::S_GAMEMODE if !body.is_empty() => crate::teleport::set_game_mode(&mut player, body[0]),
            // Shared paintings (media.rs).
            proto::S_MEDIA => {
                if let Some((id, bytes)) = proto::decode_media(&body) {
                    media.media.push((id, bytes.to_vec()));
                }
            }
            proto::S_PAINTING => media.paintings.extend(proto::Painting::decode(&body)),
            proto::S_PICTURE => new_picture.0 = proto::decode_picture(&body).or(new_picture.0),
            proto::S_HANG => media.hang.push(String::from_utf8_lossy(&body).into_owned()),
            // Survival (survival/net.rs decodes them).
            160..=219 => survival.0.push((tag, body)),
            _ => {}
        }
    }
    if link.closed.load(Ordering::Relaxed) {
        info!("server connection closed; ending the session");
        exit.write(AppExit::Success);
    }
}

/// The player's state each tick it changed (and once a second regardless), the blocks its edits
/// changed, and whether the page should hold the pointer.
#[allow(clippy::too_many_arguments)]
pub fn send(
    clock: Res<Clock>,
    player: Res<Player>,
    hand: Res<Hand>,
    inv: Res<Inventory>,
    focus: Res<Focus>,
    menu: Res<Menu>,
    mut streamer: ResMut<Streamer>,
    mut link: ResMut<Link>,
) {
    let f = if menu.open {
        proto::FOCUS_MENU
    } else if focus.in_game {
        proto::FOCUS_GAME
    } else {
        proto::FOCUS_NONE
    };
    if link.focus_sent != Some(f) {
        link.focus_sent = Some(f);
        link.send(proto::V_FOCUS, &[f]);
    }
    if !streamer.outgoing.is_empty() {
        let changes = std::mem::take(&mut streamer.outgoing);
        link.send(proto::V_BLOCKS, &proto::encode_blocks(&changes));
    }
    if clock.ticks_this_frame == 0 || !player.spawned {
        return;
    }
    let mut flags = 0;
    if player.on_ground {
        flags |= proto::ON_GROUND;
    }
    if player.is_sneaking() {
        flags |= proto::SNEAKING;
    }
    if player.sprinting {
        flags |= proto::SPRINTING;
    }
    if player.flying {
        flags |= proto::FLYING;
    }
    let s = PlayerState {
        pos: player.pos.to_array(),
        yaw: player.yaw,
        pitch: player.pitch,
        flags,
        swings: hand.swings,
        held: crate::hotbar::held_state(&inv.get_current_item()),
    };
    link.idle_ticks += clock.ticks_this_frame;
    if link.last_state != Some(s) || link.idle_ticks >= 20 {
        link.idle_ticks = 0;
        link.last_state = Some(s);
        link.send(proto::V_STATE, &s.encode());
    }
}

/// Sets the menu buttons' hover and press from the browser's cursor; the UI's own focus system
/// only follows a window's cursor.
pub fn ui_pointer(
    cursor: Res<StreamCursor>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut nodes: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform, &mut Interaction)>,
) {
    for (node, transform, mut interaction) in &mut nodes {
        let inside = cursor.0.is_some_and(|c| node.contains_point(*transform, c));
        let want = match (inside, mouse.pressed(MouseButton::Left)) {
            (false, _) => Interaction::None,
            (true, true) => Interaction::Pressed,
            (true, false) => Interaction::Hovered,
        };
        if *interaction != want {
            *interaction = want;
        }
    }
}

/// The image every camera draws into, grabbed each frame for the encoder (grab.rs).
#[derive(Resource)]
pub struct StreamTarget(pub Handle<Image>);

#[derive(Resource)]
pub struct Video {
    encoder: std::sync::Mutex<Encoder>,
    /// Where every encoder's access units go, kept to start the next one on a picture change.
    units: Sender<crate::encode::Unit>,
    /// The picture now streamed: the command line's, then the tuner's (`S_PICTURE`).
    pub config: VideoConfig,
}

/// A picture the tuner asked for, waiting for [`change_picture`].
#[derive(Resource, Default)]
pub struct NewPicture(pub Option<[u32; 4]>);

/// The time between frame starts, as `ScheduleRunnerPlugin` keeps it, but changeable.
#[derive(Resource)]
pub struct Pace {
    pub frame: Duration,
    /// When the next frame is due on the steady grid of frame starts.
    due: Instant,
}

impl Pace {
    pub fn new(fps: u32) -> Pace {
        Pace { frame: Duration::from_secs_f64(1.0 / fps.max(1) as f64), due: Instant::now() }
    }

    /// How early a frame may start for browser input: half a frame.
    fn early(&self) -> Duration {
        self.frame / 2
    }

    /// Where the frame after one starting at `now` is due: a frame on the grid, or a whole frame
    /// on from now after falling behind by more than a frame (no burst of frames to catch up).
    fn advance(&mut self, now: Instant) {
        if now > self.due + self.frame {
            self.due = now;
        }
        self.due += self.frame;
    }
}

/// How often the wait in [`pace`] looks for browser input. A timed channel wait on Windows parks
/// until the next 15.6 ms timer tick; std's sleep uses a high-resolution timer.
const INPUT_POLL: Duration = Duration::from_micros(250);

/// Waits out the rest of the frame before the next one reads input, as the runner's own wait
/// does after a frame, but starts the frame up to half a frame early when browser input comes
/// in: input otherwise waits half a frame on average for the next update to read it. Frames
/// stay on the grid of the stream's frame rate, so starting early never adds frames.
pub fn pace(mut pace: ResMut<Pace>, mut link: ResMut<Link>) {
    let open = pace.due.checked_sub(pace.early()).unwrap_or(pace.due);
    let now = Instant::now();
    if now < open {
        thread::sleep(open - now);
    }
    while Instant::now() < pace.due && !link.input_waiting() && !link.closed.load(Ordering::Relaxed) {
        thread::sleep(INPUT_POLL.min(pace.due.saturating_duration_since(Instant::now())));
    }
    pace.advance(Instant::now());
}

/// Streams the tuner's picture from the next frame on: the target image and the window take the
/// new size, a new encoder starts (its first unit a key frame the browser starts from), and the
/// frame rate changes.
#[allow(clippy::too_many_arguments)]
pub fn change_picture(
    mut wanted: ResMut<NewPicture>,
    video: Option<ResMut<Video>>,
    target: Option<Res<StreamTarget>>,
    grab: Option<ResMut<crate::grab::GrabTarget>>,
    stats: Option<ResMut<StreamStats>>,
    mut pace: ResMut<Pace>,
    mut images: ResMut<Assets<Image>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    link: Res<Link>,
) {
    let Some([w, h, fps, br]) = wanted.0.take() else { return };
    let (Some(mut video), Some(target), Some(mut grab), Some(mut stats)) = (video, target, grab, stats) else { return };
    let cfg = VideoConfig { width: w.max(2) / 2 * 2, height: h.max(2) / 2 * 2, fps: fps.clamp(1, 240), bitrate_kbps: br, encoder: video.config.encoder.clone() };
    let encoder = match Encoder::start(&cfg, video.units.clone()) {
        Ok(e) => e,
        Err(e) => {
            link.send(proto::V_LOG, format!("could not restart ffmpeg at {}x{}: {e}", cfg.width, cfg.height).as_bytes());
            return;
        }
    };
    if (cfg.width, cfg.height) != (video.config.width, video.config.height) {
        if let Some(mut image) = images.get_mut(&target.0) {
            image.resize(bevy::render::render_resource::Extent3d { width: cfg.width, height: cfg.height, depth_or_array_layers: 1 });
        }
        window.resolution.set(cfg.width as f32, cfg.height as f32);
    }
    stats.dropped = encoder.dropped.clone();
    grab.width = cfg.width;
    grab.height = cfg.height;
    grab.sink = encoder.sink();
    // The old encoder is dropped here, which ends its ffmpeg.
    *video.encoder.lock().unwrap() = encoder;
    *pace = Pace::new(cfg.fps);
    info!("picture now {}x{} at {} fps, {} kbit/s", cfg.width, cfg.height, cfg.fps, cfg.bitrate_kbps);
    video.config = cfg;
}

/// Encoded units sent, summed for the stats line.
#[derive(Default)]
pub struct UnitStats {
    pub units: u32,
    pub keys: u32,
    pub bytes: u64,
    pub pipeline_total_us: u64,
    pub pipeline_worst_us: u32,
    pub first_byte_total_us: u64,
    pub encode_total_us: u64,
    pub encode_worst_us: u32,
}

/// What the stats line reads from the other threads.
#[derive(Resource)]
pub struct StreamStats {
    units: Arc<std::sync::Mutex<UnitStats>>,
    grab: Arc<std::sync::Mutex<crate::grab::GrabStats>>,
    dropped: Arc<std::sync::atomic::AtomicU64>,
}

/// Stamps the frame with the newest input before anything else runs on it.
pub fn stamp(link: Res<Link>, mut stamp: ResMut<crate::grab::FrameStamp>) {
    *stamp = crate::grab::FrameStamp { seq: link.seq, at: std::time::Instant::now() };
}

/// Creates the target image and starts the encoder, whose access units go to the server.
pub fn setup_video(mut commands: Commands, link: Res<Link>, args: Res<crate::Args>, mut images: ResMut<Assets<Image>>) {
    let Some(stream) = &args.stream else { return };
    let v = &stream.video;
    let mut image = Image::new_target_texture(v.width, v.height, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = images.add(image);
    commands.insert_resource(StreamTarget(handle.clone()));
    commands.insert_resource(crate::grab::FrameStamp { seq: 0, at: std::time::Instant::now() });

    let units = Arc::new(std::sync::Mutex::new(UnitStats::default()));
    let (au_tx, au_rx) = unbounded::<crate::encode::Unit>();
    let out = link.sender();
    let u = units.clone();
    thread::Builder::new()
        .name("video-out".into())
        .spawn(move || {
            for unit in au_rx {
                let header = proto::VideoHeader { flags: if unit.key { proto::VIDEO_KEY } else { 0 }, seq: unit.seq, pipeline_us: unit.pipeline_us };
                let mut body = Vec::with_capacity(unit.bytes.len() + proto::VideoHeader::SIZE);
                body.extend_from_slice(&header.encode());
                body.extend_from_slice(&unit.bytes);
                if let Ok(mut s) = u.lock() {
                    s.units += 1;
                    s.keys += unit.key as u32;
                    s.bytes += unit.bytes.len() as u64;
                    s.pipeline_total_us += unit.pipeline_us as u64;
                    s.pipeline_worst_us = s.pipeline_worst_us.max(unit.pipeline_us);
                    s.first_byte_total_us += unit.first_byte_us as u64;
                    s.encode_total_us += unit.encode_us as u64;
                    s.encode_worst_us = s.encode_worst_us.max(unit.encode_us);
                }
                if out.send(proto::frame(proto::V_VIDEO, &body)).is_err() {
                    return;
                }
            }
        })
        .expect("spawn video-out");
    let au_tx_keep = au_tx.clone();
    match Encoder::start(v, au_tx) {
        Ok(encoder) => {
            let grab = Arc::new(std::sync::Mutex::new(crate::grab::GrabStats::default()));
            commands.insert_resource(StreamStats { units, grab: grab.clone(), dropped: encoder.dropped.clone() });
            commands.insert_resource(crate::grab::GrabTarget { image: handle, width: v.width, height: v.height, sink: encoder.sink(), stats: grab });
            commands.insert_resource(Video { encoder: std::sync::Mutex::new(encoder), units: au_tx_keep, config: v.clone() });
        }
        Err(e) => {
            let msg = format!("could not start ffmpeg ({}): {e}", v.encoder);
            error!("{msg}");
            link.send(proto::V_LOG, msg.as_bytes());
        }
    }
}

/// Every five seconds, one `stats` line for the server's latency.log: frames, update time,
/// grab time, frames the encoder was too busy for, and render to encoded unit times.
pub fn report_stats(
    time: Res<Time>,
    link: Res<Link>,
    video: Option<Res<Video>>,
    stats: Option<Res<StreamStats>>,
    streamer: Res<Streamer>,
    mut acc: Local<(f32, u32, f32, f32, u64)>,
) {
    let Some(stats) = stats else { return };
    let dt = time.delta_secs();
    acc.0 += dt;
    acc.1 += 1;
    acc.2 += dt;
    acc.3 = acc.3.max(dt);
    if acc.0 < 5.0 {
        return;
    }
    let (secs, frames, sum_dt, worst_dt, last_dropped) = *acc;
    let dropped = stats.dropped.load(std::sync::atomic::Ordering::Relaxed);
    let u = stats.units.lock().map(|mut u| std::mem::take(&mut *u)).unwrap_or_default();
    let g = stats.grab.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    let v = video.map(|v| v.config.clone());
    let line = serde_json::json!({
        "secs": secs,
        "fps": frames as f32 / secs,
        "frame_ms_avg": sum_dt * 1000.0 / frames.max(1) as f32,
        "frame_ms_worst": worst_dt * 1000.0,
        "grab_ms_avg": g.total.as_secs_f32() * 1000.0 / g.frames.max(1) as f32,
        "grab_ms_worst": g.worst.as_secs_f32() * 1000.0,
        "encoder_busy_drops": dropped - last_dropped,
        "units": u.units,
        "keys": u.keys,
        "kbit_s": u.bytes as f32 * 8.0 / 1000.0 / secs,
        "pipeline_ms_avg": u.pipeline_total_us as f32 / 1000.0 / u.units.max(1) as f32,
        "pipeline_ms_worst": u.pipeline_worst_us as f32 / 1000.0,
        "ffmpeg_first_byte_ms_avg": u.first_byte_total_us as f32 / 1000.0 / u.units.max(1) as f32,
        "ffmpeg_out_ms_avg": u.encode_total_us as f32 / 1000.0 / u.units.max(1) as f32,
        "ffmpeg_out_ms_worst": u.encode_worst_us as f32 / 1000.0,
        "update_to_grab_ms_avg": g.update_to_grab_total.as_secs_f32() * 1000.0 / g.frames.max(1) as f32,
        "chunks_pending": streamer.pending(),
        "picture": v.map(|v| format!("{}x{}@{} {}k {}", v.width, v.height, v.fps, v.bitrate_kbps, v.encoder)),
    });
    link.send(proto::V_LOG, format!("stats {line}").as_bytes());
    *acc = (0.0, 0, 0.0, 0.0, dropped);
}

/// Points every camera at the stream image; runs after the cameras are spawned.
pub fn retarget_cameras(mut commands: Commands, target: Option<Res<StreamTarget>>, cameras: Query<Entity, With<Camera>>) {
    let Some(target) = target else { return };
    for e in &cameras {
        commands.entity(e).insert(RenderTarget::Image(ImageRenderTarget { handle: target.0.clone(), scale_factor: 1.0 }));
    }
}

/// Ends the session when the encoder died.
pub fn watch_encoder(video: Option<Res<Video>>, link: Res<Link>, mut exit: MessageWriter<AppExit>, mut checked: Local<u32>) {
    *checked += 1;
    if *checked % 60 != 0 {
        return;
    }
    let Some(video) = video else { return };
    let Ok(mut e) = video.encoder.lock() else { return };
    if !e.alive() {
        link.send(proto::V_LOG, b"ffmpeg exited");
        exit.write(AppExit::error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_codes_map_to_keys() {
        assert_eq!(key_code("KeyW"), Some(KeyCode::KeyW));
        assert_eq!(key_code("Digit7"), Some(KeyCode::Digit7));
        assert_eq!(key_code("F1"), Some(KeyCode::F1));
        assert_eq!(key_code("F13"), None);
        assert_eq!(key_code("ShiftLeft"), Some(KeyCode::ShiftLeft));
        assert_eq!(key_code("Keyw"), None);
        assert_eq!(key_code("MediaPlay"), None);
    }

    #[test]
    fn welcome_reads_a_saved_player() {
        let w = parse_welcome(br#"{"id":3,"name":"xy","pos":[1.5,70,-2.5],"yaw":90,"pitch":10,"flying":true,"time":6000}"#).unwrap();
        assert_eq!(w.id, 3);
        assert_eq!(w.name, "xy");
        assert_eq!(w.at, Some((DVec3::new(1.5, 70.0, -2.5), 90.0, 10.0)));
        assert!(w.flying);
        assert_eq!(w.world_time, 6000);
        let fresh = parse_welcome(br#"{"id":1,"name":"a","pos":null}"#).unwrap();
        assert_eq!(fresh.at, None);
    }
}
