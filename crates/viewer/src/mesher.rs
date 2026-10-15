//! Chunk meshing: `RenderChunk.rebuildChunk` over one 16x256x16 column, with block models drawn
//! by a port of `BlockModelRenderer` (smooth and flat lighting) and fluids by `fluid.rs`.
//!
//! Vertices carry what vanilla's `BufferBuilder` holds, in Bevy's standard attributes so the
//! stock vertex and prepass shaders work:
//! - `POSITION`: chunk-relative
//! - `NORMAL`: the quad's face normal (used by the shaded mode only)
//! - `UV_0`: sprite-relative texture coordinates
//! - `UV_1`: the lightmap coordinates, `(block << 4, sky << 4)` as vanilla's two shorts
//! - `COLOR`: the vertex color bytes / 255, and in alpha the array layer plus flags, as an
//!   exact small integer the shader rounds

use worldgen::blocks::{id, PODZOL};
use worldgen::populate::state as st;

use crate::colors::Colors;
use crate::fluid;
use crate::models::{coordinate_random, BakedModel, Facing, Models, Quad};
use crate::multipart;
use crate::region::{props, Layer, Region};
use crate::vertex::LayerMesh;

/// `COLOR.a` flag: sample level 0 only (`BlockRenderLayer.CUTOUT`, unmipped).
pub const FLAG_NOMIP: u32 = 1 << 10;
/// `COLOR.a` flag: the block emits light (`getLightValue() > 0`); the shaded mode draws it unlit.
pub const FLAG_EMISSIVE: u32 = 1 << 11;

/// The three meshes a chunk draws with: opaque, alpha-tested and blended.
#[derive(Default)]
pub struct ChunkMesh {
    /// `BlockRenderLayer.SOLID`
    pub solid: LayerMesh,
    /// `CUTOUT_MIPPED` and `CUTOUT`
    pub cutout: LayerMesh,
    /// `TRANSLUCENT`
    pub translucent: LayerMesh,
    /// The chests in the chunk (world coordinates). Their block draws nothing; the tile entity
    /// renderer (survival/chest_render.rs) does.
    pub chests: Vec<(i32, i32, i32)>,
}

impl ChunkMesh {
    pub fn quads(&self) -> usize {
        self.solid.quads() + self.cutout.quads() + self.translucent.quads()
    }

    pub fn layer(&mut self, layer: Layer) -> &mut LayerMesh {
        match layer {
            Layer::Solid => &mut self.solid,
            Layer::CutoutMipped | Layer::Cutout => &mut self.cutout,
            Layer::Translucent => &mut self.translucent,
        }
    }
}

/// Meshes the center chunk of `region`. `smooth` is `Minecraft.isAmbientOcclusionEnabled()`
/// (smooth lighting Minimum or Maximum; the two render the same in 1.12.2).
pub fn mesh(region: &Region, models: &Models, colors: &Colors, smooth: bool) -> ChunkMesh {
    let mut out = ChunkMesh::default();
    let mut r = Renderer { region, colors, tint: None, offset: [0.0; 3] };
    // BlockPos.getAllInBoxMutable: x fastest, then y, then z.
    for z in 0..16 {
        for y in 0..256 {
            for x in 0..16 {
                let state = region.state(x, y, z);
                if state >> 4 == id::CHEST {
                    out.chests.push((region.origin.0 + x, y, region.origin.1 + z));
                }
                let p = props(state);
                if p.invisible {
                    continue;
                }
                if p.liquid {
                    fluid::render(region, models, colors, state, x, y, z, &mut out);
                    continue;
                }
                let (wx, wz) = (region.origin.0 + x, region.origin.1 + z);
                let mut tag = 0;
                if p.layer == Layer::Cutout {
                    tag |= FLAG_NOMIP;
                }
                if p.light > 0 {
                    tag |= FLAG_EMISSIVE;
                }
                if models.is_multipart(state) {
                    // MultipartBakedModel: every selector whose condition matches draws.
                    let mask = multipart::mask(region, state, x, y, z);
                    let Some(pick) = models.get_multipart(state, mask, coordinate_random(wx, y, wz)) else { continue };
                    r.offset = offset(state, wx, wz);
                    r.tint = None;
                    let layer = out.layer(p.layer);
                    let ao = smooth && p.light == 0 && pick.ambient_occlusion;
                    for model in pick.models {
                        r.draw(state, state, (x, y, z), model, ao, layer, tag, &|f| multipart::side_rendered(region, state, mask, x, y, z, f));
                    }
                    continue;
                }
                let (draw, flag) = actual_state(region, state, x, y, z);
                let Some(bm) = models.get(draw, flag) else { continue };
                let model = bm.pick(wx, y, wz);
                r.offset = offset(draw, wx, wz);
                r.tint = None;
                let layer = out.layer(p.layer);
                // BlockModelRenderer.renderModel
                let ao = smooth && p.light == 0 && model.ambient_occlusion;
                r.draw(state, draw, (x, y, z), model, ao, layer, tag, &|f| should_side_be_rendered(region, state, x, y, z, f));
            }
        }
    }
    out
}

