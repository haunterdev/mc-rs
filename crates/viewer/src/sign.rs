//! Signs: `TileEntitySign` as the viewer keeps it, `TileEntitySignRenderer` drawing the board,
//! the stick and the four lines in the world, and `GuiEditSign`, the screen a placed sign opens.
//!
//! A sign's lines come from the server (`S_SIGN`, on join and after every edit) or from this
//! viewer's own edit screen. The screen opens when interact.rs places a sign
//! (`Controller::sign`); closing it (Done or Esc) sends the lines as `V_SIGN`
//! (`GuiEditSign.onGuiClosed`). Without a server the lines last for the session.
//! The screen stands in for `Minecraft.currentScreen` through the menu's `open` flag, as chat
//! does: the mouse is free and the keys type.
//! DROPPED: the unicode font pages; the edit screen types only what `ascii.png` draws.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use worldgen::blocks::id;

use crate::hand::{gui_scale, BoxMesh};
use crate::hotbar::GuiWidgets;
use crate::link::Link;
use crate::material::{EntityMaterial, NameplateMaterial, PlateKind, PlateParams};
use crate::players::{Font, Skin};
use crate::settings::{self, DoneButton, Menu};
use crate::sky::{neighbor_light, View};
use crate::stream::Streamer;
use crate::target::BlockPos;

/// `GuiEditSign.keyTyped`: a line may be 90 font pixels wide.
const LINE_WIDTH: i32 = 90;
/// `TileEntity.getMaxRenderDistanceSquared`: 64 blocks.
const RENDER_DISTANCE_SQ: f32 = 4096.0;

/// An `S_SIGN` from the server.
#[derive(Message, Clone, Debug)]
pub struct SignUpdate(pub proto::SignText);

/// `textures/entity/sign.png`, as the world draws it (the entity shader's space) and as the GUI
/// does (sRGB, like the other GUI images). Set up by main.rs.
#[derive(Resource)]
pub struct SignImages {
    pub world: Handle<Image>,
    pub gui: Handle<Image>,
}

/// One `TileEntitySign`, and the entities drawing it.
#[derive(Default)]
struct Tile {
    lines: [String; 4],
    /// The block state the entities were made for; 0 before.
    state: u16,
    /// The block was seen as a sign once, so a block that is no longer one takes the sign away.
    seen: bool,
    /// The lines changed since the text mesh was made.
    dirty: bool,
    board: Option<Entity>,
    text: Option<Entity>,
    material: Option<Handle<EntityMaterial>>,
}

/// `GuiEditSign`'s state.
struct Editing {
    pos: BlockPos,
    edit_line: usize,
    /// When the screen opened, for `updateCounter` (one a tick).
    opened: f32,
    /// What the GUI was last drawn for: the window and the sign's look.
    drawn: Option<(u32, u32, f32, u16, [String; 4], Option<usize>)>,
    root: Entity,
    sign: Option<Entity>,
}

#[derive(Resource, Default)]
pub struct Signs {
    tiles: HashMap<BlockPos, Tile>,
    editing: Option<Editing>,
}

/// The meshes and the text material every sign shares, made on first use.
#[derive(Resource)]
struct SignMeshes {
    standing: Handle<Mesh>,
    wall: Handle<Mesh>,
    text: Handle<NameplateMaterial>,
}

pub struct SignPlugin;

impl Plugin for SignPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Signs>()
            .add_message::<SignUpdate>()
            .add_systems(Update, (receive, keys.after(settings::toggle_menu).before(crate::input::focus), draw_gui.after(keys)))
            .add_systems(Update, render.after(crate::player::run_ticks));
    }
}

fn is_sign(s: u16) -> bool {
    matches!(s >> 4, id::STANDING_SIGN | id::WALL_SIGN)
}

