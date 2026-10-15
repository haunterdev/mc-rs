//! Viewer for the mc-rs terrain port: walk or fly through the generated world as a creative
//! player.
//!
//! `viewer [--seed S] [--radius R] [--workers N] [--hide-gui] [--menu] [--screen creative[:TAB]|inventory|crafting|chest|chest_double|furnace] [--survival[:dead]] [--chests] [--mobs[:hurt|:dying|:love|:hostile]] [--smoke SECONDS [--shot FILE.png] [--at X,Y,Z,YAW,PITCH] [--set KEY:VALUE]...]`
//!
//! `--seed` takes a number, or any other text, which is hashed like Java's `String.hashCode`.
//! `--radius` overrides the render distance in the options file for this run.
//! `--hide-gui` starts with the GUI hidden, as F1 does. `--menu` opens the options menu on the
//! first frame, so a `--shot` shows it. `--screen creative` opens the creative inventory on the
//! first frame, on the tab with that label (`buildingBlocks`, the default, `search`, `inventory`, ...).
//! `--survival` plays the first frames in survival mode with sample health, food and xp (for a
//! `--shot` of the HUD); `--survival:dead` opens the death screen instead.
//! `--items` drops a few item stacks in front of the camera (for a `--shot` of the dropped item render).
//! `--mobs` stands an adult and a baby cow, pig, sheep and chicken, and a walking cow, in front of the `--at` camera (for a `--shot` of the mob render); `--mobs:hurt` hits the standing and the baby cow every 8 ticks (the red overlay), `--mobs:dying` kills and respawns the standing one every 24, `--mobs:love` sends status 18 (love hearts) to the adults every 12 ticks; `--mobs:hostile` stands a zombie (burning, swinging), skeleton with a bow, creeper (fuse cycling), powered creeper, spider and a baby zombie instead of the farm set.
//! `--crack N` shows break stage N on the block under the crosshair (for a `--shot` of the crack overlay).
//! `--smoke` runs headless with a hidden window and default options, flies the player along +x
//! while stepping through the AA, lighting and time options, prints the stats and exits; nonzero
//! if no chunk got meshed. With `--shot` it stands at `--at` instead (eye position, yaw and pitch
//! in degrees as the game's F3 screen shows them), saves one screenshot when the time is up and
//! exits. `--set` overrides one option for the run, in the options file's `key:value` form.
//!
//! Block models, textures and the sky, cloud, skin and GUI images come from the local 1.12.2
//! client jar at startup (jar.rs) and stay in memory. The launcher (crates/launcher) downloads
//! the jar on a PC that has none. Started by the launcher (`MCRS_LAUNCHED`), a startup error goes
//! to stderr, which it shows; started alone, a message box shows it too.
//!
//! `--stream ADDR --session TOKEN [--size WxH] [--fps N] [--encoder E] [--bitrate KBPS]` runs a
//! streamed session for one browser player, started by the server (crates/server, link.rs): no
//! OS window, the frames go to an H.264 encoder, the browser's input comes in over the server
//! connection, and the world is shared with the other sessions.
//!
//! `--join URL` (a `wss://` or `ws://` address ending `/native`) plays on such a server in a
//! window of its own, rendering here (join.rs), signed in with the token in `MCRS_JOIN_TOKEN`;
//! the launcher's Join starts it. The world seed comes from the server.
//!
//! The `share` feature (tools/dist.py) builds a window-only exe with no console.
//!
//! Five cameras draw each frame, in order: the sky camera (sky, and the clouds while the eye is
//! under 128), the main camera (terrain, loading the sky camera's depth so the clouds occlude
//! it, and the clouds from 128 up), the hand camera (the arm and the held block after a depth
//! clear, then the UI), the item camera (the hotbar icons over the UI, item_render.rs) and the chat
//! camera (the chat lines over the icons, chat.rs).

#![cfg_attr(feature = "share", windows_subsystem = "windows")]

mod audio;
mod blockstates;
mod celestial;
mod capture;
mod chat;
mod clouds;
mod collision;
mod colors;
mod dialog;
mod encode;
mod env;
mod fluid;
mod grab;
mod hand;
mod hotbar;
mod hud;
mod input;
mod interact;
mod item;
mod item_render;
mod jar;
mod join;
mod lightmap;
mod link;
mod lod;
mod lod_mesh;
mod material;
mod media;
mod media_load;
mod vertex;
mod mesher;
mod models;
mod multipart;
mod particles;
mod player;
mod players;
mod populator;
mod region;
mod settings;
mod sfx;
mod sign;
mod sky;
mod stream;
mod survival;
mod tablist;
mod target;
mod teleport;
mod textures;

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::Camera3dDepthLoadOp;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin, SystemInformationDiagnosticsPlugin};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, ShadowFilteringMethod};
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use bevy::window::PresentMode;
use bevy::winit::WinitSettings;
use worldgen::JavaRandom;

use clouds::{CloudKey, CloudMesh, CloudTexture};
use colors::Colors;
use env::Env;
use hand::{Arm, GuiIcons, Hand, HideGui};
use hotbar::{GuiWidgets, Inventory};
use input::Focus;
use item_render::ItemMaterials;
use jar::Jar;
use material::{ChunkMaterial, ChunkParams, HandMaterial, ItemMaterial, MaterialsPlugin, SkyKind, SkyMaterial};
use models::Models;
use player::Player;
use settings::{Aa, Clouds, Menu, Mode, Opt, Settings, Smooth, TimeOfDay};
use sky::{BiomeColors, Clock, SkyMesh, Stars, View};
use stream::{ChunkMaterials, Streamer};
use textures::{decode_png, flat_image, Argb};

/// The player's view; terrain, layer 0.
#[derive(Component)]
pub struct MainCamera;

/// Draws before the main camera: the sky and the low clouds, layer 1.
#[derive(Component)]
pub struct SkyCamera;

