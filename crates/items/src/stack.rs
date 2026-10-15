//! `ItemStack`: an item id, a count and a damage (metadata), with the comparisons crafting,
//! furnaces and inventories use.
//!
//! The NBT tag compound (`stackTagCompound`) is reduced to [`Tag`] (a potion type or a spawn
//! egg's entity) and the enchantment list [`EnchList`] (`ench`, or `StoredEnchantments` on an
//! enchanted book). Tag comparisons compare both. DROPPED: the rest of NBT (custom names, banner
//! patterns, the `Unbreakable` flag, `RepairCost`), `animationsToGo` (a client bob after pickup)
//! and the item frame link.

use worldgen::java_random::JavaRandom;

use crate::enchant::{self, id as ench_id};
use crate::registry::{self, ItemDef};
use crate::{ids::item, tools};

/// The most enchantments one stack carries. A table roll gives at most six compatible ones (one
/// of each exclusion family); `push` past this is DROPPED.
pub const MAX_ENCH: usize = 6;

/// The `ench` list of a stack (`StoredEnchantments` on an enchanted book): `(id, level)` pairs in
/// insertion order. A fixed array so a stack stays `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct EnchList {
    len: u8,
    e: [(u8, u8); MAX_ENCH],
}

impl EnchList {
    pub const EMPTY: EnchList = EnchList { len: 0, e: [(0, 0); MAX_ENCH] };

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The pairs, in order.
    pub fn iter(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        self.e[..self.len as usize].iter().copied()
    }

    /// `(id, level)` appended. Returns false when the list is full (DROPPED: Java's list is
    /// unbounded).
    pub fn push(&mut self, id: u8, level: u8) -> bool {
        if (self.len as usize) < MAX_ENCH {
            self.e[self.len as usize] = (id, level);
            self.len += 1;
            true
        } else {
            false
        }
    }

    /// The level stored for `id`, 0 when absent.
    pub fn level_of(&self, id: u8) -> i32 {
        self.iter().find(|(i, _)| *i == id).map_or(0, |(_, l)| l as i32)
    }

    /// The level slot of `id`.
    pub fn level_mut(&mut self, id: u8) -> Option<&mut u8> {
        let n = self.len as usize;
        self.e[..n].iter_mut().find(|(i, _)| *i == id).map(|(_, l)| l)
    }

    /// A list from pairs (the saves and the wire); unknown ids and zero levels are skipped.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (u8, u8)>) -> EnchList {
        let mut l = EnchList::EMPTY;
        for (i, v) in pairs {
            if v > 0 && enchant::exists(i) {
                l.push(i, v);
            }
        }
        l
    }

    /// The flat wire form: `MAX_ENCH` id/level byte pairs, zero padded (level 0 is empty).
    pub fn to_bytes(&self) -> [u8; MAX_ENCH * 2] {
        let mut b = [0u8; MAX_ENCH * 2];
        for (n, (i, l)) in self.iter().enumerate() {
            b[n * 2] = i;
            b[n * 2 + 1] = l;
        }
        b
    }

    pub fn from_bytes(b: &[u8]) -> EnchList {
        EnchList::from_pairs(b.chunks_exact(2).take(MAX_ENCH).map(|c| (c[0], c[1])))
    }
}

/// The part of `stackTagCompound` this port keeps besides the enchantments: one entry, the one
/// the stack's creative sub-item sets. `None` is a stack without a tag compound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Tag {
    #[default]
    None,
    /// `Potion`: a `PotionType` registry id (`PotionUtils.addPotionToItemStack`).
    Potion(u8),
    /// `EntityTag.id` (`ItemMonsterPlacer.applyEntityIdToItemStack`), as an index into
    /// `creative_table::EGGS`.
    Entity(u8),
}

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
    /// `stackTagCompound`, reduced (see [`Tag`]).
    pub tag: Tag,
    /// `ench` (`StoredEnchantments` on an enchanted book).
    pub ench: EnchList,
}

impl ItemStack {
    /// `ItemStack.EMPTY`
    pub const EMPTY: ItemStack = ItemStack { item: item::AIR, count: 0, damage: 0, tag: Tag::None, ench: EnchList::EMPTY };

    /// `new ItemStack(item, amount, meta)`: a negative meta becomes 0.
    pub const fn new(item: u16, count: i32, meta: i32) -> ItemStack {
        ItemStack { item, count, damage: if meta < 0 { 0 } else { meta }, tag: Tag::None, ench: EnchList::EMPTY }
    }

