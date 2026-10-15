//! Block models baked the way vanilla bakes them: blockstate JSON to weighted variants, model JSON
//! with parents and texture variables, then `FaceBakery` per element face, rotated by
//! `ModelRotation`. Everything is read from the client jar at startup.

use std::collections::HashMap;
use std::io;

use serde_json::Value;

use crate::blockstates::{blockstate_of, default_blockstate, generated_states, is_multipart_block, multipart_file, multipart_props};
use crate::item::{self, Display, ItemModel};
use crate::jar::Jar;
use crate::textures::{load_sprite, BlockTextures, Sprite, SpriteInfo};

/// `EnumFacing`, in `values()` order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Facing {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Facing {
    pub const ALL: [Facing; 6] = [Facing::Down, Facing::Up, Facing::North, Facing::South, Facing::West, Facing::East];
    /// `EnumFacing.Plane.HORIZONTAL` order: N, E, S, W.
    pub const HORIZONTAL: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];

    /// `EnumFacing.getDirectionVec`.
    pub const fn vec(self) -> [i32; 3] {
        match self {
            Facing::Down => [0, -1, 0],
            Facing::Up => [0, 1, 0],
            Facing::North => [0, 0, -1],
            Facing::South => [0, 0, 1],
            Facing::West => [-1, 0, 0],
            Facing::East => [1, 0, 0],
        }
    }

    pub const fn axis(self) -> Axis {
        match self {
            Facing::Down | Facing::Up => Axis::Y,
            Facing::North | Facing::South => Axis::Z,
            Facing::West | Facing::East => Axis::X,
        }
    }

    pub fn by_name(s: &str) -> Option<Facing> {
        Facing::ALL.into_iter().find(|f| f.name() == s)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Facing::Down => "down",
            Facing::Up => "up",
            Facing::North => "north",
            Facing::South => "south",
            Facing::West => "west",
            Facing::East => "east",
        }
    }

    /// `EnumFacing.rotateX`. East and West throw in vanilla; `rotateAround` never passes them.
    fn rotate_x(self) -> Facing {
        match self {
            Facing::North => Facing::Down,
            Facing::South => Facing::Up,
            Facing::Up => Facing::North,
            Facing::Down => Facing::South,
            f => f,
        }
    }

    /// `EnumFacing.rotateY`. Up and Down throw in vanilla; `rotateAround` never passes them.
    fn rotate_y(self) -> Facing {
        match self {
            Facing::North => Facing::East,
            Facing::East => Facing::South,
            Facing::South => Facing::West,
            Facing::West => Facing::North,
            f => f,
        }
    }

    /// `EnumFacing.rotateAround` for the X and Y axes.
    fn rotate_around(self, axis: Axis) -> Facing {
        match axis {
            Axis::X if self != Facing::West && self != Facing::East => self.rotate_x(),
            Axis::Y if self != Facing::Up && self != Facing::Down => self.rotate_y(),
            _ => self,
        }
    }
}

/// `EnumFaceDirection`: per face, the four corners as indices into the `getPositionsDiv16` array
/// (indexed by facing: 0 minY, 1 maxY, 2 minZ, 3 maxZ, 4 minX, 5 maxX), as (x, y, z).
const FACE_DIRECTION: [[[usize; 3]; 4]; 6] = {
    const D: usize = 0;
    const U: usize = 1;
    const N: usize = 2;
    const S: usize = 3;
    const W: usize = 4;
    const E: usize = 5;
    [
        [[W, D, S], [W, D, N], [E, D, N], [E, D, S]],
        [[W, U, N], [W, U, S], [E, U, S], [E, U, N]],
        [[E, U, N], [E, D, N], [W, D, N], [W, U, N]],
        [[W, U, S], [W, D, S], [E, D, S], [E, U, S]],
        [[W, U, N], [W, D, N], [W, D, S], [W, U, S]],
        [[E, U, S], [E, D, S], [E, D, N], [E, U, N]],
    ]
};

/// A 4x4 matrix stored like LWJGL's `Matrix4f`: `m[col][row]`.
type Mat4 = [[f32; 4]; 4];

const IDENTITY: Mat4 = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];

/// LWJGL `Matrix4f.rotate(angle, axis, src, dest)`.
fn mat_rotate(angle: f32, axis: [f32; 3], src: &Mat4) -> Mat4 {
    let c = (angle as f64).cos() as f32;
    let s = (angle as f64).sin() as f32;
    let oneminusc = 1.0 - c;
    let [ax, ay, az] = axis;
    let (xy, yz, xz) = (ax * ay, ay * az, ax * az);
    let (xs, ys, zs) = (ax * s, ay * s, az * s);
    let f00 = ax * ax * oneminusc + c;
    let f01 = xy * oneminusc + zs;
    let f02 = xz * oneminusc - ys;
    let f10 = xy * oneminusc - zs;
    let f11 = ay * ay * oneminusc + c;
    let f12 = yz * oneminusc + xs;
    let f20 = xz * oneminusc + ys;
    let f21 = yz * oneminusc - xs;
    let f22 = az * az * oneminusc + c;
    let mut d = *src;
    for r in 0..4 {
        let t0 = src[0][r] * f00 + src[1][r] * f01 + src[2][r] * f02;
        let t1 = src[0][r] * f10 + src[1][r] * f11 + src[2][r] * f12;
        let t2 = src[0][r] * f20 + src[1][r] * f21 + src[2][r] * f22;
        d[0][r] = t0;
        d[1][r] = t1;
        d[2][r] = t2;
    }
    d
}

/// LWJGL `Matrix4f.mul(left, right, dest)`.
fn mat_mul(l: &Mat4, r: &Mat4) -> Mat4 {
    let mut d = [[0f32; 4]; 4];
    for c in 0..4 {
        for row in 0..4 {
            d[c][row] = l[0][row] * r[c][0] + l[1][row] * r[c][1] + l[2][row] * r[c][2] + l[3][row] * r[c][3];
        }
    }
    d
}

/// `ModelRotation`: one of the 24 x/y quarter-turn combinations.
#[derive(Clone, Copy, Debug)]
pub struct ModelRotation {
    pub x: i32,
    pub y: i32,
    matrix: Mat4,
    quarters_x: i32,
    quarters_y: i32,
}

impl ModelRotation {
    /// `ModelRotation(x, y)`.
    pub fn new(x: i32, y: i32) -> ModelRotation {
        let mx = mat_rotate((-x) as f32 * 0.017453292, [1.0, 0.0, 0.0], &IDENTITY);
        let my = mat_rotate((-y) as f32 * 0.017453292, [0.0, 1.0, 0.0], &IDENTITY);
        ModelRotation { x, y, matrix: mat_mul(&my, &mx), quarters_x: (x / 90).abs(), quarters_y: (y / 90).abs() }
    }

    fn is_identity(&self) -> bool {
        self.x == 0 && self.y == 0
    }

    /// `ModelRotation.rotateFace`.
    pub fn rotate_face(&self, facing: Facing) -> Facing {
        let mut f = facing;
        for _ in 0..self.quarters_x {
            f = f.rotate_around(Axis::X);
        }
        if f.axis() != Axis::Y {
            for _ in 0..self.quarters_y {
                f = f.rotate_around(Axis::Y);
            }
        }
        f
    }

