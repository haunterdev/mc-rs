//! Crafting through `ContainerPlayer` and `ContainerWorkbench`, closing a window, and the seeded
//! random-click fuzz.

use std::collections::HashMap;
use std::sync::Arc;

use items::ids::{block, item};
use items::{ItemStack, JavaRandom, Recipes};

use super::player::{get_slot_for_item_stack, EquipmentSlot};
use super::tests::{click, click_as, drag, get, recipes, set, st};
use super::*;
use crate::inventory::InventoryPlayer;

fn player() -> (Container, InventoryPlayer) {
    (Container::player(recipes()), InventoryPlayer::new())
}

fn workbench() -> (Container, InventoryPlayer) {
    (Container::workbench(1, [0, 64, 0], recipes()), InventoryPlayer::new())
}

#[test]
fn planks_from_a_log() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 1, st(block::LOG, 1));
    assert_eq!(get(&c, &mut inv, 0), st(block::PLANKS, 4));
    click(&mut c, &mut inv, 0, 0, ClickType::Pickup);
    assert_eq!(inv.cursor, st(block::PLANKS, 4));
    assert!(get(&c, &mut inv, 1).is_empty());
    assert!(get(&c, &mut inv, 0).is_empty());
}

#[test]
fn clicking_the_result_again_adds_to_the_cursor() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 4, st(block::LOG, 3));
    click(&mut c, &mut inv, 0, 0, ClickType::Pickup);
    click(&mut c, &mut inv, 0, 1, ClickType::Pickup);
    assert_eq!(inv.cursor, st(block::PLANKS, 8));
    assert_eq!(get(&c, &mut inv, 4), st(block::LOG, 1));
    // A different item on the cursor takes nothing.
    inv.cursor = st(block::DIRT, 1);
    click(&mut c, &mut inv, 0, 0, ClickType::Pickup);
    assert_eq!(inv.cursor, st(block::DIRT, 1));
    assert_eq!(get(&c, &mut inv, 4), st(block::LOG, 1));
}

#[test]
fn sticks_and_a_crafting_table_in_the_2x2() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 2, ItemStack::new(block::PLANKS, 1, 2));
    set(&mut c, &mut inv, 4, ItemStack::new(block::PLANKS, 1, 0));
    assert_eq!(get(&c, &mut inv, 0), st(item::STICK, 4));
    set(&mut c, &mut inv, 1, st(block::PLANKS, 1));
    set(&mut c, &mut inv, 3, st(block::PLANKS, 1));
    assert_eq!(get(&c, &mut inv, 0), st(block::CRAFTING_TABLE, 1));
}

#[test]
fn the_2x2_refuses_3x3_recipes() {
    let (mut c, mut inv) = player();
    // The top two rows of the axe.
    set(&mut c, &mut inv, 1, st(block::PLANKS, 1));
    set(&mut c, &mut inv, 2, st(block::PLANKS, 1));
    set(&mut c, &mut inv, 3, st(block::PLANKS, 1));
    set(&mut c, &mut inv, 4, st(item::STICK, 1));
    assert!(get(&c, &mut inv, 0).is_empty());
    let r = recipes();
    assert!(!r.get("wooden_axe").unwrap().recipe.can_fit(2, 2));
    assert!(!r.get("wooden_pickaxe").unwrap().recipe.can_fit(2, 2));
}

#[test]
fn a_pickaxe_in_the_workbench() {
    let (mut c, mut inv) = workbench();
    assert_eq!(c.slots.len(), 46);
    for s in [1, 2, 3] {
        set(&mut c, &mut inv, s, st(block::PLANKS, 1));
    }
    set(&mut c, &mut inv, 5, st(item::STICK, 1));
    assert!(get(&c, &mut inv, 0).is_empty());
    set(&mut c, &mut inv, 8, st(item::STICK, 1));
    assert_eq!(get(&c, &mut inv, 0), st(item::WOODEN_PICKAXE, 1));
    click(&mut c, &mut inv, 0, 0, ClickType::Pickup);
    assert_eq!(inv.cursor, st(item::WOODEN_PICKAXE, 1));
    for s in 1..10 {
        assert!(get(&c, &mut inv, s).is_empty(), "grid slot {s}");
    }
}

