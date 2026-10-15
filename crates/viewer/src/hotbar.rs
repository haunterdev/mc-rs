//! The player's inventory as the viewer holds it: `sim::inventory::InventoryPlayer` (the slots,
//! `currentItem`, `getSlotFor`, `pickItem`, `setPickedItemStack`), the wheel and number keys
//! that change the selected slot, and `GuiIngame.renderHotbar`'s bar and selection frame.
//!
//! The hotbar starts filled with building blocks; in creative mode E opens the creative inventory
//! (`survival/gui/creative.rs`), and the middle click (`interact.rs`) puts any block in the world
//! into it.

use std::ops::{Deref, DerefMut};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use items::ids::item;
use items::{registry, ItemStack, Kind};
use sim::inventory::InventoryPlayer;
use worldgen::blocks::id;

use crate::hand::{gui_scale, HideGui};
use crate::settings::Menu;

/// `InventoryPlayer` (through `Deref`), plus the input a tick has not handled yet.
#[derive(Resource)]
pub struct Inventory {
    pub player: InventoryPlayer,
    /// Wheel notches a tick has not handled yet, up positive.
    pub scroll: i32,
    /// Presses of the hotbar keys 1 to 9 not yet handled (`KeyBinding.pressTime`).
    pub digits: [u32; 9],
}

/// The hotbar a new player starts with, as block ids (each its own `ItemBlock` id): stone,
/// torches, dirt, cobblestone, oak planks, oak log, glass, stone bricks, bricks. Grass is picked
/// from the ground.
pub const START: [u16; 9] = [id::STONE, id::TORCH, id::DIRT, id::COBBLESTONE, id::PLANKS, id::LOG, id::GLASS, id::STONEBRICK, id::BRICK_BLOCK];

impl Default for Inventory {
    fn default() -> Inventory {
        let mut player = InventoryPlayer::new();
        for (i, b) in START.into_iter().enumerate() {
            player.main[i] = block_stack(b, 0);
        }
        Inventory { player, scroll: 0, digits: [0; 9] }
    }
}

impl Deref for Inventory {
    type Target = InventoryPlayer;
    fn deref(&self) -> &InventoryPlayer {
        &self.player
    }
}

impl DerefMut for Inventory {
    fn deref_mut(&mut self) -> &mut InventoryPlayer {
        &mut self.player
    }
}

/// `Block.getItem` as a stack of one: the block's `ItemBlock` (`Item.getItemFromBlock`), else the
/// `ItemBlockSpecial` that places it (reeds). Empty when no item places the block.
pub fn block_stack(block: u16, meta: u16) -> ItemStack {
    let mut it = registry::item_from_block(block);
    if it == item::AIR {
        it = registry::all_items().iter().find(|d| matches!(d.kind, Kind::Places(b) if b == block)).map_or(item::AIR, |d| d.id);
    }
    ItemStack::new(it, 1, meta as i32)
}

/// The block a stack's item places and the stack's metadata (`Block.getBlockFromItem`, with
/// `ItemBlockSpecial`'s block): the block item's model key and what it places. `None` for an
/// empty stack or an item that places no block.
pub fn placed_block(stack: &ItemStack) -> Option<(u16, u16)> {
    if stack.is_empty() {
        return None;
    }
    match stack.def().kind {
        Kind::Block { block, .. } | Kind::Places(block) => Some((block, stack.get_metadata() as u16)),
        _ => None,
    }
}

/// The held stack as the block state `PlayerState.held` carries (0 for none).
pub fn held_state(stack: &ItemStack) -> u16 {
    placed_block(stack).map_or(0, |(b, m)| b << 4 | m)
}

/// `GameSettings.keyBindsHotbar`: the number keys queue presses for the next tick.
pub fn keys(keys: Res<ButtonInput<KeyCode>>, menu: Res<Menu>, mut inv: ResMut<Inventory>) {
    if menu.open {
        return;
    }
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (i, k) in DIGITS.into_iter().enumerate() {
        if keys.just_pressed(k) {
            inv.digits[i] += 1;
        }
    }
}

/// `textures/gui/widgets.png`
#[derive(Resource)]
pub struct GuiWidgets(pub Handle<Image>);

/// One sprite of the hotbar.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum HotbarPart {
    Bar,
    Selection,
    /// `GuiIngame.renderHotbar`: the off hand's slot, to the left of the bar.
    Offhand,
}

pub fn spawn_hotbar(mut commands: Commands, widgets: Res<GuiWidgets>) {
    for (part, z) in [(HotbarPart::Bar, 0), (HotbarPart::Selection, 1), (HotbarPart::Offhand, 0)] {
        commands.spawn((
            part,
            ImageNode::new(widgets.0.clone()),
            Node { position_type: PositionType::Absolute, ..default() },
            ZIndex(z),
            Visibility::Hidden,
        ));
    }
}