    /// `ModelRotation.rotateVertex`.
    pub fn rotate_vertex(&self, facing: Facing, vertex: usize) -> usize {
        let mut i = vertex;
        if facing.axis() == Axis::X {
            i = (vertex + self.quarters_x as usize) % 4;
        }
        let mut f = facing;
        for _ in 0..self.quarters_x {
            f = f.rotate_around(Axis::X);
        }
        if f.axis() == Axis::Y {
            i = (i + self.quarters_y as usize) % 4;
        }
        i
    }
}

/// `FaceBakery.UV_ROTATIONS` by facing and `ModelRotation.ordinal` (x / 90 * 4 + y / 90): 0 is
/// `UV_ROTATION_0`, 1 `UV_ROTATION_90`, 2 `UV_ROTATION_270`, 3 `UV_ROTATION_INVERSE`.
const UV_ROTATIONS: [[u8; 16]; 6] = [
    [0, 2, 3, 1, 0, 0, 0, 0, 0, 1, 3, 2, 3, 3, 3, 3],
    [0, 1, 3, 2, 3, 3, 3, 3, 0, 2, 3, 1, 0, 0, 0, 0],
    [0, 0, 0, 0, 3, 1, 0, 2, 3, 3, 3, 3, 3, 2, 0, 1],
    [0, 0, 0, 0, 0, 1, 3, 2, 3, 3, 3, 3, 0, 2, 3, 1],
    [0, 0, 0, 0, 2, 2, 2, 2, 3, 3, 3, 3, 1, 1, 1, 1],
    [0, 0, 0, 0, 1, 1, 1, 1, 3, 3, 3, 3, 2, 2, 2, 2],
];

/// `BlockFaceUV`.
#[derive(Clone, Copy, Debug)]
pub struct FaceUv {
    pub uvs: [f32; 4],
    pub rotation: i32,
}

impl FaceUv {
    fn rotated(&self, i: usize) -> usize {
        (i + (self.rotation / 90) as usize) % 4
    }

    /// `BlockFaceUV.getVertexRotatedRev`.
    fn rotated_rev(&self, i: usize) -> usize {
        (i + (4 - (self.rotation / 90) as usize)) % 4
    }

    /// `FaceBakery.applyUVLock`: the UVs a quad of `face` keeps when the variant turns by `rot`
    /// with `uvlock` set, so the texture stays put while the model turns.
    fn locked(&self, face: Facing, rot: &ModelRotation) -> FaceUv {
        let kind = UV_ROTATIONS[face as usize][((rot.x / 90) * 4 + rot.y / 90) as usize];
        let f = self.u(self.rotated_rev(0));
        let f1 = self.v(self.rotated_rev(0));
        let f2 = self.u(self.rotated_rev(2));
        let f3 = self.v(self.rotated_rev(2));
        match kind {
            0 => FaceUv { uvs: [f, f1, f2, f3], rotation: 0 },
            1 => FaceUv { uvs: [16.0 - f1, f2, 16.0 - f3, f], rotation: 90 },
            2 => FaceUv { uvs: [f3, 16.0 - f, f1, 16.0 - f2], rotation: 270 },
            _ => FaceUv { uvs: [16.0 - f, 16.0 - f1, 16.0 - f2, 16.0 - f3], rotation: 0 },
        }
    }

    /// `BlockFaceUV.getVertexU`.
    pub fn u(&self, i: usize) -> f32 {
        let r = self.rotated(i);
        self.uvs[if r != 0 && r != 1 { 2 } else { 0 }]
    }

    /// `BlockFaceUV.getVertexV`.
    pub fn v(&self, i: usize) -> f32 {
        let r = self.rotated(i);
        self.uvs[if r != 0 && r != 3 { 3 } else { 1 }]
    }
}

/// `BlockPartFace`.
#[derive(Clone, Debug)]
pub struct PartFace {
    pub cullface: Option<Facing>,
    pub tint: i32,
    pub texture: String,
    pub uv: FaceUv,
}

/// `BlockPartRotation`.
#[derive(Clone, Copy, Debug)]
pub struct PartRotation {
    /// Already scaled by 0.0625, as the deserializer stores it.
    pub origin: [f32; 3],
    pub axis: Axis,
    pub angle: f32,
    pub rescale: bool,
}

/// `BlockPart`: one element of a model.
#[derive(Clone, Debug)]
pub struct Part {
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// In `EnumMap` order, which is `EnumFacing` order.
    pub faces: Vec<(Facing, PartFace)>,
    pub rotation: Option<PartRotation>,
    pub shade: bool,
}

