//! Item entities (T5) over a seeded world: a stone slab at y 200 to 201 under x, z 4..=13, the
//! player at the west edge and the items at the east edge, 7 blocks apart.

use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND};

use crate::game::items as gi;
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;
const COBBLE: u16 = 4;
const FAR: [f64; 3] = [12.5, 204.0, 12.5];

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.tick(1);
    h.frames_for(1);
    h
}

fn cobble(n: i32) -> ItemStack {
    ItemStack::new(COBBLE, n, 0)
}

fn tags(frames: &[(u8, Vec<u8>)], tag: u8) -> Vec<&[u8]> {
    frames.iter().filter(|f| f.0 == tag).map(|f| f.1.as_slice()).collect()
}

fn held(h: &Harness) -> Vec<ItemStack> {
    h.game.players.players[&1].inv.main.iter().copied().filter(|s| !s.is_empty()).collect()
}

#[test]
fn a_dropped_item_comes_to_rest_on_the_stone() {
    run(|| {
        let mut h = setup();
        let id = gi::spawn_stack_at(&mut h.game, FAR, cobble(4), Some([0.0; 3]), 10);
        let mut rest = None;
        for n in 1..=200 {
            h.tick(1);
            let e = &h.game.items.entities[0];
            if e.body.on_ground && (e.body.pos.y - 201.0).abs() < 1e-9 && rest.is_none() {
                rest = Some(n);
            }
        }
        let n = rest.expect("the item rested");
        assert_eq!(h.game.items.id_at(0), id);
        println!("rested at tick {n}");
        assert!(n == 13, "rested at tick {n}");
        assert_eq!(h.game.items.entities[0].body.pos.y, 201.0);
    });
}

#[test]
fn stacks_of_forty_stay_apart_and_twenty_with_forty_merge() {
    run(|| {
        let mut h = setup();
        gi::spawn_stack_at(&mut h.game, FAR, cobble(40), Some([0.0; 3]), 10);
        gi::spawn_stack_at(&mut h.game, FAR, cobble(40), Some([0.0; 3]), 10);
        h.tick(60);
        assert_eq!(h.game.items.entities.len(), 2);
        let mut h = setup();
        gi::spawn_stack_at(&mut h.game, FAR, cobble(20), Some([0.0; 3]), 10);
        gi::spawn_stack_at(&mut h.game, FAR, cobble(40), Some([0.0; 3]), 10);
        h.tick(60);
        assert_eq!(h.game.items.entities.len(), 1);
        assert_eq!(h.game.items.entities[0].stack.count, 60);
    });
}

#[test]
fn pickup_waits_for_the_delay_then_fills_the_inventory() {
    run(|| {
        let mut h = setup();
        let id = gi::spawn_stack_at(&mut h.game, [5.5, 201.0, 5.5], cobble(7), Some([0.0; 3]), 10);
        h.tick(10);
        assert_eq!(h.game.items.entities.len(), 1, "still on the ground during the delay");
        assert!(held(&h).is_empty());
        h.frames_for(1);
        h.tick(1);
        assert!(h.game.items.entities.is_empty());
        assert_eq!(held(&h), vec![cobble(7)]);
        let frames = h.frames_for(1);
        let c = proto::Collect::decode(tags(&frames, proto::S_COLLECT)[0]).unwrap();
        assert_eq!((c.item, c.collector, c.count), (id, 1, 7));
        assert!(!tags(&frames, proto::S_SLOTS).is_empty());
        // The tracker runs after the items in the same tick and tells the viewer it is gone.
        let gone = proto::decode_destroy(tags(&frames, proto::S_DESTROY)[0]).unwrap();
        assert_eq!(gone, vec![id]);
    });
}

