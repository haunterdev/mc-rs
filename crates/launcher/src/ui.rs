//! The launcher window: two tabs, a world of its own (seed field, Play) and Join a server
//! (join.rs), then progress for the install and downloads, errors with the end of the viewer's
//! log, and the update banner. Long work runs on its own thread and reports into `Shared`, which
//! the window reads each frame.

use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align, Button, Color32, ColorImage, CornerRadius, FontId, Frame, Key, Layout, ProgressBar, RichText, ScrollArea,
    TextEdit, TextStyle, TextureHandle, TextureOptions, Vec2,
};

use crate::join::Form;
use crate::update::{self, Manifest};
use crate::uri::Link;
use crate::{game, net, payload};

const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 540.0;
const GREEN: Color32 = Color32::from_rgb(60, 133, 39);
const RED: Color32 = Color32::from_rgb(90, 32, 32);
const BLUE: Color32 = Color32::from_rgb(30, 58, 92);
const CONTROLS: &str = "WASD to walk, Space to jump (twice to fly), Ctrl to sprint, Shift to sneak, \
                        Esc for options. Click into the window to look around.";
const CHAT: &str = "T opens chat, Enter sends; commands start with /.";

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// No client jar on this PC: the window asks before downloading one.
    AskJar,
    Working { what: String, done: u64, total: Option<u64> },
    Running { seed: String },
    Finished { seed: String },
    /// Playing on a server (join.rs).
    Joined { server: String },
    Left { server: String },
    Failed { message: String, log: Vec<String> },
}

/// Which of the window's two tabs shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tab {
    World,
    Server,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateState {
    /// No update channel was built in.
    Off,
    Checking,
    Current,
    Available(Manifest),
    Downloading { done: u64, total: Option<u64> },
    Failed(String),
}

struct Shared {
    phase: Phase,
    update: UpdateState,
}

/// What a worker thread holds: the shared state and the context to wake the window with.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
}

impl Handle {
    pub fn phase(&self, p: Phase) {
        self.shared.lock().unwrap().phase = p;
        self.ctx.request_repaint();
    }

    fn update(&self, u: UpdateState) {
        self.shared.lock().unwrap().update = u;
        self.ctx.request_repaint();
    }
}

pub struct Launcher {
    handle: Handle,
    seed: String,
    updated_from: Option<String>,
    tab: Tab,
    join: Form,
    /// Why an `mcrs://` link was refused.
    link_error: Option<String>,
    logo: TextureHandle,
}

pub fn run(updated_from: Option<String>, link: Option<Result<Link, String>>) -> eframe::Result {
    let icon = egui::IconData { rgba: brand::icon_rgba(64), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("mc-rs launcher")
            .with_inner_size([WIDTH, HEIGHT])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(icon),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "mc-rs launcher",
        options,
        Box::new(move |cc| Ok(Box::new(Launcher::new(cc.egui_ctx.clone(), updated_from, link)))),
    )
}

impl Launcher {
    fn new(ctx: egui::Context, updated_from: Option<String>, link: Option<Result<Link, String>>) -> Launcher {
        ctx.set_visuals(egui::Visuals::dark());
        ctx.all_styles_mut(|s| {
            s.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
            s.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
            s.text_styles.insert(TextStyle::Small, FontId::proportional(12.5));
            s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
            s.spacing.button_padding = Vec2::new(10.0, 5.0);
            s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        });
        let logo = ctx.load_texture(
            "logo",
            ColorImage::from_rgba_unmultiplied([64, 64], &brand::icon_rgba(64)),
            TextureOptions::LINEAR,
        );
        let update = if update::URL.is_some() { UpdateState::Checking } else { UpdateState::Off };
        let handle = Handle { shared: Arc::new(Mutex::new(Shared { phase: Phase::Idle, update })), ctx };
        if update::URL.is_some() {
            let h = handle.clone();
            std::thread::spawn(move || {
                h.update(match update::check() {
                    Ok(Some(m)) => UpdateState::Available(m),
                    Ok(None) => UpdateState::Current,
                    Err(e) => UpdateState::Failed(e.to_string()),
                })
            });
        }
        let (server, link_error) = match link {
            Some(Ok(Link::Join(s))) => (Some(s), None),
            Some(Err(e)) => (None, Some(e)),
            Some(Ok(Link::Play)) | None => (None, None),
        };
        let tab = if server.is_some() { Tab::Server } else { Tab::World };
        let join = Form::new(server.as_ref());
        Launcher { handle, seed: String::new(), updated_from, tab, join, link_error, logo }
    }

