//! The distant-terrain section mesher (lod.rs): one section of 8 x 8 chunks as colored boxes, one
//! box per cell of `step` blocks, after the height-field LODs of Distant Horizons and Voxy.
//!
//! A cell's top sits on its highest column and takes the average color of the columns under it.
//! Walls go down toward lower neighbours: to the neighbour's top inside the section, and to the
//! lowest column under it at the section's edge, so a neighbour section built at another step
//! never leaves a crack. No wall faces a chunk with no data.
//!
//! Tops of one height and color merge into rectangles within each chunk (oceans and plains become
//! a quad a chunk). Every quad stays inside one chunk and carries it, so the vertex shader can drop
//! the quads over chunks whose full mesh is on screen.

use std::sync::Arc;

/// A chunk reduced to 8 x 8 cells of 2 x 2 columns, `x + z * 8`: `r | g << 8 | b << 16 | top << 24`,
/// the color in the block textures' gamma space and `top` the highest non-air block.
pub type Cells = [u32; 64];

/// Chunks along a section side.
pub const SECTION: i32 = 8;
/// Chunks along the side of a section with its one-chunk ring.
pub const AROUND: i32 = SECTION + 2;

/// The face shades baked into the vertex colors, `FaceBakery.getFaceBrightness`.
const SHADE_UP: f32 = 1.0;
const SHADE_NS: f32 = 0.8;
const SHADE_EW: f32 = 0.6;

#[derive(Default)]
pub struct LodMesh {
    pub positions: Vec<[f32; 3]>,
    /// Per vertex: the color with the face shade baked in, `r | g << 8 | b << 16` in the block
    /// textures' 8-bit gamma space, and the quad's chunk, `x mod MASK | (z mod MASK) << 16`, as
    /// [`mask_index`] addresses it.
    pub packed: Vec<[u32; 2]>,
    pub indices: Vec<u32>,
}

/// The chunk key a vertex carries.
pub fn chunk_key(c: (i32, i32)) -> u32 {
    (c.0 & (MASK - 1)) as u32 | ((c.1 & (MASK - 1)) as u32) << 16
}

impl LodMesh {
    pub fn quads(&self) -> usize {
        self.indices.len() / 6
    }

