//! The item-specific tooltip lines and name colours of 1.12.2: `PotionUtils.addPotionTooltip`
//! (potions, splash, lingering, tipped arrows), `ItemRecord.addInformation`, and
//! `Item.getRarity` for the items this port holds. The text comes from the caller's lang table,
//! so the lines are testable without a jar.
//! DROPPED: `CustomPotionEffects` and `CustomPotionColor` (NBT the port does not keep), `ItemStack
//! .getTooltip`'s advanced-tooltip lines (F3+H), and the rarity of the items that override
//! `getRarity` besides records, golden apples and enchanted books.

use crate::creative::{self, PotionDef};
use crate::ids::item;
use crate::potion::duration_string;
use crate::stack::{ItemStack, Tag};

/// `TextFormatting.GRAY`, the colour of every tooltip line after the name.
pub const GRAY: u32 = 0xAAAAAA;
/// `TextFormatting.BLUE`
pub const BLUE: u32 = 0x5555FF;
/// `TextFormatting.RED`
pub const RED: u32 = 0xFF5555;
/// `TextFormatting.DARK_PURPLE`
pub const DARK_PURPLE: u32 = 0xAA00AA;

/// `EnumRarity`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

impl Rarity {
    /// `EnumRarity.rarityColor`: white, yellow, aqua, light purple.
    pub fn color(self) -> u32 {
        match self {
            Rarity::Common => 0xFFFFFF,
            Rarity::Uncommon => 0xFFFF55,
            Rarity::Rare => 0x55FFFF,
            Rarity::Epic => 0xFF55FF,
        }
    }
}

/// `ItemStack.getRarity()`: `Item.getRarity` (COMMON, RARE when enchanted), `ItemRecord` RARE,
/// `ItemAppleGold` RARE or EPIC by meta, `ItemEnchantedBook` UNCOMMON once it holds an
/// enchantment.
pub fn rarity(stack: &ItemStack) -> Rarity {
    match stack.get_item() {
        i if record_name(i).is_some() => Rarity::Rare,
        item::GOLDEN_APPLE => {
            if stack.get_metadata() == 0 {
                Rarity::Rare
            } else {
                Rarity::Epic
            }
        }
        item::ENCHANTED_BOOK => {
            if stack.ench.is_empty() {
                Rarity::Common
            } else {
                Rarity::Uncommon
            }
        }
        _ => {
            if stack.ench.is_empty() {
                Rarity::Common
            } else {
                Rarity::Rare
            }
        }
    }
}

/// The record name of a music disc item (`ItemRecord.displayName` is `item.record.<name>.desc`,
/// its sound `record.<name>`), or `None` for any other item.
pub fn record_name(it: u16) -> Option<&'static str> {
    Some(match it {
        item::RECORD_13 => "13",
        item::RECORD_CAT => "cat",
        item::RECORD_BLOCKS => "blocks",
        item::RECORD_CHIRP => "chirp",
        item::RECORD_FAR => "far",
        item::RECORD_MALL => "mall",
        item::RECORD_MELLOHI => "mellohi",
        item::RECORD_STAL => "stal",
        item::RECORD_STRAD => "strad",
        item::RECORD_WARD => "ward",
        item::RECORD_11 => "11",
        item::RECORD_WAIT => "wait",
        _ => return None,
    })
}

/// The record item whose sound event is `record.<name>`, `ItemRecord.getBySound`.
pub fn record_by_name(name: &str) -> Option<u16> {
    (item::RECORD_13..=item::RECORD_WAIT).find(|&i| record_name(i) == Some(name))
}

/// One attribute modifier a potion effect registers (`registerPotionAttributeModifier`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modifier {
    /// The attribute's `getName()`, the suffix of the lang key `attribute.name.<attr>`.
    pub attr: &'static str,
    /// The per-level amount (`PotionAttackDamage` replaces it with its `bonusPerLevel`).
    pub amount: f64,
    /// `AttributeModifier.operation`: 0 add, 1 multiply base, 2 multiply total.
    pub op: u8,
}