/// Draws after the main camera: the arm (layer 2), then the UI.
#[derive(Component)]
pub struct HandCamera;

#[derive(Resource, Clone)]
pub struct Args {
    seed: i64,
    radius: Option<i32>,
    workers: usize,
    smoke: Option<f32>,
    pub shot: Option<std::path::PathBuf>,
    at: Option<[f32; 5]>,
    set: Vec<String>,
    hide_gui: bool,
    /// `--demo-counts`: stack sizes and a worn tool on the hotbar, for a shot of its overlay.
    demo_counts: bool,
    /// `--demo-switcher`: opens the F3+F4 game mode screen on the first frame, for a shot.
    demo_switcher: bool,
    menu: bool,
    /// `--screen creative[:TAB]`, `inventory` or `crafting`
    screen: Option<ScreenArg>,
    /// `--survival[:dead]`: Some(dead)
    survival: Option<bool>,
    /// `--crack STAGE`: the block under the crosshair shows that break stage (0 to 9).
    crack: Option<u8>,
    /// `--items`: a few dropped stacks in front of the camera (for a `--shot`).
    items: bool,
    /// `--chests`: a chest and a large chest in front of the `--at` camera, the large one open.
    chests: bool,
    /// `--mobs`: an adult and a baby of each kind, and a walking cow, in front of the `--at` camera.
    mobs: bool,
    /// `--mobs:hurt` (1), `--mobs:dying` (2) and `--mobs:love` (3): the cows are hit every 8 ticks, or the standing one dies and respawns every 24, or the adults get status 18 every 12.
    mob_mode: u8,
    uncapped: bool,
    pub stream: Option<link::StreamArgs>,
    pub join: Option<join::JoinArgs>,
}

/// `java.lang.String.hashCode`, over UTF-16 code units.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// The hotbar a new player starts with; with `--demo-counts` the stacks carry sizes and the
/// last slot is a worn pickaxe.
fn demo_inventory(demo: bool) -> Inventory {
    let mut inv = Inventory::default();
    if demo {
        for (i, n) in [64, 1, 12, 3, 40, 2, 64, 9, 27].into_iter().enumerate() {
            inv.main[i].set_count(n);
        }
        inv.main[8] = items::ItemStack::new(items::ids::item::IRON_PICKAXE, 1, 100);
    }
    inv
}

fn parse_args() -> Args {
    let mut workers = std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_sub(2).max(1);
    let mut workers_set = false;
    let (mut addr, mut session, mut size, mut fps, mut encoder, mut bitrate) =
        (None, String::new(), (1280u32, 720u32), 60u32, "libx264".to_string(), 8000u32);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64);
    let mut a = Args {
        seed: JavaRandom::new(nanos).next_long(),
        radius: None,
        workers,
        smoke: None,
        shot: None,
        at: None,
        set: Vec::new(),
        hide_gui: false,
        demo_counts: false,
        demo_switcher: false,
        menu: false,
        screen: None,
        survival: None,
        crack: None,
        items: false,
        chests: false,
        mobs: false,
        mob_mode: 0,
        uncapped: false,
        stream: None,
        join: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--seed" => {
                let v = value();
                a.seed = v.trim().parse::<i64>().unwrap_or_else(|_| java_hash(&v) as i64);
            }
            "--radius" => a.radius = Some(value().parse().expect("--radius takes a number")),
            "--workers" => {
                a.workers = value().parse::<usize>().expect("--workers takes a number").max(1);
                workers_set = true;
            }
            "--stream" => addr = Some(value()),
            "--join" => a.join = Some(join::JoinArgs { url: value(), token: std::env::var(join::TOKEN_VAR).unwrap_or_default() }),
            "--session" => session = value(),
            "--size" => {
                let v = value();
                let (w, h) = v.split_once('x').expect("--size takes WxH");
                size = (w.parse().expect("--size takes WxH"), h.parse().expect("--size takes WxH"));
            }
            "--fps" => fps = value().parse::<u32>().expect("--fps takes a number").clamp(10, 120),
            "--encoder" => encoder = value(),
            "--bitrate" => bitrate = value().parse().expect("--bitrate takes kbit/s"),
            "--smoke" => a.smoke = Some(value().parse().expect("--smoke takes seconds")),
            "--shot" => a.shot = Some(value().into()),
            "--at" => {
                let v: Vec<f32> = value().split(',').map(|n| n.trim().parse().expect("--at takes numbers")).collect();
                a.at = Some(v.try_into().expect("--at takes X,Y,Z,YAW,PITCH"));
            }
            "--set" => a.set.push(value()),
            "--hide-gui" => a.hide_gui = true,
            "--demo-counts" => a.demo_counts = true,
            "--demo-switcher" => a.demo_switcher = true,
            "--menu" => a.menu = true,
            "--screen" => {
                let v = value();
                a.screen = Some(match v.as_str() {
                    "inventory" => ScreenArg::Window(survival::gui::window::Kind::Inventory),
                    "crafting" => ScreenArg::Window(survival::gui::window::Kind::Crafting),
                    "chest" => ScreenArg::Window(survival::gui::window::Kind::Chest(3)),
                    "chest_double" => ScreenArg::Window(survival::gui::window::Kind::Chest(6)),
                    "furnace" => ScreenArg::Window(survival::gui::window::Kind::Furnace),
                    _ => {
                        let tab = v.strip_prefix("creative").expect("--screen takes creative[:TAB], inventory, crafting, chest, chest_double or furnace");
                        let label = tab.strip_prefix(':').unwrap_or("buildingBlocks");
                        let found = items::creative::Tab::ALL.into_iter().find(|t| t.label() == label);
                        ScreenArg::Creative(found.unwrap_or_else(|| panic!("--screen: no creative tab {label}")))
                    }
                });
            }
            "--survival" => a.survival = Some(false),
            "--survival:dead" => a.survival = Some(true),
            "--items" => a.items = true,
            "--chests" => a.chests = true,
            "--mobs" => a.mobs = true,
            "--mobs:hurt" => {
                a.mobs = true;
                a.mob_mode = 1;
            }
            "--mobs:dying" => {
                a.mobs = true;
                a.mob_mode = 2;
            }
            "--mobs:love" => {
                a.mobs = true;
                a.mob_mode = 3;
            }
            "--mobs:hostile" => {
                a.mobs = true;
                a.mob_mode = 4;
            }
            "--crack" => a.crack = Some(value().parse().expect("--crack takes a stage 0 to 9")),
            "--uncapped" => a.uncapped = true,
            _ => panic!("unknown argument {flag}; see the top of main.rs"),
        }
    }
    if let Some(addr) = addr {
        // The readback copies whole rows; 64 pixels keeps them at the 256-byte alignment.
        let width = (size.0.max(320) / 64) * 64;
        let video = encode::VideoConfig { width, height: size.1.max(240), fps, encoder, bitrate_kbps: bitrate };
        a.stream = Some(link::StreamArgs { addr, session, video });
        if !workers_set {
            // Several sessions share the host.
            workers = 3;
        }
    }
    a.workers = if workers_set { a.workers } else { workers };
    a
}

