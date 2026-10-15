//! The container screens, one file per screen. [`draw`] puts a screen on the window, [`slots`]
//! holds the geometry, and this file is `GuiContainer`: the [`Screen`] trait a screen implements
//! (its slots and its `handleMouseClick`), and the shared hovering, slot drawing, cursor stack,
//! and the presses and releases that become `handleMouseClick` calls, drag splitting included.
//!
//! Only `creative.rs` is filled so far; the survival screens (`inventory.rs`, `crafting.rs`,
//! `chest.rs`, `furnace.rs`) come with the server windows. DROPPED from `GuiContainer`: the
//! touchscreen mode (`clickedSlot`, `draggedStack`, the returning-stack animation).

use bevy::prelude::*;
use items::ItemStack;
use sim::container::{can_add_item_to_slot, compute_stack_size, get_quickcraft_mask, ClickType};
use sim::inventory::InventoryPlayer;

use crate::players::Font;

mod chest;
mod crafting;
pub mod creative;
pub mod draw;
pub mod enchanting;
mod furnace;
mod inventory;
pub mod slots;
pub mod switcher;
pub mod window;

pub use draw::ContainerImages;
use draw::Frame;
use slots::{clicked_outside, over_slot, Gui};

/// `TextFormatting.YELLOW`
pub const YELLOW: u32 = 0xFFFF55;

pub fn plugin(app: &mut App) {
    app.init_resource::<Frame>()
        // The viewer's setup inserts the images; a bare SurvivalPlugin (the net tests) has none.
        .add_systems(PostStartup, draw::spawn_screen.run_if(resource_exists::<ContainerImages>))
        .add_systems(PostUpdate, draw::present.run_if(resource_exists::<ContainerImages>).before(bevy::transform::TransformSystems::Propagate));
    chest::plugin(app);
    crafting::plugin(app);
    creative::plugin(app);
    enchanting::plugin(app);
    furnace::plugin(app);
    inventory::plugin(app);
    slots::plugin(app);
    switcher::plugin(app);
    window::plugin(app);
}

/// `lang/en_us.lang` from the jar: tab titles, item names and tooltips.
#[derive(Resource)]
pub struct GuiLang(pub sim::lang::Lang);

/// What a click sent besides the slot changes (`PlayerControllerMP.sendPacketDropItem`).
pub enum Sent {
    Drop(ItemStack),
}

/// `GuiContainer`'s own state.
pub struct Base {
    pub hovered: Option<usize>,
    pub drag_splitting: bool,
    drag_split_limit: i32,
    drag_split_button: i32,
    /// `dragSplittingSlots`, in the order the mouse crossed them.
    pub drag_slots: Vec<usize>,
    remnant: i32,
    ignore_mouse_up: bool,
    double_click: bool,
    last_click_slot: Option<usize>,
    /// Milliseconds, `Minecraft.getSystemTime`.
    last_click_time: f64,
    last_click_button: i32,
    shift_clicked_slot: ItemStack,
}

impl Default for Base {
    fn default() -> Base {
        Base {
            hovered: None,
            drag_splitting: false,
            drag_split_limit: 0,
            drag_split_button: 0,
            drag_slots: Vec::new(),
            remnant: 0,
            ignore_mouse_up: false,
            double_click: false,
            last_click_slot: None,
            last_click_time: 0.0,
            last_click_button: 0,
            shift_clicked_slot: ItemStack::EMPTY,
        }
    }
}

/// A container screen: its `inventorySlots` as seen through `Slot`, and its `handleMouseClick`.
/// Slots are indices into the screen's slot list.
pub trait Screen {
    fn base(&self) -> &Base;
    fn base_mut(&mut self) -> &mut Base;
    /// `xSize`, `ySize`
    fn size(&self) -> (i32, i32);
    fn slot_count(&self) -> usize;
    /// `xPos`, `yPos`
    fn slot_pos(&self, i: usize) -> (i32, i32);
    /// `slotNumber`
    fn slot_number(&self, i: usize) -> i32;
    fn stack(&self, inv: &mut InventoryPlayer, i: usize) -> ItemStack;
    /// `slot2.inventory == slot.inventory`
    fn same_inventory(&self, a: usize, b: usize) -> bool;
    fn can_take(&self, inv: &mut InventoryPlayer, i: usize) -> bool;
    fn is_item_valid(&self, i: usize, stack: &ItemStack) -> bool;
    fn item_stack_limit(&self, i: usize, stack: &ItemStack) -> i32;
    /// `Container.canDragIntoSlot`
    fn can_drag_into(&self, i: usize) -> bool;
    /// `Container.canMergeSlot(ItemStack.EMPTY, slot)`
    fn can_merge(&self, i: usize) -> bool;
    /// `getSlotTexture`: which empty-slot sprite an empty slot shows.
    fn slot_texture(&self, i: usize) -> Option<usize>;
    fn handle_mouse_click(&mut self, inv: &mut InventoryPlayer, out: &mut Vec<Sent>, slot: Option<usize>, slot_id: i32, button: i32, click: ClickType);
}

