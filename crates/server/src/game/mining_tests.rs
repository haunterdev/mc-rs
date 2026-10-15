//! Mining (T6) over a seeded world: a stone slab at y 200 under x, z 4..=13, the player on it at
//! (5.5, 201, 5.5) and the blocks to dig at y 201 within reach.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{Dig, PlayerState, DIG_FINISH, DIG_START, ON_GROUND};

use crate::game::terrain;
use crate::game::testing::{run, Harness};
use crate::game::GameMsg;

const STONE: u16 = block::STONE << 4;
const DIRT: u16 = block::DIRT << 4;
const GRASS: u16 = block::GRASS << 4;
const GLASS: u16 = block::GLASS << 4;
const FLOWER: u16 = block::RED_FLOWER << 4;
const AT: (i32, i32, i32) = (7, 201, 5);

fn setup(tool: u16) -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    if tool != 0 {
        h.game.players.players.get_mut(&1).unwrap().inv.main[0] = ItemStack::of(tool);
    }
    h.tick(1);
    h.frames_for(1);
    h
}

fn dig(h: &mut Harness, action: u8, state: u16) {
    let d = Dig { action, pos: [AT.0, AT.1, AT.2], face: 1, state };
    h.push_frame(1, proto::V_DIG, &d.encode());
}

/// Starts digging `state` at AT, holds for `ticks`, then sends the finish.
fn mine(h: &mut Harness, state: u16, ticks: u32) {
    terrain::set_block(&mut h.game, AT, state);
    h.tick(1);
    h.frames_for(1);
    dig(h, DIG_START, state);
    h.tick(1);
    h.tick(ticks - 1);
    dig(h, DIG_FINISH, state);
    h.tick(1);
}

fn dropped(h: &Harness) -> Vec<(u16, i32)> {
    h.game.items.entities.iter().map(|e| (e.stack.get_item(), e.stack.get_count())).collect()
}

#[test]
fn stone_with_a_wooden_pickaxe_drops_cobblestone_and_wears_the_tool() {
    run(|| {
        let mut h = setup(item::WOODEN_PICKAXE);
        mine(&mut h, STONE, 23);
        assert_eq!(terrain::state(&h.game, AT), 0);
        assert_eq!(dropped(&h), vec![(block::COBBLESTONE, 1)]);
        assert_eq!(h.game.players.players[&1].inv.main[0].get_metadata(), 1);
        assert!(h.game.players.players[&1].food.exhaustion > 0.0);
    });
}

#[test]
fn stone_by_hand_drops_nothing() {
    run(|| {
        let mut h = setup(0);
        mine(&mut h, STONE, 151);
        assert_eq!(terrain::state(&h.game, AT), 0);
        assert!(dropped(&h).is_empty());
    });
}

#[test]
fn grass_drops_dirt_and_glass_nothing() {
    run(|| {
        let mut h = setup(0);
        mine(&mut h, GRASS, 20);
        assert_eq!(dropped(&h), vec![(block::DIRT, 1)]);
        let mut h = setup(0);
        mine(&mut h, GLASS, 20);
        assert_eq!(terrain::state(&h.game, AT), 0);
        assert!(dropped(&h).is_empty());
    });
}

#[test]
fn a_flower_on_broken_dirt_drops_itself() {
    run(|| {
        let mut h = setup(0);
        terrain::set_block(&mut h.game, (AT.0, AT.1 + 1, AT.2), FLOWER);
        mine(&mut h, DIRT, 15);
        assert_eq!(terrain::state(&h.game, (AT.0, AT.1 + 1, AT.2)), 0);
        let mut got = dropped(&h);
        got.sort();
        assert_eq!(got, vec![(block::DIRT, 1), (block::RED_FLOWER, 1)]);
    });
}

#[test]
fn a_viewer_batch_that_breaks_a_flower_drops_it() {
    run(|| {
        let mut h = setup(0);
        let up = (AT.0, AT.1 + 1, AT.2);
        terrain::set_block(&mut h.game, AT, DIRT);
        terrain::set_block(&mut h.game, up, FLOWER);
        h.tick(1);
        h.send(GameMsg::Blocks { id: 1, changes: vec![(AT, 0), (up, 0)] });
        h.tick(1);
        assert_eq!(dropped(&h), vec![(block::RED_FLOWER, 1)]);
    });
}

#[test]
fn a_finish_at_half_time_is_refused_and_the_block_restored() {
    run(|| {
        let mut h = setup(item::WOODEN_PICKAXE);
        terrain::set_block(&mut h.game, AT, STONE);
        h.tick(1);
        h.frames_for(1);
        dig(&mut h, DIG_START, STONE);
        h.tick(1);
        h.tick(10);
        dig(&mut h, DIG_FINISH, STONE);
        h.tick(1);
        assert_eq!(terrain::state(&h.game, AT), STONE);
        assert!(dropped(&h).is_empty());
        let sent: Vec<_> = h.frames_for(1).into_iter().filter(|f| f.0 == proto::S_BLOCKS).flat_map(|f| proto::decode_blocks(&f.1)).collect();
        assert_eq!(sent, vec![(AT, STONE)]);
    });
}