/// `--screen`'s value.
#[derive(Clone, Copy)]
enum ScreenArg {
    Creative(items::creative::Tab),
    Window(survival::gui::window::Kind),
}

/// What the startup reads out of the client jar. The jar itself is closed once this is built.
#[derive(Resource)]
struct Loaded {
    sun: Argb,
    moon: Argb,
    skin: Argb,
    icons: Argb,
    widgets: Argb,
    font: Argb,
    sign: Argb,
    tabs: Argb,
    /// `tab_items.png`, `tab_item_search.png`, `tab_inventory.png`, `inventory.png`, `crafting_table.png`
    backgrounds: [Argb; 8],
    /// `textures/font/ascii_sga.png`
    galactic: Argb,
    /// `textures/misc/enchanted_item_glint.png`
    glint: Argb,
    /// `textures/entity/chest/normal.png` and `normal_double.png`
    chest: [Argb; 2],
    /// The textures of `survival::mobs::MOBS`, in that order.
    mobs: Vec<Argb>,
    /// The layer textures of `survival::mobs::MOBS`, per kind and layer.
    mob_layers: Vec<Vec<Argb>>,
    /// `textures/blocks/fire_layer_0.png` and `_1`, with the strip's frame count and frame time
    fire: ([Argb; 2], u32, u32),
    /// `textures/particle/particles.png`, for the crit particles
    particles: Argb,
    /// `textures/models/armor/<material>_layer_1.png` and `_2` per material (players.rs `ARMOR_MATERIALS`)
    armor: Vec<Argb>,
    /// `textures/entity/projectiles/arrow.png` and `textures/entity/explosion.png`
    arrow: Argb,
    explosion: Argb,
    /// `textures/entity/experience_orb.png`
    xp_orb: Argb,
    empty_slots: Argb,
    lang: Vec<u8>,
}

/// The block models and their sprite array, kept for rebuilding the array on a mipmap change.
#[derive(Resource)]
pub(crate) struct Atlas(pub(crate) Arc<Models>);

/// 16 x 16 sprites side by side in one image.
fn strip(sprites: &[Argb]) -> Argb {
    let (w, h) = (16 * sprites.len() as u32, 16);
    let mut pixels = vec![0; (w * h) as usize];
    for (i, s) in sprites.iter().enumerate() {
        for y in 0..h.min(s.height) {
            for x in 0..16.min(s.width) {
                pixels[(y * w + i as u32 * 16 + x) as usize] = s.pixels[(y * s.width + x) as usize];
            }
        }
    }
    Argb { width: w, height: h, pixels }
}

fn load_jar() -> io::Result<(Arc<Models>, Arc<Colors>, Argb, Loaded)> {
    let jar = Jar::find()?;
    let png = |rel: &str| -> io::Result<Argb> { decode_png(&jar.asset(rel)?) };
    let loaded = Loaded {
        sun: png("textures/environment/sun.png")?,
        moon: png("textures/environment/moon_phases.png")?,
        // DROPPED: the UUID-picked Alex skin; the view entity is always Steve.
        skin: png("textures/entity/steve.png")?,
        icons: png("textures/gui/icons.png")?,
        widgets: png("textures/gui/widgets.png")?,
        font: png("textures/font/ascii.png")?,
        sign: png("textures/entity/sign.png")?,
        tabs: png("textures/gui/container/creative_inventory/tabs.png")?,
        backgrounds: [
            png("textures/gui/container/creative_inventory/tab_items.png")?,
            png("textures/gui/container/creative_inventory/tab_item_search.png")?,
            png("textures/gui/container/creative_inventory/tab_inventory.png")?,
            png("textures/gui/container/inventory.png")?,
            png("textures/gui/container/crafting_table.png")?,
            png("textures/gui/container/generic_54.png")?,
            png("textures/gui/container/furnace.png")?,
            png("textures/gui/container/enchanting_table.png")?,
        ],
        galactic: png("textures/font/ascii_sga.png")?,
        glint: png("textures/misc/enchanted_item_glint.png")?,
        chest: [png("textures/entity/chest/normal.png")?, png("textures/entity/chest/normal_double.png")?],
        mobs: survival::mobs::MOBS.iter().map(|m| png(m.texture)).collect::<io::Result<Vec<_>>>()?,
        mob_layers: survival::mobs::MOBS.iter().map(|m| m.layers.iter().map(|l| png(l.texture)).collect::<io::Result<Vec<_>>>()).collect::<io::Result<Vec<_>>>()?,
        particles: png("textures/particle/particles.png")?,
        armor: players::ARMOR_MATERIALS.iter().flat_map(|&m| [false, true].map(|legs| png(&players::armor_texture_path(m, legs)))).collect::<io::Result<Vec<_>>>()?,
        arrow: png("textures/entity/projectiles/arrow.png")?,
        explosion: png("textures/entity/explosion.png")?,
        xp_orb: png("textures/entity/experience_orb.png")?,
        fire: {
            let sprite = textures::load_sprite(&jar, "blocks/fire_layer_0")?;
            ([png("textures/blocks/fire_layer_0.png")?, png("textures/blocks/fire_layer_1.png")?], sprite.frames.len() as u32, sprite.frametime)
        },
        empty_slots: {
            let sprites = survival::gui::draw::EMPTY_SLOT_NAMES.iter().map(|n| png(&format!("textures/items/{n}.png"))).collect::<io::Result<Vec<_>>>()?;
            strip(&sprites)
        },
        lang: jar.asset("lang/en_us.lang")?,
    };
    let clouds = png("textures/environment/clouds.png")?;
    Ok((Arc::new(Models::load(&jar)?), Arc::new(Colors::load(&jar)?), clouds, loaded))
}