/// `getActualState` for the blocks whose model depends on a neighbor: the state to draw and the
/// boolean property it adds (`blockstates::blockstate_of`'s `flag`).
pub fn actual_state(region: &Region, state: u16, x: i32, y: i32, z: i32) -> (u16, bool) {
    match state >> 4 {
        // BlockGrass, BlockMycelium, BlockDirt (podzol only): SNOWY.
        id::GRASS | id::MYCELIUM => (state, snowy(region, x, y, z)),
        _ if state == PODZOL => (state, snowy(region, x, y, z)),
        // BlockVine: UP when the block above has a solid bottom face.
        id::VINE => (state, st::solid_face(region.state(x, y + 1, z), st::Facing::Down)),
        // BlockDoublePlant: the upper half takes VARIANT from the lower half, else SUNFLOWER.
        id::DOUBLE_PLANT if state & 8 != 0 => {
            let below = region.state(x, y - 1, z);
            let variant = if below >> 4 == id::DOUBLE_PLANT && below & 8 == 0 { below & 7 } else { 0 };
            (id::DOUBLE_PLANT << 4 | 8 | variant, false)
        }
        // BlockDoor: each half takes the rest of its state from the other (sim::door::actual).
        b if worldgen::blocks::is_door(b) => sim::door::actual(|x, y, z| region.state(x, y, z), state, x, y, z),
        _ => (state, false),
    }
}

fn snowy(region: &Region, x: i32, y: i32, z: i32) -> bool {
    matches!(region.state(x, y + 1, z) >> 4, id::SNOW | id::SNOW_LAYER)
}

/// `Block.getOffset`, in `sim::region` so the ray trace's flower box shares it.
pub(crate) use sim::region::offset;

/// `Block.shouldSideBeRendered`, with `BlockBreakable`'s rule for ice and glass, `BlockSnow`'s for snow
/// layers: a face shows unless the block's bounding box reaches that side and the neighbor there
/// is an opaque cube. Leaves follow the fancy-graphics rule (`leavesFancy`), which is the plain one.
pub fn should_side_be_rendered(r: &Region, state: u16, x: i32, y: i32, z: i32, f: Facing) -> bool {
    let o = f.vec();
    let n = r.state(x + o[0], y + o[1], z + o[2]);
    let block = state >> 4;
    // BlockBreakable: ignoreSimilarity is false for ice and glass, so neither faces its own kind.
    if matches!(block, id::ICE | id::GLASS) && n >> 4 == block {
        return false;
    }
    // BlockLilyPad.LILY_PAD_AABB = (0.0625, 0, 0.0625, 0.9375, 0.09375, 0.9375).
    if block == id::WATERLILY && f != Facing::Down {
        return true;
    }
    if block == id::SNOW_LAYER {
        if f == Facing::Up {
            return true;
        }
        // LAYERS is meta + 1 on both sides, so the metas compare the same way.
        if n >> 4 == id::SNOW_LAYER && n & 7 >= state & 7 {
            return false;
        }
    }
    !props(n).opaque_cube
}