/// `BlockPart.getFaceUvs`: the UVs a face gets when its JSON has none.
fn default_uvs(f: Facing, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let ([fx, fy, fz], [tx, ty, tz]) = (from, to);
    match f {
        Facing::Down => [fx, 16.0 - tz, tx, 16.0 - fz],
        Facing::Up => [fx, fz, tx, tz],
        Facing::North => [16.0 - tx, 16.0 - ty, 16.0 - fx, 16.0 - fy],
        Facing::South => [fx, 16.0 - ty, tx, 16.0 - fy],
        Facing::West => [fz, 16.0 - ty, tz, 16.0 - fy],
        Facing::East => [16.0 - tz, 16.0 - ty, 16.0 - fz, 16.0 - fy],
    }
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

pub(crate) fn vec3(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
}

/// `BlockPart.Deserializer`.
fn parse_part(v: &Value) -> io::Result<Part> {
    let from = v.get("from").and_then(vec3).ok_or_else(|| bad("element without from".into()))?;
    let to = v.get("to").and_then(vec3).ok_or_else(|| bad("element without to".into()))?;
    let rotation = match v.get("rotation") {
        Some(r) => Some(parse_rotation(r)?),
        None => None,
    };
    let shade = v.get("shade").and_then(Value::as_bool).unwrap_or(true);
    let obj = v.get("faces").and_then(Value::as_object).ok_or_else(|| bad("element without faces".into()))?;
    let mut faces = Vec::new();
    for f in Facing::ALL {
        let Some(face) = obj.get(f.name()) else { continue };
        let uvs = match face.get("uv").and_then(Value::as_array) {
            Some(a) if a.len() == 4 => [0, 1, 2, 3].map(|i| a[i].as_f64().unwrap_or(0.0) as f32),
            _ => default_uvs(f, from, to),
        };
        faces.push((
            f,
            PartFace {
                cullface: face.get("cullface").and_then(Value::as_str).and_then(Facing::by_name),
                tint: face.get("tintindex").and_then(Value::as_i64).unwrap_or(-1) as i32,
                texture: face.get("texture").and_then(Value::as_str).ok_or_else(|| bad("face without texture".into()))?.to_string(),
                uv: FaceUv { uvs, rotation: face.get("rotation").and_then(Value::as_i64).unwrap_or(0) as i32 },
            },
        ));
    }
    Ok(Part { from, to, faces, rotation, shade })
}

/// `BlockPart.Deserializer.parseRotation`: the angle must be 0 or a multiple of 22.5 up to 45.
fn parse_rotation(v: &Value) -> io::Result<PartRotation> {
    let origin = v.get("origin").and_then(vec3).ok_or_else(|| bad("rotation without origin".into()))?;
    let axis = match v.get("axis").and_then(Value::as_str) {
        Some("x") => Axis::X,
        Some("y") => Axis::Y,
        Some("z") => Axis::Z,
        a => return Err(bad(format!("invalid rotation axis {a:?}"))),
    };
    let angle = v.get("angle").and_then(Value::as_f64).ok_or_else(|| bad("rotation without angle".into()))? as f32;
    if angle != 0.0 && angle.abs() != 22.5 && angle.abs() != 45.0 {
        return Err(bad(format!("invalid rotation {angle}")));
    }
    Ok(PartRotation {
        origin: origin.map(|c| c * 0.0625),
        axis,
        angle,
        rescale: v.get("rescale").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// `ModelBlock`, with its parent chain resolved into a list (child first).
pub struct ModelBlock {
    chain: Vec<ModelJson>,
    /// The `builtin/` parent the chain ends in (`builtin/generated`, `builtin/entity`).
    pub builtin: Option<String>,
}

struct ModelJson {
    ambient_occlusion: bool,
    textures: HashMap<String, String>,
    elements: Option<Vec<Part>>,
    /// `ItemCameraTransforms`: only the entries the file sets.
    display: HashMap<String, Display>,
}

impl ModelBlock {
    /// Loads `models/<name>.json` and its parents.
    pub fn load(jar: &Jar, name: &str) -> io::Result<ModelBlock> {
        let mut chain = Vec::new();
        let mut builtin = None;
        let mut next = Some(name.to_string());
        while let Some(n) = next.take() {
            if chain.len() > 16 {
                return Err(bad(format!("{name}: parent chain too long")));
            }
            let v = jar.json(&format!("models/{n}.json"))?;
            let elements = match v.get("elements").and_then(Value::as_array) {
                Some(list) => Some(list.iter().map(parse_part).collect::<io::Result<Vec<_>>>()?),
                None => None,
            };
            let textures = v
                .get("textures")
                .and_then(Value::as_object)
                .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
                .unwrap_or_default();
            let display = v
                .get("display")
                .and_then(Value::as_object)
                .map(|o| o.iter().map(|(k, v)| (k.clone(), Display::parse(v))).collect())
                .unwrap_or_default();
            chain.push(ModelJson {
                ambient_occlusion: v.get("ambientocclusion").and_then(Value::as_bool).unwrap_or(true),
                textures,
                elements,
                display,
            });
            next = v.get("parent").and_then(Value::as_str).filter(|p| !p.is_empty()).map(strip_namespace);
            // ModelBakery.MODEL_GENERATED and MODEL_ENTITY have no file.
            if next.as_deref().is_some_and(|p| p.starts_with("builtin/")) {
                builtin = next.take();
            }
        }
        Ok(ModelBlock { chain, builtin })
    }

    /// `ModelBlock.getTransform`: the nearest model in the chain that sets `key`.
    pub fn display(&self, key: &str) -> Option<Display> {
        self.chain.iter().find_map(|m| m.display.get(key).copied())
    }

    /// `ModelBlock.isTexturePresent`
    pub fn is_texture_present(&self, name: &str) -> bool {
        self.resolve_texture(name) != "missingno"
    }

    /// `ModelBlock.getElements`: the nearest model in the chain that has any.
    pub fn elements(&self) -> &[Part] {
        self.chain.iter().find_map(|m| m.elements.as_deref().filter(|e| !e.is_empty())).unwrap_or(&[])
    }

    /// `ModelBlock.isAmbientOcclusion`: the root of the chain decides.
    pub fn ambient_occlusion(&self) -> bool {
        self.chain.last().is_some_and(|m| m.ambient_occlusion)
    }

    /// `ModelBlock.resolveTextureName`: `#var` is looked up child first; a value that is itself
    /// a `#var` resolves again from the child. Unresolved names give "missingno".
    pub fn resolve_texture(&self, name: &str) -> String {
        let mut key = name.strip_prefix('#').unwrap_or(name).to_string();
        for _ in 0..16 {
            let Some(v) = self.chain.iter().find_map(|m| m.textures.get(&key)) else { return "missingno".into() };
            match v.strip_prefix('#') {
                Some(k) => key = k.to_string(),
                None => return strip_namespace(v),
            }
        }
        "missingno".into()
    }
}

fn strip_namespace(s: &str) -> String {
    s.strip_prefix("minecraft:").unwrap_or(s).to_string()
}

/// A baked quad: `BakedQuad` with the vertex data unpacked.
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub pos: [[f32; 3]; 4],
    /// Sprite-relative UV, `getVertexU / 16`.
    pub uv: [[f32; 2]; 4],
    /// The baked color byte: `getFaceShadeColor` of the rotated face, or 255 for unshaded parts.
    pub shade: u8,
    pub tint: i32,
    /// `getFacingFromVertexData`.
    pub face: Facing,
    pub sprite: SpriteInfo,
}

/// `FaceBakery.getFaceBrightness`.
pub fn face_brightness(f: Facing) -> f32 {
    match f {
        Facing::Down => 0.5,
        Facing::Up => 1.0,
        Facing::North | Facing::South => 0.8,
        Facing::West | Facing::East => 0.6,
    }
}

/// `FaceBakery.getFaceShadeColor`, the low byte.
fn face_shade(f: Facing) -> u8 {
    ((face_brightness(f) * 255.0) as i32).clamp(0, 255) as u8
}

/// `FaceBakery.getFacingFromVertexData`.
pub fn facing_from_vertices(p: &[[f32; 3]; 4]) -> Facing {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let v3 = sub(p[0], p[1]);
    let v4 = sub(p[2], p[1]);
    // Vector3f.cross(v4, v3)
    let mut n = [v4[1] * v3[2] - v4[2] * v3[1], v4[2] * v3[0] - v4[0] * v3[2], v4[0] * v3[1] - v4[1] * v3[0]];
    let len = ((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) as f64).sqrt() as f32;
    for c in &mut n {
        *c /= len;
    }
    let mut best = None;
    let mut f1 = 0.0f32;
    for f in Facing::ALL {
        let d = f.vec();
        let f2 = n[0] * d[0] as f32 + n[1] * d[1] as f32 + n[2] * d[2] as f32;
        if f2 >= 0.0 && f2 > f1 {
            f1 = f2;
            best = Some(f);
        }
    }
    best.unwrap_or(Facing::Up)
}

/// `FaceBakery.SCALE_ROTATION_22_5`
fn scale_22_5() -> f32 {
    1.0 / (0.39269909262657166f64.cos() as f32) - 1.0
}

/// `FaceBakery.SCALE_ROTATION_GENERAL`
fn scale_general() -> f32 {
    1.0 / (0.7853981852531433f64.cos() as f32) - 1.0
}

/// `FaceBakery.rotateScale`: `Matrix4f.transform` of `(p - origin, 1)`, scaled per axis, plus
/// `origin`.
fn rotate_scale(p: [f32; 3], origin: [f32; 3], m: &Mat4, scale: [f32; 3]) -> [f32; 3] {
    let (x, y, z) = (p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]);
    let tx = m[0][0] * x + m[1][0] * y + m[2][0] * z + m[3][0];
    let ty = m[0][1] * x + m[1][1] * y + m[2][1] * z + m[3][1];
    let tz = m[0][2] * x + m[1][2] * y + m[2][2] * z + m[3][2];
    [tx * scale[0] + origin[0], ty * scale[1] + origin[1], tz * scale[2] + origin[2]]
}

/// `FaceBakery.rotatePart`
fn rotate_part(p: [f32; 3], r: &PartRotation) -> [f32; 3] {
    let (axis, mut scale) = match r.axis {
        Axis::X => ([1.0, 0.0, 0.0], [0.0, 1.0, 1.0]),
        Axis::Y => ([0.0, 1.0, 0.0], [1.0, 0.0, 1.0]),
        Axis::Z => ([0.0, 0.0, 1.0], [1.0, 1.0, 0.0]),
    };
    let m = mat_rotate(r.angle * 0.017453292, axis, &IDENTITY);
    if r.rescale {
        let s = if r.angle.abs() == 22.5 { scale_22_5() } else { scale_general() };
        scale = scale.map(|c: f32| c * s + 1.0);
    } else {
        scale = [1.0, 1.0, 1.0];
    }
    rotate_scale(p, r.origin, &m, scale)
}

/// `FaceBakery.makeBakedQuad`.
pub fn bake_quad(part: &Part, face: Facing, pf: &PartFace, sprite: SpriteInfo, rot: &ModelRotation) -> Quad {
    bake_quad_locked(part, face, pf, sprite, rot, false)
}

/// `FaceBakery.makeBakedQuad` with `uvLocked`.
pub fn bake_quad_locked(part: &Part, face: Facing, pf: &PartFace, sprite: SpriteInfo, rot: &ModelRotation, uv_lock: bool) -> Quad {
    let face_uv = if uv_lock { pf.uv.locked(face, rot) } else { pf.uv };
    // getPositionsDiv16, indexed by facing.
    let p = [
        part.from[1] / 16.0,
        part.to[1] / 16.0,
        part.from[2] / 16.0,
        part.to[2] / 16.0,
        part.from[0] / 16.0,
        part.to[0] / 16.0,
    ];
    let shade = if part.shade { face_shade(rot.rotate_face(face)) } else { 255 };
    let mut pos = [[0f32; 3]; 4];
    let mut uv = [[0f32; 2]; 4];
    // makeQuadVertexData / fillVertexData
    for i in 0..4 {
        let vi = FACE_DIRECTION[face as usize][i];
        let mut v = [p[vi[0]], p[vi[1]], p[vi[2]]];
        if let Some(r) = &part.rotation {
            v = rotate_part(v, r);
        }
        // rotateVertex
        let j = if rot.is_identity() {
            i
        } else {
            v = rotate_scale(v, [0.5, 0.5, 0.5], &rot.matrix, [1.0, 1.0, 1.0]);
            rot.rotate_vertex(face, i)
        };
        pos[j] = v;
        uv[j] = [face_uv.u(i) / 16.0, face_uv.v(i) / 16.0];
    }
    let facing = facing_from_vertices(&pos);
    if part.rotation.is_none() {
        apply_facing(&mut pos, &mut uv, facing);
    }
    Quad { pos, uv, shade, tint: pf.tint, face: facing, sprite }
}

/// `FaceBakery.applyFacing`: reorders the corners to the facing's canonical order, carrying each
/// corner's UV along.
fn apply_facing(pos: &mut [[f32; 3]; 4], uv: &mut [[f32; 2]; 4], facing: Facing) {
    let (old_pos, old_uv) = (*pos, *uv);
    let mut b = [999.0f32, -999.0, 999.0, -999.0, 999.0, -999.0];
    for v in &old_pos {
        b[4] = if v[0] < b[4] { v[0] } else { b[4] };
        b[0] = if v[1] < b[0] { v[1] } else { b[0] };
        b[2] = if v[2] < b[2] { v[2] } else { b[2] };
        b[5] = if v[0] > b[5] { v[0] } else { b[5] };
        b[1] = if v[1] > b[1] { v[1] } else { b[1] };
        b[3] = if v[2] > b[3] { v[2] } else { b[3] };
    }
    let eq = |a: f32, b: f32| (b - a).abs() < 1.0e-5;
    for i in 0..4 {
        let vi = FACE_DIRECTION[facing as usize][i];
        let n = [b[vi[0]], b[vi[1]], b[vi[2]]];
        pos[i] = n;
        for k in 0..4 {
            let o = old_pos[k];
            if eq(n[0], o[0]) && eq(n[1], o[1]) && eq(n[2], o[2]) {
                uv[i] = old_uv[k];
            }
        }
    }
}

/// `SimpleBakedModel`: quads that cull against a side, and quads that never cull.
#[derive(Clone, Debug, Default)]
pub struct BakedModel {
    pub face_quads: [Vec<Quad>; 6],
    pub general: Vec<Quad>,
    pub ambient_occlusion: bool,
    /// `getParticleTexture`: the model's `#particle` sprite, for digging particles.
    pub particle: Option<SpriteInfo>,
}

/// `ModelBakery.bakeModel`.
pub fn bake_model(model: &ModelBlock, rot: &ModelRotation, sprite: &dyn Fn(&str) -> io::Result<SpriteInfo>) -> io::Result<BakedModel> {
    bake_model_locked(model, rot, false, sprite)
}

/// `ModelBakery.bakeModel(model, rotation, uvLocked)`.
pub fn bake_model_locked(model: &ModelBlock, rot: &ModelRotation, uv_lock: bool, sprite: &dyn Fn(&str) -> io::Result<SpriteInfo>) -> io::Result<BakedModel> {
    let mut out = BakedModel { ambient_occlusion: model.ambient_occlusion(), ..Default::default() };
    if model.is_texture_present("particle") {
        out.particle = sprite(&model.resolve_texture("particle")).ok();
    }
    for part in model.elements() {
        for (f, pf) in &part.faces {
            let s = sprite(&model.resolve_texture(&pf.texture))?;
            let q = bake_quad_locked(part, *f, pf, s, rot, uv_lock);
            match pf.cullface {
                None => out.general.push(q),
                Some(c) => out.face_quads[rot.rotate_face(c) as usize].push(q),
            }
        }
    }
    Ok(out)
}

/// `WeightedBakedModel`, or one model when the variant list has one entry.
#[derive(Clone, Debug)]
pub struct BlockModel {
    /// Sorted by weight, heaviest first (stable), as `WeightedBakedModel.Builder.build` sorts.
    pub variants: Vec<(BakedModel, u32)>,
    pub total_weight: u32,
}

impl BlockModel {
    /// `WeightedBakedModel.getRandomModel` with `MathHelper.getPositionRandom(pos)`.
    pub fn pick(&self, x: i32, y: i32, z: i32) -> &BakedModel {
        self.pick_rand(coordinate_random(x, y, z))
    }

    /// `WeightedBakedModel.getRandomModel(rand)`: `Math.abs((int) rand >> 16) % totalWeight`.
    pub fn pick_rand(&self, rand: i64) -> &BakedModel {
        if self.variants.len() == 1 {
            return &self.variants[0].0;
        }
        let mut w = ((rand as i32) >> 16).wrapping_abs() % self.total_weight as i32;
        for (m, weight) in &self.variants {
            w -= *weight as i32;
            if w < 0 {
                return m;
            }
        }
        &self.variants[0].0
    }
}

/// `MathHelper.getCoordinateRandom`, in `sim::region` beside `Block.getOffset`.
pub use sim::region::coordinate_random;

/// `ICondition`: the `when` of a multipart selector, over the block's property strings.
#[derive(Clone, Debug)]
pub enum Condition {
    /// `ICondition.TRUE`: a selector without `when`.
    True,
    /// `ConditionPropertyValue`: the value may be `!`-negated and hold `|` alternatives.
    Property { key: String, value: String },
    /// `ConditionAnd`
    And(Vec<Condition>),
    /// `ConditionOr`
    Or(Vec<Condition>),
}

impl Condition {
    /// `Selector.Deserializer.getOrAndCondition`.
    pub fn parse(json: &Value) -> io::Result<Condition> {
        let obj = json.as_object().ok_or_else(|| bad("multipart when is not an object".into()))?;
        if obj.is_empty() {
            return Err(bad("No elements found in selector".into()));
        }
        let list = |v: &Value| -> io::Result<Vec<Condition>> {
            v.as_array().ok_or_else(|| bad("OR / AND is not an array".into()))?.iter().map(Condition::parse).collect()
        };
        if obj.len() == 1 {
            if let Some(v) = obj.get("OR") {
                return Ok(Condition::Or(list(v)?));
            }
            if let Some(v) = obj.get("AND") {
                return Ok(Condition::And(list(v)?));
            }
        }
        let props: Vec<Condition> = obj.iter().map(|(k, v)| Condition::property(k, v)).collect::<io::Result<_>>()?;
        Ok(if props.len() == 1 { props.into_iter().next().expect("one property") } else { Condition::And(props) })
    }

    /// `Selector.Deserializer.makePropertyValue`: `getAsString` of a string, boolean or number.
    fn property(key: &str, v: &Value) -> io::Result<Condition> {
        let value = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            _ => return Err(bad(format!("multipart condition {key} has no scalar value"))),
        };
        Ok(Condition::Property { key: key.into(), value })
    }

    /// `ConditionPropertyValue.getPredicate`'s checks that the block has the property and the
    /// value is not empty.
    // DROPPED: the "unknown value" check of `makePredicate`; a value the property lacks never matches.
    pub fn validate(&self, names: &[&str]) -> io::Result<()> {
        match self {
            Condition::True => Ok(()),
            Condition::Property { key, value } => {
                if !names.contains(&key.as_str()) {
                    return Err(bad(format!("multipart condition {key}={value}: block has no property {key}")));
                }
                if value.trim_start_matches('!').split('|').all(str::is_empty) {
                    return Err(bad(format!("multipart condition {key}: has an empty value")));
                }
                Ok(())
            }
            Condition::And(c) | Condition::Or(c) => c.iter().try_for_each(|c| c.validate(names)),
        }
    }

    /// `ICondition.getPredicate(...).apply(state)`; `props` are the state's `name`, `value` strings.
    pub fn matches(&self, props: &[(&str, &str)]) -> bool {
        match self {
            Condition::True => true,
            Condition::Property { key, value } => {
                let (negate, v) = match value.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, value.as_str()),
                };
                let have = props.iter().find(|(k, _)| k == key).map(|(_, v)| *v);
                // Splitter.on('|').omitEmptyStrings()
                let hit = have.is_some_and(|h| v.split('|').filter(|a| !a.is_empty()).any(|a| a == h));
                hit != negate
            }
            Condition::And(c) => c.iter().all(|c| c.matches(props)),
            Condition::Or(c) => c.iter().any(|c| c.matches(props)),
        }
    }
}

