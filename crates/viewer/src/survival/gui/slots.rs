//! The container screens' geometry in GUI pixels: `ScaledResolution`, the mouse position
//! `GuiScreen.handleMouseInput` computes, `GuiContainer.isPointInRegion` and
//! `hasClickedOutside`, and `GuiContainerCreative`'s tab, scrollbar and inventory-tab slot
//! layout. Pure functions, so the tests check them without a window.

use bevy::prelude::*;
use items::creative::Tab;

use crate::hand::gui_scale;

/// `GuiContainerCreative`'s `xSize` and `ySize`.
pub const CREATIVE_W: i32 = 195;
pub const CREATIVE_H: i32 = 136;

pub fn plugin(_app: &mut App) {}

/// The scaled screen and a container's place on it (`guiLeft`, `guiTop`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gui {
    /// The window in physical pixels.
    pub pw: u32,
    pub ph: u32,
    /// `ScaledResolution.getScaleFactor()`
    pub k: u32,
    /// `ScaledResolution.getScaledWidth()` and height, rounded up.
    pub w: i32,
    pub h: i32,
    pub left: i32,
    pub top: i32,
}

impl Gui {
    /// `GuiContainer.initGui`: the container centered, `(width - xSize) / 2`.
    pub fn new(pw: u32, ph: u32, x_size: i32, y_size: i32) -> Gui {
        let k = gui_scale(pw, ph);
        let (w, h) = (pw.div_ceil(k) as i32, ph.div_ceil(k) as i32);
        Gui { pw, ph, k, w, h, left: (w - x_size) / 2, top: (h - y_size) / 2 }
    }

    /// `GuiScreen.handleMouseInput`: `Mouse.getEventX() * width / displayWidth` and
    /// `height - Mouse.getEventY() * height / displayHeight - 1`, where LWJGL counts y up from
    /// the bottom row. `x`, `y` are physical pixels from the top left.
    pub fn mouse(&self, x: f32, y: f32) -> (i32, i32) {
        let (px, py) = (x.floor() as i64, y.floor() as i64);
        let ly = self.ph as i64 - 1 - py;
        let mx = px * self.w as i64 / self.pw as i64;
        let my = self.h as i64 - ly * self.h as i64 / self.ph as i64 - 1;
        (mx as i32, my as i32)
    }
}

/// `GuiContainer.isPointInRegion(rectX, rectY, w, h, x, y)`: the rect relative to the
/// container, one pixel of slack on each side.
pub fn in_region(gui: &Gui, rx: i32, ry: i32, rw: i32, rh: i32, x: i32, y: i32) -> bool {
    let (x, y) = (x - gui.left, y - gui.top);
    x >= rx - 1 && x < rx + rw + 1 && y >= ry - 1 && y < ry + rh + 1
}

/// `GuiContainer.isMouseOverSlot`
pub fn over_slot(gui: &Gui, sx: i32, sy: i32, x: i32, y: i32) -> bool {
    in_region(gui, sx, sy, 16, 16, x, y)
}

/// `GuiContainer.hasClickedOutside`
pub fn clicked_outside(gui: &Gui, x_size: i32, y_size: i32, x: i32, y: i32) -> bool {
    x < gui.left || y < gui.top || x >= gui.left + x_size || y >= gui.top + y_size
}

/// `isMouseOverTab` and `renderCreativeInventoryHoveringText`'s corner of a tab, relative to
/// the container.
pub fn tab_origin(tab: Tab) -> (i32, i32) {
    let i = tab.column() as i32;
    let mut j = 28 * i;
    if tab.is_aligned_right() {
        j = CREATIVE_W - 28 * (6 - i) + 2;
    } else if i > 0 {
        j += i;
    }
    let k = if tab.is_top_row() { -32 } else { CREATIVE_H };
    (j, k)
}

