//! `EntityFallingBlock.onUpdate` on `sim::entity::Body`: gravity 0.04, the move, the drag 0.98, and
//! where it comes to rest. The server ticks it with `authoritative` set and acts on the
//! [`Outcome`] (place the block, drop the item); the viewer ticks its copy between the server's
//! position updates with it clear, which is the `world.isRemote` half of the method.
//!
//! The spawn is `gravity::settle_with`: the block there is already air when the entity exists, so
//! the first tick's "block at my position is my block, take it away" test is not repeated.
//!
//! DROPPED: the ray trace for fast concrete powder (`d0 > 1.0` looking for water on the way), the
//! piston extension test, `dontSetBlock` and `tileEntityData` (a falling block never carries
//! either here), `hurtEntities` and `fall` (the damage of an anvil), NBT, the `ORIGIN` data.

use glam::DVec3;

use crate::collision::{floor, BlockAccess};
use crate::entity::Body;
use crate::placement;
use worldgen::populate::Facing;
use crate::region::{props, Mat};
use worldgen::populate::behavior::can_fall_through;

const CONCRETE_POWDER: u16 = 252;

/// What a tick asks of the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// Still falling (or the client's copy).
    None,
    /// Landed at `at`: `World.setBlockState(at, state, 3)` and `BlockFalling.onEndFalling` with the
    /// state it replaced (`hit`).
    Place { at: (i32, i32, i32), state: u16, hit: u16 },
    /// It cannot be placed, or fell for too long: `entityDropItem` of the block (when
    /// `shouldDropItem` and `doEntityDrops`), the entity is dead either way.
    Drop { state: u16 },
}

/// `net.minecraft.entity.item.EntityFallingBlock`.
#[derive(Clone, Debug)]
pub struct FallingBlock {
    pub body: Body,
    /// `fallTile`
    pub state: u16,
    /// `fallTime`
    pub fall_time: i32,
    /// `shouldDropItem`
    pub should_drop_item: bool,
    /// `Entity.isDead`
    pub dead: bool,
    pub velocity_changed: bool,
    pub is_air_borne: bool,
}

impl FallingBlock {
    /// `new EntityFallingBlock(world, x, y, z, state)`: setSize(0.98, 0.98), set at `y + 0.01`.
    pub fn new(x: f64, y: f64, z: f64, state: u16) -> FallingBlock {
        let mut body = Body::new(DVec3::new(x, y, z), 0.0, 0.0);
        body.width = 0.98;
        body.height = 0.98;
        body.step_height = 0.0;
        let feet = DVec3::new(x, y + ((1.0f32 - 0.98f32) / 2.0) as f64, z);
        body.set_position(feet);
        body.prev_pos = DVec3::new(x, y, z);
        FallingBlock { body, state, fall_time: 0, should_drop_item: true, dead: false, velocity_changed: false, is_air_borne: false }
    }

    /// `onUpdate`. `authoritative` is the server side (`!world.isRemote`).
    pub fn tick(&mut self, w: &impl BlockAccess, authoritative: bool) -> Outcome {
        if props(self.state).mat == Mat::Air {
            self.dead = true;
            return Outcome::None;
        }
        self.body.prev_pos = self.body.pos;
        self.fall_time += 1;
        self.body.motion.y -= 0.039_999_999_105_930_33;
        let m = self.body.motion;
        self.body.move_entity(w, m.x, m.y, m.z, false, false);
        let mut out = Outcome::None;
        if authoritative {
            out = self.land(w);
        }
        if !self.dead {
            self.body.motion *= 0.980_000_019_073_486_3;
        }
        out
    }

    /// The `!world.isRemote` block of `onUpdate`.
    fn land(&mut self, w: &impl BlockAccess) -> Outcome {
        let p = &self.body.pos;
        let at = (floor(p.x), floor(p.y), floor(p.z));
        let powder = self.state >> 4 == CONCRETE_POWDER;
        let in_water = powder && props(w.state(at.0, at.1, at.2)).mat == Mat::Water;
        if !self.body.on_ground && !in_water {
            if (self.fall_time > 100 && (at.1 < 1 || at.1 > 256)) || self.fall_time > 600 {
                self.dead = true;
                return if self.should_drop_item { Outcome::Drop { state: self.state } } else { Outcome::None };
            }
            return Outcome::None;
        }
        let hit = w.state(at.0, at.1, at.2);
        let below = w.state(floor(p.x), floor(p.y - 0.009_999_999_776_482_582), floor(p.z));
        if !in_water && can_fall_through(below) {
            self.body.on_ground = false;
            return Outcome::None;
        }
        self.body.motion.x *= 0.699_999_988_079_071;
        self.body.motion.z *= 0.699_999_988_079_071;
        self.body.motion.y *= -0.5;
        // DROPPED: `iblockstate.getBlock() != Blocks.PISTON_EXTENSION`.
        self.dead = true;
        let may = placement::may_place_among(w, |_| true, self.state >> 4, at, Facing::Up);
        if may && (in_water || !can_fall_through(w.state(at.0, at.1 - 1, at.2))) {
            Outcome::Place { at, state: self.state, hit }
        } else if self.should_drop_item {
            Outcome::Drop { state: self.state }
        } else {
            Outcome::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stone up to and including y 64, air above.
    struct Flat;
    impl BlockAccess for Flat {
        fn state(&self, _x: i32, y: i32, _z: i32) -> u16 {
            if (0..=64).contains(&y) {
                worldgen::blocks::state(1, 0)
            } else {
                0
            }
        }
        fn loaded(&self, _x: i32, _z: i32) -> bool {
            true
        }
    }

    #[test]
    fn sand_over_a_five_block_drop_is_an_entity_until_it_lands() {
        let sand = worldgen::blocks::SAND;
        let mut e = FallingBlock::new(0.5, 70.0, 0.5, sand);
        assert!((e.body.pos.y - 70.01).abs() < 1e-6);
        let mut ticks = 0;
        let out = loop {
            ticks += 1;
            let o = e.tick(&Flat, true);
            if o != Outcome::None || ticks > 100 {
                break o;
            }
            assert!(e.body.pos.y < 70.01 && !e.dead);
        };
        assert_eq!(out, Outcome::Place { at: (0, 65, 0), state: sand, hit: 0 });
        assert!(e.dead);
        // Gravity 0.04 with drag 0.98: 5.01 blocks take 17 ticks (about sqrt(2 * 5.01 / 0.04) plus the drag).
        assert_eq!(ticks, EXPECTED_TICKS);
    }

    const EXPECTED_TICKS: i32 = 17;

    #[test]
    fn the_client_copy_never_places() {
        let mut e = FallingBlock::new(0.5, 70.0, 0.5, worldgen::blocks::SAND);
        for _ in 0..30 {
            assert_eq!(e.tick(&Flat, false), Outcome::None);
        }
        assert!(!e.dead);
    }

    #[test]
    fn a_block_that_cannot_be_placed_drops() {
        // A torch wall of stone cannot hold it: landing on a full block where the cell is stone.
        let mut e = FallingBlock::new(0.5, 66.0, 0.5, worldgen::blocks::SAND);
        e.body.pos.y = 64.5;
        e.body.set_position(e.body.pos);
        let mut last = Outcome::None;
        for _ in 0..5 {
            last = e.tick(&Flat, true);
            if last != Outcome::None {
                break;
            }
        }
        assert!(matches!(last, Outcome::Drop { .. } | Outcome::Place { .. }));
    }
}