/// The shared build's error handler: each failing system or command is logged the first time
/// with its error, and skipped after that, so a broken system cannot fill latest.log each frame.
#[cfg(feature = "share")]
fn log_error_once(error: bevy::ecs::error::BevyError, ctx: bevy::ecs::error::ErrorContext) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let line = format!("{ctx}: {error}");
    let mut seen = SEEN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if seen.insert(line.clone()) {
        bevy::log::error!("{line}");
    }
}

fn main() {
    let mut args = parse_args();
    let headless = args.smoke.is_some() || args.stream.is_some();
    let link = args.stream.as_ref().map(|st| {
        link::Link::connect(st).unwrap_or_else(|e| {
            eprintln!("mc-rs viewer: could not join the server at {}: {e}", st.addr);
            std::process::exit(1);
        })
    });
    let (models, colors, cloud_texture, loaded) = load_jar().unwrap_or_else(|e| {
        eprintln!("mc-rs viewer: {e}");
        if !headless && std::env::var_os("MCRS_LAUNCHED").is_none() {
            dialog::error(&e.to_string());
        }
        std::process::exit(1);
    });
    // A native session: the link runs over a WebSocket, and the server names the world.
    let mut joined = None;
    let link = match (link, args.join.clone()) {
        (None, Some(j)) => {
            let (l, seed, extra) = join::connect(&j).unwrap_or_else(|e| {
                eprintln!("mc-rs viewer: could not join {}: {e}", j.url);
                if std::env::var_os("MCRS_LAUNCHED").is_none() {
                    dialog::error(&format!("Could not join {}: {e}", j.url));
                }
                std::process::exit(1);
            });
            args.seed = seed;
            joined = Some(extra);
            Some(l)
        }
        (link, _) => link,
    };
    let mut settings = if headless { Settings::parse(&args.set.join("
")) } else { settings::load() };
    if args.stream.is_some() && args.radius.is_none() && !args.set.iter().any(|s| s.starts_with("renderDistance")) {
        // Every session renders on the host's one GPU.
        settings.render_distance = 8;
    }
    if args.stream.is_some() && !args.set.iter().any(|s| s.starts_with("lodDistance")) {
        // Every session generates its own distant terrain on the host's CPU.
        settings.lod_distance = 32;
    }
    if let Some(r) = args.radius {
        settings.render_distance = r;
    }
    let mut streamer = Streamer::new(
        args.seed,
        settings.render_distance,
        args.workers,
        settings.smooth != Smooth::Off,
        models.clone(),
        colors.clone(),
    );
    streamer.share = link.is_some();
    // Stand on the highest block of column (8, 8), or where the server last saw this player.
    let spawn = streamer.generator.generate(0, 0);
    let top = (0..256).rev().find(|&y| spawn.blocks.get(8, y, 8) != 0).unwrap_or(64);
    let mut player = match link.as_ref().and_then(|l| l.welcome.at) {
        Some((feet, yaw, pitch)) => Player::new(feet, yaw, pitch),
        None => Player::new(DVec3::new(8.5, top as f64 + 1.0, 8.5), -59.0, 10.0),
    };
    player.flying = link.as_ref().is_some_and(|l| l.welcome.flying);
    let mut window = Window {
        title: format!("mc-rs viewer, seed {}", args.seed),
        visible: !headless,
        present_mode: if args.uncapped { PresentMode::AutoNoVsync } else { PresentMode::AutoVsync },
        ..default()
    };
    let mut app = App::new();
    // A player's copy logs a failing system and goes on instead of closing the game; builds here
    // keep Bevy's panic so the tests and the smoke run catch it.
    #[cfg(feature = "share")]
    app.set_error_handler(log_error_once);
    if let Some(st) = &args.stream {
        // No OS window: nothing can grab the host's mouse, and the cameras draw into an image.
        window.resolution = bevy::window::WindowResolution::new(st.video.width, st.video.height);
        app.add_plugins((
            // No pipelined rendering: it renders each frame while the next updates, a frame of
            // delay a stream does not want.
            DefaultPlugins
                .build()
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
                .set(WindowPlugin { primary_window: Some(window), ..default() }),
            grab::GrabPlugin,
            // No wait of its own: link::pace keeps the frame rate, which the tuner can change.
            bevy::app::ScheduleRunnerPlugin::run_loop(Duration::ZERO),
        ));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(window), ..default() }));
    }
    let world_time = link.as_ref().map(|l| l.welcome.world_time);
    let streaming = args.stream.is_some();
    let stream_fps = args.stream.as_ref().map_or(60, |s| s.video.fps);
    // Sound: the session's mix goes to the server, a local or native game's to the speakers.
    let (audio, output) = match link.as_ref().filter(|_| streaming) {
        Some(l) => {
            let out = l.sender();
            (audio::Audio::open_streamed(move |pcm| out.send(proto::frame(proto::V_AUDIO, &pcm)).is_ok()), None)
        }
        None => audio::Audio::open(args.smoke.is_some()),
    };
    app.insert_resource(audio)
        .insert_resource(audio::Output(output))
        .insert_resource(audio::MusicTicker::default())
        .insert_resource(particles::Particles::default());
    if let Some(link) = link {
        app.insert_resource(link)
            .insert_resource(link::StreamCursor::default())
            // Every path with a Link runs link::receive, so NewPicture has to exist for all of
            // them: a native join only ever stores it (S_PICTURE comes from the streaming tuner).
            .init_resource::<link::NewPicture>();
    }
    app.add_plugins((
        FrameTimeDiagnosticsPlugin::default(),
        SystemInformationDiagnosticsPlugin,
        MaterialsPlugin,
    ))
    .insert_resource(DirectionalLightShadowMap { size: 2048 })
    // A new world starts at time 0.
    .insert_resource(Clock::new(settings.time.ticks().or(world_time).unwrap_or(0)))
    .insert_resource(players::Others::default())
    .init_resource::<media::Media>()
    .init_resource::<media::Inbox>()
    .insert_resource(View::default())
    .insert_resource(Stars(sky::stars()))
    .insert_resource(BiomeColors(colors))
    .insert_resource(CloudTexture(cloud_texture))
    .insert_resource(Atlas(models))
    .insert_resource(Hand::default())
    .insert_resource(demo_inventory(args.demo_counts))
    .insert_resource(if args.demo_switcher { survival::gui::switcher::Switcher::demo() } else { Default::default() })
    .insert_resource(interact::Controller::default())
    .insert_resource(HideGui(args.hide_gui))
    // A streamed session leaves from the page's own Leave button.
    .insert_resource(Menu {
        open: args.demo_switcher,
        persist: !headless,
        open_at_start: args.menu,
        quit: if streaming { None } else if joined.is_some() { Some("Disconnect") } else { Some("Quit Game") },
    })
    .insert_resource(survival::gui::creative::Creative { open_at_start: args.screen.and_then(|s| match s { ScreenArg::Creative(t) => Some(t), _ => None }), ..default() })
    .insert_resource(survival::gui::window::Windows { open_at_start: args.screen.and_then(|s| match s { ScreenArg::Window(k) => Some(k), _ => None }), ..default() })
    .insert_resource(player)
    .insert_resource(Focus::default())
    .insert_resource(loaded)
    .insert_resource(settings)
    .insert_resource(streamer)
    .insert_resource(args)
    .add_systems(
        Startup,
        (setup, (hud::spawn_hud, hand::spawn_gui, hotbar::spawn_hotbar, target::spawn_outline, item_render::spawn_items, lod::setup, particles::spawn_mesh, survival::crit::spawn_mesh)).chain(),
    )
    .add_systems(
        Update,
        (
            (
                sfx::button_clicks,
                settings::toggle_menu,
                input::focus,
                settings::menu_input,
                radius_keys,
                settings::settings_changed,
                apply_settings,
                hand::toggle_gui,
                input::wheel,
                hotbar::keys,
                input::perspective_key,
            )
                .chain(),
            (
                (players::apply_events, players::update_armor, sky::advance_clock, player::run_ticks, players::tick_others, players::own_model, players::inventory_model, sfx::play, particles::tick_particles).chain(),
                input::mouse_turn,
                sky::update_view,
                sky::write_env,
                sky::update_sky,
                clouds::update_clouds,
                hand::update_hand,
                hand::update_gui,
                hotbar::update_hotbar,
                (item_render::update_items, item_render::scroll_glint),
                target::update_outline,
                sync_cameras,
                (players::pose_others, players::pose_held, players::finish_inventory_model).chain(),
                particles::draw_particles,
            )
                .chain(),
            ((stream::stream_chunks, lod::update_lod).chain(), hud::update_hud, hud::toggle_hud, sfx::update_listener, (media::drop_files, media::receive_shared, media::finish_loads, media::edit_keys, media::update_pictures).chain()),
        )
            .chain(),
    );
    app.add_plugins((survival::SurvivalPlugin, sign::SignPlugin));
    if app.world().resource::<Args>().survival.is_some() {
        app.add_systems(Startup, preview_survival);
    }
    if streaming {
        app.add_systems(Startup, link::setup_video.before(setup))
            .add_systems(PostStartup, link::retarget_cameras)
            .insert_resource(link::Pace::new(stream_fps))
            .add_systems(First, (link::pace, link::receive, link::change_picture, link::stamp).chain())
            .add_systems(PreUpdate, link::ui_pointer.after(bevy::ui::UiSystems::Focus))
            .add_systems(Update, (link::send.after(player::run_ticks), link::watch_encoder, link::report_stats));
    } else if let Some(joined) = joined {
        app.insert_resource(joined)
            .insert_resource(chat::Chat::default())
            .init_resource::<tablist::TabList>()
            .add_systems(Startup, (chat::spawn_chat, tablist::spawn_tab))
            .add_systems(First, (join::watch, link::receive).chain())
            .add_systems(
                Update,
                (
                    link::send.after(player::run_ticks),
                    chat::receive,
                    chat::keys.after(settings::toggle_menu).before(input::focus),
                    chat::draw,
                    tablist::draw.after(chat::receive).after(chat::keys),
                ),
            );
    } else if headless {
        // A hidden window gets no redraw requests, so keep the loop running on its own.
        app.insert_resource(WinitSettings::continuous()).add_systems(Update, smoke.before(player::run_ticks));
    }
    // A native session the server ended exits with code 3, for the launcher.
    if let AppExit::Error(code) = app.run() {
        std::process::exit(code.get() as i32);
    }
}

