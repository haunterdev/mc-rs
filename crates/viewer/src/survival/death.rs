//! `GuiGameOver`, the death screen: a red gradient over the world, "You died!" at double size,
//! the death message, the score, and the Respawn and Title screen buttons. It opens on `S_DEATH`
//! and closes when Respawn sends `V_RESPAWN` (or the server's health says the player is alive
//! again). Esc does nothing here and the buttons stay disabled for 20 client ticks, as
//! `GuiGameOver.updateScreen` counts `enableButtonsTimer`.
//!
//! The layout, hover and click rules are pure ([`buttons`], [`click_at`]) and tested; the
//! systems draw into the death layers of [`super::overlay::Overlay`], so a native and a
//! streamed session show the same screen (a streamed pointer comes from `StreamCursor`).
//!
//! DROPPED: the `GuiYesNo` confirmation of Title screen (it leaves at once), the hover
//! component tooltip of the death message, the hardcore wording ("Spectate world" and
//! "Delete world"), and the real score (`EntityPlayer.getScore`, which the server does not
//! send): the experience total stands in. Title screen is offered in a native session only,
//! where it ends the process like the options menu's Disconnect; a streamed session has no
//! title screen to return to.

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::gui::creative::Creative;
use super::gui::draw::string_width;
use super::gui::slots::Gui;
use super::gui::GuiLang;
use super::overlay::{Overlay, DEATH_BACK, DEATH_BUTTONS, DEATH_TEXT};
use super::{net, Death, SurvivalState};
use crate::audio::{Audio, Category};
use crate::link::{Link, StreamCursor};
use crate::players::Font;
use crate::settings::Menu;
use crate::sky::Clock;

pub fn plugin(app: &mut App) {
    app.init_resource::<DeathScreen>().add_systems(Update, (open, click, draw).chain().run_if(resource_exists::<Menu>));
}

/// `GuiGameOver.enableButtonsTimer` runs to this.
const ENABLE_AFTER_TICKS: i32 = 20;

/// The open screen, if any.
#[derive(Resource, Default)]
pub struct DeathScreen {
    pub open: Option<Screen>,
}

pub struct Screen {
    /// `EntityPlayerSP.getCombatTracker().getDeathMessage()` as the server wrote it.
    pub message: String,
    pub score: i32,
    /// Client ticks since the screen opened.
    pub ticks: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Respawn,
    TitleScreen,
}

/// A button: where it is and what it does. All are 200 x 20 (`GuiButton`'s default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Button {
    pub action: Action,
    pub x: i32,
    pub y: i32,
}

pub const BUTTON_W: i32 = 200;
pub const BUTTON_H: i32 = 20;

/// `GuiGameOver.initGui` (non-hardcore): Respawn at `height / 4 + 72`, Title screen 24 below.
pub fn buttons(w: i32, h: i32, title_screen: bool) -> Vec<Button> {
    let mut out = vec![Button { action: Action::Respawn, x: w / 2 - 100, y: h / 4 + 72 }];
    if title_screen {
        out.push(Button { action: Action::TitleScreen, x: w / 2 - 100, y: h / 4 + 96 });
    }
    out
}

/// `GuiButton.mousePressed`: enabled and the point inside `[x, x + width) x [y, y + height)`.
pub fn hit(b: &Button, enabled: bool, mx: i32, my: i32) -> bool {
    enabled && mx >= b.x && my >= b.y && mx < b.x + BUTTON_W && my < b.y + BUTTON_H
}

/// What a press at (`mx`, `my`) does.
pub fn click_at(ticks: i32, list: &[Button], mx: i32, my: i32) -> Option<Action> {
    list.iter().find(|b| hit(b, ticks >= ENABLE_AFTER_TICKS, mx, my)).map(|b| b.action)
}

/// `GuiButton.getHoverState`: 0 disabled, 1 idle, 2 hovered; the face row in `widgets.png` is
/// `46 + state * 20`.
pub fn hover_state(enabled: bool, over: bool) -> i32 {
    if !enabled {
        0
    } else if over {
        2
    } else {
        1
    }
}

/// `GuiButton.drawButton`'s label color: grey disabled, yellow hovered, light grey otherwise.
pub fn label_color(state: i32) -> u32 {
    match state {
        0 => 10526880,
        2 => 16777120,
        _ => 14737632,
    }
}

fn pointer(window: &Window, stream: Option<&StreamCursor>) -> Option<Vec2> {
    stream.and_then(|c| c.0).or_else(|| window.physical_cursor_position())
}

/// Opens on `S_DEATH` (closing the creative inventory, which `displayGuiScreen` replaces),
/// counts the button timer and closes once the server says the player lives.
fn open(
    mut screen: ResMut<DeathScreen>,
    mut menu: ResMut<Menu>,
    mut deaths: MessageReader<Death>,
    state: Res<SurvivalState>,
    clock: Option<Res<Clock>>,
    creative: Option<ResMut<Creative>>,
) {
    let mut opened = false;
    for Death(d) in deaths.read() {
        screen.open = Some(Screen { message: d.message.clone(), score: state.xp.total, ticks: 0 });
        opened = true;
    }
    if opened {
        if let Some(mut creative) = creative {
            creative.screen = None;
        }
        menu.open = true;
    }
    let Some(s) = screen.open.as_mut() else { return };
    if !opened {
        s.ticks += clock.map_or(0, |c| c.ticks_this_frame as i32);
    }
    if !state.dead && !opened && s.ticks > 0 {
        screen.open = None;
        menu.open = false;
    }
}

