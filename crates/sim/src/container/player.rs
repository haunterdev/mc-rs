//! T7a: `net.minecraft.inventory.ContainerPlayer` (window 0), with the crafting pieces it shares
//! with `ContainerWorkbench`: `InventoryCrafting` (an `items::CraftingGrid` whose changes run
//! `onCraftMatrixChanged`), `InventoryCraftResult`, `SlotCrafting` and
//! `Container.slotChangedCraftingGrid` / `clearContainer`.
//!
//! Slots: 0 the crafting result, 1..5 the 2x2 grid, 5..9 armor (head, chest, legs, feet),
//! 9..36 the main inventory (`InventoryPlayer` 9..36), 36..45 the hotbar (0..9), 45 the off hand.
//!
//! DROPPED: `slotChangedCraftingGrid` runs only on the server in Java (`!world.isRemote`); the
//! viewer's copy of the window runs it too, so its prediction of the result slot matches the
//! `SPacketSetSlot(windowId, 0, result)` the server sends, which the server half sends instead.
//! DROPPED: the `doLimitedCrafting` game rule and the recipe book (`isUnlocked`,
//! `unlockRecipes`): the rule is off by default, so every recipe crafts. DROPPED: the crafting
//! statistics and `Item.onCreated` that `SlotCrafting.onCrafting` and
//! `ContainerWorkbench.transferStackInSlot` call (stats, the map and book hooks), with T13.

use std::sync::Arc;

use items::ids::{block, item};
use items::registry;
use items::stack::{are_item_stack_tags_equal, are_items_equal, ItemStack};
use items::{ArmorSlot, CraftingGrid, Kind, Recipes};

use super::{Container, ContainerKind, Inv, PlayerRef, SlotKind};

/// `net.minecraft.inventory.EntityEquipmentSlot`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EquipmentSlot {
    MainHand,
    OffHand,
    Feet,
    Legs,
    Chest,
    Head,
}

impl EquipmentSlot {
    /// `ContainerPlayer.VALID_EQUIPMENT_SLOTS`, the order of the armor slots 5..9.
    pub const VALID_EQUIPMENT_SLOTS: [EquipmentSlot; 4] = [Self::Head, Self::Chest, Self::Legs, Self::Feet];

    /// `getIndex()`: 0 and 1 for the hands, 0..4 feet to head for armor.
    pub fn get_index(self) -> usize {
        match self {
            Self::MainHand | Self::Feet => 0,
            Self::OffHand | Self::Legs => 1,
            Self::Chest => 2,
            Self::Head => 3,
        }
    }

    /// `getSlotType() == Type.ARMOR`
    pub fn is_armor(self) -> bool {
        !matches!(self, Self::MainHand | Self::OffHand)
    }

    /// `ItemArmor.armorType`
    pub fn from_armor(slot: ArmorSlot) -> EquipmentSlot {
        match slot {
            ArmorSlot::Feet => Self::Feet,
            ArmorSlot::Legs => Self::Legs,
            ArmorSlot::Chest => Self::Chest,
            ArmorSlot::Head => Self::Head,
        }
    }
}

/// `EntityLiving.getSlotForItemStack(stack)`: a pumpkin or a skull on the head, armor in its
/// slot, the elytra on the chest, a shield in the off hand, anything else in the main hand.
pub fn get_slot_for_item_stack(stack: &ItemStack) -> EquipmentSlot {
    let id = stack.get_item();
    if id == registry::item_from_block(block::PUMPKIN) || id == item::SKULL {
        return EquipmentSlot::Head;
    }
    if let Some(def) = registry::item(id) {
        if let Kind::Armor(_, slot) = def.kind {
            return EquipmentSlot::from_armor(slot);
        }
    }
    match id {
        item::ELYTRA => EquipmentSlot::Chest,
        // Item.isShield: only ItemShield.
        item::SHIELD => EquipmentSlot::OffHand,
        _ => EquipmentSlot::MainHand,
    }
}

/// `net.minecraft.inventory.InventoryCraftResult`: one slot, every index naming it.
#[derive(Clone, Debug, PartialEq)]
pub struct InventoryCraftResult {
    /// `stackResult.get(0)`
    pub stack: ItemStack,
    /// `recipeUsed`, by `CraftingManager` id.
    pub recipe_used: Option<usize>,
}

impl Default for InventoryCraftResult {
    fn default() -> Self {
        InventoryCraftResult { stack: ItemStack::EMPTY, recipe_used: None }
    }
}

