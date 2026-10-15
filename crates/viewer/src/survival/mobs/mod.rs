//! T10: mob entities from `S_SPAWN_MOB`, their models and rendering.
//!
//! | File | Owns |
//! |---|---|
//! | `living.rs` | `Living`: `EntityLivingBase` on the client (3 tick interpolation, limb swing, body turn, head yaw, hurtTime, deathTime) and the `doRender` numbers |
//! | `models.rs` | `MOBS`, the kinds table: parts, texture, pose, child layout, layers; `ModelQuadruped`/`ModelCow` |
//! | `farm.rs` | the pig (and saddle), sheep (and wool) and chicken part layouts and pose functions |
//! | `hostile.rs` | the zombie, skeleton (and its bow aim), creeper (swell, flash, charge layer) and spider (eyes layer) parts and poses |
//! | `held.rs` | `LayerHeldItem`: the `S_EQUIPMENT` main hand item on the right arm |
//! | `fire.rs` | `Render.renderEntityOnFire`: the fire sprites of a burning mob |
//! | `anim.rs` | the sheep eat-grass timer, the chicken wing flap, the creeper fuse, the `EnumDyeColor` fleece colors and the texture tint |
//! | `render.rs` | the frames, the tick, `RenderLivingBase.doRender`'s transforms and lighting, the layers |
//! | `preview.rs` | `--mobs`, the four kinds (an adult and a baby each) and a walking cow for a `--shot` |
//!
//! A further mob is one `MobDef` in `models::MOBS` (its boxes, texture path, pose function and
//! layers); the textures are read from the jar at startup in the table's order.

use bevy::prelude::*;

mod anim;
mod farm;
mod fire;
mod held;
mod hostile;
mod living;
mod models;
mod preview;
mod render;

pub use models::MOBS;
pub use fire::FireImages;
pub use render::{LayerImages, Mob, MobImages};

/// The images of every layer of `MOBS`: the wool layer as the 16 fleece-tinted copies of its
/// texture, the saddle as it is. `raw[kind][layer]` is the layer's jar texture.
pub fn layer_images(raw: &[Vec<crate::textures::Argb>], mut make: impl FnMut(&crate::textures::Argb) -> Handle<Image>) -> LayerImages {
    use models::LayerKind;
    LayerImages(
        MOBS.iter()
            .zip(raw)
            .map(|(m, layers)| {
                m.layers
                    .iter()
                    .zip(layers)
                    .map(|(l, img)| match l.kind {
                        LayerKind::Wool => (0..l.kind.variants() as u8).map(|c| make(&anim::tinted(img, anim::fleece_rgb(c)))).collect(),
                        _ => vec![make(img)],
                    })
                    .collect()
            })
            .collect(),
    )
}

pub fn plugin(app: &mut App) {
    render::plugin(app);
}
