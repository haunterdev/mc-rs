//! Worn armor on the server: the armor and toughness of the four pieces
//! (`ItemArmor.getItemAttributeModifiers` summed by `EntityLivingBase.getTotalArmorValue` and the
//! toughness attribute), the wear `InventoryPlayer.damageArmor` puts on them, and a right click
//! with a piece in hand (`ItemArmor.onItemRightClick`), which `V_USE_ITEM` carries (use_item.rs).
//!
//! The pieces live in container slots 5 to 8 (`InventoryPlayer.armor`, feet first). Other players
//! see them through `S_EQUIPMENT` (windows.rs `send_equipment` diffs the six slots every tick and
//! `on_join` sends them to a joiner), and a shift click or a click into an armor slot is the
//! container code's (`sim::container::player`).
//!
//! DROPPED: enchantment protection (`EnchantmentHelper.getEnchantmentModifierDamage`), the
//! Unbreaking roll in `damageItem`, the item break particles, the armor attribute modifiers of a
//! mod or a potion, and the dispenser equip.

use ::items::{ItemStack, Kind, JavaRandom};

use super::{players, Game};

/// `EntityLivingBase.getTotalArmorValue`: the armor points of the worn pieces.
pub fn total_armor_value(armor: &[ItemStack; 4]) -> i32 {
    armor
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| match s.def().kind {
            Kind::Armor(m, slot) => m.damage_reduction_amount(slot),
            _ => 0,
        })
        .sum()
}

/// The `ARMOR_TOUGHNESS` attribute value of the worn pieces.
pub fn toughness(armor: &[ItemStack; 4]) -> f32 {
    armor
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| match s.def().kind {
            Kind::Armor(m, _) => m.toughness(),
            _ => 0.0,
        })
        .sum()
}

/// `InventoryPlayer.damageArmor(damage)`: a quarter of the hit, at least 1, truncated, on every
/// piece. A piece that wears out is gone from its slot.
pub fn damage_armor(armor: &mut [ItemStack; 4], damage: f32, creative: bool, rand: &mut JavaRandom) {
    let mut damage = damage / 4.0;
    if damage < 1.0 {
        damage = 1.0;
    }
    for s in armor.iter_mut() {
        if !s.is_empty() && matches!(s.def().kind, Kind::Armor(..)) {
            s.damage_item(damage as i32, creative, rand);
        }
    }
}

/// `EntityLivingBase.applyArmorCalculations` for the player `id`: the armor wears and the damage
/// falls to what `CombatRules.getDamageAfterAbsorb` leaves.
pub fn apply_armor_calculations(game: &mut Game, id: u32, damage: f32) -> f32 {
    let creative = players::mode(game, id) == proto::GM_CREATIVE;
    let Some(p) = game.players.players.get_mut(&id) else { return damage };
    damage_armor(&mut p.inv.armor, damage, creative, &mut game.players.rand);
    let (armor, toughness) = (total_armor_value(&p.inv.armor) as f32, toughness(&p.inv.armor));
    super::damage::damage_after_absorb(damage, armor, toughness)
}

/// `ItemArmor.onItemRightClick` through `PlayerInteractionManager.processRightClick`: the piece
/// in `hand` goes to its empty slot. True when it was worn.
/// DROPPED: the cooldown check, and the `ActionResult` (the container diff sends the slots).
pub fn right_click(game: &mut Game, id: u32, hand: u8) -> bool {
    if players::mode(game, id) == proto::GM_SPECTATOR {
        return false;
    }
    let Some(p) = game.players.players.get_mut(&id) else { return false };
    if p.dead {
        return false;
    }
    let cur = p.inv.current_item;
    let held = if hand == 0 { p.inv.main[cur] } else { p.inv.off_hand[0] };
    let Kind::Armor(_, slot) = (if held.is_empty() { Kind::Plain } else { held.def().kind }) else { return false };
    // EntityLiving.getSlotForItemStack: a piece goes to its own slot.
    let i = slot.index();
    if !p.inv.armor[i].is_empty() {
        return false;
    }
    p.inv.armor[i] = held;
    if hand == 0 {
        p.inv.main[cur] = ItemStack::EMPTY;
    } else {
        p.inv.off_hand[0] = ItemStack::EMPTY;
    }
    true
}
