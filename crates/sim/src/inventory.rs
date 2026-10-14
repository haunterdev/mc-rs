//! T3: `net.minecraft.entity.player.InventoryPlayer`, the player's 36 main slots (0 to 8 the
//! hotbar), 4 armor slots (feet first) and the off hand, as the server and the viewer share it.
//!
//! The slots, `currentItem` and the stacking rules are `items::PlayerInventory`, reached through
//! `Deref`: `getCurrentItem`, `getFirstEmptyStack`, `getBestHotbarSlot`, `changeCurrentItem`,
//! `decrStackSize`, `clearMatchingItems`, `getInventoryStackLimit`, `storeItemStack`,
//! `canMergeStacks`, `getStackInSlot`, `isEmpty`, `getDestroySpeed`, `canHarvestBlock`. This file
//! adds what `items` leaves out: the pickup bob counter (`ItemStack.animationsToGo`), the pick
//! block (`getSlotFor`, `setPickedItemStack`), `deleteStack`, `dropAllItems` and the save slot
//! numbering (`writeToNBT`, `readFromNBT`). Methods that change a slot's bob are wrapped here
//! under the same names, so they shadow the `items` ones.
//!
//! `items::ItemStack` is a plain value with no `animationsToGo`, so the counter lives beside the
//! slots, one per `getSizeInventory` index, and follows a stack only where this file moves it
//! (`pickItem`, `setPickedItemStack`). A stack written straight into `main`, `armor` or
//! `off_hand` keeps the slot's counter until `decrement_animations` sees the slot empty.
//!
//! DROPPED: the NBT tag compound of every stack (enchantments, custom names), so
//! `getBestHotbarSlot`'s enchanted test and `clearMatchingItems`'s tag filter never fire;
//! `Item.onUpdate` in `decrementAnimations`; `timesChanged`; `placeItemBackInInventory`,
//! `findSlotMatchingUnusedItem`, `hasItemStack`, `damageArmor`, `copyInventory` and
//! `fillStackedContents`, which later tasks port when they need them.

use std::ops::{Deref, DerefMut};

use items::stack::{are_item_stacks_equal, ItemStack};
use items::PlayerInventory;

/// `mainInventory.size()`
pub const MAIN_SIZE: usize = 36;
/// `armorInventory.size()`
pub const ARMOR_SIZE: usize = 4;
/// `offHandInventory.size()`
pub const OFF_HAND_SIZE: usize = 1;
/// `getSizeInventory()`
pub const SIZE: usize = MAIN_SIZE + ARMOR_SIZE + OFF_HAND_SIZE;

/// `writeToNBT`: the save slot of armor slot 0 (feet); head is 103.
pub const ARMOR_SAVE_SLOT: i32 = 100;
/// `writeToNBT`: the off hand's `Slot` byte, `(byte)150`.
pub const OFF_HAND_SAVE_SLOT: i32 = 150u8 as i8 as i32;

/// `ItemStack.setAnimationsToGo(5)`, what `addResource` and `add` give a slot that took items.
const PICKUP_ANIMATION: i32 = 5;

/// `net.minecraft.entity.player.InventoryPlayer`
#[derive(Clone, Debug)]
pub struct InventoryPlayer {
    /// The slots, `currentItem`, the cursor stack and the stacking rules.
    pub inv: PlayerInventory,
    /// `ItemStack.animationsToGo` of the stack in each slot, by `getStackInSlot` index.
    pub animations: [i32; SIZE],
}

impl Default for InventoryPlayer {
    fn default() -> Self {
        InventoryPlayer { inv: PlayerInventory::new(), animations: [0; SIZE] }
    }
}

impl Deref for InventoryPlayer {
    type Target = PlayerInventory;
    fn deref(&self) -> &PlayerInventory {
        &self.inv
    }
}

impl DerefMut for InventoryPlayer {
    fn deref_mut(&mut self) -> &mut PlayerInventory {
        &mut self.inv
    }
}

