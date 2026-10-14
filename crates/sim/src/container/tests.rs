//! Click tests on `ContainerPlayer`: pickup, place, swap, merge, shift-click, throw, clone,
//! the number keys, drags and the double click. Crafting is in `tests_craft.rs`.

use std::sync::Arc;

use items::ids::{block, item};
use items::{ItemStack, Recipes};

use super::*;
use crate::inventory::InventoryPlayer;

/// Small 1.12.2 recipe files, in the jar's JSON format.
pub(super) const RECIPES: &[(&str, &str)] = &[
    ("oak_planks", r#"{"type":"crafting_shapeless","group":"planks","ingredients":[{"item":"minecraft:log","data":0}],"result":{"item":"minecraft:planks","data":0,"count":4}}"#),
    ("stick", r##"{"type":"crafting_shaped","pattern":["#","#"],"key":{"#":{"item":"minecraft:planks","data":32767}},"result":{"item":"minecraft:stick","count":4}}"##),
    ("crafting_table", r###"{"type":"crafting_shaped","pattern":["##","##"],"key":{"#":{"item":"minecraft:planks","data":32767}},"result":{"item":"minecraft:crafting_table"}}"###),
    ("wooden_pickaxe", r##"{"type":"crafting_shaped","pattern":["XXX"," # "," # "],"key":{"#":{"item":"minecraft:stick"},"X":{"item":"minecraft:planks","data":32767}},"result":{"item":"minecraft:wooden_pickaxe"}}"##),
    ("wooden_axe", r##"{"type":"crafting_shaped","pattern":["XX","X#"," #"],"key":{"#":{"item":"minecraft:stick"},"X":{"item":"minecraft:planks","data":32767}},"result":{"item":"minecraft:wooden_axe"}}"##),
    ("flint_and_steel", r#"{"type":"crafting_shapeless","ingredients":[{"item":"minecraft:iron_ingot"},{"item":"minecraft:flint"}],"result":{"item":"minecraft:flint_and_steel"}}"#),
    ("cake", r#"{"type":"crafting_shaped","pattern":["AAA","BEB","CCC"],"key":{"A":{"item":"minecraft:milk_bucket"},"B":{"item":"minecraft:sugar"},"C":{"item":"minecraft:wheat"},"E":{"item":"minecraft:egg"}},"result":{"item":"minecraft:cake"}}"#),
];

pub(super) fn recipes() -> Arc<Recipes> {
    Arc::new(Recipes::from_files(RECIPES.iter().map(|(n, j)| (*n, j.as_bytes()))).expect("test recipes parse"))
}

pub(super) fn st(id: u16, n: i32) -> ItemStack {
    ItemStack::new(id, n, 0)
}

/// `slotClick` as a player in survival or creative: the returned stack and what was dropped.
pub(super) fn click_as(c: &mut Container, inv: &mut InventoryPlayer, creative: bool, slot: i32, button: i32, ct: ClickType) -> (ItemStack, Vec<ItemStack>) {
    let mut p = PlayerRef::new(inv, creative);
    let r = c.slot_click(slot, button, ct, &mut p);
    (r, p.dropped)
}

pub(super) fn click(c: &mut Container, inv: &mut InventoryPlayer, slot: i32, button: i32, ct: ClickType) -> (ItemStack, Vec<ItemStack>) {
    click_as(c, inv, false, slot, button, ct)
}

/// `Slot.putStack` on container slot `i`.
pub(super) fn set(c: &mut Container, inv: &mut InventoryPlayer, i: usize, s: ItemStack) {
    c.put_stack(&mut PlayerRef::new(inv, false), i, s);
}

/// `Slot.getStack` of container slot `i`.
pub(super) fn get(c: &Container, inv: &mut InventoryPlayer, i: usize) -> ItemStack {
    c.get_stack(&PlayerRef::new(inv, false), i)
}

/// A whole drag: start, each slot, end, with `mode` 0 (left), 1 (right) or 2 (middle).
pub(super) fn drag(c: &mut Container, inv: &mut InventoryPlayer, creative: bool, mode: i32, slots: &[i32]) {
    click_as(c, inv, creative, -999, get_quickcraft_mask(0, mode), ClickType::QuickCraft);
    for &s in slots {
        click_as(c, inv, creative, s, get_quickcraft_mask(1, mode), ClickType::QuickCraft);
    }
    click_as(c, inv, creative, -999, get_quickcraft_mask(2, mode), ClickType::QuickCraft);
}

fn setup() -> (Container, InventoryPlayer) {
    (Container::player(recipes()), InventoryPlayer::new())
}

#[test]
fn player_layout_maps_to_inventory_indices() {
    let (mut c, mut inv) = setup();
    assert_eq!(c.slots.len(), 46);
    set(&mut c, &mut inv, 5, st(item::IRON_HELMET, 1));
    set(&mut c, &mut inv, 8, st(item::IRON_BOOTS, 1));
    set(&mut c, &mut inv, 9, st(block::STONE, 1));
    set(&mut c, &mut inv, 36, st(block::DIRT, 2));
    set(&mut c, &mut inv, 44, st(block::DIRT, 3));
    set(&mut c, &mut inv, 45, st(item::SHIELD, 1));
    assert_eq!(inv.armor[3], st(item::IRON_HELMET, 1));
    assert_eq!(inv.armor[0], st(item::IRON_BOOTS, 1));
    assert_eq!(inv.main[9], st(block::STONE, 1));
    assert_eq!(inv.main[0], st(block::DIRT, 2));
    assert_eq!(inv.main[8], st(block::DIRT, 3));
    assert_eq!(inv.off_hand[0], st(item::SHIELD, 1));
}

#[test]
fn left_pickup_takes_the_whole_stack() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 10));
    let (r, dropped) = click(&mut c, &mut inv, 9, 0, ClickType::Pickup);
    assert_eq!(r, st(block::STONE, 10));
    assert_eq!(inv.cursor, st(block::STONE, 10));
    assert!(get(&c, &mut inv, 9).is_empty());
    assert!(dropped.is_empty());
}

#[test]
fn right_pickup_takes_half_rounded_up() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 11));
    click(&mut c, &mut inv, 9, 1, ClickType::Pickup);
    assert_eq!(inv.cursor, st(block::STONE, 6));
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 5));
}