#[test]
fn the_axe_crafts_either_way_round() {
    for (planks, sticks) in [([1, 2, 4], [5, 8]), ([1, 2, 5], [4, 7])] {
        let (mut c, mut inv) = workbench();
        for s in planks {
            set(&mut c, &mut inv, s, st(block::PLANKS, 1));
        }
        for s in sticks {
            set(&mut c, &mut inv, s, st(item::STICK, 1));
        }
        assert_eq!(get(&c, &mut inv, 0), st(item::WOODEN_AXE, 1), "{planks:?} {sticks:?}");
    }
}

#[test]
fn shapeless_ignores_placement() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 4, st(item::FLINT, 1));
    set(&mut c, &mut inv, 1, st(item::IRON_INGOT, 1));
    assert_eq!(get(&c, &mut inv, 0), st(item::FLINT_AND_STEEL, 1));
    set(&mut c, &mut inv, 2, st(item::FLINT, 1));
    assert!(get(&c, &mut inv, 0).is_empty());
}

#[test]
fn milk_buckets_leave_buckets_in_the_grid() {
    let (mut c, mut inv) = workbench();
    for s in [1, 2, 3] {
        set(&mut c, &mut inv, s, st(item::MILK_BUCKET, 1));
    }
    set(&mut c, &mut inv, 4, st(item::SUGAR, 1));
    set(&mut c, &mut inv, 5, st(item::EGG, 1));
    set(&mut c, &mut inv, 6, st(item::SUGAR, 1));
    for s in [7, 8, 9] {
        set(&mut c, &mut inv, s, st(item::WHEAT, 1));
    }
    assert_eq!(get(&c, &mut inv, 0), st(item::CAKE, 1));
    click(&mut c, &mut inv, 0, 0, ClickType::Pickup);
    assert_eq!(inv.cursor, st(item::CAKE, 1));
    for s in [1, 2, 3] {
        assert_eq!(get(&c, &mut inv, s), st(item::BUCKET, 1));
    }
    for s in 4..10 {
        assert!(get(&c, &mut inv, s).is_empty());
    }
    assert!(get(&c, &mut inv, 0).is_empty());
}

#[test]
fn shift_click_on_the_result_crafts_as_many_as_fit() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 1, st(block::LOG, 5));
    click(&mut c, &mut inv, 0, 0, ClickType::QuickMove);
    // mergeItemStack(9, 45, true) fills from the last hotbar slot.
    assert_eq!(inv.main[8], st(block::PLANKS, 20));
    assert!(get(&c, &mut inv, 1).is_empty());
    assert!(get(&c, &mut inv, 0).is_empty());

    // One free slot: sixteen crafts fill it and the rest stays in the grid.
    let (mut c, mut inv) = player();
    for i in 0..36 {
        if i != 8 {
            inv.inv.main[i] = st(block::DIRT, 64);
        }
    }
    set(&mut c, &mut inv, 1, st(block::LOG, 20));
    let (_, dropped) = click(&mut c, &mut inv, 0, 0, ClickType::QuickMove);
    assert!(dropped.is_empty());
    assert_eq!(inv.main[8], st(block::PLANKS, 64));
    assert_eq!(get(&c, &mut inv, 1), st(block::LOG, 4));
    assert_eq!(get(&c, &mut inv, 0), st(block::PLANKS, 4));
    assert!(inv.off_hand[0].is_empty());
}

