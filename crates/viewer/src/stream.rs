//! Chunk streaming: a worker pool generates chunks and meshes each one against its eight
//! neighbors, and the populator thread (`populator.rs`) populates them in between; the main thread
//! schedules the nearest missing work each frame, uploads finished meshes and unloads what fell
//! out of range.
//!
//! A chunk is populated out to `radius + 1` and meshed out to `radius`. Once meshed, a chunk keeps
//! only its run-length coded states, light and biomes ([`ChunkInfo`]): the lookups at the camera
//! read them, and meshing it again (smooth lighting toggled) rebuilds the block array from them.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use crossbeam_channel::{unbounded, Receiver, Sender};
use worldgen::{Chunk, ChunkGenerator, ChunkPrimer};

use crate::collision::BlockAccess;
use crate::colors::Colors;
use crate::material::ChunkMaterial;
use crate::mesher::{self, ChunkMesh};
use crate::models::Models;
use crate::populator::{self, Edit, PopMsg};
use crate::region::{ColumnLight, Columns, Region};
use crate::sky::View;
use crate::vertex;

pub type Pos = (i32, i32);

/// What a meshed chunk keeps.
pub struct ChunkInfo {
    pub states: Columns<u16>,
    pub light: ColumnLight,
    /// `x + z * 16`, as `Chunk.biomes`.
    pub biomes: [u8; 256],
}

impl ChunkInfo {
    pub(crate) fn to_chunk(&self, p: Pos) -> Chunk {
        let mut blocks = ChunkPrimer::new();
        for x in 0..16 {
            for z in 0..16 {
                for (y0, y1, v) in self.states.column(x, z) {
                    if v != 0 {
                        for y in y0..y1 {
                            blocks.set(x, y, z, v);
                        }
                    }
                }
            }
        }
        Chunk { x: p.0, z: p.1, blocks, biomes: self.biomes }
    }
}

/// A neighbor as a mesh job receives it: the populated chunk, or what a meshed one kept.
#[derive(Clone)]
pub enum Src {
    Data(Arc<Chunk>),
    Info(Arc<ChunkInfo>),
}

pub enum Job {
    /// Sent by the populator; the result goes back to it.
    Generate(Pos),
    /// The 3x3 around `p`, indexed `(dx + 1) + (dz + 1) * 3`.
    Mesh { p: Pos, around: Vec<Src>, key: Key, smooth: bool },
    /// Other work for the pool (lod.rs); it sends its own result.
    Task(Box<dyn FnOnce() + Send>),
}

pub enum Done {
    /// A worker generated a chunk (counters only; the chunk went to the populator).
    Generated(Duration),
    /// The populator's answer to an ensure: the final chunk, or `None` once it fell out of range.
    Ensured(Pos, Option<Arc<Chunk>>),
    Meshed(Pos, Key, ChunkMesh, Arc<ChunkInfo>, Duration),
    /// The populator's answer to an edit: every block it changed, in order. True for the
    /// player's own edits, false for another world's ([`PopMsg::Remote`]).
    Edited(Vec<((i32, i32, i32), u16)>, bool),
}

/// What a mesh was built from: the render epoch and the slot's edit revision.
pub type Key = (u32, u32);

#[derive(Default)]
struct Slot {
    /// The populated chunk, kept until this chunk is meshed, and the edited chunk until the mesh
    /// with the edit in it is back.
    data: Option<Arc<Chunk>>,
    info: Option<Arc<ChunkInfo>>,
    /// An ensure is with the populator.
    generating: bool,
    /// Bumped by every edit in or next to this chunk.
    rev: u32,
    /// The key a mesh job is running for.
    meshing: Option<Key>,
    /// The key of the mesh on screen.
    shown: Option<Key>,
    quads: usize,
    entities: Vec<Entity>,
    /// The chests in the shown mesh.
    chests: Vec<(i32, i32, i32)>,
}

impl Slot {
    fn source(&self) -> Option<Src> {
        self.data.clone().map(Src::Data).or_else(|| self.info.clone().map(Src::Info))
    }
}