    /// This stack with `tag` as its tag compound (`setTagCompound`).
    pub fn with_tag(mut self, tag: Tag) -> ItemStack {
        self.tag = tag;
        self
    }

    /// This stack with `ench` as its enchantment list.
    pub fn with_ench(mut self, ench: EnchList) -> ItemStack {
        self.ench = ench;
        self
    }

    /// `hasTagCompound()`
    pub fn has_tag_compound(&self) -> bool {
        self.tag != Tag::None || !self.ench.is_empty()
    }

    /// `isItemEnchanted()`: a non-empty `ench` list.
    pub fn is_item_enchanted(&self) -> bool {
        !self.ench.is_empty()
    }

    /// `addEnchantment(ench, level)`: appends, with the level cast to a byte as Java does.
    pub fn add_enchantment(&mut self, id: u8, level: i32) {
        self.ench.push(id, level as u8);
    }

    /// `EnchantmentHelper.getEnchantmentLevel(ench, this)`
    pub fn get_enchantment_level(&self, id: u8) -> i32 {
        enchant::get_enchantment_level(id, self)
    }

    /// `isItemEnchantable()`
    pub fn is_item_enchantable(&self) -> bool {
        enchant::is_item_enchantable(self)
    }

    /// `hasEffect()`: the glint. `Item.hasEffect` is `isItemEnchanted()`; `ItemEnchantedBook`
    /// always, and `ItemAppleGold` also for metadata above 0.
    pub fn has_effect(&self) -> bool {
        match self.get_item() {
            item::ENCHANTED_BOOK => true,
            item::GOLDEN_APPLE => self.is_item_enchanted() || self.get_metadata() > 0,
            _ => self.is_item_enchanted(),
        }
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

    /// `copy()`: the tag compound is copied too.
    pub fn copy(&self) -> ItemStack {
        ItemStack::new(self.item, self.count, self.damage).with_tag(self.tag).with_ench(self.ench)
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

    /// `attemptDamageItem(amount, rand, damager)`: the Unbreaking roll draws from `rand` only when
    /// the stack has Unbreaking. Returns whether the stack broke (`itemDamage > getMaxDamage()`).
    /// DROPPED: the ITEM_DURABILITY_CHANGED advancement trigger.
    pub fn attempt_damage_item(&mut self, mut amount: i32, rand: &mut JavaRandom) -> bool {
        if !self.is_item_stack_damageable() {
            return false;
        }
        if amount > 0 {
            let i = enchant::get_enchantment_level(ench_id::UNBREAKING, self);
            let mut j = 0;
            let mut k = 0;
            while i > 0 && k < amount {
                if enchant::negate_damage(self, i, rand) {
                    j += 1;
                }
                k += 1;
            }
            amount -= j;
            if amount <= 0 {
                return false;
            }
        }
        self.damage += amount;
        self.damage > self.get_max_damage()
    }

    /// `damageItem(amount, entity)`: nothing in creative; a stack that breaks shrinks by one and
    /// its damage resets. Returns whether it broke, where Java calls `renderBrokenItemStack` and
    /// adds the break stat.
    pub fn damage_item(&mut self, amount: i32, creative: bool, rand: &mut JavaRandom) -> bool {
        if creative || !self.is_item_stack_damageable() {
            return false;
        }
        if self.attempt_damage_item(amount, rand) {
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

    /// `isItemStackEqual(other)`: count, item, damage and tag.
    fn is_item_stack_equal(&self, other: &ItemStack) -> bool {
        self.count == other.count
            && self.get_item() == other.get_item()
            && self.damage == other.damage
            && self.tag == other.tag
            && self.ench == other.ench
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

/// `ItemStack.areItemStackTagsEqual(a, b)`: both empty, or both non-empty with equal tags.
pub fn are_item_stack_tags_equal(a: &ItemStack, b: &ItemStack) -> bool {
    if a.is_empty() && b.is_empty() {
        return true;
    }
    !a.is_empty() && !b.is_empty() && a.tag == b.tag && a.ench == b.ench
}

/// `ItemStack.areItemStacksEqual(a, b)`: item, damage, count and tag.
pub fn are_item_stacks_equal(a: &ItemStack, b: &ItemStack) -> bool {
    if a.is_empty() && b.is_empty() {
        return true;
    }
    !a.is_empty() && !b.is_empty() && a.is_item_stack_equal(b)
}
