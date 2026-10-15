//! T8 tests: chests and furnaces over a seeded world (a stone slab at y 200 under x, z 4..=13,
//! the players near (5.5, 201, 5.5), a chest at (7, 201, 6)).

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND};

use crate::game::testing::{run, Harness};
use crate::game::{terrain, tiles};

const STONE: u16 = block::STONE << 4;
const CHEST_AT: (i32, i32, i32) = (7, 201, 6);

fn setup(players: u32) -> Harness {
    let mut h = Harness::new(7, 2);
    for id in 1..=players {
        h.join(id, &format!("P{id}"), [5.5, 230.0, 5.5]);
    }
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    for id in 1..=players {
        h.game.sessions.get_mut(&id).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    }
    h.tick(1);
    for id in 1..=players {
        h.frames_for(id);
    }
    h
}

fn st(id: u16, n: i32) -> ItemStack {
    ItemStack::new(id, n, 0)
}

fn use_on(h: &mut Harness, id: u32, at: (i32, i32, i32)) {
    let m = proto::UseOn { pos: [at.0, at.1, at.2], face: 1, hit: [0.5; 3], hand: 0, state: 0 };
    h.push_frame(id, proto::V_USE_ON, &m.encode());
}

fn click(h: &mut Harness, id: u32, window: u8, slot: i16, mode: u8) {
    h.push_frame(id, proto::V_CLICK, &proto::Click { window, slot, button: 0, mode, action: 1 }.encode());
}

fn tagged(frames: &[(u8, Vec<u8>)], tag: u8) -> Vec<&[u8]> {
    frames.iter().filter(|f| f.0 == tag).map(|f| f.1.as_slice()).collect()
}

#[test]
fn eight_iron_ore_and_a_coal_give_eight_iron_in_1600_ticks() {
    run(|| {
        let mut h = setup(1);
        terrain::set_block(&mut h.game, CHEST_AT, block::FURNACE << 4 | 2);
        let f = h.game.tiles.furnaces.get_mut(&CHEST_AT).unwrap();
        f.set_inventory_slot_contents(0, st(block::IRON_ORE, 8));
        f.set_inventory_slot_contents(1, st(item::COAL, 1));
        h.tick(5);
        assert_eq!(terrain::state(&h.game, CHEST_AT) >> 4, block::LIT_FURNACE, "the furnace is lit");
        assert_eq!(terrain::state(&h.game, CHEST_AT) & 15, 2, "facing kept");
        h.tick(1700);
        let f = &h.game.tiles.furnaces[&CHEST_AT];
        assert_eq!(f.items[2], st(item::IRON_INGOT, 8));
        assert!(f.items[0].is_empty());
        assert_eq!(terrain::state(&h.game, CHEST_AT) >> 4, block::FURNACE, "the furnace went out");
    });
}

#[test]
fn the_lit_state_persists() {
    run(|| {
        let mut h = setup(1);
        terrain::set_block(&mut h.game, CHEST_AT, block::FURNACE << 4);
        let f = h.game.tiles.furnaces.get_mut(&CHEST_AT).unwrap();
        f.set_inventory_slot_contents(0, st(block::IRON_ORE, 2));
        f.set_inventory_slot_contents(1, st(item::COAL, 1));
        h.tick(50);
        assert_eq!(terrain::state(&h.game, CHEST_AT) >> 4, block::LIT_FURNACE);
        let saved = tiles::save(&h.game);
        let mut g = setup(1);
        tiles::load(&mut g.game, saved);
        let f = &g.game.tiles.furnaces[&CHEST_AT];
        assert!(f.is_burning());
        assert_eq!(f.cook_time, h.game.tiles.furnaces[&CHEST_AT].cook_time);
        assert_eq!(f.items[0], st(block::IRON_ORE, 2));
    });
}