    /// Play or Join, whichever tab shows. `download` is the player's yes to fetching the jar
    /// when none is on the PC.
    fn start(&self, download: bool) {
        match self.tab {
            Tab::World => self.play(download),
            Tab::Server => self.join.start(self.handle.clone(), download),
        }
    }

    /// Install, then the client jar, then the viewer, on a worker thread.
    fn play(&self, download: bool) {
        let h = self.handle.clone();
        let seed = self.seed.trim().to_owned();
        h.phase(Phase::Working { what: "Preparing".into(), done: 0, total: None });
        std::thread::spawn(move || {
            let Some((viewer, jar)) = prepare(&h, download) else { return };
            let seed = if seed.is_empty() { game::random_seed() } else { seed };
            let mut child = match game::start(&viewer, &["--seed", &seed], &[], &jar) {
                Ok(c) => c,
                Err(e) => return h.phase(Phase::Failed { message: format!("mc-rs could not start: {e}"), log: Vec::new() }),
            };
            h.phase(Phase::Running { seed: seed.clone() });
            h.phase(match child.wait() {
                Ok(s) if s.success() => Phase::Finished { seed },
                Ok(s) => exit_failure(s, game::log_tail(14)),
                Err(e) => Phase::Failed { message: e.to_string(), log: Vec::new() },
            });
        });
    }

    fn apply_update(&self, m: Manifest) {
        let h = self.handle.clone();
        h.update(UpdateState::Downloading { done: 0, total: m.size });
        std::thread::spawn(move || {
            match update::apply(&m, &|done, total| h.update(UpdateState::Downloading { done, total })) {
                Ok(()) => std::process::exit(0),
                Err(e) => h.update(UpdateState::Failed(format!("Updating failed: {e}"))),
            }
        });
    }

    /// The Single player tab: the seed and Play.
    fn world_tab(&mut self, ui: &mut egui::Ui, busy: bool, phase: &Phase) {
        ui.label(RichText::new("World seed").strong());
        let mut enter = false;
        ui.horizontal(|ui| {
            let edit = ui.add_enabled(
                !busy,
                TextEdit::singleline(&mut self.seed).hint_text("Random").desired_width(ui.available_width() - 82.0),
            );
            enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.add_enabled(!busy, Button::new("Random")).on_hover_text("Fill in a random seed").clicked() {
                self.seed = game::random_seed();
            }
        });
        ui.label(RichText::new("A number or any text, as on Minecraft's world screen. Empty picks a random world.").small().weak());
        ui.add_space(6.0);

        let label = match *phase {
            Phase::Running { .. } => "Playing",
            Phase::Working { .. } | Phase::AskJar => "Preparing",
            _ => "Play",
        };
        let play = ui.add_enabled(
            !busy,
            Button::new(RichText::new(label).size(21.0).strong().color(Color32::WHITE))
                .fill(GREEN)
                .corner_radius(CornerRadius::same(6))
                .min_size(Vec2::new(ui.available_width(), 50.0)),
        );
        if play.clicked() || (enter && !busy) {
            self.play(false);
        }
    }