/// Rolling counters for the HUD.
#[derive(Default)]
pub struct Stats {
    pub generated_total: u64,
    pub meshed_total: u64,
    /// Generation finish times over the last second.
    recent: VecDeque<Instant>,
    /// Worker-side time spent in `ChunkGenerator::generate`.
    pub gen_time: Duration,
    pub mesh_time: Duration,
    /// Quads in the meshes on screen.
    pub faces: u64,
}

impl Stats {
    pub fn chunks_per_sec(&mut self, now: Instant) -> usize {
        while self.recent.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1)) {
            self.recent.pop_front();
        }
        self.recent.len()
    }

    pub fn avg_gen_ms(&self) -> f64 {
        if self.generated_total == 0 {
            0.0
        } else {
            self.gen_time.as_secs_f64() * 1000.0 / self.generated_total as f64
        }
    }

    pub fn avg_mesh_ms(&self) -> f64 {
        if self.meshed_total == 0 {
            0.0
        } else {
            self.mesh_time.as_secs_f64() * 1000.0 / self.meshed_total as f64
        }
    }
}

/// Pauses generation while the machine runs short of memory, so a large render distance stalls
/// at the edge instead of exhausting RAM. Chunks within four of the player still generate.
pub struct Brake {
    sys: sysinfo::System,
    checked: Option<Instant>,
    pub on: bool,
    /// Available system memory at the last check.
    pub available_mb: u64,
}

/// The brake engages below this much available memory and releases above the second figure.
const BRAKE_ON_MB: u64 = 2048;
const BRAKE_OFF_MB: u64 = 2560;

impl Brake {
    fn new() -> Brake {
        Brake { sys: sysinfo::System::new(), checked: None, on: false, available_mb: 0 }
    }

    /// Reads the available memory, once a second.
    fn update(&mut self, now: Instant) {
        if self.checked.is_some_and(|t| now.duration_since(t) < Duration::from_secs(1)) {
            return;
        }
        self.checked = Some(now);
        self.sys.refresh_memory_specifics(sysinfo::MemoryRefreshKind::nothing().with_ram());
        self.available_mb = self.sys.available_memory() / (1024 * 1024);
        if self.on {
            self.on = self.available_mb < BRAKE_OFF_MB;
        } else {
            self.on = self.available_mb < BRAKE_ON_MB;
        }
    }
}

#[derive(Resource)]
pub struct Streamer {
    pub generator: Arc<ChunkGenerator>,
    pub workers: usize,
    pub radius: i32,
    smooth: bool,
    /// Bumped when every chunk has to be meshed again.
    epoch: u32,
    jobs: Sender<Job>,
    pop: Sender<PopMsg>,
    /// The center and radius the populator last heard.
    pop_center: Option<(Pos, i32)>,
    done: Receiver<Done>,
    /// Ensures and mesh jobs not yet answered.
    in_flight: usize,
    slots: HashMap<Pos, Slot>,
    /// Offsets within `radius + 1`, nearest first.
    order: Vec<Pos>,
    /// Finished meshes waiting for upload, bounded per frame to keep frame times flat.
    uploads: VecDeque<(Pos, Key, ChunkMesh)>,
    pub stats: Stats,
    pub brake: Brake,
    /// Scheduling stopped at the in-flight budget last frame.
    pub busy: bool,
    /// Bumped when a chunk's first mesh shows or a shown chunk unloads.
    pub shown_rev: u64,
    /// Collect what the player's own edits changed in [`Self::outgoing`] (a streamed session
    /// shares them with the server, link.rs).
    pub share: bool,
    pub outgoing: Vec<((i32, i32, i32), u16)>,
    /// Edit answers still to come that the server hears of through `V_USE_ON` instead of
    /// `V_BLOCKS` (a survival placement, [`Self::edit_quiet`]).
    pub quiet: u32,
    /// Blocks broken and placed, the player's own and the other players', for the sounds and
    /// particles (sfx.rs).
    pub events: Vec<WorldEvent>,
}

/// `World.playEvent(2001)` and `ItemBlock`'s place sound, as sfx.rs plays them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorldEvent {
    /// The block and the state it had.
    Broke((i32, i32, i32), u16),
    /// The block and the state placed.
    Placed((i32, i32, i32), u16),
}

/// Meshes uploaded per frame at most.
const UPLOADS_PER_FRAME: usize = 64;