/// `getSlotAtPosition`
pub fn slot_at<S: Screen>(s: &S, gui: &Gui, x: i32, y: i32) -> Option<usize> {
    (0..s.slot_count()).find(|&i| {
        let (sx, sy) = s.slot_pos(i);
        over_slot(gui, sx, sy, x, y)
    })
}

/// The mouse buttons as `GuiScreen` numbers them; the pick block key is the middle button,
/// `keyBindPickBlock.getKeyCode() + 100`.
pub const PICK: i32 = 2;

/// `GuiContainer.mouseClicked`
#[allow(clippy::too_many_arguments)]
pub fn mouse_clicked<S: Screen>(s: &mut S, inv: &mut InventoryPlayer, out: &mut Vec<Sent>, gui: &Gui, x: i32, y: i32, button: i32, shift: bool, now: f64) {
    let pick = button == PICK;
    let slot = slot_at(s, gui, x, y);
    let b = s.base_mut();
    b.double_click = b.last_click_slot == slot && now - b.last_click_time < 250.0 && b.last_click_button == button;
    b.ignore_mouse_up = false;
    if button == 0 || button == 1 || pick {
        let (xs, ys) = s.size();
        let outside = clicked_outside(gui, xs, ys, x, y);
        let mut l = slot.map_or(-1, |i| s.slot_number(i));
        if outside {
            l = -999;
        }
        if l != -1 && !s.base().drag_splitting {
            if inv.cursor.is_empty() {
                if pick {
                    s.handle_mouse_click(inv, out, slot, l, button, ClickType::Clone);
                } else {
                    let flag2 = l != -999 && shift;
                    let mut click = ClickType::Pickup;
                    if flag2 {
                        let shifted = match slot {
                            Some(i) => s.stack(inv, i),
                            None => ItemStack::EMPTY,
                        };
                        s.base_mut().shift_clicked_slot = shifted.copy();
                        click = ClickType::QuickMove;
                    } else if l == -999 {
                        click = ClickType::Throw;
                    }
                    s.handle_mouse_click(inv, out, slot, l, button, click);
                }
                s.base_mut().ignore_mouse_up = true;
            } else {
                let b = s.base_mut();
                b.drag_splitting = true;
                b.drag_split_button = button;
                b.drag_slots.clear();
                match button {
                    0 => b.drag_split_limit = 0,
                    1 => b.drag_split_limit = 1,
                    PICK => b.drag_split_limit = 2,
                    _ => {}
                }
            }
        }
    }
    let b = s.base_mut();
    b.last_click_slot = slot;
    b.last_click_time = now;
    b.last_click_button = button;
}

/// `GuiContainer.mouseClickMove`, called each frame a button is held.
pub fn mouse_click_move<S: Screen>(s: &mut S, inv: &mut InventoryPlayer, gui: &Gui, x: i32, y: i32) {
    let Some(i) = slot_at(s, gui, x, y) else { return };
    let cursor = inv.cursor;
    let b = s.base();
    if b.drag_splitting
        && !cursor.is_empty()
        && (cursor.get_count() > b.drag_slots.len() as i32 || b.drag_split_limit == 2)
        && can_add_item_to_slot(&s.stack(inv, i), &cursor, true)
        && s.is_item_valid(i, &cursor)
        && s.can_drag_into(i)
    {
        if !s.base().drag_slots.contains(&i) {
            s.base_mut().drag_slots.push(i);
        }
        update_drag_splitting(s, inv);
    }
}

