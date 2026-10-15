//! `net.minecraft.inventory.ContainerEnchantment`, the enchanting table's window.
//!
//! Slots: 0 the item (`getSlotStackLimit` 1), 1 the lapis (blue dye only), 2..29 the main inventory
//! (`InventoryPlayer` 9..36), 29..38 the hotbar (0..9). The two table slots are the container's own
//! `InventoryBasic` ([`EnchantTable::items`]); closing gives them back (`clearContainer`).
//!
//! Java's `onCraftMatrixChanged` only computes the offers on the server (`!world.isRemote`): here
//! [`EnchantTable::power`] is `Some(bookshelf count)` on the server copy, set from the world
//! before a click, and `None` on the viewer copy, which only zeroes the offers when the item
//! goes away. The ten window properties of `broadcastData` reach the viewer through
//! [`Container::prop_listener`] (0..3 levels, 3 the seed `& -16`, 4..7 clues, 7..10 world clues).
//!
//! DROPPED: the `ENCHANTED_ITEM` advancement trigger, the `ITEM_ENCHANTED` stat, and the enchantment
//! table sound (the server plays it, the container has no world).

use items::enchant::{self, EnchantmentData};
use items::ids::item;
use items::stack::ItemStack;
use items::JavaRandom;

use super::{Container, ContainerKind, Inv, PlayerRef, SlotKind};

/// `ContainerEnchantment`'s fields.
#[derive(Clone, Debug, PartialEq)]
pub struct EnchantTable {
    /// `tableInventory`: the item and the lapis.
    pub items: [ItemStack; 2],
    /// `xpSeed`
    pub xp_seed: i32,
    /// `enchantLevels`
    pub levels: [i32; 3],
    /// `enchantClue`: the enchantment id shown on hover, -1 for none.
    pub clue: [i32; 3],
    /// `worldClue`: its level, -1 for none.
    pub world_clue: [i32; 3],
    /// The bookshelf count of the server copy; `None` on a client copy (`world.isRemote`).
    pub power: Option<i32>,
    /// The ten properties as last sent.
    pub props_sent: [i32; 10],
}

impl EnchantTable {
    pub fn new(xp_seed: i32, power: Option<i32>) -> EnchantTable {
        EnchantTable { items: [ItemStack::EMPTY; 2], xp_seed, levels: [0; 3], clue: [-1; 3], world_clue: [-1; 3], power, props_sent: [i32::MIN; 10] }
    }

    /// `broadcastData`: the ten properties in order.
    pub fn props(&self) -> [i32; 10] {
        [
            self.levels[0],
            self.levels[1],
            self.levels[2],
            self.xp_seed & -16,
            self.clue[0],
            self.clue[1],
            self.clue[2],
            self.world_clue[0],
            self.world_clue[1],
            self.world_clue[2],
        ]
    }

    /// `getLapisAmount`
    pub fn lapis_amount(&self) -> i32 {
        if self.items[1].is_empty() {
            0
        } else {
            self.items[1].get_count()
        }
    }

    /// `getEnchantmentList(stack, enchantSlot, level)`
    fn enchantment_list(&self, rand: &mut JavaRandom, stack: &ItemStack, slot: i32, level: i32) -> Vec<EnchantmentData> {
        *rand = JavaRandom::new(self.xp_seed.wrapping_add(slot) as i64);
        let mut list = enchant::build_enchantment_list(rand, stack, level, false);
        if stack.get_item() == item::BOOK && list.len() > 1 {
            let k = rand.next_int_bounded(list.len() as i32) as usize;
            list.remove(k);
        }
        list
    }

    /// `onCraftMatrixChanged(tableInventory)`, given the bookshelf count of the server copy.
    fn recompute(&mut self) {
        let stack = self.items[0];
        if !stack.is_empty() && stack.is_item_enchantable() {
            let Some(l) = self.power else { return };
            let mut rand = JavaRandom::new(self.xp_seed as i64);
            for i in 0..3 {
                self.levels[i] = enchant::calc_item_stack_enchantability(&mut rand, i as i32, l, &stack);
                self.clue[i] = -1;
                self.world_clue[i] = -1;
                if self.levels[i] < i as i32 + 1 {
                    self.levels[i] = 0;
                }
            }
            for j in 0..3 {
                if self.levels[j] > 0 {
                    let list = self.enchantment_list(&mut rand, &stack, j as i32, self.levels[j]);
                    if !list.is_empty() {
                        let k = rand.next_int_bounded(list.len() as i32) as usize;
                        self.clue[j] = list[k].id as i32;
                        self.world_clue[j] = list[k].level;
                    }
                }
            }
        } else {
            self.levels = [0; 3];
            self.clue = [-1; 3];
            self.world_clue = [-1; 3];
        }
    }
}