impl BlockAccess for Streamer {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        self.state_at(x, y, z).unwrap_or(0)
    }

    fn loaded(&self, x: i32, z: i32) -> bool {
        self.info_at(x, z).is_some()
    }
}

impl Streamer {
    pub fn new(seed: i64, radius: i32, workers: usize, smooth: bool, models: Arc<Models>, colors: Arc<Colors>) -> Streamer {
        let generator = Arc::new(ChunkGenerator::new(seed));
        let (jobs, job_rx) = unbounded::<Job>();
        let (done_tx, done) = unbounded::<Done>();
        let (pop, pop_rx) = unbounded::<PopMsg>();
        for i in 0..workers {
            let (rx, tx, raw, g, m, c) =
                (job_rx.clone(), done_tx.clone(), pop.clone(), generator.clone(), models.clone(), colors.clone());
            thread::Builder::new().name(format!("chunkgen-{i}")).spawn(move || worker(g, m, c, rx, tx, raw)).expect("spawn worker");
        }
        populator::spawn(generator.clone(), pop_rx, jobs.clone(), done_tx);
        let mut s = Streamer {
            generator,
            workers,
            radius,
            smooth,
            epoch: 0,
            jobs,
            pop,
            pop_center: None,
            done,
            in_flight: 0,
            slots: HashMap::new(),
            order: Vec::new(),
            uploads: VecDeque::new(),
            stats: Stats::default(),
            brake: Brake::new(),
            busy: false,
            shown_rev: 0,
            share: false,
            quiet: 0,
            outgoing: Vec::new(),
            events: Vec::new(),
        };
        s.set_radius(radius);
        s
    }

    pub fn set_radius(&mut self, radius: i32) {
        self.radius = radius.max(2);
        let r = self.radius + 1;
        let mut order: Vec<Pos> = (-r..=r).flat_map(|x| (-r..=r).map(move |z| (x, z))).filter(|&(x, z)| x * x + z * z <= r * r).collect();
        order.sort_by_key(|&(x, z)| x * x + z * z);
        self.order = order;
    }

    /// Smooth lighting on or off; every chunk is meshed again, nearest first, and keeps its old
    /// mesh until the new one is uploaded.
    pub fn set_smooth(&mut self, smooth: bool) {
        if smooth != self.smooth {
            self.smooth = smooth;
            self.epoch += 1;
        }
    }

    pub fn loaded(&self) -> usize {
        self.slots.values().filter(|s| s.shown.is_some()).count()
    }