#[test]
fn picking_up_into_an_existing_stack_sends_the_collect_that_sounds() {
    run(|| {
        let mut h = setup();
        h.game.players.players.get_mut(&1).unwrap().inv.set_inventory_slot_contents(0, cobble(5));
        let id = gi::spawn_stack_at(&mut h.game, [5.5, 201.0, 5.5], cobble(7), Some([0.0; 3]), 0);
        h.frames_for(1);
        h.tick(1);
        assert_eq!(held(&h), vec![cobble(12)]);
        // EntityLivingBase.onItemPickup: the viewer sounds ENTITY_ITEM_PICKUP on this frame, merged
        // into a slot or not.
        let frames = h.frames_for(1);
        let c = proto::Collect::decode(tags(&frames, proto::S_COLLECT)[0]).unwrap();
        assert_eq!((c.item, c.collector, c.count), (id, 1, 7));
    });
}

#[test]
fn a_full_inventory_leaves_the_item() {
    run(|| {
        let mut h = setup();
        for i in 0..36 {
            h.game.players.players.get_mut(&1).unwrap().inv.set_inventory_slot_contents(i, ItemStack::new(1, 64, 0));
        }
        gi::spawn_stack_at(&mut h.game, [5.5, 201.0, 5.5], cobble(7), Some([0.0; 3]), 0);
        h.tick(40);
        assert_eq!(h.game.items.entities.len(), 1);
        assert_eq!(h.game.items.entities[0].stack, cobble(7));
    });
}

#[test]
fn a_block_drop_is_not_collected_before_ten_ticks() {
    run(|| {
        let mut h = setup();
        // Block.spawnAsEntity sets the default pickup delay (10) on the dropped item.
        let id = gi::spawn_as_entity(&mut h.game, (5, 201, 5), cobble(3)).expect("doTileDrops");
        h.tick(10);
        assert_eq!(h.game.items.entities.len(), 1, "still on the ground at tick 10");
        assert_eq!(h.game.items.id_at(0), id);
        assert!(held(&h).is_empty());
        h.frames_for(1);
        h.tick(1);
        assert!(h.game.items.entities.is_empty(), "collected on the tick after the delay");
        assert_eq!(held(&h), vec![cobble(3)]);
    });
}

#[test]
fn an_item_two_blocks_away_is_never_collected_nor_pulled() {
    run(|| {
        let mut h = setup();
        // Two blocks east of the player at (5.5, 201, 5.5): outside grow(1.0, 0.5, 1.0). An item
        // past that grown box is never collected and is never pulled toward the player.
        gi::spawn_stack_at(&mut h.game, [7.5, 201.0, 5.5], cobble(3), Some([0.0; 3]), 0);
        h.tick(60);
        assert_eq!(h.game.items.entities.len(), 1, "never collected");
        assert!(held(&h).is_empty());
        let e = &h.game.items.entities[0];
        assert!((e.body.pos.x - 7.5).abs() < 1e-9, "no magnet: {}", e.body.pos.x);
        assert!(tags(&h.frames_for(1), proto::S_COLLECT).is_empty());
    });
}

#[test]
fn a_thrown_item_waits_forty_ticks_before_it_can_be_picked_up() {
    run(|| {
        let mut h = setup();
        // Look straight down so the throw lands on the player and stays in reach.
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], pitch: 90.0, flags: ON_GROUND, ..Default::default() });
        h.game.players.players.get_mut(&1).unwrap().inv.set_inventory_slot_contents(0, cobble(1));
        h.frames_for(1);
        h.push_frame(1, proto::V_DROP, &proto::DropItem { all: false }.encode());
        h.tick(1);
        assert_eq!(h.game.items.entities.len(), 1);
        // EntityPlayer.dropItem: setPickupDelay(40).
        assert_eq!(h.game.items.entities[0].pickup_delay, 39, "40 ticks, one spent");
        h.tick(39);
        assert_eq!(h.game.items.entities.len(), 1, "still on the ground at 40 ticks");
        assert!(held(&h).is_empty());
        h.tick(1);
        assert!(h.game.items.entities.is_empty(), "collected on the tick after 40");
        assert_eq!(held(&h), vec![cobble(1)]);
    });
}

#[test]
fn an_item_despawns_at_age_6000() {
    run(|| {
        let mut h = setup();
        gi::spawn_stack_at(&mut h.game, FAR, cobble(1), Some([0.0; 3]), 10);
        h.tick(5999);
        assert_eq!(h.game.items.entities.len(), 1);
        h.frames_for(1);
        h.tick(2);
        assert!(h.game.items.entities.is_empty());
        assert!(!tags(&h.frames_for(1), proto::S_DESTROY).is_empty());
    });
}