/// `Selector`: a condition and the variant list it applies.
#[derive(Clone, Debug)]
pub struct Selector {
    pub condition: Condition,
    pub model: BlockModel,
}

/// `MultipartBakedModel`: the selectors in file order.
#[derive(Clone, Debug)]
pub struct Multipart {
    pub selectors: Vec<Selector>,
    /// The first selector's model decides (`MultipartBakedModel` constructor).
    pub ambient_occlusion: bool,
}

/// The models a multipart state draws, in selector order.
pub struct MultipartPick<'a> {
    pub models: Vec<&'a BakedModel>,
    pub ambient_occlusion: bool,
}

impl Multipart {
    /// `MultipartBakedModel.getQuads`: every selector whose condition matches contributes its
    /// model, and each one takes the next `rand` (`rand++`).
    pub fn pick(&self, props: &[(&str, &str)], rand: i64) -> MultipartPick<'_> {
        let mut rand = rand;
        let mut models = Vec::new();
        for s in &self.selectors {
            if s.condition.matches(props) {
                models.push(s.model.pick_rand(rand));
                rand = rand.wrapping_add(1);
            }
        }
        MultipartPick { models, ambient_occlusion: self.ambient_occlusion }
    }
}

/// One `Variant` of a variant list, before baking.
struct VariantSpec {
    name: String,
    rot: ModelRotation,
    uv_lock: bool,
    weight: u32,
}

