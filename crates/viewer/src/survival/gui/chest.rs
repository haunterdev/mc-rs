//! `GuiChest`: a chest or a large chest (`ContainerChest`) on `textures/gui/container/generic_54.png`.
//! The window is 114 + 18 per row tall; the top is the chest's rows, the bottom the player's
//! inventory, both cut from the same texture.

use bevy::prelude::*;

use super::draw::{self, Frame};
use super::slots::Gui;
use crate::players::Font;

pub fn plugin(_app: &mut App) {}

/// `GuiContainer.xSize` and `GuiChest.ySize = 114 + inventoryRows * 18`
pub fn size(rows: u8) -> (i32, i32) {
    (176, 114 + rows as i32 * 18)
}

/// `drawGuiContainerBackgroundLayer`: the rows' part of the texture, then the inventory's part
/// from v 126.
pub fn background(frame: &mut Frame, gui: &Gui, rows: u8) {
    let r = rows as i32;
    frame.bands[draw::BACKGROUND].sprite(gui.left, gui.top, 0, 0, 176, r * 18 + 17);
    frame.bands[draw::BACKGROUND].sprite(gui.left, gui.top + r * 18 + 17, 0, 126, 176, 96);
}

/// `drawGuiContainerForegroundLayer`: the chest's name at (8, 6) and the inventory's at
/// `(8, ySize - 96 + 2)`, `4210752` gray.
pub fn foreground(frame: &mut Frame, font: &Font, title: &str, inventory: &str, rows: u8, gui: &Gui) {
    frame.bands[draw::TEXT].text(font, title, gui.left + 8, gui.top + 6, 0x404040, false, false);
    frame.bands[draw::TEXT].text(font, inventory, gui.left + 8, gui.top + size(rows).1 - 96 + 2, 0x404040, false, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_as_tall_as_its_rows() {
        assert_eq!(size(3), (176, 168));
        assert_eq!(size(6), (176, 222));
    }
}