fn projection(fov_degrees: f32, view_chunks: i32) -> Projection {
    // EntityRenderer.setupCameraTransform: zNear 0.05. The far plane only bounds culling here, so
    // it reaches the corner of the loaded square, distant terrain included.
    let far = (view_chunks as f32 * 16.0 * std::f32::consts::SQRT_2).max(4096.0);
    Projection::Perspective(PerspectiveProjection { fov: fov_degrees.to_radians(), near: 0.05, far, ..default() })
}

/// The sun's shadow cascades: three over the Shadow Distance option, never past the render
/// distance. Every cascade draws all terrain inside it each frame, so this distance sets the
/// shaded mode's cost (four 4096 cascades over the whole render distance cost more than the
/// rest of the frame together).
fn cascades(settings: &Settings) -> CascadeShadowConfigBuilder {
    let chunks = settings.shadow_distance.min(settings.render_distance).max(2);
    CascadeShadowConfigBuilder {
        num_cascades: 3,
        first_cascade_far_bound: 12.0,
        maximum_distance: (chunks * 16) as f32,
        ..default()
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    atlas: Res<Atlas>,
    settings: Res<Settings>,
    clock: Res<Clock>,
    view: Res<View>,
    stars: Res<Stars>,
    cloud_texture: Res<CloudTexture>,
    env: Res<Env>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut chunk_materials: ResMut<Assets<ChunkMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut hand_materials: ResMut<Assets<HandMaterial>>,
) {
    // Terrain.
    let textures = &atlas.0.textures;
    let blocks = images.add(textures.image(settings.mipmaps));
    let params = ChunkParams::new(&textures.anim_table());
    let chunk = |alpha_mode| ChunkMaterial { params: params.clone(), blocks: blocks.clone(), env: env.image.clone(), alpha_mode, crack: false };
    let mut crack = chunk(AlphaMode::Blend);
    crack.crack = true;
    commands.insert_resource(ChunkMaterials {
        crack: chunk_materials.add(crack),
        solid: chunk_materials.add(chunk(AlphaMode::Opaque)),
        cutout: chunk_materials.add(chunk(AlphaMode::Mask(0.5))),
        translucent: chunk_materials.add(chunk(AlphaMode::Blend)),
    });

    // Sky and clouds, on the sky camera's layer until update_clouds moves the clouds.
    let sun = images.add(flat_image(&loaded.sun, false));
    let moon = images.add(flat_image(&loaded.moon, false));
    let sky_material = |kind| SkyMaterial { env: env.image.clone(), sun: sun.clone(), moon: moon.clone(), kind };
    commands.spawn((
        SkyMesh,
        Mesh3d(meshes.add(sky::sky_mesh(&clock, &view, &stars.0))),
        MeshMaterial3d(sky_materials.add(sky_material(SkyKind::Sky))),
        RenderLayers::layer(1),
        NoFrustumCulling,
        Transform::default(),
        Visibility::default(),
    ));
    let first = CloudKey { texel: (0, 0), bottom: true, top: false, alpha_ref: 0.5, reach: 0 };
    let cloud_mesh = meshes.add(clouds::cloud_mesh(&cloud_texture.0, first));
    for kind in [SkyKind::CloudDepth, SkyKind::CloudColor] {
        commands.spawn((
            CloudMesh,
            Mesh3d(cloud_mesh.clone()),
            MeshMaterial3d(sky_materials.add(sky_material(kind))),
            RenderLayers::layer(1),
            NoFrustumCulling,
            Transform::default(),
            Visibility::Hidden,
        ));
    }

    // The arm, and the GUI sprite sheet.
    let skin = images.add(flat_image(&loaded.skin, false));
    commands.insert_resource(players::Skin { skin: skin.clone(), env: env.image.clone() });
    commands.insert_resource(players::Font { image: images.add(flat_image(&loaded.font, false)), widths: players::Font::widths(&loaded.font) });
    // The glint texture is blurred (its .mcmeta) and repeats.
    let mut glint = flat_image(&loaded.glint, false);
    glint.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        ..Default::default()
    });
    commands.insert_resource(item_render::GlintImage(images.add(glint)));
    commands.insert_resource(survival::gui::enchanting::Galactic(players::Font { image: images.add(flat_image(&loaded.galactic, false)), widths: players::Font::widths(&loaded.galactic) }));
    commands.spawn((
        Arm,
        Mesh3d(meshes.add(hand::arm_mesh())),
        MeshMaterial3d(hand_materials.add(HandMaterial { env: env.image.clone(), skin })),
        RenderLayers::layer(2),
        NoFrustumCulling,
        Transform::default(),
        Visibility::default(),
    ));
    commands.insert_resource(GuiIcons(images.add(flat_image(&loaded.icons, true))));
    commands.insert_resource(GuiWidgets(images.add(flat_image(&loaded.widgets, true))));
    commands.insert_resource(survival::chest_render::ChestImages {
        single: images.add(flat_image(&loaded.chest[0], false)),
        double: images.add(flat_image(&loaded.chest[1], false)),
    });
    commands.insert_resource(survival::mobs::MobImages(loaded.mobs.iter().map(|t| images.add(flat_image(t, false))).collect()));
    commands.insert_resource(survival::mobs::FireImages { layers: [images.add(flat_image(&loaded.fire.0[0], false)), images.add(flat_image(&loaded.fire.0[1], false))], frames: loaded.fire.1, frametime: loaded.fire.2 });
    commands.insert_resource(survival::mobs::layer_images(&loaded.mob_layers, |t| images.add(flat_image(t, false))));
    commands.insert_resource(players::ArmorImages(loaded.armor.iter().map(|t| images.add(flat_image(t, false))).collect()));
    commands.insert_resource(survival::crit::CritImage(images.add(flat_image(&loaded.particles, true))));
    commands.insert_resource(survival::arrows::ArrowImage(images.add(flat_image(&loaded.arrow, true))));
    commands.insert_resource(survival::xp_orbs::XpOrbImage(images.add(flat_image(&loaded.xp_orb, true))));
    commands.insert_resource(survival::explosion::ExplosionImage(images.add(flat_image(&loaded.explosion, true))));
    commands.insert_resource(sign::SignImages { world: images.add(flat_image(&loaded.sign, false)), gui: images.add(flat_image(&loaded.sign, true)) });
    commands.insert_resource(survival::gui::ContainerImages {
        tabs: images.add(flat_image(&loaded.tabs, true)),
        backgrounds: loaded.backgrounds.each_ref().map(|b| images.add(flat_image(b, true))),
        empty_slots: images.add(flat_image(&loaded.empty_slots, true)),
    });
    commands.insert_resource(survival::gui::GuiLang(sim::lang::Lang::from_bytes(&loaded.lang)));
    commands.remove_resource::<Loaded>();

    // The shaded mode's sun. Only the main camera shares its layer, so only it gets cascades.
    commands.spawn((
        DirectionalLight { shadow_maps_enabled: false, ..default() },
        cascades(&settings).build(),
        Transform::default(),
        RenderLayers::layer(0),
    ));

    // sync_cameras places the sky and main cameras each frame.
    let rd = settings.view_chunks();
    commands.spawn((
        SkyCamera,
        Camera3d::default(),
        Camera { order: -1, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        projection(settings.fov, rd),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        Transform::default(),
        RenderLayers::layer(1),
    ));
    commands.spawn((
        MainCamera,
        Camera3d { depth_load_op: Camera3dDepthLoadOp::Load, ..default() },
        Camera { order: 0, clear_color: ClearColorConfig::None, ..default() },
        // One hardware 2x2 compare per fragment; the default Gaussian filter takes many more.
        ShadowFilteringMethod::Hardware2x2,
        projection(settings.fov, rd),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        Transform::default(),
    ));
    commands.spawn((
        HandCamera,
        Camera3d::default(),
        Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
        projection(70.0, 0),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        Transform::IDENTITY,
        RenderLayers::layer(2),
        IsDefaultUiCamera,
    ));
}

