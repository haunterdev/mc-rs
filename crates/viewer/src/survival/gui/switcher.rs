//! F3+F4, the game mode switcher. 1.12.2 has none: it is `GameModeSwitcherScreen` of
//! 1.16, with the keys of `KeyboardHandler.handleDebugKeys`. Holding F3 and pressing F4 opens the
//! row of mode icons with the previous mode (else creative, or survival from creative) picked;
//! each further F4 steps to the next mode, and releasing F3 sends `/gamemode <mode>` through the
//! chat line's path (`V_TEXT` chat), so the server's permission check and feedback apply.
//! F3+T stays unbound (1.12.2 reloads resources with it, which the viewer does not). Esc closes
//! the screen without a switch.
//!
//! DROPPED: the mouse hover and click on a slot, the `gamemode_switcher.png` sprites (the panel,
//! slots and selection are drawn as plain quads over the item icons, since that texture is not in
//! the 1.12.2 jar), and the client-side permission check (`hasPermissions(2)`): the server answers the command.
//! Every mode the server's `/gamemode` takes is offered ([`supported_modes`]).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use items::ids::{block, item};
use items::ItemStack;

use super::draw::{self, string_width, Frame, Icon};
use super::slots::Gui;
use super::GuiLang;
use crate::link::Link;
use crate::player::Player;
use crate::players::Font;
use crate::settings::Menu;

/// One slot is 26 wide at a pitch of 31, as `GameModeSwitcherScreen.GameModeIcon`.
const SLOT: i32 = 26;
const PITCH: i32 = 31;

pub fn plugin(app: &mut App) {
    app.init_resource::<Switcher>().add_systems(
        Update,
        (keys, draw_screen).chain().after(super::creative::draw_screen).run_if(resource_exists::<super::ContainerImages>),
    );
}

#[derive(Resource, Default)]
pub struct Switcher {
    pub open: bool,
    /// `currentlyHovered`, a `proto::GM_*` mode.
    pub hovered: u8,
    /// The mode before the current one (`getPreviousPlayerMode`) and the one seen last frame.
    previous: Option<u8>,
    last: Option<u8>,
    /// `--demo-switcher`: F3 counts as held, so a shot can show the screen.
    pub hold: bool,
}

impl Switcher {
    /// The screen already open on adventure with F3 held, for `--demo-switcher`.
    pub fn demo() -> Switcher {
        Switcher { open: true, hovered: proto::GM_ADVENTURE, hold: true, ..Switcher::default() }
    }
}

/// `GameModeIcon.VALUES`: creative, survival, adventure, spectator.
pub fn supported_modes() -> &'static [u8] {
    &[proto::GM_CREATIVE, proto::GM_SURVIVAL, proto::GM_ADVENTURE, proto::GM_SPECTATOR]
}

/// `GameModeIcon.getNext`: the mode after `current` in the list, wrapping.
pub fn next_mode(current: u8, modes: &[u8]) -> u8 {
    match modes.iter().position(|&m| m == current) {
        Some(i) => modes[(i + 1) % modes.len()],
        None => modes[0],
    }
}

/// `GameModeSwitcherScreen.getDefaultSelected`: the previous mode when there is one, else
/// survival from creative and creative from any other.
pub fn default_selected(current: u8, previous: Option<u8>, modes: &[u8]) -> u8 {
    let pick = previous.filter(|p| modes.contains(p)).unwrap_or(if current == proto::GM_CREATIVE { proto::GM_SURVIVAL } else { proto::GM_CREATIVE });
    if modes.contains(&pick) {
        pick
    } else {
        modes[0]
    }
}

/// The command `switchToHoveredGameMode` sends.
pub fn command(mode: u8) -> String {
    format!("/gamemode {}", mode_word(mode))
}

fn mode_word(mode: u8) -> &'static str {
    match mode {
        proto::GM_CREATIVE => "creative",
        proto::GM_ADVENTURE => "adventure",
        proto::GM_SPECTATOR => "spectator",
        _ => "survival",
    }
}

/// `GameModeIcon.stack`: grass block, iron sword, map, ender eye.
fn icon_stack(mode: u8) -> ItemStack {
    match mode {
        proto::GM_CREATIVE => ItemStack::of(block::GRASS),
        proto::GM_ADVENTURE => ItemStack::of(item::MAP),
        proto::GM_SPECTATOR => ItemStack::of(item::ENDER_EYE),
        _ => ItemStack::of(item::IRON_SWORD),
    }
}

