//! The options menu (Esc) and its file, `%APPDATA%\mc-rs\options.txt`, written as `key:value`
//! lines like the game's own `options.txt`.
//!
//! Each button cycles its option: left click or wheel up steps forward, right click or wheel down
//! steps back. Systems elsewhere read [`Settings`] and react when it changes.

use std::path::PathBuf;

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::hotbar::GuiWidgets;
use crate::input::Focus;

/// `GameSettings.ambientOcclusion`: Minimum and Maximum render the same in 1.12.2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Smooth {
    Off,
    Minimum,
    Maximum,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Aa {
    Off,
    Fxaa,
    Smaa,
    Msaa,
}

/// Vanilla is the fixed-function look; Shaded adds a sun with shadows (not in the game).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Vanilla,
    Shaded,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clouds {
    Off,
    Fancy,
}

/// A fixed world time, as `/time set` names them, or the running day cycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeOfDay {
    Day,
    Noon,
    Sunset,
    Midnight,
    Cycle,
}

impl TimeOfDay {
    pub fn ticks(self) -> Option<i64> {
        match self {
            TimeOfDay::Day => Some(1000),
            TimeOfDay::Noon => Some(6000),
            TimeOfDay::Sunset => Some(12000),
            TimeOfDay::Midnight => Some(18000),
            TimeOfDay::Cycle => None,
        }
    }
}

#[derive(Resource, Clone, PartialEq, Debug)]
pub struct Settings {
    /// Chunks, from 2 up, with no upper limit.
    pub render_distance: i32,
    pub smooth: Smooth,
    /// `mipmapLevels`, 0..=4.
    pub mipmaps: u32,
    /// `gammaSetting`, 0..=1.
    pub gamma: f32,
    pub aa: Aa,
    pub mode: Mode,
    pub shadows: bool,
    pub clouds: Clouds,
    pub time: TimeOfDay,
    /// Degrees, 30..=110.
    pub fov: f32,
    /// `mouseSensitivity`, 0..=1.
    pub sensitivity: f32,
    /// `viewBobbing`
    pub bob_view: bool,
    /// `autoJump`
    pub auto_jump: bool,
    /// Distant terrain radius in chunks, one of [`LOD`]; 0 is off (not in the game).
    pub lod_distance: i32,
    /// How far the shaded mode's sun casts shadows, in chunks, one of [`SHADOW`].
    pub shadow_distance: i32,
    /// `soundCategory_master`, `soundCategory_music`, and `soundCategory_block` (block, player
    /// and ambient sounds together), 0..=1.
    pub master_volume: f32,
    pub music_volume: f32,
    pub sound_volume: f32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            render_distance: 12,
            smooth: Smooth::Maximum,
            mipmaps: 4,
            gamma: 0.0,
            aa: Aa::Fxaa,
            mode: Mode::Vanilla,
            shadows: true,
            clouds: Clouds::Fancy,
            time: TimeOfDay::Cycle,
            fov: 70.0,
            sensitivity: 0.5,
            bob_view: true,
            auto_jump: true,
            lod_distance: 64,
            shadow_distance: 6,
            master_volume: 1.0,
            music_volume: 1.0,
            sound_volume: 1.0,
        }
    }
}

const SMOOTH: [Smooth; 3] = [Smooth::Off, Smooth::Minimum, Smooth::Maximum];
const AA: [Aa; 4] = [Aa::Off, Aa::Fxaa, Aa::Smaa, Aa::Msaa];
const MODE: [Mode; 2] = [Mode::Vanilla, Mode::Shaded];
const CLOUDS: [Clouds; 2] = [Clouds::Off, Clouds::Fancy];
const TIME: [TimeOfDay; 5] = [TimeOfDay::Day, TimeOfDay::Noon, TimeOfDay::Sunset, TimeOfDay::Midnight, TimeOfDay::Cycle];
/// The distant terrain choices. The shown-chunk mask (lod_mesh.rs, 1024 wide) holds any of
/// them without wrapping onto a shown chunk.
pub const LOD: [i32; 10] = [0, 32, 48, 64, 96, 128, 192, 256, 384, 512];
/// The shadow distance choices.
pub const SHADOW: [i32; 5] = [4, 6, 8, 12, 16];