/// Applies option changes that need more than a shader parameter: the radius and its shadow
/// cascades, smooth lighting, the mipmap array, and the anti-aliasing on the cameras.
#[allow(clippy::too_many_arguments)]
fn apply_settings(
    mut commands: Commands,
    settings: Res<Settings>,
    mut last: Local<Option<Settings>>,
    mut streamer: ResMut<Streamer>,
    atlas: Res<Atlas>,
    chunk_materials: Res<ChunkMaterials>,
    mut materials: ResMut<Assets<ChunkMaterial>>,
    item_materials: Res<ItemMaterials>,
    mut items: ResMut<Assets<ItemMaterial>>,
    mut images: ResMut<Assets<Image>>,
    main: Single<Entity, With<MainCamera>>,
    light: Single<Entity, With<DirectionalLight>>,
    mut msaa: Query<&mut Msaa, With<Camera>>,
) {
    let s = &*settings;
    let prev = last.replace(s.clone());
    if prev.as_ref() == Some(s) {
        return;
    }
    if prev.as_ref().is_none_or(|p| p.render_distance != s.render_distance) {
        streamer.set_radius(s.render_distance);
    }
    if prev.as_ref().is_none_or(|p| p.render_distance != s.render_distance || p.shadow_distance != s.shadow_distance) {
        commands.entity(*light).insert(cascades(s).build());
    }
    streamer.set_smooth(s.smooth != Smooth::Off);
    if prev.as_ref().is_some_and(|p| p.mipmaps != s.mipmaps) {
        let blocks = images.add(atlas.0.textures.image(s.mipmaps));
        for h in [&chunk_materials.solid, &chunk_materials.cutout, &chunk_materials.translucent, &chunk_materials.crack] {
            if let Some(mut m) = materials.get_mut(h) {
                m.blocks = blocks.clone();
            }
        }
        for h in item_materials.all() {
            if let Some(mut m) = items.get_mut(h) {
                m.blocks = blocks.clone();
            }
        }
    }
    if prev.as_ref().is_none_or(|p| p.aa != s.aa) {
        let mut e = commands.entity(*main);
        e.remove::<(Fxaa, Smaa)>();
        match s.aa {
            Aa::Fxaa => {
                e.insert(Fxaa::default());
            }
            Aa::Smaa => {
                e.insert(Smaa::default());
            }
            Aa::Off | Aa::Msaa => {}
        }
        // Cameras sharing a window share its textures only at the same sample count.
        let samples = if s.aa == Aa::Msaa { Msaa::Sample4 } else { Msaa::Off };
        for mut m in &mut msaa {
            *m = samples;
        }
    }
}