    /// One quad of chunk `key`, wound so its front faces along `normal`.
    fn quad(&mut self, mut p: [[f32; 3]; 4], normal: [f32; 3], rgb: [f32; 3], shade: f32, key: u32) {
        let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let (u, v) = (sub(p[1], p[0]), sub(p[2], p[0]));
        let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        if cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2] < 0.0 {
            p.reverse();
        }
        let base = self.positions.len() as u32;
        let ch = |v: f32| ((v * shade).clamp(0.0, 1.0) * 255.0).round() as u32;
        let color = ch(rgb[0]) | ch(rgb[1]) << 8 | ch(rgb[2]) << 16;
        self.positions.extend_from_slice(&p);
        self.packed.extend_from_slice(&[[color, key]; 4]);
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// One cell at the section's step.
#[derive(Clone, Copy, Debug)]
struct Cell {
    top: i32,
    /// The lowest top among the columns under the cell.
    low: i32,
    rgb: [f32; 3],
}

/// The cell `(i, j)` at `step` blocks of the section whose chunks (with the ring) are `chunks`,
/// `(dx + 1) + (dz + 1) * AROUND`; `None` where its chunk has no data. A cell never spans two
/// chunks: `step` divides 16.
fn cell(chunks: &[Option<Arc<Cells>>], i: i32, j: i32, step: i32) -> Option<Cell> {
    let k = step / 2;
    let (cx, cz) = ((i * k).div_euclid(8), (j * k).div_euclid(8));
    let cells = chunks.get(((cx + 1) + (cz + 1) * AROUND) as usize)?.as_ref()?;
    let (mut top, mut low, mut sum) = (0, 255, [0u32; 3]);
    for a in 0..k {
        for b in 0..k {
            let v = cells[((i * k + a).rem_euclid(8) + (j * k + b).rem_euclid(8) * 8) as usize];
            let h = (v >> 24) as i32;
            top = top.max(h);
            low = low.min(h);
            for (c, s) in sum.iter_mut().enumerate() {
                *s += (v >> (c * 8)) & 255;
            }
        }
    }
    let n = (k * k) as f32 * 255.0;
    Some(Cell { top, low, rgb: sum.map(|s| s as f32 / n) })
}

/// The section's mesh at `step` blocks (2, 4, 8 or 16), with positions from its corner; `origin`
/// is the section's first chunk.
pub fn mesh_section(chunks: &[Option<Arc<Cells>>], step: i32, origin: (i32, i32)) -> LodMesh {
    let n = SECTION * 16 / step;
    let per = 16 / step;
    let side = (n + 2) as usize;
    let grid: Vec<Option<Cell>> = (0..side * side).map(|g| cell(chunks, (g % side) as i32 - 1, (g / side) as i32 - 1, step)).collect();
    let at = |i: i32, j: i32| grid[((i + 1) + (j + 1) * (n + 2)) as usize];
    // Tops merge on height and 8-bit color.
    let merge_key = |c: &Cell| (c.top, c.rgb.map(|v| (v * 255.0).round() as u32));
    let mut out = LodMesh::default();
    let s = step as f32;
    let mut done = vec![false; (n * n) as usize];
    for cz in 0..SECTION {
        for cx in 0..SECTION {
            let key = chunk_key((origin.0 + cx, origin.1 + cz));
            let (i0, j0) = (cx * per, cz * per);
            // Greedy rectangles of equal tops inside the chunk.
            for j in j0..j0 + per {
                for i in i0..i0 + per {
                    if done[(i + j * n) as usize] {
                        continue;
                    }
                    let Some(c) = at(i, j) else { continue };
                    let k = merge_key(&c);
                    let same = |a: i32, b: i32| !done[(a + b * n) as usize] && at(a, b).is_some_and(|d| merge_key(&d) == k);
                    let mut w = 1;
                    while i + w < i0 + per && same(i + w, j) {
                        w += 1;
                    }
                    let mut h = 1;
                    while j + h < j0 + per && (i..i + w).all(|a| same(a, j + h)) {
                        h += 1;
                    }
                    for b in j..j + h {
                        for a in i..i + w {
                            done[(a + b * n) as usize] = true;
                        }
                    }
                    let (x0, z0, x1, z1) = (i as f32 * s, j as f32 * s, (i + w) as f32 * s, (j + h) as f32 * s);
                    let y = (c.top + 1) as f32;
                    out.quad([[x0, y, z0], [x0, y, z1], [x1, y, z1], [x1, y, z0]], [0.0, 1.0, 0.0], c.rgb, SHADE_UP, key);
                }
            }
            for j in j0..j0 + per {
                for i in i0..i0 + per {
                    let Some(c) = at(i, j) else { continue };
                    let (x0, z0, x1, z1) = (i as f32 * s, j as f32 * s, (i + 1) as f32 * s, (j + 1) as f32 * s);
                    let y = (c.top + 1) as f32;
                    for (di, dj) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                        let Some(nb) = at(i + di, j + dj) else { continue };
                        let edge = !(0..n).contains(&(i + di)) || !(0..n).contains(&(j + dj));
                        let bottom = (if edge { nb.low } else { nb.top } + 1) as f32;
                        if bottom >= y {
                            continue;
                        }
                        let normal = [di as f32, 0.0, dj as f32];
                        let wall = match (di, dj) {
                            (0, -1) => [[x0, bottom, z0], [x1, bottom, z0], [x1, y, z0], [x0, y, z0]],
                            (0, _) => [[x0, bottom, z1], [x1, bottom, z1], [x1, y, z1], [x0, y, z1]],
                            (-1, _) => [[x0, bottom, z0], [x0, bottom, z1], [x0, y, z1], [x0, y, z0]],
                            _ => [[x1, bottom, z0], [x1, bottom, z1], [x1, y, z1], [x1, y, z0]],
                        };
                        out.quad(wall, normal, c.rgb, if di == 0 { SHADE_NS } else { SHADE_EW }, key);
                    }
                }
            }
        }
    }
    out
}