/// `BlockModelRenderer.fillQuadBounds`: the twelve `quadBounds` (`EnumFacing` index for the
/// near bound, plus 6 for one minus it), then bit 0 "the quad lies on the block face" and bit 1
/// "the quad does not cover the whole face".
fn fill_quad_bounds(full_cube: bool, q: &Quad) -> ([f32; 12], bool, bool) {
    let (mut f, mut f1, mut f2, mut f3, mut f4, mut f5) = (32.0f32, 32.0f32, 32.0f32, -32.0f32, -32.0f32, -32.0f32);
    for v in &q.pos {
        f = f.min(v[0]);
        f1 = f1.min(v[1]);
        f2 = f2.min(v[2]);
        f3 = f3.max(v[0]);
        f4 = f4.max(v[1]);
        f5 = f5.max(v[2]);
    }
    let mut b = [0f32; 12];
    b[Facing::West as usize] = f;
    b[Facing::East as usize] = f3;
    b[Facing::Down as usize] = f1;
    b[Facing::Up as usize] = f4;
    b[Facing::North as usize] = f2;
    b[Facing::South as usize] = f5;
    for i in 0..6 {
        b[i + 6] = 1.0 - b[i];
    }
    let (on_face, partial) = match q.face {
        Facing::Down => (
            (f1 < 1.0E-4 || full_cube) && f1 == f4,
            f >= 1.0E-4 || f2 >= 1.0E-4 || f3 <= 0.9999 || f5 <= 0.9999,
        ),
        Facing::Up => (
            (f4 > 0.9999 || full_cube) && f1 == f4,
            f >= 1.0E-4 || f2 >= 1.0E-4 || f3 <= 0.9999 || f5 <= 0.9999,
        ),
        Facing::North => (
            (f2 < 1.0E-4 || full_cube) && f2 == f5,
            f >= 1.0E-4 || f1 >= 1.0E-4 || f3 <= 0.9999 || f4 <= 0.9999,
        ),
        Facing::South => (
            (f5 > 0.9999 || full_cube) && f2 == f5,
            f >= 1.0E-4 || f1 >= 1.0E-4 || f3 <= 0.9999 || f4 <= 0.9999,
        ),
        Facing::West => (
            (f < 1.0E-4 || full_cube) && f == f3,
            f1 >= 1.0E-4 || f2 >= 1.0E-4 || f4 <= 0.9999 || f5 <= 0.9999,
        ),
        Facing::East => (
            (f3 > 0.9999 || full_cube) && f == f3,
            f1 >= 1.0E-4 || f2 >= 1.0E-4 || f4 <= 0.9999 || f5 <= 0.9999,
        ),
    };
    (b, on_face, partial)
}

/// `BlockModelRenderer.EnumNeighborInfo` vert0Weights to vert3Weights: `quadBounds` indices,
/// multiplied in pairs, by facing index.
const WEIGHTS: [[[usize; 8]; 4]; 6] = {
    const D: usize = 0;
    const U: usize = 1;
    const N: usize = 2;
    const S: usize = 3;
    const W: usize = 4;
    const E: usize = 5;
    const FD: usize = 6;
    const FU: usize = 7;
    const FN: usize = 8;
    const FS: usize = 9;
    const FW: usize = 10;
    const FE: usize = 11;
    [
        [
            [FW, S, FW, FS, W, FS, W, S],
            [FW, N, FW, FN, W, FN, W, N],
            [FE, N, FE, FN, E, FN, E, N],
            [FE, S, FE, FS, E, FS, E, S],
        ],
        [
            [E, S, E, FS, FE, FS, FE, S],
            [E, N, E, FN, FE, FN, FE, N],
            [W, N, W, FN, FW, FN, FW, N],
            [W, S, W, FS, FW, FS, FW, S],
        ],
        [
            [U, FW, U, W, FU, W, FU, FW],
            [U, FE, U, E, FU, E, FU, FE],
            [D, FE, D, E, FD, E, FD, FE],
            [D, FW, D, W, FD, W, FD, FW],
        ],
        [
            [U, FW, FU, FW, FU, W, U, W],
            [D, FW, FD, FW, FD, W, D, W],
            [D, FE, FD, FE, FD, E, D, E],
            [U, FE, FU, FE, FU, E, U, E],
        ],
        [
            [U, S, U, FS, FU, FS, FU, S],
            [U, N, U, FN, FU, FN, FU, N],
            [D, N, D, FN, FD, FN, FD, N],
            [D, S, D, FS, FD, FS, FD, S],
        ],
        [
            [FD, S, FD, FS, D, FS, D, S],
            [FD, N, FD, FN, D, FN, D, N],
            [FU, N, FU, FN, U, FN, U, N],
            [FU, S, FU, FS, U, FS, U, S],
        ],
    ]
};

/// `AmbientOcclusionFace.getVertexBrightness`
fn vertex_brightness(b: [i32; 4], w: [f32; 4]) -> i32 {
    let sum = |shift: u32| (0..4).map(|k| (b[k] >> shift & 255) as f32 * w[k]).fold(0.0f32, |a, v| a + v) as i32 & 255;
    sum(16) << 16 | sum(0)
}

/// `BlockModelRenderer.EnumNeighborInfo.corners`, by facing index.
const CORNERS: [[Facing; 4]; 6] = {
    use Facing::*;
    [[West, East, North, South], [East, West, North, South], [Up, Down, East, West], [West, East, Down, Up], [Up, Down, North, South], [Down, Up, North, South]]
};

