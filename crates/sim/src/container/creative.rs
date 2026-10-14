//! `GuiContainerCreative.ContainerCreative`, the window behind the creative item tabs.
//!
//! Slots: 0..45 the item list (`LockedSlot`s over `basicInventory`, `i * 9 + j`), 45..54 the
//! hotbar (`InventoryPlayer` 0..9). `scrollTo` fills `basicInventory` from `itemList`. The
//! container is client only: the creative GUI sends what changed in the player's own window as
//! `CPacketCreativeInventoryAction`, and the GUI's own click rules (`handleMouseClick`) are in
//! the viewer.
//!
//! `basicInventory` is a static `InventoryBasic` in Java, shared by every creative GUI; here the
//! container holds it, and the viewer keeps one container for the session.
//!
//! `LockedSlot.canTakeStack` refuses a stack with a `CustomCreativeLock` tag, which no stack
//! here carries, so it is the base `Slot`'s true.

use items::stack::ItemStack;

use super::{Container, ContainerKind, Inv, PlayerRef, SlotKind};

/// `basicInventory` and `itemList`.
#[derive(Clone, Debug, Default)]
pub struct Creative {
    /// `basicInventory`, 45 stacks.
    pub basic: Vec<ItemStack>,
    /// `itemList`
    pub item_list: Vec<ItemStack>,
}

impl Creative {
    pub(crate) fn set_basic(&mut self, index: usize, stack: ItemStack) {
        if let Some(s) = self.basic.get_mut(index) {
            // InventoryBasic.setInventorySlotContents caps at the stack limit, 64.
            let mut stack = stack;
            if !stack.is_empty() && stack.get_count() > 64 {
                stack.set_count(64);
            }
            *s = stack;
        }
    }

    /// `InventoryBasic.decrStackSize`: `ItemStackHelper.getAndSplit`.
    pub(crate) fn decr_basic(&mut self, index: usize, count: i32) -> ItemStack {
        match self.basic.get_mut(index) {
            Some(s) if !s.is_empty() && count > 0 => {
                let out = s.split_stack(count);
                if s.is_empty() {
                    *s = ItemStack::EMPTY;
                }
                out
            }
            _ => ItemStack::EMPTY,
        }
    }
}

impl Container {
    /// `new ContainerCreative(player)`, with an empty item list.
    pub fn creative() -> Container {
        let mut c = Container::empty(ContainerKind::Creative, 0, None);
        c.creative = Some(Creative { basic: vec![ItemStack::EMPTY; 45], item_list: Vec::new() });
        for i in 0..5 {
            for j in 0..9 {
                c.add_slot_to_container(Inv::Basic, i * 9 + j, 9 + j as i32 * 18, 18 + i as i32 * 18, SlotKind::Plain);
            }
        }
        for k in 0..9 {
            c.add_slot_to_container(Inv::Player, k, 9 + k as i32 * 18, 112, SlotKind::Plain);
        }
        c.scroll_to(0.0);
        c
    }

    pub(crate) fn basic_inv(&self) -> &Creative {
        self.creative.as_ref().expect("a creative slot in a container without basicInventory")
    }

    pub(crate) fn basic_inv_mut(&mut self) -> &mut Creative {
        self.creative.as_mut().expect("a creative slot in a container without basicInventory")
    }

    /// `ContainerCreative.scrollTo(pos)`: shows the rows from `pos` (0 top, 1 bottom) of the
    /// item list.
    pub fn scroll_to(&mut self, pos: f32) {
        let c = self.basic_inv_mut();
        let n = c.item_list.len() as i32;
        let i = (n + 9 - 1) / 9 - 5;
        let j = ((pos * i as f32) as f64 + 0.5) as i32;
        let j = j.max(0);
        for k in 0..5 {
            for l in 0..9 {
                let i1 = l + (k + j) * 9;
                let s = if i1 >= 0 && i1 < n { c.item_list[i1 as usize] } else { ItemStack::EMPTY };
                c.set_basic((l + k * 9) as usize, s);
            }
        }
    }

    /// `ContainerCreative.canScroll()`
    pub fn can_scroll(&self) -> bool {
        self.basic_inv().item_list.len() > 45
    }

    /// `ContainerCreative.transferStackInSlot`: a shift-click on the hotbar row deletes the
    /// stack; nothing else moves.
    pub(crate) fn transfer_creative(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        let n = self.slots.len();
        if index >= n - 9 && index < n && self.get_has_stack(p, index) {
            self.put_stack(p, index, ItemStack::EMPTY);
        }
        ItemStack::EMPTY
    }
}