/// `VariantList.Deserializer`: the entries of a variant list (one object or an array), loading
/// each model into `cache`. `strict` fails on a bad entry; otherwise the entry is skipped.
fn variant_specs(jar: &Jar, cache: &mut HashMap<String, ModelBlock>, file: &str, list: &Value, strict: bool) -> io::Result<Vec<VariantSpec>> {
    let entries: Vec<&Value> = match list {
        Value::Array(a) => a.iter().collect(),
        v => vec![v],
    };
    let mut out = Vec::new();
    for e in entries {
        let Some(name) = e.get("model").and_then(Value::as_str) else {
            if strict {
                return Err(bad(format!("{file}: variant without model")));
            }
            continue;
        };
        // Variant.Deserializer: models live under block/.
        let name = format!("block/{}", strip_namespace(name));
        let x = e.get("x").and_then(Value::as_i64).unwrap_or(0) as i32;
        let y = e.get("y").and_then(Value::as_i64).unwrap_or(0) as i32;
        let uv_lock = e.get("uvlock").and_then(Value::as_bool).unwrap_or(false);
        let weight = e.get("weight").and_then(Value::as_u64).unwrap_or(1) as u32;
        if !cache.contains_key(&name) {
            match ModelBlock::load(jar, &name) {
                Ok(m) => {
                    cache.insert(name.clone(), m);
                }
                Err(err) if strict => return Err(err),
                Err(_) => continue,
            }
        }
        out.push(VariantSpec { name, rot: ModelRotation::new(x.rem_euclid(360), y.rem_euclid(360)), uv_lock, weight: weight.max(1) });
    }
    Ok(out)
}

