//! Tests for the game's terrain, on the harness (testing.rs).

use super::*;
use crate::game::testing::{run, temp_dir, Harness};
use worldgen::blocks::{id, state as st};

/// Feet at the middle of chunk `c`, high in the air.
fn above(c: Pos) -> [f64; 3] {
    [c.0 as f64 * 16.0 + 8.5, 120.0, c.1 as f64 * 16.0 + 8.5]
}

#[test]
fn final_chunks_match_sim_terrain_alone() {
    run(|| {
        let (seed, sim) = (7, 2);
        let mut h = Harness::new(seed, sim);
        h.join(1, "a", above((3, -2)));
        h.settle();
        let order = plan(&[(3, -2)], sim);
        assert_eq!(order.len(), 13, "the circle of radius 2");
        // The same chunks from `Terrain` alone: generate what each needs, then ensure it, in order.
        let gen = ChunkGenerator::new(seed);
        let mut alone = Terrain::new(&gen);
        for &p in &order {
            for q in alone.needed(p) {
                alone.insert_generated(q, gen.generate(q.0, q.1), |_| false);
            }
            assert!(alone.ensure(p, |_| true));
        }
        let core = h.game.terrain.core.as_mut().unwrap();
        for &p in &order {
            let mine = core.terrain.final_chunk(p).expect("final in the game");
            let theirs = alone.final_chunk(p).unwrap();
            assert!(mine.blocks.data == theirs.blocks.data, "chunk {p:?} differs");
        }
        // `state` reads the same blocks.
        let p = order[0];
        let chunk = alone.final_chunk(p).unwrap();
        for y in [1, 40, 63, 64, 70, 90] {
            assert_eq!(state(&h.game, (p.0 * 16 + 5, y, p.1 * 16 + 9)), chunk.blocks.get(5, y, 9));
        }
        assert!(is_final(&h.game, p));
        assert!(!is_final(&h.game, (3 + 9, -2)), "out of range");
    });
}

#[test]
fn a_saved_edit_shows_in_state() {
    run(|| {
        let dir = temp_dir();
        let stone = st(id::STONE, 0);
        {
            let mut w = World::load(&dir).unwrap();
            w.apply(&[((5, 200, 5), stone)]);
            w.save();
        }
        let mut h = Harness::in_dir(1, 1, dir);
        assert_eq!(state(&h.game, (5, 200, 5)), worldgen::blocks::AIR, "air until the chunk is final");
        h.join(1, "a", above((0, 0)));
        h.settle();
        assert_eq!(state(&h.game, (5, 200, 5)), stone);
        assert_eq!(state(&h.game, (5, 201, 5)), worldgen::blocks::AIR);
    });
}

#[test]
fn set_block_reaches_every_session_and_edits_bin() {
    run(|| {
        let mut h = Harness::new(3, 1);
        h.join(1, "a", above((0, 0)));
        h.join(2, "b", above((40, 40)));
        h.settle();
        h.frames_for(1);
        h.frames_for(2);
        let glass = st(id::GLASS, 0);
        let changes = set_block(&mut h.game, (3, 210, 4), glass);
        assert_eq!(changes, vec![((3, 210, 4), glass)]);
        assert_eq!(state(&h.game, (3, 210, 4)), glass);
        for id in [1, 2] {
            let frames = h.frames_for(id);
            let blocks: Vec<_> = frames.iter().filter(|f| f.0 == proto::S_BLOCKS).flat_map(|f| proto::decode_blocks(&f.1)).collect();
            assert_eq!(blocks, changes, "session {id}");
        }
        let saved = World::load(&h.dir).unwrap().all_edits();
        assert!(saved.contains(&((3, 210, 4), glass)), "{saved:?}");
        // A chunk nobody generated is left alone.
        assert!(set_block(&mut h.game, (5000, 70, 5000), glass).is_empty());
    });
}

#[test]
fn light_is_cached_per_chunk_and_dropped_by_edits() {
    run(|| {
        // Radius 3, so chunk (1, 0) has its 3x3 final too.
        let mut h = Harness::new(5, 3);
        h.join(1, "a", above((0, 0)));
        h.settle();
        assert_eq!(light(&mut h.game, (8, 255, 8)), Some((15, 0)));
        assert_eq!(light(&mut h.game, (8, 1, 8)).map(|l| l.0), Some(0), "no sky under the ground");
        assert_eq!(light(&mut h.game, (8, 300, 8)), None);
        assert_eq!(light(&mut h.game, (16 * 40, 100, 0)), None, "not final");
        let cached = |h: &Harness, c: Pos| h.game.terrain.core.as_ref().unwrap().light.contains_key(&c);
        assert!(cached(&h, (0, 0)));
        light(&mut h.game, (20, 100, 8));
        assert!(cached(&h, (1, 0)));
        // Glowstone at x 30 drops chunks 0 to 2 in x; chunk (0, 0) (x 0..15) is within 15 blocks.
        // (A torch in the air would pop off at once.)
        assert_eq!(set_block(&mut h.game, (30, 200, 8), st(id::GLOWSTONE, 0)).len(), 1);
        assert!(!cached(&h, (0, 0)) && !cached(&h, (1, 0)));
        assert_eq!(light(&mut h.game, (29, 200, 8)).map(|l| l.1), Some(14));
    });
}

/// The resident chunk count for one player at the default sim distance.
#[test]
fn one_player_at_sim_distance_8() {
    run(|| {
        let mut h = Harness::new(11, 8);
        h.join(1, "a", above((0, 0)));
        h.settle();
        let order = plan(&[(0, 0)], 8);
        assert!(order.iter().all(|&p| is_final(&h.game, p)));
        let resident = h.game.stats.loaded_chunks;
        eprintln!("final chunks {}, resident chunks {resident}", order.len());
        assert!(resident >= order.len() && resident < 2 * order.len(), "{resident}");
    });
}

#[test]
fn chunks_out_of_range_pack_and_come_back() {
    run(|| {
        let mut h = Harness::new(9, 1);
        h.join(1, "a", above((0, 0)));
        h.settle();
        let before = state(&h.game, (8, 60, 8));
        h.move_to(1, above((30, 0)));
        h.settle();
        assert!(h.game.terrain.core.as_ref().unwrap().terrain.is_packed((0, 0)));
        assert_eq!(state(&h.game, (8, 60, 8)), worldgen::blocks::AIR, "packed reads as unloaded");
        h.move_to(1, above((0, 0)));
        h.settle();
        assert_eq!(state(&h.game, (8, 60, 8)), before);
    });
}
