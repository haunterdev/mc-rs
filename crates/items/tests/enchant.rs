//! Enchantments on stacks: Silk Touch and Fortune on a harvest, Unbreaking wear, tag comparison.

use items::enchant::{self, id};
use items::harvest::{harvest_block, silk_touch_drop};
use items::ids::{block, item};
use items::registry::item_from_block;
use items::stack::{are_item_stacks_equal, are_items_equal};
use items::{ItemStack, JavaRandom};

fn enchanted(it: u16, e: u8, level: u8) -> ItemStack {
    let mut s = ItemStack::of(it);
    s.add_enchantment(e, level as i32);
    s
}

fn coal_total(pick: &ItemStack, trials: i64) -> i32 {
    let mut total = 0;
    for seed in 0..trials {
        let mut rand = JavaRandom::new(seed);
        let h = harvest_block(block::COAL_ORE << 4, pick, &mut rand);
        total += h.drops.stacks().iter().filter(|s| s.item == item::COAL).map(|s| s.count).sum::<i32>();
    }
    total
}

#[test]
fn silk_touch_drops_the_ore_block() {
    let pick = enchanted(item::DIAMOND_PICKAXE, id::SILK_TOUCH, 1);
    let mut rand = JavaRandom::new(1);
    let h = harvest_block(block::COAL_ORE << 4, &pick, &mut rand);
    let stacks = h.drops.stacks();
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].item, item_from_block(block::COAL_ORE));
    assert_eq!(h.drops.xp, 0, "silk touch skips the experience");
    // without it the ore gives coal
    let plain = harvest_block(block::COAL_ORE << 4, &ItemStack::of(item::DIAMOND_PICKAXE), &mut JavaRandom::new(1));
    assert_eq!(plain.drops.stacks()[0].item, item::COAL);
    // stone drops stone, a lit redstone ore its unlit form, a slab is not silk harvestable
    assert_eq!(silk_touch_drop(block::STONE << 4).item, item_from_block(block::STONE));
    assert_eq!(silk_touch_drop(block::LIT_REDSTONE_ORE << 4).item, item_from_block(block::REDSTONE_ORE));
}

#[test]
fn fortune_three_raises_the_coal_average() {
    let plain = coal_total(&ItemStack::of(item::DIAMOND_PICKAXE), 2000);
    let fortune = coal_total(&enchanted(item::DIAMOND_PICKAXE, id::FORTUNE, 3), 2000);
    // quantityDroppedWithBonus: 1 with chance 1 - 2/(fortune + 2) of a multiplier of 2..=fortune + 1
    assert_eq!(plain, 2000);
    assert!(fortune as f64 > plain as f64 * 1.7, "{fortune} vs {plain}");
}

#[test]
fn unbreaking_three_lowers_the_wear() {
    let wear = |e: Option<u8>| {
        let mut rand = JavaRandom::new(7);
        let mut s = ItemStack::of(item::DIAMOND_PICKAXE);
        if let Some(l) = e {
            s.add_enchantment(id::UNBREAKING, l as i32);
        }
        for _ in 0..1000 {
            s.attempt_damage_item(1, &mut rand);
        }
        s.damage
    };
    assert_eq!(wear(None), 1000);
    let w3 = wear(Some(3));
    // each use lands with chance 1/(level + 1), so about 250
    assert!((150..350).contains(&w3), "{w3}");
    // armor lands 60% of the time first
    let mut rand = JavaRandom::new(7);
    let mut boots = enchanted(item::DIAMOND_BOOTS, id::UNBREAKING, 3);
    for _ in 0..1000 {
        boots.attempt_damage_item(1, &mut rand);
    }
    assert!(boots.damage > w3 && boots.damage < 1000, "{}", boots.damage);
}

#[test]
fn the_enchantment_list_is_part_of_stack_equality() {
    let plain = ItemStack::of(item::DIAMOND_SWORD);
    let sharp = enchanted(item::DIAMOND_SWORD, id::SHARPNESS, 5);
    assert!(are_items_equal(&plain, &sharp));
    assert!(!are_item_stacks_equal(&plain, &sharp));
    assert!(are_item_stacks_equal(&sharp, &sharp.copy()));
    assert!(sharp.has_tag_compound() && sharp.is_item_enchanted() && sharp.has_effect());
    assert!(!plain.has_effect());
    assert!(ItemStack::of(item::ENCHANTED_BOOK).has_effect());
    assert!(ItemStack::new(item::GOLDEN_APPLE, 1, 1).has_effect() && !ItemStack::of(item::GOLDEN_APPLE).has_effect());
    assert_eq!(sharp.get_enchantment_level(id::SHARPNESS), 5);
    assert!(plain.is_item_enchantable() && !sharp.is_item_enchantable());
    assert!(enchant::is_item_enchantable(&ItemStack::of(item::BOOK)));
    assert!(!enchant::is_item_enchantable(&ItemStack::of(item::ENCHANTED_BOOK)));
}