/// `GuiContainer.updateDragSplitting`
fn update_drag_splitting<S: Screen>(s: &mut S, inv: &mut InventoryPlayer) {
    let cursor = inv.cursor;
    if cursor.is_empty() || !s.base().drag_splitting {
        return;
    }
    if s.base().drag_split_limit == 2 {
        s.base_mut().remnant = cursor.get_max_stack_size();
        return;
    }
    let mut remnant = cursor.get_count();
    let (n, limit) = (s.base().drag_slots.len(), s.base().drag_split_limit);
    for i in s.base().drag_slots.clone() {
        let mut itemstack1 = cursor.copy();
        let itemstack2 = s.stack(inv, i);
        let k = if itemstack2.is_empty() { 0 } else { itemstack2.get_count() };
        compute_stack_size(n, limit, &mut itemstack1, k);
        let j = itemstack1.get_max_stack_size().min(s.item_stack_limit(i, &itemstack1));
        if itemstack1.get_count() > j {
            itemstack1.set_count(j);
        }
        remnant -= itemstack1.get_count() - k;
    }
    s.base_mut().remnant = remnant;
}

/// `GuiContainer.mouseReleased`
#[allow(clippy::too_many_arguments)]
pub fn mouse_released<S: Screen>(s: &mut S, inv: &mut InventoryPlayer, out: &mut Vec<Sent>, gui: &Gui, x: i32, y: i32, state: i32, shift: bool) {
    let slot = slot_at(s, gui, x, y);
    let (xs, ys) = s.size();
    let mut k = slot.map_or(-1, |i| s.slot_number(i));
    if clicked_outside(gui, xs, ys, x, y) {
        k = -999;
    }
    if let Some(si) = slot.filter(|&si| s.base().double_click && state == 0 && s.can_merge(si)) {
        if shift {
            let shifted = s.base().shift_clicked_slot;
            if !shifted.is_empty() {
                for j in 0..s.slot_count() {
                    if s.can_take(inv, j) && !s.stack(inv, j).is_empty() && s.same_inventory(j, si) && can_add_item_to_slot(&s.stack(inv, j), &shifted, true) {
                        let n = s.slot_number(j);
                        s.handle_mouse_click(inv, out, Some(j), n, state, ClickType::QuickMove);
                    }
                }
            }
        } else {
            s.handle_mouse_click(inv, out, slot, k, state, ClickType::PickupAll);
        }
        let b = s.base_mut();
        b.double_click = false;
        b.last_click_time = 0.0;
    } else {
        let b = s.base_mut();
        if b.drag_splitting && b.drag_split_button != state {
            b.drag_splitting = false;
            b.drag_slots.clear();
            b.ignore_mouse_up = true;
            return;
        }
        if b.ignore_mouse_up {
            b.ignore_mouse_up = false;
            return;
        }
        if b.drag_splitting && !b.drag_slots.is_empty() {
            let limit = b.drag_split_limit;
            s.handle_mouse_click(inv, out, None, -999, get_quickcraft_mask(0, limit), ClickType::QuickCraft);
            for j in s.base().drag_slots.clone() {
                let n = s.slot_number(j);
                s.handle_mouse_click(inv, out, Some(j), n, get_quickcraft_mask(1, limit), ClickType::QuickCraft);
            }
            s.handle_mouse_click(inv, out, None, -999, get_quickcraft_mask(2, limit), ClickType::QuickCraft);
        } else if !inv.cursor.is_empty() {
            if state == PICK {
                s.handle_mouse_click(inv, out, slot, k, state, ClickType::Clone);
            } else {
                let flag1 = k != -999 && shift;
                if flag1 {
                    let shifted = match slot {
                        Some(i) => s.stack(inv, i),
                        None => ItemStack::EMPTY,
                    };
                    s.base_mut().shift_clicked_slot = shifted.copy();
                }
                s.handle_mouse_click(inv, out, slot, k, state, if flag1 { ClickType::QuickMove } else { ClickType::Pickup });
            }
        }
    }
    if inv.cursor.is_empty() {
        s.base_mut().last_click_time = 0.0;
    }
    s.base_mut().drag_splitting = false;
}

/// `GuiContainer.checkHotbarKeys`: a number key swaps the hovered slot with that hotbar slot.
pub fn check_hotbar_keys<S: Screen>(s: &mut S, inv: &mut InventoryPlayer, out: &mut Vec<Sent>, digit: Option<usize>) -> bool {
    let (Some(d), Some(h)) = (digit, s.base().hovered) else { return false };
    if !inv.cursor.is_empty() {
        return false;
    }
    let n = s.slot_number(h);
    s.handle_mouse_click(inv, out, Some(h), n, d as i32, ClickType::Swap);
    true
}

