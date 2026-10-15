//! Enchantments: `Enchantment` and its subclasses (rarity, enchantability ranges, `canApply`,
//! `canApplyTogether`, damage modifiers), `EnumEnchantmentType.canEnchantItem`, `EnchantmentHelper`
//! (`calcItemStackEnchantability`, `buildEnchantmentList`, `getEnchantmentDatas`,
//! `removeIncompatible`, the level reads) and `WeightedRandom.getRandomItem`.
//!
//! An enchantment is its 1.12.2 registry id (see `creative_table::ENCHANTMENTS` for the name,
//! lang key, type and level range). The applied list lives on the stack ([`crate::stack::EnchList`]).
//!
//! Bane of Arthropods' Slowness is applied in the server's mobs/combat.rs.
//!
//! DROPPED: `getTranslatedName` colouring (the viewer does it), `getEntityEquipment` (the
//! callers pick the slots), and the equipment-slot arrays.

use worldgen::java_random::JavaRandom;

use crate::creative::{self, EnchType};
use crate::ids::{block, item};
use crate::registry::{self, Kind};
use crate::stack::ItemStack;
use crate::tools::{self, ArmorSlot, ToolClass};

/// `Enchantments.*` registry ids.
pub mod id {
    pub const PROTECTION: u8 = 0;
    pub const FIRE_PROTECTION: u8 = 1;
    pub const FEATHER_FALLING: u8 = 2;
    pub const BLAST_PROTECTION: u8 = 3;
    pub const PROJECTILE_PROTECTION: u8 = 4;
    pub const RESPIRATION: u8 = 5;
    pub const AQUA_AFFINITY: u8 = 6;
    pub const THORNS: u8 = 7;
    pub const DEPTH_STRIDER: u8 = 8;
    pub const FROST_WALKER: u8 = 9;
    pub const BINDING_CURSE: u8 = 10;
    pub const SHARPNESS: u8 = 16;
    pub const SMITE: u8 = 17;
    pub const BANE_OF_ARTHROPODS: u8 = 18;
    pub const KNOCKBACK: u8 = 19;
    pub const FIRE_ASPECT: u8 = 20;
    pub const LOOTING: u8 = 21;
    pub const SWEEPING: u8 = 22;
    pub const EFFICIENCY: u8 = 32;
    pub const SILK_TOUCH: u8 = 33;
    pub const UNBREAKING: u8 = 34;
    pub const FORTUNE: u8 = 35;
    pub const POWER: u8 = 48;
    pub const PUNCH: u8 = 49;
    pub const FLAME: u8 = 50;
    pub const INFINITY: u8 = 51;
    pub const LUCK_OF_THE_SEA: u8 = 61;
    pub const LURE: u8 = 62;
    pub const MENDING: u8 = 70;
    pub const VANISHING_CURSE: u8 = 71;
}
use id::*;

/// `EnumCreatureAttribute`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreatureAttribute {
    Undefined,
    Undead,
    Arthropod,
}

/// What `calcModifierDamage` reads of a `DamageSource`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageFlags {
    /// `canHarmInCreative()`
    pub can_harm_in_creative: bool,
    /// `isFireDamage()`
    pub fire: bool,
    /// `source == DamageSource.FALL`
    pub fall: bool,
    /// `isExplosion()`
    pub explosion: bool,
    /// `isProjectile()`
    pub projectile: bool,
}

/// An `EnchantmentData`: an enchantment and a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnchantmentData {
    pub id: u8,
    pub level: i32,
}

/// Whether an id is registered.
pub fn exists(e: u8) -> bool {
    creative::enchantment(e).is_some()
}

/// `Enchantment.getRarity().getWeight()`: COMMON 10, UNCOMMON 5, RARE 2, VERY_RARE 1.
pub fn rarity_weight(e: u8) -> i32 {
    match e {
        PROTECTION | SHARPNESS | EFFICIENCY | POWER => 10,
        FIRE_PROTECTION | FEATHER_FALLING | PROJECTILE_PROTECTION | SMITE | BANE_OF_ARTHROPODS | KNOCKBACK | UNBREAKING => 5,
        THORNS | BINDING_CURSE | SILK_TOUCH | INFINITY | VANISHING_CURSE => 1,
        _ => 2,
    }
}

