//! `ContainerEnchantment`: the bookshelf count, the offers, `enchantItem` and the shift-click.

use items::ids::{block, item};

use super::enchant::{count_bookshelves, Enchanted};
use super::tests::{click_as, st};
use super::*;
use crate::inventory::InventoryPlayer;

fn table(power: i32, seed: i32) -> Container {
    Container::enchanting(3, [0, 64, 0], seed, Some(power))
}

fn put_item(c: &mut Container, inv: &mut InventoryPlayer, i: usize, s: ItemStack) {
    c.put_stack(&mut PlayerRef::new(inv, false), i, s);
}

#[test]
fn the_window_has_the_vanilla_layout() {
    let c = table(0, 1);
    assert_eq!(c.slots.len(), 38);
    assert_eq!((c.slots[0].x, c.slots[0].y, c.slots[0].inventory), (15, 47, Inv::Enchant));
    assert_eq!((c.slots[1].x, c.slots[1].y), (35, 47));
    assert_eq!((c.slots[2].x, c.slots[2].y, c.slots[2].inventory, c.slots[2].index), (8, 84, Inv::Player, 9));
    assert_eq!((c.slots[29].x, c.slots[29].y, c.slots[29].index), (8, 142, 0));
    assert_eq!(c.get_slot_stack_limit(0), 1);
    assert!(c.is_item_valid(0, &st(item::DIAMOND_SWORD, 1)));
    assert!(c.is_item_valid(1, &ItemStack::new(item::DYE, 1, 4)));
    assert!(!c.is_item_valid(1, &ItemStack::new(item::DYE, 1, 1)));
    assert!(!c.is_item_valid(1, &st(item::DIAMOND, 1)));
}

#[test]
fn fifteen_bookshelves_are_counted_in_the_air_ringed_cells() {
    // Fifteen shelves anywhere on the 16 two-block-out cells over two layers that vanilla counts.
    let shelves: Vec<(i32, i32, i32)> = {
        let mut v = Vec::new();
        for y in 0..2 {
            for (x, z) in [(-2, -2), (-2, -1), (-2, 0), (-2, 1), (-2, 2), (2, -2), (2, -1), (2, 0), (2, 1), (2, 2), (-1, -2), (0, -2), (1, -2), (-1, 2), (0, 2), (1, 2)] {
                v.push((x, y, z));
            }
        }
        v
    };
    // The cells vanilla reaches: only those the loop names, so count with all 32 placed.
    let all = count_bookshelves(|_, _, _| false, |x, y, z| shelves.contains(&(x, y, z)));
    assert_eq!(all, 0, "no air between table and ring, nothing counts");
    let all = count_bookshelves(|x, _, z| x.abs() <= 1 && z.abs() <= 1, |x, y, z| shelves.contains(&(x, y, z)));
    assert_eq!(all, 32, "every ring cell on both layers");
    // No air at (1, 0, 1): the iteration for that corner reads its 2 + 4 cells no more.
    let n2 = count_bookshelves(|x, y, z| x.abs() <= 1 && z.abs() <= 1 && !(x == 1 && z == 1 && y == 0), |x, y, z| shelves.contains(&(x, y, z)));
    assert_eq!(n2, 32 - 6);
}

fn fifteen() -> i32 {
    // 15 shelves: the bottom layer's 16 ring cells less one, as many as the loop reaches.
    let cells = [(-2, -2), (-2, -1), (-2, 0), (-2, 1), (-2, 2), (2, -2), (2, -1), (2, 0), (2, 1), (2, 2), (-1, -2), (0, -2), (1, -2), (-1, 2), (0, 2), (1, 2)];
    let set: Vec<(i32, i32, i32)> = cells.iter().skip(1).map(|&(x, z)| (x, 0, z)).collect();
    count_bookshelves(|x, _, z| x.abs() <= 1 && z.abs() <= 1, |x, y, z| set.contains(&(x, y, z)))
}

