//! Server windows (T7a) over a seeded world: a stone slab at y 200 under x, z 4..=13, the player at
//! (5.5, 201, 5.5) and a crafting table at (7, 201, 6).

use std::sync::Arc;

use ::items::ids::{block, item};
use ::items::{ItemStack, JavaRandom, Recipes};
use proto::{PlayerState, ON_GROUND};
use sim::container::{ClickType, Container, PlayerRef};
use sim::inventory::InventoryPlayer;

use crate::game::testing::{run, Harness};
use crate::game::{terrain, GameMsg};

const STONE: u16 = block::STONE << 4;
const TABLE_AT: [i32; 3] = [7, 201, 6];

const RECIPES: &[(&str, &str)] = &[
    ("oak_planks", r#"{"type":"crafting_shapeless","group":"planks","ingredients":[{"item":"minecraft:log","data":0}],"result":{"item":"minecraft:planks","data":0,"count":4}}"#),
    ("stick", r##"{"type":"crafting_shaped","pattern":["#","#"],"key":{"#":{"item":"minecraft:planks","data":32767}},"result":{"item":"minecraft:stick","count":4}}"##),
];

fn recipes() -> Arc<Recipes> {
    Arc::new(Recipes::from_files(RECIPES.iter().map(|(n, j)| (*n, j.as_bytes()))).unwrap())
}

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.game.windows.recipes = Some(recipes());
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    terrain::set_block(&mut h.game, (TABLE_AT[0], TABLE_AT[1], TABLE_AT[2]), block::CRAFTING_TABLE << 4);
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.tick(1);
    h.frames_for(1);
    h
}

fn tags(frames: &[(u8, Vec<u8>)], tag: u8) -> Vec<&[u8]> {
    frames.iter().filter(|f| f.0 == tag).map(|f| f.1.as_slice()).collect()
}

fn give(h: &mut Harness, slot: usize, s: ItemStack) {
    h.game.players.players.get_mut(&1).unwrap().inv.main[slot] = s;
}

fn main(h: &Harness, slot: usize) -> ItemStack {
    h.game.players.players[&1].inv.main[slot]
}

fn send_click(h: &mut Harness, window: u8, slot: i16, button: u8, mode: u8, action: u16) {
    h.push_frame(1, proto::V_CLICK, &proto::Click { window, slot, button, mode, action }.encode());
}

fn use_table(h: &mut Harness) {
    let m = proto::UseOn { pos: TABLE_AT, face: 1, hit: [0.5; 3], hand: 0, state: 0 };
    h.push_frame(1, proto::V_USE_ON, &m.encode());
}

fn st(id: u16, n: i32) -> ItemStack {
    ItemStack::new(id, n, 0)
}

/// Opens the table and drops the frames it sent.
fn open_table(h: &mut Harness) -> Vec<(u8, Vec<u8>)> {
    use_table(h);
    h.tick(1);
    h.frames_for(1)
}

#[test]
fn using_a_crafting_table_opens_window_1_with_all_its_slots() {
    run(|| {
        let mut h = setup();
        give(&mut h, 0, st(block::LOG, 3));
        h.tick(1);
        h.frames_for(1);
        let frames = open_table(&mut h);
        let o = proto::OpenWindow::decode(tags(&frames, proto::S_OPEN_WINDOW)[0]).unwrap();
        assert_eq!((o.window, o.kind, o.pos, o.title.as_str()), (1, proto::WK_CRAFTING_TABLE, TABLE_AT, "container.crafting"));
        let slots = proto::WindowSlots::decode(tags(&frames, proto::S_SLOTS)[0]).unwrap();
        assert_eq!(slots.window, 1);
        // result, 9 grid, 27 main, 9 hotbar, then the cursor
        assert_eq!(slots.slots.len(), 47);
        assert_eq!(slots.slots[37], (37, crate::game::items::wire_stack(&st(block::LOG, 3))));
        assert_eq!(slots.slots[46].0, -1);
    });
}

#[test]
fn a_table_out_of_reach_or_not_a_table_is_left_to_others() {
    run(|| {
        let mut h = setup();
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5 + 12.0], ..Default::default() });
        assert!(open_table(&mut h).iter().all(|f| f.0 != proto::S_OPEN_WINDOW));
        let m = proto::UseOn { pos: [8, 200, 8], face: 1, hit: [0.5; 3], hand: 0, state: 0 };
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], ..Default::default() });
        h.push_frame(1, proto::V_USE_ON, &m.encode());
        h.tick(1);
        assert!(h.frames_for(1).iter().all(|f| f.0 != proto::S_OPEN_WINDOW));
        assert_eq!(h.game.stats.frames_unhandled, 0, "use_on takes every V_USE_ON");
    });
}