#[test]
fn right_places_one_and_left_places_all() {
    let (mut c, mut inv) = setup();
    inv.cursor = st(block::STONE, 10);
    click(&mut c, &mut inv, 9, 1, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 1));
    assert_eq!(inv.cursor, st(block::STONE, 9));
    click(&mut c, &mut inv, 10, 0, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 10), st(block::STONE, 9));
    assert!(inv.cursor.is_empty());
}

#[test]
fn different_items_swap_with_the_cursor() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::DIRT, 5));
    inv.cursor = st(block::STONE, 10);
    click(&mut c, &mut inv, 9, 0, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 10));
    assert_eq!(inv.cursor, st(block::DIRT, 5));
}

#[test]
fn merging_stops_at_the_max_stack() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 30));
    inv.cursor = st(block::STONE, 40);
    click(&mut c, &mut inv, 9, 0, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 64));
    assert_eq!(inv.cursor, st(block::STONE, 6));
    // A full slot takes nothing more, left or right.
    click(&mut c, &mut inv, 9, 1, ClickType::Pickup);
    assert_eq!(inv.cursor, st(block::STONE, 6));
    // Ender pearls stack to 16.
    set(&mut c, &mut inv, 10, st(item::ENDER_PEARL, 10));
    inv.cursor = st(item::ENDER_PEARL, 10);
    click(&mut c, &mut inv, 10, 0, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 10), st(item::ENDER_PEARL, 16));
    assert_eq!(inv.cursor, st(item::ENDER_PEARL, 4));
    // Right click adds one.
    set(&mut c, &mut inv, 11, st(block::STONE, 3));
    inv.cursor = st(block::STONE, 5);
    click(&mut c, &mut inv, 11, 1, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 11), st(block::STONE, 4));
}