#[test]
fn v_drop_throws_one_or_the_whole_stack() {
    run(|| {
        let mut h = setup();
        let inv = &mut h.game.players.players.get_mut(&1).unwrap().inv;
        inv.set_inventory_slot_contents(0, ItemStack::new(280, 5, 0));
        h.frames_for(1);
        h.push_frame(1, proto::V_DROP, &proto::DropItem { all: false }.encode());
        h.tick(1);
        assert_eq!(h.game.players.players[&1].inv.main[0].count, 4);
        assert_eq!(h.game.items.entities.len(), 1);
        let e = &h.game.items.entities[0];
        assert_eq!((e.stack.item, e.stack.count, e.pickup_delay), (280, 1, 39), "40 ticks, one spent");
        assert_eq!(e.thrower.as_deref(), Some("Xy"));
        let frames = h.frames_for(1);
        assert_eq!(tags(&frames, proto::S_SPAWN_OBJECT).len(), 1);
        h.push_frame(1, proto::V_DROP, &proto::DropItem { all: true }.encode());
        h.tick(1);
        assert!(h.game.players.players[&1].inv.main[0].is_empty());
        // The thrown four land beside the thrown one, and merge with it within a few ticks.
        h.tick(30);
        let total: i32 = h.game.items.entities.iter().map(|e| e.stack.count).sum();
        assert_eq!(total, 5);
    });
}

#[test]
fn items_save_and_load() {
    run(|| {
        let mut h = setup();
        gi::spawn_stack_at(&mut h.game, FAR, cobble(9), Some([0.0; 3]), 10);
        h.tick(30);
        h.game.items.entities[0].owner = Some("Xy".into());
        let (age, pos) = (h.game.items.entities[0].age, h.game.items.entities[0].body.pos);
        let v = gi::save(&h.game);
        let text = serde_json::to_string(&v).unwrap();
        let mut h2 = Harness::new(7, 2);
        gi::load(&mut h2.game, serde_json::from_str(&text).unwrap());
        assert_eq!(h2.game.items.entities.len(), 1);
        let e = &h2.game.items.entities[0];
        assert_eq!((e.stack, e.age, e.body.pos, e.owner.as_deref()), (cobble(9), age, pos, Some("Xy")));
        assert!(h2.game.items.id_at(0) >= 1_000_000);
    });
}

#[test]
fn the_tracker_sends_spawn_move_and_destroy() {
    run(|| {
        let mut h = setup();
        let id = gi::spawn_stack_at(&mut h.game, FAR, cobble(3), Some([0.0; 3]), 10);
        h.tick(1);
        let frames = h.frames_for(1);
        let spawn = proto::SpawnObject::decode(tags(&frames, proto::S_SPAWN_OBJECT)[0]).unwrap();
        assert_eq!(spawn.id, id);
        assert!(matches!(spawn.data, proto::ObjectData::Item(w) if w.count == 3 && w.id == COBBLE as i16));
        assert_eq!(tags(&frames, proto::S_VELOCITY).len(), 1);
        // Falling 3 blocks takes over 20 ticks, so a move goes out at the interval.
        h.tick(25);
        let frames = h.frames_for(1);
        let moves: Vec<_> = tags(&frames, proto::S_ENTITY_MOVE).iter().flat_map(|b| proto::decode_entity_moves(b).unwrap()).collect();
        assert!(moves.iter().any(|m| m.id == id));
        // A merge changes the survivor's stack: metadata to the viewer, destroy for the absorbed.
        gi::spawn_stack_at(&mut h.game, FAR, cobble(2), Some([0.0; 3]), 10);
        h.tick(60);
        let frames = h.frames_for(1);
        assert!(!tags(&frames, proto::S_ENTITY_META).is_empty());
        assert!(!tags(&frames, proto::S_DESTROY).is_empty());
    });
}