fn step<T: Copy + PartialEq>(all: &[T], v: T, dir: i32) -> T {
    let i = all.iter().position(|&x| x == v).unwrap_or(0) as i32;
    all[(i + dir).rem_euclid(all.len() as i32) as usize]
}

/// The options the menu shows, in its order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Component)]
pub enum Opt {
    RenderDistance,
    Smooth,
    Mipmaps,
    Gamma,
    Aa,
    Mode,
    Shadows,
    Clouds,
    Time,
    Fov,
    Sensitivity,
    BobView,
    AutoJump,
    Lod,
    ShadowDistance,
    MasterVolume,
    MusicVolume,
    SoundVolume,
}

const OPTS: [Opt; 18] = [
    Opt::RenderDistance,
    Opt::Smooth,
    Opt::Mipmaps,
    Opt::Gamma,
    Opt::Aa,
    Opt::Mode,
    Opt::Shadows,
    Opt::Clouds,
    Opt::Time,
    Opt::Fov,
    Opt::Sensitivity,
    Opt::BobView,
    Opt::AutoJump,
    Opt::Lod,
    Opt::ShadowDistance,
    Opt::MasterVolume,
    Opt::MusicVolume,
    Opt::SoundVolume,
];

impl Settings {
    /// The distant terrain radius, 0 when it would not reach past the render distance.
    pub fn lod_chunks(&self) -> i32 {
        if self.lod_distance > self.render_distance {
            self.lod_distance
        } else {
            0
        }
    }

    /// How far terrain of either kind reaches; fog and the far plane go this far.
    pub fn view_chunks(&self) -> i32 {
        self.render_distance.max(self.lod_chunks())
    }

    /// Moves one option a step forward (`dir` 1) or back (-1). Numbers stop at their ends;
    /// choices wrap.
    pub fn cycle(&mut self, opt: Opt, dir: i32) {
        match opt {
            Opt::RenderDistance => self.render_distance = (self.render_distance + dir).max(2),
            Opt::Smooth => self.smooth = step(&SMOOTH, self.smooth, dir),
            Opt::Mipmaps => self.mipmaps = (self.mipmaps as i32 + dir).clamp(0, 4) as u32,
            Opt::Gamma => self.gamma = ((self.gamma * 10.0).round() + dir as f32).clamp(0.0, 10.0) / 10.0,
            Opt::Aa => self.aa = step(&AA, self.aa, dir),
            Opt::Mode => self.mode = step(&MODE, self.mode, dir),
            Opt::Shadows => self.shadows = !self.shadows,
            Opt::Clouds => self.clouds = step(&CLOUDS, self.clouds, dir),
            Opt::Time => self.time = step(&TIME, self.time, dir),
            Opt::Fov => self.fov = (self.fov + 5.0 * dir as f32).clamp(30.0, 110.0),
            Opt::Sensitivity => self.sensitivity = ((self.sensitivity * 20.0).round() + dir as f32).clamp(0.0, 20.0) / 20.0,
            Opt::BobView => self.bob_view = !self.bob_view,
            Opt::AutoJump => self.auto_jump = !self.auto_jump,
            Opt::Lod => self.lod_distance = step(&LOD, self.lod_distance, dir),
            Opt::ShadowDistance => self.shadow_distance = step(&SHADOW, self.shadow_distance, dir),
            Opt::MasterVolume => self.master_volume = volume_step(self.master_volume, dir),
            Opt::MusicVolume => self.music_volume = volume_step(self.music_volume, dir),
            Opt::SoundVolume => self.sound_volume = volume_step(self.sound_volume, dir),
        }
    }