#[test]
fn shift_click_in_the_workbench() {
    let (mut c, mut inv) = workbench();
    set(&mut c, &mut inv, 1, st(block::LOG, 2));
    click(&mut c, &mut inv, 0, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 45), st(block::PLANKS, 8));
    click(&mut c, &mut inv, 45, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 10), st(block::PLANKS, 8));
    click(&mut c, &mut inv, 10, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 37), st(block::PLANKS, 8));
    // Out of the grid into the inventory.
    set(&mut c, &mut inv, 5, st(block::STONE, 3));
    click(&mut c, &mut inv, 5, 0, ClickType::QuickMove);
    assert_eq!(get(&c, &mut inv, 10), st(block::STONE, 3));
}

#[test]
fn number_key_on_the_result_crafts() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 1, st(block::LOG, 2));
    click(&mut c, &mut inv, 0, 3, ClickType::Swap);
    assert_eq!(inv.main[3], st(block::PLANKS, 4));
    assert_eq!(get(&c, &mut inv, 1), st(block::LOG, 1));
    assert_eq!(get(&c, &mut inv, 0), st(block::PLANKS, 4));
    // The result takes nothing from the hotbar.
    click(&mut c, &mut inv, 0, 3, ClickType::Swap);
    assert_eq!(inv.main[3], st(block::PLANKS, 4));
}

#[test]
fn throwing_the_result_crafts_and_drops_it() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 1, st(block::LOG, 1));
    let (_, d) = click(&mut c, &mut inv, 0, 0, ClickType::Throw);
    assert_eq!(d, vec![st(block::PLANKS, 4)]);
    assert!(get(&c, &mut inv, 1).is_empty());
}

#[test]
fn double_click_skips_the_result() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 1, st(block::LOG, 1));
    inv.cursor = st(block::PLANKS, 1);
    click(&mut c, &mut inv, 9, 0, ClickType::PickupAll);
    assert_eq!(inv.cursor, st(block::PLANKS, 1));
    assert_eq!(get(&c, &mut inv, 0), st(block::PLANKS, 4));
    assert_eq!(get(&c, &mut inv, 1), st(block::LOG, 1));
}

#[test]
fn a_right_drag_lays_out_a_recipe() {
    let (mut c, mut inv) = player();
    inv.cursor = st(block::PLANKS, 3);
    drag(&mut c, &mut inv, false, 1, &[1, 3]);
    assert_eq!(get(&c, &mut inv, 0), st(item::STICK, 4));
    assert_eq!(inv.cursor, st(block::PLANKS, 1));
}

#[test]
fn closing_gives_the_grid_back_and_drops_the_cursor() {
    let (mut c, mut inv) = player();
    set(&mut c, &mut inv, 1, st(block::LOG, 3));
    set(&mut c, &mut inv, 9, st(block::LOG, 62));
    inv.cursor = st(block::STONE, 5);
    let mut p = PlayerRef::new(&mut inv, false);
    c.on_container_closed(&mut p);
    let dropped = p.dropped;
    assert_eq!(dropped, vec![st(block::STONE, 5)]);
    assert!(inv.cursor.is_empty());
    assert_eq!(inv.main[9], st(block::LOG, 64));
    assert_eq!(inv.main[0], st(block::LOG, 1));
    assert!(c.crafting.as_ref().unwrap().craft_matrix.slots.iter().all(|s| s.is_empty()));
    assert!(c.crafting.as_ref().unwrap().craft_result.stack.is_empty());

    // A dead player drops the grid.
    let (mut c, mut inv) = workbench();
    set(&mut c, &mut inv, 5, st(block::LOG, 3));
    set(&mut c, &mut inv, 9, st(item::STICK, 2));
    let mut p = PlayerRef::new(&mut inv, false);
    p.alive = false;
    c.on_container_closed(&mut p);
    assert_eq!(p.dropped, vec![st(block::LOG, 3), st(item::STICK, 2)]);
    assert!(inv.main.iter().all(|s| s.is_empty()));

    // A full inventory drops what does not fit.
    let (mut c, mut inv) = workbench();
    for i in 0..36 {
        inv.inv.main[i] = st(block::DIRT, 64);
    }
    set(&mut c, &mut inv, 2, st(block::LOG, 3));
    let mut p = PlayerRef::new(&mut inv, false);
    c.on_container_closed(&mut p);
    assert_eq!(p.dropped, vec![st(block::LOG, 3)]);
}

