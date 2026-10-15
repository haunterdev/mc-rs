//! `ItemDoor.onItemUse` and `BlockDoor.onBlockActivated` through `V_USE_ON`, over a stone
//! slab at y 200 under x, z 4..=13 and the player at (5.5, 201, 5.5) looking south.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND};
use sim::door::{lower_meta, upper_meta};
use worldgen::blocks::{block_of, id, state};
use worldgen::populate::Facing;

use crate::game::testing::{run, Harness};
use crate::game::terrain;

const STONE: u16 = block::STONE << 4;

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

fn give(h: &mut Harness, s: ItemStack) {
    let p = h.game.players.players.get_mut(&1).unwrap();
    let c = p.inv.current_item;
    p.inv.main[c] = s;
}

fn held(h: &Harness) -> ItemStack {
    let p = &h.game.players.players[&1];
    p.inv.main[p.inv.current_item]
}

fn click(h: &mut Harness, pos: [i32; 3], face: u8, hit: [f32; 3]) {
    h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos, face, hit, hand: 0, state: 0 }.encode());
    h.tick(1);
}

#[test]
fn a_door_item_places_both_halves_and_is_used_up() {
    run(|| {
        let mut h = setup();
        give(&mut h, ItemStack::new(item::OAK_DOOR, 2, 0));
        click(&mut h, [8, 200, 8], 1, [0.5; 3]);
        // The player looks south (yaw 0): FACING south, hinge by the click (z 0.5 is not past it).
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), state(id::WOODEN_DOOR, lower_meta(Facing::South, false)));
        let upper = terrain::state(&h.game, (8, 202, 8));
        assert_eq!(block_of(upper), id::WOODEN_DOOR);
        assert_eq!(upper & 8, 8);
        assert_eq!(held(&h).count, 1);
        let frames = h.frames_for(1);
        assert!(frames.iter().any(|f| f.0 == proto::S_BLOCKS), "the viewer is sent the halves");
    });
}

#[test]
fn a_door_needs_a_top_face_and_room() {
    run(|| {
        let mut h = setup();
        give(&mut h, ItemStack::new(item::OAK_DOOR, 2, 0));
        // A side face: ItemDoor.onItemUse fails.
        click(&mut h, [8, 200, 8], 2, [0.5; 3]);
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), 0);
        assert_eq!(held(&h).count, 2);
        // No room above: a block in the upper cell.
        terrain::set_block(&mut h.game, (9, 202, 9), STONE);
        click(&mut h, [9, 200, 9], 1, [0.5; 3]);
        assert_eq!(terrain::state(&h.game, (9, 201, 9)), 0);
        assert_eq!(held(&h).count, 2);
        // Not on a block with a solid top (no floor): the cell beside a slab edge over air.
        click(&mut h, [13, 200, 13], 3, [0.5; 3]);
        assert_eq!(terrain::state(&h.game, (13, 201, 14)), 0);
    });
}

#[test]
fn right_clicking_a_wooden_door_swings_it_and_an_iron_door_stays() {
    run(|| {
        let mut h = setup();
        let lower = state(id::WOODEN_DOOR, lower_meta(Facing::South, false));
        terrain::set_block(&mut h.game, (8, 201, 8), lower);
        terrain::set_block(&mut h.game, (8, 202, 8), state(id::WOODEN_DOOR, upper_meta(false, false)));
        click(&mut h, [8, 202, 8], 2, [0.5; 3]);
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), lower | 4, "clicking the upper half opens the lower half");
        click(&mut h, [8, 201, 8], 2, [0.5; 3]);
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), lower, "and shuts it again");
        let iron = state(id::IRON_DOOR, lower_meta(Facing::South, false));
        terrain::set_block(&mut h.game, (10, 201, 10), iron);
        terrain::set_block(&mut h.game, (10, 202, 10), state(id::IRON_DOOR, upper_meta(false, false)));
        click(&mut h, [10, 201, 10], 2, [0.5; 3]);
        assert_eq!(terrain::state(&h.game, (10, 201, 10)), iron, "an iron door needs redstone");
    });
}

#[test]
fn a_second_door_beside_a_door_takes_the_left_hinge() {
    run(|| {
        let mut h = setup();
        give(&mut h, ItemStack::new(item::OAK_DOOR, 2, 0));
        click(&mut h, [8, 200, 8], 1, [0.5; 3]);
        click(&mut h, [9, 200, 8], 1, [0.5; 3]);
        let a = terrain::state(&h.game, (8, 202, 8));
        let b = terrain::state(&h.game, (9, 202, 8));
        assert_eq!(block_of(b), id::WOODEN_DOOR);
        // placeDoor: a door on the right side and none on the left forces the left hinge.
        assert_eq!(b & 1, 0, "left hinge beside a door on the right");
        assert_eq!(block_of(a), id::WOODEN_DOOR);
        assert_eq!(held(&h).count, 0);
    });
}
