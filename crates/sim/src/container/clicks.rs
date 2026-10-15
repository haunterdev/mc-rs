//! T7a: `Container.slotClick` and what it calls: `pickup`, `swap`, `pickup_all`,
//! `mergeItemStack`, `finish_transfer` and `onContainerClosed`. A second `impl Container`;
//! the state and the slot accessors are in mod.rs.

use items::stack::{are_item_stack_tags_equal, are_items_equal, ItemStack};

use super::*;

impl Container {
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
        if index == 0 && self.slots[index].kind == SlotKind::Crafting {
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
            ContainerKind::Enchanting { .. } => self.clear_enchanting(p),
            // ContainerChest and ContainerFurnace: the base, and the server counts the lid user out.
            ContainerKind::Chest { .. } | ContainerKind::Furnace { .. } => {}
            // ContainerCreative keeps the base: the cursor only.
            ContainerKind::Creative => {}
        }
    }
}
