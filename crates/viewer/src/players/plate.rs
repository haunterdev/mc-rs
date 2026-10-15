//! Name tags: the game font (`FontRenderer` widths), the tag materials and the tag meshes.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::material::NameplateMaterial;
use crate::textures::Argb;

/// `textures/font/ascii.png` and `FontRenderer.charWidth`.
#[derive(Resource)]
pub struct Font {
    pub image: Handle<Image>,
    pub widths: [u8; 256],
}

impl Font {
    /// `FontRenderer.readFontTexture`: each glyph's width is its last column with any opaque
    /// pixel, plus one, scaled to 8 pixels a cell, plus one.
    pub fn widths(img: &Argb) -> [u8; 256] {
        let (w, h) = (img.width as usize, img.height as usize);
        let (cw, ch) = (w / 16, h / 16);
        let scale = 8.0 / cw as f32;
        std::array::from_fn(|i| {
            let (gx, gy) = (i % 16, i / 16);
            let mut l1 = cw as i32 - 1;
            while l1 >= 0 {
                let x = gx * cw + l1 as usize;
                let opaque = (0..ch).any(|y| img.pixels[(gy * ch + y) * w + x] >> 24 != 0);
                if opaque {
                    break;
                }
                l1 -= 1;
            }
            let l1 = l1 + 1;
            if i == 32 {
                // A space is 4 wide.
                return 4;
            }
            ((0.5 + (l1 as f32 * scale) as f64) as i32 + 1) as u8
        })
    }

    /// `FontRenderer.getStringWidth`
    pub fn string_width(&self, s: &str) -> i32 {
        s.bytes().map(|b| self.widths[b as usize] as i32).sum()
    }
}

/// The name tag materials, one per way a piece draws.
#[derive(Resource)]
pub struct PlateMaterials {
    pub(super) bg: Handle<NameplateMaterial>,
    pub(super) bg_depth: Handle<NameplateMaterial>,
    pub(super) through: Handle<NameplateMaterial>,
    pub(super) text: Handle<NameplateMaterial>,
    pub(super) text_dim: Handle<NameplateMaterial>,
}

/// A name tag's three pieces.
#[derive(Clone, Copy)]
pub(super) struct Plate {
    pub(super) bg: Entity,
    pub(super) through: Entity,
    pub(super) text: Entity,
}

/// A name tag's background quad and its text, in the font's pixels (y down), centered as
/// `drawNameplate` centers them: the text from `-width / 2`, the quad one pixel wider each side,
/// from -1 to 8 tall.
pub(super) fn plate_meshes(font: &Font, name: &str) -> (Mesh, Mesh) {
    let half = font.string_width(name) / 2;
    let quad_mesh = |quads: &[([f32; 4], [f32; 4])]| {
        let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
        for ([x0, y0, x1, y1], [u0, v0, u1, v1]) in quads.iter().copied() {
            let base = pos.len() as u32;
            pos.extend_from_slice(&[[x0, y0, 0.0], [x0, y1, 0.0], [x1, y1, 0.0], [x1, y0, 0.0]]);
            uv.extend_from_slice(&[[u0, v0], [u0, v1], [u1, v1], [u1, v0]]);
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let n = vec![[0.0, 0.0, 1.0]; pos.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx))
    };
    let bg = quad_mesh(&[([-half as f32 - 1.0, -1.0, half as f32 + 1.0, 8.0], [0.0; 4])]);
    // FontRenderer.renderDefaultChar: a glyph quad charWidth - 1.01 wide, 7.99 tall.
    let mut glyphs = Vec::new();
    let mut x = -half as f32;
    for b in name.bytes() {
        let w = font.widths[b as usize] as f32;
        let f = w - 0.01;
        let (gx, gy) = ((b % 16) as f32 * 8.0, (b / 16) as f32 * 8.0);
        glyphs.push(([x, 0.0, x + f - 1.0, 7.99], [gx / 128.0, gy / 128.0, (gx + f - 1.0) / 128.0, (gy + 7.99) / 128.0]));
        x += w;
    }
    (bg, quad_mesh(&glyphs))
}