fn mode_title(mode: u8, lang: Option<&GuiLang>) -> String {
    let key = format!("gameMode.{}", mode_word(mode));
    lang.map_or_else(|| key.clone(), |l| l.0.translate(&key).to_string())
}

/// What `keys` does this frame, so the state machine is testable without a window.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Open(u8),
    Cycle(u8),
    /// F3 released: the mode to switch to, or `None` when it is the current one.
    Close(Option<u8>),
    /// The menu key closed the screen: no switch.
    Cancel,
}

pub fn step(sw: &Switcher, f3: bool, f4_pressed: bool, menu_open: bool, current: u8, can_open: bool) -> Step {
    let modes = supported_modes();
    if !sw.open {
        return if can_open && f3 && f4_pressed && !menu_open { Step::Open(default_selected(current, sw.previous, modes)) } else { Step::Nothing };
    }
    if !menu_open {
        return Step::Cancel;
    }
    if !f3 {
        return Step::Close((sw.hovered != current).then_some(sw.hovered));
    }
    if f4_pressed {
        return Step::Cycle(next_mode(sw.hovered, modes));
    }
    Step::Nothing
}

fn keys(
    mut sw: ResMut<Switcher>,
    mut menu: ResMut<Menu>,
    pressed: Res<ButtonInput<KeyCode>>,
    player: Res<Player>,
    link: Option<Res<Link>>,
    death: Option<Res<crate::survival::death::DeathScreen>>,
    creative: Option<Res<super::creative::Creative>>,
    windows: Option<Res<super::window::Windows>>,
) {
    if sw.last != Some(player.game_mode) {
        sw.previous = sw.last;
        sw.last = Some(player.game_mode);
    }
    let screen = death.is_some_and(|d| d.open.is_some())
        || creative.is_some_and(|c| c.screen.is_some())
        || windows.is_some_and(|w| w.screen.is_some());
    let can_open = player.spawned && !screen;
    match step(&sw, pressed.pressed(KeyCode::F3) || sw.hold, pressed.just_pressed(KeyCode::F4), menu.open, player.game_mode, can_open) {
        Step::Nothing => {}
        Step::Open(mode) => {
            sw.open = true;
            sw.hovered = mode;
            // A screen frees the mouse and swallows the game keys, as the chat line does.
            menu.open = true;
        }
        Step::Cycle(mode) => sw.hovered = mode,
        Step::Close(mode) => {
            sw.open = false;
            menu.open = false;
            if let (Some(mode), Some(link)) = (mode, link) {
                link.send(proto::V_TEXT, serde_json::json!({"t": "chat", "text": command(mode)}).to_string().as_bytes());
            }
        }
        Step::Cancel => sw.open = false,
    }
}

