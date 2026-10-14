//! T7a: `net.minecraft.inventory.Container` (`slotClick` for every `ClickType`, `mergeItemStack`,
//! the drag helpers, `detectAndSendChanges`, `onContainerClosed`) and `Slot`, shared by the
//! server copy of a window and the viewer's predicting copy. Both copies run the same code on the
//! same clicks, so they stay equal.
//!
//! Java's `Slot` points at an `IInventory`; here a [`Slot`] names one of the inventories a window
//! sees ([`Inv`]): the player's `InventoryPlayer` (borrowed per click through [`PlayerRef`]) or
//! the crafting matrix and result the container owns ([`player::Crafting`]). The slot subclasses
//! are [`SlotKind`]: plain `Slot`, `SlotCrafting` and `ContainerPlayer`'s armor slots. The window
//! types are [`ContainerKind`], each with its own `transferStackInSlot` in its file.
//!
//! Java mutates `ItemStack` objects in place; `items::ItemStack` is a value. Where Java grows or
//! shrinks the stack a slot holds without `setInventorySlotContents`, this writes the new value
//! back with [`Container::set_raw`], which fires no `onCraftMatrixChanged` and keeps the pickup
//! bob, as the in-place change does. Every `player.dropItem` pushes the stack onto
//! [`PlayerRef::dropped`]; the caller spawns the items.
//!
//! DROPPED: `IContainerListener` (the slot changes `detectAndSendChanges` finds go into
//! [`Container::listener`] for the server's window code to send), `getSlotFromInventory`,
//! `calcRedstone`, `updateProgressBar`, `setAll`, the client's `transactionID`, and every
//! `Slot.onSlotChanged` call, which is `IInventory.markDirty()`: a no-op for `InventoryCrafting`
//! and `InventoryCraftResult`, and `InventoryPlayer.timesChanged++` (dropped with T3).

pub mod chest;
pub mod furnace;
pub mod player;
pub mod workbench;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_craft;

use items::stack::{are_item_stack_tags_equal, are_items_equal, ItemStack};

use crate::inventory::InventoryPlayer;
use player::{Crafting, EquipmentSlot};

/// `net.minecraft.inventory.ClickType`, in ordinal order (the `mode` byte of `V_CLICK`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClickType {
    Pickup,
    QuickMove,
    Swap,
    Clone,
    Throw,
    QuickCraft,
    PickupAll,
}

impl ClickType {
    /// `ClickType.values()[ordinal]`
    pub fn from_ordinal(n: u8) -> Option<ClickType> {
        use ClickType::*;
        [Pickup, QuickMove, Swap, Clone, Throw, QuickCraft, PickupAll].get(n as usize).copied()
    }

    /// `ordinal()`
    pub fn ordinal(self) -> u8 {
        self as u8
    }
}

/// The `IInventory` a [`Slot`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Inv {
    /// `InventoryPlayer`: main 0..36, armor 36..40, off hand 40.
    Player,
    /// `InventoryCrafting`, the container's `craftMatrix`.
    Craft,
    /// `InventoryCraftResult`, the container's `craftResult`.
    Result,
}

/// Which `Slot` class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotKind {
    /// `net.minecraft.inventory.Slot`
    Plain,
    /// `SlotCrafting`
    Crafting,
    /// `ContainerPlayer`'s anonymous armor slot for this `EntityEquipmentSlot`.
    Armor(EquipmentSlot),
}

/// `net.minecraft.inventory.Slot`: an index into an inventory and its place in the GUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// `inventory`
    pub inventory: Inv,
    /// `slotIndex`
    pub index: usize,
    /// `slotNumber`, the slot's place in `inventorySlots`.
    pub slot_number: usize,
    /// `xPos`, `yPos` in the GUI's unscaled pixels.
    pub x: i32,
    pub y: i32,
    pub kind: SlotKind,
}

/// The `EntityPlayer` a click or close acts for: the inventory, `capabilities.isCreativeMode`,
/// `isEntityAlive()` (and not disconnected, for `clearContainer`), and the stacks
/// `EntityPlayer.dropItem` threw, in order.
#[derive(Debug)]
pub struct PlayerRef<'a> {
    pub inventory: &'a mut InventoryPlayer,
    pub creative: bool,
    pub alive: bool,
    pub dropped: Vec<ItemStack>,
}

impl<'a> PlayerRef<'a> {
    pub fn new(inventory: &'a mut InventoryPlayer, creative: bool) -> Self {
        PlayerRef { inventory, creative, alive: true, dropped: Vec::new() }
    }