/// `+` and `-` step the render distance, as the menu does.
fn radius_keys(keys: Res<ButtonInput<KeyCode>>, menu: Res<Menu>, mut settings: ResMut<Settings>) {
    if menu.open {
        return;
    }
    if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        settings.cycle(Opt::RenderDistance, 1);
    } else if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        settings.cycle(Opt::RenderDistance, -1);
    }
}

/// The main camera takes the view's transform and the sky camera copies it; the main camera's
/// FOV takes the flying and sprint modifier (`getFOVModifier(pt, true)`), the hand camera's stays
/// at 70, or 60 in water (`getFOVModifier(pt, false)`).
fn sync_cameras(
    clock: Res<Clock>,
    settings: Res<Settings>,
    view: Res<View>,
    main: Single<(&mut Transform, &mut Projection), (With<MainCamera>, Without<SkyCamera>, Without<HandCamera>)>,
    sky: Single<(&mut Transform, &mut Projection), (With<SkyCamera>, Without<MainCamera>, Without<HandCamera>)>,
    mut hand_projection: Single<&mut Projection, (With<HandCamera>, Without<MainCamera>, Without<SkyCamera>)>,
) {
    let (mut main_transform, mut main_projection) = main.into_inner();
    *main_transform = Transform::from_matrix(view.camera);
    *main_projection = projection(view.fov(&settings, clock.partial), settings.view_chunks());
    let (mut sky_transform, mut sky_projection) = sky.into_inner();
    *sky_transform = *main_transform;
    *sky_projection = main_projection.clone();
    let hand_fov = if view.fluid == celestial::Fluid::Water { 60.0 } else { 70.0 };
    **hand_projection = projection(hand_fov, 0);
}

