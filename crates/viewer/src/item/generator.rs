//! `ItemModelGenerator`: a flat item model from its layer sprites, the sprite as a front and a
//! back face 1/16 apart plus one side face per run of edge pixels.

use crate::models::{Facing, FaceUv, Part, PartFace};
use crate::textures::Sprite;

/// `ItemModelGenerator.LAYERS`
pub const LAYERS: usize = 5;

/// `ItemModelGenerator.SpanFacing`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpanFacing {
    Up,
    Down,
    Left,
    Right,
}

impl SpanFacing {
    const ALL: [SpanFacing; 4] = [SpanFacing::Up, SpanFacing::Down, SpanFacing::Left, SpanFacing::Right];

    fn facing(self) -> Facing {
        match self {
            SpanFacing::Up => Facing::Up,
            SpanFacing::Down => Facing::Down,
            SpanFacing::Left => Facing::East,
            SpanFacing::Right => Facing::West,
        }
    }

    fn offset(self) -> (i32, i32) {
        match self {
            SpanFacing::Up => (0, -1),
            SpanFacing::Down => (0, 1),
            SpanFacing::Left => (-1, 0),
            SpanFacing::Right => (1, 0),
        }
    }

    fn is_horizontal(self) -> bool {
        matches!(self, SpanFacing::Down | SpanFacing::Up)
    }
}

/// `ItemModelGenerator.Span`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    facing: SpanFacing,
    min: i32,
    max: i32,
    anchor: i32,
}

/// `ItemModelGenerator.isTransparent`: outside the sprite, or alpha 0.
fn is_transparent(pixels: &[u32], x: i32, y: i32, w: i32, h: i32) -> bool {
    if x >= 0 && y >= 0 && x < w && y < h {
        (pixels[(y * w + x) as usize] >> 24 & 255) == 0
    } else {
        true
    }
}

/// `ItemModelGenerator.getSpans`, `checkTransition` and `createOrExpandSpan`, over every frame.
fn spans(sprite: &Sprite) -> Vec<Span> {
    let w = sprite.width as i32;
    let mut list: Vec<Span> = Vec::new();
    for frame in &sprite.frames {
        let h = frame.len() as i32 / w;
        for y in 0..h {
            for x in 0..w {
                let opaque = !is_transparent(frame, x, y, w, h);
                for f in SpanFacing::ALL {
                    let (dx, dy) = f.offset();
                    if !(opaque && is_transparent(frame, x + dx, y + dy, w, h)) {
                        continue;
                    }
                    let (pixel, anchor) = if f.is_horizontal() { (x, y) } else { (y, x) };
                    match list.iter_mut().find(|s| s.facing == f && s.anchor == anchor) {
                        // Span.expand
                        Some(s) => {
                            if pixel < s.min {
                                s.min = pixel;
                            } else if pixel > s.max {
                                s.max = pixel;
                            }
                        }
                        None => list.push(Span { facing: f, min: pixel, max: pixel, anchor }),
                    }
                }
            }
        }
    }
    list
}

fn part(from: [f32; 3], to: [f32; 3], faces: Vec<(Facing, PartFace)>) -> Part {
    Part { from, to, faces, rotation: None, shade: true }
}

fn face(tint: i32, texture: &str, uvs: [f32; 4]) -> PartFace {
    PartFace { cullface: None, tint, texture: texture.to_string(), uv: FaceUv { uvs, rotation: 0 } }
}