/// `ModelBakery.bakeModels` for one variant list.
fn bake_variants(specs: &[VariantSpec], cache: &HashMap<String, ModelBlock>, sprite: &dyn Fn(&str) -> io::Result<SpriteInfo>) -> io::Result<BlockModel> {
    let mut baked = Vec::new();
    for v in specs {
        baked.push((bake_model_locked(&cache[&v.name], &v.rot, v.uv_lock, sprite)?, v.weight));
    }
    baked.sort_by(|a, b| b.1.cmp(&a.1));
    let total_weight = baked.iter().map(|b| b.1).sum();
    Ok(BlockModel { variants: baked, total_weight })
}

/// Every block model and sprite the viewer draws.
pub struct Models {
    /// `(state << 1 | flag)` to `models` index + 1; 0 means the state has no model. `flag` is the
    /// property `getActualState` adds (see [`blockstate_of`]).
    table: Vec<u16>,
    pub models: Vec<BlockModel>,
    /// State to `multiparts` index + 1, for the blocks whose blockstate is `multipart`.
    mp_table: Vec<u16>,
    pub multiparts: Vec<Multipart>,
    pub textures: BlockTextures,
    pub water_still: SpriteInfo,
    pub water_flow: SpriteInfo,
    pub lava_still: SpriteInfo,
    pub lava_flow: SpriteInfo,
    /// The block items the hotbar can hold, by block and metadata (item.rs).
    pub items: HashMap<(u16, u16), ItemModel>,
}

const FLUID_SPRITES: [&str; 4] = ["blocks/water_still", "blocks/water_flow", "blocks/lava_still", "blocks/lava_flow"];

impl Models {
    pub fn load(jar: &Jar) -> io::Result<Models> {
        // Parse every variant list and model first, so the sprite set is known before baking.
        let mut model_cache: HashMap<String, ModelBlock> = HashMap::new();
        let mut keyed: Vec<(usize, Vec<VariantSpec>)> = Vec::new();
        let mut variant_lists: HashMap<(String, String), usize> = HashMap::new();
        let mut table = vec![0u16; 1 << 17];
        for s in generated_states() {
            for flag in [false, true] {
                let (file, key) = blockstate_of(s, flag).expect("generated state has a blockstate");
                let n = variant_lists.len();
                let idx = *variant_lists.entry((file.clone(), key.clone())).or_insert(n);
                if idx == n {
                    let bs = jar.json(&format!("blockstates/{file}.json"))?;
                    let list = bs
                        .get("variants")
                        .and_then(|v| v.get(&key))
                        .ok_or_else(|| bad(format!("blockstates/{file}.json has no variant {key}")))?;
                    keyed.push((idx, variant_specs(jar, &mut model_cache, &file, list, true)?));
                }
                table[((s as usize) << 1) | flag as usize] = idx as u16 + 1;
            }
        }
        // Every other state with a model (a block the player places that generation never does):
        // `default_blockstate`, skipped when its file or variant is missing.
        let mut files: HashMap<String, Option<Value>> = HashMap::new();
        for s in 0..(256u16 << 4) {
            if table[(s as usize) << 1] != 0 {
                continue;
            }
            let Some((file, key)) = default_blockstate(s) else { continue };
            let n = variant_lists.len();
            let idx = match variant_lists.get(&(file.clone(), key.clone())) {
                Some(&i) => i,
                None => {
                    let bs = files.entry(file.clone()).or_insert_with(|| jar.json(&format!("blockstates/{file}.json")).ok());
                    let Some(list) = bs.as_ref().and_then(|b| b.get("variants")).and_then(|v| v.get(&key)) else { continue };
                    let variants = variant_specs(jar, &mut model_cache, &file, list, false)?;
                    if variants.is_empty() {
                        continue;
                    }
                    variant_lists.insert((file, key), n);
                    keyed.push((n, variants));
                    n
                }
            };
            table[(s as usize) << 1] = idx as u16 + 1;
            table[((s as usize) << 1) | 1] = idx as u16 + 1;
        }

        // The multipart blockstates (fences, panes, walls, bars, fire, redstone wire, chorus
        // plant, brewing stand): one selector list per file.
        let mut mp_table = vec![0u16; 1 << 16];
        let mut mp_files: HashMap<String, usize> = HashMap::new();
        let mut mp_specs: Vec<Vec<(Condition, Vec<VariantSpec>)>> = Vec::new();
        for s in 0..(256u16 << 4) {
            if !is_multipart_block(s >> 4) {
                continue;
            }
            let Some(file) = multipart_file(s) else { continue };
            let idx = match mp_files.get(&file) {
                Some(&i) => i,
                None => {
                    let bs = jar.json(&format!("blockstates/{file}.json"))?;
                    let parts = bs
                        .get("multipart")
                        .and_then(Value::as_array)
                        .ok_or_else(|| bad(format!("blockstates/{file}.json has no multipart")))?;
                    let names: Vec<&str> = multipart_props(s >> 4, 0).iter().map(|p| p.0).collect();
                    let mut selectors = Vec::new();
                    for part in parts {
                        let condition = match part.get("when") {
                            Some(w) => Condition::parse(w)?,
                            None => Condition::True,
                        };
                        condition.validate(&names)?;
                        let apply = part.get("apply").ok_or_else(|| bad(format!("{file}: selector without apply")))?;
                        selectors.push((condition, variant_specs(jar, &mut model_cache, &file, apply, true)?));
                    }
                    mp_specs.push(selectors);
                    mp_files.insert(file, mp_specs.len() - 1);
                    mp_specs.len() - 1
                }
            };
            mp_table[s as usize] = idx as u16 + 1;
        }

        let item_sources = item::load_sources(jar)?;
        let mut names: Vec<String> = model_cache
            .values()
            .flat_map(|m| m.elements().iter().flat_map(|p| p.faces.iter().map(|(_, f)| m.resolve_texture(&f.texture))).collect::<Vec<_>>())
            .collect();
        names.extend(model_cache.values().filter(|m| m.is_texture_present("particle")).map(|m| m.resolve_texture("particle")));
        names.extend(FLUID_SPRITES.iter().map(|s| s.to_string()));
        names.extend(crate::textures::DESTROY_SPRITES.iter().map(|s| s.to_string()));
        names.extend(item::sprite_names(&item_sources));
        names.sort();
        names.dedup();
        let sprites: Vec<(String, Sprite)> =
            names.into_iter().map(|n| load_sprite(jar, &n).map(|s| (n, s))).collect::<io::Result<_>>()?;
        let textures = BlockTextures::new(sprites);
        let sprite = |n: &str| textures.get(n).ok_or_else(|| bad(format!("no sprite {n}")));

        keyed.sort_by_key(|(i, _)| *i);
        let mut models = Vec::new();
        for (_, variants) in keyed {
            models.push(bake_variants(&variants, &model_cache, &sprite)?);
        }
        let mut multiparts = Vec::new();
        for selectors in mp_specs {
            let mut baked = Vec::new();
            for (condition, specs) in selectors {
                baked.push(Selector { condition, model: bake_variants(&specs, &model_cache, &sprite)? });
            }
            let ambient_occlusion = baked.first().is_some_and(|s| s.model.variants[0].0.ambient_occlusion);
            multiparts.push(Multipart { selectors: baked, ambient_occlusion });
        }
        let [ws, wf, ls, lf] = FLUID_SPRITES.map(|n| textures.get(n).expect("fluid sprite loaded"));
        let items = item::bake_items(&item_sources, &textures)?;
        Ok(Models { table, models, mp_table, multiparts, textures, water_still: ws, water_flow: wf, lava_still: ls, lava_flow: lf, items })
    }