    /// The chunks whose mesh is on screen.
    pub fn shown(&self) -> impl Iterator<Item = Pos> + '_ {
        self.slots.iter().filter(|(_, s)| s.shown.is_some()).map(|(&p, _)| p)
    }

    /// Runs `f` on the worker pool, behind the jobs already queued.
    pub fn spawn_task(&self, f: impl FnOnce() + Send + 'static) {
        let _ = self.jobs.send(Job::Task(Box::new(f)));
    }

    pub fn known(&self) -> usize {
        self.slots.len()
    }

    pub fn pending(&self) -> usize {
        self.in_flight + self.uploads.len()
    }

    fn in_mesh_range(&self, d: Pos) -> bool {
        d.0 * d.0 + d.1 * d.1 <= self.radius * self.radius
    }

    fn in_keep_range(&self, d: Pos) -> bool {
        let r = self.radius + 3;
        d.0 * d.0 + d.1 * d.1 <= r * r
    }

    /// The kept data of the meshed chunk holding block column `x, z`; `None` where the client
    /// would have no chunk loaded.
    pub fn info_at(&self, x: i32, z: i32) -> Option<&ChunkInfo> {
        self.slots.get(&(x >> 4, z >> 4)).and_then(|s| s.info.as_deref())
    }

    /// The chests in the meshes on screen, for the tile entity renderer.
    pub fn chests(&self) -> Vec<(i32, i32, i32)> {
        self.slots.values().flat_map(|s| s.chests.iter().copied()).collect()
    }

    /// The block state at a world position; air above and below the world. An edit shows here
    /// at once, before the chunk is meshed again.
    pub fn state_at(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        let slot = self.slots.get(&(x >> 4, z >> 4))?;
        let info = slot.info.as_deref()?;
        if !(0..256).contains(&y) {
            return Some(0);
        }
        Some(match &slot.data {
            Some(c) => c.blocks.get(x & 15, y, z & 15),
            None => info.states.get(x & 15, y, z & 15),
        })
    }

    /// The player's change to a block: shown at once, and sent to the populator, whose answer
    /// (`Done::Edited`) is what the world ends up with. `flags` as `World.setBlockState`.
    pub fn edit(&mut self, edits: Vec<Edit>) {
        self.edit_with(edits, false)
    }

    /// [`Self::edit`] for a survival placement: the server places the block itself from the
    /// `V_USE_ON`, so the change is not shared as `V_BLOCKS`.
    pub fn edit_quiet(&mut self, edits: Vec<Edit>) {
        self.edit_with(edits, true)
    }

    fn edit_with(&mut self, edits: Vec<Edit>, quiet: bool) {
        // A break ends with the clicked block going to air (a double plant's other half first);
        // a place starts with the placed block.
        if let Some(last) = edits.last().filter(|e| e.s == 0) {
            if let Some(old) = self.state_at(last.at.0, last.at.1, last.at.2).filter(|&o| o != 0) {
                self.events.push(WorldEvent::Broke(last.at, old));
            }
        } else if let Some(first) = edits.iter().find(|e| e.s != 0) {
            self.events.push(WorldEvent::Placed(first.at, first.s));
        }
        let mut any = false;
        for e in &edits {
            any |= self.set_block(e.at.0, e.at.1, e.at.2, e.s);
        }
        if any {
            if quiet {
                self.quiet += 1;
            }
            let _ = self.pop.send(PopMsg::Edit(edits));
        }
    }

    /// One game tick of flowing liquids in the populator, single player only ([`PopMsg::Tick`]).
    pub fn tick_blocks(&mut self) {
        if !self.share {
            let _ = self.pop.send(PopMsg::Tick);
        }
    }

    /// Blocks another world already settled (another player, the saved world). They join the
    /// populator's overlay and show once it answers.
    pub fn remote(&mut self, changes: Vec<((i32, i32, i32), u16)>) {
        for &(at, s) in changes.iter().take(64) {
            match self.state_at(at.0, at.1, at.2) {
                Some(old) if old != 0 && s == 0 => self.events.push(WorldEvent::Broke(at, old)),
                Some(0) if s != 0 => self.events.push(WorldEvent::Placed(at, s)),
                _ => {}
            }
        }
        if !changes.is_empty() {
            let _ = self.pop.send(PopMsg::Remote(changes));
        }
    }

    /// Writes one block into a meshed chunk and marks the 3x3 around it for meshing; light and
    /// the neighbours' faces and shading depend on it. False where nothing changed.
    fn set_block(&mut self, x: i32, y: i32, z: i32, s: u16) -> bool {
        if !(0..256).contains(&y) {
            return false;
        }
        let p = (x >> 4, z >> 4);
        let Some(slot) = self.slots.get_mut(&p) else { return false };
        let Some(info) = slot.info.as_ref() else { return false };
        let data = slot.data.get_or_insert_with(|| Arc::new(info.to_chunk(p)));
        if data.blocks.get(x & 15, y, z & 15) == s {
            return false;
        }
        Arc::make_mut(data).blocks.set(x & 15, y, z & 15, s);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(n) = self.slots.get_mut(&(p.0 + dx, p.1 + dz)) {
                    n.rev += 1;
                }
            }
        }
        true
    }

    /// `(sky, block)` stored at a world position, `y` in 0..256.
    pub fn light_at(&self, x: i32, y: i32, z: i32) -> Option<(u8, u8)> {
        Some(self.info_at(x, z)?.light.get(x & 15, y, z & 15))
    }

    /// The biome id stored for a block column.
    pub fn biome_at(&self, x: i32, z: i32) -> Option<u8> {
        Some(self.info_at(x, z)?.biomes[((x & 15) + (z & 15) * 16) as usize])
    }
}

