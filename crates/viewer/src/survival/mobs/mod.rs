//! T10 fills this: mob entities from `S_SPAWN_MOB`, their models and rendering.

use bevy::prelude::*;

mod models;
mod render;

pub fn plugin(app: &mut App) {
    models::plugin(app);
    render::plugin(app);
}
