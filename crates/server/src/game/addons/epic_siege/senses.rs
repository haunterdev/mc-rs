//! ESM-4, the xray senses. `ai/hooks/EntitySensesProxy.java` replaces every mob's `EntitySenses`
//! so a target inside the xray distance is seen with no line of sight at all, and past it vanilla's
//! trace decides:
//!
//! ```text
//! entityIn.getDistance(entityObj) <= Xray
//!     || (entityObj.getDistance(entityIn) < 128.0f && entityObj.canEntityBeSeen(entityIn))
//! ```
//!
//! DROPPED: the proxy's per-tick seen and unseen caches (the trace runs each time, as the vanilla
//! goal it replaces does) and the profiler section.

use crate::game::mobs::ai::{dist_sq, Ctx, PlayerView};

/// How far vanilla's sight trace reaches when the target is outside the xray distance.
const TRACE_LIMIT: f64 = 128.0;

/// `EntitySensesProxy.canSee` from this mob's eyes to a player's.
pub fn can_see(c: &Ctx<'_>, target: &PlayerView, xray: f64) -> bool {
    let m = c.mob.body.pos;
    let d = dist_sq([m.x, m.y, m.z], target.pos).sqrt();
    d <= xray || (d < TRACE_LIMIT && c.can_see(target))
}
