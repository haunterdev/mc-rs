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

mod clicks;
pub mod chest;
pub mod creative;
pub mod enchant;
pub mod furnace;
pub mod player;
pub mod workbench;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_craft;
#[cfg(test)]
mod tests_creative;
#[cfg(test)]
mod tests_enchant;

use items::stack::{are_item_stack_tags_equal, ItemStack};

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
    /// `GuiContainerCreative.basicInventory`, the 45 slots the item list scrolls through.
    Basic,
    /// The tile entity's inventory: a chest (or the two halves of a large one) or a furnace,
    /// [`Container::tile`].
    Tile,
    /// `ContainerEnchantment.tableInventory`: the item and the lapis, [`Container::enchant`].
    Enchant,
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
    /// `SlotFurnaceFuel`
    FurnaceFuel,
    /// `SlotFurnaceOutput`
    FurnaceOutput,
    /// `ContainerEnchantment`'s item slot: any stack, one at a time.
    EnchantItem,
    /// `ContainerEnchantment`'s lapis slot: blue dye only.
    EnchantLapis,
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
    /// `GuiContainerCreative.ContainerCreative`, the creative item tabs. Client only.
    Creative,
    /// `ContainerChest` over a `TileEntityChest` at `pos`, or an `InventoryLargeChest` of it
    /// and the chest at `other` (`pos` is the half that was clicked). `rows` is 3 or 6.
    Chest { pos: [i32; 3], other: Option<[i32; 3]>, rows: u8 },
    /// `ContainerFurnace` over the `TileEntityFurnace` at `pos`.
    Furnace { pos: [i32; 3] },
    /// `ContainerEnchantment` at the enchanting table at `pos`.
    Enchanting { pos: [i32; 3] },
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
    /// `basicInventory` and `itemList` of the creative window.
    pub creative: Option<creative::Creative>,
    /// The tile entity's inventory as this copy sees it. The server refreshes it from the
    /// world's tile before each click and writes it back after; the viewer's copy is fed by
    /// `S_SLOTS` and `S_WINDOW_PROP`.
    pub tile: Option<Tile>,
    /// The furnace fields as last sent, in `TileEntityFurnace.getField` order (burn time,
    /// current item burn time, cook time, total cook time).
    pub props_sent: [i32; 4],
    /// `IContainerListener.sendWindowProperty` calls `detectAndSendChanges` made: (field, value).
    pub prop_listener: Vec<(u16, i32)>,
    /// `SlotFurnaceOutput.removeCount`: what has been taken from the output since the last
    /// `onCrafting`.
    pub furnace_remove_count: i32,
    /// Each `SlotFurnaceOutput.onCrafting(stack)` since the server last drained them: the output
    /// stack and the `removeCount` it paid out. The server turns them into experience orbs
    /// (`items::smelting::furnace_output_xp`), since the container has no world to spawn them in.
    pub furnace_xp: Vec<(ItemStack, i32)>,
    /// `ContainerEnchantment`'s table inventory and offers.
    pub enchant: Option<enchant::EnchantTable>,
}

/// What [`Inv::Tile`] is.
#[derive(Clone, Debug, PartialEq)]
pub enum Tile {
    /// `TileEntityChest.chestContents` (27), or the two halves of an `InventoryLargeChest` (54):
    /// the first half's 27 stacks, then the second's.
    Chest(Vec<ItemStack>),
    /// `TileEntityFurnace`
    Furnace(items::Furnace),
}

impl Tile {
    /// `getStackInSlot`
    pub fn get(&self, i: usize) -> ItemStack {
        match self {
            Tile::Chest(v) => v[i],
            Tile::Furnace(f) => f.items[i],
        }
    }

    /// `setInventorySlotContents`: a chest clamps to its stack limit; a furnace restarts its
    /// cook timer on a new input.
    pub fn set(&mut self, i: usize, mut stack: ItemStack) {
        match self {
            Tile::Chest(v) => {
                if stack.get_count() > 64 {
                    stack.set_count(64);
                }
                v[i] = stack;
            }
            Tile::Furnace(f) => f.set_inventory_slot_contents(i, stack),
        }
    }