/// The bookshelves around a table (`ContainerEnchantment.onCraftMatrixChanged`'s loop): `air(dx,
/// dy, dz)` and `shelf(dx, dy, dz)` read the world at an offset from the table.
pub fn count_bookshelves(air: impl Fn(i32, i32, i32) -> bool, shelf: impl Fn(i32, i32, i32) -> bool) -> i32 {
    let mut l = 0;
    for j in -1..=1 {
        for k in -1..=1 {
            if (j != 0 || k != 0) && air(k, 0, j) && air(k, 1, j) {
                if shelf(k * 2, 0, j * 2) {
                    l += 1;
                }
                if shelf(k * 2, 1, j * 2) {
                    l += 1;
                }
                if k != 0 && j != 0 {
                    if shelf(k * 2, 0, j) {
                        l += 1;
                    }
                    if shelf(k * 2, 1, j) {
                        l += 1;
                    }
                    if shelf(k, 0, j * 2) {
                        l += 1;
                    }
                    if shelf(k, 1, j * 2) {
                        l += 1;
                    }
                }
            }
        }
    }
    l
}

/// `EnumDyeColor.BLUE.getDyeDamage()`: lapis lazuli.
pub fn is_lapis(stack: &ItemStack) -> bool {
    stack.get_item() == item::DYE && stack.get_metadata() == 4
}

/// What [`Container::enchant_item`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enchanted {
    /// `enchantItem` returned false: not enough lapis or levels, or no offer.
    Refused,
    /// It returned true with nothing to apply (a client copy, or an empty list).
    Accepted,
    /// The item was enchanted for `cost` levels; the player's `xpSeed` becomes `new_seed`.
    Done { cost: i32, new_seed: i32 },
}