    /// The digging particle sprite of a state: its first variant's `#particle`.
    pub fn particle(&self, state: u16) -> Option<SpriteInfo> {
        if let Some(mp) = self.multipart(state) {
            return mp.selectors.first().and_then(|s| s.model.variants.first()).and_then(|v| v.0.particle);
        }
        self.get(state, false).and_then(|m| m.variants.first()).and_then(|v| v.0.particle)
    }

    pub fn get(&self, state: u16, flag: bool) -> Option<&BlockModel> {
        match self.table.get(((state as usize) << 1) | flag as usize).copied().unwrap_or(0) {
            0 => None,
            i => Some(&self.models[i as usize - 1]),
        }
    }

    fn multipart(&self, state: u16) -> Option<&Multipart> {
        match self.mp_table[state as usize] {
            0 => None,
            i => Some(&self.multiparts[i as usize - 1]),
        }
    }

    /// Whether the state's blockstate is `multipart`, drawn through [`Models::get_multipart`].
    pub fn is_multipart(&self, state: u16) -> bool {
        self.mp_table[state as usize] != 0
    }

    /// The models of a multipart state whose `getActualState` properties are `mask`
    /// ([`multipart_props`]); `rand` is `MathHelper.getPositionRandom(pos)`.
    pub fn get_multipart(&self, state: u16, mask: u16, rand: i64) -> Option<MultipartPick<'_>> {
        let mp = self.multipart(state)?;
        Some(mp.pick(&multipart_props(state >> 4, mask), rand))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite() -> SpriteInfo {
        SpriteInfo { layer: 0, frames: 1, frametime: 1 }
    }

    fn cube_part() -> Part {
        let faces = Facing::ALL
            .iter()
            .map(|&f| {
                let uv = FaceUv { uvs: default_uvs(f, [0.0; 3], [16.0; 3]), rotation: 0 };
                (f, PartFace { cullface: Some(f), tint: -1, texture: "#all".into(), uv })
            })
            .collect();
        Part { from: [0.0; 3], to: [16.0; 3], faces, rotation: None, shade: true }
    }

    #[test]
    fn cross_planes_turn_45_degrees_and_stretch() {
        // cross.json's first element: from (0.8, 0, 8) to (15.2, 16, 8), y 45 about the center,
        // rescaled.
        let uv = FaceUv { uvs: [0.0, 0.0, 16.0, 16.0], rotation: 0 };
        let r = PartRotation { origin: [0.5; 3], axis: Axis::Y, angle: 45.0, rescale: true };
        let part = Part {
            from: [0.8, 0.0, 8.0],
            to: [15.2, 16.0, 8.0],
            faces: vec![(Facing::North, PartFace { cullface: None, tint: -1, texture: "#cross".into(), uv })],
            rotation: Some(r),
            shade: false,
        };
        let (f, pf) = &part.faces[0];
        let q = bake_quad(&part, *f, pf, sprite(), &ModelRotation::new(0, 0));
        // Turned onto the diagonal and stretched by 1 / cos 45: each corner sits 0.45 from the
        // center on both x and z.
        for p in q.pos {
            assert!((p[0] - 0.5).abs() > 0.44 && (p[0] - 0.5).abs() < 0.46, "{p:?}");
            assert!(((p[0] - 0.5).abs() - (p[2] - 0.5).abs()).abs() < 1e-5, "{p:?}");
        }
        assert_eq!(q.shade, 255);
        // No applyFacing: the corners stay in vertex order.
        assert_eq!(q.pos[0][1], 1.0);
        assert_eq!(q.pos[1][1], 0.0);
    }

