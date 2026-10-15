//! T8: `net.minecraft.inventory.ContainerFurnace`, `SlotFurnaceFuel` and `SlotFurnaceOutput`.
//!
//! Slots: 0 input, 1 fuel (`SlotFurnaceFuel`), 2 output (`SlotFurnaceOutput`), 3..30 the main
//! inventory, 30..39 the hotbar. The four furnace fields reach the viewer as window properties
//! ([`Container::prop_listener`]): 0 burn time, 1 current item burn time, 2 cook time, 3 total
//! cook time (`TileEntityFurnace.getField`).
//!
//! `SlotFurnaceOutput`'s `removeCount` and `onCrafting` are `Container::furnace_remove_count` and
//! `Container::furnace_xp`; the server spawns the orbs.

use items::smelting::{get_smelting_result, is_item_fuel};
use items::stack::ItemStack;
use items::Furnace;

use super::{Container, ContainerKind, Inv, PlayerRef, SlotKind, Tile};

impl Container {
    /// `new ContainerFurnace(playerInventory, furnaceInventory)`. A fresh viewer has every field
    /// at 0, so the first `detectAndSendChanges` (`addListener`'s `sendAllWindowProperties`)
    /// sends the ones that are not.
    pub fn furnace(window_id: i32, pos: [i32; 3], furnace: Furnace) -> Container {
        let mut c = Container::empty(ContainerKind::Furnace { pos }, window_id, None);
        c.tile = Some(Tile::Furnace(furnace));
        c.add_slot_to_container(Inv::Tile, 0, 56, 17, SlotKind::Plain);
        c.add_slot_to_container(Inv::Tile, 1, 56, 53, SlotKind::FurnaceFuel);
        c.add_slot_to_container(Inv::Tile, 2, 116, 35, SlotKind::FurnaceOutput);
        for i in 0..3 {
            for j in 0..9 {
                c.add_slot_to_container(Inv::Player, j + i * 9 + 9, 8 + j as i32 * 18, 84 + i as i32 * 18, SlotKind::Plain);
            }
        }
        for k in 0..9 {
            c.add_slot_to_container(Inv::Player, k, 8 + k as i32 * 18, 142, SlotKind::Plain);
        }
        c
    }

    /// `ContainerFurnace.transferStackInSlot(player, index)`
    pub(crate) fn transfer_furnace(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        if !self.get_has_stack(p, index) {
            return ItemStack::EMPTY;
        }
        let mut itemstack1 = self.get_stack(p, index);
        let itemstack = itemstack1.copy();
        if index == 2 {
            if !self.merge_item_stack(p, &mut itemstack1, 3, 39, true) {
                return ItemStack::EMPTY;
            }
            self.on_slot_change(index, &itemstack1, &itemstack);
        } else if index != 1 && index != 0 {
            if !get_smelting_result(&itemstack1).is_empty() {
                if !self.merge_item_stack(p, &mut itemstack1, 0, 1, false) {
                    return ItemStack::EMPTY;
                }
            } else if is_item_fuel(&itemstack1) {
                if !self.merge_item_stack(p, &mut itemstack1, 1, 2, false) {
                    return ItemStack::EMPTY;
                }
            } else if (3..30).contains(&index) {
                if !self.merge_item_stack(p, &mut itemstack1, 30, 39, false) {
                    return ItemStack::EMPTY;
                }
            } else if (30..39).contains(&index) && !self.merge_item_stack(p, &mut itemstack1, 3, 30, false) {
                return ItemStack::EMPTY;
            }
        } else if !self.merge_item_stack(p, &mut itemstack1, 3, 39, false) {
            return ItemStack::EMPTY;
        }
        self.finish_transfer(p, index, itemstack1, itemstack)
    }
}