impl Container {
    /// `new ContainerEnchantment(playerInv, world, pos)` with the player's `xpSeed`. `power` is the
    /// server's bookshelf count (`Some`), or `None` for the viewer's copy.
    pub fn enchanting(window_id: i32, pos: [i32; 3], xp_seed: i32, power: Option<i32>) -> Container {
        let mut c = Container::empty(ContainerKind::Enchanting { pos }, window_id, None);
        c.enchant = Some(EnchantTable::new(xp_seed, power));
        c.add_slot_to_container(Inv::Enchant, 0, 15, 47, SlotKind::EnchantItem);
        c.add_slot_to_container(Inv::Enchant, 1, 35, 47, SlotKind::EnchantLapis);
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

    pub fn enchant_ref(&self) -> &EnchantTable {
        self.enchant.as_ref().expect("an enchanting slot in a container without a table")
    }

    pub(crate) fn enchant_mut(&mut self) -> &mut EnchantTable {
        self.enchant.as_mut().expect("an enchanting slot in a container without a table")
    }

    /// `onCraftMatrixChanged(tableInventory)`: the offers from the item in slot 0.
    pub fn on_enchant_changed(&mut self) {
        if let Some(t) = &mut self.enchant {
            t.recompute();
        }
    }

    /// The server's bookshelf count changed (the world around the table); the offers follow.
    pub fn set_bookshelf_power(&mut self, power: i32) {
        if let Some(t) = &mut self.enchant {
            if t.power != Some(power) {
                t.power = Some(power);
                t.recompute();
            }
        }
    }

    /// `ContainerEnchantment.updateProgressBar(id, data)`.
    pub fn update_enchant_property(&mut self, id: u16, data: i32) {
        let Some(t) = &mut self.enchant else { return };
        match id {
            0..=2 => t.levels[id as usize] = data,
            3 => t.xp_seed = data,
            4..=6 => t.clue[id as usize - 4] = data,
            7..=9 => t.world_clue[id as usize - 7] = data,
            _ => {}
        }
    }

    /// `ContainerEnchantment.detectAndSendChanges`' `broadcastData`: the properties that differ from
    /// what was last sent go to [`Container::prop_listener`].
    pub(crate) fn detect_enchant_props(&mut self) {
        let Some(t) = &mut self.enchant else { return };
        let now = t.props();
        for (n, v) in now.iter().enumerate() {
            if t.props_sent[n] != *v {
                t.props_sent[n] = *v;
                self.prop_listener.push((n as u16, *v));
            }
        }
    }

    /// `ContainerEnchantment.enchantItem(player, id)` for a player with `xp_level` levels.
    /// `new_seed` draws `EntityPlayer.rand.nextInt()` for `onEnchant`. The caller takes the levels
    /// (`addExperienceLevel(-cost)`) and stores the new seed in the player.
    pub fn enchant_item(&mut self, creative: bool, xp_level: i32, id: usize, new_seed: impl FnOnce() -> i32) -> Enchanted {
        if id > 2 {
            return Enchanted::Refused;
        }
        let t = self.enchant_ref();
        let (stack, lapis) = (t.items[0], t.items[1]);
        let i = id as i32 + 1;
        if (lapis.is_empty() || lapis.get_count() < i) && !creative {
            return Enchanted::Refused;
        }
        if !(t.levels[id] > 0 && !stack.is_empty() && (xp_level >= i && xp_level >= t.levels[id] || creative)) {
            return Enchanted::Refused;
        }
        if t.power.is_none() {
            return Enchanted::Accepted;
        }
        let mut rand = JavaRandom::new(0);
        let list = t.enchantment_list(&mut rand, &stack, id as i32, t.levels[id]);
        if list.is_empty() {
            return Enchanted::Accepted;
        }
        // playerIn.onEnchant(itemstack, i): the levels (the caller) and the new xpSeed.
        let seed = new_seed();
        let t = self.enchant_mut();
        let mut stack = stack;
        if stack.get_item() == item::BOOK {
            stack = ItemStack::of(item::ENCHANTED_BOOK);
        }
        enchant::apply_list(&mut stack, &list);
        t.items[0] = stack;
        if !creative {
            let mut l = t.items[1];
            l.shrink(i);
            t.items[1] = if l.is_empty() { ItemStack::EMPTY } else { l };
        }
        // tableInventory.markDirty(); xpSeed = player.getXPSeed(); onCraftMatrixChanged
        t.xp_seed = seed;
        t.recompute();
        Enchanted::Done { cost: i, new_seed: seed }
    }

    /// `ContainerEnchantment.transferStackInSlot(player, index)`
    pub(crate) fn transfer_enchanting(&mut self, p: &mut PlayerRef, index: usize) -> ItemStack {
        if !self.get_has_stack(p, index) {
            return ItemStack::EMPTY;
        }
        let mut itemstack1 = self.get_stack(p, index);
        let itemstack = itemstack1.copy();
        if index == 0 || index == 1 {
            if !self.merge_item_stack(p, &mut itemstack1, 2, 38, true) {
                return ItemStack::EMPTY;
            }
        } else if is_lapis(&itemstack1) {
            if !self.merge_item_stack(p, &mut itemstack1, 1, 2, true) {
                return ItemStack::EMPTY;
            }
        } else {
            if self.get_has_stack(p, 0) || !self.is_item_valid(0, &itemstack1) {
                return ItemStack::EMPTY;
            }
            if itemstack1.has_tag_compound() && itemstack1.get_count() == 1 {
                self.put_stack(p, 0, itemstack1.copy());
                itemstack1.set_count(0);
            } else if !itemstack1.is_empty() {
                self.put_stack(p, 0, ItemStack::new(itemstack1.get_item(), 1, itemstack1.get_metadata()));
                itemstack1.shrink(1);
            }
        }
        self.finish_transfer(p, index, itemstack1, itemstack)
    }

    /// `ContainerEnchantment.onContainerClosed`' `clearContainer(player, world, tableInventory)`.
    pub(crate) fn clear_enchanting(&mut self, p: &mut PlayerRef) {
        for i in 0..2 {
            let s = self.enchant_ref().items[i];
            self.enchant_mut().items[i] = ItemStack::EMPTY;
            if p.alive {
                super::player::place_item_back_in_inventory(p, s);
            } else {
                p.drop_item(s);
            }
        }
    }
}
