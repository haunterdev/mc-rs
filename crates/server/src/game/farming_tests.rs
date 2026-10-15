//! Farming and tree growth: the hoe, seeds and bone meal through `V_USE_ON`, and the random ticks
//! of blockticks.rs driven directly over a seeded world. A stone slab at y 200 under x, z 4..=13,
//! the player at (5.5, 201, 5.5), the plots at y 201 around x, z 8..=12, noon.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND};

use crate::game::testing::{run, Harness};
use crate::game::{blockticks, mining, terrain};

const STONE: u16 = block::STONE << 4;
const DIRT: u16 = block::DIRT << 4;
const GRASS: u16 = block::GRASS << 4;
const FARMLAND: u16 = block::FARMLAND << 4;
const WHEAT: u16 = block::WHEAT << 4;
const SAPLING: u16 = block::SAPLING << 4;

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    terrain::world(&h.game).unwrap().lock().unwrap().set_time(6000);
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

fn click(h: &mut Harness, pos: [i32; 3], face: u8) {
    h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos, face, hit: [0.5; 3], hand: 0, state: 0 }.encode());
    h.tick(1);
}

fn at(h: &Harness, p: (i32, i32, i32)) -> u16 {
    terrain::state(&h.game, p)
}

/// `n` random ticks of the block at `p`.
fn tick_block(h: &mut Harness, p: (i32, i32, i32), n: usize) {
    blockticks::with_rand(&mut h.game, |g, r| {
        for _ in 0..n {
            blockticks::random_tick(g, p, r);
        }
    });
}

#[test]
fn a_hoe_tills_grass_to_farmland_and_wears() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), GRASS);
        give(&mut h, ItemStack::of(item::IRON_HOE));
        click(&mut h, [8, 201, 8], 1);
        assert_eq!(at(&h, (8, 201, 8)), FARMLAND);
        assert_eq!(held(&h).damage, 1);
        // stone is left alone, and the bottom face does nothing
        click(&mut h, [8, 200, 8], 1);
        assert_eq!(at(&h, (8, 200, 8)), STONE);
        terrain::set_block(&mut h.game, (9, 201, 8), GRASS);
        click(&mut h, [9, 201, 8], 0);
        assert_eq!(at(&h, (9, 201, 8)), GRASS);
    });
}

#[test]
fn seeds_plant_on_farmland_and_not_on_dirt() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), FARMLAND);
        terrain::set_block(&mut h.game, (10, 201, 8), DIRT);
        give(&mut h, ItemStack::new(item::WHEAT_SEEDS, 5, 0));
        click(&mut h, [10, 201, 8], 1);
        assert_eq!(at(&h, (10, 202, 8)), 0);
        assert_eq!(held(&h).count, 5);
        click(&mut h, [8, 201, 8], 1);
        assert_eq!(at(&h, (8, 202, 8)), WHEAT);
        assert_eq!(held(&h).count, 4);
    });
}

#[test]
fn wheat_reaches_age_seven_by_random_ticks() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), FARMLAND | 7);
        terrain::set_block(&mut h.game, (8, 202, 8), WHEAT);
        let mut ticks = 0;
        while at(&h, (8, 202, 8)) != WHEAT | 7 && ticks < 5000 {
            tick_block(&mut h, (8, 202, 8), 1);
            ticks += 1;
        }
        assert_eq!(at(&h, (8, 202, 8)), WHEAT | 7, "after {ticks} ticks");
        assert!(ticks > 7, "one stage per tick would be wrong: {ticks}");
    });
}

#[test]
fn bone_meal_advances_a_crop_and_is_used_up() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), FARMLAND | 7);
        terrain::set_block(&mut h.game, (8, 202, 8), WHEAT);
        give(&mut h, ItemStack::new(item::DYE, 3, 15));
        click(&mut h, [8, 202, 8], 1);
        let age = at(&h, (8, 202, 8)) & 15;
        assert!((2..=5).contains(&age), "age {age}");
        assert_eq!(held(&h).count, 2);
    });
}

#[test]
fn a_sapling_grows_into_a_tree() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (10, 201, 10), GRASS);
        terrain::set_block(&mut h.game, (10, 202, 10), SAPLING);
        let mut ticks = 0;
        while at(&h, (10, 202, 10)) >> 4 == block::SAPLING && ticks < 20000 {
            tick_block(&mut h, (10, 202, 10), 1);
            ticks += 1;
        }
        assert_eq!(at(&h, (10, 202, 10)) >> 4, block::LOG, "after {ticks} ticks");
        let mut logs = 0;
        let mut leaves = 0;
        for x in 6..=14 {
            for z in 6..=14 {
                for y in 202..=222 {
                    match at(&h, (x, y, z)) >> 4 {
                        block::LOG => logs += 1,
                        block::LEAVES => leaves += 1,
                        _ => {}
                    }
                }
            }
        }
        assert!(logs >= 3, "{logs} logs");
        assert!(leaves >= 10, "{leaves} leaves");
    });
}

