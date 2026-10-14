//! Chat in a native session (join.rs), as `GuiNewChat` and `GuiChat` show it, in Bevy's UI
//! font: T opens the line (`/` opens it with the slash), Enter sends it, Esc drops it. While the
//! line is open the keys type instead of playing, as with any screen: the menu's `open` flag
//! stands in for `Minecraft.currentScreen`, which frees the mouse and stops the movement keys.
//! Lines show over the hotbar for 200 ticks, fading in the last 20 (`GuiNewChat.drawChat`);
//! while the line is open the last [`ROWS`] all show.
//!
//! What it shows is the server page's JSON: `chat` messages, from a player (`<name> text`) or
//! from the server (joins and leaves in yellow, command answers in the color they name).
//! DROPPED: the chat history (up and down), scrolling, the player list (Tab), clickable text.

use std::collections::VecDeque;
use std::time::Instant;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::hand::{gui_scale, HideGui};
use crate::join::Joined;
use crate::link::Link;
use crate::settings::Menu;

/// Lines shown at most.
pub const ROWS: usize = 10;
/// `GuiNewChat`: a line shows for 200 ticks.
const SHOW_TICKS: f32 = 200.0;
/// What a line may hold, as the server cuts it.
const MAX_LINE: usize = 256;
const WHITE: Color = Color::WHITE;
const YELLOW: Color = Color::srgb(1.0, 1.0, 0.33);

#[derive(Resource, Default)]
pub struct Chat {
    pub open: bool,
    pub line: String,
    lines: VecDeque<(String, Color, Instant)>,
}

impl Chat {
    pub fn push(&mut self, text: String, color: Color) {
        self.lines.push_back((text, color, Instant::now()));
        while self.lines.len() > 100 {
            self.lines.pop_front();
        }
    }
}

/// One of the shown lines, counted from the top.
#[derive(Component)]
pub struct ChatRow(usize);

#[derive(Component)]
pub struct ChatInput;

#[derive(Component)]
pub struct ChatBox;

/// `GuiNewChat.drawChat`: full for 180 ticks, then fading out over 20, squared.
pub fn opacity(age_ticks: f32, open: bool) -> f32 {
    if open {
        return 1.0;
    }
    let d = ((1.0 - age_ticks / SHOW_TICKS) * 10.0).clamp(0.0, 1.0);
    d * d
}

/// A line of the page's JSON as chat shows it: the text and its color; `None` for what chat
/// does not show.
pub fn line_of(json: &str) -> Option<(String, Color)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("t")?.as_str()? != "chat" {
        return None;
    }
    let text = v.get("text")?.as_str()?.to_string();
    match v.get("from").and_then(|f| f.as_str()).filter(|f| !f.is_empty()) {
        Some(from) => Some((format!("<{from}> {text}"), WHITE)),
        None => Some((text, v.get("color").and_then(|c| c.as_str()).and_then(formatting).unwrap_or(YELLOW))),
    }
}

