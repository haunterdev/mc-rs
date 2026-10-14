//! Fancy clouds: `RenderGlobal.renderCloudsFancy`.
//!
//! Vanilla draws 8 x 8 tiles of 8 x 8 texels, each face an 8-cell textured strip, and alpha tests
//! away the empty texels. The mesh here holds the same faces split per cell, keeping only the
//! cells the alpha test would keep, so what is drawn is the same set of fragments. The vertex
//! color carries the face shade times the texel; sky.wgsl multiplies in `getCloudColour`.
//!
//! The mesh is in cloud cells (12 blocks across, 1 high) and moves with the feet; it is rebuilt
//! only when the texel under the view changes or the view crosses a face rule's height.
//!
//! Not in the game: vanilla's field is 8 x 8 tiles (768 blocks) whatever the render
//! distance. Here it grows by whole tiles out to the distance terrain is drawn (`view_chunks`), up
//! to [`MAX_HALF`] tiles each way. The added tiles lie past vanilla's, where the view is never
//! inside them, so they keep only the walls facing an empty cell. Two
//! entities share it and draw it: a depth pass with color writes off, then the color pass, so only the nearest
//! cloud face is colored (`colorMask(false)` on pass 0).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use worldgen::math_helper::floor;

use crate::settings::{Clouds, Settings};
use crate::sky::{Clock, View};
use crate::textures::Argb;

/// `textures/environment/clouds.png`, 256 x 256.
#[derive(Resource)]
pub struct CloudTexture(pub Argb);

/// Marks the two cloud entities.
#[derive(Component)]
pub struct CloudMesh;

/// The inputs a cloud mesh depends on.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CloudKey {
    /// `MathHelper.floor(d4)`, `MathHelper.floor(d5)`: the texel under the view.
    pub texel: (i32, i32),
    /// `f2 > -5`: bottom faces drawn.
    pub bottom: bool,
    /// `f2 <= 5`: top faces drawn.
    pub top: bool,
    /// The alpha test threshold in force: 0.5 before the terrain, 0.1 after it.
    pub alpha_ref: f32,
    /// Tiles added on every side past vanilla's -3..=4; 0 is vanilla.
    pub reach: i32,
}

/// The most tiles the field reaches from the view on a side: 1536 blocks, half the texture's
/// 3072-block repeat, so no stretch of sky shows twice.
pub const MAX_HALF: i32 = 16;

/// The tiles added past vanilla's for terrain drawn `view_chunks` out: enough 96-block tiles to
/// cover it, at least vanilla's 3 west of the view.
pub fn reach_for(view_chunks: i32) -> i32 {
    let half = (view_chunks * 16 + 95) / 96;
    (half.min(MAX_HALF) - 3).max(0)
}

/// Where the cloud mesh sits: the view feet plus `(-12 f18, f2, -12 f19)`, in blocks.
pub struct CloudPlacement {
    pub key: CloudKey,
    pub offset: Vec3,
}

/// The positioning half of `renderCloudsFancy`, from the feet position `(x, y, z)`.
pub fn placement(cloud_ticks: i32, partial: f32, x: f64, y: f64, z: f64, alpha_ref: f32, reach: i32) -> CloudPlacement {
    let d3 = (cloud_ticks as f32 + partial) as f64;
    let mut d4 = (x + d3 * 0.029_999_999_329_447_746) / 12.0;
    let mut d5 = z / 12.0 + 0.330_000_013_113_021_85;
    // WorldProvider.getCloudHeight
    let f2 = 128.0f32 - y as f32 + 0.33;
    let k1 = floor(d4 / 2048.0);
    let l1 = floor(d5 / 2048.0);
    d4 -= (k1 * 2048) as f64;
    d5 -= (l1 * 2048) as f64;
    let f18 = (d4 - floor(d4) as f64) as f32;
    let f19 = (d5 - floor(d5) as f64) as f32;
    CloudPlacement {
        key: CloudKey { texel: (floor(d4), floor(d5)), bottom: f2 > -5.0, top: f2 <= 5.0, alpha_ref, reach },
        offset: Vec3::new(-12.0 * f18, f2, -12.0 * f19),
    }
}

