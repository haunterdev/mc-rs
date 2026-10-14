//! `ItemStack`: an item id, a count and a damage (metadata), with the comparisons crafting,
//! furnaces and inventories use.
//!
//! DROPPED: the NBT tag compound (`stackTagCompound`): enchantments, custom names, potion types,
//! banner patterns and the `Unbreakable` flag. Every tag comparison here treats both stacks as
//! untagged, which is what vanilla does for stacks that never carried a tag. Also dropped:
//! `animationsToGo` (a client bob after pickup) and the item frame link.

use crate::registry::{self, ItemDef};
use crate::{ids::item, tools};

/// `net.minecraft.item.ItemStack`. A plain value: Java mutates stacks in place, Rust callers pass
/// `&mut` where the Java method mutates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ItemStack {
    /// `item`, as an `Item.getIdFromItem` id.
    pub item: u16,
    /// `stackSize`
    pub count: i32,
    /// `itemDamage`: durability used for damageable items, the subtype for the rest.
    pub damage: i32,
}

impl ItemStack {
    /// `ItemStack.EMPTY`
    pub const EMPTY: ItemStack = ItemStack { item: item::AIR, count: 0, damage: 0 };

    /// `new ItemStack(item, amount, meta)`: a negative meta becomes 0.
    pub fn new(item: u16, count: i32, meta: i32) -> ItemStack {
        ItemStack { item, count, damage: meta.max(0) }
    }

    /// `new ItemStack(item)`: one, meta 0.
    pub fn of(item: u16) -> ItemStack {
        ItemStack::new(item, 1, 0)
    }

    /// `isEmpty()`: no item (or `Items.AIR`, or an id nothing is registered under), a count of 0
    /// or less, or a damage outside -32768..=65535.
    #[allow(clippy::if_same_then_else)]
    pub fn is_empty(&self) -> bool {
        if self.item == item::AIR || registry::item(self.item).is_none() {
            true
        } else if self.count <= 0 {
            true
        } else {
            self.damage < -32768 || self.damage > 65535
        }
    }

    /// `getItem()`: `Items.AIR` for an empty stack.
    pub fn get_item(&self) -> u16 {
        if self.is_empty() {
            item::AIR
        } else {
            self.item
        }
    }

