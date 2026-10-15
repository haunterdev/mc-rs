//! `Render.renderEntityOnFire`: the two fire sprites standing in a burning mob's place, facing
//! the camera. A mob that `isBurning` (`MM_BURNING`) also renders at lightmap coordinates
//! (15, 15) (`RenderManager.renderEntity`'s 15728880), which `render.rs` applies.
//!
//! The quads alternate `fire_layer_0` and `fire_layer_1`, so each mob has two part entities, one
//! per sprite, each with the sprites' animation strip on its own material: the mesh covers one
//! frame of the strip and `EntityMaterial::fx` y steps it down the strip with the world clock.
//!
//! DROPPED: the sprite's mipmapped atlas filtering (the strip is its own nearest filtered
//! image), and the rebuild of the quads when a mob's `isChild` changes after its spawn.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// The two sprites' strips, the frames in each and the ticks a frame lasts (`.mcmeta`).
#[derive(Resource)]
pub struct FireImages {
    pub layers: [Handle<Image>; 2],
    pub frames: u32,
    pub frametime: u32,
}

impl FireImages {
    /// The strip's frame `TextureMap.updateAnimations` shows at this client tick.
    pub fn frame(&self, ticks: u64) -> u32 {
        ((ticks / self.frametime.max(1) as u64) % self.frames.max(1) as u64) as u32
    }
}

/// One quad of the stack: the corner positions (already scaled by `entity.width * 1.4`) and
/// whether its u axis is swapped.
pub struct FireQuad {
    pub corners: [[f32; 3]; 4],
    pub flip: bool,
    /// `i % 2`: which sprite
    pub sprite: usize,
}

/// The quads `renderEntityOnFire` emits for an entity of this size, in the frame whose origin is
/// the entity's feet and whose z axis points at the camera.
pub fn quads(width: f32, height: f32) -> Vec<FireQuad> {
    let f = width * 1.4;
    let mut f1 = 0.5f32;
    let mut f3 = height / f;
    let mut f4 = 0.0f32;
    let mut f5 = 0.0f32;
    // translate(0, 0, -0.3 + (int) f3 * 0.02) under the scale, so it scales too
    let tz = -0.3 + (f3 as i32) as f32 * 0.02;
    let mut out = Vec::new();
    let mut i = 0;
    while f3 > 0.0 {
        let p = |x: f32, y: f32| [x * f, y * f, (f5 + tz) * f];
        out.push(FireQuad { corners: [p(f1, 0.0 - f4), p(-f1, 0.0 - f4), p(-f1, 1.4 - f4), p(f1, 1.4 - f4)], flip: (i / 2) % 2 == 0, sprite: i % 2 });
        f3 -= 0.45;
        f4 -= 0.45;
        f1 *= 0.9;
        f5 += 0.03;
        i += 1;
    }
    out
}

/// The mesh of the quads that use one sprite, covering one frame (`1 / frames` of the strip).
pub fn mesh(width: f32, height: f32, sprite: usize, frames: u32) -> Mesh {
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    let v1 = 1.0 / frames.max(1) as f32;
    for q in quads(width, height).into_iter().filter(|q| q.sprite == sprite) {
        // f6/f8 are minU/maxU, swapped when the quad is flipped; f7/f9 are minV/maxV
        let (f6, f8) = if q.flip { (1.0, 0.0) } else { (0.0, 1.0) };
        let base = pos.len() as u32;
        for (c, t) in q.corners.iter().zip([[f8, v1], [f6, v1], [f6, 0.0], [f8, 0.0]]) {
            pos.push(*c);
            uv.push(t);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let normal = vec![[0.0, 0.0, 1.0]; pos.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

/// The fire's matrix in world space: the feet, turned to face the view (`rotate(-playerViewY, y)`).
pub fn world_matrix(feet: Vec3, view_yaw: f32) -> Mat4 {
    Mat4::from_translation(feet) * Mat4::from_rotation_y((-view_yaw).to_radians())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zombie_gets_six_quads_shrinking_and_alternating_sprites() {
        // 1.95 / (0.6 * 1.4) = 2.32: 2.32, 1.87, 1.42, 0.97, 0.52, 0.07 are above 0
        let q = quads(0.6, 1.95);
        assert_eq!(q.len(), 6);
        assert_eq!(q.iter().map(|q| q.sprite).collect::<Vec<_>>(), [0, 1, 0, 1, 0, 1]);
        assert_eq!(q.iter().map(|q| q.flip).collect::<Vec<_>>(), [true, true, false, false, true, true]);
        let f = 0.6 * 1.4;
        // the first quad: half a unit wide each side, from the feet to 1.4 up, scaled by f
        assert!((q[0].corners[0][0] - 0.5 * f).abs() < 1e-6 && (q[0].corners[2][1] - 1.4 * f).abs() < 1e-6);
        // the next ones are 0.45 lower, 0.9 times as wide and 0.03 nearer
        assert!((q[1].corners[0][1] - 0.45 * f).abs() < 1e-6 && (q[1].corners[0][0] - 0.45 * f).abs() < 1e-6);
        assert!(q[1].corners[0][2] > q[0].corners[0][2]);
        // translate(0, 0, -0.3 + 2 * 0.02) under the scale
        assert!((q[0].corners[0][2] - (-0.3 + 0.04) * f).abs() < 1e-6);
    }

    #[test]
    fn the_meshes_split_the_quads_by_sprite_and_keep_to_one_frame() {
        let a = mesh(0.6, 1.95, 0, 32);
        let b = mesh(0.6, 1.95, 1, 32);
        let n = |m: &Mesh| m.count_vertices();
        assert_eq!((n(&a), n(&b)), (12, 12));
        let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) = a.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!("uv") };
        assert!(uv.iter().all(|t| t[1] <= 1.0 / 32.0 + 1e-6));
    }

    #[test]
    fn the_strip_frame_follows_the_clock() {
        let f = FireImages { layers: [Handle::default(), Handle::default()], frames: 32, frametime: 2 };
        assert_eq!((f.frame(0), f.frame(1), f.frame(2), f.frame(64)), (0, 0, 1, 0));
    }

    #[test]
    fn the_fire_faces_the_view() {
        // yaw 0 looks along +z, so the quad's front (+z) must face -z toward the viewer: a quarter
        // turn of the view turns the quad with it
        let m = world_matrix(Vec3::new(1.0, 2.0, 3.0), 90.0);
        let n = m.transform_vector3(Vec3::Z);
        assert!((n - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5, "{n}");
        assert_eq!(m.transform_point3(Vec3::ZERO), Vec3::new(1.0, 2.0, 3.0));
    }
}