    /// `EntityPlayer.dropItem(stack, ...)`: nothing for an empty stack. DROPPED: the
    /// `traceItem` flag (the thrower's name on the item), which only `dropItem(stack, true)`
    /// sets; the caller knows which player threw.
    pub fn drop_item(&mut self, stack: ItemStack) {
        if !stack.is_empty() {
            self.dropped.push(stack);
        }
    }
}

/// The `Container` subclass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    /// `ContainerPlayer`, window 0.
    Player,
    /// `ContainerWorkbench` at the crafting table's block position.
    Workbench { pos: [i32; 3] },
}

/// `net.minecraft.inventory.Container`
#[derive(Clone, Debug)]
pub struct Container {
    pub kind: ContainerKind,
    /// `windowId`
    pub window_id: i32,
    /// `inventorySlots`
    pub slots: Vec<Slot>,
    /// `inventoryItemStacks`: what `detectAndSendChanges` last saw.
    pub inventory_item_stacks: Vec<ItemStack>,
    /// What `detectAndSendChanges` would have sent (`IContainerListener.sendSlotContents`), in
    /// order; the server's window code drains it.
    pub listener: Vec<(usize, ItemStack)>,
    /// `dragMode`
    pub drag_mode: i32,
    /// `dragEvent`
    pub drag_event: i32,
    /// `dragSlots`, by slot number. Java's is a `HashSet<Slot>` iterated in identity-hash
    /// order; the stage-2 loop gives every slot the same share whatever the order, so insertion
    /// order stands in.
    pub drag_slots: Vec<usize>,
    /// `craftMatrix`, `craftResult` and `SlotCrafting.amountCrafted` of the windows that craft.
    pub crafting: Option<Crafting>,
    /// `getCanCraft(player)`: the player is not in `playerList`. A container here belongs to
    /// one player.
    pub can_craft: bool,
}

impl Container {
    /// An empty container: `dragMode` -1, nothing in `inventorySlots`.
    pub(crate) fn empty(kind: ContainerKind, window_id: i32, crafting: Option<Crafting>) -> Container {
        Container {
            kind,
            window_id,
            slots: Vec::new(),
            inventory_item_stacks: Vec::new(),
            listener: Vec::new(),
            drag_mode: -1,
            drag_event: 0,
            drag_slots: Vec::new(),
            crafting,
            can_craft: true,
        }
    }

    /// `Container.addSlotToContainer(slot)`
    pub(crate) fn add_slot_to_container(&mut self, inventory: Inv, index: usize, x: i32, y: i32, kind: SlotKind) {
        let slot_number = self.slots.len();
        self.slots.push(Slot { inventory, index, slot_number, x, y, kind });
        self.inventory_item_stacks.push(ItemStack::EMPTY);
    }

    fn crafting(&self) -> &Crafting {
        self.crafting.as_ref().expect("a crafting slot in a container without a crafting matrix")
    }

    fn crafting_mut(&mut self) -> &mut Crafting {
        self.crafting.as_mut().expect("a crafting slot in a container without a crafting matrix")
    }

    /// `inventorySlots.get(slotId)` for an id in range; Java throws `IndexOutOfBoundsException`
    /// for any other, which the server's packet task swallows, so the click does nothing more.
    pub fn slot_index(&self, slot_id: i32) -> Option<usize> {
        (slot_id >= 0 && (slot_id as usize) < self.slots.len()).then_some(slot_id as usize)
    }

    // ---- IInventory, by slot ------------------------------------------------------------

    /// `IInventory.getStackInSlot(index)` of slot `i`'s inventory.
    fn inv_get(&self, p: &PlayerRef, inv: Inv, index: usize) -> ItemStack {
        match inv {
            Inv::Player => p.inventory.get_stack_in_slot(index),
            Inv::Craft => self.crafting().craft_matrix.get_stack_in_slot(index),
            Inv::Result => self.crafting().craft_result.get_stack_in_slot(index),
        }
    }

    /// `IInventory.setInventorySlotContents(index, stack)`. `InventoryCrafting`'s calls
    /// `eventHandler.onCraftMatrixChanged`.
    fn inv_set(&mut self, p: &mut PlayerRef, inv: Inv, index: usize, stack: ItemStack) {
        match inv {
            Inv::Player => p.inventory.set_inventory_slot_contents(index, stack),
            Inv::Craft => {
                self.crafting_mut().craft_matrix.set_inventory_slot_contents(index, stack);
                self.on_craft_matrix_changed();
            }
            Inv::Result => self.crafting_mut().craft_result.set_inventory_slot_contents(index, stack),
        }
    }