/// `getMinLevel()`
pub fn min_level(_e: u8) -> i32 {
    1
}

/// `getMaxLevel()`
pub fn max_level(e: u8) -> i32 {
    creative::enchantment(e).map_or(1, |d| d.max)
}

/// `EnchantmentProtection.Type`: `(minimal, enchantIncreasePerLevel)` of a protection id.
fn protection_costs(e: u8) -> (i32, i32) {
    match e {
        PROTECTION => (1, 11),
        FIRE_PROTECTION => (10, 8),
        FEATHER_FALLING => (5, 6),
        BLAST_PROTECTION => (5, 8),
        _ => (3, 6),
    }
}

/// `Enchantment.getMinEnchantability(level)` with every override.
pub fn min_enchantability(e: u8, l: i32) -> i32 {
    match e {
        PROTECTION..=PROJECTILE_PROTECTION => {
            let (min, per) = protection_costs(e);
            min + (l - 1) * per
        }
        RESPIRATION | DEPTH_STRIDER | FROST_WALKER => l * 10,
        AQUA_AFFINITY => 1,
        THORNS | FIRE_ASPECT => 10 + 20 * (l - 1),
        BINDING_CURSE | VANISHING_CURSE => 25,
        SHARPNESS => 1 + (l - 1) * 11,
        SMITE | BANE_OF_ARTHROPODS => 5 + (l - 1) * 8,
        KNOCKBACK => 5 + 20 * (l - 1),
        LOOTING | FORTUNE | LUCK_OF_THE_SEA | LURE => 15 + (l - 1) * 9,
        SWEEPING => 5 + (l - 1) * 9,
        EFFICIENCY => 1 + 10 * (l - 1),
        SILK_TOUCH => 15,
        UNBREAKING => 5 + (l - 1) * 8,
        POWER => 1 + (l - 1) * 10,
        PUNCH => 12 + (l - 1) * 20,
        FLAME | INFINITY => 20,
        MENDING => l * 25,
        // Enchantment.getMinEnchantability
        _ => 1 + l * 10,
    }
}

/// `Enchantment.getMaxEnchantability(level)` with every override.
pub fn max_enchantability(e: u8, l: i32) -> i32 {
    match e {
        PROTECTION..=PROJECTILE_PROTECTION => min_enchantability(e, l) + protection_costs(e).1,
        RESPIRATION => min_enchantability(e, l) + 30,
        AQUA_AFFINITY => min_enchantability(e, l) + 40,
        DEPTH_STRIDER | FROST_WALKER => min_enchantability(e, l) + 15,
        BINDING_CURSE | VANISHING_CURSE => 50,
        SHARPNESS => min_enchantability(e, l) + 20,
        SMITE | BANE_OF_ARTHROPODS => min_enchantability(e, l) + 20,
        SWEEPING => min_enchantability(e, l) + 15,
        POWER => min_enchantability(e, l) + 15,
        PUNCH => min_enchantability(e, l) + 25,
        FLAME | INFINITY => 50,
        MENDING => min_enchantability(e, l) + 50,
        // `super.getMinEnchantability(level) + 50` in Thorns, Fire Aspect, Knockback, Looting
        // and the loot family, Efficiency, Silk Touch and Unbreaking: the base 1 + level * 10.
        THORNS | FIRE_ASPECT | KNOCKBACK | LOOTING | FORTUNE | LUCK_OF_THE_SEA | LURE | EFFICIENCY | SILK_TOUCH | UNBREAKING => 1 + l * 10 + 50,
        _ => min_enchantability(e, l) + 5,
    }
}

/// `Enchantment.isTreasureEnchantment()`: frost walker, the two curses and mending.
pub fn is_treasure(e: u8) -> bool {
    matches!(e, FROST_WALKER | BINDING_CURSE | MENDING | VANISHING_CURSE)
}