#[test]
fn window_ids_count_from_1_and_wrap_after_100() {
    run(|| {
        let mut h = setup();
        for want in [1u8, 2] {
            let frames = open_table(&mut h);
            assert_eq!(proto::OpenWindow::decode(tags(&frames, proto::S_OPEN_WINDOW)[0]).unwrap().window, want);
            h.push_frame(1, proto::V_CLOSE, &proto::CloseWindow { window: want }.encode());
            h.tick(1);
            h.frames_for(1);
        }
        h.game.windows.players.get_mut(&1).unwrap().window_counter = 100;
        let frames = open_table(&mut h);
        assert_eq!(proto::OpenWindow::decode(tags(&frames, proto::S_OPEN_WINDOW)[0]).unwrap().window, 1);
    });
}

#[test]
fn crafting_planks_is_confirmed_and_needs_no_slot_frames() {
    run(|| {
        let mut h = setup();
        give(&mut h, 0, st(block::LOG, 1));
        h.tick(1);
        let sync = h.frames_for(1);
        assert_eq!(tags(&sync, proto::S_SLOTS).len(), 1, "the pickup-less change goes out as a diff");
        // log from hotbar slot 36 into grid slot 1, then take the planks to the hotbar slot 37
        send_click(&mut h, 0, 36, 0, 0, 1);
        send_click(&mut h, 0, 1, 0, 0, 2);
        send_click(&mut h, 0, 0, 0, 0, 3);
        send_click(&mut h, 0, 37, 0, 0, 4);
        h.tick(1);
        let frames = h.frames_for(1);
        let confirms: Vec<_> = tags(&frames, proto::S_CONFIRM).iter().map(|b| proto::Confirm::decode(b).unwrap()).collect();
        assert_eq!(confirms.len(), 4);
        assert!(confirms.iter().enumerate().all(|(i, c)| c.accepted && c.window == 0 && c.action == i as u16 + 1));
        assert!(tags(&frames, proto::S_SLOTS).is_empty(), "the viewer predicted all of it");
        assert_eq!(main(&h, 1), st(block::PLANKS, 4));
        assert!(main(&h, 0).is_empty());
    });
}

#[test]
fn a_click_on_a_window_that_is_not_open_is_refused_and_the_contents_come_back() {
    run(|| {
        let mut h = setup();
        give(&mut h, 0, st(block::COBBLESTONE, 5));
        h.tick(1);
        h.frames_for(1);
        send_click(&mut h, 7, 36, 0, 0, 9);
        h.tick(1);
        let frames = h.frames_for(1);
        let c = proto::Confirm::decode(tags(&frames, proto::S_CONFIRM)[0]).unwrap();
        assert_eq!((c.window, c.action, c.accepted), (7, 9, false));
        assert_eq!(proto::WindowSlots::decode(tags(&frames, proto::S_SLOTS)[0]).unwrap().slots.len(), 47);
        assert_eq!(main(&h, 0), st(block::COBBLESTONE, 5));
    });
}

#[test]
fn a_click_after_a_change_the_viewer_missed_sends_the_whole_window() {
    run(|| {
        let mut h = setup();
        // The change is made after the tick, so the viewer has not been told yet.
        give(&mut h, 0, st(block::COBBLESTONE, 5));
        send_click(&mut h, 0, 36, 0, 0, 1);
        h.tick(1);
        let frames = h.frames_for(1);
        assert!(tags(&frames, proto::S_CONFIRM).len() == 1);
        let full = proto::WindowSlots::decode(tags(&frames, proto::S_SLOTS)[0]).unwrap();
        assert_eq!(full.slots.len(), 47);
        assert_eq!(full.slots[46], (-1, crate::game::items::wire_stack(&st(block::COBBLESTONE, 5))));
    });
}

#[test]
fn closing_the_table_gives_the_grid_back_and_throws_the_cursor() {
    run(|| {
        let mut h = setup();
        give(&mut h, 0, st(block::LOG, 2));
        give(&mut h, 1, st(block::COBBLESTONE, 9));
        h.tick(1);
        open_table(&mut h);
        // hotbar slot 37 is workbench slot 37; the log goes to grid slot 1, the cobble to the cursor
        send_click(&mut h, 1, 37, 0, 0, 1);
        send_click(&mut h, 1, 1, 0, 0, 2);
        send_click(&mut h, 1, 38, 0, 0, 3);
        h.tick(1);
        h.frames_for(1);
        assert!(main(&h, 0).is_empty());
        h.push_frame(1, proto::V_CLOSE, &proto::CloseWindow { window: 1 }.encode());
        h.tick(1);
        let frames = h.frames_for(1);
        assert!(tags(&frames, proto::S_CLOSE_WINDOW).is_empty(), "the viewer closed it");
        let back = proto::WindowSlots::decode(tags(&frames, proto::S_SLOTS).last().unwrap()).unwrap();
        assert_eq!(back.window, 0);
        assert_eq!(main(&h, 0), st(block::LOG, 2));
        assert!(h.game.windows.players[&1].open.is_none());
        let thrown: Vec<_> = h.game.items.entities.iter().map(|e| e.stack).collect();
        assert_eq!(thrown, vec![st(block::COBBLESTONE, 9)]);
        assert!(h.game.players.players[&1].inv.cursor.is_empty());
    });
}