#[test]
fn others_see_the_crack_progress() {
    run(|| {
        let mut h = setup(item::WOODEN_PICKAXE);
        h.join(2, "Ally", [6.5, 201.0, 6.5]);
        h.tick(1);
        h.frames_for(2);
        terrain::set_block(&mut h.game, AT, STONE);
        h.tick(1);
        dig(&mut h, DIG_START, STONE);
        h.tick(12);
        let anims: Vec<_> = h.frames_for(2).into_iter().filter(|f| f.0 == proto::S_BREAK_ANIM).map(|f| proto::BreakAnim::decode(&f.1).unwrap()).collect();
        assert!(anims.len() >= 2, "{anims:?}");
        assert!(anims.iter().all(|a| a.breaker == 1 && a.pos == [AT.0, AT.1, AT.2]));
        assert!(anims.last().unwrap().stage > anims[0].stage);
        assert!(h.frames_for(1).iter().all(|f| f.0 != proto::S_BREAK_ANIM));
    });
}

#[test]
fn creative_breaks_at_once_with_no_drops() {
    run(|| {
        let mut h = setup(0);
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        terrain::set_block(&mut h.game, AT, STONE);
        h.tick(1);
        dig(&mut h, DIG_START, STONE);
        h.tick(1);
        assert_eq!(terrain::state(&h.game, AT), 0);
        assert!(dropped(&h).is_empty());
    });
}

const LOG: u16 = block::LOG << 4;
const LEAVES: u16 = block::LEAVES << 4;

/// A stone floor at y 200 over an 11x11 area whose corner is (x0, z0); the player stands on it at
/// (x0 + 1.5, 201, z0 + 1.5), so a log at (x0, 201, z0) is within reach.
fn setup_at(x0: i32, z0: i32) -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [x0 as f64 + 1.5, 230.0, z0 as f64 + 1.5]);
    h.settle();
    for x in x0..=x0 + 10 {
        for z in z0..=z0 + 10 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [x0 as f64 + 1.5, 201.0, z0 as f64 + 1.5], flags: ON_GROUND, ..Default::default() });
    h.tick(1);
    h.frames_for(1);
    h
}

/// Digs `at` by hand with the V_DIG protocol: a start, the ticks the block takes, then a finish.
fn mine_at(h: &mut Harness, at: (i32, i32, i32)) {
    let state = terrain::state(&h.game, at);
    let ticks = sim::mining::break_ticks(state, &ItemStack::EMPTY, &::items::DigContext::default()).unwrap();
    let start = Dig { action: DIG_START, pos: [at.0, at.1, at.2], face: 1, state };
    h.push_frame(1, proto::V_DIG, &start.encode());
    h.tick(1);
    h.tick(ticks - 1);
    let finish = Dig { action: DIG_FINISH, pos: [at.0, at.1, at.2], face: 1, state };
    h.push_frame(1, proto::V_DIG, &finish.encode());
    h.tick(1);
}

/// The states in the 5x5x5 box around `c`, as `(offset, state)` pairs.
fn box_5(h: &Harness, c: (i32, i32, i32)) -> Vec<((i32, i32, i32), u16)> {
    let mut out = Vec::new();
    for dx in -2..=2 {
        for dy in -2..=2 {
            for dz in -2..=2 {
                out.push(((dx, dy, dz), terrain::state(&h.game, (c.0 + dx, c.1 + dy, c.2 + dz))));
            }
        }
    }
    out
}

/// Builds a log column at (x0, 201..=203, z0) with a leaf diamond at the top, so a break of one log
/// leaves another within four blocks and the leaves stay.
fn tree(h: &mut Harness, x0: i32, z0: i32) {
    for dy in 0..3 {
        terrain::set_block(&mut h.game, (x0, 201 + dy, z0), LOG);
    }
    for dx in -2i32..=2 {
        for dz in -2i32..=2 {
            if dx.abs() + dz.abs() <= 2 {
                terrain::set_block(&mut h.game, (x0 + dx, 203, z0 + dz), LEAVES);
            }
        }
    }
    h.tick(1);
    h.frames_for(1);
}

/// Breaking one block changes only that block, at negative and positive coordinates alike.
#[test]
fn breaking_a_log_changes_only_that_block() {
    run(|| {
        for &(x0, z0) in &[(8, 8), (-37, -5)] {
            let mut h = setup_at(x0, z0);
            h.game.cfg.fast_leaf_decay = true;
            tree(&mut h, x0, z0);
            let at = (x0, 202, z0);
            let before = box_5(&h, at);
            mine_at(&mut h, at);
            h.tick(40);
            assert_eq!(terrain::state(&h.game, at), 0, "({x0},{z0}): the log is gone");
            let after = box_5(&h, at);
            for ((off, old), (_, now)) in before.iter().zip(&after) {
                if *off == (0, 0, 0) {
                    continue;
                }
                assert_eq!(*old > 0, *now > 0, "({x0},{z0}): offset {off:?} changed from {old} to {now}");
            }
            let logs: Vec<_> = h.game.items.entities.iter().filter(|e| e.stack.get_item() == block::LOG).collect();
            assert_eq!(logs.len(), 1, "({x0},{z0}): one log dropped");
        }
    });
}

/// Breaking one leaf changes only that leaf, at negative and positive coordinates alike.
#[test]
fn breaking_a_leaf_changes_only_that_leaf() {
    run(|| {
        for &(x0, z0) in &[(8, 8), (-37, -5)] {
            let mut h = setup_at(x0, z0);
            tree(&mut h, x0, z0);
            let at = (x0 + 1, 203, z0);
            let before = box_5(&h, at);
            mine_at(&mut h, at);
            h.tick(40);
            assert_eq!(terrain::state(&h.game, at), 0, "({x0},{z0}): the leaf is gone");
            let after = box_5(&h, at);
            for ((off, old), (_, now)) in before.iter().zip(&after) {
                if *off == (0, 0, 0) {
                    continue;
                }
                assert_eq!(*old > 0, *now > 0, "({x0},{z0}): offset {off:?} changed from {old} to {now}");
            }
        }
    });
}