#[test]
fn workbench_reach() {
    let c = Container::workbench(1, [10, 64, 10], recipes());
    assert!(c.can_interact_with(block::CRAFTING_TABLE, [10.5, 64.0, 10.5]));
    assert!(c.can_interact_with(block::CRAFTING_TABLE, [18.5, 64.5, 10.5]));
    assert!(!c.can_interact_with(block::CRAFTING_TABLE, [18.6, 64.5, 10.5]));
    assert!(!c.can_interact_with(block::STONE, [10.5, 64.0, 10.5]));
    assert!(Container::player(recipes()).can_interact_with(0, [1e6, 0.0, 0.0]));
}

#[test]
fn equipment_slot_for_items() {
    assert_eq!(get_slot_for_item_stack(&st(item::IRON_HELMET, 1)), EquipmentSlot::Head);
    assert_eq!(get_slot_for_item_stack(&st(item::IRON_BOOTS, 1)), EquipmentSlot::Feet);
    assert_eq!(get_slot_for_item_stack(&st(block::PUMPKIN, 1)), EquipmentSlot::Head);
    assert_eq!(get_slot_for_item_stack(&st(item::SKULL, 1)), EquipmentSlot::Head);
    assert_eq!(get_slot_for_item_stack(&st(item::ELYTRA, 1)), EquipmentSlot::Chest);
    assert_eq!(get_slot_for_item_stack(&st(item::SHIELD, 1)), EquipmentSlot::OffHand);
    assert_eq!(get_slot_for_item_stack(&st(block::STONE, 1)), EquipmentSlot::MainHand);
    assert_eq!(get_slot_for_item_stack(&ItemStack::EMPTY), EquipmentSlot::MainHand);
}

// ---- fuzz ---------------------------------------------------------------------------------

const POOL: &[u16] = &[
    block::STONE,
    block::DIRT,
    block::LOG,
    block::PLANKS,
    block::PUMPKIN,
    item::STICK,
    item::ENDER_PEARL,
    item::IRON_HELMET,
    item::IRON_BOOTS,
    item::IRON_INGOT,
    item::FLINT,
    item::SHIELD,
    item::MILK_BUCKET,
    item::DIAMOND_SWORD,
];

fn random_inventory(r: &mut JavaRandom) -> InventoryPlayer {
    let mut inv = InventoryPlayer::new();
    for i in 0..36 {
        if r.next_int_bounded(3) != 0 {
            let id = POOL[r.next_int_bounded(POOL.len() as i32) as usize];
            let max = ItemStack::of(id).get_max_stack_size();
            inv.inv.main[i] = st(id, 1 + r.next_int_bounded(max));
        }
    }
    inv
}

fn random_click(r: &mut JavaRandom, n_slots: usize) -> (i32, i32, ClickType) {
    let ct = ClickType::from_ordinal(r.next_int_bounded(7) as u8).unwrap();
    let slot = match r.next_int_bounded(12) {
        0 => -999,
        1 => -1,
        2 => n_slots as i32 + r.next_int_bounded(3),
        _ => r.next_int_bounded(n_slots as i32),
    };
    let button = if ct == ClickType::QuickCraft {
        get_quickcraft_mask(r.next_int_bounded(3), r.next_int_bounded(3))
    } else {
        r.next_int_bounded(10)
    };
    (slot, button, ct)
}