#[test]
fn walking_away_from_the_table_closes_it() {
    run(|| {
        let mut h = setup();
        open_table(&mut h);
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5 + 12.0], ..Default::default() });
        h.tick(1);
        let frames = h.frames_for(1);
        assert_eq!(proto::CloseWindow::decode(tags(&frames, proto::S_CLOSE_WINDOW)[0]).unwrap().window, 1);
        assert!(h.game.windows.players[&1].open.is_none());
    });
}

#[test]
fn breaking_the_table_closes_it() {
    run(|| {
        let mut h = setup();
        open_table(&mut h);
        terrain::set_block(&mut h.game, (TABLE_AT[0], TABLE_AT[1], TABLE_AT[2]), 0);
        h.tick(1);
        assert!(tags(&h.frames_for(1), proto::S_CLOSE_WINDOW).len() == 1);
    });
}

#[test]
fn creative_slots_work_for_creative_players_only() {
    run(|| {
        let mut h = setup();
        let sword = crate::game::items::wire_stack(&ItemStack::of(item::IRON_SWORD));
        let put = proto::CreativeSlot { slot: 36, stack: sword };
        h.push_frame(1, proto::V_CREATIVE_SLOT, &put.encode());
        h.tick(1);
        assert!(main(&h, 0).is_empty(), "survival is refused");
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        h.tick(1);
        h.frames_for(1);
        h.push_frame(1, proto::V_CREATIVE_SLOT, &put.encode());
        h.tick(1);
        assert_eq!(main(&h, 0), ItemStack::of(item::IRON_SWORD));
        let frames = h.frames_for(1);
        let diff = proto::WindowSlots::decode(tags(&frames, proto::S_SLOTS)[0]).unwrap();
        assert_eq!(diff.slots, vec![(36, sword)]);
        // throwing: 20 a stack, none after 200, and one back every tick
        let throw = proto::CreativeSlot { slot: -1, stack: sword };
        for _ in 0..11 {
            h.push_frame(1, proto::V_CREATIVE_SLOT, &throw.encode());
        }
        h.tick(1);
        assert_eq!(h.game.items.entities.len(), 10);
        assert!(h.game.items.entities.iter().all(|e| e.age > 4000), "creative despawn time");
    });
}

#[test]
fn the_selected_slot_and_swapped_hands_reach_the_others_as_equipment() {
    run(|| {
        let mut h = setup();
        h.join(2, "Ab", [6.5, 201.0, 5.5]);
        h.tick(1);
        h.frames_for(2);
        give(&mut h, 2, ItemStack::of(item::IRON_SWORD));
        h.tick(1);
        assert!(tags(&h.frames_for(2), proto::S_EQUIPMENT).is_empty(), "not selected yet");
        h.push_frame(1, proto::V_HELD, &proto::HeldSlot { slot: 2 }.encode());
        h.tick(1);
        let eq = proto::Equipment::decode(tags(&h.frames_for(2), proto::S_EQUIPMENT)[0]).unwrap();
        assert_eq!((eq.id, eq.slot, eq.stack.id), (1, proto::EQ_MAIN_HAND, item::IRON_SWORD as i16));
        assert!(tags(&h.frames_for(1), proto::S_EQUIPMENT).is_empty(), "not to oneself");
        h.push_frame(1, proto::V_SWAP_HANDS, &[]);
        h.tick(1);
        let frames = h.frames_for(2);
        let eqs: Vec<_> = tags(&frames, proto::S_EQUIPMENT).iter().map(|b| proto::Equipment::decode(b).unwrap()).collect();
        assert!(eqs.iter().any(|e| e.slot == proto::EQ_MAIN_HAND && e.stack.id == -1));
        assert!(eqs.iter().any(|e| e.slot == proto::EQ_OFF_HAND && e.stack.id == item::IRON_SWORD as i16));
        assert_eq!(h.game.players.players[&1].inv.off_hand[0], ItemStack::of(item::IRON_SWORD));
        // a newcomer is told what is already worn
        h.join(3, "Cd", [6.5, 201.0, 6.5]);
        h.tick(1);
        let late = h.frames_for(3);
        assert!(tags(&late, proto::S_EQUIPMENT).iter().any(|b| proto::Equipment::decode(b).unwrap().slot == proto::EQ_OFF_HAND));
    });
}

