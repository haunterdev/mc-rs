//! A falling block is an entity for the ticks it takes to come down, then it is placed, over
//! the slab world of xp_tests.rs (stone at y 200 under x, z 4..=13).

use ::items::ids::block;
use proto::{PlayerState, ON_GROUND};

use crate::game::testing::Harness;
use crate::game::terrain;

const STONE: u16 = block::STONE << 4;
const SAND: u16 = block::SAND << 4;

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.world.lock().unwrap().set_rule("doMobSpawning", "false");
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

#[test]
fn sand_over_a_five_block_drop_is_an_entity_for_seventeen_ticks_then_placed() {
    let mut h = setup();
    terrain::set_block(&mut h.game, (8, 206, 8), SAND);
    // The block is gone from where it was and nothing is at the bottom yet.
    assert_eq!(terrain::state(&h.game, (8, 201, 8)), 0);
    let mut ticks = 0;
    while !h.game.falling.entities.is_empty() || ticks == 0 {
        h.tick(1);
        ticks += 1;
        assert!(ticks < 60, "never landed");
        if !h.game.falling.entities.is_empty() {
            assert_eq!(terrain::state(&h.game, (8, 201, 8)), 0, "the block is not at the bottom while it falls");
            assert_eq!(terrain::state(&h.game, (8, 206, 8)), 0);
        }
    }
    assert_eq!(ticks, 17);
    assert_eq!(terrain::state(&h.game, (8, 201, 8)), SAND);
    // The tracker told the player: a spawn object for the block, then its destroy.
    let tags: Vec<u8> = h.frames_for(1).iter().map(|f| f.0).collect();
    assert!(tags.contains(&proto::S_SPAWN_OBJECT));
}

#[test]
fn a_block_that_lands_where_it_cannot_be_placed_drops_as_an_item() {
    let mut h = setup();
    terrain::set_block(&mut h.game, (9, 206, 9), SAND);
    // A stone block appears in the entity's way at the landing cell after it has spawned.
    h.tick(1);
    terrain::set_block(&mut h.game, (9, 201, 9), STONE);
    for _ in 0..30 {
        h.tick(1);
    }
    assert!(h.game.falling.entities.is_empty());
    assert_eq!(terrain::state(&h.game, (9, 202, 9)), SAND, "it lands on the new block");
}
