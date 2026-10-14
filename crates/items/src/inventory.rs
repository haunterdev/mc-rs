//! `InventoryPlayer`: the 36 main slots (0 to 8 the hotbar), 4 armor slots and the off hand,
//! with the stacking rules (`storeItemStack`, `canMergeStacks`, `addResource`) that picking an
//! item up goes through, and the held-item queries mining asks (`getDestroySpeed`,
//! `canHarvestBlock`).
//!
//! `sim::inventory::InventoryPlayer` wraps this with the save slot numbering, `decrementAnimations`
//! (the pickup bob), the pick block (`setPickedItemStack`, `getSlotFor`), `deleteStack` and
//! `dropAllItems`. DROPPED here: `timesChanged` and the crash report around `add`.

use worldgen::populate::state::mat;

use crate::dig::is_tool_not_required;
use crate::stack::{are_item_stack_tags_equal, ItemStack};

/// `net.minecraft.entity.player.InventoryPlayer`
#[derive(Clone, Debug)]
pub struct PlayerInventory {
    /// `mainInventory`
    pub main: [ItemStack; 36],
    /// `armorInventory`, by `EntityEquipmentSlot.getIndex()` (feet first)
    pub armor: [ItemStack; 4],
    /// `offHandInventory`
    pub off_hand: [ItemStack; 1],
    /// `currentItem`, the selected hotbar slot
    pub current_item: usize,
    /// `itemStack`: the stack held on the cursor while a container is open
    pub cursor: ItemStack,
}

impl Default for PlayerInventory {
    fn default() -> Self {
        PlayerInventory {
            main: [ItemStack::EMPTY; 36],
            armor: [ItemStack::EMPTY; 4],
            off_hand: [ItemStack::EMPTY; 1],
            current_item: 0,
            cursor: ItemStack::EMPTY,
        }
    }
}

impl PlayerInventory {
    pub fn new() -> Self {
        Self::default()
    }

    /// `getHotbarSize()`
    pub const HOTBAR_SIZE: usize = 9;

    /// `isHotbar(index)`
    pub fn is_hotbar(index: i32) -> bool {
        (0..9).contains(&index)
    }

    /// `getCurrentItem()`
    pub fn get_current_item(&self) -> ItemStack {
        if Self::is_hotbar(self.current_item as i32) {
            self.main[self.current_item]
        } else {
            ItemStack::EMPTY
        }
    }

    /// `getInventoryStackLimit()`
    pub fn get_inventory_stack_limit(&self) -> i32 {
        64
    }

    /// `getSizeInventory()`: 36 + 4 + 1.
    pub fn get_size_inventory(&self) -> usize {
        41
    }

    fn slot(&self, index: usize) -> Option<&ItemStack> {
        match index {
            0..=35 => Some(&self.main[index]),
            36..=39 => Some(&self.armor[index - 36]),
            40 => Some(&self.off_hand[0]),
            _ => None,
        }
    }

    fn slot_mut(&mut self, index: usize) -> Option<&mut ItemStack> {
        match index {
            0..=35 => Some(&mut self.main[index]),
            36..=39 => Some(&mut self.armor[index - 36]),
            40 => Some(&mut self.off_hand[0]),
            _ => None,
        }
    }

    /// `getStackInSlot(index)`: main 0..36, armor 36..40, off hand 40.
    pub fn get_stack_in_slot(&self, index: usize) -> ItemStack {
        self.slot(index).copied().unwrap_or(ItemStack::EMPTY)
    }

    /// `setInventorySlotContents(index, stack)`
    pub fn set_inventory_slot_contents(&mut self, index: usize, stack: ItemStack) {
        if let Some(s) = self.slot_mut(index) {
            *s = stack;
        }
    }