/// `GuiContainerCreative.isMouseOverTab`, with the mouse relative to the container.
pub fn over_tab(tab: Tab, x: i32, y: i32) -> bool {
    let (j, k) = tab_origin(tab);
    x >= j && x <= j + 28 && y >= k && y <= k + 32
}

/// The tab under the mouse (relative to the container), as the click loops find it.
pub fn tab_at(x: i32, y: i32) -> Option<Tab> {
    Tab::ALL.into_iter().find(|&t| over_tab(t, x, y))
}

/// `renderCreativeInventoryHoveringText`'s test.
pub fn tab_hovered(gui: &Gui, tab: Tab, x: i32, y: i32) -> bool {
    let (j, k) = tab_origin(tab);
    in_region(gui, j + 3, k + 3, 23, 27, x, y)
}

/// `GuiContainerCreative.drawTab`: the screen corner of the 28 x 32 sprite, its corner in
/// `tabs.png`, and where the icon goes.
pub fn tab_sprite(gui: &Gui, tab: Tab, selected: bool) -> TabSprite {
    let top = tab.is_top_row();
    let i = tab.column() as i32;
    let u = i * 28;
    let mut v = if selected { 32 } else { 0 };
    let mut x = gui.left + 28 * i;
    let mut y = gui.top;
    if tab.is_aligned_right() {
        x = gui.left + CREATIVE_W - 28 * (6 - i);
    } else if i > 0 {
        x += i;
    }
    if top {
        y -= 28;
    } else {
        v += 64;
        y += CREATIVE_H - 4;
    }
    TabSprite { x, y, u, v, icon: (x + 6, y + 8 + if top { 1 } else { -1 }) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabSprite {
    pub x: i32,
    pub y: i32,
    pub u: i32,
    pub v: i32,
    pub icon: (i32, i32),
}

/// `setCurrentCreativeTab`'s place for inventory-tab slot `l` (a `ContainerPlayer` slot
/// number): the crafting grid off screen, armor in two columns, the off hand beside them.
pub fn inventory_slot_pos(l: usize) -> (i32, i32) {
    match l {
        0..=4 => (-2000, -2000),
        5..=8 => {
            let j = (l - 5) as i32;
            (54 + j / 2 * 54, 6 + j % 2 * 27)
        }
        45 => (35, 20),
        _ => {
            let i = l as i32 - 9;
            (9 + i % 9 * 18, if l >= 36 { 112 } else { 54 + i / 9 * 18 })
        }
    }
}

/// `drawScreen`'s scrollbar track: `k`, `l`, `i1`, `j1`.
pub fn scroll_track(gui: &Gui) -> (i32, i32, i32, i32) {
    let k = gui.left + 175;
    let l = gui.top + 18;
    (k, l, k + 14, l + 112)
}

/// `drawScreen` while dragging the thumb: `((mouseY - l) - 7.5) / ((j1 - l) - 15)`, clamped.
pub fn scroll_from_mouse(gui: &Gui, y: i32) -> f32 {
    let (_, l, _, j1) = scroll_track(gui);
    (((y - l) as f32 - 7.5) / ((j1 - l) as f32 - 15.0)).clamp(0.0, 1.0)
}

/// `drawGuiContainerBackgroundLayer`'s thumb: `j + (int)((k - j - 17) * currentScroll)`.
pub fn thumb_y(gui: &Gui, scroll: f32) -> i32 {
    let j = gui.top + 18;
    let k = j + 112;
    j + ((k - j - 17) as f32 * scroll) as i32
}

/// `handleMouseInput`'s wheel: one notch moves one row of the list, up for a positive notch.
pub fn wheel_scroll(scroll: f32, notch: i32, items: usize) -> f32 {
    let j = (items as i32 + 9 - 1) / 9 - 5;
    let i = notch.signum();
    ((scroll as f64 - i as f64 / j as f64) as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mouse_maps_to_gui_pixels_at_every_scale() {
        // 854 x 480 is scale 2 (427 x 240); 1920 x 1080 is scale 4 (480 x 270).
        for (pw, ph, k) in [(320, 240, 1), (854, 480, 2), (1280, 720, 3), (1920, 1080, 4)] {
            let g = Gui::new(pw, ph, CREATIVE_W, CREATIVE_H);
            assert_eq!(g.k, k);
            assert_eq!(g.mouse(0.0, 0.0), (0, 0));
            assert_eq!(g.mouse(pw as f32 - 1.0, ph as f32 - 1.0), (g.w - 1, g.h - 1));
            // A physical pixel inside GUI pixel (10, 20) maps to it.
            assert_eq!(g.mouse((10 * k + k / 2) as f32, (20 * k + k / 2) as f32), (10, 20));
        }
    }

    #[test]
    fn the_container_is_centered() {
        let g = Gui::new(854, 480, CREATIVE_W, CREATIVE_H);
        assert_eq!((g.w, g.h), (427, 240));
        assert_eq!((g.left, g.top), (116, 52));
    }

    #[test]
    fn regions_have_a_pixel_of_slack() {
        let g = Gui::new(854, 480, CREATIVE_W, CREATIVE_H);
        let (sx, sy) = (9, 18);
        assert!(over_slot(&g, sx, sy, g.left + sx - 1, g.top + sy - 1));
        assert!(over_slot(&g, sx, sy, g.left + sx + 16, g.top + sy + 16));
        assert!(!over_slot(&g, sx, sy, g.left + sx + 17, g.top + sy));
        assert!(!over_slot(&g, sx, sy, g.left + sx - 2, g.top + sy));
    }

    #[test]
    fn every_tab_click_lands_on_its_sprite() {
        let g = Gui::new(854, 480, CREATIVE_W, CREATIVE_H);
        for tab in Tab::ALL {
            let s = tab_sprite(&g, tab, false);
            let (cx, cy) = (s.x + 14 - g.left, s.y + 16 - g.top);
            assert_eq!(tab_at(cx, cy), Some(tab), "{tab:?}");
            assert!(tab_hovered(&g, tab, s.x + 14, s.y + 16), "{tab:?}");
        }
        // The middle of the container is no tab.
        assert_eq!(tab_at(CREATIVE_W / 2, CREATIVE_H / 2), None);
    }

    #[test]
    fn the_inventory_tab_lays_out_the_player_slots() {
        assert_eq!(inventory_slot_pos(0), (-2000, -2000));
        assert_eq!(inventory_slot_pos(5), (54, 6));
        assert_eq!(inventory_slot_pos(6), (54, 33));
        assert_eq!(inventory_slot_pos(7), (108, 6));
        assert_eq!(inventory_slot_pos(8), (108, 33));
        assert_eq!(inventory_slot_pos(9), (9, 54));
        assert_eq!(inventory_slot_pos(35), (9 + 8 * 18, 90));
        assert_eq!(inventory_slot_pos(36), (9, 112));
        assert_eq!(inventory_slot_pos(44), (9 + 8 * 18, 112));
        assert_eq!(inventory_slot_pos(45), (35, 20));
    }

    #[test]
    fn the_scrollbar_spans_the_track() {
        let g = Gui::new(854, 480, CREATIVE_W, CREATIVE_H);
        assert_eq!(thumb_y(&g, 0.0), g.top + 18);
        assert_eq!(thumb_y(&g, 1.0), g.top + 18 + 95);
        assert_eq!(scroll_from_mouse(&g, g.top + 18), 0.0);
        assert_eq!(scroll_from_mouse(&g, g.top + 130), 1.0);
        // 90 items is 10 rows, 5 past the window: a notch down moves a fifth.
        assert!((wheel_scroll(0.0, -1, 90) - 0.2).abs() < 1e-6);
        assert_eq!(wheel_scroll(0.0, 1, 90), 0.0);
    }
}