/// `Enchantment.isCurse()`
pub fn is_curse(e: u8) -> bool {
    matches!(e, BINDING_CURSE | VANISHING_CURSE)
}

/// `EnumEnchantmentType.canEnchantItem(item)`
pub fn type_can_enchant_item(ty: EnchType, item_id: u16) -> bool {
    let Some(def) = registry::item(item_id) else { return false };
    let armor = |slot: Option<ArmorSlot>| match def.kind {
        Kind::Armor(_, s) => slot.is_none_or(|w| w == s),
        _ => false,
    };
    match ty {
        EnchType::All => [
            EnchType::Armor,
            EnchType::ArmorFeet,
            EnchType::ArmorLegs,
            EnchType::ArmorChest,
            EnchType::ArmorHead,
            EnchType::Weapon,
            EnchType::Digger,
            EnchType::FishingRod,
            EnchType::Breakable,
            EnchType::Bow,
            EnchType::Wearable,
        ]
        .into_iter()
        .any(|t| type_can_enchant_item(t, item_id)),
        EnchType::Armor => armor(None),
        EnchType::ArmorFeet => armor(Some(ArmorSlot::Feet)),
        EnchType::ArmorLegs => armor(Some(ArmorSlot::Legs)),
        EnchType::ArmorChest => armor(Some(ArmorSlot::Chest)),
        EnchType::ArmorHead => armor(Some(ArmorSlot::Head)),
        EnchType::Weapon => matches!(def.kind, Kind::Sword(_)),
        EnchType::Digger => matches!(def.kind, Kind::Tool(..)),
        EnchType::FishingRod => item_id == item::FISHING_ROD,
        EnchType::Breakable => def.is_damageable(),
        EnchType::Bow => item_id == item::BOW,
        EnchType::Wearable => {
            let pumpkin = matches!(def.kind, Kind::Block { block: b, .. } if b == block::PUMPKIN || b == block::LIT_PUMPKIN);
            armor(None) || item_id == item::ELYTRA || matches!(def.kind, Kind::Skull) || pumpkin
        }
    }
}

/// `Enchantment.canApply(stack)` with the `EnchantmentDamage` (axes), `EnchantmentDigging`
/// (shears), `EnchantmentDurability` (any damageable stack) and `EnchantmentThorns` (any armor)
/// overrides.
pub fn can_apply(e: u8, stack: &ItemStack) -> bool {
    let Some(d) = creative::enchantment(e) else { return false };
    let it = stack.get_item();
    let Some(ty) = d.ty else { return false };
    let is_axe = matches!(registry::item(it).map(|x| x.kind), Some(Kind::Tool(ToolClass::Axe, _)));
    match e {
        SHARPNESS | SMITE | BANE_OF_ARTHROPODS if is_axe => true,
        EFFICIENCY if it == item::SHEARS => true,
        UNBREAKING if stack.is_item_stack_damageable() => true,
        THORNS if matches!(registry::item(it).map(|x| x.kind), Some(Kind::Armor(..))) => true,
        _ => type_can_enchant_item(ty, it),
    }
}

/// `Enchantment.canApplyTogether(other)`: one direction of `isCompatibleWith`.
fn can_apply_together_one(a: u8, b: u8) -> bool {
    let protection = |e: u8| (PROTECTION..=PROJECTILE_PROTECTION).contains(&e);
    let damage = |e: u8| matches!(e, SHARPNESS | SMITE | BANE_OF_ARTHROPODS);
    match a {
        // EnchantmentProtection: another protection only when the types differ and one is FALL.
        PROTECTION..=PROJECTILE_PROTECTION if protection(b) => a != b && (a == FEATHER_FALLING || b == FEATHER_FALLING),
        // EnchantmentDamage
        SHARPNESS | SMITE | BANE_OF_ARTHROPODS => !damage(b),
        // EnchantmentArrowInfinite
        INFINITY if b == MENDING => false,
        // EnchantmentFrostWalker, EnchantmentWaterWalker
        FROST_WALKER => a != b && b != DEPTH_STRIDER,
        DEPTH_STRIDER => a != b && b != FROST_WALKER,
        // EnchantmentLootBonus (looting, fortune, luck of the sea) and EnchantmentUntouching
        LOOTING | FORTUNE | LUCK_OF_THE_SEA => a != b && b != SILK_TOUCH,
        SILK_TOUCH => a != b && b != FORTUNE,
        _ => a != b,
    }
}