    /// The registry entry of `get_item()`.
    pub fn def(&self) -> &'static ItemDef {
        registry::def(self.get_item())
    }

    /// `getCount()`: 0 for an empty stack.
    pub fn get_count(&self) -> i32 {
        if self.is_empty() {
            0
        } else {
            self.count
        }
    }

    /// `setCount(size)`
    pub fn set_count(&mut self, size: i32) {
        self.count = size;
    }

    /// `grow(quantity)`
    pub fn grow(&mut self, quantity: i32) {
        self.count += quantity;
    }

    /// `shrink(quantity)`
    pub fn shrink(&mut self, quantity: i32) {
        self.grow(-quantity);
    }

    /// `copy()`
    pub fn copy(&self) -> ItemStack {
        ItemStack::new(self.item, self.count, self.damage)
    }

    /// `splitStack(amount)`: takes up to `amount` off this stack and returns them.
    pub fn split_stack(&mut self, amount: i32) -> ItemStack {
        let i = amount.min(self.count);
        let mut out = self.copy();
        out.set_count(i);
        self.shrink(i);
        out
    }

    /// `getMetadata()` and `getItemDamage()`
    pub fn get_metadata(&self) -> i32 {
        self.damage
    }

    /// `setItemDamage(meta)`
    pub fn set_item_damage(&mut self, meta: i32) {
        self.damage = meta.max(0);
    }

    /// `getMaxStackSize()`
    pub fn get_max_stack_size(&self) -> i32 {
        self.def().item_stack_limit()
    }

    /// `getMaxDamage()`
    pub fn get_max_damage(&self) -> i32 {
        self.def().max_damage
    }

    /// `getHasSubtypes()`
    pub fn get_has_subtypes(&self) -> bool {
        self.def().has_subtypes
    }

    /// `isItemStackDamageable()`
    pub fn is_item_stack_damageable(&self) -> bool {
        // The Unbreakable tag check is DROPPED with NBT.
        !self.is_empty() && self.def().max_damage > 0
    }

    /// `isItemDamaged()`
    pub fn is_item_damaged(&self) -> bool {
        self.is_item_stack_damageable() && self.damage > 0
    }

    /// `isStackable()`
    pub fn is_stackable(&self) -> bool {
        self.get_max_stack_size() > 1 && (!self.is_item_stack_damageable() || !self.is_item_damaged())
    }

    /// `attemptDamageItem(amount, rand, damager)` with Unbreaking 0, which never draws from the
    /// random. Returns whether the stack broke (`itemDamage > getMaxDamage()`).
    pub fn attempt_damage_item(&mut self, amount: i32) -> bool {
        if !self.is_item_stack_damageable() {
            return false;
        }
        // DROPPED: the Unbreaking roll (EnchantmentDurability.negateDamage) and the
        // ITEM_DURABILITY_CHANGED advancement trigger; enchantments live in NBT.
        self.damage += amount;
        self.damage > self.get_max_damage()
    }

    /// `damageItem(amount, entity)`: nothing in creative; a stack that breaks shrinks by one and
    /// its damage resets. Returns whether it broke, where Java calls `renderBrokenItemStack` and
    /// adds the break stat.
    pub fn damage_item(&mut self, amount: i32, creative: bool) -> bool {
        if creative || !self.is_item_stack_damageable() {
            return false;
        }
        if self.attempt_damage_item(amount) {
            self.shrink(1);
            self.damage = 0;
            return true;
        }
        false
    }

    /// `getDestroySpeed(state)`
    pub fn get_destroy_speed(&self, state: u16) -> f32 {
        tools::destroy_speed(self.get_item(), state)
    }

    /// `canHarvestBlock(state)`
    pub fn can_harvest_block(&self, state: u16) -> bool {
        tools::can_harvest_block(self.get_item(), state)
    }

    /// `isItemEqual(other)`: same item and damage.
    pub fn is_item_equal(&self, other: &ItemStack) -> bool {
        !other.is_empty() && self.item == other.item && self.damage == other.damage
    }

    /// `isItemEqualIgnoreDurability(other)`
    pub fn is_item_equal_ignore_durability(&self, other: &ItemStack) -> bool {
        if !self.is_item_stack_damageable() {
            self.is_item_equal(other)
        } else {
            !other.is_empty() && self.item == other.item
        }
    }

    /// `isItemStackEqual(other)`: count, item and damage (tags are DROPPED).
    fn is_item_stack_equal(&self, other: &ItemStack) -> bool {
        self.count == other.count && self.get_item() == other.get_item() && self.damage == other.damage
    }
}

/// `ItemStack.areItemsEqual(a, b)`. Java's first test is reference identity, which holds for two
/// `ItemStack.EMPTY`s; two empty values count as that here.
pub fn are_items_equal(a: &ItemStack, b: &ItemStack) -> bool {
    if a.is_empty() && b.is_empty() {
        return true;
    }
    !a.is_empty() && !b.is_empty() && a.is_item_equal(b)
}

/// `ItemStack.areItemsEqualIgnoreDurability(a, b)`
pub fn are_items_equal_ignore_durability(a: &ItemStack, b: &ItemStack) -> bool {
    if a.is_empty() && b.is_empty() {
        return true;
    }
    !a.is_empty() && !b.is_empty() && a.is_item_equal_ignore_durability(b)
}

/// `ItemStack.areItemStackTagsEqual(a, b)`: both empty, or both non-empty (no stack carries a
/// tag here).
pub fn are_item_stack_tags_equal(a: &ItemStack, b: &ItemStack) -> bool {
    a.is_empty() == b.is_empty()
}

/// `ItemStack.areItemStacksEqual(a, b)`: item, damage and count.
pub fn are_item_stacks_equal(a: &ItemStack, b: &ItemStack) -> bool {
    if a.is_empty() && b.is_empty() {
        return true;
    }
    !a.is_empty() && !b.is_empty() && a.is_item_stack_equal(b)
}
