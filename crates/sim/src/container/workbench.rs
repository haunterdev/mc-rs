//! T7a: `net.minecraft.inventory.ContainerWorkbench`, the crafting table's 3x3 window.
//!
//! Slots: 0 the crafting result, 1..10 the grid (`j + i * 3`), 10..37 the main inventory
//! (`InventoryPlayer` 9..36), 37..46 the hotbar (0..9). Closing gives the grid back
//! (`clearContainer`); the result is not cleared, as in Java, since the container is thrown away.

use std::sync::Arc;

use items::ids::block;
use items::stack::ItemStack;
use items::Recipes;

use super::player::Crafting;
use super::{Container, ContainerKind, Inv, PlayerRef, SlotKind};

impl Container {
    /// `new ContainerWorkbench(playerInventory, world, pos)`
    pub fn workbench(window_id: i32, pos: [i32; 3], recipes: Arc<Recipes>) -> Container {
        let mut c = Container::empty(ContainerKind::Workbench { pos }, window_id, Some(Crafting::new(3, 3, recipes)));
        c.add_slot_to_container(Inv::Result, 0, 124, 35, SlotKind::Crafting);
        for i in 0..3 {
            for j in 0..3 {
                c.add_slot_to_container(Inv::Craft, j + i * 3, 30 + j as i32 * 18, 17 + i as i32 * 18, SlotKind::Plain);
            }
        }
        for k in 0..3 {
            for i1 in 0..9 {
                c.add_slot_to_container(Inv::Player, i1 + k * 9 + 9, 8 + i1 as i32 * 18, 84 + k as i32 * 18, SlotKind::Plain);
            }
        }
        for l in 0..9 {
            c.add_slot_to_container(Inv::Player, l, 8 + l as i32 * 18, 142, SlotKind::Plain);
        }
        c
    }

    /// `ContainerWorkbench.canInteractWith(player)`: the block at `pos` is still a crafting
    /// table and the player's position is within 8 blocks of its centre. `block_at_pos` is the
    /// block id there; `player_pos` is the player's feet (`getDistanceSq` uses `posX/Y/Z`).
    /// `ContainerPlayer` always answers true.
    pub fn can_interact_with(&self, block_at_pos: u16, player_pos: [f64; 3]) -> bool {
        match self.kind {
            ContainerKind::Player | ContainerKind::Creative => true,
            ContainerKind::Workbench { pos } => {
                if block_at_pos != block::CRAFTING_TABLE {
                    return false;
                }
                let dx = player_pos[0] - (pos[0] as f64 + 0.5);
                let dy = player_pos[1] - (pos[1] as f64 + 0.5);
                let dz = player_pos[2] - (pos[2] as f64 + 0.5);
                dx * dx + dy * dy + dz * dz <= 64.0
            }
        }
    }

    /// `ContainerWorkbench.transferStackInSlot(player, index)`. DROPPED: the result's
    /// `Item.onCreated` (stats and the map hook, see `player.rs`).
    pub(crate) fn transfer_workbench(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        if !self.get_has_stack(p, index) {
            return ItemStack::EMPTY;
        }
        let mut itemstack1 = self.get_stack(p, index);
        let itemstack = itemstack1.copy();
        let (start, end, reverse) = if index == 0 {
            (10, 46, true)
        } else if (10..37).contains(&index) {
            (37, 46, false)
        } else if (37..46).contains(&index) {
            (10, 37, false)
        } else {
            (10, 46, false)
        };
        if !self.merge_item_stack(p, &mut itemstack1, start, end, reverse) {
            return ItemStack::EMPTY;
        }
        if index == 0 {
            self.on_slot_change(index, &itemstack1, &itemstack);
        }
        self.finish_transfer(p, index, itemstack1, itemstack)
    }
}