    /// The button text, worded like the game's video settings where the option exists there.
    pub fn label(&self, opt: Opt) -> String {
        match opt {
            Opt::RenderDistance => format!("Render Distance: {} chunks", self.render_distance),
            Opt::Smooth => format!("Smooth Lighting: {:?}", self.smooth).replace("Off", "OFF"),
            Opt::Mipmaps => match self.mipmaps {
                0 => "Mipmap Levels: OFF".into(),
                n => format!("Mipmap Levels: {n}"),
            },
            Opt::Gamma => {
                let g = (self.gamma * 100.0).round() as i32;
                match g {
                    0 => "Brightness: Moody".into(),
                    100 => "Brightness: Bright".into(),
                    _ => format!("Brightness: +{g}%"),
                }
            }
            Opt::Aa => format!(
                "Anti-aliasing: {}",
                match self.aa {
                    Aa::Off => "OFF",
                    Aa::Fxaa => "FXAA",
                    Aa::Smaa => "SMAA",
                    Aa::Msaa => "MSAA 4x",
                }
            ),
            Opt::Mode => format!("Lighting: {:?}", self.mode),
            Opt::Shadows => format!("Shadows: {}", if self.shadows { "ON" } else { "OFF" }),
            Opt::Clouds => format!("Clouds: {}", if self.clouds == Clouds::Fancy { "Fancy" } else { "OFF" }),
            Opt::Time => format!("Time: {:?}", self.time),
            Opt::Fov => match self.fov as i32 {
                70 => "FOV: Normal".into(),
                110 => "FOV: Quake Pro".into(),
                f => format!("FOV: {f}"),
            },
            Opt::Sensitivity => match self.sensitivity {
                0.0 => "Sensitivity: *yawn*".into(),
                1.0 => "Sensitivity: HYPERSPEED!!!".into(),
                f => format!("Sensitivity: {}%", (f * 200.0) as i32),
            },
            Opt::BobView => format!("View Bobbing: {}", if self.bob_view { "ON" } else { "OFF" }),
            Opt::AutoJump => format!("Auto-Jump: {}", if self.auto_jump { "ON" } else { "OFF" }),
            Opt::Lod => match self.lod_distance {
                0 => "Distant Terrain: OFF".into(),
                n => format!("Distant Terrain: {n} chunks"),
            },
            Opt::ShadowDistance => format!("Shadow Distance: {} chunks", self.shadow_distance),
            Opt::MasterVolume => volume_label("Master Volume", self.master_volume),
            Opt::MusicVolume => volume_label("Music", self.music_volume),
            Opt::SoundVolume => volume_label("Sounds", self.sound_volume),
        }
    }

    fn to_text(&self) -> String {
        let name = |s: &str| s.to_ascii_lowercase();
        [
            format!("renderDistance:{}", self.render_distance),
            format!("ao:{}", SMOOTH.iter().position(|&s| s == self.smooth).unwrap_or(2)),
            format!("mipmapLevels:{}", self.mipmaps),
            format!("gamma:{}", self.gamma),
            format!("antialiasing:{}", name(&format!("{:?}", self.aa))),
            format!("lighting:{}", name(&format!("{:?}", self.mode))),
            format!("shadows:{}", self.shadows),
            format!("renderClouds:{}", if self.clouds == Clouds::Fancy { "true" } else { "false" }),
            format!("time:{}", name(&format!("{:?}", self.time))),
            format!("fov:{}", self.fov),
            format!("mouseSensitivity:{}", self.sensitivity),
            format!("bobView:{}", self.bob_view),
            format!("autoJump:{}", self.auto_jump),
            format!("lodDistance:{}", self.lod_distance),
            format!("shadowDistance:{}", self.shadow_distance),
            format!("soundCategory_master:{}", self.master_volume),
            format!("soundCategory_music:{}", self.music_volume),
            format!("soundCategory_block:{}", self.sound_volume),
        ]
        .join("\n")
            + "\n"
    }