/// The faces of `renderCloudsFancy` in cloud cells, before `scale(12, 1, 12)`.
pub fn cloud_mesh(tex: &Argb, key: CloudKey) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut color: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut quad = |p: [[f32; 3]; 4], c: [f32; 4]| {
        let base = pos.len() as u32;
        pos.extend_from_slice(&p);
        color.extend_from_slice(&[c; 4]);
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    const E: f32 = 9.765_625E-4;
    let (fx, fz) = key.texel;
    // GL_REPEAT over the 256 x 256 texture.
    let texel_at = |cx: i32, cz: i32| {
        let tx = (fx + cx).rem_euclid(tex.width as i32) as usize;
        let tz = (fz + cz).rem_euclid(tex.height as i32) as usize;
        tex.pixels[tz * tex.width as usize + tx]
    };
    // alphaFunc(GL_GREATER, ref) on texel alpha times the vertex alpha 0.8.
    let kept = |texel: u32| (texel >> 24) as f32 / 255.0 * 0.8 > key.alpha_ref;
    let r = key.reach;
    for l2 in -3 - r..=4 + r {
        for i3 in -3 - r..=4 + r {
            let added = !(-3..=4).contains(&l2) || !(-3..=4).contains(&i3);
            for j in 0..8 {
                for k in 0..8 {
                    let (cx, cz) = (l2 * 8 + j, i3 * 8 + k);
                    let texel = texel_at(cx, cz);
                    let a = (texel >> 24) as f32 / 255.0;
                    if !kept(texel) {
                        continue;
                    }
                    // An added tile's wall toward a kept cell is never the nearest face.
                    let open = |dx: i32, dz: i32| !added || !kept(texel_at(cx + dx, cz + dz));
                    let ch = |s: u32| ((texel >> s) & 255) as f32 / 255.0;
                    let rgb = [ch(16), ch(8), ch(0)];
                    let shade = |s: f32| [rgb[0] * s, rgb[1] * s, rgb[2] * s, a * 0.8];
                    let (x0, x1, z0, z1) = (cx as f32, (cx + 1) as f32, cz as f32, (cz + 1) as f32);
                    if key.bottom {
                        quad([[x0, 0.0, z1], [x1, 0.0, z1], [x1, 0.0, z0], [x0, 0.0, z0]], shade(0.7));
                    }
                    if key.top {
                        let y = 4.0 - E;
                        quad([[x0, y, z1], [x1, y, z1], [x1, y, z0], [x0, y, z0]], shade(1.0));
                    }
                    if l2 > -1 && open(-1, 0) {
                        quad([[x0, 0.0, z1], [x0, 4.0, z1], [x0, 4.0, z0], [x0, 0.0, z0]], shade(0.9));
                    }
                    if l2 <= 1 && open(1, 0) {
                        let x = x1 - E;
                        quad([[x, 0.0, z1], [x, 4.0, z1], [x, 4.0, z0], [x, 0.0, z0]], shade(0.9));
                    }
                    if i3 > -1 && open(0, -1) {
                        quad([[x0, 4.0, z0], [x1, 4.0, z0], [x1, 0.0, z0], [x0, 0.0, z0]], shade(0.8));
                    }
                    if i3 <= 1 && open(0, 1) {
                        let z = z1 - E;
                        quad([[x0, 4.0, z], [x1, 4.0, z], [x1, 0.0, z], [x0, 0.0, z]], shade(0.8));
                    }
                }
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, color)
        .with_inserted_indices(Indices::U32(idx))
}

/// `EntityRenderer.renderCloudsCheck`, both call sites: under eye height 128 the clouds draw
/// before the terrain (the sky camera, layer 1), from 128 up after everything (layer 0).
/// `GameSettings.shouldRenderClouds` turns them off below render distance 4. The field reaches as
/// far as the terrain does.
pub fn update_clouds(
    clock: Res<Clock>,
    settings: Res<Settings>,
    view: Res<View>,
    tex: Res<CloudTexture>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last: Local<Option<CloudKey>>,
    mut clouds: Query<(&Mesh3d, &mut Transform, &mut Visibility, &mut RenderLayers), With<CloudMesh>>,
) {
    let on = settings.clouds == Clouds::Fancy && settings.render_distance >= 4;
    let before_terrain = view.feet.y + view.eye_height < 128.0;
    let alpha_ref = if before_terrain { 0.5 } else { 0.1 };
    let feet = view.feet.as_dvec3();
    let p = placement(clock.cloud_ticks, clock.partial, feet.x, feet.y, feet.z, alpha_ref, reach_for(settings.view_chunks()));
    let rebuild = on && *last != Some(p.key);
    if rebuild {
        *last = Some(p.key);
    }
    let layer = if before_terrain { 1 } else { 0 };
    // Both entities share one mesh; build it once.
    let mut built = rebuild.then(|| cloud_mesh(&tex.0, p.key));
    for (mesh, mut transform, mut visibility, mut layers) in &mut clouds {
        *visibility = if on { Visibility::Inherited } else { Visibility::Hidden };
        if !on {
            continue;
        }
        if let Some(m) = built.take() {
            if let Some(mut slot) = meshes.get_mut(&mesh.0) {
                *slot = m;
            }
        }
        *transform = Transform::from_translation(view.feet + p.offset).with_scale(Vec3::new(12.0, 1.0, 12.0));
        if !layers.intersects(&RenderLayers::layer(layer)) {
            *layers = RenderLayers::layer(layer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(f: impl Fn(usize, usize) -> bool) -> Argb {
        let pixels = (0..256 * 256).map(|i| if f(i % 256, i / 256) { 0xFFFF_FFFF } else { 0x00FF_FFFF }).collect();
        Argb { width: 256, height: 256, pixels }
    }

    #[test]
    fn placement_tracks_the_view() {
        // Feet at the origin, tick 0: d4 = 0, d5 = 0.33, f2 = 128.33.
        let p = placement(0, 0.0, 0.0, 0.0, 0.0, 0.5, 0);
        assert_eq!(p.key.texel, (0, 0));
        assert!(!p.key.top && p.key.bottom);
        assert!((p.offset.y - 128.33).abs() < 1e-3);
        assert!((p.offset.z + 12.0 * 0.33).abs() < 1e-4);
        // Just over 400 ticks drift the clouds one cell east (0.03 blocks a tick, 12 a cell).
        let q = placement(401, 0.0, 0.0, 0.0, 0.0, 0.5, 0);
        assert_eq!(q.key.texel.0, 1);
        // Inside the cloud layer both face rules hold.
        let r = placement(0, 0.0, 0.0, 128.0, 0.0, 0.1, 0);
        assert!(r.key.top && r.key.bottom);
    }

    #[test]
    fn one_texel_makes_one_closed_cell() {
        // A single opaque texel at (0, 0), seen from below (bottom faces only) out in the tile
        // grid's interior: bottom, both x walls and both z walls.
        let tex = texture(|x, z| x == 0 && z == 0);
        let key = CloudKey { texel: (0, 0), bottom: true, top: false, alpha_ref: 0.5, reach: 0 };
        let m = cloud_mesh(&tex, key);
        // Cell (0, 0) is tile (0, 0): l2 = 0 and i3 = 0 pass all four wall rules.
        assert_eq!(m.indices().unwrap().len() / 6, 5);
        // An alpha test of 0.8 or more keeps nothing.
        let none = cloud_mesh(&tex, CloudKey { alpha_ref: 0.8, ..key });
        assert_eq!(none.indices().unwrap().len(), 0);
    }

    #[test]
    fn outer_tiles_drop_their_far_walls() {
        // Every texel opaque: tiles at l2 = -3 have no west walls, tiles at l2 = 4 no east walls.
        let tex = texture(|_, _| true);
        let key = CloudKey { texel: (0, 0), bottom: false, top: false, alpha_ref: 0.5, reach: 0 };
        let m = cloud_mesh(&tex, key);
        // West walls: l2 in 0..=4 (5 tiles); east: l2 in -3..=1 (5); north: i3 in 0..=4 (5);
        // south: i3 in -3..=1 (5). Each tile wall set is 8 x 8 cells, across 8 tiles.
        assert_eq!(m.indices().unwrap().len() / 6, 4 * 5 * 8 * 64);
        // Two added tiles a side: vanilla's walls stay, the added tiles have none between kept
        // cells, and every cell keeps its bottom.
        let wide = cloud_mesh(&tex, CloudKey { bottom: true, reach: 2, ..key });
        assert_eq!(wide.indices().unwrap().len() / 6, 4 * 5 * 8 * 64 + 12 * 12 * 64);
    }

    #[test]
    fn the_field_follows_the_view_distance() {
        // Vanilla's field until the terrain passes 3 tiles (18 chunks), then a tile per 6 chunks.
        assert_eq!([2, 12, 18, 19, 64, 96, 512].map(reach_for), [0, 0, 0, 1, 8, 13, 13]);
    }
}
