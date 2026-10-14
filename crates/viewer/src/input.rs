//! Mouse focus and the mouse look: `Minecraft.setIngameFocus` and `setIngameNotInFocus`, the
//! click that captures the mouse, and the turn `EntityRenderer.updateCameraAndRender` applies each
//! frame.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::hand::Hand;
use crate::hotbar::Inventory;
use crate::interact::Controller;
use crate::player::Player;
use crate::settings::{Menu, Settings};

/// `Minecraft.inGameHasFocus`: the mouse is captured and turns the view.
#[derive(Resource, Default)]
pub struct Focus {
    pub in_game: bool,
}

/// `Minecraft.setIngameFocus`: grabs the mouse. The click that did it is not an attack, which
/// `leftClickCounter` enforces until the button is let go.
fn grab(focus: &mut Focus, cursor: &mut CursorOptions, hand: &mut Hand) {
    focus.in_game = true;
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
    hand.left_click_counter = 10000;
}

/// `Minecraft.setIngameNotInFocus`: frees the mouse. The keys read as released while a screen is
/// open, which `run_ticks` does by ignoring them.
fn release(focus: &mut Focus, cursor: &mut CursorOptions) {
    focus.in_game = false;
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
}

/// Follows the menu (open frees the mouse, closing grabs it), queues attack presses, and grabs
/// the mouse on a click into the game (`Minecraft.runTickMouse`).
pub fn focus(
    menu: Res<Menu>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut focus: ResMut<Focus>,
    mut hand: ResMut<Hand>,
    mut ctl: ResMut<Controller>,
) {
    let (window, mut cursor) = window.into_inner();
    if menu.is_changed() && !menu.is_added() {
        if menu.open {
            release(&mut focus, &mut cursor);
        } else if window.focused {
            grab(&mut focus, &mut cursor, &mut hand);
        }
    }
    // KeyBinding.onTick queues the press even when it is the one that grabs the mouse. The press
    // that hits Done closes the menu this frame and is not queued.
    if !menu.open && !menu.is_changed() {
        if mouse.just_pressed(MouseButton::Left) {
            hand.clicks += 1;
        }
        if mouse.just_pressed(MouseButton::Right) {
            ctl.uses += 1;
        }
        if mouse.just_pressed(MouseButton::Middle) {
            ctl.picks += 1;
        }
    }
    if !menu.open && !focus.in_game && window.focused && mouse.get_just_pressed().next().is_some() {
        grab(&mut focus, &mut cursor, &mut hand);
    }
}

/// F5: `GameSettings.thirdPersonView` steps first person, behind, in front.
pub fn perspective_key(keys: Res<ButtonInput<KeyCode>>, menu: Res<Menu>, mut view: ResMut<crate::sky::View>) {
    if !menu.open && keys.just_pressed(KeyCode::F5) {
        view.third_person = (view.third_person + 1) % 3;
    }
}

/// `EntityRenderer.updateCameraAndRender`: the mouse turns the player while the game has focus,
/// after this frame's ticks.
/// DROPPED: the smooth camera (cinematic mode).
pub fn mouse_turn(
    focus: Res<Focus>,
    settings: Res<Settings>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut player: ResMut<Player>,
) {
    if !focus.in_game || !window.focused {
        return;
    }
    let d = motion.delta;
    if d == Vec2::ZERO {
        return;
    }
    let f = settings.sensitivity * 0.6 + 0.2;
    let f1 = f * f * f * 8.0;
    // LWJGL's y grows upward, the window's downward.
    player.turn(d.x * f1, -d.y * f1);
}

/// `Minecraft.runTickMouse`'s wheel: whole notches queue for the next tick, which turns each into
/// a hotbar step (`InventoryPlayer.changeCurrentItem`).
pub fn wheel(menu: Res<Menu>, scroll: Res<AccumulatedMouseScroll>, mut inv: ResMut<Inventory>, mut acc: Local<f32>) {
    if menu.open {
        *acc = 0.0;
        return;
    }
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
    };
    *acc += lines;
    let notches = acc.trunc() as i32;
    *acc -= notches as f32;
    inv.scroll += notches;
}
