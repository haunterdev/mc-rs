//! `EntityFallingBlock` on the server: the spawn when `BlockFalling.checkFallable` finds nothing
//! below a block (`sim::gravity::settle_with` hands the blocks back through `Terrain::take_falls`),
//! the per-tick physics of `sim::falling_block`, the landing (`World.setBlockState` with
//! `BlockConcretePowder.onEndFalling`) or the item drop when the block cannot be placed, and the
//! `Tracked` view the entity tracker reads (range 160, interval 20, velocity on, from
//! `sim::kinds`).
//!
//! DROPPED: saving falling blocks with the world (a block mid-fall at shutdown is gone), the 2 tick
//! `tickRate` delay of `BlockFalling.updateTick` before the entity spawns.

use proto::{ObjectData, SpawnObject};
use sim::collision::{floor, BlockAccess};
use sim::falling_block::{FallingBlock, Outcome};
use sim::kinds::{Kind, FALLING_BLOCK};
use sim::region::{props, Mat};

use super::terrain::{self, Blocks};
use super::tracker::Tracked;
use super::{items as item_entities, net, Game};

const CONCRETE: u16 = 251;
const CONCRETE_POWDER: u16 = 252;

#[derive(Default)]
pub struct State {
    pub entities: Vec<FallingBlock>,
    /// Beside `entities`: the entity id.
    pub ids: Vec<u32>,
}

/// `World.spawnEntity(new EntityFallingBlock(world, x + 0.5, y, z + 0.5, state))`.
pub fn spawn(game: &mut Game, at: (i32, i32, i32), state: u16) -> u32 {
    let id = net::new_entity_id(game);
    game.falling.entities.push(FallingBlock::new(at.0 as f64 + 0.5, at.1 as f64, at.2 as f64 + 0.5, state));
    game.falling.ids.push(id);
    id
}

/// One tick: the blocks that started to fall become entities, then every entity moves and the
/// ones that landed are placed or dropped (`World.updateEntities`).
pub fn tick(game: &mut Game) {
    for f in terrain::take_falls(game) {
        spawn(game, f.at, f.state);
    }
    let mut list = std::mem::take(&mut game.falling.entities);
    let mut outcomes: Vec<(Outcome, [f64; 3])> = Vec::new();
    for e in list.iter_mut() {
        let blocks = Blocks(game);
        // Entities in unloaded chunks are not updated.
        if !blocks.loaded(floor(e.body.pos.x), floor(e.body.pos.z)) {
            continue;
        }
        let o = e.tick(&blocks, true);
        outcomes.push((o, [e.body.pos.x, e.body.pos.y, e.body.pos.z]));
    }
    let mut keep = list.iter().map(|e| !e.dead);
    game.falling.ids.retain(|_| keep.next().unwrap_or(true));
    list.retain(|e| !e.dead);
    game.falling.entities = list;
    for (o, pos) in outcomes {
        match o {
            Outcome::None => {}
            Outcome::Place { at, state, hit } => place(game, at, state, hit),
            Outcome::Drop { state } => drop_item(game, state, pos),
        }
    }
}

/// `world.setBlockState(pos, fallTile, 3)` and `BlockFalling.onEndFalling`.
fn place(game: &mut Game, at: (i32, i32, i32), state: u16, hit: u16) {
    terrain::set_block(game, at, state);
    // BlockConcretePowder.onEndFalling: landing in a liquid sets it.
    if state >> 4 == CONCRETE_POWDER && matches!(props(hit).mat, Mat::Water | Mat::Lava) {
        terrain::set_block(game, at, worldgen::blocks::state(CONCRETE, state & 15));
    }
}

/// `entityDropItem(new ItemStack(block, 1, damageDropped(state)), 0.0F)` while `doEntityDrops`.
fn drop_item(game: &mut Game, state: u16, pos: [f64; 3]) {
    if !terrain::rule(game, "doEntityDrops", true) {
        return;
    }
    let stack = items::ItemStack::new(state >> 4, 1, items::drop_rules::damage_dropped(state));
    item_entities::spawn_stack_at(game, pos, stack, None, 10);
}

/// What the tracker reads off one falling block.
pub struct View<'a> {
    pub id: u32,
    pub e: &'a FallingBlock,
}

pub fn views(state: &State) -> Vec<View<'_>> {
    state.entities.iter().enumerate().map(|(i, e)| View { id: state.ids[i], e }).collect()
}

impl Tracked for View<'_> {
    fn id(&self) -> u32 {
        self.id
    }
    fn kind(&self) -> &'static Kind {
        &FALLING_BLOCK
    }
    fn pos(&self) -> [f64; 3] {
        let p = self.e.body.pos;
        [p.x, p.y, p.z]
    }
    fn rotation(&self) -> [f32; 3] {
        [0.0, 0.0, 0.0]
    }
    fn on_ground(&self) -> bool {
        self.e.body.on_ground
    }
    fn motion(&self) -> [f64; 3] {
        let m = self.e.body.motion;
        [m.x, m.y, m.z]
    }
    fn spawn_body(&self) -> Vec<u8> {
        let p = self.e.body.pos;
        let m = self.e.body.motion;
        SpawnObject {
            id: self.id,
            pos: [p.x, p.y, p.z],
            yaw: 0.0,
            pitch: 0.0,
            motion: [m.x as f32, m.y as f32, m.z as f32],
            data: ObjectData::FallingBlock { state: self.e.state },
        }
        .encode()
    }
}

/// The tracker has run: nothing to clear (a falling block is never knocked).
pub fn after_tracker(_game: &mut Game) {}