    /// The whole window. Returns the bottom of the main content, which the layout test checks
    /// against the footer.
    pub fn draw(&mut self, ui: &mut egui::Ui) -> f32 {
        let (phase, upd) = {
            let s = self.handle.shared.lock().unwrap();
            (s.phase.clone(), s.update.clone())
        };

        let busy = matches!(phase, Phase::Working { .. } | Phase::Running { .. } | Phase::Joined { .. } | Phase::AskJar);

        ui.horizontal(|ui| {
            ui.image((self.logo.id(), Vec2::splat(44.0)));
            ui.vertical(|ui| {
                ui.label(RichText::new("mc-rs").size(26.0).strong().color(Color32::WHITE));
                ui.label(RichText::new("Minecraft 1.12.2 terrain, rebuilt in Rust").weak());
            });
            // The version over the two tabs, which stay put while a game runs.
            ui.with_layout(Layout::top_down(Align::Max), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(format!("v{}", update::VERSION)).small().weak());
                ui.add_enabled_ui(!busy, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.tab, Tab::World, "Single player");
                        ui.selectable_value(&mut self.tab, Tab::Server, "Join a server");
                    });
                });
            });
        });
        ui.separator();

        match &upd {
            UpdateState::Available(m) => {
                let m = m.clone();
                banner(ui, BLUE, |ui| {
                    ui.horizontal(|ui| {
                        let r = ui.label(RichText::new(format!("mc-rs v{} is out.", m.version)).strong());
                        if !m.notes.is_empty() {
                            r.on_hover_text(&m.notes);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Update and restart").clicked() {
                                self.apply_update(m.clone());
                            }
                        });
                    });
                });
            }
            UpdateState::Downloading { done, total } => {
                banner(ui, BLUE, |ui| {
                    ui.add(progress(*done, *total).text("Downloading the update"));
                });
            }
            _ => {}
        }

        if let Some(e) = &self.link_error {
            let mut close = false;
            banner(ui, RED, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("That link did not open mc-rs: {e}"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| close = ui.small_button("Close").clicked());
                });
            });
            if close {
                self.link_error = None;
            }
        }
        if self.tab == Tab::Server {
            let label = match phase {
                Phase::Joined { .. } => "Playing",
                Phase::Working { .. } | Phase::AskJar => "Joining",
                _ => "Join",
            };
            let hint = matches!(phase, Phase::Idle | Phase::Left { .. });
            if self.join.draw(ui, busy, label, hint) {
                self.start(false);
            }
        } else {
            self.world_tab(ui, busy, &phase);
        }
        ui.add_space(6.0);

        match &phase {
            Phase::Idle => {
                ui.label(RichText::new(CONTROLS).small().weak());
            }
            Phase::AskJar => {
                banner(ui, BLUE, |ui| {
                    ui.label(
                        "mc-rs draws the world with Minecraft 1.12.2's own textures, and this PC has no \
                         1.12.2 client. Download it from Mojang? It is 10 MB, once.",
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Download and play").clicked() {
                            self.start(true);
                        }
                        if ui.button("Cancel").clicked() {
                            self.handle.phase(Phase::Idle);
                        }
                    });
                });
            }
            Phase::Working { what, done, total } => {
                ui.label(what.as_str());
                ui.add(progress(*done, *total));
            }
            Phase::Running { seed } => {
                seed_row(ui, "Playing seed", seed);
                ui.label(RichText::new("Closing the game window brings you back here.").small().weak());
                ui.label(RichText::new(CONTROLS).small().weak());
            }
            Phase::Finished { seed } => {
                seed_row(ui, "Last world: seed", seed);
                if ui.small_button("Play it again").clicked() {
                    self.seed = seed.clone();
                    self.tab = Tab::World;
                    self.play(false);
                }
            }
            Phase::Joined { server } => {
                ui.label(format!("Playing on {server}"));
                ui.label(RichText::new("Closing the game window brings you back here.").small().weak());
                ui.label(RichText::new(format!("{CONTROLS} {CHAT}")).small().weak());
            }
            Phase::Left { server } => {
                ui.label(format!("Left {server}."));
            }
            Phase::Failed { message, log } => {
                banner(ui, RED, |ui| {
                    ui.label(RichText::new(message).strong());
                    if !log.is_empty() {
                        ScrollArea::vertical().max_height(72.0).stick_to_bottom(true).show(ui, |ui| {
                            ui.label(RichText::new(log.join("\n")).monospace());
                        });
                    }
                    ui.horizontal(|ui| {
                        if !log.is_empty() && ui.button("Copy error").clicked() {
                            ui.ctx().copy_text(format!("{message}\n{}", log.join("\n")));
                        }
                        if let Some(p) = game::log_path().filter(|p| p.is_file()) {
                            if ui.button("Open log").clicked() {
                                game::reveal(&p);
                            }
                        }
                    });
                });
            }
        }
        let content_bottom = ui.cursor().top();

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                if ui.link("Game folder").on_hover_text("Options, logs and the downloaded client").clicked() {
                    if let Some(d) = paths::data_dir() {
                        game::reveal(&d);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = match &upd {
                        _ if self.updated_from.is_some() && matches!(upd, UpdateState::Checking | UpdateState::Current) => {
                            format!("Updated from v{}", self.updated_from.as_deref().unwrap_or_default())
                        }
                        UpdateState::Off => String::new(),
                        UpdateState::Checking => "Checking for updates".into(),
                        UpdateState::Current => "Up to date".into(),
                        UpdateState::Available(m) => format!("v{} available", m.version),
                        UpdateState::Downloading { .. } => "Updating".into(),
                        UpdateState::Failed(_) => "Update check failed".into(),
                    };
                    let r = ui.label(RichText::new(text).small().weak());
                    if let UpdateState::Failed(e) = &upd {
                        r.on_hover_text(e);
                    }
                });
            });
            ui.separator();
        });
        content_bottom
    }
}

