//! T7b and later fill this: the container screens, one file per screen.

use bevy::prelude::*;

mod chest;
mod crafting;
mod creative;
mod furnace;
mod inventory;
mod slots;

pub fn plugin(app: &mut App) {
    chest::plugin(app);
    crafting::plugin(app);
    creative::plugin(app);
    furnace::plugin(app);
    inventory::plugin(app);
    slots::plugin(app);
}