#[test]
fn fifteen_bookshelves_give_a_level_thirty_bottom_offer() {
    assert_eq!(fifteen(), 15);
    let mut c = table(15, 12345);
    let mut inv = InventoryPlayer::new();
    put_item(&mut c, &mut inv, 0, st(item::DIAMOND_SWORD, 1));
    let t = c.enchant_ref();
    assert_eq!(t.levels[2], 30);
    assert!(t.levels[0] >= 1 && t.levels[0] <= 10 && t.levels[1] > t.levels[0] && t.levels[1] < 30, "{:?}", t.levels);
    assert!(t.clue.iter().all(|&e| e >= 0) && t.world_clue.iter().all(|&l| l >= 1));
    // No bookshelves: at most level 8 (rand 1..8 + 0, then thirds).
    let mut c0 = table(0, 12345);
    put_item(&mut c0, &mut inv, 0, st(item::DIAMOND_SWORD, 1));
    assert!(c0.enchant_ref().levels.iter().all(|&l| l <= 8), "{:?}", c0.enchant_ref().levels);
}

#[test]
fn offers_are_the_same_for_the_same_seed_and_change_with_it() {
    let offers = |seed: i32| {
        let mut c = table(15, seed);
        let mut inv = InventoryPlayer::new();
        put_item(&mut c, &mut inv, 0, st(item::IRON_PICKAXE, 1));
        let t = c.enchant_ref();
        (t.levels, t.clue, t.world_clue)
    };
    assert_eq!(offers(77), offers(77));
    assert!((0..20).any(|s| offers(s) != offers(77)));
    // Taking the item away zeroes them.
    let mut c = table(15, 77);
    let mut inv = InventoryPlayer::new();
    put_item(&mut c, &mut inv, 0, st(item::IRON_PICKAXE, 1));
    put_item(&mut c, &mut inv, 0, ItemStack::EMPTY);
    assert_eq!((c.enchant_ref().levels, c.enchant_ref().clue), ([0; 3], [-1; 3]));
    // A viewer copy computes nothing on its own.
    let mut v = Container::enchanting(3, [0, 64, 0], 77, None);
    put_item(&mut v, &mut inv, 0, st(item::IRON_PICKAXE, 1));
    assert_eq!(v.enchant_ref().levels, [0; 3]);
}

#[test]
fn enchanting_costs_levels_and_lapis_and_enchants_the_item() {
    let mut c = table(15, 4242);
    let mut inv = InventoryPlayer::new();
    put_item(&mut c, &mut inv, 0, st(item::DIAMOND_SWORD, 1));
    put_item(&mut c, &mut inv, 1, ItemStack::new(item::DYE, 5, 4));
    let offer = c.enchant_ref().levels[2];
    assert_eq!(offer, 30);
    // Too few levels, then too little lapis for the third offer.
    assert_eq!(c.enchant_item(false, 29, 2, || 9), Enchanted::Refused);
    let mut few = table(15, 4242);
    put_item(&mut few, &mut inv, 0, st(item::DIAMOND_SWORD, 1));
    put_item(&mut few, &mut inv, 1, ItemStack::new(item::DYE, 2, 4));
    assert_eq!(few.enchant_item(false, 30, 2, || 9), Enchanted::Refused);
    // Enough: 3 lapis and the 3 levels of the button are paid, the offer needs 30 levels held.
    let done = c.enchant_item(false, 30, 2, || 555);
    assert_eq!(done, Enchanted::Done { cost: 3, new_seed: 555 });
    let t = c.enchant_ref();
    assert!(t.items[0].is_item_enchanted());
    assert_eq!(t.items[1].get_count(), 2);
    assert_eq!(t.xp_seed, 555);
    // Creative pays nothing.
    let mut cr = table(15, 4242);
    put_item(&mut cr, &mut inv, 0, st(item::DIAMOND_SWORD, 1));
    assert_eq!(cr.enchant_item(true, 0, 0, || 1), Enchanted::Done { cost: 1, new_seed: 1 });
    assert!(cr.enchant_ref().items[1].is_empty());
}