/// `GuiContainer.drawScreen` between the background and the foreground: every slot
/// (`drawSlot`), and the white wash over the hovered one.
pub fn draw_slots<S: Screen>(s: &mut S, frame: &mut Frame, font: &Font, inv: &mut InventoryPlayer, gui: &Gui, mx: i32, my: i32) {
    s.base_mut().hovered = None;
    for i in 0..s.slot_count() {
        draw_slot(s, frame, font, inv, gui, i);
        let (sx, sy) = s.slot_pos(i);
        if over_slot(gui, sx, sy, mx, my) {
            s.base_mut().hovered = Some(i);
            let (x, y) = (gui.left + sx, gui.top + sy);
            frame.bands[draw::HIGHLIGHT].gradient(x, y, x + 16, y + 16, 0x80FF_FFFF, 0x80FF_FFFF);
        }
    }
}

/// `GuiContainer.drawSlot`
fn draw_slot<S: Screen>(s: &mut S, frame: &mut Frame, font: &Font, inv: &mut InventoryPlayer, gui: &Gui, i: usize) {
    let (sx, sy) = s.slot_pos(i);
    let (x, y) = (gui.left + sx, gui.top + sy);
    let mut itemstack = s.stack(inv, i);
    let mut flag = false;
    let itemstack1 = inv.cursor;
    let mut text = None;
    if s.base().drag_splitting && s.base().drag_slots.contains(&i) && !itemstack1.is_empty() {
        if s.base().drag_slots.len() == 1 {
            return;
        }
        if can_add_item_to_slot(&itemstack, &itemstack1, true) && s.can_drag_into(i) {
            let in_slot = itemstack;
            itemstack = itemstack1.copy();
            flag = true;
            let n = s.base().drag_slots.len();
            compute_stack_size(n, s.base().drag_split_limit, &mut itemstack, if in_slot.is_empty() { 0 } else { in_slot.get_count() });
            let k = itemstack.get_max_stack_size().min(s.item_stack_limit(i, &itemstack));
            if itemstack.get_count() > k {
                text = Some((k.to_string(), YELLOW));
                itemstack.set_count(k);
            }
        } else {
            s.base_mut().drag_slots.retain(|&j| j != i);
            update_drag_splitting(s, inv);
        }
    }
    if itemstack.is_empty() {
        if let Some(t) = s.slot_texture(i) {
            let q = &mut frame.bands[draw::EMPTY_SLOTS];
            let du = 1.0 / draw::EMPTY_SLOT_SPRITES as f32;
            let u = t as f32 * du;
            q.quad([x as f32, y as f32, (x + 16) as f32, (y + 16) as f32], [u, 0.0, u + du, 1.0], 0xFFFF_FFFF, 0xFFFF_FFFF);
            return;
        }
    }
    if flag {
        frame.bands[draw::UNDER].rect(x, y, x + 16, y + 16, 0x80FF_FFFF);
    }
    frame.icons.push(draw::Icon { stack: itemstack, x, y, z: draw::SLOT_Z });
    frame.overlay(font, &itemstack, x, y, text, draw::COUNTS, draw::BARS);
}

/// The end of `GuiContainer.drawScreen`: the cursor stack under the mouse, showing what a drag
/// split leaves on it.
pub fn draw_cursor<S: Screen>(s: &S, frame: &mut Frame, font: &Font, inv: &InventoryPlayer, mx: i32, my: i32) {
    let mut itemstack = inv.cursor;
    if itemstack.is_empty() {
        return;
    }
    let mut text = None;
    let b = s.base();
    if b.drag_splitting && b.drag_slots.len() > 1 {
        itemstack.set_count(b.remnant);
        if itemstack.is_empty() {
            text = Some(("0".to_string(), YELLOW));
        }
    }
    let (x, y) = (mx - 8, my - 8);
    frame.icons.push(draw::Icon { stack: itemstack, x, y, z: draw::CURSOR_Z });
    frame.overlay(font, &itemstack, x, y, text, draw::CURSOR_COUNT, draw::CURSOR_BARS);
}