/// The modifiers `Potion.getAttributeModifierMap` holds for an effect's lang key
/// (`Potion.setPotionName`). Strength and weakness carry `PotionAttackDamage.bonusPerLevel`
/// (3.0, -4.0) as their amount.
pub fn attribute_modifiers(key: &str) -> &'static [Modifier] {
    const SPEED: &[Modifier] = &[Modifier { attr: "generic.movementSpeed", amount: 0.200_000_002_98, op: 2 }];
    const SLOW: &[Modifier] = &[Modifier { attr: "generic.movementSpeed", amount: -0.150_000_005_96, op: 2 }];
    const HASTE: &[Modifier] = &[Modifier { attr: "generic.attackSpeed", amount: 0.100_000_001_49, op: 2 }];
    const FATIGUE: &[Modifier] = &[Modifier { attr: "generic.attackSpeed", amount: -0.100_000_001_49, op: 2 }];
    const STRENGTH: &[Modifier] = &[Modifier { attr: "generic.attackDamage", amount: 3.0, op: 0 }];
    const WEAKNESS: &[Modifier] = &[Modifier { attr: "generic.attackDamage", amount: -4.0, op: 0 }];
    const HEALTH: &[Modifier] = &[Modifier { attr: "generic.maxHealth", amount: 4.0, op: 0 }];
    const LUCK: &[Modifier] = &[Modifier { attr: "generic.luck", amount: 1.0, op: 0 }];
    const UNLUCK: &[Modifier] = &[Modifier { attr: "generic.luck", amount: -1.0, op: 0 }];
    match key {
        "effect.moveSpeed" => SPEED,
        "effect.moveSlowdown" => SLOW,
        "effect.digSpeed" => HASTE,
        "effect.digSlowDown" => FATIGUE,
        "effect.damageBoost" => STRENGTH,
        "effect.weakness" => WEAKNESS,
        "effect.healthBoost" => HEALTH,
        "effect.luck" => LUCK,
        "effect.unluck" => UNLUCK,
        _ => &[],
    }
}