    /// `IInventory.decrStackSize(index, count)`. `InventoryCrafting`'s fires
    /// `onCraftMatrixChanged` when it took something; `InventoryCraftResult`'s takes the whole
    /// stack (`ItemStackHelper.getAndRemove`) whatever the count.
    fn inv_decr(&mut self, p: &mut PlayerRef, inv: Inv, index: usize, count: i32) -> ItemStack {
        match inv {
            Inv::Player => p.inventory.decr_stack_size(index, count),
            Inv::Craft => {
                let s = self.crafting_mut().craft_matrix.decr_stack_size(index, count);
                if !s.is_empty() {
                    self.on_craft_matrix_changed();
                }
                s
            }
            Inv::Result => self.crafting_mut().craft_result.decr_stack_size(index, count),
        }
    }

    /// `IInventory.getInventoryStackLimit()`: 64 for all three.
    fn inv_limit(&self, inv: Inv) -> i32 {
        match inv {
            Inv::Player | Inv::Craft | Inv::Result => 64,
        }
    }

    /// The stack in slot `i` changed in place (Java's `stack.grow`/`shrink`/`setCount` on the
    /// slot's own object): written back with no inventory event and the bob kept.
    pub(crate) fn set_raw(&mut self, p: &mut PlayerRef, i: usize, stack: ItemStack) {
        let s = self.slots[i];
        match s.inventory {
            Inv::Player => p.inventory.inv.set_inventory_slot_contents(s.index, stack),
            Inv::Craft => self.crafting_mut().craft_matrix.set_inventory_slot_contents(s.index, stack),
            Inv::Result => self.crafting_mut().craft_result.set_inventory_slot_contents(s.index, stack),
        }
    }

    /// `Container.onCraftMatrixChanged(inv)`: `ContainerPlayer` and `ContainerWorkbench` run
    /// `slotChangedCraftingGrid`. The base's `detectAndSendChanges` call is left to the caller,
    /// as no subclass here keeps it.
    pub fn on_craft_matrix_changed(&mut self) {
        if let Some(c) = &mut self.crafting {
            c.slot_changed_crafting_grid();
        }
    }

    // ---- Slot -----------------------------------------------------------------------------

    /// `Slot.getStack()`
    pub fn get_stack(&self, p: &PlayerRef, i: usize) -> ItemStack {
        let s = self.slots[i];
        self.inv_get(p, s.inventory, s.index)
    }

    /// `Slot.getHasStack()`
    pub fn get_has_stack(&self, p: &PlayerRef, i: usize) -> bool {
        !self.get_stack(p, i).is_empty()
    }

    /// `Slot.putStack(stack)`: `setInventorySlotContents`, then `onSlotChanged` (a no-op here).
    pub fn put_stack(&mut self, p: &mut PlayerRef, i: usize, stack: ItemStack) {
        let s = self.slots[i];
        self.inv_set(p, s.inventory, s.index, stack);
    }

    /// `Slot.decrStackSize(amount)`; `SlotCrafting.decrStackSize` first adds what it takes to
    /// `amountCrafted`.
    pub fn decr_stack_size(&mut self, p: &mut PlayerRef, i: usize, amount: i32) -> ItemStack {
        let s = self.slots[i];
        if s.kind == SlotKind::Crafting && self.get_has_stack(p, i) {
            let n = amount.min(self.get_stack(p, i).get_count());
            self.crafting_mut().amount_crafted += n;
        }
        self.inv_decr(p, s.inventory, s.index, amount)
    }

    /// `Slot.isItemValid(stack)`: `SlotCrafting` takes nothing; an armor slot only what
    /// `EntityLiving.getSlotForItemStack` puts there.
    pub fn is_item_valid(&self, i: usize, stack: &ItemStack) -> bool {
        match self.slots[i].kind {
            SlotKind::Plain => true,
            SlotKind::Crafting => false,
            SlotKind::Armor(e) => e == player::get_slot_for_item_stack(stack),
        }
    }

    /// `Slot.getSlotStackLimit()`: the inventory's limit; 1 in an armor slot.
    pub fn get_slot_stack_limit(&self, i: usize) -> i32 {
        match self.slots[i].kind {
            SlotKind::Armor(_) => 1,
            _ => self.inv_limit(self.slots[i].inventory),
        }
    }

    /// `Slot.getItemStackLimit(stack)`: `getSlotStackLimit()` for every slot here.
    pub fn get_item_stack_limit(&self, i: usize, _stack: &ItemStack) -> i32 {
        self.get_slot_stack_limit(i)
    }

    /// `Slot.canTakeStack(player)`: true. The armor slots refuse a stack with Curse of Binding
    /// outside creative, which is DROPPED with NBT (no stack carries enchantments).
    pub fn can_take_stack(&self, _p: &PlayerRef, _i: usize) -> bool {
        true
    }