/// `ModelSign`: the board (24 x 12 x 2 at texture 0, 0) and the stick (2 x 14 x 2 at 0, 14) on a
/// 64 x 32 texture, under `TileEntitySignRenderer`'s `scale(0.6666667, -0.6666667, -0.6666667)`
/// and `ModelRenderer.render(0.0625)`, baked into the vertices.
fn sign_mesh(stick: bool) -> Mesh {
    let mut b = BoxMesh::default();
    b.model_box_on(64.0, 32.0, 0, 0, -12.0, -14.0, -1.0, 24, 12, 2, 0.0);
    if stick {
        b.model_box_on(64.0, 32.0, 0, 14, -1.0, -2.0, -1.0, 2, 14, 2, 0.0);
    }
    let k = 0.0625 * 0.6666667;
    let pos: Vec<[f32; 3]> = b.pos.iter().map(|p| [p[0] * k, -p[1] * k, -p[2] * k]).collect();
    let normal: Vec<[f32; 3]> = b.normal.iter().map(|n| [n[0], -n[1], -n[2]]).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
        .with_inserted_indices(Indices::U32(b.idx))
}

/// The glyph quads `FontRenderer.renderDefaultChar` draws for `s` from (`x`, `y`) in font pixels,
/// y down: each `charWidth - 1.01` wide and 7.99 tall, the next one `charWidth` along.
fn glyphs(font: &Font, s: &str, mut x: f32, y: f32, out: &mut Vec<([f32; 4], [f32; 4])>) {
    for b in s.bytes() {
        let w = font.widths[b as usize] as f32;
        let f = w - 0.01;
        let (gx, gy) = ((b % 16) as f32 * 8.0, (b / 16) as f32 * 8.0);
        out.push(([x, y, x + f - 1.0, y + 7.99], [gx, gy, gx + f - 1.0, gy + 7.99]));
        x += w;
    }
}

/// `GuiUtilRenderComponents.splitText(line, 90)`'s first line.
/// DROPPED: the word wrap; a line the edit screen made always fits, and a longer one is cut at
/// the last character that fits.
fn fitted(font: &Font, s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        out.push(c);
        if font.string_width(&out) > LINE_WIDTH {
            out.pop();
            break;
        }
    }
    out
}

/// The line as drawn: `"> " + s + " <"` while it is the line being edited.
fn shown(line: &str, edited: bool) -> String {
    if edited {
        format!("> {line} <")
    } else {
        line.to_string()
    }
}

/// The four lines' glyph quads, each centered (`-getStringWidth(s) / 2`) at `j * 10 - 20`.
fn line_quads(font: &Font, lines: &[String; 4], edited: Option<usize>) -> Vec<([f32; 4], [f32; 4])> {
    let mut quads = Vec::new();
    for (j, line) in lines.iter().enumerate() {
        let s = shown(&fitted(font, line), edited == Some(j));
        let x = -font.string_width(&s) / 2;
        glyphs(font, &s, x as f32, (j as i32 * 10 - 20) as f32, &mut quads);
    }
    quads
}

