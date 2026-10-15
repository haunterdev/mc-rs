//! T8: `net.minecraft.inventory.ContainerChest` over a `TileEntityChest` (27 slots) or an
//! `InventoryLargeChest` (54: the first chest's 27, then the second's).
//!
//! Slots: 0..rows*9 the chest, then the main inventory (`InventoryPlayer` 9..36) and the hotbar.
//! `InventoryLargeChest` is only a view of two tiles; here [`Tile::Chest`] holds the stacks of
//! both halves in order and the server maps them back to the tiles.
//!
//! DROPPED: `getLockCode`/`isLocked` (no key locks), `ILockableContainer` names (the title is
//! the server's), `IInventory.openInventory/closeInventory` (the server counts the lid users).

use items::stack::ItemStack;

use super::{Container, ContainerKind, Inv, PlayerRef, SlotKind, Tile};

impl Container {
    /// `new ContainerChest(playerInventory, chestInventory, player)`: `contents` is 27 or 54
    /// stacks, `pos` the clicked chest and `other` the second half of a large one.
    pub fn chest(window_id: i32, pos: [i32; 3], other: Option<[i32; 3]>, contents: Vec<ItemStack>) -> Container {
        let rows = contents.len() / 9;
        let mut c = Container::empty(ContainerKind::Chest { pos, other, rows: rows as u8 }, window_id, None);
        c.tile = Some(Tile::Chest(contents));
        let i = (rows as i32 - 4) * 18;
        for j in 0..rows {
            for k in 0..9 {
                c.add_slot_to_container(Inv::Tile, k + j * 9, 8 + k as i32 * 18, 18 + j as i32 * 18, SlotKind::Plain);
            }
        }
        for l in 0..3 {
            for j1 in 0..9 {
                c.add_slot_to_container(Inv::Player, j1 + l * 9 + 9, 8 + j1 as i32 * 18, 103 + l as i32 * 18 + i, SlotKind::Plain);
            }
        }
        for i1 in 0..9 {
            c.add_slot_to_container(Inv::Player, i1, 8 + i1 as i32 * 18, 161 + i, SlotKind::Plain);
        }
        c
    }

    /// `ContainerChest.getNumRows()`
    pub fn chest_rows(&self) -> usize {
        match self.kind {
            ContainerKind::Chest { rows, .. } => rows as usize,
            _ => 0,
        }
    }

    /// `ContainerChest.transferStackInSlot(player, index)`
    pub(crate) fn transfer_chest(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        if !self.get_has_stack(p, index) {
            return ItemStack::EMPTY;
        }
        let mut itemstack1 = self.get_stack(p, index);
        let itemstack = itemstack1.copy();
        let n = self.chest_rows() * 9;
        if index < n {
            if !self.merge_item_stack(p, &mut itemstack1, n, self.slots.len(), true) {
                return ItemStack::EMPTY;
            }
        } else if !self.merge_item_stack(p, &mut itemstack1, 0, n, false) {
            return ItemStack::EMPTY;
        }
        self.finish_transfer(p, index, itemstack1, itemstack)
    }
}