    /// `Slot.onTake(player, stack)`; `SlotCrafting.onTake` crafts (see
    /// [`Container::slot_crafting_on_take`]).
    pub fn on_take(&mut self, p: &mut PlayerRef, i: usize, stack: ItemStack) -> ItemStack {
        match self.slots[i].kind {
            SlotKind::Crafting => self.slot_crafting_on_take(p, stack),
            _ => stack,
        }
    }

    /// `Slot.onSlotChange(oldStack, newStack)`: `onCrafting(newStack, grown)` when it grew.
    pub fn on_slot_change(&mut self, i: usize, old: &ItemStack, new: &ItemStack) {
        let n = new.get_count() - old.get_count();
        if n > 0 {
            self.on_crafting_amount(i, n);
        }
    }

    /// `Slot.onCrafting(stack, amount)`: only `SlotCrafting` does anything.
    fn on_crafting_amount(&mut self, i: usize, amount: i32) {
        if self.slots[i].kind == SlotKind::Crafting {
            self.crafting_mut().amount_crafted += amount;
            self.crafting_mut().on_crafting();
        }
    }

    /// `Slot.onSwapCraft(count)`: only `SlotCrafting` counts it.
    fn on_swap_craft(&mut self, i: usize, count: i32) {
        if self.slots[i].kind == SlotKind::Crafting {
            self.crafting_mut().amount_crafted += count;
        }
    }

    // ---- Container ------------------------------------------------------------------------

    /// `Container.getInventory()`: every slot's stack.
    pub fn get_inventory(&self, p: &PlayerRef) -> Vec<ItemStack> {
        (0..self.slots.len()).map(|i| self.get_stack(p, i)).collect()
    }

    /// `Container.detectAndSendChanges()`: every slot whose stack differs from what was last
    /// seen goes to [`Container::listener`].
    pub fn detect_and_send_changes(&mut self, p: &PlayerRef) {
        for i in 0..self.slots.len() {
            let itemstack = self.get_stack(p, i);
            if !items::stack::are_item_stacks_equal(&self.inventory_item_stacks[i], &itemstack) {
                let s1 = if itemstack.is_empty() { ItemStack::EMPTY } else { itemstack.copy() };
                self.inventory_item_stacks[i] = s1;
                self.listener.push((i, s1));
            }
        }
    }

    /// `Container.transferStackInSlot(player, index)`, the shift-click, per subclass.
    pub fn transfer_stack_in_slot(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        match self.kind {
            ContainerKind::Player => self.transfer_player(p, index),
            ContainerKind::Workbench { .. } => self.transfer_workbench(p, index),
        }
    }

    /// `Container.canMergeSlot(stack, slot)`: `ContainerPlayer` and `ContainerWorkbench` keep
    /// the double click out of the crafting result.
    pub fn can_merge_slot(&self, _stack: &ItemStack, i: usize) -> bool {
        match self.kind {
            ContainerKind::Player | ContainerKind::Workbench { .. } => self.slots[i].inventory != Inv::Result,
        }
    }

    /// `Container.canDragIntoSlot(slot)`: true for every container here.
    pub fn can_drag_into_slot(&self, _i: usize) -> bool {
        true
    }

    /// `Container.resetDrag()`
    pub fn reset_drag(&mut self) {
        self.drag_event = 0;
        self.drag_slots.clear();
    }