    /// Reads `key:value` lines over the defaults; unknown keys and bad values are skipped.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let v = v.trim();
            let pick = |all: &[&str]| all.iter().position(|&n| n == v);
            match k.trim() {
                "renderDistance" => s.render_distance = v.parse().map_or(s.render_distance, |r: i32| r.max(2)),
                "ao" => s.smooth = v.parse::<usize>().ok().and_then(|i| SMOOTH.get(i).copied()).unwrap_or(s.smooth),
                "mipmapLevels" => s.mipmaps = v.parse().map_or(s.mipmaps, |m: u32| m.min(4)),
                "gamma" => s.gamma = v.parse().map_or(s.gamma, |g: f32| g.clamp(0.0, 1.0)),
                // A saved "taa" (an option older builds offered) falls back to FXAA.
                "antialiasing" => s.aa = pick(&["off", "fxaa", "smaa", "msaa"]).map_or(s.aa, |i| AA[i]),
                "lighting" => s.mode = pick(&["vanilla", "shaded"]).map_or(s.mode, |i| MODE[i]),
                "shadows" => s.shadows = v != "false",
                "renderClouds" => s.clouds = if v == "false" { Clouds::Off } else { Clouds::Fancy },
                "time" => s.time = pick(&["day", "noon", "sunset", "midnight", "cycle"]).map_or(s.time, |i| TIME[i]),
                "fov" => s.fov = v.parse().map_or(s.fov, |f: f32| f.clamp(30.0, 110.0)),
                "mouseSensitivity" => s.sensitivity = v.parse().map_or(s.sensitivity, |f: f32| f.clamp(0.0, 1.0)),
                "bobView" => s.bob_view = v != "false",
                "autoJump" => s.auto_jump = v != "false",
                // The largest choice not over the value.
                "lodDistance" => s.lod_distance = v.parse().map_or(s.lod_distance, |d: i32| LOD.iter().copied().filter(|&l| l <= d).last().unwrap_or(0)),
                "shadowDistance" => s.shadow_distance = v.parse().map_or(s.shadow_distance, |d: i32| SHADOW.iter().copied().filter(|&l| l <= d).last().unwrap_or(SHADOW[0])),
                "soundCategory_master" => s.master_volume = v.parse().map_or(s.master_volume, |f: f32| f.clamp(0.0, 1.0)),
                "soundCategory_music" => s.music_volume = v.parse().map_or(s.music_volume, |f: f32| f.clamp(0.0, 1.0)),
                "soundCategory_block" => s.sound_volume = v.parse().map_or(s.sound_volume, |f: f32| f.clamp(0.0, 1.0)),
                _ => {}
            }
        }
        s
    }
}

/// Volumes step by 10%, stopping at the ends.
fn volume_step(v: f32, dir: i32) -> f32 {
    ((v * 10.0).round() + dir as f32).clamp(0.0, 10.0) / 10.0
}

/// `GameSettings.getKeyBinding` for a sound slider: "OFF" at 0.
fn volume_label(name: &str, v: f32) -> String {
    match (v * 100.0).round() as i32 {
        0 => format!("{name}: OFF"),
        n => format!("{name}: {n}%"),
    }
}

/// Where the options live, or `None` when `APPDATA` is unset.
pub fn options_path() -> Option<PathBuf> {
    paths::data_dir().map(|d| d.join("options.txt"))
}

pub fn load() -> Settings {
    options_path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Settings::default, |t| Settings::parse(&t))
}

fn save(s: &Settings) {
    let Some(path) = options_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, s.to_text()) {
        warn!("could not save {}: {e}", path.display());
    }
}

/// Whether the menu is open, and whether changes go to disk (off for `--smoke`).
#[derive(Resource)]
pub struct Menu {
    pub open: bool,
    pub persist: bool,
    /// `--menu`: open it on the first frame, for a `--shot` of the menu.
    pub open_at_start: bool,
}

#[derive(Component)]
pub struct MenuRoot;

#[derive(Component)]
pub struct OptLabel(Opt);

#[derive(Component)]
pub struct DoneButton;

/// `GuiButton.drawButton` text colors: 14737632, and 16777120 under the pointer.
const TEXT: Color = Color::srgb_u8(0xe0, 0xe0, 0xe0);
const TEXT_HOVER: Color = Color::srgb_u8(0xff, 0xff, 0xa0);
/// `GuiOptions` spacing in GUI pixels: 150 x 20 buttons, columns 160 apart, rows 24 apart.
const OPT_W: u32 = 150;
const DONE_W: u32 = 200;
const BUTTON_H: u32 = 20;
const ROW: u32 = 24;

/// One half of a button's face: `drawTexturedModalRect` draws the left `width / 2` of the
/// `widgets.png` button row and the right `width / 2`, so any width up to 200 keeps both edges.
#[derive(Component)]
pub struct ButtonFace {
    right: bool,
    width: u32,
}

/// `GuiButton.getHoverState`: row 1 (v 66) normally, row 2 (v 86) under the pointer.
fn face_rect(face: &ButtonFace, hovered: bool) -> Rect {
    let v = 46.0 + if hovered { 40.0 } else { 20.0 };
    let half = (face.width / 2) as f32;
    let u = if face.right { 200.0 - half } else { 0.0 };
    Rect::new(u, v, u + half, v + BUTTON_H as f32)
}