/// `BlockModelRenderer.VertexTranslations`, by facing index.
const VERTEX_TRANSLATIONS: [[usize; 4]; 6] = [[0, 1, 2, 3], [2, 3, 0, 1], [3, 0, 1, 2], [0, 1, 2, 3], [3, 0, 1, 2], [1, 2, 3, 0]];

/// `BlockModelRenderer.AmbientOcclusionFace.getAoBrightness`
pub fn ao_brightness(mut br1: i32, mut br2: i32, mut br3: i32, br4: i32) -> i32 {
    if br1 == 0 {
        br1 = br4;
    }
    if br2 == 0 {
        br2 = br4;
    }
    if br3 == 0 {
        br3 = br4;
    }
    (br1 + br2 + br3 + br4) >> 2 & 16711935
}

/// `BufferBuilder.putColorMultiplier` on a baked color whose three channels are `shade`.
fn color_multiplier(shade: u8, r: f32, g: f32, b: f32) -> [u8; 3] {
    let s = shade as f32;
    [(s * r) as i32 as u8, (s * g) as i32 as u8, (s * b) as i32 as u8]
}

struct Renderer<'a> {
    region: &'a Region,
    colors: &'a Colors,
    /// The block's `colorMultiplier`, computed on its first tinted quad.
    tint: Option<i32>,
    /// The block's `getOffset`.
    offset: [f64; 3],
}