#[test]
fn armor_slots_take_only_their_piece_one_at_a_time() {
    let (mut c, mut inv) = setup();
    inv.cursor = st(item::IRON_HELMET, 1);
    click(&mut c, &mut inv, 6, 0, ClickType::Pickup);
    assert!(get(&c, &mut inv, 6).is_empty());
    assert_eq!(inv.cursor, st(item::IRON_HELMET, 1));
    click(&mut c, &mut inv, 5, 0, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 5), st(item::IRON_HELMET, 1));
    assert!(inv.cursor.is_empty());
    inv.cursor = st(block::STONE, 2);
    click(&mut c, &mut inv, 8, 0, ClickType::Pickup);
    assert!(get(&c, &mut inv, 8).is_empty());
    // A pumpkin goes on the head, one of the stack.
    let (mut c, mut inv) = setup();
    inv.cursor = st(block::PUMPKIN, 5);
    click(&mut c, &mut inv, 5, 0, ClickType::Pickup);
    assert_eq!(get(&c, &mut inv, 5), st(block::PUMPKIN, 1));
    assert_eq!(inv.cursor, st(block::PUMPKIN, 4));
}

#[test]
fn shift_click_moves_between_main_and_hotbar() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 10));
    click(&mut c, &mut inv, 9, 0, ClickType::QuickMove);
    assert!(get(&c, &mut inv, 9).is_empty());
    assert_eq!(get(&c, &mut inv, 36), st(block::STONE, 10));
    click(&mut c, &mut inv, 36, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 10));
    // Tops up a matching hotbar stack before an empty slot.
    set(&mut c, &mut inv, 40, st(block::STONE, 60));
    click(&mut c, &mut inv, 9, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 40), st(block::STONE, 64));
    assert_eq!(get(&c, &mut inv, 36), st(block::STONE, 6));
}

#[test]
fn shift_click_equips_armor_and_shield() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 20, st(item::IRON_HELMET, 1));
    set(&mut c, &mut inv, 21, st(item::IRON_HELMET, 1));
    set(&mut c, &mut inv, 22, st(item::SHIELD, 1));
    click(&mut c, &mut inv, 20, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 5), st(item::IRON_HELMET, 1));
    // The head is taken, so the second helmet goes to the hotbar.
    click(&mut c, &mut inv, 21, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 36), st(item::IRON_HELMET, 1));
    click(&mut c, &mut inv, 22, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 45), st(item::SHIELD, 1));
    // Out of an armor slot back into the inventory.
    click(&mut c, &mut inv, 5, 0, ClickType::QuickMove);
    assert!(get(&c, &mut inv, 5).is_empty());
    assert_eq!(get(&c, &mut inv, 9), st(item::IRON_HELMET, 1));
}

#[test]
fn throw_drops_one_or_the_stack() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 10));
    let (_, d) = click(&mut c, &mut inv, 9, 0, ClickType::Throw);
    assert_eq!(d, vec![st(block::STONE, 1)]);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 9));
    let (_, d) = click(&mut c, &mut inv, 9, 1, ClickType::Throw);
    assert_eq!(d, vec![st(block::STONE, 9)]);
    assert!(get(&c, &mut inv, 9).is_empty());
    // Not while holding something.
    set(&mut c, &mut inv, 9, st(block::STONE, 3));
    inv.cursor = st(block::DIRT, 1);
    let (_, d) = click(&mut c, &mut inv, 9, 1, ClickType::Throw);
    assert!(d.is_empty());
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 3));
}

#[test]
fn clicking_outside_drops_the_cursor() {
    let (mut c, mut inv) = setup();
    inv.cursor = st(block::STONE, 10);
    let (_, d) = click(&mut c, &mut inv, -999, 1, ClickType::Pickup);
    assert_eq!(d, vec![st(block::STONE, 1)]);
    assert_eq!(inv.cursor, st(block::STONE, 9));
    let (_, d) = click(&mut c, &mut inv, -999, 0, ClickType::Pickup);
    assert_eq!(d, vec![st(block::STONE, 9)]);
    assert!(inv.cursor.is_empty());
}

#[test]
fn clone_only_in_creative() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 3));
    click(&mut c, &mut inv, 9, 2, ClickType::Clone);
    assert!(inv.cursor.is_empty());
    click_as(&mut c, &mut inv, true, 9, 2, ClickType::Clone);
    assert_eq!(inv.cursor, st(block::STONE, 64));
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 3));
}