/// `GuiGameOver.actionPerformed` and `mouseClicked`: Respawn sends `V_RESPAWN` and closes.
#[allow(clippy::too_many_arguments)]
fn click(
    mut screen: ResMut<DeathScreen>,
    mut menu: ResMut<Menu>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    stream: Option<Res<StreamCursor>>,
    link: Option<Res<Link>>,
    audio: Option<Res<Audio>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(s) = screen.open.as_ref() else { return };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(p) = pointer(&window, stream.as_deref()) else { return };
    let gui = Gui::new(window.physical_width(), window.physical_height(), 0, 0);
    let (mx, my) = gui.mouse(p.x, p.y);
    let Some(action) = click_at(s.ticks, &buttons(gui.w, gui.h, menu.quit.is_some()), mx, my) else { return };
    if let Some(audio) = audio {
        audio.play("ui.button.click", Category::Master, None, 0.25, 1.0);
    }
    match action {
        Action::Respawn => {
            if let Some(link) = link {
                net::send_respawn(&link);
            }
            screen.open = None;
            menu.open = false;
        }
        Action::TitleScreen => {
            exit.write(AppExit::Success);
        }
    }
}

/// `GuiGameOver.drawScreen`.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut overlay: ResMut<Overlay>,
    screen: Res<DeathScreen>,
    menu: Res<Menu>,
    window: Single<&Window, With<PrimaryWindow>>,
    stream: Option<Res<StreamCursor>>,
    font: Option<Res<Font>>,
    lang: Option<Res<GuiLang>>,
) {
    for layer in [DEATH_BACK, DEATH_BUTTONS, DEATH_TEXT] {
        overlay.layers[layer] = Default::default();
    }
    let (Some(s), Some(font)) = (screen.open.as_ref(), font) else { return };
    let t = |key: &'static str| lang.as_ref().map_or(key, |l| l.0.translate(key)).to_string();
    let gui = Gui::new(window.physical_width(), window.physical_height(), 0, 0);
    let (w, h) = (gui.w, gui.h);
    let mouse = pointer(&window, stream.as_deref()).map_or((-10_000, -10_000), |p| gui.mouse(p.x, p.y));

    overlay.layers[DEATH_BACK].gradient(0, 0, w, h, 0x6050_0000, 0xA080_3030);

    let text = &mut overlay.layers[DEATH_TEXT];
    // pushMatrix; scale(2, 2, 2); drawCenteredString(title, width / 2 / 2, 30, 16777215)
    let from = text.vertices();
    let title = t("deathScreen.title");
    text.text(&font, &title, w / 2 / 2 - string_width(&font, &title, false) / 2, 30, 0xFFFFFF, true, false);
    text.scale_from(from, 2.0);
    let message = &s.message;
    text.text(&font, message, w / 2 - string_width(&font, message, false) / 2, 85, 0xFFFFFF, true, false);
    // "Score: " then the number in yellow (TextFormatting.YELLOW).
    let label = format!("{}: ", t("deathScreen.score"));
    let number = s.score.to_string();
    let x = w / 2 - (string_width(&font, &label, false) + string_width(&font, &number, false)) / 2;
    let after = text.text(&font, &label, x, 100, 0xFFFFFF, true, false);
    text.text(&font, &number, after - 1, 100, 0xFFFF55, true, false);

    let enabled = s.ticks >= ENABLE_AFTER_TICKS;
    for b in buttons(w, h, menu.quit.is_some()) {
        let over = mouse.0 >= b.x && mouse.1 >= b.y && mouse.0 < b.x + BUTTON_W && mouse.1 < b.y + BUTTON_H;
        let state = hover_state(enabled, over);
        let v = 46 + state * 20;
        let faces = &mut overlay.layers[DEATH_BUTTONS];
        faces.sprite(b.x, b.y, 0, v, BUTTON_W / 2, BUTTON_H);
        faces.sprite(b.x + BUTTON_W / 2, b.y, 200 - BUTTON_W / 2, v, BUTTON_W / 2, BUTTON_H);
        let label = t(match b.action {
            Action::Respawn => "deathScreen.respawn",
            Action::TitleScreen => "deathScreen.titleScreen",
        });
        let text = &mut overlay.layers[DEATH_TEXT];
        text.text(&font, &label, b.x + BUTTON_W / 2 - string_width(&font, &label, false) / 2, b.y + (BUTTON_H - 8) / 2, label_color(state), true, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buttons_sit_where_gui_game_over_puts_them() {
        // 320 x 240 scaled: Respawn at (60, 132), Title screen at (60, 156).
        let list = buttons(320, 240, true);
        assert_eq!(list[0], Button { action: Action::Respawn, x: 60, y: 132 });
        assert_eq!(list[1], Button { action: Action::TitleScreen, x: 60, y: 156 });
        assert_eq!(buttons(320, 240, false).len(), 1);
    }

    #[test]
    fn a_press_does_nothing_until_the_timer_runs_out_and_edges_follow_guibutton() {
        let list = buttons(320, 240, true);
        assert_eq!(click_at(19, &list, 100, 140), None);
        assert_eq!(click_at(20, &list, 100, 140), Some(Action::Respawn));
        assert_eq!(click_at(20, &list, 100, 160), Some(Action::TitleScreen));
        // Left and top edges are in, right and bottom edges are out.
        assert_eq!(click_at(20, &list, 60, 132), Some(Action::Respawn));
        assert_eq!(click_at(20, &list, 260, 140), None);
        assert_eq!(click_at(20, &list, 100, 152), None);
        assert_eq!(click_at(20, &list, 59, 140), None);
    }

    #[test]
    fn faces_and_label_colors_follow_the_hover_state() {
        assert_eq!(hover_state(false, true), 0);
        assert_eq!(hover_state(true, false), 1);
        assert_eq!(hover_state(true, true), 2);
        assert_eq!([0, 1, 2].map(label_color), [10526880, 14737632, 16777120]);
    }
}