impl Renderer<'_> {
    /// `BlockModelRenderer.renderModel` for one baked model: each side's quads when `side` says the
    /// face shows, then the quads that never cull.
    #[allow(clippy::too_many_arguments)]
    fn draw(&mut self, state: u16, draw: u16, (x, y, z): (i32, i32, i32), model: &BakedModel, ao: bool, layer: &mut LayerMesh, tag: u32, side: &dyn Fn(Facing) -> bool) {
        for f in Facing::ALL {
            let quads = &model.face_quads[f as usize];
            if !quads.is_empty() && side(f) {
                if ao {
                    self.quads_smooth(state, draw, x, y, z, quads, layer, tag);
                } else {
                    let o = f.vec();
                    let light = self.packed_light(x + o[0], y + o[1], z + o[2]);
                    self.quads_flat(state, draw, x, y, z, Some(light), quads, layer, tag);
                }
            }
        }
        if !model.general.is_empty() {
            if ao {
                self.quads_smooth(state, draw, x, y, z, &model.general, layer, tag);
            } else {
                self.quads_flat(state, draw, x, y, z, None, &model.general, layer, tag);
            }
        }
    }

    /// `Block.getPackedLightmapCoords` for a block with light value 0 (every block a model draws).
    fn packed_light(&self, x: i32, y: i32, z: i32) -> i32 {
        self.region.combined_light(x, y, z, 0)
    }

    fn ao_value(&self, x: i32, y: i32, z: i32) -> f32 {
        self.region.props(x, y, z).ao_light_value()
    }

    fn tint_rgb(&mut self, state: u16, x: i32, y: i32, z: i32) -> (f32, f32, f32) {
        let k = *self.tint.get_or_insert_with(|| self.colors.multiplier(state, self.region, x, y, z));
        ((k >> 16 & 255) as f32 / 255.0, (k >> 8 & 255) as f32 / 255.0, (k & 255) as f32 / 255.0)
    }

    /// `BlockModelRenderer.renderQuadsSmooth` with `AmbientOcclusionFace.updateVertexBrightness`.
    #[allow(clippy::too_many_arguments)]
    fn quads_smooth(&mut self, state: u16, draw: u16, x: i32, y: i32, z: i32, quads: &[Quad], out: &mut LayerMesh, tag: u32) {
        let full_cube = props(state).full_cube;
        for q in quads {
            let (bounds, on_face, partial) = fill_quad_bounds(full_cube, q);
            let dir = q.face;
            let d = dir.vec();
            let bp = if on_face { [x + d[0], y + d[1], z + d[2]] } else { [x, y, z] };
            let c = CORNERS[dir as usize];
            let at = |a: [i32; 3], f: Facing| {
                let o = f.vec();
                [a[0] + o[0], a[1] + o[1], a[2] + o[2]]
            };
            let light = |p: [i32; 3]| self.packed_light(p[0], p[1], p[2]);
            let ao = |p: [i32; 3]| self.ao_value(p[0], p[1], p[2]);
            let translucent = |p: [i32; 3]| self.region.props(p[0], p[1], p[2]).translucent();
            let p0 = at(bp, c[0]);
            let p1 = at(bp, c[1]);
            let p2 = at(bp, c[2]);
            let p3 = at(bp, c[3]);
            let (i, f) = (light(p0), ao(p0));
            let (j, f1) = (light(p1), ao(p1));
            let (k, f2) = (light(p2), ao(p2));
            let (l, f3) = (light(p3), ao(p3));
            let flag = translucent(at(p0, dir));
            let flag1 = translucent(at(p1, dir));
            let flag2 = translucent(at(p2, dir));
            let flag3 = translucent(at(p3, dir));
            let (f4, i1) = if !flag2 && !flag { (f, i) } else { let p = at(p0, c[2]); (ao(p), light(p)) };
            let (f5, j1) = if !flag3 && !flag { (f, i) } else { let p = at(p0, c[3]); (ao(p), light(p)) };
            let (f6, k1) = if !flag2 && !flag1 { (f1, j) } else { let p = at(p1, c[2]); (ao(p), light(p)) };
            let (f7, l1) = if !flag3 && !flag1 { (f1, j) } else { let p = at(p1, c[3]); (ao(p), light(p)) };
            let mut i3 = light([x, y, z]);
            let front = [x + d[0], y + d[1], z + d[2]];
            if on_face || !self.region.props(front[0], front[1], front[2]).opaque_cube {
                i3 = light(front);
            }
            let f8 = if on_face { ao(bp) } else { ao([x, y, z]) };
            let vt = VERTEX_TRANSLATIONS[dir as usize];
            let mut brightness = [0i32; 4];
            let mut cm = [0f32; 4];
            let corners = [(f3 + f + f5 + f8) * 0.25, (f2 + f + f4 + f8) * 0.25, (f2 + f1 + f6 + f8) * 0.25, (f3 + f1 + f7 + f8) * 0.25];
            let lights = [ao_brightness(l, i, j1, i3), ao_brightness(k, i, i1, i3), ao_brightness(k, j, k1, i3), ao_brightness(l, j, l1, i3)];
            if partial {
                // doNonCubicWeight is set for every facing.
                for v in 0..4 {
                    let t = WEIGHTS[dir as usize][v];
                    let w: [f32; 4] = std::array::from_fn(|k| bounds[t[2 * k]] * bounds[t[2 * k + 1]]);
                    cm[vt[v]] = (0..4).map(|k| corners[k] * w[k]).fold(0.0f32, |a, x| a + x);
                    brightness[vt[v]] = vertex_brightness(lights, w);
                }
            } else {
                for v in 0..4 {
                    cm[vt[v]] = corners[v];
                    brightness[vt[v]] = lights[v];
                }
            }
            let colors = if q.tint >= 0 {
                let (r, g, b) = self.tint_rgb(draw, x, y, z);
                cm.map(|m| color_multiplier(q.shade, m * r, m * g, m * b))
            } else {
                cm.map(|m| color_multiplier(q.shade, m, m, m))
            };
            emit(out, self.offset, x, y, z, q, brightness, colors, tag);
        }
    }

    /// `BlockModelRenderer.renderQuadsFlat`. `brightness` is the face's light, or `None` for the
    /// general quads (`ownBrightness`).
    #[allow(clippy::too_many_arguments)]
    fn quads_flat(&mut self, state: u16, draw: u16, x: i32, y: i32, z: i32, brightness: Option<i32>, quads: &[Quad], out: &mut LayerMesh, tag: u32) {
        let full_cube = props(state).full_cube;
        for q in quads {
            let b = brightness.unwrap_or_else(|| {
                let (_, on_face, _) = fill_quad_bounds(full_cube, q);
                if on_face {
                    let d = q.face.vec();
                    self.packed_light(x + d[0], y + d[1], z + d[2])
                } else {
                    self.packed_light(x, y, z)
                }
            });
            let colors = if q.tint >= 0 {
                let (r, g, b) = self.tint_rgb(draw, x, y, z);
                [color_multiplier(q.shade, r, g, b); 4]
            } else {
                [[q.shade; 3]; 4]
            };
            emit(out, self.offset, x, y, z, q, [b; 4], colors, tag);
        }
    }
}