#[test]
fn number_keys_swap_with_the_hotbar() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 10));
    inv.inv.main[2] = st(block::DIRT, 3);
    click(&mut c, &mut inv, 9, 2, ClickType::Swap);
    assert_eq!(get(&c, &mut inv, 9), st(block::DIRT, 3));
    assert_eq!(inv.main[2], st(block::STONE, 10));
    // Into an empty hotbar slot.
    click(&mut c, &mut inv, 9, 5, ClickType::Swap);
    assert!(get(&c, &mut inv, 9).is_empty());
    assert_eq!(inv.main[5], st(block::DIRT, 3));
    // From the hotbar into an empty slot.
    click(&mut c, &mut inv, 10, 5, ClickType::Swap);
    assert_eq!(get(&c, &mut inv, 10), st(block::DIRT, 3));
    assert!(inv.main[5].is_empty());
    // Only the armor piece the slot takes; a pumpkin stack leaves the rest behind.
    inv.inv.main[0] = st(block::PUMPKIN, 10);
    click(&mut c, &mut inv, 6, 0, ClickType::Swap);
    assert!(get(&c, &mut inv, 6).is_empty());
    click(&mut c, &mut inv, 5, 0, ClickType::Swap);
    assert_eq!(get(&c, &mut inv, 5), st(block::PUMPKIN, 1));
    assert_eq!(inv.main[0], st(block::PUMPKIN, 9));
    // Button 9 is not a hotbar slot.
    click(&mut c, &mut inv, 10, 9, ClickType::Swap);
    assert_eq!(get(&c, &mut inv, 10), st(block::DIRT, 3));
}

#[test]
fn left_drag_splits_evenly() {
    let (mut c, mut inv) = setup();
    inv.cursor = st(block::STONE, 10);
    drag(&mut c, &mut inv, false, 0, &[9, 10, 11]);
    for i in 9..12 {
        assert_eq!(get(&c, &mut inv, i), st(block::STONE, 3));
    }
    assert_eq!(inv.cursor, st(block::STONE, 1));
    assert_eq!(c.drag_event, 0);
    assert!(c.drag_slots.is_empty());
}

#[test]
fn right_drag_places_one_each() {
    let (mut c, mut inv) = setup();
    inv.cursor = st(block::STONE, 10);
    drag(&mut c, &mut inv, false, 1, &[9, 10, 11, 10]);
    for i in 9..12 {
        assert_eq!(get(&c, &mut inv, i), st(block::STONE, 1));
    }
    assert_eq!(inv.cursor, st(block::STONE, 7));
}

#[test]
fn left_drag_tops_up_and_caps_at_the_max() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 10, st(block::STONE, 62));
    set(&mut c, &mut inv, 11, st(block::DIRT, 1));
    inv.cursor = st(block::STONE, 10);
    // Slot 11 holds dirt and is skipped.
    drag(&mut c, &mut inv, false, 0, &[9, 10, 11]);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 5));
    assert_eq!(get(&c, &mut inv, 10), st(block::STONE, 64));
    assert_eq!(get(&c, &mut inv, 11), st(block::DIRT, 1));
    assert_eq!(inv.cursor, st(block::STONE, 3));
}

#[test]
fn a_drag_cannot_cover_more_slots_than_items() {
    let (mut c, mut inv) = setup();
    inv.cursor = st(block::STONE, 2);
    drag(&mut c, &mut inv, false, 0, &[9, 10, 11]);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 1));
    assert_eq!(get(&c, &mut inv, 10), st(block::STONE, 1));
    assert!(get(&c, &mut inv, 11).is_empty());
    assert!(inv.cursor.is_empty());
}

#[test]
fn middle_drag_is_creative_only() {
    let (mut c, mut inv) = setup();
    inv.cursor = st(block::STONE, 1);
    drag(&mut c, &mut inv, false, 2, &[9, 10]);
    assert!(get(&c, &mut inv, 9).is_empty());
    assert_eq!(inv.cursor, st(block::STONE, 1));
    drag(&mut c, &mut inv, true, 2, &[9, 10]);
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 64));
    assert_eq!(get(&c, &mut inv, 10), st(block::STONE, 64));
}