    /// `Container.slotClick(slotId, dragType, clickType, player)`: returns the clicked slot's
    /// stack before a PICKUP, or the last stack a QUICK_MOVE moved, which the server compares
    /// with the client's. Slot -999 is outside the window.
    pub fn slot_click(&mut self, slot_id: i32, drag_type: i32, click: ClickType, p: &mut PlayerRef) -> ItemStack {
        let mut itemstack = ItemStack::EMPTY;
        if click == ClickType::QuickCraft {
            let j1 = self.drag_event;
            self.drag_event = get_drag_event(drag_type);
            if (j1 != 1 || self.drag_event != 2) && j1 != self.drag_event {
                self.reset_drag();
            } else if p.inventory.cursor.is_empty() {
                self.reset_drag();
            } else if self.drag_event == 0 {
                self.drag_mode = extract_drag_mode(drag_type);
                if is_valid_drag_mode(self.drag_mode, p.creative) {
                    self.drag_event = 1;
                    self.drag_slots.clear();
                } else {
                    self.reset_drag();
                }
            } else if self.drag_event == 1 {
                let Some(i) = self.slot_index(slot_id) else { return ItemStack::EMPTY };
                let itemstack12 = p.inventory.cursor;
                if can_add_item_to_slot(&self.get_stack(p, i), &itemstack12, true)
                    && self.is_item_valid(i, &itemstack12)
                    && (self.drag_mode == 2 || itemstack12.get_count() > self.drag_slots.len() as i32)
                    && self.can_drag_into_slot(i)
                    && !self.drag_slots.contains(&i)
                {
                    self.drag_slots.push(i);
                }
            } else if self.drag_event == 2 {
                if !self.drag_slots.is_empty() {
                    let mut itemstack9 = p.inventory.cursor.copy();
                    let mut k1 = p.inventory.cursor.get_count();
                    let n = self.drag_slots.len();
                    for slot8 in self.drag_slots.clone() {
                        let itemstack13 = p.inventory.cursor;
                        if can_add_item_to_slot(&self.get_stack(p, slot8), &itemstack13, true)
                            && self.is_item_valid(slot8, &itemstack13)
                            && (self.drag_mode == 2 || itemstack13.get_count() >= n as i32)
                            && self.can_drag_into_slot(slot8)
                        {
                            let mut itemstack14 = itemstack9.copy();
                            let j3 = if self.get_has_stack(p, slot8) { self.get_stack(p, slot8).get_count() } else { 0 };
                            compute_stack_size(n, self.drag_mode, &mut itemstack14, j3);
                            let k3 = itemstack14.get_max_stack_size().min(self.get_item_stack_limit(slot8, &itemstack14));
                            if itemstack14.get_count() > k3 {
                                itemstack14.set_count(k3);
                            }
                            k1 -= itemstack14.get_count() - j3;
                            self.put_stack(p, slot8, itemstack14);
                        }
                    }
                    itemstack9.set_count(k1);
                    p.inventory.cursor = itemstack9;
                }
                self.reset_drag();
            } else {
                self.reset_drag();
            }
        } else if self.drag_event != 0 {
            self.reset_drag();
        } else if (click == ClickType::Pickup || click == ClickType::QuickMove) && (drag_type == 0 || drag_type == 1) {
            if slot_id == -999 {
                if !p.inventory.cursor.is_empty() {
                    if drag_type == 0 {
                        let c = p.inventory.cursor;
                        p.drop_item(c);
                        p.inventory.cursor = ItemStack::EMPTY;
                    }
                    if drag_type == 1 {
                        let one = p.inventory.cursor.split_stack(1);
                        p.drop_item(one);
                    }
                }
            } else if click == ClickType::QuickMove {
                let Some(i) = self.slot_index(slot_id) else { return ItemStack::EMPTY };
                if !self.can_take_stack(p, i) {
                    return ItemStack::EMPTY;
                }
                let mut itemstack7 = self.transfer_stack_in_slot(p, i);
                while !itemstack7.is_empty() && are_items_equal(&self.get_stack(p, i), &itemstack7) {
                    itemstack = itemstack7.copy();
                    itemstack7 = self.transfer_stack_in_slot(p, i);
                }
            } else {
                let Some(i) = self.slot_index(slot_id) else { return ItemStack::EMPTY };
                itemstack = self.pickup(p, i, drag_type);
            }
        } else if click == ClickType::Swap && (0..9).contains(&drag_type) {
            let Some(i) = self.slot_index(slot_id) else { return ItemStack::EMPTY };
            self.swap(p, i, drag_type as usize);
        } else if click == ClickType::Clone && p.creative && p.inventory.cursor.is_empty() && slot_id >= 0 {
            let Some(i) = self.slot_index(slot_id) else { return ItemStack::EMPTY };
            if self.get_has_stack(p, i) {
                let mut itemstack5 = self.get_stack(p, i).copy();
                itemstack5.set_count(itemstack5.get_max_stack_size());
                p.inventory.cursor = itemstack5;
            }
        } else if click == ClickType::Throw && p.inventory.cursor.is_empty() && slot_id >= 0 {
            let Some(i) = self.slot_index(slot_id) else { return ItemStack::EMPTY };
            if self.get_has_stack(p, i) && self.can_take_stack(p, i) {
                let n = if drag_type == 0 { 1 } else { self.get_stack(p, i).get_count() };
                let itemstack4 = self.decr_stack_size(p, i, n);
                self.on_take(p, i, itemstack4);
                p.drop_item(itemstack4);
            }
        } else if click == ClickType::PickupAll && slot_id >= 0 {
            let Some(i) = self.slot_index(slot_id) else { return ItemStack::EMPTY };
            self.pickup_all(p, i, drag_type);
            self.detect_and_send_changes(p);
        }
        itemstack
    }