/// Every stack the player and the window hold, in a fixed order.
fn snapshot(c: &Container, inv: &InventoryPlayer) -> Vec<ItemStack> {
    let mut v: Vec<ItemStack> = (0..41).map(|i| inv.get_stack_in_slot(i)).collect();
    v.push(inv.cursor);
    if let Some(cr) = &c.crafting {
        v.extend(cr.craft_matrix.slots.iter().copied());
        v.push(cr.craft_result.stack);
    }
    v
}

fn totals<'a>(stacks: impl Iterator<Item = &'a ItemStack>) -> HashMap<u16, i32> {
    let mut m = HashMap::new();
    for s in stacks.filter(|s| !s.is_empty()) {
        *m.entry(s.get_item()).or_insert(0) += s.get_count();
    }
    m
}

/// No stack over its size, armor slots hold one piece that belongs there.
fn check_invariants(c: &Container, inv: &mut InventoryPlayer, step: usize) {
    for i in 0..c.slots.len() {
        let s = get(c, inv, i);
        if s.is_empty() {
            continue;
        }
        assert!(s.get_count() <= s.get_max_stack_size(), "step {step}: slot {i} holds {s:?}");
        if let SlotKind::Armor(e) = c.slots[i].kind {
            assert_eq!(s.get_count(), 1, "step {step}: armor slot {i}");
            assert_eq!(get_slot_for_item_stack(&s), e, "step {step}: armor slot {i} holds {s:?}");
        }
    }
    assert!(inv.cursor.is_empty() || inv.cursor.get_count() <= inv.cursor.get_max_stack_size(), "step {step}: cursor {:?}", inv.cursor);
}

/// Survival with no recipes moves items and never makes or loses one.
#[test]
fn fuzz_survival_clicks_conserve_items() {
    for seed in 0..4i64 {
        let mut r = JavaRandom::new(seed);
        let mut inv = random_inventory(&mut r);
        let mut c = if seed % 2 == 0 {
            Container::player(Arc::new(Recipes::default()))
        } else {
            Container::workbench(1, [0, 0, 0], Arc::new(Recipes::default()))
        };
        let want = totals(snapshot(&c, &inv).iter());
        let mut dropped = Vec::new();
        for step in 0..3000 {
            let (slot, button, ct) = random_click(&mut r, c.slots.len());
            let (_, d) = click(&mut c, &mut inv, slot, button, ct);
            dropped.extend(d);
            check_invariants(&c, &mut inv, step);
        }
        let mut p = PlayerRef::new(&mut inv, false);
        c.on_container_closed(&mut p);
        dropped.extend(p.dropped);
        let all = snapshot(&c, &inv);
        assert_eq!(totals(all.iter().chain(dropped.iter())), want, "seed {seed}");
    }
}

/// Two copies of a window fed the same clicks end equal: the server's and the viewer's.
#[test]
fn fuzz_two_copies_stay_equal() {
    for seed in 0..4i64 {
        let mut r = JavaRandom::new(1000 + seed);
        let inv0 = random_inventory(&mut r);
        let c0 = if seed % 2 == 0 { Container::player(recipes()) } else { Container::workbench(3, [0, 0, 0], recipes()) };
        let (mut ca, mut ia) = (c0.clone(), inv0.clone());
        let (mut cb, mut ib) = (c0, inv0);
        let creative = seed >= 2;
        for step in 0..3000 {
            let (slot, button, ct) = random_click(&mut r, ca.slots.len());
            let (ra, da) = click_as(&mut ca, &mut ia, creative, slot, button, ct);
            let (rb, db) = click_as(&mut cb, &mut ib, creative, slot, button, ct);
            assert_eq!((ra, da), (rb, db), "seed {seed} step {step}");
            assert_eq!(snapshot(&ca, &ia), snapshot(&cb, &ib), "seed {seed} step {step}");
            assert_eq!((ca.drag_event, ca.drag_mode, &ca.drag_slots), (cb.drag_event, cb.drag_mode, &cb.drag_slots));
            if !creative {
                check_invariants(&ca, &mut ia, step);
            }
        }
    }
}