fn worker(g: Arc<ChunkGenerator>, models: Arc<Models>, colors: Arc<Colors>, jobs: Receiver<Job>, done: Sender<Done>, raw: Sender<PopMsg>) {
    for job in jobs {
        let t = Instant::now();
        let msg = match job {
            Job::Generate(p) => {
                let chunk = g.generate(p.0, p.1);
                let elapsed = t.elapsed();
                if raw.send(PopMsg::Raw(p, chunk)).is_err() {
                    return;
                }
                Done::Generated(elapsed)
            }
            Job::Mesh { p, around, key, smooth } => {
                let chunks: Vec<Arc<Chunk>> = around
                    .into_iter()
                    .enumerate()
                    .map(|(i, s)| match s {
                        Src::Data(c) => c,
                        Src::Info(info) => Arc::new(info.to_chunk((p.0 + i as i32 % 3 - 1, p.1 + i as i32 / 3 - 1))),
                    })
                    .collect();
                let region = Region::new(std::array::from_fn(|i| &*chunks[i]));
                let mesh = mesher::mesh(&region, &models, &colors, smooth);
                let info = ChunkInfo { states: region.center_states(), light: region.center_light(), biomes: region.center_biomes() };
                Done::Meshed(p, key, mesh, Arc::new(info), t.elapsed())
            }
            Job::Task(f) => {
                f();
                continue;
            }
        };
        if done.send(msg).is_err() {
            return;
        }
    }
}

/// Chunk coordinate of a world position.
pub fn chunk_of(v: Vec3) -> Pos {
    ((v.x.floor() as i32) >> 4, (v.z.floor() as i32) >> 4)
}

/// The three terrain materials. `blocks` is swapped on all three when the mipmap setting changes.
#[derive(Resource)]
pub struct ChunkMaterials {
    pub solid: Handle<ChunkMaterial>,
    pub cutout: Handle<ChunkMaterial>,
    pub translucent: Handle<ChunkMaterial>,
    /// The crack overlay's multiply blend (crack.rs).
    pub crack: Handle<ChunkMaterial>,
}

#[derive(Component)]
pub struct ChunkMeshMarker;

/// The entity sits at the chunk's center column so the translucent layer sorts by it; vertices
/// move by -8 to match.