/// `ItemStack.DECIMALFORMAT`, `#.##`: at most two decimals, none when whole.
pub fn decimal(d: f64) -> String {
    let s = format!("{:.2}", (d * 100.0).round_ties_even() / 100.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// `PotionUtils.getPotionFromItem(stack)`: the stack's type, `empty` when untagged.
pub fn potion_of(stack: &ItemStack) -> &'static PotionDef {
    match stack.tag {
        Tag::Potion(p) => creative::potion(p),
        _ => creative::potion(0),
    }
}

/// `PotionUtils.getColor(stack)` as RGB: 16253176 for an `empty` type, else the colour of the
/// type's effects (`getPotionColorFromEffectList`, 3694022 for none).
pub fn potion_color(stack: &ItemStack) -> u32 {
    match stack.tag {
        Tag::Potion(p) if p != 0 => creative::potion(p).color,
        _ => 16_253_176,
    }
}

/// `ItemPotion`, `ItemSplashPotion`, `ItemLingeringPotion` or `ItemTippedArrow`.
pub fn is_potion_item(it: u16) -> bool {
    matches!(it, item::POTIONITEM | item::SPLASH_POTION | item::LINGERING_POTION | item::TIPPED_ARROW)
}

/// `PotionUtils.addPotionTooltip(stack, lores, durationFactor)`: `(text, colour)` per line,
/// the empty string for the blank line. `tr` is `I18n.translateToLocal`, `fmt` is
/// `translateToLocalFormatted` with its arguments already text.
pub fn add_potion_tooltip(stack: &ItemStack, duration_factor: f32, tr: &dyn Fn(&str) -> String, fmt: &dyn Fn(&str, &[String]) -> String) -> Vec<(String, u32)> {
    let effects = potion_of(stack).effects;
    let mut lines = Vec::new();
    let mut mods: Vec<(&'static str, f64, u8)> = Vec::new();
    if effects.is_empty() {
        lines.push((tr("effect.none").trim().to_string(), GRAY));
    } else {
        for e in effects {
            let mut s = tr(e.key).trim().to_string();
            for m in attribute_modifiers(e.key) {
                // Potion.getAttributeModifierAmount: amount * (amplifier + 1).
                mods.push((m.attr, m.amount * (e.amplifier + 1) as f64, m.op));
            }
            if e.amplifier > 0 {
                s += " ";
                s += tr(&format!("potion.potency.{}", e.amplifier)).trim();
            }
            if e.duration > 20 {
                s += &format!(" ({})", duration_string((e.duration as f32 * duration_factor).floor() as i32));
            }
            lines.push((s, if e.bad { RED } else { BLUE }));
        }
    }
    if !mods.is_empty() {
        lines.push((String::new(), GRAY));
        lines.push((tr("potion.whenDrank"), DARK_PURPLE));
        for (attr, amount, op) in mods {
            let shown = if op == 1 || op == 2 { amount * 100.0 } else { amount };
            let name = tr(&format!("attribute.name.{attr}"));
            if amount > 0.0 {
                lines.push((fmt(&format!("attribute.modifier.plus.{op}"), &[decimal(shown), name]), BLUE));
            } else if amount < 0.0 {
                lines.push((fmt(&format!("attribute.modifier.take.{op}"), &[decimal(-shown), name]), RED));
            }
        }
    }
    lines
}

/// `Item.addInformation` for the items that have any here: the potion lines (factor 1 for a
/// potion and a splash potion, 0.25 lingering, 0.125 tipped arrow) and a record's
/// `item.record.<name>.desc`.
pub fn item_information(stack: &ItemStack, tr: &dyn Fn(&str) -> String, fmt: &dyn Fn(&str, &[String]) -> String) -> Vec<(String, u32)> {
    let it = stack.get_item();
    match it {
        item::POTIONITEM | item::SPLASH_POTION => add_potion_tooltip(stack, 1.0, tr, fmt),
        item::LINGERING_POTION => add_potion_tooltip(stack, 0.25, tr, fmt),
        item::TIPPED_ARROW => add_potion_tooltip(stack, 0.125, tr, fmt),
        _ => match record_name(it) {
            Some(n) => vec![(tr(&format!("item.record.{n}.desc")), GRAY)],
            None => Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creative::potion_id;

    fn tr(k: &str) -> String {
        match k {
            "effect.none" => "No Effects".into(),
            "effect.moveSpeed" => "Speed".into(),
            "effect.weakness" => "Weakness".into(),
            "potion.potency.1" => "II".into(),
            "potion.whenDrank" => "When Applied:".into(),
            "attribute.name.generic.movementSpeed" => "Speed".into(),
            "attribute.name.generic.attackDamage" => "Attack Damage".into(),
            "item.record.cat.desc" => "C418 - cat".into(),
            _ => k.into(),
        }
    }
    fn fmt(k: &str, a: &[String]) -> String {
        match k {
            "attribute.modifier.plus.2" => format!("+{}% {}", a[0], a[1]),
            "attribute.modifier.take.0" => format!("-{} {}", a[0], a[1]),
            _ => k.into(),
        }
    }
    fn of(it: u16, name: &str) -> ItemStack {
        ItemStack::of(it).with_tag(Tag::Potion(potion_id(name)))
    }

    #[test]
    fn a_potion_without_effects_says_so() {
        let l = item_information(&of(item::POTIONITEM, "minecraft:water"), &tr, &fmt);
        assert_eq!(l, vec![("No Effects".to_string(), GRAY)]);
        assert_eq!(item_information(&ItemStack::of(item::POTIONITEM), &tr, &fmt), l);
    }

    #[test]
    fn strong_swiftness_lists_the_effect_and_its_modifier() {
        let l = item_information(&of(item::POTIONITEM, "minecraft:strong_swiftness"), &tr, &fmt);
        assert_eq!(l[0], ("Speed II (1:30)".to_string(), BLUE));
        assert_eq!(l[1], (String::new(), GRAY));
        assert_eq!(l[2], ("When Applied:".to_string(), DARK_PURPLE));
        // 0.2 * 2 levels = 40%.
        assert_eq!(l[3], ("+40% Speed".to_string(), BLUE));
        assert_eq!(l.len(), 4);
    }

    #[test]
    fn weakness_is_red_with_a_take_line() {
        let l = item_information(&of(item::POTIONITEM, "minecraft:weakness"), &tr, &fmt);
        assert_eq!(l[0], ("Weakness (1:30)".to_string(), RED));
        assert_eq!(l[3], ("-4 Attack Damage".to_string(), RED));
    }

    #[test]
    fn the_duration_factor_scales_lingering_and_arrows() {
        let w = |it| item_information(&of(it, "minecraft:swiftness"), &tr, &fmt)[0].0.clone();
        assert_eq!(w(item::POTIONITEM), "Speed (3:00)");
        assert_eq!(w(item::SPLASH_POTION), "Speed (3:00)");
        assert_eq!(w(item::LINGERING_POTION), "Speed (0:45)");
        assert_eq!(w(item::TIPPED_ARROW), "Speed (0:22)");
    }

    #[test]
    fn an_instant_potion_shows_no_duration() {
        let l = item_information(&of(item::POTIONITEM, "minecraft:healing"), &tr, &fmt);
        assert_eq!(l[0], ("effect.heal".to_string(), BLUE));
    }

    #[test]
    fn the_potion_colour_follows_the_type() {
        assert_eq!(potion_color(&ItemStack::of(item::POTIONITEM)), 16_253_176);
        assert_eq!(potion_color(&of(item::POTIONITEM, "minecraft:water")), 3_694_022);
        assert_eq!(potion_color(&of(item::SPLASH_POTION, "minecraft:healing")), 0xf82423);
    }

    #[test]
    fn records_name_themselves_and_are_rare() {
        let cat = ItemStack::of(item::RECORD_CAT);
        assert_eq!(item_information(&cat, &tr, &fmt), vec![("C418 - cat".to_string(), GRAY)]);
        assert_eq!(rarity(&cat), Rarity::Rare);
        assert_eq!(rarity(&cat).color(), 0x55FFFF);
        assert_eq!(cat.get_max_stack_size(), 1);
        assert_eq!(record_by_name("wait"), Some(item::RECORD_WAIT));
        assert_eq!(record_by_name("nope"), None);
        assert_eq!(rarity(&ItemStack::of(item::STICK)), Rarity::Common);
        assert_eq!(rarity(&ItemStack::new(item::GOLDEN_APPLE, 1, 1)), Rarity::Epic);
    }

    #[test]
    fn decimals_follow_the_java_format() {
        assert_eq!(decimal(40.0), "40");
        assert_eq!(decimal(10.000_000_149), "10");
        assert_eq!(decimal(0.5), "0.5");
        assert_eq!(decimal(-0.001), "0");
    }
}