/// The halves are pinned to the button's left and right edges, so a fractional scale cannot open
/// a gap between them; the label is the button's only flow child, centered by the button.
fn button(commands: &mut Commands, widgets: &GuiWidgets, label: impl Bundle, width: u32, unit: f32) -> Entity {
    let size = |n: u32| px(n as f32 * unit);
    let half = |right: bool| {
        let face = ButtonFace { right, width };
        let rect = face_rect(&face, false);
        let (left, right) = if right { (Val::Auto, px(0)) } else { (px(0), Val::Auto) };
        (
            face,
            ImageNode { rect: Some(rect), ..ImageNode::new(widgets.0.clone()) },
            Node {
                position_type: PositionType::Absolute,
                left,
                right,
                top: px(0),
                width: size(width / 2),
                height: size(BUTTON_H),
                ..default()
            },
        )
    };
    let label = commands.spawn(label).id();
    let b = commands
        .spawn((
            Button,
            Node {
                width: size(width),
                height: size(BUTTON_H),
                flex_shrink: 0.0,
                margin: UiRect::axes(size(5), size((ROW - BUTTON_H) / 2)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .id();
    let (l, r) = (commands.spawn(half(false)).id(), commands.spawn(half(true)).id());
    commands.entity(b).add_children(&[l, r, label]);
    b
}

/// `drawCenteredString` with its shadow: the color at a quarter, one GUI pixel down and right.
/// DROPPED: the jar's `ascii.png` font; the menu writes in Bevy's default font at the same height.
fn text(s: String, unit: f32) -> impl Bundle {
    (
        Text::new(s),
        TextFont { font_size: FontSize::Px(9.0 * unit), ..default() },
        TextColor(TEXT),
        TextShadow { offset: Vec2::splat(unit), color: Color::srgb_u8(0x38, 0x38, 0x38) },
    )
}

/// GUI pixels the menu needs top to bottom: the option rows, the Done row, title and hint.
const MENU_H: u32 = ((OPTS.len() as u32).div_ceil(2) + 1) * ROW + 50;

/// Physical pixels per GUI pixel for the menu: the game's GUI scale, stepped down to the largest
/// whole number that fits the menu in `height` physical pixels, and fractional only when not
/// even one does. A whole number keeps the `widgets.png` faces and the text on the pixel grid.
fn menu_scale(width: u32, height: u32) -> f32 {
    let k = crate::hand::gui_scale(width, height);
    match k.min(height / MENU_H) {
        0 => height as f32 / MENU_H as f32,
        fit => fit as f32,
    }
}

/// The menu in GUI pixels at [`menu_scale`], two columns as `GuiOptions` lays them out: option
/// `i` in column `i % 2`, so an odd last one sits on the left.
fn spawn_menu(commands: &mut Commands, widgets: &GuiWidgets, s: &Settings, window: &Window) {
    let unit = menu_scale(window.physical_width(), window.physical_height()) / window.scale_factor();
    let root = commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // `drawDefaultBackground` in game: `drawGradientRect` -1072689136 to -804253680.
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::auto(Color::srgba_u8(0x10, 0x10, 0x10, 0xc0)),
                ColorStop::auto(Color::srgba_u8(0x10, 0x10, 0x10, 0xd0)),
            ])),
            GlobalZIndex(10),
        ))
        .id();
    let title = commands
        .spawn((
            Text::new("Options"),
            TextFont { font_size: FontSize::Px(9.0 * unit), ..default() },
            TextColor(Color::WHITE),
            TextShadow { offset: Vec2::splat(unit), color: Color::srgb_u8(0x3f, 0x3f, 0x3f) },
            Node { margin: UiRect::bottom(px(12.0 * unit)), ..default() },
        ))
        .id();
    let grid = commands.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).id();
    for pair in OPTS.chunks(2) {
        let row = commands.spawn(Node { flex_direction: FlexDirection::Row, width: px(((OPT_W + 10) * 2) as f32 * unit), ..default() }).id();
        for &opt in pair {
            let b = button(commands, widgets, (OptLabel(opt), text(s.label(opt), unit)), OPT_W, unit);
            commands.entity(b).insert(opt);
            commands.entity(row).add_child(b);
        }
        commands.entity(grid).add_child(row);
    }
    let hint = commands
        .spawn((
            Text::new("left click or wheel up: next, right click or wheel down: back"),
            TextFont { font_size: FontSize::Px(7.0 * unit), ..default() },
            TextColor(Color::srgb_u8(0xa0, 0xa0, 0xa0)),
            Node { margin: UiRect::vertical(px(4.0 * unit)), ..default() },
        ))
        .id();
    let done = button(commands, widgets, text("Done".into(), unit), DONE_W, unit);
    commands.entity(done).insert(DoneButton);
    commands.entity(root).add_children(&[title, grid, hint, done]);
}