#[test]
fn what_the_clicks_arranged_comes_back_after_a_rejoin() {
    run(|| {
        let mut h = setup();
        give(&mut h, 0, st(block::COBBLESTONE, 12));
        give(&mut h, 1, ItemStack::of(item::IRON_SWORD));
        h.tick(1);
        // cobble to the cursor and into main slot 9 (window slot 9), sword to the armor-free slot 20
        send_click(&mut h, 0, 36, 0, 0, 1);
        send_click(&mut h, 0, 9, 1, 0, 2);
        send_click(&mut h, 0, 9, 0, 0, 3);
        send_click(&mut h, 0, 37, 0, 0, 4);
        send_click(&mut h, 0, 20, 0, 0, 5);
        h.tick(1);
        let before: Vec<ItemStack> = (0..41).map(|i| h.game.players.players[&1].inv.get_stack_in_slot(i)).collect();
        assert_eq!(before[20], ItemStack::of(item::IRON_SWORD));
        assert_eq!(before[9], st(block::COBBLESTONE, 12));
        h.leave(1);
        h.tick(1);
        h.join(1, "Xy", [5.5, 201.0, 5.5]);
        h.tick(1);
        let after: Vec<ItemStack> = (0..41).map(|i| h.game.players.players[&1].inv.get_stack_in_slot(i)).collect();
        assert_eq!(before, after);
    });
}

#[test]
fn leaving_with_a_stack_on_the_cursor_puts_it_on_the_ground() {
    run(|| {
        let mut h = setup();
        give(&mut h, 0, st(block::COBBLESTONE, 4));
        h.tick(1);
        send_click(&mut h, 0, 36, 0, 0, 1);
        h.tick(1);
        h.leave(1);
        h.tick(1);
        assert_eq!(h.game.items.entities.len(), 1);
        assert_eq!(h.game.items.entities[0].stack, st(block::COBBLESTONE, 4));
        assert!(h.game.windows.players.is_empty());
    });
}

/// The viewer's copy, fed the same clicks, ends where the server's window ends, and the server
/// sends no slot frame for any of it.
#[test]
fn a_random_click_fuzz_keeps_the_predicting_copy_and_the_server_equal() {
    run(|| {
        for seed in 0..3i64 {
            let mut h = setup();
            let mut r = JavaRandom::new(seed + 11);
            let kinds = [block::LOG, block::PLANKS, item::STICK, block::COBBLESTONE, item::WOODEN_PICKAXE, item::IRON_HELMET];
            for i in 0..36 {
                if r.next_int_bounded(3) == 0 {
                    let id = kinds[r.next_int_bounded(kinds.len() as i32) as usize];
                    let n = if id == item::WOODEN_PICKAXE || id == item::IRON_HELMET { 1 } else { 1 + r.next_int_bounded(40) };
                    give(&mut h, i, st(id, n));
                }
            }
            h.tick(1);
            h.frames_for(1);
            let mut c = Container::player(recipes());
            let mut inv: InventoryPlayer = h.game.players.players[&1].inv.clone();
            let mut action = 0u16;
            for _ in 0..400 {
                let (slot, button, mode) = match r.next_int_bounded(8) {
                    0 | 1 => (r.next_int_bounded(46) as i16, r.next_int_bounded(2) as u8, 0),
                    2 => (r.next_int_bounded(46) as i16, r.next_int_bounded(2) as u8, 1),
                    3 => (r.next_int_bounded(46) as i16, r.next_int_bounded(9) as u8, 2),
                    4 => (r.next_int_bounded(46) as i16, r.next_int_bounded(2) as u8, 4),
                    5 => (r.next_int_bounded(46) as i16, 0, 6),
                    6 => (-999, r.next_int_bounded(2) as u8, 0),
                    _ => (r.next_int_bounded(46) as i16, 0, 0),
                };
                action += 1;
                send_click(&mut h, 0, slot, button, mode, action);
                let mut p = PlayerRef::new(&mut inv, false);
                c.slot_click(slot as i32, button as i32, ClickType::from_ordinal(mode).unwrap(), &mut p);
                if action % 7 == 0 {
                    h.tick(1);
                }
            }
            h.tick(1);
            let frames = h.frames_for(1);
            assert_eq!(tags(&frames, proto::S_CONFIRM).len(), 400);
            assert!(tags(&frames, proto::S_SLOTS).is_empty(), "seed {seed}: the prediction held");
            let server = &h.game.players.players[&1].inv;
            for i in 0..41 {
                assert_eq!(server.get_stack_in_slot(i), inv.get_stack_in_slot(i), "seed {seed} slot {i}");
            }
            assert_eq!(server.cursor, inv.cursor, "seed {seed}");
            let (a, b) = (h.game.windows.players[&1].inv.crafting.as_ref().unwrap(), c.crafting.as_ref().unwrap());
            assert_eq!(a.craft_matrix.slots, b.craft_matrix.slots, "seed {seed}");
        }
    });
}