/// `TextFormatting`'s color names and their `FontRenderer` colors.
fn formatting(name: &str) -> Option<Color> {
    let rgb = match name {
        "black" => 0x000000,
        "dark_blue" => 0x0000AA,
        "dark_green" => 0x00AA00,
        "dark_aqua" => 0x00AAAA,
        "dark_red" => 0xAA0000,
        "dark_purple" => 0xAA00AA,
        "gold" => 0xFFAA00,
        "gray" => 0xAAAAAA,
        "dark_gray" => 0x555555,
        "blue" => 0x5555FF,
        "green" => 0x55FF55,
        "aqua" => 0x55FFFF,
        "red" => 0xFF5555,
        "light_purple" => 0xFF55FF,
        "yellow" => 0xFFFF55,
        "white" => 0xFFFFFF,
        _ => return None,
    };
    Some(Color::srgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
}

pub fn spawn_chat(mut commands: Commands) {
    let rows: Vec<Entity> = (0..ROWS)
        .map(|i| {
            commands
                .spawn((ChatRow(i), Text::new(""), TextFont::default(), TextColor(WHITE), BackgroundColor(Color::NONE), Node::default(), Visibility::Hidden))
                .id()
        })
        .collect();
    commands
        .spawn((ChatBox, Node { position_type: PositionType::Absolute, flex_direction: FlexDirection::Column, ..default() }, GlobalZIndex(5)))
        .add_children(&rows);
    commands.spawn((
        ChatInput,
        Text::new(""),
        TextFont::default(),
        TextColor(WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        Node { position_type: PositionType::Absolute, ..default() },
        GlobalZIndex(5),
        Visibility::Hidden,
    ));
}

/// The server's chat into the lines.
pub fn receive(joined: Res<Joined>, mut chat: ResMut<Chat>) {
    while let Ok(json) = joined.texts.try_recv() {
        if let Some((text, color)) = line_of(&json) {
            chat.push(text, color);
        }
    }
}

/// T, `/` and Enter open the line, typing fills it, Enter sends it; Esc, which the menu's own
/// toggle reads first, drops it. Runs after `settings::toggle_menu` and before `input::focus`, so
/// the mouse is freed and grabbed as for a screen.
pub fn keys(
    mut chat: ResMut<Chat>,
    mut menu: ResMut<Menu>,
    mut typed: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    link: Res<Link>,
) {
    if chat.open && !menu.open {
        chat.open = false;
        chat.line.clear();
    }
    for ev in typed.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if !chat.open {
            if menu.open {
                continue;
            }
            match ev.key_code {
                KeyCode::KeyT | KeyCode::Enter => chat.line.clear(),
                KeyCode::Slash => chat.line = "/".into(),
                _ => continue,
            }
            chat.open = true;
            menu.open = true;
            // The key that opened the line types nothing.
            continue;
        }
        match &ev.logical_key {
            Key::Enter => {
                let text = chat.line.trim().to_string();
                if !text.is_empty() {
                    link.send(proto::V_TEXT, serde_json::json!({"t": "chat", "text": text}).to_string().as_bytes());
                }
                chat.open = false;
                chat.line.clear();
                menu.open = false;
            }
            Key::Backspace => {
                chat.line.pop();
            }
            _ => {
                for c in ev.text.iter().flat_map(|t| t.chars()).filter(|c| !c.is_control()) {
                    if chat.line.chars().count() < MAX_LINE {
                        chat.line.push(c);
                    }
                }
            }
        }
    }
    if chat.open {
        // Typed letters are not key bindings (H for the stats, F5 for the view).
        keys.clear();
    }
}

/// Places the lines at the GUI scale and fades them.
#[allow(clippy::type_complexity)]
pub fn draw(
    chat: Res<Chat>,
    hide: Res<HideGui>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut chat_box: Single<&mut Node, (With<ChatBox>, Without<ChatRow>, Without<ChatInput>)>,
    mut rows: Query<(&ChatRow, &mut Text, &mut TextFont, &mut TextColor, &mut BackgroundColor, &mut Node, &mut Visibility), Without<ChatInput>>,
    input: Single<(&mut Text, &mut TextFont, &mut Node, &mut Visibility), (With<ChatInput>, Without<ChatRow>)>,
) {
    let k = gui_scale(window.physical_width(), window.physical_height());
    let unit = k as f32 / window.scale_factor();
    // GuiIngame draws the chat 48 GUI pixels up, 320 wide (`chatWidth`), 9 a line.
    chat_box.left = px(2.0 * unit);
    chat_box.bottom = px(48.0 * unit);
    chat_box.width = px(320.0 * unit);
    let now = Instant::now();
    let shown: Vec<_> = chat
        .lines
        .iter()
        .rev()
        .take(ROWS)
        .map(|(text, color, at)| (text, *color, opacity(now.duration_since(*at).as_secs_f32() * 20.0, chat.open)))
        .filter(|&(_, _, a)| a > 0.0)
        .collect();
    for (row, mut text, mut font, mut color, mut bg, mut node, mut vis) in &mut rows {
        // Row 0 is the top; the newest line sits at the bottom.
        let slot = (ROWS - 1 - row.0, shown.len());
        let line = (slot.0 < slot.1).then(|| shown[slot.0]).filter(|_| !hide.0);
        let want = if line.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let Some((s, c, a)) = line else { continue };
        if text.0 != *s {
            text.0 = s.clone();
        }
        font.font_size = FontSize::Px(8.0 * unit);
        color.0 = c.with_alpha(a);
        bg.0 = Color::srgba(0.0, 0.0, 0.0, a * 0.5);
        node.padding = UiRect::horizontal(px(unit));
        node.min_height = px(9.0 * unit);
    }
    let (mut text, mut font, mut node, mut vis) = input.into_inner();
    let want = if chat.open { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    if chat.open {
        let line = format!("{}_", chat.line);
        if text.0 != line {
            text.0 = line;
        }
        font.font_size = FontSize::Px(8.0 * unit);
        node.left = px(2.0 * unit);
        node.bottom = px(2.0 * unit);
        node.width = px(window.width() - 4.0 * unit);
        node.padding = UiRect::all(px(2.0 * unit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_json_becomes_lines() {
        assert_eq!(line_of(r#"{"t":"chat","from":"xy","text":"hi"}"#), Some(("<xy> hi".into(), WHITE)));
        assert_eq!(line_of(r#"{"t":"chat","from":"","text":"xy joined the game"}"#), Some(("xy joined the game".into(), YELLOW)));
        assert_eq!(line_of(r#"{"t":"chat","from":"","text":"Unknown command","color":"red"}"#), Some(("Unknown command".into(), Color::srgb_u8(255, 85, 85))));
        assert_eq!(line_of(r#"{"t":"players","list":["xy"]}"#), None);
        assert_eq!(line_of("garbage"), None);
    }

    #[test]
    fn lines_fade_out_in_the_last_second() {
        assert_eq!(opacity(0.0, false), 1.0);
        assert_eq!(opacity(180.0, false), 1.0);
        assert!(opacity(190.0, false) > 0.2 && opacity(190.0, false) < 0.3);
        assert_eq!(opacity(200.0, false), 0.0);
        assert_eq!(opacity(5000.0, true), 1.0);
    }
}