/// Esc and Done open and close the menu; [`crate::input::focus`] frees or grabs the mouse to
/// match. Losing the window while in game opens it, as `Minecraft.displayInGameMenu` does.
/// DROPPED: `pauseOnLostFocus`'s half-second grace; the menu opens on the frame focus goes.
#[allow(clippy::too_many_arguments)]
pub fn toggle_menu(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<Focus>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut menu: ResMut<Menu>,
    settings: Res<Settings>,
    widgets: Res<GuiWidgets>,
    roots: Query<Entity, With<MenuRoot>>,
    done: Query<&Interaction, (Changed<Interaction>, With<DoneButton>)>,
) {
    let clicked_done = done.iter().any(|i| *i == Interaction::Pressed);
    let lost_focus = focus.in_game && !window.focused && !menu.open;
    // Read before writing: a ResMut write marks Menu changed, and input::focus drops every
    // click on a frame where Menu changed, so taking the flag each frame ate all clicks.
    let at_start = menu.open_at_start;
    if at_start {
        menu.open_at_start = false;
    }
    if !keys.just_pressed(KeyCode::Escape) && !clicked_done && !lost_focus && !at_start {
        return;
    }
    menu.open = !menu.open;
    if menu.open {
        spawn_menu(&mut commands, &widgets, &settings, &window);
    } else {
        for e in &roots {
            commands.entity(e).despawn();
        }
    }
}

/// Clicks and the wheel over an option button step it; hover colors follow the pointer.
pub fn menu_input(
    menu: Res<Menu>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut settings: ResMut<Settings>,
    buttons: Query<(&Interaction, Option<&Opt>, &Children), With<Button>>,
    mut faces: Query<(&ButtonFace, &mut ImageNode)>,
    mut colors: Query<&mut TextColor>,
    mut wheel: Local<f32>,
) {
    if !menu.open {
        *wheel = 0.0;
        return;
    }
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    *wheel += lines;
    let notches = wheel.trunc() as i32;
    *wheel -= notches as f32;
    for (interaction, opt, children) in &buttons {
        let hovered = *interaction != Interaction::None;
        for &c in children {
            if let Ok((face, mut image)) = faces.get_mut(c) {
                let rect = Some(face_rect(face, hovered));
                if image.rect != rect {
                    image.rect = rect;
                }
            } else if let Ok(mut color) = colors.get_mut(c) {
                color.set_if_neq(TextColor(if hovered { TEXT_HOVER } else { TEXT }));
            }
        }
        let Some(&opt) = opt.filter(|_| hovered) else { continue };
        let mut dir = notches;
        if mouse.just_pressed(MouseButton::Left) {
            dir += 1;
        }
        if mouse.just_pressed(MouseButton::Right) {
            dir -= 1;
        }
        for _ in 0..dir.unsigned_abs() {
            settings.cycle(opt, dir.signum());
        }
    }
}