/// Collects worker results, uploads meshes, unloads far chunks and schedules new work.
pub fn stream_chunks(
    mut commands: Commands,
    mut streamer: ResMut<Streamer>,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<ChunkMaterials>,
    view: Res<View>,
) {
    let s = &mut *streamer;
    let center = chunk_of(view.feet);
    let now = Instant::now();
    s.brake.update(now);

    // Results.
    while let Ok(msg) = s.done.try_recv() {
        match msg {
            Done::Generated(t) => {
                s.stats.generated_total += 1;
                s.stats.gen_time += t;
                s.stats.recent.push_back(now);
            }
            Done::Ensured(p, chunk) => {
                s.in_flight -= 1;
                if let Some(slot) = s.slots.get_mut(&p) {
                    slot.generating = false;
                    if slot.info.is_none() {
                        slot.data = chunk;
                    }
                }
            }
            Done::Meshed(p, key, m, info, t) => {
                s.in_flight -= 1;
                s.stats.meshed_total += 1;
                s.stats.mesh_time += t;
                // A result for a job that was superseded, or for a slot that was unloaded and
                // created again, is dropped. One built before an edit is dropped too: its info
                // would lose the edit.
                if let Some(slot) = s.slots.get_mut(&p).filter(|slot| slot.meshing == Some(key)) {
                    slot.meshing = None;
                    if key.1 == slot.rev {
                        slot.info = Some(info);
                        slot.data = None;
                        s.uploads.push_back((p, key, m));
                    }
                }
            }
            Done::Edited(changes, local) => {
                for &((x, y, z), b) in &changes {
                    s.set_block(x, y, z, b);
                }
                let quiet = local && s.quiet > 0;
                if quiet {
                    s.quiet -= 1;
                }
                if local && s.share && !quiet {
                    // The answer ends with each edited block's final state, which repeats a
                    // change it already lists; the server keeps one record a block, the last.
                    for c in changes {
                        match s.outgoing.iter_mut().find(|o| o.0 == c.0) {
                            Some(o) => o.1 = c.1,
                            None => s.outgoing.push(c),
                        }
                    }
                }
            }
        }
    }

    // Uploads.
    for _ in 0..UPLOADS_PER_FRAME {
        let Some((p, key, mut m)) = s.uploads.pop_front() else { break };
        let Some(slot) = s.slots.get_mut(&p) else { continue };
        for e in slot.entities.drain(..) {
            commands.entity(e).despawn();
        }
        slot.chests = std::mem::take(&mut m.chests);
        s.stats.faces -= slot.quads as u64;
        slot.quads = m.quads();
        s.stats.faces += slot.quads as u64;
        let origin = Transform::from_xyz((p.0 * 16 + 8) as f32, 0.0, (p.1 * 16 + 8) as f32);
        for (data, material, translucent) in
            [(m.solid, &materials.solid, false), (m.cutout, &materials.cutout, false), (m.translucent, &materials.translucent, true)]
        {
            if data.is_empty() {
                continue;
            }
            let (mesh, aabb) = vertex::to_mesh(data);
            let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), aabb, MeshMaterial3d(material.clone()), origin, ChunkMeshMarker));
            if translucent {
                // Water and ice let the sun through in the shaded mode.
                e.insert(NotShadowCaster);
            }
            slot.entities.push(e.id());
        }
        if slot.shown.is_none() {
            s.shown_rev += 1;
        }
        slot.shown = Some(key);
    }

    // Unload.
    let far: Vec<Pos> = s.slots.keys().copied().filter(|&(x, z)| !s.in_keep_range((x - center.0, z - center.1))).collect();
    for p in far {
        if let Some(slot) = s.slots.remove(&p) {
            s.stats.faces -= slot.quads as u64;
            s.shown_rev += slot.shown.is_some() as u64;
            for e in slot.entities {
                commands.entity(e).despawn();
            }
        }
    }
    s.uploads.retain(|(p, _, _)| s.slots.contains_key(p));

    // Schedule, nearest first.
    if s.pop_center != Some((center, s.radius)) {
        s.pop_center = Some((center, s.radius));
        let _ = s.pop.send(PopMsg::Center(center, s.radius));
    }
    let budget = s.workers * 3;
    let epoch = s.epoch;
    let mut i = 0;
    while s.in_flight < budget && i < s.order.len() {
        let d = s.order[i];
        i += 1;
        let p = (center.0 + d.0, center.1 + d.1);
        let mesh_range = s.in_mesh_range(d);
        let braked = s.brake.on && d.0 * d.0 + d.1 * d.1 > 16;
        let slot = s.slots.entry(p).or_default();
        if slot.data.is_none() && slot.info.is_none() {
            if !slot.generating && !braked {
                slot.generating = true;
                s.in_flight += 1;
                let _ = s.pop.send(PopMsg::Ensure(p));
            }
            continue;
        }
        let key = (epoch, slot.rev);
        if !mesh_range || slot.shown == Some(key) || slot.meshing == Some(key) {
            continue;
        }
        let around: Option<Vec<Src>> =
            (0..9).map(|k| s.slots.get(&(p.0 + k % 3 - 1, p.1 + k / 3 - 1)).and_then(Slot::source)).collect();
        let Some(around) = around else { continue };
        if let Some(slot) = s.slots.get_mut(&p) {
            slot.meshing = Some(key);
        }
        s.in_flight += 1;
        let _ = s.jobs.send(Job::Mesh { p, around, key, smooth: s.smooth });
    }
    s.busy = s.in_flight >= budget;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::tests::{chunk, region_of};
    use worldgen::blocks::WATER;

    #[test]
    fn kept_info_rebuilds_the_chunk() {
        let c = chunk(3, -2, 64, |p| {
            for y in 60..64 {
                p.set(8, y, 8, WATER);
            }
        });
        let original = c.blocks.clone();
        let r = region_of(c, 64);
        let info = ChunkInfo { states: r.center_states(), light: r.center_light(), biomes: r.center_biomes() };
        let back = info.to_chunk((3, -2));
        assert_eq!((back.x, back.z), (3, -2));
        assert!(back.blocks.data == original.data);
        assert_eq!(back.biomes, [1; 256]);
    }
}
