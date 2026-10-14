//! The packed chunk vertex: 20 bytes where Bevy's position, normal, two UVs and color take 56,
//! the way Sodium's compact vertex format packs terrain. shaders/chunk_vertex.wgsl unpacks it.
//!
//! `packed` (`Uint32x4`):
//! - x: x | z, 16 bits each, `(local + 16) * 2048`, so a vertex sits on a 1/2048 grid in
//!   [-16, 16) around the chunk's center
//! - y: tag (array layer and flags, 12 bits) | y << 12, `(y + 8) * 2048` in 20 bits
//! - z: u | v << 16, `(uv + 0.5) * 32768`
//! - w: r | g << 8 | b << 16 | block light << 24
//!
//! `normal` (`Uint32`): sky light | nx << 8 | ny << 16 | nz << 24, the normal as snorm8.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::Aabb;
use bevy::mesh::{Indices, MeshVertexAttribute, VertexFormat};
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;

pub const ATTRIBUTE_PACKED: MeshVertexAttribute = MeshVertexAttribute::new("Vertex_ChunkPacked", 0x6d63_7273_0001, VertexFormat::Uint32x4);
pub const ATTRIBUTE_PACKED_NORMAL: MeshVertexAttribute =
    MeshVertexAttribute::new("Vertex_ChunkPackedNormal", 0x6d63_7273_0002, VertexFormat::Uint32);

/// One render layer's vertices.
#[derive(Default)]
pub struct LayerMesh {
    pub packed: Vec<[u32; 4]>,
    pub normals: Vec<u32>,
    pub indices: Vec<u32>,
    /// The vertices' bounds in mesh coordinates, for frustum culling.
    pub min: [f32; 3],
    pub max: [f32; 3],
}

fn fixed(v: f32, offset: f32, scale: f32, max: u32) -> u32 {
    ((v + offset) * scale).round().clamp(0.0, max as f32) as u32
}

fn snorm8(v: f32) -> u32 {
    ((v.clamp(-1.0, 1.0) * 127.0).round() as i32 as u8) as u32
}

/// One vertex at `pos` (0 to 16 across the chunk), with lightmap coordinates `light` (block |
/// sky << 16, 0 to 240 each) and `tag` (array layer | flags).
pub fn pack(pos: [f32; 3], uv: [f32; 2], light: i32, color: [u8; 3], normal: [f32; 3], tag: u32) -> ([u32; 4], u32) {
    debug_assert!(tag < 1 << 12, "tag {tag} needs more than 12 bits");
    let x = fixed(pos[0] - 8.0, 16.0, 2048.0, 0xFFFF);
    let z = fixed(pos[2] - 8.0, 16.0, 2048.0, 0xFFFF);
    let y = fixed(pos[1], 8.0, 2048.0, 0xF_FFFF);
    let u = fixed(uv[0], 0.5, 32768.0, 0xFFFF);
    let v = fixed(uv[1], 0.5, 32768.0, 0xFFFF);
    let block = (light & 0xFFFF).clamp(0, 255) as u32;
    let sky = (light >> 16 & 0xFFFF).clamp(0, 255) as u32;
    let rgb = color[0] as u32 | (color[1] as u32) << 8 | (color[2] as u32) << 16;
    let packed = [x | z << 16, (tag & 0xFFF) | y << 12, u | v << 16, rgb | block << 24];
    let n = sky | snorm8(normal[0]) << 8 | snorm8(normal[1]) << 16 | snorm8(normal[2]) << 24;
    (packed, n)
}