impl InventoryCraftResult {
    /// `getStackInSlot(index)`
    pub fn get_stack_in_slot(&self, _index: usize) -> ItemStack {
        self.stack
    }

    /// `setInventorySlotContents(index, stack)`
    pub fn set_inventory_slot_contents(&mut self, _index: usize, stack: ItemStack) {
        self.stack = stack;
    }

    /// `decrStackSize(index, count)`: `ItemStackHelper.getAndRemove`, the whole stack.
    pub fn decr_stack_size(&mut self, _index: usize, _count: i32) -> ItemStack {
        std::mem::replace(&mut self.stack, ItemStack::EMPTY)
    }

    /// `clear()`
    pub fn clear(&mut self) {
        self.stack = ItemStack::EMPTY;
    }
}

/// A crafting window's `craftMatrix` (`InventoryCrafting`), `craftResult`, its `SlotCrafting`'s
/// `amountCrafted`, and the recipes `CraftingManager` would search.
#[derive(Clone, Debug)]
pub struct Crafting {
    pub craft_matrix: CraftingGrid,
    pub craft_result: InventoryCraftResult,
    /// `SlotCrafting.amountCrafted`
    pub amount_crafted: i32,
    pub recipes: Arc<Recipes>,
}

impl Crafting {
    /// `new InventoryCrafting(container, width, height)` and `new InventoryCraftResult()`.
    pub fn new(width: usize, height: usize, recipes: Arc<Recipes>) -> Crafting {
        Crafting {
            craft_matrix: CraftingGrid::new(width, height),
            craft_result: InventoryCraftResult::default(),
            amount_crafted: 0,
            recipes,
        }
    }

    /// `Container.slotChangedCraftingGrid(world, player, craftMatrix, craftResult)`: the matching
    /// recipe's result, or nothing, goes into the result slot; `recipeUsed` only changes when a
    /// recipe matched.
    pub fn slot_changed_crafting_grid(&mut self) {
        let (used, itemstack) = match self.recipes.find_matching_recipe(&self.craft_matrix) {
            Some(r) => (Some(r.id), r.recipe.get_crafting_result(&self.craft_matrix)),
            None => (None, ItemStack::EMPTY),
        };
        if used.is_some() {
            self.craft_result.recipe_used = used;
        }
        self.craft_result.set_inventory_slot_contents(0, itemstack);
    }

    /// `SlotCrafting.onCrafting(stack)`: resets `amountCrafted` and forgets a non-dynamic
    /// `recipeUsed` (which Java first unlocks in the recipe book, DROPPED).
    pub fn on_crafting(&mut self) {
        self.amount_crafted = 0;
        if let Some(id) = self.craft_result.recipe_used {
            let dynamic = self.recipes.by_id(id).is_some_and(|r| r.recipe.is_dynamic());
            if !dynamic {
                self.craft_result.recipe_used = None;
            }
        }
    }
}

impl Container {
    /// `new ContainerPlayer(playerInventory, localWorld, player)`
    pub fn player(recipes: Arc<Recipes>) -> Container {
        let mut c = Container::empty(ContainerKind::Player, 0, Some(Crafting::new(2, 2, recipes)));
        c.add_slot_to_container(Inv::Result, 0, 154, 28, SlotKind::Crafting);
        for i in 0..2 {
            for j in 0..2 {
                c.add_slot_to_container(Inv::Craft, j + i * 2, 98 + j as i32 * 18, 18 + i as i32 * 18, SlotKind::Plain);
            }
        }
        for (k, e) in EquipmentSlot::VALID_EQUIPMENT_SLOTS.into_iter().enumerate() {
            c.add_slot_to_container(Inv::Player, 36 + (3 - k), 8, 8 + k as i32 * 18, SlotKind::Armor(e));
        }
        for l in 0..3 {
            for j1 in 0..9 {
                c.add_slot_to_container(Inv::Player, j1 + (l + 1) * 9, 8 + j1 as i32 * 18, 84 + l as i32 * 18, SlotKind::Plain);
            }
        }
        for i1 in 0..9 {
            c.add_slot_to_container(Inv::Player, i1, 8 + i1 as i32 * 18, 142, SlotKind::Plain);
        }
        // The off hand is a plain `Slot` that only overrides `getSlotTexture`.
        c.add_slot_to_container(Inv::Player, 40, 77, 62, SlotKind::Plain);
        c
    }