/// `GameModeSwitcherScreen.drawScreen`: the panel, the slots with the item icons, the selection
/// frame, the hovered mode's name above and `[ F4 ] Next` below.
fn draw_screen(
    sw: Res<Switcher>,
    mut frame: ResMut<Frame>,
    window: Single<&Window, With<PrimaryWindow>>,
    font: Option<Res<Font>>,
    lang: Option<Res<GuiLang>>,
) {
    if !sw.open {
        return;
    }
    let Some(font) = font else { return };
    let frame = &mut *frame;
    frame.clear();
    frame.open = true;
    let gui = Gui::new(window.physical_width(), window.physical_height(), 0, 0);
    let modes = supported_modes();
    let total = modes.len() as i32 * PITCH - 5;
    let (cx, cy) = (gui.w / 2, gui.h / 2);
    let first = cx - total / 2;
    let top = cy - PITCH;
    let back = &mut frame.bands[draw::BACKDROP];
    back.rect(cx - 62, top - 27, cx - 62 + 125, top - 27 + 75, 0xC010_1010);
    back.rect(cx - 62, top - 27, cx - 62 + 125, top - 26, 0xFF55_5555);
    back.rect(cx - 62, top - 27 + 74, cx - 62 + 125, top - 27 + 75, 0xFF55_5555);
    for (i, &mode) in modes.iter().enumerate() {
        let x = first + i as i32 * PITCH;
        let under = &mut frame.bands[draw::UNDER];
        under.rect(x, top, x + SLOT, top + SLOT, 0xFF37_3737);
        under.rect(x + 1, top + 1, x + SLOT - 1, top + SLOT - 1, 0xFF8B_8B8B);
        under.rect(x + 2, top + 2, x + SLOT - 2, top + SLOT - 2, 0xFF37_3737);
        frame.icons.push(Icon { stack: icon_stack(mode), x: x + 5, y: top + 5, z: draw::SLOT_Z });
        if mode == sw.hovered {
            // The selection: a white two pixel frame, as the hovered sprite's border.
            let q = &mut frame.bands[draw::HIGHLIGHT];
            for (x0, y0, x1, y1) in [(x, top, x + SLOT, top + 2), (x, top + SLOT - 2, x + SLOT, top + SLOT), (x, top, x + 2, top + SLOT), (x + SLOT - 2, top, x + SLOT, top + SLOT)] {
                q.rect(x0, y0, x1, y1, 0xFFFF_FFFF);
            }
        }
    }
    let name = mode_title(sw.hovered, lang.as_deref());
    let text = &mut frame.bands[draw::TEXT];
    text.text(&font, &name, cx - string_width(&font, &name, false) / 2, cy - PITCH - 20, 0xFFFFFF, true, false);
    let next = "[ F4 ] Next";
    text.text(&font, next, cx - string_width(&font, next, false) / 2, cy + 5, 0xFFFFFF, true, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(hovered: u8) -> Switcher {
        Switcher { open: true, hovered, previous: None, last: None, hold: false }
    }

    #[test]
    fn f4_cycles_creative_survival_adventure_spectator() {
        let m = supported_modes();
        assert_eq!(next_mode(proto::GM_CREATIVE, m), proto::GM_SURVIVAL);
        assert_eq!(next_mode(proto::GM_SURVIVAL, m), proto::GM_ADVENTURE);
        assert_eq!(next_mode(proto::GM_ADVENTURE, m), proto::GM_SPECTATOR);
        assert_eq!(next_mode(proto::GM_SPECTATOR, m), proto::GM_CREATIVE);
        // A mode the list lacks is skipped over.
        assert_eq!(next_mode(proto::GM_SURVIVAL, &[proto::GM_CREATIVE, proto::GM_SURVIVAL, proto::GM_SPECTATOR]), proto::GM_SPECTATOR);
    }

    #[test]
    fn the_first_pick_is_the_previous_mode_else_survival_from_creative() {
        let m = supported_modes();
        assert_eq!(default_selected(proto::GM_CREATIVE, None, m), proto::GM_SURVIVAL);
        assert_eq!(default_selected(proto::GM_SURVIVAL, None, m), proto::GM_CREATIVE);
        assert_eq!(default_selected(proto::GM_SURVIVAL, Some(proto::GM_SPECTATOR), m), proto::GM_SPECTATOR);
        assert_eq!(default_selected(proto::GM_SURVIVAL, Some(proto::GM_SPECTATOR), &[proto::GM_CREATIVE]), proto::GM_CREATIVE);
    }

    #[test]
    fn f3_with_f4_opens_cycles_and_releasing_f3_sends_the_command() {
        let sw = Switcher::default();
        assert_eq!(step(&sw, true, false, false, proto::GM_CREATIVE, true), Step::Nothing);
        assert_eq!(step(&sw, false, true, false, proto::GM_CREATIVE, true), Step::Nothing, "F4 alone");
        assert_eq!(step(&sw, true, true, false, proto::GM_CREATIVE, false), Step::Nothing, "a screen is open");
        assert_eq!(step(&sw, true, true, false, proto::GM_CREATIVE, true), Step::Open(proto::GM_SURVIVAL));
        let sw = open(proto::GM_SURVIVAL);
        assert_eq!(step(&sw, true, true, true, proto::GM_CREATIVE, true), Step::Cycle(proto::GM_ADVENTURE));
        assert_eq!(step(&sw, true, false, true, proto::GM_CREATIVE, true), Step::Nothing);
        assert_eq!(step(&sw, false, false, true, proto::GM_CREATIVE, true), Step::Close(Some(proto::GM_SURVIVAL)));
        assert_eq!(step(&sw, false, false, true, proto::GM_SURVIVAL, true), Step::Close(None), "the current mode sends nothing");
        assert_eq!(step(&sw, true, false, false, proto::GM_CREATIVE, true), Step::Cancel);
        assert_eq!(command(proto::GM_SPECTATOR), "/gamemode spectator");
        assert_eq!(command(proto::GM_SURVIVAL), "/gamemode survival");
    }
}
