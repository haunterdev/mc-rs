//! `GuiFurnace` on `textures/gui/container/furnace.png`: the window, the flame that burns down
//! and the arrow that fills. The four fields are `TileEntityFurnace.getField`: 0 burn time, 1
//! current item burn time, 2 cook time, 3 total cook time, as `S_WINDOW_PROP` brings them.

use bevy::prelude::*;

use super::draw::{self, string_width, Frame};
use super::slots::Gui;
use crate::players::Font;

pub fn plugin(_app: &mut App) {}

/// `GuiContainer.xSize` and `ySize`
pub const SIZE: (i32, i32) = (176, 166);

/// `GuiFurnace.getCookProgressScaled`: `field2 * pixels / field3`, 0 when either is 0 (Java int
/// division).
pub fn cook_progress_scaled(fields: &[i32; 4], pixels: i32) -> i32 {
    let (i, j) = (fields[2], fields[3]);
    if j != 0 && i != 0 {
        i * pixels / j
    } else {
        0
    }
}

/// `GuiFurnace.getBurnLeftScaled`: `field0 * pixels / field1`, field 1 read as 200 when 0.
pub fn burn_left_scaled(fields: &[i32; 4], pixels: i32) -> i32 {
    let mut i = fields[1];
    if i == 0 {
        i = 200;
    }
    fields[0] * pixels / i
}

/// `drawGuiContainerBackgroundLayer`: the window, the flame when `isBurning` (field 0 above 0),
/// and the arrow.
pub fn background(frame: &mut Frame, gui: &Gui, fields: &[i32; 4]) {
    let q = &mut frame.bands[draw::BACKGROUND];
    q.sprite(gui.left, gui.top, 0, 0, SIZE.0, SIZE.1);
    if fields[0] > 0 {
        let k = burn_left_scaled(fields, 13);
        q.sprite(gui.left + 56, gui.top + 36 + 12 - k, 176, 12 - k, 14, k + 1);
    }
    let l = cook_progress_scaled(fields, 24);
    q.sprite(gui.left + 79, gui.top + 34, 176, 14, l + 1, 16);
}

/// `drawGuiContainerForegroundLayer`: the furnace's name centered at y 6 and the inventory's at
/// `(8, ySize - 96 + 2)`.
pub fn foreground(frame: &mut Frame, font: &Font, title: &str, inventory: &str, gui: &Gui) {
    let x = SIZE.0 / 2 - string_width(font, title, false) / 2;
    frame.bands[draw::TEXT].text(font, title, gui.left + x, gui.top + 6, 0x404040, false, false);
    frame.bands[draw::TEXT].text(font, inventory, gui.left + 8, gui.top + SIZE.1 - 96 + 2, 0x404040, false, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_arrow_fills_in_whole_pixels() {
        // 100 of 200 ticks: 12 of 24 pixels. Java truncates: 199 of 200 is 23.
        assert_eq!(cook_progress_scaled(&[0, 0, 100, 200], 24), 12);
        assert_eq!(cook_progress_scaled(&[0, 0, 199, 200], 24), 23);
        assert_eq!(cook_progress_scaled(&[0, 0, 7, 200], 24), 0);
        // Either field 0 gives 0.
        assert_eq!(cook_progress_scaled(&[0, 0, 0, 200], 24), 0);
        assert_eq!(cook_progress_scaled(&[0, 0, 50, 0], 24), 0);
    }

    #[test]
    fn the_flame_burns_down_against_the_last_fuels_time() {
        // Coal is 1600 ticks: full, half and nearly out.
        assert_eq!(burn_left_scaled(&[1600, 1600, 0, 0], 13), 13);
        assert_eq!(burn_left_scaled(&[800, 1600, 0, 0], 13), 6);
        assert_eq!(burn_left_scaled(&[1, 1600, 0, 0], 13), 0);
        // Field 1 of 0 reads as 200 (a stick-length burn).
        assert_eq!(burn_left_scaled(&[100, 0, 0, 0], 13), 6);
    }
}