    /// `ContainerPlayer.transferStackInSlot(player, index)`
    pub(crate) fn transfer_player(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        if !self.get_has_stack(p, index) {
            return ItemStack::EMPTY;
        }
        let mut itemstack1 = self.get_stack(p, index);
        let itemstack = itemstack1.copy();
        let e = get_slot_for_item_stack(&itemstack);
        let (start, end, reverse) = if index == 0 {
            (9, 45, true)
        } else if (1..9).contains(&index) {
            (9, 45, false)
        } else if e.is_armor() && !self.get_has_stack(p, 8 - e.get_index()) {
            let i = 8 - e.get_index();
            (i, i + 1, false)
        } else if e == EquipmentSlot::OffHand && !self.get_has_stack(p, 45) {
            (45, 46, false)
        } else if (9..36).contains(&index) {
            (36, 45, false)
        } else if (36..45).contains(&index) {
            (9, 36, false)
        } else {
            (9, 45, false)
        };
        if !self.merge_item_stack(p, &mut itemstack1, start, end, reverse) {
            return ItemStack::EMPTY;
        }
        if index == 0 {
            self.on_slot_change(index, &itemstack1, &itemstack);
        }
        self.finish_transfer(p, index, itemstack1, itemstack)
    }

    /// `SlotCrafting.onTake(player, stack)`: takes one of every grid stack, puts the recipe's
    /// container items (a milk bucket's bucket) back into the grid or the inventory, and drops
    /// what fits nowhere. Each grid change recomputes the result, as Java's events do.
    pub(crate) fn slot_crafting_on_take(&mut self, p: &mut PlayerRef, stack: ItemStack) -> ItemStack {
        self.crafting_mut().on_crafting();
        let nonnulllist = {
            let c = self.crafting();
            c.recipes.get_remaining_items(&c.craft_matrix)
        };
        for (i, mut itemstack1) in nonnulllist.into_iter().enumerate() {
            let mut itemstack = self.inv_get(p, Inv::Craft, i);
            if !itemstack.is_empty() {
                self.inv_decr(p, Inv::Craft, i, 1);
                itemstack = self.inv_get(p, Inv::Craft, i);
            }
            if !itemstack1.is_empty() {
                if itemstack.is_empty() {
                    self.inv_set(p, Inv::Craft, i, itemstack1);
                } else if are_items_equal(&itemstack, &itemstack1) && are_item_stack_tags_equal(&itemstack, &itemstack1) {
                    itemstack1.grow(itemstack.get_count());
                    self.inv_set(p, Inv::Craft, i, itemstack1);
                } else {
                    let creative = p.creative;
                    if !p.inventory.add_item_stack_to_inventory(&mut itemstack1, creative) {
                        p.drop_item(itemstack1);
                    }
                }
            }
        }
        stack
    }

    /// `Container.clearContainer(player, world, craftMatrix)`: every grid stack goes back into
    /// the inventory, or is dropped when the player is dead or gone. `removeStackFromSlot`
    /// fires no `onCraftMatrixChanged`.
    pub(crate) fn clear_container(&mut self, p: &mut PlayerRef) {
        let n = self.crafting().craft_matrix.slots.len();
        for i in 0..n {
            let grid = &mut self.crafting_mut().craft_matrix;
            let s = grid.get_stack_in_slot(i);
            grid.set_inventory_slot_contents(i, ItemStack::EMPTY);
            if p.alive {
                place_item_back_in_inventory(p, s);
            } else {
                p.drop_item(s);
            }
        }
    }
}

/// `InventoryPlayer.placeItemBackInInventory(world, stack)`: tops up a matching stack or fills
/// the first empty slot, a slot's worth at a time, and drops the rest when nothing is left.
/// DROPPED: the `SPacketSetSlot(-2, slot, stack)` each step sends; the window sync covers it.
pub fn place_item_back_in_inventory(p: &mut PlayerRef, mut stack: ItemStack) {
    while !stack.is_empty() {
        let mut i = p.inventory.store_item_stack(&stack);
        if i == -1 {
            i = p.inventory.get_first_empty_stack();
        }
        if i == -1 {
            p.drop_item(stack);
            break;
        }
        let j = stack.get_max_stack_size() - p.inventory.get_stack_in_slot(i as usize).get_count();
        let mut part = stack.split_stack(j);
        let creative = p.creative;
        p.inventory.add(i, &mut part, creative);
    }
}