/// `Enchantment.isCompatibleWith(other)`
pub fn is_compatible_with(a: u8, b: u8) -> bool {
    can_apply_together_one(a, b) && can_apply_together_one(b, a)
}

/// `Enchantment.canApplyTogether(other)` as named in the port notes; the symmetric form.
pub fn can_apply_together(a: u8, b: u8) -> bool {
    is_compatible_with(a, b)
}

/// `Enchantment.calcModifierDamage(level, source)`: only the protection family answers.
pub fn calc_modifier_damage(e: u8, level: i32, src: &DamageFlags) -> i32 {
    if !(PROTECTION..=PROJECTILE_PROTECTION).contains(&e) {
        return 0;
    }
    if src.can_harm_in_creative {
        0
    } else if e == PROTECTION {
        level
    } else if e == FIRE_PROTECTION && src.fire {
        level * 2
    } else if e == FEATHER_FALLING && src.fall {
        level * 3
    } else if e == BLAST_PROTECTION && src.explosion {
        level * 2
    } else if e == PROJECTILE_PROTECTION && src.projectile {
        level * 2
    } else {
        0
    }
}

/// `Enchantment.calcDamageByCreature(level, creatureType)`: Sharpness, Smite, Bane of Arthropods.
pub fn calc_damage_by_creature(e: u8, level: i32, ty: CreatureAttribute) -> f32 {
    match e {
        SHARPNESS => 1.0 + (level - 1).max(0) as f32 * 0.5,
        SMITE if ty == CreatureAttribute::Undead => level as f32 * 2.5,
        BANE_OF_ARTHROPODS if ty == CreatureAttribute::Arthropod => level as f32 * 2.5,
        _ => 0.0,
    }
}

/// `EnchantmentThorns.shouldHit(level, rnd)`
pub fn thorns_should_hit(level: i32, rnd: &mut JavaRandom) -> bool {
    if level <= 0 {
        false
    } else {
        rnd.next_float() < 0.15 * level as f32
    }
}

/// `EnchantmentThorns.getDamage(level, rnd)`
pub fn thorns_damage(level: i32, rnd: &mut JavaRandom) -> i32 {
    if level > 10 {
        level - 10
    } else {
        1 + rnd.next_int_bounded(4)
    }
}

/// `EnchantmentDurability.negateDamage(stack, level, rand)`
pub fn negate_damage(stack: &ItemStack, level: i32, rand: &mut JavaRandom) -> bool {
    if matches!(registry::item(stack.get_item()).map(|d| d.kind), Some(Kind::Armor(..))) && rand.next_float() < 0.6 {
        false
    } else {
        rand.next_int_bounded(level + 1) > 0
    }
}

/// `EnchantmentProtection.getFireTimeForEntity(entity, ticks)` given the Fire Protection level.
pub fn fire_time_for_entity(fire_protection: i32, ticks: i32) -> i32 {
    if fire_protection > 0 {
        ticks - ((ticks as f32 * fire_protection as f32 * 0.15) as f64).floor() as i32
    } else {
        ticks
    }
}

/// `EnchantmentProtection.getBlastDamageReduction(entity, damage)` given the Blast Protection level.
pub fn blast_damage_reduction(blast_protection: i32, damage: f64) -> f64 {
    if blast_protection > 0 {
        damage - (damage * (blast_protection as f32 * 0.15) as f64).floor()
    } else {
        damage
    }
}

/// `CombatRules.getDamageAfterMagicAbsorb(damage, enchantModifiers)`
pub fn damage_after_magic_absorb(damage: f32, modifiers: i32) -> f32 {
    let f = modifiers.clamp(0, 20) as f32;
    damage * (1.0 - f / 25.0)
}