/// The LOD step for a section whose nearest chunk is `d` chunks away: 2-block cells out to 24
/// chunks, doubling at 48 and 96, so a cell stays a few pixels wide on a 1080p screen.
pub fn step_for(d: i32) -> i32 {
    match d {
        ..24 => 2,
        24..48 => 4,
        48..96 => 8,
        _ => 16,
    }
}

/// The distance in chunks from `c` to the nearest chunk of section `s`.
pub fn section_distance(s: (i32, i32), c: (i32, i32)) -> i32 {
    let axis = |s: i32, c: i32| {
        let (lo, hi) = (s * SECTION, s * SECTION + SECTION - 1);
        if c < lo {
            lo - c
        } else if c > hi {
            c - hi
        } else {
            0
        }
    };
    let (dx, dz) = (axis(s.0, c.0), axis(s.1, c.1));
    ((dx * dx + dz * dz) as f32).sqrt() as i32
}

/// The distance in chunks from `c` to the farthest chunk of section `s`, rounded up.
pub fn section_far_distance(s: (i32, i32), c: (i32, i32)) -> i32 {
    let axis = |s: i32, c: i32| (s * SECTION - c).abs().max((s * SECTION + SECTION - 1 - c).abs());
    let (dx, dz) = (axis(s.0, c.0), axis(s.1, c.1));
    ((dx * dx + dz * dz) as f32).sqrt().ceil() as i32
}

/// Chunks along the side of the shown-chunk mask. It wraps, so a distant-terrain chunk and a shown
/// chunk only share a texel when their distances from the view add up to `MASK`.
pub const MASK: i32 = 1024;