    /// `slotClick`'s PICKUP branch on slot `i`: left (`drag_type` 0) or right (1).
    fn pickup(&mut self, p: &mut PlayerRef, i: usize, drag_type: i32) -> ItemStack {
        let mut itemstack = ItemStack::EMPTY;
        let mut itemstack8 = self.get_stack(p, i);
        let itemstack11 = p.inventory.cursor;
        if !itemstack8.is_empty() {
            itemstack = itemstack8.copy();
        }
        if itemstack8.is_empty() {
            if !itemstack11.is_empty() && self.is_item_valid(i, &itemstack11) {
                let mut i3 = if drag_type == 0 { itemstack11.get_count() } else { 1 };
                if i3 > self.get_item_stack_limit(i, &itemstack11) {
                    i3 = self.get_item_stack_limit(i, &itemstack11);
                }
                let part = p.inventory.cursor.split_stack(i3);
                self.put_stack(p, i, part);
            }
        } else if self.can_take_stack(p, i) {
            if itemstack11.is_empty() {
                // Java's `itemstack8.isEmpty()` test here is dead: the slot is not empty.
                let l2 = if drag_type == 0 { itemstack8.get_count() } else { (itemstack8.get_count() + 1) / 2 };
                let taken = self.decr_stack_size(p, i, l2);
                p.inventory.cursor = taken;
                // `itemstack8` is the slot's own object: split in place by `getAndSplit`, or,
                // in the crafting result (`getAndRemove`), the very stack now on the cursor.
                let left_empty = if self.slots[i].inventory == Inv::Result {
                    taken.is_empty()
                } else {
                    itemstack8.get_count() - taken.get_count() <= 0
                };
                if left_empty {
                    self.put_stack(p, i, ItemStack::EMPTY);
                }
                let c = p.inventory.cursor;
                self.on_take(p, i, c);
            } else if self.is_item_valid(i, &itemstack11) {
                if itemstack8.get_item() == itemstack11.get_item()
                    && itemstack8.get_metadata() == itemstack11.get_metadata()
                    && are_item_stack_tags_equal(&itemstack8, &itemstack11)
                {
                    let mut k2 = if drag_type == 0 { itemstack11.get_count() } else { 1 };
                    if k2 > self.get_item_stack_limit(i, &itemstack11) - itemstack8.get_count() {
                        k2 = self.get_item_stack_limit(i, &itemstack11) - itemstack8.get_count();
                    }
                    if k2 > itemstack11.get_max_stack_size() - itemstack8.get_count() {
                        k2 = itemstack11.get_max_stack_size() - itemstack8.get_count();
                    }
                    p.inventory.cursor.shrink(k2);
                    itemstack8.grow(k2);
                    self.set_raw(p, i, itemstack8);
                } else if itemstack11.get_count() <= self.get_item_stack_limit(i, &itemstack11) {
                    self.put_stack(p, i, itemstack11);
                    p.inventory.cursor = itemstack8;
                }
            } else if itemstack8.get_item() == itemstack11.get_item()
                && itemstack11.get_max_stack_size() > 1
                && (!itemstack8.get_has_subtypes() || itemstack8.get_metadata() == itemstack11.get_metadata())
                && are_item_stack_tags_equal(&itemstack8, &itemstack11)
                && !itemstack8.is_empty()
            {
                let j2 = itemstack8.get_count();
                if j2 + itemstack11.get_count() <= itemstack11.get_max_stack_size() {
                    p.inventory.cursor.grow(j2);
                    let taken = self.decr_stack_size(p, i, j2);
                    if taken.is_empty() {
                        self.put_stack(p, i, ItemStack::EMPTY);
                    }
                    let c = p.inventory.cursor;
                    self.on_take(p, i, c);
                }
            }
        }
        // slot6.onSlotChanged(): markDirty, a no-op (module notes).
        itemstack
    }