/// `EnchantmentSweepingEdge.getSweepingDamageRatio(level)`
pub fn sweeping_damage_ratio(level: i32) -> f32 {
    1.0 - 1.0 / (level + 1) as f32
}

// ---- EnchantmentHelper ----

/// `EnchantmentHelper.getEnchantmentLevel(ench, stack)`: the `ench` list only (a book's stored
/// list is not read).
pub fn get_enchantment_level(e: u8, stack: &ItemStack) -> i32 {
    if stack.is_empty() || stack.get_item() == item::ENCHANTED_BOOK {
        return 0;
    }
    stack.ench.level_of(e)
}

/// `EnchantmentHelper.getEnchantments(stack)`: a book's `StoredEnchantments`, else `ench`.
pub fn get_enchantments(stack: &ItemStack) -> Vec<(u8, i32)> {
    stack.ench.iter().filter(|(id, _)| exists(*id)).map(|(id, l)| (id, l as i32)).collect()
}

/// `EnchantmentHelper.getMaxEnchantmentLevel(ench, living)` over the stacks in the enchantment's
/// equipment slots (the caller passes them).
pub fn get_max_enchantment_level<'a>(e: u8, stacks: impl IntoIterator<Item = &'a ItemStack>) -> i32 {
    stacks.into_iter().map(|s| get_enchantment_level(e, s)).max().unwrap_or(0)
}

/// `EnchantmentHelper.getModifierForCreature(stack, attribute)`: the held stack's Sharpness,
/// Smite and Bane of Arthropods summed.
pub fn get_modifier_for_creature(stack: &ItemStack, attr: CreatureAttribute) -> f32 {
    if stack.is_empty() || stack.get_item() == item::ENCHANTED_BOOK {
        return 0.0;
    }
    stack.ench.iter().filter(|(id, _)| exists(*id)).map(|(id, l)| calc_damage_by_creature(id, l as i32, attr)).sum()
}

/// `EnchantmentHelper.getEnchantmentModifierDamage(stacks, source)`: the protection total.
pub fn get_enchantment_modifier_damage<'a>(stacks: impl IntoIterator<Item = &'a ItemStack>, src: &DamageFlags) -> i32 {
    let mut total = 0;
    for s in stacks {
        if s.is_empty() || s.get_item() == item::ENCHANTED_BOOK {
            continue;
        }
        for (id, l) in s.ench.iter() {
            if exists(id) {
                total += calc_modifier_damage(id, l as i32, src);
            }
        }
    }
    total
}

/// `Item.isEnchantable(stack)` with the `ItemBook` and `ItemEnchantedBook` overrides.
pub fn is_enchantable(stack: &ItemStack) -> bool {
    let it = stack.get_item();
    if it == item::BOOK {
        return stack.get_count() == 1;
    }
    if it == item::ENCHANTED_BOOK {
        return false;
    }
    let d = stack.def();
    d.item_stack_limit() == 1 && d.is_damageable()
}

/// `ItemStack.isItemEnchantable()`
pub fn is_item_enchantable(stack: &ItemStack) -> bool {
    !stack.is_empty() && is_enchantable(stack) && !stack.is_item_enchanted()
}

/// `EnchantmentHelper.calcItemStackEnchantability(rand, enchantNum, power, stack)`
pub fn calc_item_stack_enchantability(rand: &mut JavaRandom, enchant_num: i32, mut power: i32, stack: &ItemStack) -> i32 {
    let i = tools::item_enchantability(stack.get_item());
    if i <= 0 {
        return 0;
    }
    if power > 15 {
        power = 15;
    }
    let j = rand.next_int_bounded(8) + 1 + (power >> 1) + rand.next_int_bounded(power + 1);
    if enchant_num == 0 {
        (j / 3).max(1)
    } else if enchant_num == 1 {
        j * 2 / 3 + 1
    } else {
        j.max(power * 2)
    }
}