impl eframe::App for Launcher {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default_margins()
            .frame(Frame::central_panel(ui.style()).inner_margin(18))
            .show(ui, |ui| {
                self.draw(ui);
            });
    }
}

/// Installs the viewer and finds (or, with `download`, fetches) the client jar, reporting into
/// the window; `None` once it has set the phase to a failure or to asking for the jar.
pub fn prepare(h: &Handle, download: bool) -> Option<(PathBuf, PathBuf)> {
    let fail = |message: String| h.phase(Phase::Failed { message, log: Vec::new() });
    let viewer = match payload::install(&|done, total| h.phase(Phase::Working { what: "Installing mc-rs".into(), done, total: Some(total) })) {
        Ok(v) => v,
        Err(e) => {
            fail(format!("Installing mc-rs failed: {e}"));
            return None;
        }
    };
    let jar = match paths::find_client_jar() {
        Some(j) => j,
        None if !download => {
            h.phase(Phase::AskJar);
            return None;
        }
        None => {
            let Some(dest) = paths::cached_jar() else {
                fail("APPDATA is not set".into());
                return None;
            };
            let what = "Downloading Minecraft 1.12.2 from Mojang".to_owned();
            let r = net::download(paths::CLIENT_URL, &dest, net::Hash::Sha1(paths::CLIENT_SHA1), &|done, total| {
                h.phase(Phase::Working { what: what.clone(), done, total })
            });
            if let Err(e) = r {
                fail(format!("Downloading Minecraft 1.12.2 failed: {e}"));
                return None;
            }
            dest
        }
    };
    Some((viewer, jar))
}

/// The game ended with an error: its exit code and the end of its log.
pub fn exit_failure(s: ExitStatus, log: Vec<String>) -> Phase {
    let crashed = log.iter().any(|l| l.contains("panicked at"));
    let what = if crashed { "mc-rs crashed" } else { "mc-rs closed with an error" };
    let message = match s.code() {
        // Windows status codes (0xC0000409 for an aborting panic) read as large negatives.
        Some(c) if c < 0 => format!("{what} (exit code 0x{:08X}).", c as u32),
        Some(c) => format!("{what} (exit code {c})."),
        None => format!("{what}."),
    };
    Phase::Failed { message, log }
}