/// The lines as a mesh in font pixels with y up (the renderer's `scale(f, -f, f)` flip baked in),
/// facing +z.
fn text_mesh(font: &Font, lines: &[String; 4]) -> Mesh {
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for ([x0, y0, x1, y1], [u0, v0, u1, v1]) in line_quads(font, lines, None) {
        let base = pos.len() as u32;
        pos.extend_from_slice(&[[x0, -y0, 0.0], [x0, -y1, 0.0], [x1, -y1, 0.0], [x1, -y0, 0.0]]);
        uv.extend_from_slice(&[[u0 / 128.0, v0 / 128.0], [u0 / 128.0, v1 / 128.0], [u1 / 128.0, v1 / 128.0], [u1 / 128.0, v0 / 128.0]]);
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let n = vec![[0.0, 0.0, 1.0]; pos.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

/// `TileEntitySignRenderer.render`'s matrix for the board: a standing sign turned by its
/// rotation, a wall sign by its facing and pushed back against the wall.
fn board_matrix(pos: BlockPos, s: u16) -> Mat4 {
    let center = Mat4::from_translation(Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5));
    let meta = s & 15;
    if s >> 4 == id::STANDING_SIGN {
        let f1 = (meta * 360) as f32 / 16.0;
        center * Mat4::from_rotation_y((-f1).to_radians())
    } else {
        let f2: f32 = match meta {
            2 => 180.0,
            4 => 90.0,
            5 => -90.0,
            _ => 0.0,
        };
        center * Mat4::from_rotation_y((-f2).to_radians()) * Mat4::from_translation(Vec3::new(0.0, -0.3125, -0.4375))
    }
}

/// The text's matrix: `translate(0, 0.33333334, 0.046666667)` and `scale(0.010416667)` from the
/// board's (the y flip is in the mesh).
fn text_matrix(board: Mat4) -> Mat4 {
    board * Mat4::from_translation(Vec3::new(0.0, 0.33333334, 0.046666667)) * Mat4::from_scale(Vec3::splat(0.010416667))
}

/// The server's lines into the signs.
fn receive(mut updates: MessageReader<SignUpdate>, mut signs: ResMut<Signs>) {
    for SignUpdate(m) in updates.read() {
        let tile = signs.tiles.entry((m.pos[0], m.pos[1], m.pos[2])).or_default();
        tile.lines = m.lines.clone();
        tile.dirty = true;
    }
}

/// `ChatAllowedCharacters.isAllowedCharacter`, kept to what `ascii.png` draws.
fn allowed(c: char) -> bool {
    (' '..='~').contains(&c)
}

/// The character a key types on a US layout, for a streamed session, whose browser sends keys
/// and no text.
pub(crate) fn typed_char(key: KeyCode, shift: bool) -> Option<char> {
    let letters = "abcdefghijklmnopqrstuvwxyz";
    let letter_keys = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    if let Some(i) = letter_keys.iter().position(|&k| k == key) {
        let c = letters.as_bytes()[i] as char;
        return Some(if shift { c.to_ascii_uppercase() } else { c });
    }
    let digit_keys = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    if let Some(i) = digit_keys.iter().position(|&k| k == key) {
        return Some(if shift { ")!@#$%^&*(".as_bytes()[i] } else { b'0' + i as u8 } as char);
    }
    let (plain, shifted) = match key {
        KeyCode::Space => (' ', ' '),
        KeyCode::Minus => ('-', '_'),
        KeyCode::Equal => ('=', '+'),
        KeyCode::BracketLeft => ('[', '{'),
        KeyCode::BracketRight => (']', '}'),
        KeyCode::Backslash => ('\\', '|'),
        KeyCode::Semicolon => (';', ':'),
        KeyCode::Quote => ('\'', '"'),
        KeyCode::Comma => (',', '<'),
        KeyCode::Period => ('.', '>'),
        KeyCode::Slash => ('/', '?'),
        KeyCode::Backquote => ('`', '~'),
        _ => return None,
    };
    Some(if shift { shifted } else { plain })
}

/// One key on the edit screen: `GuiEditSign.keyTyped`. Esc is the menu toggle's, which closes the
/// screen as Done does.
fn key_typed(font: &Font, line: &mut usize, lines: &mut [String; 4], key: KeyCode, typed: Option<char>) {
    if key == KeyCode::ArrowUp {
        *line = (*line + 3) & 3;
    }
    if matches!(key, KeyCode::ArrowDown | KeyCode::Enter | KeyCode::NumpadEnter) {
        *line = (*line + 1) & 3;
    }
    let s = &mut lines[*line];
    if key == KeyCode::Backspace && !s.is_empty() {
        s.pop();
    }
    if let Some(c) = typed.filter(|&c| allowed(c)) {
        let mut t = s.clone();
        t.push(c);
        if font.string_width(&t) <= LINE_WIDTH {
            *s = t;
        }
    }
}

/// Opens the edit screen for a sign just placed, types into it, and closes it when the menu flag
/// drops (Done or Esc): `onGuiClosed` sends the lines.
#[allow(clippy::too_many_arguments)]
fn keys(
    mut commands: Commands,
    mut signs: ResMut<Signs>,
    mut ctl: ResMut<crate::interact::Controller>,
    mut menu: ResMut<Menu>,
    mut typed: MessageReader<KeyboardInput>,
    mut pressed: ResMut<ButtonInput<KeyCode>>,
    time: Res<Time>,
    font: Option<Res<Font>>,
    link: Option<Res<Link>>,
) {
    let signs = &mut *signs;
    if let Some(pos) = ctl.sign.take() {
        if !menu.open && signs.editing.is_none() {
            let tile = signs.tiles.entry(pos).or_default();
            tile.lines = Default::default();
            tile.dirty = true;
            let root = commands.spawn((Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }, GlobalZIndex(10))).id();
            signs.editing = Some(Editing { pos, edit_line: 0, opened: time.elapsed_secs(), drawn: None, root, sign: None });
            menu.open = true;
            typed.clear();
            return;
        }
    }
    let Some(editing) = signs.editing.as_mut() else { return };
    if !menu.open {
        let lines = signs.tiles.get(&editing.pos).map(|t| t.lines.clone()).unwrap_or_default();
        if let Some(link) = link.as_deref() {
            let p = editing.pos;
            link.send(proto::V_SIGN, &proto::SignText { pos: [p.0, p.1, p.2], lines }.encode());
        }
        commands.entity(editing.root).despawn();
        signs.editing = None;
        return;
    }
    let shift = link.as_deref().is_some_and(|l| l.shift_held()) || pressed.pressed(KeyCode::ShiftLeft) || pressed.pressed(KeyCode::ShiftRight);
    if let (Some(font), Some(tile)) = (font.as_deref(), signs.tiles.get_mut(&editing.pos)) {
        for ev in typed.read() {
            if ev.state != ButtonState::Pressed {
                continue;
            }
            let c = match &ev.text {
                Some(t) => t.chars().next(),
                None => typed_char(ev.key_code, shift),
            };
            key_typed(font, &mut editing.edit_line, &mut tile.lines, ev.key_code, c);
            tile.dirty = true;
        }
    }
    // Typed letters are not key bindings.
    pressed.clear();
}

/// `GuiEditSign.drawScreen` in GUI pixels: the darkened screen, the title, the Done button, and
/// the sign as `TileEntityRendererDispatcher` draws it there, facing the screen at 93.75 GUI
/// pixels a block. Redrawn when the window, the lines or the blinking marker change.
#[allow(clippy::too_many_arguments)]
fn draw_gui(
    mut commands: Commands,
    mut signs: ResMut<Signs>,
    window: Single<&Window, With<PrimaryWindow>>,
    streamer: Res<Streamer>,
    time: Res<Time>,
    font: Option<Res<Font>>,
    images: Option<Res<SignImages>>,
    widgets: Option<Res<GuiWidgets>>,
) {
    let signs = &mut *signs;
    let Some(editing) = signs.editing.as_mut() else { return };
    let (Some(font), Some(images), Some(widgets)) = (font, images, widgets) else { return };
    let lines = signs.tiles.get(&editing.pos).map(|t| t.lines.clone()).unwrap_or_default();
    let state = streamer.state_at(editing.pos.0, editing.pos.1, editing.pos.2).filter(|&s| is_sign(s)).unwrap_or(id::STANDING_SIGN << 4);
    let (pw, ph) = (window.physical_width(), window.physical_height());
    let k = gui_scale(pw, ph);
    let unit = k as f32 / window.scale_factor();
    // ScaledResolution: the GUI size rounded up.
    let (w, h) = (pw.div_ceil(k), ph.div_ceil(k));
    // updateScreen counts ticks; the marker shows while updateCounter / 6 is even.
    let counter = ((time.elapsed_secs() - editing.opened) * 20.0) as i32;
    let marker = (counter / 6 % 2 == 0).then_some(editing.edit_line);
    let key = (w, h, unit, state, lines.clone(), marker);
    let first = editing.drawn.is_none();
    let resized = editing.drawn.as_ref().is_some_and(|d| (d.0, d.1, d.2) != (w, h, unit));
    if editing.drawn.as_ref() == Some(&key) {
        return;
    }
    editing.drawn = Some(key);
    let gx = |x: f32| px(x * unit);
    let half_w = (w / 2) as f32;
    if first || resized {
        // The fixed part: background, title and Done, made again only for a new window size, so
        // the button keeps its pointer state.
        commands.entity(editing.root).despawn_children();
        let bg = commands
            .spawn((
                Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
                // drawDefaultBackground in game.
                BackgroundGradient::from(LinearGradient::to_bottom(vec![
                    ColorStop::auto(Color::srgba_u8(0x10, 0x10, 0x10, 0xc0)),
                    ColorStop::auto(Color::srgba_u8(0x10, 0x10, 0x10, 0xd0)),
                ])),
            ))
            .id();
        // drawCenteredString(I18n "sign.edit", width / 2, 40, white).
        let title = commands
            .spawn((
                Node { position_type: PositionType::Absolute, top: gx(40.0), width: percent(100), justify_content: JustifyContent::Center, ..default() },
                children![(
                    Text::new("Edit sign message"),
                    TextFont { font_size: FontSize::Px(9.0 * unit), ..default() },
                    TextColor(Color::WHITE),
                    TextShadow { offset: Vec2::splat(unit), color: Color::srgb_u8(0x3f, 0x3f, 0x3f) },
                )],
            ))
            .id();
        // The Done button at (width / 2 - 100, height / 4 + 120), 200 x 20; settings.rs's button
        // carries a 5 by 2 margin.
        let done = settings::button(&mut commands, &widgets, settings::text("Done".into(), unit), 200, unit);
        commands.entity(done).insert(DoneButton);
        let at = commands
            .spawn(Node { position_type: PositionType::Absolute, left: gx(half_w - 105.0), top: gx((h / 4) as f32 + 118.0), ..default() })
            .add_child(done)
            .id();
        commands.entity(editing.root).add_children(&[bg, title, at]);
        editing.sign = None;
    }
    if let Some(e) = editing.sign.take() {
        commands.entity(e).despawn();
    }
    // The sign: GuiEditSign's translate(width / 2, 0) scale(-93.75) rotate(180) and the
    // renderer's own turn cancel to a board facing the screen, its center 1.3125 blocks down
    // (1.625 for a wall sign).
    let k = 93.75;
    let drop = if state >> 4 == id::STANDING_SIGN { 0.0 } else { 0.3125 * k };
    let sign = commands.spawn(Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }).id();
    let mut parts = Vec::new();
    let image = |commands: &mut Commands, img: &Handle<Image>, rect: Rect, color: Color, x: f32, y: f32, w: f32, h: f32| {
        commands
            .spawn((
                ImageNode { rect: Some(rect), color, ..ImageNode::new(img.clone()) },
                Node { position_type: PositionType::Absolute, left: gx(x), top: gx(y), width: gx(w), height: gx(h), ..default() },
            ))
            .id()
    };
    // The board's front, texels (2, 2) to (26, 14), from y 0.5833 to 0.0833 of the model.
    let board_top = (1.3125 - 0.5833333) * k + drop;
    parts.push(image(&mut commands, &images.gui, Rect::new(2.0, 2.0, 26.0, 14.0), Color::WHITE, half_w - 0.5 * k, board_top, k, 0.5 * k));
    if state >> 4 == id::STANDING_SIGN {
        // The stick's front, texels (2, 16) to (4, 30), below the board.
        let stick_w = 2.0 * 0.0625 * 0.6666667 * k;
        parts.push(image(&mut commands, &images.gui, Rect::new(2.0, 16.0, 4.0, 30.0), Color::WHITE, half_w - stick_w / 2.0, board_top + 0.5 * k, stick_w, 14.0 * 0.0625 * 0.6666667 * k));
    }
    // The lines: the text origin 0.33333334 above the board's center, 0.010416667 a font pixel.
    let (fx, fy) = (0.010416667 * k, (1.3125 - 0.33333334) * k + drop);
    for ([x0, y0, x1, y1], [u0, v0, u1, v1]) in line_quads(&font, &lines, marker) {
        parts.push(image(&mut commands, &font.image, Rect::new(u0, v0, u1, v1), Color::BLACK, half_w + x0 * fx, fy + y0 * fx, (x1 - x0) * fx, (y1 - y0) * fx));
    }
    commands.entity(sign).add_children(&parts);
    commands.entity(editing.root).add_child(sign);
    editing.sign = Some(sign);
}