/// `EnchantmentHelper.getEnchantmentDatas(level, stack, allowTreasure)`: registry order.
pub fn get_enchantment_datas(level: i32, stack: &ItemStack, allow_treasure: bool) -> Vec<EnchantmentData> {
    let mut list = Vec::new();
    let is_book = stack.get_item() == item::BOOK;
    for d in crate::creative_table::ENCHANTMENTS {
        let e = d.id;
        if (!is_treasure(e) || allow_treasure) && (d.ty.is_some_and(|t| type_can_enchant_item(t, stack.get_item())) || is_book) {
            let mut i = max_level(e);
            while i > min_level(e) - 1 {
                if level >= min_enchantability(e, i) && level <= max_enchantability(e, i) {
                    list.push(EnchantmentData { id: e, level: i });
                    break;
                }
                i -= 1;
            }
        }
    }
    list
}

/// `EnchantmentHelper.removeIncompatible(list, last)`
pub fn remove_incompatible(list: &mut Vec<EnchantmentData>, last: EnchantmentData) {
    list.retain(|d| is_compatible_with(last.id, d.id));
}

/// `WeightedRandom.getRandomItem(random, list)`
fn random_item(rand: &mut JavaRandom, list: &[EnchantmentData]) -> EnchantmentData {
    let total: i32 = list.iter().map(|d| rarity_weight(d.id)).sum();
    let mut w = rand.next_int_bounded(total);
    for d in list {
        w -= rarity_weight(d.id);
        if w < 0 {
            return *d;
        }
    }
    list[0]
}

/// `EnchantmentHelper.buildEnchantmentList(random, stack, level, allowTreasure)`
pub fn build_enchantment_list(rand: &mut JavaRandom, stack: &ItemStack, mut level: i32, allow_treasure: bool) -> Vec<EnchantmentData> {
    let mut list = Vec::new();
    let i = tools::item_enchantability(stack.get_item());
    if i <= 0 {
        return list;
    }
    level = level + 1 + rand.next_int_bounded(i / 4 + 1) + rand.next_int_bounded(i / 4 + 1);
    let f = (rand.next_float() + rand.next_float() - 1.0) * 0.15;
    // Math.round((float)level + (float)level * f), clamped to 1..
    level = (((level as f32 + level as f32 * f) + 0.5).floor() as i32).max(1);
    let mut list1 = get_enchantment_datas(level, stack, allow_treasure);
    if !list1.is_empty() {
        list.push(random_item(rand, &list1));
        while rand.next_int_bounded(50) <= level {
            let last = *list.last().unwrap();
            remove_incompatible(&mut list1, last);
            if list1.is_empty() {
                break;
            }
            list.push(random_item(rand, &list1));
            level /= 2;
        }
    }
    list
}

/// `ItemStack.addEnchantment(ench, level)` for a plain stack, `ItemEnchantedBook.addEnchantment`
/// for a book: a book keeps the higher level of an enchantment it already stores.
pub fn add_enchantment_data(stack: &mut ItemStack, data: EnchantmentData) {
    if stack.get_item() == item::ENCHANTED_BOOK {
        // ItemEnchantedBook.addEnchantment
        if let Some(l) = stack.ench.level_mut(data.id) {
            if (*l as i32) < data.level {
                *l = data.level as u8;
            }
            return;
        }
    }
    stack.add_enchantment(data.id, data.level);
}