fn banner(ui: &mut egui::Ui, fill: Color32, add: impl FnOnce(&mut egui::Ui)) {
    Frame::new().fill(fill).corner_radius(CornerRadius::same(6)).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn seed_row(ui: &mut egui::Ui, what: &str, seed: &str) {
    ui.horizontal(|ui| {
        ui.label(format!("{what} {seed}"));
        if ui.small_button("Copy").on_hover_text("Copy the seed to share it").clicked() {
            ui.ctx().copy_text(seed.to_owned());
        }
    });
}

fn progress(done: u64, total: Option<u64>) -> ProgressBar {
    let mb = |b: u64| b as f64 / 1e6;
    match total.filter(|&t| t > 0) {
        Some(t) => ProgressBar::new(done as f32 / t as f32).text(format!("{:.1} of {:.1} MB", mb(done), mb(t))),
        None => ProgressBar::new(0.0).animate(true).text(format!("{:.1} MB", mb(done))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_crash_reads_as_one() {
        use std::os::windows::process::ExitStatusExt;
        let log = vec!["thread 'main' panicked at src/main.rs:1:1:".to_owned()];
        let Phase::Failed { message, .. } = exit_failure(ExitStatus::from_raw(0xC000_0409), log) else { panic!() };
        assert_eq!(message, "mc-rs crashed (exit code 0xC0000409).");
        let Phase::Failed { message, .. } = exit_failure(ExitStatus::from_raw(3), Vec::new()) else { panic!() };
        assert_eq!(message, "mc-rs closed with an error (exit code 3).");
    }

    /// Lays every phase out on both tabs in a window-sized context and checks the content stays
    /// above the footer, the longest case being an error with a full log tail on the Join tab.
    #[test]
    fn every_phase_fits_the_window() {
        let ctx = egui::Context::default();
        let link = crate::uri::parse("mcrs://join/a-long-tunnel-name.gl.joinmc.link:41234");
        let mut app = Launcher::new(ctx.clone(), Some("0.1.0".into()), Some(link));
        assert_eq!((app.tab, app.join.address.as_str()), (Tab::Server, "a-long-tunnel-name.gl.joinmc.link:41234"));
        let log: Vec<String> = (0..14).map(|i| format!("2026-10-10T12:00:{i:02} ERROR viewer: a long line of log output that wraps")).collect();
        let manifest = Manifest { version: "9.9.9".into(), url: "https://x".into(), sha256: "0".into(), size: None, notes: "Trees.".into() };
        let phases = [
            Phase::Idle,
            Phase::AskJar,
            Phase::Working { what: "Installing mc-rs".into(), done: 3_000_000, total: Some(8_000_000) },
            Phase::Running { seed: "-4172144997902289642".into() },
            Phase::Finished { seed: "42".into() },
            Phase::Joined { server: "a-long-tunnel-name.trycloudflare.com".into() },
            Phase::Left { server: "a-long-tunnel-name.trycloudflare.com".into() },
            Phase::Failed { message: "Could not join: the server did not let this player in within 30 seconds".into(), log },
        ];
        for (phase, tab) in phases.iter().flat_map(|p| [(p.clone(), Tab::World), (p.clone(), Tab::Server)]) {
            app.tab = tab;
            for upd in [UpdateState::Current, UpdateState::Available(manifest.clone())] {
                {
                    let mut s = app.handle.shared.lock().unwrap();
                    s.phase = phase.clone();
                    s.update = upd.clone();
                }
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(WIDTH, HEIGHT))),
                    ..Default::default()
                };
                let mut bottom = 0.0;
                // Two passes: egui sizes some widgets from the frame before.
                for _ in 0..2 {
                    let mut out = ctx.run_ui(input.clone(), |ui| {
                        egui::CentralPanel::default_margins()
                            .frame(Frame::central_panel(ui.style()).inner_margin(18))
                            .show(ui, |ui| bottom = app.draw(ui));
                    });
                    // No painter here to upload the font atlas and logo to.
                    out.textures_delta.clear();
                }
                assert!(bottom < HEIGHT - 50.0, "{phase:?} on {tab:?} with {upd:?} reaches {bottom}");
            }
        }
    }
}