impl InventoryPlayer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every slot in `getStackInSlot` order.
    fn slots(&self) -> [ItemStack; SIZE] {
        std::array::from_fn(|i| self.inv.get_stack_in_slot(i))
    }

    /// Runs a stacking call and gives every slot it changed `setAnimationsToGo(5)`. `addResource`
    /// sets it only on a slot that grew and the damaged-item branch of `add` only on the slot it
    /// fills, so a changed slot is exactly a slot Java bobs.
    fn bobbing<R>(&mut self, f: impl FnOnce(&mut PlayerInventory) -> R) -> R {
        let before = self.slots();
        let r = f(&mut self.inv);
        for (i, old) in before.iter().enumerate() {
            if self.inv.get_stack_in_slot(i) != *old {
                self.animations[i] = PICKUP_ANIMATION;
            }
        }
        r
    }

    /// `ItemStack.getAnimationsToGo()` of the stack in slot `index`.
    pub fn get_animations_to_go(&self, index: usize) -> i32 {
        self.animations.get(index).copied().unwrap_or(0)
    }

    /// `InventoryPlayer.addItemStackToInventory(stack)`: `add(-1, stack)`.
    pub fn add_item_stack_to_inventory(&mut self, stack: &mut ItemStack, creative: bool) -> bool {
        self.add(-1, stack, creative)
    }

    /// `InventoryPlayer.add(slot, stack)` (`items::PlayerInventory::add`) with the pickup bob.
    /// `creative` is `player.capabilities.isCreativeMode`.
    pub fn add(&mut self, slot: i32, stack: &mut ItemStack, creative: bool) -> bool {
        self.bobbing(|inv| inv.add(slot, stack, creative))
    }

    /// `InventoryPlayer.storePartialItemStack(stack)` with the pickup bob: returns what is left.
    pub fn store_partial_item_stack(&mut self, stack: &ItemStack) -> i32 {
        self.bobbing(|inv| inv.store_partial_item_stack(stack))
    }

    /// `InventoryPlayer.setInventorySlotContents(index, stack)`. The new stack starts with no bob,
    /// as a stack made by `new ItemStack` or `copy()` does.
    pub fn set_inventory_slot_contents(&mut self, index: usize, stack: ItemStack) {
        self.inv.set_inventory_slot_contents(index, stack);
        if index < SIZE {
            self.animations[index] = 0;
        }
    }

    /// `InventoryPlayer.removeStackFromSlot(index)`
    pub fn remove_stack_from_slot(&mut self, index: usize) -> ItemStack {
        let out = self.inv.remove_stack_from_slot(index);
        if index < SIZE {
            self.animations[index] = 0;
        }
        out
    }

    /// `InventoryPlayer.getSlotFor(stack)`: the first non-empty main slot `stackEqualExact` to
    /// `stack`, or -1.
    pub fn get_slot_for(&self, stack: &ItemStack) -> i32 {
        for i in 0..MAIN_SIZE {
            let s = &self.inv.main[i];
            if !s.is_empty() && PlayerInventory::stack_equal_exact(stack, s) {
                return i as i32;
            }
        }
        -1
    }

    /// `InventoryPlayer.pickItem(index)`: swaps main slot `index` into the best hotbar slot and
    /// selects it. The bob moves with each stack.
    pub fn pick_item(&mut self, index: usize) {
        self.inv.current_item = self.inv.get_best_hotbar_slot();
        let c = self.inv.current_item;
        self.inv.main.swap(c, index);
        self.animations.swap(c, index);
    }

    /// `InventoryPlayer.setPickedItemStack(stack)`: the middle click. A stack already in the
    /// hotbar is selected; one further in is swapped into the best hotbar slot; a new one goes
    /// into the best hotbar slot, whose stack moves to the first empty main slot (or is replaced
    /// when there is none).
    pub fn set_picked_item_stack(&mut self, stack: ItemStack) {
        let i = self.get_slot_for(&stack);
        if PlayerInventory::is_hotbar(i) {
            self.inv.current_item = i as usize;
        } else if i == -1 {
            self.inv.current_item = self.inv.get_best_hotbar_slot();
            let c = self.inv.current_item;
            if !self.inv.main[c].is_empty() {
                let j = self.inv.get_first_empty_stack();
                if j != -1 {
                    let j = j as usize;
                    self.inv.main[j] = self.inv.main[c];
                    self.animations[j] = self.animations[c];
                }
            }
            self.inv.main[c] = stack;
            self.animations[c] = 0;
        } else {
            self.pick_item(i as usize);
        }
    }

    /// `InventoryPlayer.deleteStack(stack)`: empties the slot holding `stack`, the first match in
    /// each of main, armor and off hand. Java matches by reference, so one slot at most; with
    /// value stacks the first equal stack (item, damage, count) of each list goes.
    pub fn delete_stack(&mut self, stack: &ItemStack) {
        for (start, len) in [(0, MAIN_SIZE), (MAIN_SIZE, ARMOR_SIZE), (MAIN_SIZE + ARMOR_SIZE, OFF_HAND_SIZE)] {
            for i in start..start + len {
                if are_item_stacks_equal(&self.inv.get_stack_in_slot(i), stack) {
                    self.set_inventory_slot_contents(i, ItemStack::EMPTY);
                    break;
                }
            }
        }
    }

    /// `InventoryPlayer.dropAllItems()`: empties main, armor and off hand, in that order, and
    /// returns the stacks where Java calls `player.dropItem(stack, true, false)` on each.
    pub fn drop_all_items(&mut self) -> Vec<ItemStack> {
        let mut out = Vec::new();
        for i in 0..SIZE {
            let s = self.inv.get_stack_in_slot(i);
            if !s.is_empty() {
                out.push(s);
                self.set_inventory_slot_contents(i, ItemStack::EMPTY);
            }
        }
        out
    }

    /// `InventoryPlayer.decrementAnimations()`: `ItemStack.updateAnimation` on every non-empty
    /// stack, which counts `animationsToGo` down to 0. An empty slot's counter is cleared, since
    /// it has no stack to belong to.
    pub fn decrement_animations(&mut self) {
        for i in 0..SIZE {
            if self.inv.get_stack_in_slot(i).is_empty() {
                self.animations[i] = 0;
            } else if self.animations[i] > 0 {
                self.animations[i] -= 1;
            }
            // DROPPED: Item.onUpdate(stack, world, player, slot, isCurrentItem); no 1.12.2 item
            // that survival carries yet does anything there but maps and armor ticks.
        }
    }

    /// `InventoryPlayer.writeToNBT(list)`: every non-empty stack with its `Slot` byte, main 0..35,
    /// armor 100..103 (feet to head), off hand `(byte)150` = -106.
    pub fn to_save(&self) -> Vec<(i32, ItemStack)> {
        let mut out = Vec::new();
        for (i, s) in self.inv.main.iter().enumerate() {
            if !s.is_empty() {
                out.push((i as i32, *s));
            }
        }
        for (j, s) in self.inv.armor.iter().enumerate() {
            if !s.is_empty() {
                out.push((j as i32 + ARMOR_SAVE_SLOT, *s));
            }
        }
        for (k, s) in self.inv.off_hand.iter().enumerate() {
            if !s.is_empty() {
                out.push(((k as i32 + 150) as u8 as i8 as i32, *s));
            }
        }
        // DROPPED: ItemStack.writeToNBT's tag compound; the stack is item, count and damage.
        out
    }

    /// `InventoryPlayer.readFromNBT(list)`: clears main, armor and off hand, then places each
    /// non-empty stack by `Slot & 255`; a slot number that maps nowhere is skipped. `currentItem`
    /// and the cursor stack are left alone, as in Java.
    pub fn from_save(&mut self, list: &[(i32, ItemStack)]) {
        self.inv.main = [ItemStack::EMPTY; MAIN_SIZE];
        self.inv.armor = [ItemStack::EMPTY; ARMOR_SIZE];
        self.inv.off_hand = [ItemStack::EMPTY; OFF_HAND_SIZE];
        self.animations = [0; SIZE];
        for &(slot, stack) in list {
            let j = (slot & 255) as usize;
            if !stack.is_empty() {
                if j < MAIN_SIZE {
                    self.inv.main[j] = stack;
                } else if (100..100 + ARMOR_SIZE).contains(&j) {
                    self.inv.armor[j - 100] = stack;
                } else if (150..150 + OFF_HAND_SIZE).contains(&j) {
                    self.inv.off_hand[j - 150] = stack;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::ids::{block, item};

    fn stack(id: u16, count: i32) -> ItemStack {
        ItemStack::new(id, count, 0)
    }

    #[test]
    fn merges_into_a_partial_stack() {
        let mut inv = InventoryPlayer::new();
        inv.main[3] = stack(block::STONE, 10);
        let mut s = stack(block::STONE, 20);
        assert!(inv.add_item_stack_to_inventory(&mut s, false));
        assert!(s.is_empty());
        assert_eq!(inv.main[3], stack(block::STONE, 30));
        assert!(inv.main[0].is_empty());
        assert_eq!(inv.get_animations_to_go(3), 5);
        assert_eq!(inv.get_animations_to_go(0), 0);
    }

    #[test]
    fn the_selected_slot_and_off_hand_come_first() {
        let mut inv = InventoryPlayer::new();
        inv.main[1] = stack(block::DIRT, 1);
        inv.off_hand[0] = stack(block::DIRT, 1);
        inv.main[7] = stack(block::DIRT, 1);
        inv.current_item = 7;
        let mut s = stack(block::DIRT, 3);
        inv.add_item_stack_to_inventory(&mut s, false);
        assert_eq!(inv.main[7].count, 4);
        inv.current_item = 0;
        let mut s = stack(block::DIRT, 3);
        inv.add_item_stack_to_inventory(&mut s, false);
        assert_eq!(inv.off_hand[0].count, 4);
        assert_eq!(inv.get_animations_to_go(40), 5);
    }

    #[test]
    fn overflow_goes_to_the_next_slot() {
        let mut inv = InventoryPlayer::new();
        inv.main[0] = stack(block::STONE, 60);
        let mut s = stack(block::STONE, 10);
        assert!(inv.add_item_stack_to_inventory(&mut s, false));
        assert!(s.is_empty());
        assert_eq!(inv.main[0].count, 64);
        assert_eq!(inv.main[1], stack(block::STONE, 6));
        // An ender pearl stack tops up to 16 and the rest goes on.
        inv.main[2] = stack(item::ENDER_PEARL, 10);
        let mut p = stack(item::ENDER_PEARL, 10);
        assert!(inv.add_item_stack_to_inventory(&mut p, false));
        assert_eq!(inv.main[2], stack(item::ENDER_PEARL, 16));
        assert_eq!(inv.main[3], stack(item::ENDER_PEARL, 4));
    }

    #[test]
    fn an_empty_slot_takes_up_to_64_of_anything() {
        // addResource's placeholder `new ItemStack(item, 0, meta)` is empty, so its
        // getMaxStackSize() is air's 64: 1.12.2's /give overstacking.
        let mut inv = InventoryPlayer::new();
        let mut p = stack(item::ENDER_PEARL, 20);
        assert!(inv.add_item_stack_to_inventory(&mut p, false));
        assert_eq!(inv.main[0], stack(item::ENDER_PEARL, 20));
    }

    #[test]
    fn a_full_inventory_leaves_the_remainder() {
        let mut inv = InventoryPlayer::new();
        for i in 0..MAIN_SIZE {
            inv.main[i] = stack(block::DIRT, 64);
        }
        inv.main[5] = stack(block::STONE, 60);
        let mut s = stack(block::STONE, 10);
        assert_eq!(inv.store_partial_item_stack(&s), 6);
        inv.main[5].count = 60;
        // add's `i` is reset each pass of its loop, so a second pass that takes nothing returns
        // false although the first took 4.
        assert!(!inv.add_item_stack_to_inventory(&mut s, false));
        assert_eq!(s.count, 6);
        assert_eq!(inv.main[5].count, 64);
        // Nothing more fits: false, the stack untouched.
        assert!(!inv.add_item_stack_to_inventory(&mut s, false));
        assert_eq!(s.count, 6);
        // Creative uses it up.
        assert!(inv.add_item_stack_to_inventory(&mut s, true));
        assert!(s.is_empty());
        // A damaged tool needs an empty slot.
        let mut sword = ItemStack::new(item::DIAMOND_SWORD, 1, 5);
        assert!(!inv.add_item_stack_to_inventory(&mut sword, false));
        assert_eq!(sword.count, 1);
    }

    #[test]
    fn pick_block_into_an_empty_hotbar_slot() {
        let mut inv = InventoryPlayer::new();
        for i in 0..9 {
            inv.main[i] = stack(block::DIRT, 1 + i as i32);
        }
        inv.main[4] = ItemStack::EMPTY;
        inv.current_item = 2;
        inv.set_picked_item_stack(stack(block::GLASS, 1));
        assert_eq!(inv.current_item, 4);
        assert_eq!(inv.main[4], stack(block::GLASS, 1));
        // Already in the hotbar: selected, nothing moves.
        inv.current_item = 0;
        inv.set_picked_item_stack(stack(block::GLASS, 1));
        assert_eq!(inv.current_item, 4);
        assert_eq!(inv.get_slot_for(&stack(block::GLASS, 1)), 4);
    }

    #[test]
    fn pick_block_onto_the_best_slot() {
        let mut inv = InventoryPlayer::new();
        for i in 0..9 {
            inv.main[i] = stack(block::DIRT, 1 + i as i32);
        }
        inv.current_item = 3;
        let old = inv.main[3];
        // New, full hotbar: replaces the selected slot, the old stack goes to the first empty.
        inv.set_picked_item_stack(stack(block::GLASS, 1));
        assert_eq!(inv.current_item, 3);
        assert_eq!(inv.main[3], stack(block::GLASS, 1));
        assert_eq!(inv.main[9], old);
        // Further in: swapped with the best hotbar slot.
        inv.main[20] = stack(block::COBBLESTONE, 7);
        inv.animations[20] = 4;
        inv.set_picked_item_stack(stack(block::COBBLESTONE, 1));
        assert_eq!(inv.current_item, 3);
        assert_eq!(inv.main[3], stack(block::COBBLESTONE, 7));
        assert_eq!(inv.main[20], stack(block::GLASS, 1));
        assert_eq!(inv.get_animations_to_go(3), 4);
        // An empty hotbar slot ahead of the selected one, wrapping, comes first.
        inv.main[1] = ItemStack::EMPTY;
        inv.current_item = 6;
        inv.pick_item(9);
        assert_eq!(inv.current_item, 1);
        assert_eq!(inv.main[1], old);
        assert!(inv.main[9].is_empty());
    }

    #[test]
    fn change_current_item_wraps() {
        let mut inv = InventoryPlayer::new();
        inv.change_current_item(1);
        assert_eq!(inv.current_item, 8);
        inv.change_current_item(-1);
        assert_eq!(inv.current_item, 0);
        inv.change_current_item(-120);
        assert_eq!(inv.current_item, 1);
        inv.current_item = 8;
        inv.change_current_item(-1);
        assert_eq!(inv.current_item, 0);
        assert!(inv.get_current_item().is_empty());
        assert_eq!(inv.get_inventory_stack_limit(), 64);
    }

    #[test]
    fn save_round_trip_with_armor_and_off_hand() {
        let mut inv = InventoryPlayer::new();
        inv.main[0] = stack(block::STONE, 64);
        inv.main[35] = stack(block::PLANKS, 3);
        inv.armor[0] = stack(item::IRON_BOOTS, 1);
        inv.armor[3] = ItemStack::new(item::IRON_HELMET, 1, 12);
        inv.off_hand[0] = stack(item::SHIELD, 1);
        inv.current_item = 5;
        let saved = inv.to_save();
        assert_eq!(
            saved,
            vec![
                (0, stack(block::STONE, 64)),
                (35, stack(block::PLANKS, 3)),
                (100, stack(item::IRON_BOOTS, 1)),
                (103, ItemStack::new(item::IRON_HELMET, 1, 12)),
                (-106, stack(item::SHIELD, 1)),
            ]
        );
        assert_eq!(OFF_HAND_SAVE_SLOT, -106);
        let mut back = InventoryPlayer::new();
        back.main[7] = stack(block::DIRT, 1);
        back.from_save(&saved);
        assert_eq!(back.main, inv.main);
        assert_eq!(back.armor, inv.armor);
        assert_eq!(back.off_hand, inv.off_hand);
        assert!(back.main[7].is_empty());
        // readFromNBT reads the byte unsigned: 150 is the off hand too; unknown slots are skipped.
        back.from_save(&[(150, stack(item::SHIELD, 1)), (36, stack(block::DIRT, 1)), (104, stack(block::DIRT, 1))]);
        assert_eq!(back.off_hand[0], stack(item::SHIELD, 1));
        assert!(back.main.iter().all(|s| s.is_empty()));
        assert!(back.armor.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn animations_count_down_and_drops_empty_everything() {
        let mut inv = InventoryPlayer::new();
        let mut s = stack(block::STONE, 3);
        inv.add_item_stack_to_inventory(&mut s, false);
        inv.armor[2] = stack(item::IRON_HELMET, 1);
        for _ in 0..3 {
            inv.decrement_animations();
        }
        assert_eq!(inv.get_animations_to_go(0), 2);
        for _ in 0..4 {
            inv.decrement_animations();
        }
        assert_eq!(inv.get_animations_to_go(0), 0);

        let taken = inv.decr_stack_size(0, 2);
        assert_eq!(taken, stack(block::STONE, 2));
        assert_eq!(inv.main[0].count, 1);
        assert_eq!(inv.remove_stack_from_slot(0), stack(block::STONE, 1));
        assert!(inv.main[0].is_empty());

        inv.main[4] = stack(block::DIRT, 9);
        inv.off_hand[0] = stack(item::SHIELD, 1);
        inv.delete_stack(&stack(item::SHIELD, 1));
        assert!(inv.off_hand[0].is_empty());
        inv.off_hand[0] = stack(item::SHIELD, 1);
        assert_eq!(inv.clear_matching_items(Some(block::DIRT), -1, 4), 4);
        assert_eq!(inv.main[4].count, 5);

        let dropped = inv.drop_all_items();
        assert_eq!(dropped, vec![stack(block::DIRT, 5), stack(item::IRON_HELMET, 1), stack(item::SHIELD, 1)]);
        assert!(inv.is_empty());
    }
}