    /// `slotClick`'s SWAP branch: the number key `h` (hotbar slot 0..8) over slot `i`.
    fn swap(&mut self, p: &mut PlayerRef, i: usize, h: usize) {
        let mut itemstack6 = p.inventory.get_stack_in_slot(h);
        let itemstack10 = self.get_stack(p, i);
        // The slot may be that hotbar slot itself, one Java object under two names.
        let same = self.slots[i].inventory == Inv::Player && self.slots[i].index == h;
        if itemstack6.is_empty() && itemstack10.is_empty() {
            return;
        }
        if itemstack6.is_empty() {
            if self.can_take_stack(p, i) {
                p.inventory.set_inventory_slot_contents(h, itemstack10);
                self.on_swap_craft(i, itemstack10.get_count());
                self.put_stack(p, i, ItemStack::EMPTY);
                self.on_take(p, i, itemstack10);
            }
        } else if itemstack10.is_empty() {
            if self.is_item_valid(i, &itemstack6) {
                let l1 = self.get_item_stack_limit(i, &itemstack6);
                if itemstack6.get_count() > l1 {
                    let part = itemstack6.split_stack(l1);
                    p.inventory.inv.set_inventory_slot_contents(h, itemstack6);
                    self.put_stack(p, i, part);
                } else {
                    self.put_stack(p, i, itemstack6);
                    p.inventory.set_inventory_slot_contents(h, ItemStack::EMPTY);
                }
            }
        } else if self.can_take_stack(p, i) && self.is_item_valid(i, &itemstack6) {
            let i2 = self.get_item_stack_limit(i, &itemstack6);
            if itemstack6.get_count() > i2 {
                let part = itemstack6.split_stack(i2);
                p.inventory.inv.set_inventory_slot_contents(h, itemstack6);
                self.put_stack(p, i, part);
                let mut s10 = if same { itemstack6 } else { itemstack10 };
                self.on_take(p, i, s10);
                let creative = p.creative;
                if !p.inventory.add_item_stack_to_inventory(&mut s10, creative) {
                    p.drop_item(s10);
                }
            } else {
                self.put_stack(p, i, itemstack6);
                p.inventory.set_inventory_slot_contents(h, itemstack10);
                self.on_take(p, i, itemstack10);
            }
        }
    }

    /// `slotClick`'s PICKUP_ALL branch, the double click: gathers stacks like the cursor's from
    /// the first slot on (`drag_type` 0) or the last back, part stacks first, then full ones.
    fn pickup_all(&mut self, p: &mut PlayerRef, i: usize, drag_type: i32) {
        let itemstack1 = p.inventory.cursor;
        if itemstack1.is_empty() || (self.get_has_stack(p, i) && self.can_take_stack(p, i)) {
            return;
        }
        let n = self.slots.len() as i32;
        let start = if drag_type == 0 { 0 } else { n - 1 };
        let step = if drag_type == 0 { 1 } else { -1 };
        for k in 0..2 {
            let mut l = start;
            while l >= 0 && l < n && p.inventory.cursor.get_count() < p.inventory.cursor.get_max_stack_size() {
                let li = l as usize;
                let cur = p.inventory.cursor;
                if self.get_has_stack(p, li)
                    && can_add_item_to_slot(&self.get_stack(p, li), &cur, true)
                    && self.can_take_stack(p, li)
                    && self.can_merge_slot(&cur, li)
                {
                    let itemstack2 = self.get_stack(p, li);
                    if k != 0 || itemstack2.get_count() != itemstack2.get_max_stack_size() {
                        let i1 = (cur.get_max_stack_size() - cur.get_count()).min(itemstack2.get_count());
                        let itemstack3 = self.decr_stack_size(p, li, i1);
                        p.inventory.cursor.grow(i1);
                        if itemstack3.is_empty() {
                            self.put_stack(p, li, ItemStack::EMPTY);
                        }
                        self.on_take(p, li, itemstack3);
                    }
                }
                l += step;
            }
        }
    }

    /// `Container.mergeItemStack(stack, startIndex, endIndex, reverseDirection)`: tops up
    /// matching stacks in `start..end`, then puts the rest into the first empty valid slot.
    /// `stack` is the caller's copy of the source slot, written back by the caller.
    pub fn merge_item_stack(&mut self, p: &mut PlayerRef, stack: &mut ItemStack, start: usize, end: usize, reverse: bool) -> bool {
        let (start, end) = (start as i32, end as i32);
        let mut flag = false;
        let mut i = if reverse { end - 1 } else { start };
        if stack.is_stackable() {
            while !stack.is_empty() {
                if reverse {
                    if i < start {
                        break;
                    }
                } else if i >= end {
                    break;
                }
                let iu = i as usize;
                let mut itemstack = self.get_stack(p, iu);
                if !itemstack.is_empty()
                    && itemstack.get_item() == stack.get_item()
                    && (!stack.get_has_subtypes() || stack.get_metadata() == itemstack.get_metadata())
                    && are_item_stack_tags_equal(stack, &itemstack)
                {
                    let j = itemstack.get_count() + stack.get_count();
                    if j <= stack.get_max_stack_size() {
                        stack.set_count(0);
                        itemstack.set_count(j);
                        self.set_raw(p, iu, itemstack);
                        flag = true;
                    } else if itemstack.get_count() < stack.get_max_stack_size() {
                        stack.shrink(stack.get_max_stack_size() - itemstack.get_count());
                        itemstack.set_count(stack.get_max_stack_size());
                        self.set_raw(p, iu, itemstack);
                        flag = true;
                    }
                }
                i += if reverse { -1 } else { 1 };
            }
        }
        if !stack.is_empty() {
            i = if reverse { end - 1 } else { start };
            loop {
                if reverse {
                    if i < start {
                        break;
                    }
                } else if i >= end {
                    break;
                }
                let iu = i as usize;
                if self.get_stack(p, iu).is_empty() && self.is_item_valid(iu, stack) {
                    let lim = self.get_slot_stack_limit(iu);
                    let part = if stack.get_count() > lim { stack.split_stack(lim) } else { stack.split_stack(stack.get_count()) };
                    self.put_stack(p, iu, part);
                    flag = true;
                    break;
                }
                i += if reverse { -1 } else { 1 };
            }
        }
        flag
    }