/// `GuiIngame.renderHotbar`: the bar, and the frame over the selected slot.
pub fn update_hotbar(
    window: Single<&Window, With<PrimaryWindow>>,
    inv: Res<Inventory>,
    hide: Res<HideGui>,
    mut parts: Query<(&HotbarPart, &mut Node, &mut ImageNode, &mut Visibility)>,
) {
    let (w, h) = (window.physical_width(), window.physical_height());
    let k = gui_scale(w, h);
    let sw = (w as f64 / k as f64).ceil() as i32;
    let sh = (h as f64 / k as f64).ceil() as i32;
    let unit = k as f32 / window.scale_factor();
    let i = sw / 2;
    // The off hand is drawn on the left for a right-handed player (`getPrimaryHand().opposite()`).
    let offhand = !inv.get_stack_in_slot(40).is_empty();
    for (part, mut node, mut image, mut visibility) in &mut parts {
        let (x, y, u, v, width, height) = match part {
            HotbarPart::Bar => (i - 91, sh - 22, 0, 0, 182, 22),
            HotbarPart::Selection => (i - 91 - 1 + inv.current_item as i32 * 20, sh - 22 - 1, 0, 22, 24, 22),
            HotbarPart::Offhand => (i - 91 - 29, sh - 23, 24, 22, 29, 24),
        };
        let shown = !hide.0 && (*part != HotbarPart::Offhand || offhand);
        *visibility = if shown { Visibility::Inherited } else { Visibility::Hidden };
        node.left = px(x as f32 * unit);
        node.top = px(y as f32 * unit);
        node.width = px(width as f32 * unit);
        node.height = px(height as f32 * unit);
        let rect = Rect::new(u as f32, v as f32, (u + width) as f32, (v + height) as f32);
        if image.rect != Some(rect) {
            image.rect = Some(rect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the creative middle click hands `setPickedItemStack`.
    fn picked(inv: &mut Inventory, block: u16, meta: u16) {
        inv.set_picked_item_stack(block_stack(block, meta));
    }

    /// The held stack's block, as the old block-keyed stack read.
    fn held_block(inv: &Inventory) -> Option<u16> {
        placed_block(&inv.get_current_item()).map(|(b, _)| b)
    }

    #[test]
    fn the_wheel_wraps() {
        let mut inv = Inventory::default();
        // Up (positive) moves left.
        inv.change_current_item(120);
        assert_eq!(inv.current_item, 8);
        inv.change_current_item(-1);
        inv.change_current_item(-1);
        assert_eq!(inv.current_item, 1);
    }

    #[test]
    fn picking_selects_swaps_or_replaces() {
        let mut inv = Inventory::default();
        // In the hotbar: selected.
        picked(&mut inv, id::GLASS, 0);
        assert_eq!(inv.current_item, 6);
        // New, with a full hotbar: replaces the current slot and keeps the old stack further in.
        let glass = inv.get_current_item();
        picked(&mut inv, id::SAND, 0);
        assert_eq!(inv.current_item, 6);
        assert_eq!(held_block(&inv), Some(id::SAND));
        assert_eq!(inv.main[9], glass);
        // In the main inventory: swapped into the current slot.
        picked(&mut inv, id::GLASS, 0);
        assert_eq!(inv.get_current_item(), glass);
        assert_eq!(placed_block(&inv.main[9]).map(|(b, _)| b), Some(id::SAND));
        // An empty hotbar slot ahead of the current one, wrapping, is taken first.
        inv.main[2] = ItemStack::EMPTY;
        picked(&mut inv, id::CLAY, 0);
        assert_eq!(inv.current_item, 2);
        // Metadata tells stacks apart.
        picked(&mut inv, id::STONE, 1);
        assert_ne!(placed_block(&inv.get_current_item()).map(|(_, m)| m), Some(0));
    }

    #[test]
    fn every_hotbar_key_is_a_stack_and_back() {
        // Every (block, meta) the hotbar can hold, the start set and every pick, becomes a
        // non-empty stack whose placed block and metadata are the key again.
        for key in crate::item::block_keys() {
            let s = block_stack(key.0, key.1);
            assert!(!s.is_empty(), "no item for {key:?}");
            assert_eq!(placed_block(&s), Some(key));
        }
        // Reeds are an ItemBlockSpecial: item 338, not block 83's id.
        assert_eq!(block_stack(id::REEDS, 0).item, item::REEDS);
        assert_eq!(held_state(&block_stack(id::REEDS, 0)), id::REEDS << 4);
        // A tool places nothing.
        assert_eq!(placed_block(&ItemStack::of(item::DIAMOND_SWORD)), None);
        assert_eq!(held_state(&ItemStack::EMPTY), 0);
    }
}