/// `ItemModelGenerator.getBlockParts(tintIndex, textureName, sprite)`: the sprite as a front and
/// back face 1/16 apart, and one side face per span of edge pixels. `makeItemModel` passes the
/// layer number as the tint index.
pub fn generated_parts(tint: i32, texture: &str, sprite: &Sprite) -> Vec<Part> {
    let mut list = vec![part(
        [0.0, 0.0, 7.5],
        [16.0, 16.0, 8.5],
        vec![(Facing::North, face(tint, texture, [16.0, 0.0, 0.0, 16.0])), (Facing::South, face(tint, texture, [0.0, 0.0, 16.0, 16.0]))],
    )];
    let f = sprite.width as f32;
    let f1 = (sprite.frames[0].len() / sprite.width as usize) as f32;
    for span in spans(sprite) {
        let (mut f2, mut f3, mut f4, mut f5, mut f6, mut f7, mut f8, mut f9, f10, f11);
        let (f12, f13, f14) = (span.min as f32, span.max as f32, span.anchor as f32);
        match span.facing {
            SpanFacing::Up => {
                (f6, f2) = (f12, f12);
                (f4, f7) = (f13 + 1.0, f13 + 1.0);
                (f8, f3, f9, f5) = (f14, f14, f14, f14);
                f10 = 16.0 / f;
                f11 = 16.0 / (f1 - 1.0);
            }
            SpanFacing::Down => {
                (f9, f8) = (f14, f14);
                (f6, f2) = (f12, f12);
                (f4, f7) = (f13 + 1.0, f13 + 1.0);
                (f3, f5) = (f14 + 1.0, f14 + 1.0);
                f10 = 16.0 / f;
                f11 = 16.0 / (f1 - 1.0);
            }
            SpanFacing::Left => {
                (f6, f2, f7, f4) = (f14, f14, f14, f14);
                (f9, f3) = (f12, f12);
                (f5, f8) = (f13 + 1.0, f13 + 1.0);
                f10 = 16.0 / (f - 1.0);
                f11 = 16.0 / f1;
            }
            SpanFacing::Right => {
                (f7, f6) = (f14, f14);
                (f2, f4) = (f14 + 1.0, f14 + 1.0);
                (f9, f3) = (f12, f12);
                (f5, f8) = (f13 + 1.0, f13 + 1.0);
                f10 = 16.0 / (f - 1.0);
                f11 = 16.0 / f1;
            }
        }
        let (f15, f16) = (16.0 / f, 16.0 / f1);
        f2 *= f15;
        f4 *= f15;
        f3 *= f16;
        f5 *= f16;
        f3 = 16.0 - f3;
        f5 = 16.0 - f5;
        f6 *= f10;
        f7 *= f10;
        f8 *= f11;
        f9 *= f11;
        let faces = vec![(span.facing.facing(), face(tint, texture, [f6, f8, f7, f9]))];
        list.push(match span.facing {
            SpanFacing::Up => part([f2, f3, 7.5], [f4, f3, 8.5], faces),
            SpanFacing::Down => part([f2, f5, 7.5], [f4, f5, 8.5], faces),
            SpanFacing::Left => part([f2, f3, 7.5], [f2, f5, 8.5], faces),
            SpanFacing::Right => part([f4, f3, 7.5], [f4, f5, 8.5], faces),
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{bake_quad, ModelRotation};
    use crate::textures::SpriteInfo;

    /// A 4 x 4 sprite with the middle 2 x 2 opaque: one span per edge, each two pixels long.
    #[test]
    fn spans_follow_the_opaque_edge() {
        let mut px = vec![0u32; 16];
        for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
            px[y * 4 + x] = 0xFF00_0000;
        }
        let sprite = Sprite { width: 4, frames: vec![px], frametime: 1 };
        let s = spans(&sprite);
        assert_eq!(s.len(), 4);
        assert!(s.contains(&Span { facing: SpanFacing::Up, min: 1, max: 2, anchor: 1 }));
        assert!(s.contains(&Span { facing: SpanFacing::Down, min: 1, max: 2, anchor: 2 }));
        assert!(s.contains(&Span { facing: SpanFacing::Left, min: 1, max: 2, anchor: 1 }));
        assert!(s.contains(&Span { facing: SpanFacing::Right, min: 1, max: 2, anchor: 2 }));
        let parts = generated_parts(0, "t", &sprite);
        assert_eq!(parts.len(), 5);
        // The top edge of pixel row 1 sits at 16 - 1 * 4 = 12, from x 4 to 12.
        let up = parts.iter().find(|p| p.faces[0].0 == Facing::Up).unwrap();
        assert_eq!((up.from, up.to), ([4.0, 12.0, 7.5], [12.0, 12.0, 8.5]));
    }

    /// A fully opaque 2 x 2 sprite bakes to six faces: front, back and one per edge, each edge
    /// face spanning the whole side of the model.
    #[test]
    fn two_by_two_run_bakes_six_faces() {
        let sprite = Sprite { width: 2, frames: vec![vec![0xFF12_3456; 4]], frametime: 1 };
        let parts = generated_parts(1, "items/t", &sprite);
        let faces: Vec<Facing> = parts.iter().flat_map(|p| p.faces.iter().map(|(f, _)| *f)).collect();
        assert_eq!(faces.len(), 6);
        for f in [Facing::North, Facing::South, Facing::Up, Facing::Down, Facing::East, Facing::West] {
            assert_eq!(faces.iter().filter(|&&g| g == f).count(), 1, "{f:?}");
        }
        let info = SpriteInfo { layer: 3, frames: 1, frametime: 1 };
        let rot = ModelRotation::new(0, 0);
        for p in &parts {
            for (f, pf) in &p.faces {
                let q = bake_quad(p, *f, pf, info, &rot);
                assert_eq!(q.tint, 1);
                assert_eq!(q.sprite.layer, 3);
                // Every edge face is a full 16 wide, 1 deep strip: corners within the unit cube.
                for v in q.pos {
                    assert!(v.iter().all(|c| (-1.0e-6..=1.0 + 1.0e-6).contains(c)), "{f:?} {v:?}");
                }
            }
        }
        let up = parts.iter().find(|p| p.faces[0].0 == Facing::Up).unwrap();
        assert_eq!((up.from, up.to), ([0.0, 16.0, 7.5], [16.0, 16.0, 8.5]));
        // getBlockParts builds the side parts from y f3 to y f5, top to bottom.
        let right = parts.iter().find(|p| p.faces[0].0 == Facing::West).unwrap();
        assert_eq!((right.from, right.to), ([16.0, 16.0, 7.5], [16.0, 0.0, 8.5]));
    }
}