/// `--survival[:dead]`: survival mode with sample numbers, or the death screen.
fn preview_survival(
    args: Res<Args>,
    mut player: ResMut<Player>,
    mut state: ResMut<survival::SurvivalState>,
    mut vitals: ResMut<survival::hurt::Vitals>,
    mut screen: ResMut<survival::death::DeathScreen>,
    mut menu: ResMut<Menu>,
) {
    let Some(dead) = args.survival else { return };
    player.game_mode = proto::GM_SURVIVAL;
    state.food = 15;
    state.xp = proto::Xp { bar: 0.4, level: 7, total: 91 };
    vitals.set_health(20.0);
    if dead {
        state.health = 0.0;
        state.dead = true;
        vitals.set_health(0.0);
        screen.open = Some(survival::death::Screen { message: "xy fell from a high place".into(), score: state.xp.total, ticks: 20 });
        menu.open = true;
    } else {
        state.health = 13.5;
        vitals.set_health(13.5);
    }
}

/// Smoke run: fly 23 blocks over the spawn along +x at 40 blocks per second, stepping through
/// five option sets so every pipeline gets built, then print the stats and exit. With `--shot`,
/// hover at `--at` with the options as set, and take the screenshot instead of exiting.
#[allow(clippy::too_many_arguments)]
fn smoke(
    mut commands: Commands,
    args: Res<Args>,
    (streamer, lod): (Res<Streamer>, Res<lod::Lod>),
    clock: Res<Clock>,
    diagnostics: Res<DiagnosticsStore>,
    mut settings: ResMut<Settings>,
    mut hand: ResMut<Hand>,
    mut player: ResMut<Player>,
    mut started: Local<Option<Instant>>,
    mut origin: Local<Option<DVec3>>,
    mut frames: Local<u64>,
    mut phase: Local<usize>,
    mut done: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    if *done {
        return;
    }
    let start = *started.get_or_insert_with(Instant::now);
    *frames += 1;
    let elapsed = start.elapsed();
    let shot = args.shot.is_some();
    player.flying = true;
    if let (true, Some([x, y, z, yaw, pitch])) = (shot, args.at) {
        // `--at` gives the eye, standing.
        player.teleport(DVec3::new(x as f64, y as f64 - 1.62f32 as f64, z as f64), yaw, pitch);
    } else if !shot {
        let o = *origin.get_or_insert(player.pos + DVec3::new(0.0, 23.0, 0.0));
        let (yaw, pitch) = (player.yaw, player.pitch);
        player.teleport(o + DVec3::new(40.0 * elapsed.as_secs_f64(), 0.0, 0.0), yaw, pitch);
    }
    let total = args.smoke.unwrap_or(0.0).max(0.001);
    let now = ((elapsed.as_secs_f32() / total * 5.0) as usize).min(4);
    if !shot && (now != *phase || *frames == 1) {
        *phase = now;
        let (aa, mode, time_of_day, mipmaps, smooth) = [
            (Aa::Fxaa, Mode::Vanilla, TimeOfDay::Cycle, 4, Smooth::Maximum),
            (Aa::Smaa, Mode::Shaded, TimeOfDay::Noon, 4, Smooth::Maximum),
            (Aa::Msaa, Mode::Shaded, TimeOfDay::Sunset, 2, Smooth::Maximum),
            (Aa::Smaa, Mode::Vanilla, TimeOfDay::Midnight, 4, Smooth::Maximum),
            (Aa::Off, Mode::Shaded, TimeOfDay::Day, 0, Smooth::Off),
        ][now];
        settings.aa = aa;
        settings.mode = mode;
        settings.time = time_of_day;
        settings.mipmaps = mipmaps;
        settings.smooth = smooth;
        settings.clouds = Clouds::Fancy;
        hand.clicks += 1;
    }
    if elapsed < Duration::from_secs_f32(total) {
        return;
    }
    let s = &streamer.stats;
    let mem = hud::process_mem_mb(&diagnostics).map_or("-".into(), |v| format!("{v:.0}"));
    let (lod_chunks, lod_sections) = lod.counts();
    println!(
        "SMOKE seconds={:.1} frames={} fps_avg={:.1} generated={} meshed={} loaded={} tracked={} faces={} lod_chunks={lod_chunks} lod_sections={lod_sections} lod_quads={} avg_gen_ms={:.3} avg_mesh_ms={:.3} mem_mb={mem} x={:.0} world_time={} swing={:.2}",
        elapsed.as_secs_f64(),
        *frames,
        *frames as f64 / elapsed.as_secs_f64(),
        s.generated_total,
        s.meshed_total,
        streamer.loaded(),
        streamer.known(),
        s.faces,
        lod.quads,
        s.avg_gen_ms(),
        s.avg_mesh_ms(),
        player.pos.x,
        clock.world_time,
        hand.swing,
    );
    *done = true;
    if let Some(path) = &args.shot {
        commands.spawn(Screenshot::primary_window()).observe(capture::save_png(path.clone()));
    } else {
        exit.write(if streamer.loaded() > 0 { AppExit::Success } else { AppExit::error() });
    }
}