#[test]
fn another_click_during_a_drag_only_cancels_it() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 12, st(block::DIRT, 4));
    inv.cursor = st(block::STONE, 10);
    click(&mut c, &mut inv, -999, get_quickcraft_mask(0, 0), ClickType::QuickCraft);
    click(&mut c, &mut inv, 9, get_quickcraft_mask(1, 0), ClickType::QuickCraft);
    click(&mut c, &mut inv, 12, 0, ClickType::Pickup);
    assert_eq!(c.drag_event, 0);
    assert_eq!(get(&c, &mut inv, 12), st(block::DIRT, 4));
    assert_eq!(inv.cursor, st(block::STONE, 10));
    // The end of the abandoned drag does nothing either.
    click(&mut c, &mut inv, -999, get_quickcraft_mask(2, 0), ClickType::QuickCraft);
    assert!(get(&c, &mut inv, 9).is_empty());
    assert_eq!(inv.cursor, st(block::STONE, 10));
}

#[test]
fn double_click_takes_part_stacks_before_full_ones() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 10, st(block::STONE, 64));
    set(&mut c, &mut inv, 11, st(block::STONE, 10));
    set(&mut c, &mut inv, 13, st(block::DIRT, 10));
    inv.cursor = st(block::STONE, 5);
    click(&mut c, &mut inv, 12, 0, ClickType::PickupAll);
    assert_eq!(inv.cursor, st(block::STONE, 64));
    assert!(get(&c, &mut inv, 11).is_empty());
    assert_eq!(get(&c, &mut inv, 10), st(block::STONE, 15));
    assert_eq!(get(&c, &mut inv, 13), st(block::DIRT, 10));
    // Not over a slot that holds something.
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 10, st(block::STONE, 3));
    set(&mut c, &mut inv, 11, st(block::STONE, 3));
    inv.cursor = st(block::STONE, 5);
    click(&mut c, &mut inv, 10, 0, ClickType::PickupAll);
    assert_eq!(inv.cursor, st(block::STONE, 5));
}

#[test]
fn bad_slot_ids_change_nothing() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 10));
    inv.cursor = st(block::DIRT, 2);
    for slot in [-1, -2, 46, 100] {
        for ct in [ClickType::Pickup, ClickType::QuickMove, ClickType::Swap, ClickType::Clone, ClickType::Throw, ClickType::PickupAll] {
            for button in 0..3 {
                let (r, d) = click(&mut c, &mut inv, slot, button, ct);
                assert!(r.is_empty() && d.is_empty(), "{slot} {ct:?} {button}");
            }
        }
    }
    assert_eq!(get(&c, &mut inv, 9), st(block::STONE, 10));
    assert_eq!(inv.cursor, st(block::DIRT, 2));
}

#[test]
fn click_types_and_masks() {
    assert_eq!(ClickType::from_ordinal(0), Some(ClickType::Pickup));
    assert_eq!(ClickType::from_ordinal(6), Some(ClickType::PickupAll));
    assert_eq!(ClickType::from_ordinal(7), None);
    assert_eq!(ClickType::QuickCraft.ordinal(), 5);
    for event in 0..3 {
        for mode in 0..3 {
            let m = get_quickcraft_mask(event, mode);
            assert_eq!((get_drag_event(m), extract_drag_mode(m)), (event, mode));
        }
    }
    assert!(is_valid_drag_mode(1, false) && !is_valid_drag_mode(2, false) && is_valid_drag_mode(2, true) && !is_valid_drag_mode(3, true));
    let mut s = st(block::STONE, 10);
    compute_stack_size(3, 0, &mut s, 2);
    assert_eq!(s.get_count(), 5);
}

#[test]
fn detect_and_send_changes_reports_only_changed_slots() {
    let (mut c, mut inv) = setup();
    set(&mut c, &mut inv, 9, st(block::STONE, 10));
    c.detect_and_send_changes(&PlayerRef::new(&mut inv, false));
    assert_eq!(c.listener, vec![(9, st(block::STONE, 10))]);
    c.listener.clear();
    c.detect_and_send_changes(&PlayerRef::new(&mut inv, false));
    assert!(c.listener.is_empty());
    click(&mut c, &mut inv, 9, 1, ClickType::Pickup);
    c.detect_and_send_changes(&PlayerRef::new(&mut inv, false));
    assert_eq!(c.listener, vec![(9, st(block::STONE, 5))]);
}