    /// The shared body of the shift-click after the subclass picked its target range: Java's
    /// in-place merge written back, the empty source cleared, `onTake`, and the crafting
    /// result's leftover dropped. `merged` is the `mergeItemStack` outcome, `itemstack1` the
    /// merged copy and `itemstack` the stack before.
    pub(crate) fn finish_transfer(&mut self, p: &mut PlayerRef, index: usize, itemstack1: ItemStack, itemstack: ItemStack) -> ItemStack {
        if itemstack1.is_empty() {
            self.put_stack(p, index, ItemStack::EMPTY);
        } else {
            self.set_raw(p, index, itemstack1);
        }
        if itemstack1.get_count() == itemstack.get_count() {
            return ItemStack::EMPTY;
        }
        let itemstack2 = self.on_take(p, index, itemstack1);
        if index == 0 {
            p.drop_item(itemstack2);
        }
        itemstack
    }

    /// `Container.onContainerClosed(player)` and the subclasses': the cursor stack is dropped;
    /// `ContainerPlayer` clears its result; both crafting windows give the grid back
    /// (`clearContainer`).
    pub fn on_container_closed(&mut self, p: &mut PlayerRef) {
        if !p.inventory.cursor.is_empty() {
            let c = p.inventory.cursor;
            p.drop_item(c);
            p.inventory.cursor = ItemStack::EMPTY;
        }
        match self.kind {
            ContainerKind::Player => {
                self.crafting_mut().craft_result.clear();
                self.clear_container(p);
            }
            ContainerKind::Workbench { .. } => self.clear_container(p),
        }
    }
}

/// `Container.extractDragMode(eventButton)`: 0 split evenly, 1 one each, 2 full stacks
/// (creative).
pub fn extract_drag_mode(event_button: i32) -> i32 {
    event_button >> 2 & 3
}

/// `Container.getDragEvent(clickedButton)`: 0 start, 1 add a slot, 2 end.
pub fn get_drag_event(clicked_button: i32) -> i32 {
    clicked_button & 3
}

/// `Container.getQuickcraftMask(event, mode)`: the QUICK_CRAFT button the GUI sends.
pub fn get_quickcraft_mask(event: i32, mode: i32) -> i32 {
    event & 3 | (mode & 3) << 2
}

/// `Container.isValidDragMode(mode, player)`: mode 2 only in creative.
pub fn is_valid_drag_mode(mode: i32, creative: bool) -> bool {
    match mode {
        0 | 1 => true,
        _ => mode == 2 && creative,
    }
}

/// `Container.canAddItemToSlot(slot, stack, stackSizeMatters)`, given the slot's stack (a null
/// slot is an empty one).
pub fn can_add_item_to_slot(slot_stack: &ItemStack, stack: &ItemStack, stack_size_matters: bool) -> bool {
    let flag = slot_stack.is_empty();
    if !flag && stack.is_item_equal(slot_stack) && are_item_stack_tags_equal(slot_stack, stack) {
        slot_stack.get_count() + if stack_size_matters { 0 } else { stack.get_count() } <= stack.get_max_stack_size()
    } else {
        flag
    }
}

/// `Container.computeStackSize(dragSlots, dragMode, stack, slotStackSize)`
pub fn compute_stack_size(drag_slots: usize, drag_mode: i32, stack: &mut ItemStack, slot_stack_size: i32) {
    match drag_mode {
        // MathHelper.floor((float)count / (float)size)
        0 => stack.set_count(floor_f(stack.get_count() as f32 / drag_slots as f32)),
        1 => stack.set_count(1),
        // stack.getItem().getItemStackLimit()
        2 => stack.set_count(stack.def().item_stack_limit()),
        _ => {}
    }
    stack.grow(slot_stack_size);
}

/// `MathHelper.floor(float)`
fn floor_f(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 {
        i - 1
    } else {
        i
    }
}