impl LayerMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn quads(&self) -> usize {
        self.indices.len() / 6
    }

    /// One `GL_QUADS` quad, split along the 0-2 diagonal as the driver splits it.
    #[allow(clippy::too_many_arguments)]
    pub fn quad(&mut self, pos: [[f32; 3]; 4], uv: [[f32; 2]; 4], light: [i32; 4], color: [[u8; 3]; 4], normal: [f32; 3], tag: u32) {
        let base = self.packed.len() as u32;
        if base == 0 {
            self.min = pos[0];
            self.max = pos[0];
        }
        for k in 0..4 {
            let (p, n) = pack(pos[k], uv[k], light[k], color[k], normal, tag);
            self.packed.push(p);
            self.normals.push(n);
            for a in 0..3 {
                self.min[a] = self.min[a].min(pos[k][a]);
                self.max[a] = self.max[a].max(pos[k][a]);
            }
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// The mesh for a chunk entity at the chunk's center, and its bounds. Indices are 16-bit when the
/// layer has at most 65536 vertices.
pub fn to_mesh(d: LayerMesh) -> (Mesh, Aabb) {
    let aabb = Aabb::from_min_max(
        Vec3::new(d.min[0] - 8.0, d.min[1], d.min[2] - 8.0),
        Vec3::new(d.max[0] - 8.0, d.max[1], d.max[2] - 8.0),
    );
    let indices = if d.packed.len() <= 1 << 16 {
        Indices::U16(d.indices.into_iter().map(|i| i as u16).collect())
    } else {
        Indices::U32(d.indices)
    };
    let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(ATTRIBUTE_PACKED, d.packed)
        .with_inserted_attribute(ATTRIBUTE_PACKED_NORMAL, d.normals)
        .with_inserted_indices(indices);
    (mesh, aabb)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// shaders/chunk_vertex.wgsl's unpacking, in Rust.
    fn unpack(p: [u32; 4], n: u32) -> ([f32; 3], [f32; 2], [u32; 2], [u32; 3], [f32; 3], u32) {
        let pos = [
            (p[0] & 0xFFFF) as f32 / 2048.0 - 16.0,
            (p[1] >> 12) as f32 / 2048.0 - 8.0,
            (p[0] >> 16) as f32 / 2048.0 - 16.0,
        ];
        let uv = [(p[2] & 0xFFFF) as f32 / 32768.0 - 0.5, (p[2] >> 16) as f32 / 32768.0 - 0.5];
        let light = [p[3] >> 24, n & 0xFF];
        let rgb = [p[3] & 0xFF, p[3] >> 8 & 0xFF, p[3] >> 16 & 0xFF];
        let normal = [(n >> 8) as u8 as i8 as f32 / 127.0, (n >> 16) as u8 as i8 as f32 / 127.0, (n >> 24) as u8 as i8 as f32 / 127.0];
        (pos, uv, light, rgb, normal, p[1] & 0xFFF)
    }

    #[test]
    fn a_vertex_round_trips() {
        let (p, n) = pack([0.0, 255.0 + 1.0 / 16.0, 16.0], [0.3125, 1.0], 240 | 128 << 16, [10, 200, 255], [0.0, -1.0, 0.0], 1023 | 1 << 11);
        let (pos, uv, light, rgb, normal, tag) = unpack(p, n);
        assert_eq!(pos, [-8.0, 255.0625, 8.0]);
        assert_eq!(uv, [0.3125, 1.0]);
        assert_eq!(light, [240, 128]);
        assert_eq!(rgb, [10, 200, 255]);
        assert_eq!(normal, [0.0, -1.0, 0.0]);
        assert_eq!(tag, 1023 | 1 << 11);
    }

    #[test]
    fn offset_plants_and_rotated_normals_stay_close() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let (p, n) = pack([-0.27, 0.0, 16.31], [-0.1, 1.2], 0, [0; 3], [s, 0.0, -s], 0);
        let (pos, uv, _, _, normal, _) = unpack(p, n);
        assert!((pos[0] + 8.27).abs() < 1.0 / 4096.0 && (pos[2] - 8.31).abs() < 1.0 / 4096.0, "{pos:?}");
        assert!((uv[0] + 0.1).abs() < 1.0 / 65536.0 && (uv[1] - 1.2).abs() < 1.0 / 65536.0, "{uv:?}");
        assert!((normal[0] - s).abs() < 0.01 && (normal[2] + s).abs() < 0.01, "{normal:?}");
    }

    #[test]
    fn bounds_and_index_width_follow_the_vertices() {
        let mut m = LayerMesh::default();
        m.quad([[1.0, 60.0, 2.0], [3.0, 60.0, 2.0], [3.0, 64.0, 2.0], [1.0, 64.0, 2.0]], [[0.0; 2]; 4], [0; 4], [[0; 3]; 4], [0.0, 0.0, -1.0], 0);
        m.quad([[5.0, 70.0, 9.0]; 4], [[0.0; 2]; 4], [0; 4], [[0; 3]; 4], [0.0, 1.0, 0.0], 0);
        assert_eq!((m.min, m.max), ([1.0, 60.0, 2.0], [5.0, 70.0, 9.0]));
        let (mesh, aabb) = to_mesh(m);
        assert_eq!(aabb.min(), Vec3A::new(-7.0, 60.0, -6.0));
        assert_eq!(aabb.max(), Vec3A::new(-3.0, 70.0, 1.0));
        assert!(matches!(mesh.indices(), Some(Indices::U16(i)) if i.len() == 12));
    }
}