#[test]
fn a_book_becomes_an_enchanted_book() {
    let mut c = table(15, 99);
    let mut inv = InventoryPlayer::new();
    put_item(&mut c, &mut inv, 0, st(item::BOOK, 1));
    put_item(&mut c, &mut inv, 1, ItemStack::new(item::DYE, 3, 4));
    assert!(c.enchant_ref().levels[2] > 0);
    assert!(matches!(c.enchant_item(false, 30, 2, || 7), Enchanted::Done { .. }));
    let b = c.enchant_ref().items[0];
    assert_eq!(b.get_item(), item::ENCHANTED_BOOK);
    assert!(!b.ench.is_empty());
}

#[test]
fn shift_click_moves_the_item_one_at_a_time_and_lapis_to_its_slot() {
    let mut c = table(15, 1);
    let mut inv = InventoryPlayer::new();
    inv.set_inventory_slot_contents(0, st(item::DIAMOND_SWORD, 1));
    inv.set_inventory_slot_contents(1, ItemStack::new(item::DYE, 6, 4));
    inv.set_inventory_slot_contents(2, st(item::IRON_INGOT, 5));
    // hotbar slot 0 is container slot 29
    click_as(&mut c, &mut inv, false, 29, 0, ClickType::QuickMove);
    assert_eq!(c.enchant_ref().items[0].get_item(), item::DIAMOND_SWORD);
    click_as(&mut c, &mut inv, false, 30, 0, ClickType::QuickMove);
    assert_eq!(c.enchant_ref().items[1].get_count(), 6);
    // Occupied: an ingot does not replace the sword.
    click_as(&mut c, &mut inv, false, 31, 0, ClickType::QuickMove);
    assert_eq!(c.enchant_ref().items[0].get_item(), item::DIAMOND_SWORD);
    // Shift-click the sword out again.
    click_as(&mut c, &mut inv, false, 0, 0, ClickType::QuickMove);
    assert!(c.enchant_ref().items[0].is_empty());
}

#[test]
fn closing_gives_the_table_slots_back() {
    let mut c = table(15, 1);
    let mut inv = InventoryPlayer::new();
    put_item(&mut c, &mut inv, 0, st(item::DIAMOND_SWORD, 1));
    put_item(&mut c, &mut inv, 1, ItemStack::new(item::DYE, 6, 4));
    let mut p = PlayerRef::new(&mut inv, false);
    c.on_container_closed(&mut p);
    assert!(p.dropped.is_empty());
    assert_eq!(inv.get_stack_in_slot(0).get_item(), item::DIAMOND_SWORD);
    assert!(c.enchant_ref().items.iter().all(|s| s.is_empty()));
    let _ = block::BOOKSHELF;
}

#[test]
fn the_ten_properties_go_out_once_and_again_when_they_change() {
    let mut c = table(15, 4242);
    let mut inv = InventoryPlayer::new();
    let p = PlayerRef::new(&mut inv, false);
    c.detect_and_send_changes(&p);
    assert_eq!(c.prop_listener.len(), 10);
    c.prop_listener.clear();
    c.detect_and_send_changes(&p);
    assert!(c.prop_listener.is_empty());
    drop(p);
    put_item(&mut c, &mut inv, 0, st(item::DIAMOND_SWORD, 1));
    c.detect_and_send_changes(&PlayerRef::new(&mut inv, false));
    assert!(c.prop_listener.iter().any(|(n, v)| *n == 2 && *v == 30));
    // The viewer copy takes them back with update_enchant_property.
    let mut v = Container::enchanting(3, [0, 64, 0], 0, None);
    for (n, val) in c.prop_listener.clone() {
        v.update_enchant_property(n, val);
    }
    assert_eq!(v.enchant_ref().levels, c.enchant_ref().levels);
    assert_eq!(v.enchant_ref().clue, c.enchant_ref().clue);
}
