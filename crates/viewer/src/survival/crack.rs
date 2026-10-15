//! The crack on a block being dug: `RenderGlobal.sendBlockBreakProgress` (one
//! `DestroyBlockProgress` per breaker, expiring after 400 ticks without an update) and
//! `drawBlockDamageTexture` (the block's own model quads re-textured with
//! `destroy_stage_0..9`). The player's own dig comes from mining.rs, other players' from
//! `S_BREAK_ANIM`, so a native and a streamed session draw the same.
//!
//! The quads go through the packed chunk vertex (vertex.rs) on `ChunkMaterials::crack`, one mesh
//! per chunk: `blendFunc(DST_COLOR, SRC_COLOR)` (0.5 grey is neutral, only the dark lines darken)
//! and a polygon offset of -3 (material.rs). The color blend runs in linear space on the sRGB target.
//!
//! DROPPED: the quads also sit 0.002 out along their normal (kept beside the offset),
//! the tile-entity blocks that draw their own break overlay (chests and signs have no model here),
//! and `Block.getOffset` on plants.

use std::collections::BTreeMap;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::BreakAnim;
use crate::mesher::FLAG_NOMIP;
use crate::models::Models;
use crate::sky::{Clock, View};
use crate::stream::{ChunkMaterials, Streamer};
use crate::target::BlockPos;
use crate::vertex::{self, LayerMesh};
use crate::Atlas;

pub fn plugin(app: &mut App) {
    app.init_resource::<Cracks>().add_systems(Update, (preview, read_anims, tick, draw).chain().run_if(resource_exists::<Streamer>));
}

/// `RenderGlobal.updateClouds` removes an entry this many ticks after its last update.
const EXPIRE: u32 = 400;
/// `RenderGlobal.drawBlockDamageTexture` drops an entry farther than this (squared) from the camera.
const RANGE_SQ: f64 = 1024.0;

/// `DestroyBlockProgress`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub breaker: u32,
    pub pos: BlockPos,
    /// `partialBlockProgress`, 0 to 9.
    pub stage: u8,
    /// `createdAtCloudUpdateTick`, set on each update.
    updated: u32,
}

/// `RenderGlobal.damagedBlocks`, by breaker.
#[derive(Resource, Default, Debug)]
pub struct Cracks {
    /// `cloudTickCounter`
    ticks: u32,
    pub list: Vec<Progress>,
}

impl Cracks {
    /// `RenderGlobal.sendBlockBreakProgress`: 0 to 9 sets the breaker's entry, anything else clears it.
    pub fn send_block_break_progress(&mut self, breaker: u32, pos: BlockPos, progress: i32) {
        if (0..10).contains(&progress) {
            let stage = progress as u8;
            match self.list.iter_mut().find(|p| p.breaker == breaker) {
                Some(p) if p.pos == pos => {
                    p.stage = stage;
                    p.updated = self.ticks;
                }
                Some(p) => *p = Progress { breaker, pos, stage, updated: self.ticks },
                None => self.list.push(Progress { breaker, pos, stage, updated: self.ticks }),
            }
        } else {
            self.list.retain(|p| p.breaker != breaker);
        }
    }

    /// `RenderGlobal.updateClouds`: every 20 ticks, entries not updated for 400 go.
    pub fn tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks % 20 == 0 {
            let now = self.ticks;
            self.list.retain(|p| now.wrapping_sub(p.updated) <= EXPIRE);
        }
    }
}

/// `--crack N`: the block under the crosshair gets stage N, as breaker 0. Out of reach, the
/// top solid block in the camera's column does, and its position is printed once.
fn preview(args: Res<crate::Args>, ctl: Res<crate::interact::Controller>, streamer: Res<Streamer>, view: Res<View>, mut cracks: ResMut<Cracks>, mut said: Local<bool>) {
    let Some(stage) = args.crack else { return };
    let pos = match ctl.over.filter(|h| h.kind == crate::target::Kind::Block) {
        Some(h) => Some(h.pos),
        None => {
            let (x, z) = (view.eye.x.floor() as i32, view.eye.z.floor() as i32);
            (0..view.eye.y as i32).rev().map(|y| (x, y, z)).find(|&(x, y, z)| {
                let s = streamer.state_at(x, y, z).unwrap_or(0);
                crate::region::props(s).mat == crate::region::Mat::Solid && !matches!(s >> 4, worldgen::blocks::id::LEAVES | worldgen::blocks::id::LEAVES2)
            })
        }
    };
    if let Some(p) = pos {
        if !std::mem::replace(&mut *said, true) {
            eprintln!("CRACK stage {stage} at {p:?}");
        }
        cracks.send_block_break_progress(0, p, stage as i32);
    }
}

fn read_anims(mut anims: MessageReader<BreakAnim>, mut cracks: ResMut<Cracks>) {
    for BreakAnim(a) in anims.read() {
        cracks.send_block_break_progress(a.breaker, (a.pos[0], a.pos[1], a.pos[2]), a.stage as i32);
    }
}

pub fn tick(clock: Res<Clock>, mut cracks: ResMut<Cracks>) {
    for _ in 0..clock.ticks_this_frame {
        cracks.tick();
    }
}

/// The sprite name of a stage.
pub fn stage_sprite(stage: u8) -> &'static str {
    crate::textures::DESTROY_SPRITES[stage.min(9) as usize]
}

/// One re-textured quad: corners (block-relative), the sprite-relative uvs, the normal.
pub type CrackQuad = ([[f32; 3]; 4], [[f32; 2]; 4], [f32; 3]);

