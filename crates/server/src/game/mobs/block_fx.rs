//! The world writes a mob's goal asks for through an [`Effect`](super::ai::Effect): a block broken
//! with its neighbour updates, a harvest drop, and the break-progress animation. Applied by
//! `apply_effects` after the mob tick.

use crate::game::items as item_entities;
use crate::game::mining;
use crate::game::net;
use crate::game::terrain;
use crate::game::Game;

const AIR: u16 = 0;
/// `World.sendBlockBreakProgress` reaches this far (`mining::ANIM_RANGE`).
const ANIM_RANGE: f64 = 32.0;

/// `World.destroyBlock(pos, drop)` for a mob: the block goes with its neighbour updates, and the
/// drops `Block.dropBlockAsItem` would give when `drop` (the griefing goal).
pub fn destroy_block(game: &mut Game, at: (i32, i32, i32), state: u16, drop: bool) {
    let (changes, olds) = terrain::set_block_logged(game, at, AIR);
    if changes.is_empty() {
        return;
    }
    if drop {
        let drops = items::drops::drop_block_as_item(state, &mut game.mobs.rand);
        for stack in drops.stacks() {
            item_entities::spawn_as_entity(game, at, stack);
        }
        crate::game::xp_orbs::drop_xp_on_block_break(game, at, drops.xp);
    }
    // The block itself is the first change; what follows is the neighbour updates.
    mining::neighbour_drops(game, &changes[1..], &olds[1..]);
    // DROPPED: `playEvent(2001, pos, stateId)`, the break particles and sound.
}

/// `Block.harvestBlock(world, player, pos, state, te, stack)` with a player who has nothing
/// enchanted: the digging goal's own drop, run beside the destroy. The tool takes no damage (the
/// mod hands the fake player a copy of the held stack).
pub fn harvest(game: &mut Game, at: (i32, i32, i32), state: u16, stack: &items::ItemStack) {
    let h = items::harvest::harvest_block(state, stack, &mut game.mobs.rand);
    for s in h.drops.stacks() {
        item_entities::spawn_as_entity(game, at, s);
    }
    crate::game::xp_orbs::drop_xp_on_block_break(game, at, h.drops.xp);
    if h.ice_melts {
        terrain::set_block(game, at, items::ids::block::FLOWING_WATER << 4);
    }
    // DROPPED: `player.addStat` / `addExhaustion(0.005F)`.
}

/// `World.sendBlockBreakProgress(breaker, pos, stage)`: to the players within 32 blocks, the same
/// way `mining::anim` sends a player's.
pub fn send_progress(game: &Game, breaker: u32, at: (i32, i32, i32), stage: i32) {
    let msg = proto::BreakAnim { breaker, pos: [at.0, at.1, at.2], stage: stage.clamp(-1, 9) as i8 };
    net::send_near(game, Some(breaker), [at.0 as f64, at.1 as f64, at.2 as f64], ANIM_RANGE, proto::S_BREAK_ANIM, &msg.encode());
}