    /// A change in place (Java's `grow`/`shrink` on the held object): no inventory event.
    pub fn set_raw(&mut self, i: usize, stack: ItemStack) {
        match self {
            Tile::Chest(v) => v[i] = stack,
            Tile::Furnace(f) => f.items[i] = stack,
        }
    }

    /// `decrStackSize` = `ItemStackHelper.getAndSplit`
    pub fn decr(&mut self, i: usize, count: i32) -> ItemStack {
        let mut s = self.get(i);
        if count <= 0 || s.is_empty() {
            return ItemStack::EMPTY;
        }
        let out = s.split_stack(count);
        self.set_raw(i, if s.is_empty() { ItemStack::EMPTY } else { s });
        out
    }
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
            creative: None,
            tile: None,
            props_sent: [0; 4],
            prop_listener: Vec::new(),
            furnace_remove_count: 0,
            furnace_xp: Vec::new(),
            enchant: None,
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
            Inv::Basic => self.basic_inv().basic[index],
            Inv::Tile => self.tile_ref().get(index),
            Inv::Enchant => self.enchant_ref().items[index],
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
            Inv::Basic => self.basic_inv_mut().set_basic(index, stack),
            Inv::Tile => self.tile_mut().set(index, stack),
            // InventoryBasic.setInventorySlotContents: markDirty runs onCraftMatrixChanged.
            Inv::Enchant => {
                self.enchant_mut().items[index] = stack;
                self.on_enchant_changed();
            }
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
            Inv::Basic => self.basic_inv_mut().decr_basic(index, count),
            Inv::Tile => self.tile_mut().decr(index, count),
            Inv::Enchant => {
                let mut s = self.enchant_ref().items[index];
                if count <= 0 || s.is_empty() {
                    return ItemStack::EMPTY;
                }
                let out = s.split_stack(count);
                self.enchant_mut().items[index] = if s.is_empty() { ItemStack::EMPTY } else { s };
                self.on_enchant_changed();
                out
            }
        }
    }

    /// `IInventory.getInventoryStackLimit()`: 64 for all four.
    fn inv_limit(&self, inv: Inv) -> i32 {
        match inv {
            Inv::Player | Inv::Craft | Inv::Result | Inv::Basic | Inv::Tile | Inv::Enchant => 64,
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
            Inv::Basic => self.basic_inv_mut().set_basic(s.index, stack),
            Inv::Tile => self.tile_mut().set_raw(s.index, stack),
            // Slot.onSlotChanged after the in-place change: InventoryBasic.markDirty.
            Inv::Enchant => {
                self.enchant_mut().items[s.index] = stack;
                self.on_enchant_changed();
            }
        }
    }

    fn tile_ref(&self) -> &Tile {
        self.tile.as_ref().expect("a tile slot in a container without a tile")
    }

    fn tile_mut(&mut self) -> &mut Tile {
        self.tile.as_mut().expect("a tile slot in a container without a tile")
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
        if s.kind == SlotKind::FurnaceOutput && self.get_has_stack(p, i) {
            // SlotFurnaceOutput.decrStackSize
            self.furnace_remove_count += amount.min(self.get_stack(p, i).get_count());
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
            SlotKind::FurnaceOutput => false,
            SlotKind::FurnaceFuel => items::smelting::is_item_fuel(stack) || stack.get_item() == items::ids::item::BUCKET,
            SlotKind::EnchantItem => true,
            SlotKind::EnchantLapis => enchant::is_lapis(stack),
        }
    }

    /// `Slot.getSlotStackLimit()`: the inventory's limit; 1 in an armor slot.
    pub fn get_slot_stack_limit(&self, i: usize) -> i32 {
        match self.slots[i].kind {
            SlotKind::Armor(_) | SlotKind::EnchantItem => 1,
            _ => self.inv_limit(self.slots[i].inventory),
        }
    }

    /// `Slot.getItemStackLimit(stack)`: `getSlotStackLimit()` for every slot here.
    pub fn get_item_stack_limit(&self, i: usize, stack: &ItemStack) -> i32 {
        // SlotFurnaceFuel.getItemStackLimit: one bucket.
        if self.slots[i].kind == SlotKind::FurnaceFuel && stack.get_item() == items::ids::item::BUCKET {
            return 1;
        }
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
            SlotKind::FurnaceOutput => {
                self.furnace_on_crafting(&stack);
                stack
            }
            _ => stack,
        }
    }

    /// `Slot.onSlotChange(oldStack, newStack)`: `onCrafting(newStack, grown)` when it grew.
    /// `SlotFurnaceOutput.onCrafting(stack, amount)` adds the amount to `removeCount` and pays out
    /// (see [`Container::furnace_xp`]).
    pub fn on_slot_change(&mut self, i: usize, old: &ItemStack, new: &ItemStack) {
        let n = new.get_count() - old.get_count();
        if n > 0 {
            self.on_crafting_amount(i, n, new);
        }
    }

    /// `Slot.onCrafting(stack, amount)`: only `SlotCrafting` does anything.
    fn on_crafting_amount(&mut self, i: usize, amount: i32, stack: &ItemStack) {
        match self.slots[i].kind {
            SlotKind::Crafting => {
                self.crafting_mut().amount_crafted += amount;
                self.crafting_mut().on_crafting();
            }
            SlotKind::FurnaceOutput => {
                self.furnace_remove_count += amount;
                self.furnace_on_crafting(stack);
            }
            _ => {}
        }
    }

    /// `SlotFurnaceOutput.onCrafting(stack)`: the stack and `removeCount` for the server's orbs,
    /// then `removeCount = 0`. DROPPED: `stack.onCrafting` (the stats and achievements).
    fn furnace_on_crafting(&mut self, stack: &ItemStack) {
        self.furnace_xp.push((*stack, self.furnace_remove_count));
        self.furnace_remove_count = 0;
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
        self.detect_enchant_props();
        // ContainerFurnace.detectAndSendChanges: the fields that differ from what was sent, in
        // the Java loop's order: cookTime (2), furnaceBurnTime (0), currentItemBurnTime (1),
        // totalCookTime (3).
        if let Some(Tile::Furnace(f)) = &self.tile {
            let now = [f.burn_time, f.current_item_burn_time, f.cook_time, f.total_cook_time];
            for field in [2usize, 0, 1, 3] {
                if self.props_sent[field] != now[field] {
                    self.prop_listener.push((field as u16, now[field]));
                    self.props_sent[field] = now[field];
                }
            }
        }
    }

    /// `Container.transferStackInSlot(player, index)`, the shift-click, per subclass.
    pub fn transfer_stack_in_slot(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        match self.kind {
            ContainerKind::Player => self.transfer_player(p, index),
            ContainerKind::Workbench { .. } => self.transfer_workbench(p, index),
            ContainerKind::Creative => self.transfer_creative(p, index),
            ContainerKind::Chest { .. } => self.transfer_chest(p, index),
            ContainerKind::Furnace { .. } => self.transfer_furnace(p, index),
            ContainerKind::Enchanting { .. } => self.transfer_enchanting(p, index),
        }
    }

    /// `Container.canMergeSlot(stack, slot)`: `ContainerPlayer` and `ContainerWorkbench` keep
    /// the double click out of the crafting result; `ContainerCreative` out of the item list
    /// (`slotIn.yPos > 90`).
    pub fn can_merge_slot(&self, _stack: &ItemStack, i: usize) -> bool {
        match self.kind {
            ContainerKind::Player | ContainerKind::Workbench { .. } => self.slots[i].inventory != Inv::Result,
            ContainerKind::Creative => self.slots[i].y > 90,
            ContainerKind::Chest { .. } | ContainerKind::Furnace { .. } | ContainerKind::Enchanting { .. } => true,
        }
    }

    /// `Container.canDragIntoSlot(slot)`: true, except that `ContainerCreative` keeps a drag out
    /// of the item list.
    pub fn can_drag_into_slot(&self, i: usize) -> bool {
        match self.kind {
            ContainerKind::Creative => {
                let s = self.slots[i];
                s.inventory == Inv::Player || s.y > 90 && s.x <= 162
            }
            _ => true,
        }
    }

    /// `Container.resetDrag()`
    pub fn reset_drag(&mut self) {
        self.drag_event = 0;
        self.drag_slots.clear();
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
