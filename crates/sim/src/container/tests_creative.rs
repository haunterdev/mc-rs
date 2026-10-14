//! `ContainerCreative`: the slot layout, `scrollTo`, the shift-click delete and the merge and
//! drag rules.

use items::ids::{block, item};

use super::tests::{click_as, st};
use super::*;
use crate::inventory::InventoryPlayer;

fn list(n: usize) -> Vec<ItemStack> {
    (0..n).map(|i| ItemStack::new(block::STONE, 1, i as i32 % 7)).collect()
}

#[test]
fn creative_window_layout() {
    let c = Container::creative();
    assert_eq!(c.slots.len(), 54);
    assert_eq!((c.slots[0].x, c.slots[0].y, c.slots[0].inventory), (9, 18, Inv::Basic));
    assert_eq!((c.slots[44].x, c.slots[44].y, c.slots[44].index), (9 + 8 * 18, 18 + 4 * 18, 44));
    assert_eq!((c.slots[45].x, c.slots[45].y, c.slots[45].inventory, c.slots[45].index), (9, 112, Inv::Player, 0));
    assert!(!c.can_scroll());
}

#[test]
fn scroll_to_fills_the_rows() {
    let mut c = Container::creative();
    let items = list(100);
    c.creative.as_mut().unwrap().item_list = items.clone();
    assert!(c.can_scroll());
    c.scroll_to(0.0);
    assert_eq!(c.creative.as_ref().unwrap().basic[0], items[0]);
    assert_eq!(c.creative.as_ref().unwrap().basic[44], items[44]);
    // 100 items are 12 rows, 7 past the five shown: the bottom shows rows 7..12.
    c.scroll_to(1.0);
    let b = &c.creative.as_ref().unwrap().basic;
    assert_eq!(b[0], items[63]);
    assert_eq!(b[36], items[99]);
    assert!(b[37].is_empty());
    // (0.5 * 7 + 0.5) as int = 4.
    c.scroll_to(0.5);
    assert_eq!(c.creative.as_ref().unwrap().basic[0], items[36]);
}

#[test]
fn shift_click_on_the_hotbar_deletes() {
    let mut c = Container::creative();
    let mut inv = InventoryPlayer::new();
    inv.main[3] = st(item::DIAMOND, 5);
    let (r, dropped) = click_as(&mut c, &mut inv, true, 48, 0, ClickType::QuickMove);
    assert!(r.is_empty() && dropped.is_empty());
    assert!(inv.main[3].is_empty());
    // A shift-click in the list moves nothing.
    c.creative.as_mut().unwrap().item_list = list(9);
    c.scroll_to(0.0);
    click_as(&mut c, &mut inv, true, 2, 0, ClickType::QuickMove);
    assert_eq!(c.creative.as_ref().unwrap().basic[2], ItemStack::new(block::STONE, 1, 2));
}

#[test]
fn merge_and_drag_stay_out_of_the_list() {
    let c = Container::creative();
    let s = st(item::DIAMOND, 1);
    assert!(!c.can_merge_slot(&s, 0));
    assert!(c.can_merge_slot(&s, 45));
    assert!(!c.can_drag_into_slot(10));
    assert!(c.can_drag_into_slot(53));
}

#[test]
fn a_drag_across_the_hotbar_splits_the_cursor() {
    let mut c = Container::creative();
    let mut inv = InventoryPlayer::new();
    inv.cursor = st(item::DIAMOND, 10);
    click_as(&mut c, &mut inv, true, -999, get_quickcraft_mask(0, 0), ClickType::QuickCraft);
    for slot in [45, 46, 3] {
        click_as(&mut c, &mut inv, true, slot, get_quickcraft_mask(1, 0), ClickType::QuickCraft);
    }
    click_as(&mut c, &mut inv, true, -999, get_quickcraft_mask(2, 0), ClickType::QuickCraft);
    assert_eq!(inv.main[0].get_count(), 5);
    assert_eq!(inv.main[1].get_count(), 5);
    assert!(inv.cursor.is_empty());
    assert!(c.creative.as_ref().unwrap().basic[3].is_empty());
}