    #[test]
    fn cube_faces_keep_vanilla_corner_order() {
        let part = cube_part();
        let rot = ModelRotation::new(0, 0);
        let (f, pf) = &part.faces[1];
        let q = bake_quad(&part, *f, pf, sprite(), &rot);
        assert_eq!(q.face, Facing::Up);
        assert_eq!(q.pos, [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]]);
        assert_eq!(q.uv, [[0., 0.], [0., 1.], [1., 1.], [1., 0.]]);
        assert_eq!(q.shade, 255);
        let (f, pf) = &part.faces[2];
        let q = bake_quad(&part, *f, pf, sprite(), &rot);
        assert_eq!(q.face, Facing::North);
        assert_eq!(q.pos, [[1., 1., 0.], [1., 0., 0.], [0., 0., 0.], [0., 1., 0.]]);
        // getVertexU(0) = uvs[0] = 16 - 16, getVertexV(0) = uvs[1] = 16 - 16.
        assert_eq!(q.uv[0], [0., 0.]);
        assert_eq!(q.uv[2], [1., 1.]);
        assert_eq!(q.shade, 204);
        let shades: Vec<u8> = part.faces.iter().map(|(f, pf)| bake_quad(&part, *f, pf, sprite(), &rot).shade).collect();
        assert_eq!(shades, vec![127, 255, 204, 204, 153, 153]);
    }

    #[test]
    fn every_quad_winds_outward() {
        let part = cube_part();
        for (x, y) in [(0, 0), (0, 90), (0, 180), (0, 270), (90, 0), (180, 90)] {
            let rot = ModelRotation::new(x, y);
            for (f, pf) in &part.faces {
                let q = bake_quad(&part, *f, pf, sprite(), &rot);
                assert_eq!(q.face, rot.rotate_face(*f), "x{x} y{y} {f:?}");
                let [a, b, c, _] = q.pos;
                let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let d = q.face.vec();
                assert!(n[0] * d[0] as f32 + n[1] * d[1] as f32 + n[2] * d[2] as f32 > 0.5, "x{x} y{y} {f:?}");
            }
        }
    }

    #[test]
    fn y_rotation_turns_the_top_texture() {
        let part = cube_part();
        let (f, pf) = &part.faces[1];
        let q0 = bake_quad(&part, *f, pf, sprite(), &ModelRotation::new(0, 0));
        let q90 = bake_quad(&part, *f, pf, sprite(), &ModelRotation::new(0, 90));
        // Same corners after applyFacing; the UVs move one corner along.
        for i in 0..4 {
            for c in 0..3 {
                assert!((q0.pos[i][c] - q90.pos[i][c]).abs() < 1e-6);
            }
        }
        assert_ne!(q0.uv, q90.uv);
        let mut rotated = q0.uv;
        rotated.rotate_left(1);
        assert!(q90.uv == rotated || {
            let mut r = q0.uv;
            r.rotate_right(1);
            q90.uv == r
        });
    }

    #[test]
    fn coordinate_random_matches_java() {
        // long i = (long)(x * 3129871) ^ (long)z * 116129781L ^ (long)y; i = i * i * 42317861L + i * 11L;
        assert_eq!(coordinate_random(0, 0, 0), 0);
        let x = 1000i32;
        let ix = x.wrapping_mul(3129871) as i64;
        assert_eq!(coordinate_random(x, 0, 0), ix.wrapping_mul(ix).wrapping_mul(42317861).wrapping_add(ix * 11));
    }

    #[test]
    fn weighted_pick_uses_the_high_bits() {
        let m = BakedModel::default();
        let bm = BlockModel { variants: (0..4).map(|_| (m.clone(), 1)).collect(), total_weight: 4 };
        for (x, y, z) in [(0, 64, 0), (5, 70, -3), (-100, 12, 999)] {
            let r = coordinate_random(x, y, z);
            let want = (((r as i32) >> 16).abs() % 4) as usize;
            let got = bm.variants.iter().position(|(v, _)| std::ptr::eq(v, bm.pick(x, y, z))).unwrap();
            assert_eq!(got, want);
        }
    }

    #[test]
    fn bakes_jar_models() {
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        let stone = models.get(worldgen::blocks::STONE, false).unwrap();
        assert_eq!(stone.variants.len(), 4);
        assert!(stone.variants.iter().all(|(m, _)| m.ambient_occlusion && m.general.is_empty()));
        let grass = models.get(worldgen::blocks::GRASS, false).unwrap();
        // Two elements: the block and the side overlay.
        assert_eq!(grass.variants[0].0.face_quads[Facing::North as usize].len(), 2);
        assert_eq!(grass.variants[0].0.face_quads[Facing::Up as usize][0].tint, 0);
        let lily = models.get(worldgen::blocks::WATERLILY, false).unwrap();
        assert!(!lily.variants[0].0.ambient_occlusion);
        assert_eq!(lily.variants[0].0.general.len(), 2);
        for c in 0..16 {
            assert!(models.get(worldgen::blocks::stained_hardened_clay(c), false).is_some());
        }
        assert_eq!(models.water_still.frames, 32);
        for s in generated_states() {
            assert!(models.get(s, false).is_some() && models.get(s, true).is_some(), "{}:{}", s >> 4, s & 15);
        }
        // tall_grass: tinted_cross, two rotated planes with two faces each.
        let grass = models.get(worldgen::blocks::state(worldgen::blocks::id::TALLGRASS, 1), false).unwrap();
        assert_eq!(grass.variants[0].0.general.len(), 4);
        assert!(grass.variants[0].0.general.iter().all(|q| q.tint == 0 && q.shade == 255));
    }

    #[test]
    fn the_farming_blocks_resolve_to_drawn_models() {
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        use worldgen::blocks::{id, state};
        // Crop ages: wheat, carrots and potatoes 0 to 7, beetroots 0 to 3; farmland's moisture 0 to 7.
        let cases = [(id::WHEAT, 8), (id::CARROTS, 8), (id::POTATOES, 8), (id::BEETROOTS, 4), (id::FARMLAND, 8)];
        for (block, count) in cases {
            for meta in 0..count {
                let s = state(block, meta);
                let m = models.get(s, false).unwrap_or_else(|| panic!("block {block}:{meta} has no model"));
                let b = &m.variants[0].0;
                let quads = b.general.len() + b.face_quads.iter().map(|q| q.len()).sum::<usize>();
                assert!(quads > 0, "block {block}:{meta} bakes no quads");
            }
        }
    }

    /// A block with a model renders; one with none is invisible while its neighbors cull
    /// against it (the x-ray sponge), so every Model state of every block has a baked model, a
    /// multipart one included.
    /// DROPPED: the metas the block's `getStateFromMeta` never produces in the world.
    #[test]
    fn every_model_state_has_a_baked_model() {
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let models = Models::load(&jar).expect("models");
        let unreachable = |b: u16, m: u16| match b {
            3 => m >= 3,
            5 | 125 => m >= 6,
            126 | 175 => matches!(m & 7, 6 | 7),
            181 | 182 => m & 7 >= 1,
            _ => false,
        };
        let mut missing = Vec::new();
        for s in 0..(256u16 << 4) {
            let (b, m) = (s >> 4, s & 15);
            if sim::states::render_type(s) == sim::states::RenderType::Model
                && !unreachable(b, m)
                && models.get(s, false).is_none()
                && models.get_multipart(s, 0, 0).is_none()
            {
                missing.push(format!("{b}:{m} {}", sim::states::block_name(b).unwrap_or("?")));
            }
        }
        assert!(missing.is_empty(), "no model: {missing:?}");
    }

    #[test]
    fn conditions_read_when_like_vanilla() {
        let json = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        let p = [("north", "true"), ("east", "false"), ("wire", "side")];
        let c = |s: &str| Condition::parse(&json(s)).unwrap();
        // ConditionPropertyValue: a boolean in the file is the string "true".
        assert!(c(r#"{"north": true}"#).matches(&p));
        assert!(c(r#"{"north": "true"}"#).matches(&p));
        assert!(!c(r#"{"east": true}"#).matches(&p));
        // Several keys are ConditionAnd.
        assert!(c(r#"{"north": true, "east": false}"#).matches(&p));
        assert!(!c(r#"{"north": true, "east": true}"#).matches(&p));
        // OR and AND lists of conditions.
        assert!(c(r#"{"OR": [{"east": true}, {"north": true}]}"#).matches(&p));
        assert!(!c(r#"{"AND": [{"east": true}, {"north": true}]}"#).matches(&p));
        // `|` alternatives and `!` negation, over the whole alternative list.
        assert!(c(r#"{"wire": "side|up"}"#).matches(&p));
        assert!(!c(r#"{"wire": "none|up"}"#).matches(&p));
        assert!(c(r#"{"wire": "!none|up"}"#).matches(&p));
        assert!(!c(r#"{"wire": "!side|up"}"#).matches(&p));
        assert!(Condition::parse(&json("{}")).is_err());
        assert!(c(r#"{"north": true}"#).validate(&["north"]).is_ok());
        assert!(c(r#"{"north": true}"#).validate(&["east"]).is_err());
    }

    #[test]
    fn uv_lock_keeps_the_texture_still_under_a_turn() {
        // fence_side with y 90 and uvlock: the up face is `UV_ROTATION_90` at X0_Y90, so an
        // unrotated face stays unrotated and the locked one comes back turned.
        let uv = FaceUv { uvs: [2.0, 4.0, 6.0, 8.0], rotation: 0 };
        let same = uv.locked(Facing::North, &ModelRotation::new(0, 90));
        assert_eq!((same.uvs, same.rotation), ([2.0, 4.0, 6.0, 8.0], 0));
        let up = uv.locked(Facing::Up, &ModelRotation::new(0, 90));
        assert_eq!((up.uvs, up.rotation), ([16.0 - 4.0, 6.0, 16.0 - 8.0, 2.0], 90));
        let inv = uv.locked(Facing::Up, &ModelRotation::new(0, 180));
        assert_eq!(inv.uvs, [14.0, 12.0, 10.0, 8.0]);
    }
}