/// `TileEntitySignRenderer` for every sign: the board (and stick) under the block's light, and
/// the lines in black, within 64 blocks. A sign whose block is gone goes with it.
#[allow(clippy::too_many_arguments)]
fn render(
    mut commands: Commands,
    mut signs: ResMut<Signs>,
    have: Option<Res<SignMeshes>>,
    streamer: Res<Streamer>,
    view: Res<View>,
    skin: Option<Res<Skin>>,
    font: Option<Res<Font>>,
    images: Option<Res<SignImages>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut plates: ResMut<Assets<NameplateMaterial>>,
    mut parts: Query<(&mut Transform, &mut Visibility)>,
    mut mesh_handles: Query<&mut Mesh3d>,
) {
    if signs.tiles.is_empty() {
        return;
    }
    let (Some(skin), Some(font), Some(images)) = (skin, font, images) else { return };
    let shared = match have {
        Some(m) => (m.standing.clone(), m.wall.clone(), m.text.clone()),
        None => {
            let m = SignMeshes {
                standing: meshes.add(sign_mesh(true)),
                wall: meshes.add(sign_mesh(false)),
                // drawString(..., 0): opaque black.
                text: plates.add(NameplateMaterial {
                    font: font.image.clone(),
                    params: PlateParams { color: Vec4::new(0.0, 0.0, 0.0, 1.0), mode: Vec4::ZERO },
                    kind: PlateKind::Text,
                }),
            };
            let out = (m.standing.clone(), m.wall.clone(), m.text.clone());
            commands.insert_resource(m);
            out
        }
    };
    let mut gone = Vec::new();
    for (&pos, tile) in signs.tiles.iter_mut() {
        let state = streamer.state_at(pos.0, pos.1, pos.2);
        let Some(s) = state.filter(|&s| is_sign(s)) else {
            if state.is_some() && tile.seen {
                gone.push(pos);
            }
            for e in [tile.board, tile.text].into_iter().flatten() {
                if let Ok((_, mut v)) = parts.get_mut(e) {
                    *v = Visibility::Hidden;
                }
            }
            continue;
        };
        tile.seen = true;
        let board = *tile.board.get_or_insert_with(|| {
            let material = materials.add(EntityMaterial { env: skin.env.clone(), skin: images.world.clone(), light: Vec4::new(0.0, 240.0, 0.0, 0.0) });
            tile.material = Some(material.clone());
            commands.spawn((Mesh3d(shared.0.clone()), MeshMaterial3d(material), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster)).id()
        });
        let text = *tile.text.get_or_insert_with(|| {
            tile.dirty = true;
            commands.spawn((Mesh3d::default(), MeshMaterial3d(shared.2.clone()), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster)).id()
        });
        if tile.dirty {
            tile.dirty = false;
            let mesh = meshes.add(text_mesh(&font, &tile.lines));
            match mesh_handles.get_mut(text) {
                Ok(mut h) => h.0 = mesh,
                Err(_) => {
                    commands.entity(text).insert(Mesh3d(mesh));
                }
            }
        }
        if tile.state != s {
            tile.state = s;
            let mesh = if s >> 4 == id::STANDING_SIGN { shared.0.clone() } else { shared.1.clone() };
            match mesh_handles.get_mut(board) {
                Ok(mut h) => h.0 = mesh,
                Err(_) => {
                    commands.entity(board).insert(Mesh3d(mesh));
                }
            }
        }
        let center = Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
        let near = center.distance_squared(view.feet) < RENDER_DISTANCE_SQ;
        let m = board_matrix(pos, s);
        for (e, matrix) in [(board, m), (text, text_matrix(m))] {
            if let Ok((mut t, mut v)) = parts.get_mut(e) {
                let want = Transform::from_matrix(matrix);
                if *t != want {
                    *t = want;
                }
                let vis = if near { Visibility::Inherited } else { Visibility::Hidden };
                if *v != vis {
                    *v = vis;
                }
            }
        }
        // TileEntityRendererDispatcher.render: the lightmap at the sign, getCombinedLight(pos, 0).
        let (sky, block) = neighbor_light(&streamer, pos.0, pos.1, pos.2);
        let light = Vec4::new(block as f32 * 16.0, sky as f32 * 16.0, 0.0, 0.0);
        if let Some(h) = &tile.material {
            if materials.get(h).is_some_and(|m| m.light != light) {
                if let Some(mut m) = materials.get_mut(h) {
                    m.light = light;
                }
            }
        }
    }
    for pos in gone {
        if signs.editing.as_ref().is_some_and(|e| e.pos == pos) {
            continue;
        }
        if let Some(tile) = signs.tiles.remove(&pos) {
            for e in [tile.board, tile.text].into_iter().flatten() {
                commands.entity(e).despawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Font {
        // Every glyph 6 wide but the space, as most of ascii.png is.
        let mut widths = [6u8; 256];
        widths[32] = 4;
        Font { image: Handle::default(), widths }
    }

    #[test]
    fn the_edit_screen_types_moves_and_stops_at_90_pixels() {
        let f = font();
        let (mut line, mut lines) = (0, <[String; 4]>::default());
        for c in "Hi there".chars() {
            key_typed(&f, &mut line, &mut lines, KeyCode::KeyA, Some(c));
        }
        assert_eq!(lines[0], "Hi there");
        key_typed(&f, &mut line, &mut lines, KeyCode::Backspace, None);
        assert_eq!(lines[0], "Hi ther");
        // Enter and Down go to the next line, Up to the one before, all wrapping.
        key_typed(&f, &mut line, &mut lines, KeyCode::Enter, Some('\r'));
        assert_eq!((line, lines[1].as_str()), (1, ""));
        key_typed(&f, &mut line, &mut lines, KeyCode::ArrowUp, None);
        key_typed(&f, &mut line, &mut lines, KeyCode::ArrowUp, None);
        assert_eq!(line, 3);
        key_typed(&f, &mut line, &mut lines, KeyCode::ArrowDown, None);
        assert_eq!(line, 0);
        // 15 glyphs are 90 pixels; the 16th does not fit. The section sign never types.
        let (mut line, mut lines) = (2, <[String; 4]>::default());
        for _ in 0..20 {
            key_typed(&f, &mut line, &mut lines, KeyCode::KeyX, Some('x'));
        }
        key_typed(&f, &mut line, &mut lines, KeyCode::KeyX, Some('\u{a7}'));
        assert_eq!(lines[2], "x".repeat(15));
    }

    #[test]
    fn a_streamed_key_types_its_character() {
        assert_eq!(typed_char(KeyCode::KeyQ, false), Some('q'));
        assert_eq!(typed_char(KeyCode::KeyQ, true), Some('Q'));
        assert_eq!(typed_char(KeyCode::Digit1, true), Some('!'));
        assert_eq!(typed_char(KeyCode::Digit7, false), Some('7'));
        assert_eq!(typed_char(KeyCode::Slash, true), Some('?'));
        assert_eq!(typed_char(KeyCode::ArrowUp, false), None);
    }

    #[test]
    fn lines_center_and_the_edited_one_is_marked() {
        let f = font();
        let lines = ["ab".to_string(), String::new(), String::new(), String::new()];
        let quads = line_quads(&f, &lines, None);
        // "ab" is 12 wide: from -6, on line 0 at y -20.
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].0[0], -6.0);
        assert_eq!(quads[0].0[1], -20.0);
        // "> ab <" on line 0, and "> " + " <" on the empty line 2.
        let marked = line_quads(&f, &lines, Some(0));
        assert_eq!(marked.len(), 6);
        assert_eq!(line_quads(&f, &lines, Some(2)).len(), 2 + 4);
    }

    #[test]
    fn a_wall_sign_hangs_against_the_block_behind_it() {
        // Facing north (meta 2): the board's center sits 0.4375 into the north side of its cell,
        // that is toward +z, against the block it hangs on.
        let m = board_matrix((0, 64, 0), id::WALL_SIGN << 4 | 2);
        let c = m.transform_point3(Vec3::ZERO);
        assert!((c - Vec3::new(0.5, 64.1875, 0.9375)).length() < 1e-5, "{c}");
        // A standing sign at rotation 0 faces south (+z): its front normal is +z.
        let m = board_matrix((0, 64, 0), id::STANDING_SIGN << 4);
        assert!((m.transform_vector3(Vec3::Z) - Vec3::Z).length() < 1e-5);
    }
}