/// The texel of the `MASK` x `MASK` shown-chunk mask for chunk `p`.
pub fn mask_index(p: (i32, i32)) -> usize {
    ((p.0 & (MASK - 1)) + (p.1 & (MASK - 1)) * MASK) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(top: u32) -> Option<Arc<Cells>> {
        Some(Arc::new([0x0080_4020 | top << 24; 64]))
    }

    fn section(f: impl Fn(i32, i32) -> Option<Arc<Cells>>) -> Vec<Option<Arc<Cells>>> {
        (0..AROUND * AROUND).map(|i| f(i % AROUND - 1, i / AROUND - 1)).collect()
    }

    #[test]
    fn flat_ground_is_one_top_a_chunk() {
        let chunks = section(|_, _| flat(10));
        for step in [2, 4, 8, 16] {
            let m = mesh_section(&chunks, step, (0, 0));
            assert_eq!(m.quads(), 64, "step {step}");
            assert!(m.positions.iter().all(|p| p[1] == 11.0));
        }
        // Gamma color, top face unshaded, and the chunk it covers.
        let m = mesh_section(&chunks, 16, (-8, 16));
        assert_eq!(m.packed[0], [32 | 64 << 8 | 128 << 16, chunk_key((-8, 16))]);
        assert_eq!(m.packed[4][1], chunk_key((-7, 16)));
    }

    #[test]
    fn different_tops_do_not_merge_and_quads_stay_in_their_chunk() {
        // Two heights in a checkerboard of 2 x 2 column cells.
        let mut c = [0u32; 64];
        for (i, v) in c.iter_mut().enumerate() {
            *v = 0x0080_4020 | (10 + ((i % 8 + i / 8) % 2) as u32) << 24;
        }
        let chunks = section(|_, _| Some(Arc::new(c)));
        let m = mesh_section(&chunks, 2, (0, 0));
        let tops = (0..m.quads()).filter(|&q| m.positions[q * 4..q * 4 + 4].iter().all(|p| p[1] == m.positions[q * 4][1])).count();
        assert_eq!(tops, 64 * 64);
        for q in 0..m.quads() {
            let key = m.packed[q * 4][1];
            let (cx, cz) = ((key & 0xFFFF) as f32, (key >> 16) as f32);
            for p in &m.positions[q * 4..q * 4 + 4] {
                assert!(p[0] >= cx * 16.0 && p[0] <= cx * 16.0 + 16.0 && p[2] >= cz * 16.0 && p[2] <= cz * 16.0 + 16.0);
            }
        }
    }

    /// The quads whose corners are not all at one height.
    fn walls(m: &LodMesh) -> Vec<usize> {
        (0..m.quads()).filter(|&q| m.positions[q * 4..q * 4 + 4].iter().any(|p| p[1] != m.positions[q * 4][1])).collect()
    }

    #[test]
    fn a_tall_cell_gets_four_walls_down_to_its_neighbours() {
        let chunks = section(|x, z| {
            if (x, z) == (3, 4) {
                let mut c = [0x00FF_FFFF | 10 << 24; 64];
                c[0] |= 20 << 24;
                Some(Arc::new(c))
            } else {
                flat(10)
            }
        });
        let m = mesh_section(&chunks, 16, (0, 0));
        assert_eq!(m.quads(), 64 + 4);
        let w = walls(&m);
        assert_eq!(w, [36, 37, 38, 39]);
        assert!(w.iter().all(|&q| m.positions[q * 4..q * 4 + 4].iter().all(|p| p[1] == 11.0 || p[1] == 31.0)));
        // North and south walls shade 0.8, west and east 0.6.
        assert_eq!(w.iter().map(|&q| m.packed[q * 4][0] & 255).collect::<Vec<_>>(), [204, 204, 153, 153]);
    }

    #[test]
    fn edge_walls_reach_the_ring_cells_lowest_column() {
        // Ring chunk west of the section: one column at 4, the rest at 10. Section at 12.
        let chunks = section(|x, _| {
            if x == -1 {
                let mut c = [10 << 24; 64];
                c[7] = 4 << 24;
                Some(Arc::new(c))
            } else if x < 8 {
                flat(12)
            } else {
                None
            }
        });
        let m = mesh_section(&chunks, 16, (0, 0));
        // 64 tops, the west column's 8 walls; none east, where the ring has no data.
        assert_eq!(m.quads(), 64 + 8);
        let low = m.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert_eq!(low, 5.0);
        // Every wall faces west and sits on x = 0, wound to face out.
        assert_eq!(walls(&m).len(), 8);
        for q in walls(&m) {
            let p = &m.positions[q * 4..q * 4 + 3];
            assert!(p.iter().all(|v| v[0] == 0.0));
            let (u, v) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
            assert!(u[1] * v[2] - u[2] * v[1] < 0.0, "quad {q} faces -x");
        }
    }

    #[test]
    fn tops_face_up() {
        let m = mesh_section(&section(|_, _| flat(10)), 8, (0, 0));
        let p = &m.positions[0..3];
        let (u, v) = ([p[1][0] - p[0][0], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][2] - p[0][2]]);
        assert!(u[1] * v[0] - u[0] * v[1] > 0.0);
    }

    #[test]
    fn steps_distances_and_mask_wrap() {
        assert_eq!([0, 23, 24, 47, 48, 95, 96, 200].map(step_for), [2, 2, 4, 4, 8, 8, 16, 16]);
        assert_eq!(section_distance((0, 0), (3, 3)), 0);
        assert_eq!(section_distance((1, 0), (3, 3)), 5);
        assert_eq!(section_distance((-1, -1), (0, 0)), 1);
        assert_eq!(section_distance((2, 2), (0, 0)), 22);
        assert_eq!(mask_index((-1, -1)), (MASK * MASK - 1) as usize);
        assert_eq!(mask_index((MASK, 3)), mask_index((0, 3)));
        assert_ne!(mask_index((256, 3)), mask_index((0, 3)));
        assert_eq!(chunk_key((-1, 2)), 1023 | 2 << 16);
        assert_eq!(mask_index((5, 0)), 5);
    }
}
