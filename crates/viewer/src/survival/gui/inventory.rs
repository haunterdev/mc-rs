//! `GuiInventory`: E in survival. The window is `ContainerPlayer` (window 0, the 2 x 2 grid, the
//! armor slots and the off hand) on `textures/gui/container/inventory.png`.
//!
//! The player model is `drawEntityOnScreen(left + 51, top + 75, 30, left + 51 - mouseX,
//! top + 75 - 50 - mouseY)`, drawn by `players::inventory_model` from `Frame::player_model`.
//!
//! DROPPED: the recipe book and its button.

use bevy::prelude::*;

use super::draw::{self, Frame};
use super::slots::Gui;
use crate::players::Font;

pub fn plugin(_app: &mut App) {}

/// `GuiContainer.xSize` and `ySize`
pub const SIZE: (i32, i32) = (176, 166);

/// `GuiInventory.drawScreen`'s `drawEntityOnScreen` call.
pub fn player_model(frame: &mut Frame, gui: &Gui, mx: i32, my: i32) {
    let (x, y) = (gui.left + 51, gui.top + 75);
    frame.player_model = Some(draw::PlayerModel { x, y, scale: 30, mouse_x: (x - mx) as f32, mouse_y: (y - 50 - my) as f32 });
}

/// `drawGuiContainerForegroundLayer`: "Crafting" over the grid at (97, 8), `4210752` gray.
pub fn foreground(frame: &mut Frame, font: &Font, crafting: &str, gui: &Gui) {
    frame.bands[draw::TEXT].text(font, crafting, gui.left + 97, gui.top + 8, 0x404040, false, false);
}