/// `EnchantmentHelper.addRandomEnchantment`-style application of a list onto a stack.
pub fn apply_list(stack: &mut ItemStack, list: &[EnchantmentData]) {
    for d in list {
        add_enchantment_data(stack, *d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sword() -> ItemStack {
        ItemStack::of(item::DIAMOND_SWORD)
    }

    #[test]
    fn registry_ranges_match_1_12_2() {
        // Sharpness: 1 + (l-1)*11, span 20. Protection: 1 + (l-1)*11, span 11.
        assert_eq!((min_enchantability(SHARPNESS, 1), max_enchantability(SHARPNESS, 1)), (1, 21));
        assert_eq!((min_enchantability(SHARPNESS, 5), max_enchantability(SHARPNESS, 5)), (45, 65));
        assert_eq!((min_enchantability(PROTECTION, 4), max_enchantability(PROTECTION, 4)), (34, 45));
        assert_eq!((min_enchantability(FIRE_PROTECTION, 1), max_enchantability(FIRE_PROTECTION, 1)), (10, 18));
        assert_eq!((min_enchantability(EFFICIENCY, 3), max_enchantability(EFFICIENCY, 3)), (21, 81));
        assert_eq!((min_enchantability(UNBREAKING, 3), max_enchantability(UNBREAKING, 3)), (21, 81));
        assert_eq!((min_enchantability(SILK_TOUCH, 1), max_enchantability(SILK_TOUCH, 1)), (15, 61));
        assert_eq!((min_enchantability(FORTUNE, 3), max_enchantability(FORTUNE, 3)), (33, 81));
        assert_eq!((min_enchantability(KNOCKBACK, 2), max_enchantability(KNOCKBACK, 2)), (25, 71));
        assert_eq!((min_enchantability(POWER, 5), max_enchantability(POWER, 5)), (41, 56));
        assert_eq!((min_enchantability(FLAME, 1), max_enchantability(FLAME, 1)), (20, 50));
        assert_eq!((min_enchantability(THORNS, 3), max_enchantability(THORNS, 3)), (50, 81));
        assert_eq!(rarity_weight(PROTECTION), 10);
        assert_eq!(rarity_weight(SMITE), 5);
        assert_eq!(rarity_weight(LOOTING), 2);
        assert_eq!(rarity_weight(INFINITY), 1);
    }

    #[test]
    fn exclusions_follow_can_apply_together() {
        assert!(!can_apply_together(SHARPNESS, SMITE));
        assert!(!can_apply_together(SHARPNESS, SHARPNESS));
        assert!(can_apply_together(SHARPNESS, KNOCKBACK));
        assert!(!can_apply_together(PROTECTION, FIRE_PROTECTION));
        assert!(!can_apply_together(BLAST_PROTECTION, PROJECTILE_PROTECTION));
        assert!(can_apply_together(PROTECTION, FEATHER_FALLING));
        assert!(can_apply_together(FEATHER_FALLING, BLAST_PROTECTION));
        assert!(!can_apply_together(SILK_TOUCH, FORTUNE));
        assert!(!can_apply_together(FORTUNE, SILK_TOUCH));
        assert!(!can_apply_together(DEPTH_STRIDER, FROST_WALKER));
        assert!(!can_apply_together(INFINITY, MENDING));
        assert!(can_apply_together(UNBREAKING, MENDING));
        assert!(!can_apply_together(LOOTING, LOOTING));
    }

    #[test]
    fn types_decide_what_applies() {
        assert!(can_apply(SHARPNESS, &sword()));
        assert!(can_apply(SHARPNESS, &ItemStack::of(item::DIAMOND_AXE)));
        assert!(!can_apply(SHARPNESS, &ItemStack::of(item::DIAMOND_PICKAXE)));
        assert!(can_apply(EFFICIENCY, &ItemStack::of(item::SHEARS)));
        assert!(can_apply(UNBREAKING, &ItemStack::of(item::BOW)));
        assert!(can_apply(POWER, &ItemStack::of(item::BOW)));
        assert!(!can_apply(POWER, &sword()));
        assert!(can_apply(FEATHER_FALLING, &ItemStack::of(item::DIAMOND_BOOTS)));
        assert!(!can_apply(FEATHER_FALLING, &ItemStack::of(item::DIAMOND_HELMET)));
        assert!(can_apply(THORNS, &ItemStack::of(item::DIAMOND_HELMET)));
        assert!(can_apply(RESPIRATION, &ItemStack::of(item::DIAMOND_HELMET)));
    }

    #[test]
    fn enchantability_math_matches_known_values() {
        // item enchantability: diamond 10, iron 14 (tools), gold 22, leather 15.
        assert_eq!(tools::item_enchantability(item::DIAMOND_SWORD), 10);
        assert_eq!(tools::item_enchantability(item::GOLDEN_SWORD), 22);
        // power 0 offers: j = nextInt(8) + 1 + 0 + nextInt(1), so (j/3).max(1) and so on.
        let mut r = JavaRandom::new(1);
        let mut r2 = JavaRandom::new(1);
        let j = r2.next_int_bounded(8) + 1 + r2.next_int_bounded(1);
        assert_eq!(calc_item_stack_enchantability(&mut r, 0, 0, &sword()), (j / 3).max(1));
        // power above 15 is capped: 15 bookshelves and 40 give the same draws
        let a = calc_item_stack_enchantability(&mut JavaRandom::new(9), 2, 15, &sword());
        let b = calc_item_stack_enchantability(&mut JavaRandom::new(9), 2, 40, &sword());
        assert_eq!(a, b);
        assert!(a >= 30);
        // an item with no enchantability gives 0 without drawing
        assert_eq!(calc_item_stack_enchantability(&mut JavaRandom::new(9), 2, 15, &ItemStack::of(item::DIAMOND_HOE)), 0);
    }

    #[test]
    fn enchantment_datas_pick_the_highest_fitting_level() {
        // Level 30 on a diamond sword: Sharpness 3 fits 23..43 and 4 fits 34..54, so level 3.
        let list = get_enchantment_datas(30, &sword(), false);
        assert!(list.contains(&EnchantmentData { id: SHARPNESS, level: 3 }));
        assert!(list.contains(&EnchantmentData { id: KNOCKBACK, level: 2 }));
        assert!(!list.iter().any(|d| d.id == POWER));
        // treasure never appears on the table
        let book = get_enchantment_datas(30, &ItemStack::of(item::BOOK), false);
        assert!(!book.iter().any(|d| is_treasure(d.id)));
        assert!(get_enchantment_datas(30, &ItemStack::of(item::BOOK), true).iter().any(|d| d.id == MENDING));
    }

    #[test]
    fn built_lists_are_deterministic_and_compatible() {
        for seed in 0..50 {
            let a = build_enchantment_list(&mut JavaRandom::new(seed), &sword(), 30, false);
            let b = build_enchantment_list(&mut JavaRandom::new(seed), &sword(), 30, false);
            assert_eq!(a, b);
            assert!(!a.is_empty());
            for (i, x) in a.iter().enumerate() {
                for y in &a[i + 1..] {
                    assert!(is_compatible_with(x.id, y.id), "{x:?} with {y:?}");
                }
            }
        }
    }

    #[test]
    fn damage_modifiers() {
        let src = DamageFlags::default();
        assert_eq!(calc_modifier_damage(PROTECTION, 4, &src), 4);
        assert_eq!(calc_modifier_damage(FIRE_PROTECTION, 2, &src), 0);
        assert_eq!(calc_modifier_damage(FIRE_PROTECTION, 2, &DamageFlags { fire: true, ..src }), 4);
        assert_eq!(calc_modifier_damage(FEATHER_FALLING, 4, &DamageFlags { fall: true, ..src }), 12);
        assert_eq!(calc_modifier_damage(PROTECTION, 4, &DamageFlags { can_harm_in_creative: true, ..src }), 0);
        // Sharpness V: 1 + 4 * 0.5 = 3 extra on a sword (the 1.25 per level is 1.8's).
        assert_eq!(calc_damage_by_creature(SHARPNESS, 5, CreatureAttribute::Undefined), 3.0);
        assert_eq!(calc_damage_by_creature(SMITE, 5, CreatureAttribute::Undead), 12.5);
        assert_eq!(calc_damage_by_creature(SMITE, 5, CreatureAttribute::Arthropod), 0.0);
        assert_eq!(calc_damage_by_creature(BANE_OF_ARTHROPODS, 3, CreatureAttribute::Arthropod), 7.5);
        assert!((damage_after_magic_absorb(10.0, 20) - 2.0).abs() < 1e-5);
        assert!((damage_after_magic_absorb(10.0, 40) - 2.0).abs() < 1e-5);
    }
}