/// `BlockRendererDispatcher.renderBlockDamage`: the quads of the block's model, pushed out a
/// little. None for a state with no model.
pub fn damage_quads(models: &Models, state: u16, p: BlockPos) -> Option<Vec<CrackQuad>> {
    let model = models.get(state, false)?.pick(p.0, p.1, p.2);
    let mut out = Vec::new();
    for q in model.face_quads.iter().flatten().chain(model.general.iter()) {
        let n = q.face.vec().map(|c| c as f32);
        let pos = q.pos.map(|v| [v[0] + n[0] * 0.002, v[1] + n[1] * 0.002, v[2] + n[2] * 0.002]);
        out.push((pos, q.uv, n));
    }
    Some(out)
}

/// The crack meshes: a pool, one per chunk with a crack in it.
#[derive(Component)]
pub struct CrackMesh;

fn new_mesh() -> Mesh {
    // One degenerate quad: the mesh allocator refuses an empty mesh.
    let mut first = LayerMesh::default();
    first.quad([[8.0, 0.0, 8.0]; 4], [[0.0; 2]; 4], [0; 4], [[0; 3]; 4], [0.0, 1.0, 0.0], 0);
    vertex::to_mesh(first).0
}

/// `RenderGlobal.drawBlockDamageTexture`.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    view: Res<View>,
    streamer: Res<Streamer>,
    atlas: Res<Atlas>,
    materials: Option<Res<ChunkMaterials>>,
    mut cracks: ResMut<Cracks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut pool: Local<Vec<Entity>>,
    mut shown: Query<(&Mesh3d, &mut Transform, &mut Visibility), With<CrackMesh>>,
) {
    let Some(materials) = materials else { return };
    let eye = view.eye;
    // Farther than 32 blocks: the entry is dropped, as vanilla does.
    cracks.list.retain(|p| {
        let d = Vec3::new(p.pos.0 as f32 + 0.5, p.pos.1 as f32 + 0.5, p.pos.2 as f32 + 0.5) - eye;
        d.length_squared() as f64 <= RANGE_SQ
    });
    let mut chunks: BTreeMap<(i32, i32), LayerMesh> = BTreeMap::new();
    for pr in &cracks.list {
        let (x, y, z) = pr.pos;
        let s = streamer.state_at(x, y, z).unwrap_or(0);
        let Some(sprite) = atlas.0.textures.get(stage_sprite(pr.stage)) else { continue };
        let Some(quads) = damage_quads(&atlas.0, s, pr.pos) else { continue };
        let (cx, cz) = (x >> 4, z >> 4);
        let out = chunks.entry((cx, cz)).or_default();
        let (ox, oz) = ((x - cx * 16) as f32, (z - cz * 16) as f32);
        // Full bright: the crack is drawn unlit.
        let lm = 240 | 240 << 16;
        for (pos, uv, n) in quads {
            let pos = pos.map(|v| [v[0] + ox, v[1] + y as f32, v[2] + oz]);
            out.quad(pos, uv, [lm; 4], [[255; 3]; 4], n, sprite.layer | FLAG_NOMIP);
        }
    }
    let mut groups = chunks.into_iter();
    let mut i = 0;
    loop {
        let next = groups.next();
        if next.is_none() && i >= pool.len() {
            break;
        }
        if i >= pool.len() {
            pool.push(
                commands
                    .spawn((
                        CrackMesh,
                        Mesh3d(meshes.add(new_mesh())),
                        MeshMaterial3d(materials.crack.clone()),
                        Transform::default(),
                        Visibility::Hidden,
                        NoFrustumCulling,
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }
        if let Ok((mesh, mut transform, mut visibility)) = shown.get_mut(pool[i]) {
            match next {
                Some(((cx, cz), out)) => {
                    *visibility = Visibility::Inherited;
                    *transform = Transform::from_xyz((cx * 16) as f32 + 8.0, 0.0, (cz * 16) as f32 + 8.0);
                    let _ = meshes.insert(&mesh.0, vertex::to_mesh(out).0);
                }
                None => *visibility = Visibility::Hidden,
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_follows_progress() {
        use super::super::mining::progress_of;
        // curBlockDamageMP 0.1 to 0.19 is stage 0 ... 0.9 to 0.99 is stage 8; stage 9 never
        // shows from a dig below 1.0 because 1.0 breaks the block.
        let mut c = Cracks::default();
        for (cur, want) in [(0.05, None), (0.1, Some(0)), (0.35, Some(2)), (0.99, Some(8))] {
            c.send_block_break_progress(1, (0, 64, 0), progress_of(cur));
            assert_eq!(c.list.first().map(|p| p.stage), want, "cur {cur}");
        }
    }

    #[test]
    fn one_entry_per_breaker_and_it_expires() {
        let mut c = Cracks::default();
        c.send_block_break_progress(1, (0, 64, 0), 3);
        c.send_block_break_progress(2, (5, 64, 0), 9);
        c.send_block_break_progress(1, (1, 64, 0), 0);
        assert_eq!(c.list.iter().map(|p| (p.breaker, p.pos, p.stage)).collect::<Vec<_>>(), [(1, (1, 64, 0), 0), (2, (5, 64, 0), 9)]);
        // Breaker 1 keeps updating, breaker 2 goes silent.
        for t in 1..=420 {
            if t % 100 == 0 {
                c.send_block_break_progress(1, (1, 64, 0), 1);
            }
            c.tick();
        }
        assert_eq!(c.list.len(), 1);
        assert_eq!(c.list[0].breaker, 1);
        c.send_block_break_progress(1, (1, 64, 0), -1);
        assert!(c.list.is_empty());
        c.send_block_break_progress(2, (0, 0, 0), 10);
        assert!(c.list.is_empty());
    }
}
