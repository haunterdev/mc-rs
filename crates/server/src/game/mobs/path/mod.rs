//! Mob pathfinding (T9f), ported 1:1 from 1.12.2 `net.minecraft.pathfinding` and the three
//! controls in `net.minecraft.entity.ai`.
//!
//! | File | Source |
//! |---|---|
//! | `point.rs` | `PathPoint`, `Path` |
//! | `heap.rs` | `PathHeap` |
//! | `node.rs` | `PathNodeType`, `NodeProcessor`, `NavEntity` (the `EntityLiving` fields it reads) |
//! | `blocks.rs` | `Block.isPassable` and the block state questions |
//! | `walk.rs` | `WalkNodeProcessor` |
//! | `swim.rs` | `SwimNodeProcessor` |
//! | `finder.rs` | `PathFinder` (200 dequeued nodes a search) |
//! | `navigate.rs` | `PathNavigate` + `PathNavigateGround` |
//! | `helpers.rs` | `EntityMoveHelper`, `EntityJumpHelper`, `EntityLookHelper` |
//! | `math.rs` | `MathHelper.atan2`, `wrapDegrees` |
//!
//! DROPPED as a whole: `FlyingNodeProcessor`, `PathNavigateFlying`, `PathNavigateClimber` and
//! `PathNavigateSwimmer`, `PathWorldListener` (block updates re-path in `updatePath` only), and
//! the AI tasks that call the navigator.

pub mod blocks;
pub mod finder;
pub mod heap;
pub mod helpers;
pub mod math;
pub mod navigate;
pub mod node;
pub mod point;
pub mod swim;
pub mod walk;

pub use helpers::{EntityJumpHelper, EntityLookHelper, EntityMoveHelper};
pub use navigate::PathNavigateGround;
pub use node::NavEntity;

#[cfg(test)]
mod tests;