    /// `decrStackSize(index, count)` (`ItemStackHelper.getAndSplit`): an empty slot left behind
    /// is cleared.
    pub fn decr_stack_size(&mut self, index: usize, count: i32) -> ItemStack {
        match self.slot_mut(index) {
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

    /// `removeStackFromSlot(index)`
    pub fn remove_stack_from_slot(&mut self, index: usize) -> ItemStack {
        match self.slot_mut(index) {
            Some(s) if !s.is_empty() => std::mem::replace(s, ItemStack::EMPTY),
            _ => ItemStack::EMPTY,
        }
    }

    /// `isEmpty()`
    pub fn is_empty(&self) -> bool {
        self.main.iter().chain(&self.armor).chain(&self.off_hand).all(|s| s.is_empty())
    }

    /// `getFirstEmptyStack()`: the first empty main slot, or -1.
    pub fn get_first_empty_stack(&self) -> i32 {
        self.main.iter().position(|s| s.is_empty()).map_or(-1, |i| i as i32)
    }

    /// `stackEqualExact(stack1, stack2)`: same item, same metadata when the item has subtypes.
    pub fn stack_equal_exact(stack1: &ItemStack, stack2: &ItemStack) -> bool {
        stack1.get_item() == stack2.get_item()
            && (!stack1.get_has_subtypes() || stack1.get_metadata() == stack2.get_metadata())
            && are_item_stack_tags_equal(stack1, stack2)
    }

    /// `canMergeStacks(stack1, stack2)`: whether `stack2` can top up `stack1`.
    pub fn can_merge_stacks(&self, stack1: &ItemStack, stack2: &ItemStack) -> bool {
        !stack1.is_empty()
            && Self::stack_equal_exact(stack1, stack2)
            && stack1.is_stackable()
            && stack1.get_count() < stack1.get_max_stack_size()
            && stack1.get_count() < self.get_inventory_stack_limit()
    }

    /// `storeItemStack(stack)`: the selected slot, then the off hand (40), then the first main
    /// slot that can take more of `stack`; -1 if none.
    pub fn store_item_stack(&self, stack: &ItemStack) -> i32 {
        if self.can_merge_stacks(&self.get_stack_in_slot(self.current_item), stack) {
            self.current_item as i32
        } else if self.can_merge_stacks(&self.get_stack_in_slot(40), stack) {
            40
        } else {
            for (i, s) in self.main.iter().enumerate() {
                if self.can_merge_stacks(s, stack) {
                    return i as i32;
                }
            }
            -1
        }
    }

    /// `storePartialItemStack(stack)`: returns what is left over.
    pub fn store_partial_item_stack(&mut self, stack: &ItemStack) -> i32 {
        let mut i = self.store_item_stack(stack);
        if i == -1 {
            i = self.get_first_empty_stack();
        }
        if i == -1 {
            stack.get_count()
        } else {
            self.add_resource(i as usize, stack)
        }
    }

    /// `addResource(slot, stack)`: moves as many as fit into `slot`, returns what is left over.
    fn add_resource(&mut self, slot: usize, stack: &ItemStack) -> i32 {
        let item = stack.get_item();
        let mut i = stack.get_count();
        let mut itemstack = self.get_stack_in_slot(slot);
        if itemstack.is_empty() {
            // new ItemStack(item, 0, meta): an empty stack until it grows below.
            itemstack = ItemStack::new(item, 0, stack.get_metadata());
        }
        let mut j = i;
        let max = itemstack.get_max_stack_size();
        let count = itemstack.count;
        if i > max - count {
            j = max - count;
        }
        if j > self.get_inventory_stack_limit() - count {
            j = self.get_inventory_stack_limit() - count;
        }
        if j == 0 {
            self.set_inventory_slot_contents(slot, itemstack);
            return i;
        }
        i -= j;
        itemstack.grow(j);
        // DROPPED: setAnimationsToGo(5), the client pickup bob.
        self.set_inventory_slot_contents(slot, itemstack);
        i
    }

    /// `addItemStackToInventory(stack)`: `add(-1, stack)`.
    pub fn add_item_stack_to_inventory(&mut self, stack: &mut ItemStack, creative: bool) -> bool {
        self.add(-1, stack, creative)
    }

    /// `add(slot, stack)`: a damaged item goes whole into `slot` or the first empty slot; anything
    /// else is spread over matching stacks, then empty slots. `stack` keeps what did not fit;
    /// returns whether anything was taken. In creative mode a stack that does not fit is used up.
    pub fn add(&mut self, mut slot: i32, stack: &mut ItemStack, creative: bool) -> bool {
        if stack.is_empty() {
            return false;
        }
        if stack.is_item_damaged() {
            if slot == -1 {
                slot = self.get_first_empty_stack();
            }
            if slot >= 0 {
                // mainInventory.set(slot, copy): Java throws for a slot past the main
                // inventory; this writes the armor or off-hand slot instead.
                self.set_inventory_slot_contents(slot as usize, stack.copy());
                stack.set_count(0);
                true
            } else if creative {
                stack.set_count(0);
                true
            } else {
                false
            }
        } else {
            let mut i;
            loop {
                i = stack.get_count();
                if slot == -1 {
                    let left = self.store_partial_item_stack(stack);
                    stack.set_count(left);
                } else {
                    let left = self.add_resource(slot as usize, stack);
                    stack.set_count(left);
                }
                if stack.is_empty() || stack.get_count() >= i {
                    break;
                }
            }
            if stack.get_count() == i && creative {
                stack.set_count(0);
                true
            } else {
                stack.get_count() < i
            }
        }
    }

    /// `getBestHotbarSlot()`: an empty hotbar slot from the selected one on, else the first slot
    /// whose stack is not enchanted (every stack here), else the selected one.
    pub fn get_best_hotbar_slot(&self) -> usize {
        for i in 0..9 {
            let j = (self.current_item + i) % 9;
            if self.main[j].is_empty() {
                return j;
            }
        }
        // isItemEnchanted() is always false without NBT.
        self.current_item % 9
    }

    /// `pickItem(index)`: swaps main slot `index` into the best hotbar slot and selects it.
    pub fn pick_item(&mut self, index: usize) {
        self.current_item = self.get_best_hotbar_slot();
        self.main.swap(self.current_item, index);
    }

    /// `changeCurrentItem(direction)`: the mouse wheel.
    pub fn change_current_item(&mut self, direction: i32) {
        let d = direction.signum();
        let mut c = self.current_item as i32 - d;
        while c < 0 {
            c += 9;
        }
        while c >= 9 {
            c -= 9;
        }
        self.current_item = c as usize;
    }

    /// `getDestroySpeed(state)`: the held stack's destroy speed, 1.0 bare-handed.
    pub fn get_destroy_speed(&self, state: u16) -> f32 {
        let mut f = 1.0f32;
        let held = self.main[self.current_item];
        if !held.is_empty() {
            f *= held.get_destroy_speed(state);
        }
        f
    }

    /// `canHarvestBlock(state)`: any hand when the material needs no tool, else the held stack.
    pub fn can_harvest_block(&self, state: u16) -> bool {
        if is_tool_not_required(mat(state)) {
            true
        } else {
            let held = self.get_stack_in_slot(self.current_item);
            !held.is_empty() && held.can_harvest_block(state)
        }
    }

    /// `clearMatchingItems(item, meta, removeCount, null)`: removes up to `remove_count` of
    /// `item` (any item when `None`) with metadata `meta` (any when -1), the cursor stack last.
    /// A negative `remove_count` removes every match; 0 only counts. Returns how many matched.
    pub fn clear_matching_items(&mut self, item: Option<u16>, meta: i32, remove_count: i32) -> i32 {
        let mut i = 0;
        for j in 0..self.get_size_inventory() {
            let s = self.get_stack_in_slot(j);
            if !s.is_empty() && item.is_none_or(|it| s.get_item() == it) && (meta <= -1 || s.get_metadata() == meta) {
                let k = if remove_count <= 0 { s.get_count() } else { (remove_count - i).min(s.get_count()) };
                i += k;
                if remove_count != 0 {
                    let mut s2 = s;
                    s2.shrink(k);
                    self.set_inventory_slot_contents(j, if s2.is_empty() { ItemStack::EMPTY } else { s2 });
                    if remove_count > 0 && i >= remove_count {
                        return i;
                    }
                }
            }
        }
        if !self.cursor.is_empty() {
            if item.is_some_and(|it| self.cursor.get_item() != it) {
                return i;
            }
            if meta > -1 && self.cursor.get_metadata() != meta {
                return i;
            }
            let l = if remove_count <= 0 { self.cursor.get_count() } else { (remove_count - i).min(self.cursor.get_count()) };
            i += l;
            if remove_count != 0 {
                self.cursor.shrink(l);
                if self.cursor.is_empty() {
                    self.cursor = ItemStack::EMPTY;
                }
                if remove_count > 0 && i >= remove_count {
                    return i;
                }
            }
        }
        i
    }
}