/// `BufferBuilder.putPosition(pos + getOffset)`: `(float)(x + offset) + vertex`.
#[allow(clippy::too_many_arguments)]
fn emit(out: &mut LayerMesh, o: [f64; 3], x: i32, y: i32, z: i32, q: &Quad, light: [i32; 4], colors: [[u8; 3]; 4], tag: u32) {
    let (bx, by, bz) = ((x as f64 + o[0]) as f32, (y as f64 + o[1]) as f32, (z as f64 + o[2]) as f32);
    let pos = q.pos.map(|p| [p[0] + bx, p[1] + by, p[2] + bz]);
    let n = q.face.vec();
    out.quad(pos, q.uv, light, colors, [n[0] as f32, n[1] as f32, n[2] as f32], q.sprite.layer | tag);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::tests::{chunk, region_of};
    use worldgen::blocks::{GRASS, STONE};

    #[test]
    fn ao_brightness_fills_dark_corners_from_the_face() {
        // A corner behind an opaque block reads 0 and takes the face's light instead.
        let sky15 = 15 << 20;
        assert_eq!(ao_brightness(0, sky15, sky15, sky15), sky15);
        assert_eq!(ao_brightness(0, 0, 0, 240 << 16), 240 << 16);
        assert_eq!(ao_brightness(15 << 4, 0, 0, 0), (15 << 4) >> 2);
    }

    #[test]
    fn flat_face_takes_the_neighbor_light_and_full_quads_lie_on_the_face() {
        let q = Quad {
            pos: [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]],
            uv: [[0.0; 2]; 4],
            shade: 255,
            tint: -1,
            face: Facing::Up,
            sprite: crate::textures::SpriteInfo { layer: 0, frames: 1, frametime: 1 },
        };
        let (b, on_face, partial) = fill_quad_bounds(true, &q);
        assert_eq!((on_face, partial), (true, false));
        assert_eq!(b[Facing::Up as usize], 1.0);
        assert_eq!(b[Facing::Up as usize + 6], 0.0);
        let mut lily = q;
        lily.pos.iter_mut().for_each(|p| p[1] = 0.015625);
        let (_, on_face, partial) = fill_quad_bounds(false, &lily);
        assert_eq!((on_face, partial), (false, false));
    }

    #[test]
    fn full_weights_reduce_to_the_corner_average() {
        // A full face: each vertex's weights pick one corner with weight 1.
        let mut b = [0f32; 12];
        for i in 0..6 {
            b[i] = if i % 2 == 0 { 0.0 } else { 1.0 };
            b[i + 6] = 1.0 - b[i];
        }
        // Up face of a full cube: bounds D 0 U 1 N 0 S 1 W 0 E 1.
        let w: [f32; 4] = std::array::from_fn(|k| b[WEIGHTS[1][0][2 * k]] * b[WEIGHTS[1][0][2 * k + 1]]);
        assert_eq!(w, [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertex_brightness([240 << 16 | 48, 0, 0, 0], w), 240 << 16 | 48);
    }

    #[test]
    fn plants_shift_by_the_coordinate_hash() {
        use worldgen::blocks::state;
        assert_eq!(offset(STONE, 5, 7), [0.0; 3]);
        let o = offset(state(id::TALLGRASS, 1), 5, 7);
        assert!(o[0].abs() <= 0.25 && o[2].abs() <= 0.25 && (-0.2..=0.0).contains(&o[1]));
        assert_eq!(offset(state(id::RED_FLOWER, 0), 5, 7)[1], 0.0);
    }

    #[test]
    fn upper_plant_half_copies_the_variant_below() {
        use worldgen::blocks::state;
        let r = region_of(
            chunk(0, 0, 64, |p| {
                p.set(4, 64, 4, state(id::DOUBLE_PLANT, 2));
                p.set(4, 65, 4, state(id::DOUBLE_PLANT, 8));
            }),
            64,
        );
        assert_eq!(actual_state(&r, state(id::DOUBLE_PLANT, 8), 4, 65, 4), (state(id::DOUBLE_PLANT, 10), false));
        assert_eq!(actual_state(&r, GRASS, 4, 63, 4), (GRASS, false));
    }

    #[test]
    fn hidden_faces_are_culled() {
        let r = region_of(
            chunk(0, 0, 64, |p| {
                p.set(4, 64, 4, GRASS);
                p.set(4, 65, 4, STONE);
            }),
            64,
        );
        assert!(!should_side_be_rendered(&r, STONE, 4, 30, 4, Facing::Up));
        assert!(should_side_be_rendered(&r, GRASS, 4, 64, 4, Facing::North));
        assert!(!should_side_be_rendered(&r, GRASS, 4, 64, 4, Facing::Up));
    }

    #[test]
    fn open_ground_is_lit_by_the_sky() {
        let r = region_of(chunk(0, 0, 64, |_| ()), 64);
        // Top face of the stone at y 63, flat: the light of the air above.
        let up = r.combined_light(3, 64, 3, 0);
        assert_eq!(up, 15 << 20 | 0);
        // Smooth: every corner of an open face reads full sky, so the AO average keeps it.
        assert_eq!(ao_brightness(up, up, up, up), 240 << 16);
    }

    /// Every placeable model state, set alone in the air, meshes at least one quad, in
    /// the layer `Block.getRenderLayer` names, and a full opaque cube beside it is culled.
    #[test]
    fn placed_blocks_mesh() {
        use crate::{colors::Colors, jar::Jar};
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        let colors = Colors::load(&jar).expect("colors");
        let mut bad = Vec::new();
        for (b, m) in [(id::SAPLING, 0u16), (id::SAPLING, 3), (id::SAPLING, 5), (id::SAPLING, 8 | 2), (19, 0), (19, 1), (id::TALLGRASS, 1), (id::DEADBUSH, 0), (id::YELLOW_FLOWER, 0)] {
            let s = worldgen::blocks::state(b, m);
            let r = region_of(chunk(0, 0, 64, |p| p.set(4, 70, 4, s)), 64);
            let mesh = mesh(&r, &models, &colors, true);
            let layer = props(s).layer;
            let in_layer = match layer {
                Layer::Solid => mesh.solid.quads(),
                Layer::Translucent => mesh.translucent.quads(),
                _ => mesh.cutout.quads(),
            };
            if in_layer == 0 {
                bad.push(format!("{b}:{m} {layer:?}"));
            }
        }
        assert!(bad.is_empty(), "no quads: {bad:?}");
        // The sponge is an opaque cube like stone: a neighbor's face toward it is culled.
        let sponge = worldgen::blocks::state(19, 0);
        assert!(props(sponge).opaque_cube && props(sponge).full_cube);
        let r = region_of(chunk(0, 0, 64, |p| p.set(4, 70, 4, sponge)), 64);
        assert!(!should_side_be_rendered(&r, STONE, 4, 71, 4, Facing::Down));
    }

    /// A multipart block set in the air, with `others` around it; the models it draws.
    fn multipart_models(models: &Models, block: u16, others: &[(i32, i32, i32, u16)]) -> (u16, usize) {
        use worldgen::blocks::state;
        let (x, y, z) = (4, 70, 4);
        let others = others.to_vec();
        let r = region_of(
            chunk(0, 0, 64, |p| {
                p.set(x, y, z, state(block, 0));
                for &(dx, dy, dz, s) in &others {
                    p.set(x + dx, y + dy, z + dz, s);
                }
            }),
            64,
        );
        let mask = multipart::mask(&r, state(block, 0), x, y, z);
        (mask, models.get_multipart(state(block, 0), mask, 0).expect("multipart").models.len())
    }

    #[test]
    fn a_fence_between_two_fences_bakes_a_post_and_two_sides() {
        use crate::blockstates::{EAST, WEST};
        use crate::{colors::Colors, jar::Jar};
        use worldgen::blocks::state;
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        let fence = state(85, 0);
        let lone = multipart_models(&models, 85, &[]);
        assert_eq!(lone, (0, 1));
        let both = multipart_models(&models, 85, &[(1, 0, 0, fence), (-1, 0, 0, fence)]);
        assert_eq!(both, (EAST | WEST, 3));
        // A nether brick fence does not join a wooden one, and stone joins both.
        assert_eq!(multipart_models(&models, 85, &[(1, 0, 0, state(113, 0))]).0, 0);
        assert_eq!(multipart_models(&models, 113, &[(1, 0, 0, STONE)]).0, EAST);
        // A fence is not a full cube: the stone beside it still draws its face toward it.
        assert!(!props(fence).opaque_cube && !props(fence).full_cube);
        let r = region_of(chunk(0, 0, 64, |p| p.set(5, 70, 4, fence)), 64);
        assert!(should_side_be_rendered(&r, STONE, 4, 70, 4, Facing::East));
        // The mesh holds the post and both arms.
        let colors = Colors::load(&jar).expect("colors");
        let mesh_of = |others: &[(i32, u16)]| {
            let r = region_of(
                chunk(0, 0, 64, |p| {
                    p.set(4, 70, 4, fence);
                    for &(dx, s) in others {
                        p.set(4 + dx, 70, 4, s);
                    }
                }),
                64,
            );
            mesh(&r, &models, &colors, true).quads()
        };
        let one = mesh_of(&[]);
        assert!(one > 0);
        let three = mesh_of(&[(1, fence), (-1, fence)]);
        assert!(three > one, "three fences draw more than one lone post: {three} vs {one}");
    }

    #[test]
    fn a_lone_pane_bakes_its_post_shape() {
        use crate::blockstates::{NORTH, SOUTH};
        use crate::jar::Jar;
        use worldgen::blocks::state;
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        // glass_pane.json: the post, and a noside model for each of the four open sides.
        assert_eq!(multipart_models(&models, 102, &[]), (0, 5));
        // Joined north and south (glass and stone): the post, two sides and two nosides.
        let (mask, n) = multipart_models(&models, 102, &[(0, 0, -1, state(102, 0)), (0, 0, 1, STONE)]);
        assert_eq!((mask, n), (NORTH | SOUTH, 5));
        // Iron bars and stained panes join panes.
        assert_eq!(multipart_models(&models, 101, &[(0, 0, -1, state(160, 3))]).0, NORTH);
    }

    #[test]
    fn a_wall_with_only_north_and_south_has_no_up_post() {
        use crate::blockstates::{NORTH, SOUTH, UP};
        use crate::jar::Jar;
        use worldgen::blocks::state;
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        let wall = state(139, 0);
        let (mask, n) = multipart_models(&models, 139, &[(0, 0, -1, wall), (0, 0, 1, wall)]);
        assert_eq!(mask, NORTH | SOUTH);
        assert_eq!(n, 2, "two sides and no post");
        // A block above, or a third side, brings the post back.
        assert_eq!(multipart_models(&models, 139, &[(0, 0, -1, wall), (0, 0, 1, wall), (0, 1, 0, STONE)]).0, NORTH | SOUTH | UP);
        assert_eq!(multipart_models(&models, 139, &[(0, 0, -1, wall), (0, 0, 1, wall), (1, 0, 0, wall)]).0, NORTH | SOUTH | crate::blockstates::EAST | UP);
        assert_eq!(multipart_models(&models, 139, &[]), (UP, 1));
    }

    #[test]
    fn redstone_wire_next_to_a_block_one_higher_bakes_the_up_piece() {
        use crate::blockstates::{WIRE_SIDE, WIRE_UP};
        use crate::jar::Jar;
        use worldgen::blocks::state;
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        let wire = state(55, 0);
        // Alone it is the dot.
        assert_eq!(multipart_models(&models, 55, &[]), (0, 1));
        // A wire beside it joins as a side.
        assert_eq!(multipart_models(&models, 55, &[(1, 0, 0, wire)]).0, WIRE_SIDE << 2);
        // Stone beside it with wire on top climbs: the east side is UP, and redstone_up draws.
        let (mask, n) = multipart_models(&models, 55, &[(1, 0, 0, STONE), (1, 1, 0, wire)]);
        assert_eq!(mask, WIRE_UP << 2);
        assert_eq!(n, 3, "the line pieces and redstone_up");
        // Stone beside it with nothing on top leaves the side open.
        assert_eq!(multipart_models(&models, 55, &[(1, 0, 0, STONE)]).0, 0);
    }

    #[test]
    fn fire_chorus_and_the_brewing_stand_pick_their_models() {
        use crate::blockstates::{DOWN, EAST, UP};
        use crate::jar::Jar;
        use worldgen::blocks::state;
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        // Fire with nothing to burn or stand on draws the floor and every side; over a floor that
        // holds it the neighbors do not count; a plank wall beside it with air below flames that side.
        // (the floor pair and the four side flames: the `OR` with all properties false matches).
        assert_eq!(multipart_models(&models, 51, &[]), (0, 5));
        assert_eq!(multipart_models(&models, 51, &[(0, -1, 0, STONE), (1, 0, 0, state(5, 0))]).0, 0);
        assert_eq!(multipart_models(&models, 51, &[(1, 0, 0, state(5, 0))]).0, EAST);
        // Chorus plant: stacked on end stone and under another plant.
        let chorus = state(199, 0);
        assert_eq!(multipart_models(&models, 199, &[(0, -1, 0, state(121, 0)), (0, 1, 0, chorus)]).0, DOWN | UP);
        // Brewing stand: bottles are meta bits.
        let r = region_of(chunk(0, 0, 64, |p| p.set(4, 70, 4, state(117, 5))), 64);
        assert_eq!(multipart::mask(&r, state(117, 5), 4, 70, 4), 5);
    }
}