#[test]
fn two_players_see_one_chest_change() {
    run(|| {
        let mut h = setup(2);
        terrain::set_block(&mut h.game, CHEST_AT, block::CHEST << 4 | 2);
        h.game.players.players.get_mut(&1).unwrap().inv.main[0] = st(block::COBBLESTONE, 5);
        for id in [1, 2] {
            use_on(&mut h, id, CHEST_AT);
        }
        h.tick(1);
        let f1 = h.frames_for(1);
        let f2 = h.frames_for(2);
        let open = proto::OpenWindow::decode(tagged(&f1, proto::S_OPEN_WINDOW)[0]).unwrap();
        assert_eq!((open.kind, open.rows), (proto::WK_CHEST, 3));
        let w1 = open.window;
        // The lid: the second viewer made it two.
        let lids: Vec<proto::BlockAction> = tagged(&f2, proto::S_BLOCK_ACTION).iter().filter_map(|b| proto::BlockAction::decode(b)).collect();
        assert_eq!(lids.last().map(|l| (l.a, l.b)), Some((1, 2)));
        // Player 1 shift-clicks the hotbar stack (window slot 27 + 27 = 54) into the chest.
        click(&mut h, 1, w1, 54, 1);
        h.tick(2);
        assert_eq!(h.game.tiles.chests[&CHEST_AT][0], st(block::COBBLESTONE, 5));
        let f2 = h.frames_for(2);
        let seen = tagged(&f2, proto::S_SLOTS)
            .iter()
            .filter_map(|b| proto::WindowSlots::decode(b))
            .any(|w| w.slots.iter().any(|(i, s)| *i == 0 && s.count == 5 && s.id == block::COBBLESTONE as i16));
        assert!(seen, "player 2 was sent the changed slot");
    });
}

#[test]
fn a_broken_chest_drops_everything() {
    run(|| {
        let mut h = setup(1);
        terrain::set_block(&mut h.game, CHEST_AT, block::CHEST << 4);
        let c = h.game.tiles.chests.get_mut(&CHEST_AT).unwrap();
        c[0] = st(block::COBBLESTONE, 64);
        c[13] = st(item::COAL, 7);
        terrain::set_block(&mut h.game, CHEST_AT, 0);
        assert!(!h.game.tiles.chests.contains_key(&CHEST_AT));
        let (mut stone, mut coal) = (0, 0);
        for e in &h.game.items.entities {
            match e.stack.item {
                i if i == block::COBBLESTONE => stone += e.stack.count,
                i if i == item::COAL => coal += e.stack.count,
                _ => {}
            }
        }
        assert_eq!((stone, coal), (64, 7));
    });
}

#[test]
fn a_double_chest_has_54_slots() {
    run(|| {
        let mut h = setup(1);
        terrain::set_block(&mut h.game, CHEST_AT, block::CHEST << 4);
        terrain::set_block(&mut h.game, (8, 201, 6), block::CHEST << 4);
        use_on(&mut h, 1, (8, 201, 6));
        h.tick(1);
        let f = h.frames_for(1);
        let open = proto::OpenWindow::decode(tagged(&f, proto::S_OPEN_WINDOW)[0]).unwrap();
        assert_eq!((open.rows, open.title.as_str()), (6, "container.chestDouble"));
        // The west chest is the first half.
        assert_eq!(open.pos, [7, 201, 6]);
        let all = tagged(&f, proto::S_SLOTS).iter().filter_map(|b| proto::WindowSlots::decode(b)).next().unwrap();
        // 54 chest slots, 36 inventory slots and the cursor.
        assert_eq!(all.slots.len(), 54 + 36 + 1);
    });
}

#[test]
fn a_restart_keeps_the_contents() {
    run(|| {
        let mut h = setup(1);
        terrain::set_block(&mut h.game, CHEST_AT, block::CHEST << 4);
        h.game.tiles.chests.get_mut(&CHEST_AT).unwrap()[5] = st(item::COAL, 33);
        let saved = tiles::save(&h.game);
        let text = serde_json::to_string(&saved).unwrap();
        let mut g = setup(1);
        tiles::load(&mut g.game, serde_json::from_str(&text).unwrap());
        assert_eq!(g.game.tiles.chests[&CHEST_AT][5], st(item::COAL, 33));
    });
}

#[test]
fn shift_click_sends_fuel_to_the_fuel_slot() {
    run(|| {
        let mut h = setup(1);
        terrain::set_block(&mut h.game, CHEST_AT, block::FURNACE << 4);
        h.game.players.players.get_mut(&1).unwrap().inv.main[0] = st(item::COAL, 4);
        use_on(&mut h, 1, CHEST_AT);
        h.tick(1);
        let f = h.frames_for(1);
        let open = proto::OpenWindow::decode(tagged(&f, proto::S_OPEN_WINDOW)[0]).unwrap();
        assert_eq!(open.kind, proto::WK_FURNACE);
        // Hotbar slot 0 is window slot 30.
        click(&mut h, 1, open.window, 30, 1);
        h.tick(1);
        assert_eq!(h.game.tiles.furnaces[&CHEST_AT].items[1], st(item::COAL, 4));
    });
}
