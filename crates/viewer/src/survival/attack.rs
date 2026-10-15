//! Attacking entities: the viewer half of `Minecraft.clickMouse`'s ENTITY case and
//! `PlayerControllerMP.attackEntity`.
//!
//! The pick is target.rs's `pick_entity`, run by `interact::tick` over the boxes [`gather`] keeps
//! in `Controller::entities` (every tracked mob, from mobs/render.rs, and every other player);
//! `interact::attack_entity` queues the target in `Controller::attacks` and resets the attack
//! cooldown, and [`flush`] sends each as `V_ATTACK` (`CPacketUseEntity` ATTACK). The swing, the
//! cooldown indicator and the weakened hit all read `Hand` as for a block click.
//!
//! The server owns the result. On a `WorldClient` `attackEntityFrom` answers false, so
//! `attackTargetEntityWithCurrentItem`'s knockback, `setSprinting(false)`, crit and sounds run
//! only there; the crit particles come back as `S_ANIMATION` (crit.rs), the hurt as
//! `S_ENTITY_EVENT` (mobs/render.rs).
//!
//! A right click on `Controller::entity` is `interactWithEntity` (`interact::right_click_mouse`):
//! it queues the target and the hand in `Controller::interacts` and [`flush`] sends `V_INTERACT`,
//! in the same slot as the attack.
//!
//! DROPPED: picking dead or spectating players, and the item frame and riding cases of
//! `getMouseOver`.

use bevy::prelude::*;

use super::mobs::Mob;
use crate::interact::Controller;
use crate::link::Link;
use crate::players::{Other, Others, OwnModel};

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            gather.after(crate::players::apply_events).before(crate::player::run_ticks),
            flush.after(crate::player::run_ticks).run_if(resource_exists::<Link>),
        )
            .run_if(resource_exists::<Controller>),
    );
}

/// `getEntitiesInAABBexcluding(..., canBeCollidedWith)`'s candidates: each tracked mob's and each
/// other player's box, refreshed before the ticks pick.
fn gather(mut ctl: ResMut<Controller>, mobs: Query<&Mob>, others: Res<Others>, players: Query<&Other, Without<OwnModel>>) {
    ctl.entities.clear();
    ctl.entities.extend(mobs.iter().filter(|m| !m.living.dead).map(|m| (m.id, m.bb())));
    ctl.entities.extend(others.all().filter_map(|(id, e)| players.get(e).ok()?.bb().map(|b| (id, b))));
}

/// `CPacketUseEntity` ATTACK and INTERACT for every `attackEntity` and `interactWithEntity` the ticks made.
fn flush(mut ctl: ResMut<Controller>, link: Res<Link>) {
    for id in std::mem::take(&mut ctl.attacks) {
        super::net::send_attack(&link, &proto::Attack { target: id });
    }
    for (id, hand) in std::mem::take(&mut ctl.interacts) {
        super::net::send_interact(&link, &proto::Interact { target: id, hand });
    }
}