/// Rewrites the button labels and saves the file when the settings change.
pub fn settings_changed(menu: Res<Menu>, settings: Res<Settings>, mut labels: Query<(&OptLabel, &mut Text)>) {
    if !settings.is_changed() || settings.is_added() {
        return;
    }
    for (l, mut t) in &mut labels {
        t.0 = settings.label(l.0);
    }
    if menu.persist {
        save(&settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_scale_is_whole_and_fits() {
        assert_eq!(MENU_H, 290);
        assert_eq!(menu_scale(1280, 720), 2.0, "GUI scale 3 does not fit 720 rows");
        assert_eq!(menu_scale(1920, 1080), 3.0);
        assert_eq!(menu_scale(2560, 1440), 4.0);
        assert_eq!(menu_scale(854, 480), 1.0);
        assert_eq!(menu_scale(640, 145), 0.5, "fractional only below one whole step");
        for (w, h) in [(1280, 720), (1920, 1080), (1366, 768), (3840, 2160), (640, 145)] {
            assert!(menu_scale(w, h) * MENU_H as f32 <= h as f32, "{w}x{h}");
        }
    }

    #[test]
    fn text_round_trips() {
        let mut s = Settings::default();
        s.render_distance = 20;
        s.smooth = Smooth::Off;
        s.mipmaps = 2;
        s.gamma = 0.7;
        s.aa = Aa::Msaa;
        s.shadow_distance = 12;
        s.master_volume = 0.3;
        s.music_volume = 0.0;
        s.sound_volume = 0.7;
        s.mode = Mode::Shaded;
        s.shadows = false;
        s.clouds = Clouds::Off;
        s.time = TimeOfDay::Midnight;
        s.fov = 90.0;
        s.sensitivity = 0.35;
        s.bob_view = false;
        s.auto_jump = false;
        s.lod_distance = 96;
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse("lodDistance:100").lod_distance, 96);
        assert_eq!(Settings::parse("lodDistance:500").lod_distance, 384);
        assert_eq!(Settings::parse("lodDistance:9000").lod_distance, 512);
        assert_eq!(Settings::parse("lodDistance:-3").lod_distance, 0);
        assert_eq!(Settings::parse("renderDistance:99\nnonsense\nfov:abc"), Settings { render_distance: 99, ..Settings::default() });
        assert_eq!(Settings::parse("renderDistance:0").render_distance, 2);
    }

    #[test]
    fn cycling_wraps_choices_and_stops_numbers() {
        let mut s = Settings::default();
        s.cycle(Opt::Aa, -1);
        assert_eq!(s.aa, Aa::Off);
        s.cycle(Opt::Aa, -1);
        assert_eq!(s.aa, Aa::Msaa);
        assert_eq!(Settings::parse("antialiasing:taa").aa, Aa::Fxaa, "TAA is gone");
        s.music_volume = 0.1;
        s.cycle(Opt::MusicVolume, -1);
        assert_eq!(s.label(Opt::MusicVolume), "Music: OFF");
        s.cycle(Opt::MusicVolume, -1);
        assert_eq!(s.music_volume, 0.0);
        s.render_distance = 32;
        s.cycle(Opt::RenderDistance, 1);
        assert_eq!(s.render_distance, 33);
        s.render_distance = 2;
        s.cycle(Opt::RenderDistance, -1);
        assert_eq!(s.render_distance, 2);
        assert_eq!(s.label(Opt::Sensitivity), "Sensitivity: 100%");
        s.sensitivity = 0.95;
        s.cycle(Opt::Sensitivity, 1);
        assert_eq!(s.label(Opt::Sensitivity), "Sensitivity: HYPERSPEED!!!");
        for _ in 0..3 {
            s.cycle(Opt::Gamma, 1);
        }
        assert!((s.gamma - 0.3).abs() < 1e-6);
        assert_eq!(s.label(Opt::Gamma), "Brightness: +30%");
        assert_eq!(s.label(Opt::Fov), "FOV: Normal");
        // Distant terrain wraps through its choices and only counts past the render distance.
        let mut s = Settings::default();
        assert_eq!((s.lod_chunks(), s.view_chunks()), (64, 64));
        s.cycle(Opt::Lod, 2);
        assert_eq!(s.label(Opt::Lod), "Distant Terrain: 128 chunks");
        s.cycle(Opt::Lod, 4);
        assert_eq!(s.label(Opt::Lod), "Distant Terrain: 512 chunks");
        s.cycle(Opt::Lod, 1);
        assert_eq!(s.label(Opt::Lod), "Distant Terrain: OFF");
        s.lod_distance = 32;
        s.render_distance = 40;
        assert_eq!((s.lod_chunks(), s.view_chunks()), (0, 40));
    }
}
