//! `GuiCrafting`: the crafting table window (`ContainerWorkbench`) on
//! `textures/gui/container/crafting_table.png`.
//!
//! DROPPED: the recipe book and its button.

use bevy::prelude::*;

use super::draw::{self, Frame};
use super::slots::Gui;
use crate::players::Font;

pub fn plugin(_app: &mut App) {}

/// `GuiContainer.xSize` and `ySize`
pub const SIZE: (i32, i32) = (176, 166);

/// `drawGuiContainerForegroundLayer`: the table's name at (28, 6) and the inventory's at
/// `(8, ySize - 96 + 2)`, `4210752` gray.
pub fn foreground(frame: &mut Frame, font: &Font, crafting: &str, inventory: &str, gui: &Gui) {
    frame.bands[draw::TEXT].text(font, crafting, gui.left + 28, gui.top + 6, 0x404040, false, false);
    frame.bands[draw::TEXT].text(font, inventory, gui.left + 8, gui.top + SIZE.1 - 96 + 2, 0x404040, false, false);
}