#[test]
fn ripe_wheat_drops_wheat_and_seeds() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), FARMLAND | 7);
        // BlockCrops.getDrops: one wheat, then 3 tries of nextInt(14) <= 7 for a seed each, so a
        // single harvest drops no seeds (6/14)^3 = 8% of the time; 20 harvests make that ~0.
        let mut wheat = 0;
        let mut seeds = 0;
        for _ in 0..20 {
            terrain::set_block(&mut h.game, (8, 202, 8), WHEAT | 7);
            assert!(mining::try_harvest(&mut h.game, 1, (8, 202, 8), WHEAT | 7));
            for e in std::mem::take(&mut h.game.items.entities) {
                match e.stack.get_item() {
                    i if i == item::WHEAT => wheat += e.stack.get_count(),
                    i if i == item::WHEAT_SEEDS => seeds += e.stack.get_count(),
                    _ => {}
                }
            }
            h.game.items.meta.clear();
        }
        assert_eq!(wheat, 20);
        assert!((1..=60).contains(&seeds), "{seeds} seeds");
    });
}

#[test]
fn dry_farmland_reverts_to_dirt() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), FARMLAND | 2);
        tick_block(&mut h, (8, 201, 8), 2);
        assert_eq!(at(&h, (8, 201, 8)), FARMLAND);
        tick_block(&mut h, (8, 201, 8), 1);
        assert_eq!(at(&h, (8, 201, 8)), DIRT);
    });
}

#[test]
fn the_game_tick_draws_random_cells_by_the_rule() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), FARMLAND);
        h.tick(5);
        assert_eq!(at(&h, (8, 201, 8)), FARMLAND, "randomTickSpeed 0 ticks nothing");
        terrain::world(&h.game).unwrap().lock().unwrap().set_rule("randomTickSpeed", "4096");
        h.tick(10);
        assert_eq!(at(&h, (8, 201, 8)), DIRT, "dry farmland with no crop dries out once ticked");
    });
}

#[test]
fn watered_farmland_soaks_and_a_hard_landing_tramples_it() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 201, 8), FARMLAND);
        terrain::set_block(&mut h.game, (10, 201, 8), block::WATER << 4);
        tick_block(&mut h, (8, 201, 8), 1);
        assert_eq!(at(&h, (8, 201, 8)), FARMLAND | 7);
        blockticks::farmland_fallen_upon(&mut h.game, (8, 201, 8), 10.0);
        assert_eq!(at(&h, (8, 201, 8)), DIRT);
    });
}

/// With `fastLeafDecay`, a leaf that starts checking decay is gone within 10 ticks; with it
/// off nothing is scheduled.
#[test]
fn fast_leaf_decay_schedules_the_check_within_ten_ticks() {
    run(|| {
        let leaf = (block::LEAVES as u16) << 4;
        for fast in [true, false] {
            let mut h = setup();
            h.game.cfg.fast_leaf_decay = fast;
            terrain::set_block(&mut h.game, (10, 205, 10), leaf);
            terrain::set_block(&mut h.game, (11, 205, 10), leaf);
            blockticks::log_broken(&mut h.game, (10, 204, 10));
            assert_eq!(at(&h, (10, 205, 10)) & 8, 8, "the leaf starts checking");
            assert_eq!(h.game.blockticks.pending.len(), if fast { 2 } else { 0 });
            if fast {
                assert!(h.game.blockticks.pending.iter().all(|(t, _)| (2..=10).contains(&(t - h.game.blockticks.now))));
                h.tick(11);
                assert_eq!(at(&h, (10, 205, 10)) >> 4, 0, "decayed");
                assert_eq!(at(&h, (11, 205, 10)) >> 4, 0, "decayed");
                assert!(h.game.blockticks.pending.is_empty());
            }
        }
    });
}

/// A liquid placed on a flat floor runs `BlockDynamicLiquid.updateTick` by itself and
/// spreads 7 blocks (water) or 3 (lava, overworld).
#[test]
fn a_water_source_spreads_seven_blocks_and_lava_three() {
    run(|| {
        for (liquid, reach) in [(block::FLOWING_WATER, 7), (block::FLOWING_LAVA, 3)] {
            let mut h = setup();
            // A wide flat floor at y 200 (setup lays stone under x, z 4..=13; make room).
            for x in -12..=28 {
                for z in -12..=28 {
                    terrain::set_block(&mut h.game, (x, 200, z), (block::STONE as u16) << 4);
                }
            }
            terrain::set_block(&mut h.game, (8, 201, 8), (liquid as u16) << 4);
            h.tick(120);
            let mut far = 0;
            for d in 1..=12 {
                let b = at(&h, (8 + d, 201, 8)) >> 4;
                if b == block::FLOWING_WATER as u16 || b == block::WATER as u16 || b == block::FLOWING_LAVA as u16 || b == block::LAVA as u16 {
                    far = d;
                }
            }
            assert_eq!(far, reach, "liquid {liquid} reached {far}");
        }
    });
}
