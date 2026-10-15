//! Population for the streamed world: one thread owns a [`Terrain`] (`sim::terrain`, which runs
//! `ChunkGeneratorOverworld.populate` the way `Chunk.populate(provider, generator)` schedules it),
//! takes generated chunks from the worker pool, and hands the main thread each chunk it asked for
//! once nothing more writes to it.

use std::collections::HashSet;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, Sender};
use sim::terrain::{dist2, Terrain};
use worldgen::{Chunk, ChunkGenerator};

use crate::stream::{Done, Job, Pos};

pub use sim::terrain::Edit;

pub enum PopMsg {
    /// The main thread wants chunk `p` populated and final.
    Ensure(Pos),
    /// The player's chunk and the render distance.
    Center(Pos, i32),
    /// A worker generated chunk `p`.
    Raw(Pos, Chunk),
    /// The player changed blocks, in order; the server stops at the first one it refuses.
    Edit(Vec<Edit>),
    /// Blocks another world already settled: another player's edits, or the saved world's on
    /// joining. They join the overlay, which wins over generation and population from then on.
    Remote(Vec<((i32, i32, i32), u16)>),
    /// One game tick of the pending tick list (flowing liquids), single player only: a server
    /// session takes the server's ticks as remote changes. The first one turns the list on.
    Tick,
}

/// `populate` recurses deeply through neighbour updates (falling sand, liquids).
const STACK: usize = 256 << 20;

pub fn spawn(gen: Arc<ChunkGenerator>, rx: Receiver<PopMsg>, jobs: Sender<Job>, done: Sender<Done>) {
    thread::Builder::new()
        .name("populator".into())
        .stack_size(STACK)
        .spawn(move || {
            let mut p = Populator::new(&gen, jobs, done);
            for msg in rx {
                if !p.handle(msg) {
                    return;
                }
            }
        })
        .expect("spawn populator");
}

struct Populator<'g> {
    terrain: Terrain<'g>,
    /// Generation asked of the workers and not yet back.
    requested: HashSet<Pos>,
    /// Ensures waiting for their reply.
    wanted: HashSet<Pos>,
    jobs: Sender<Job>,
    done: Sender<Done>,
    center: Pos,
    radius: i32,
    /// The pending tick list is on (the first [`PopMsg::Tick`]).
    ticking: bool,
}

impl<'g> Populator<'g> {
    fn new(gen: &'g ChunkGenerator, jobs: Sender<Job>, done: Sender<Done>) -> Self {
        Populator {
            terrain: Terrain::new(gen),
            requested: HashSet::new(),
            wanted: HashSet::new(),
            jobs,
            done,
            center: (0, 0),
            radius: i32::MAX / 4,
            ticking: false,
        }
    }

    /// False once the main thread is gone.
    fn handle(&mut self, msg: PopMsg) -> bool {
        match msg {
            PopMsg::Ensure(p) => {
                self.wanted.insert(p);
                for q in self.terrain.needed(p) {
                    if self.requested.insert(q) && self.jobs.send(Job::Generate(q)).is_err() {
                        return false;
                    }
                }
                let wanted = &self.wanted;
                let finals = if self.terrain.ensure(p, |c| wanted.contains(&c)) { vec![p] } else { Vec::new() };
                self.reply_finals(finals)
            }
            PopMsg::Raw(p, chunk) => {
                self.requested.remove(&p);
                let wanted = &self.wanted;
                let finals = self.terrain.insert_generated(p, chunk, |c| wanted.contains(&c));
                self.reply_finals(finals)
            }
            PopMsg::Center(c, radius) => {
                self.center = c;
                self.radius = radius;
                // Ensures that fell out of range are answered empty, so the main thread can count them.
                let keep = (radius + 3) * (radius + 3);
                let gone: Vec<Pos> = self.wanted.iter().copied().filter(|&r| dist2(r, c) > keep).collect();
                for r in gone {
                    self.wanted.remove(&r);
                    if self.done.send(Done::Ensured(r, None)).is_err() {
                        return false;
                    }
                }
                self.terrain.pack_beyond(&[c], radius + 5);
                true
            }
            PopMsg::Edit(edits) => {
                let changes = self.terrain.edit(&edits);
                self.done.send(Done::Edited(changes, true)).is_ok()
            }
            PopMsg::Remote(changes) => {
                let applied = self.terrain.remote(&changes);
                applied.is_empty() || self.done.send(Done::Edited(applied, false)).is_ok()
            }
            PopMsg::Tick => {
                if !self.ticking {
                    self.terrain.set_ticking(true);
                    self.ticking = true;
                }
                let flowed = self.terrain.tick_blocks();
                flowed.is_empty() || self.done.send(Done::Edited(flowed, false)).is_ok()
            }
        }
    }

    /// Replies to every wanted chunk among `finals`, in order.
    fn reply_finals(&mut self, finals: Vec<Pos>) -> bool {
        for r in finals {
            if !self.wanted.remove(&r) {
                continue;
            }
            let chunk = self.terrain.final_chunk(r).expect("a final chunk");
            if self.done.send(Done::Ensured(r, Some(Arc::new(chunk)))).is_err() {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use worldgen::blocks::{id, state};

    #[test]
    fn ensured_chunks_come_back_populated() {
        let t = thread::Builder::new()
            .stack_size(STACK)
            .spawn(|| {
                let gen = ChunkGenerator::new(1);
                let (jobs, job_rx) = unbounded();
                let (done, done_rx) = unbounded();
                let mut p = Populator::new(&gen, jobs, done);
                p.handle(PopMsg::Ensure((0, 0)));
                // Play the worker pool: generate whatever was asked for.
                while let Ok(Job::Generate(q)) = job_rx.try_recv() {
                    p.handle(PopMsg::Raw(q, gen.generate(q.0, q.1)));
                }
                let Ok(Done::Ensured(r, Some(chunk))) = done_rx.try_recv() else { panic!("no reply") };
                assert_eq!(r, (0, 0));
                assert!(chunk.blocks.data.iter().any(|&s| s == state(id::COAL_ORE, 0)), "ores come from population");
                assert!(p.terrain.is_populated((-1, -1)) && p.terrain.is_populated((0, 0)));
                // Out of range: packed, and answered empty if still wanted.
                p.handle(PopMsg::Ensure((40, 40)));
                p.handle(PopMsg::Center((0, 0), 4));
                assert!(matches!(done_rx.try_recv(), Ok(Done::Ensured((40, 40), None))));
                p.handle(PopMsg::Center((100, 100), 4));
                assert!(p.terrain.is_packed((0, 0)));
            })
            .unwrap();
        t.join().unwrap();
    }

    /// Guards population through the move into `crates/sim`: seed 0, every chunk of the 5x5
    /// around (0, 0) ensured in order, the final states hashed (FNV-1a) as recorded before the move.
    #[test]
    fn golden_chunks() {
        let t = thread::Builder::new()
            .stack_size(STACK)
            .spawn(|| {
                let gen = ChunkGenerator::new(0);
                let (jobs, job_rx) = unbounded();
                let (done, done_rx) = unbounded();
                let mut p = Populator::new(&gen, jobs, done);
                let mut finals = std::collections::BTreeMap::new();
                for x in -2..=2 {
                    for z in -2..=2 {
                        p.handle(PopMsg::Ensure((x, z)));
                        while let Ok(Job::Generate(q)) = job_rx.try_recv() {
                            p.handle(PopMsg::Raw(q, gen.generate(q.0, q.1)));
                        }
                        while let Ok(m) = done_rx.try_recv() {
                            if let Done::Ensured(r, Some(c)) = m {
                                finals.insert(r, c);
                            }
                        }
                    }
                }
                assert_eq!(finals.len(), 25);
                let mut h = 0xcbf2_9ce4_8422_2325u64;
                for (r, c) in &finals {
                    for v in [r.0, r.1] {
                        for b in v.to_le_bytes() {
                            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
                        }
                    }
                    for s in c.blocks.data.iter() {
                        for b in s.to_le_bytes() {
                            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
                        }
                    }
                }
                assert_eq!(h, GOLDEN, "the populated chunks changed: {h:#x}");
            })
            .unwrap();
        t.join().unwrap();
    }

    const GOLDEN: u64 = 0xa610_9d2c_f5de_c0ec;

    #[test]
    fn breaking_the_soil_drops_the_plant() {
        let t = thread::Builder::new()
            .stack_size(STACK)
            .spawn(|| {
                let gen = ChunkGenerator::new(1);
                let (jobs, job_rx) = unbounded();
                let (done, done_rx) = unbounded();
                let mut p = Populator::new(&gen, jobs, done);
                p.handle(PopMsg::Ensure((0, 0)));
                while let Ok(Job::Generate(q)) = job_rx.try_recv() {
                    p.handle(PopMsg::Raw(q, gen.generate(q.0, q.1)));
                }
                while done_rx.try_recv().is_ok() {}
                let top = p.terrain.world.get_height(8, 8) + 4;
                let grass = state(id::GRASS, 0);
                let flower = state(id::YELLOW_FLOWER, 0);
                let edit = |at, s, flags, place| Edit { at, s, flags, place };
                p.handle(PopMsg::Edit(vec![edit((8, top, 8), grass, 3, false), edit((8, top + 1, 8), flower, 11, true)]));
                let Ok(Done::Edited(placed, true)) = done_rx.try_recv() else { panic!("no reply") };
                assert_eq!(placed.last(), Some(&((8, top + 1, 8), flower)));
                // A flower in the air is refused and the edits after it are not tried.
                p.handle(PopMsg::Edit(vec![edit((9, top + 3, 9), flower, 11, true), edit((9, top + 4, 9), grass, 3, false)]));
                let Ok(Done::Edited(refused, true)) = done_rx.try_recv() else { panic!("no reply") };
                assert_eq!(refused, vec![((9, top + 3, 9), 0), ((9, top + 4, 9), 0)]);
                p.handle(PopMsg::Edit(vec![edit((8, top, 8), 0, 11, false)]));
                let Ok(Done::Edited(broken, true)) = done_rx.try_recv() else { panic!("no reply") };
                assert!(broken.contains(&((8, top + 1, 8), 0)), "{broken:?}");
            })
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn remote_edits_show_now_and_outlast_generation() {
        let t = thread::Builder::new()
            .stack_size(STACK)
            .spawn(|| {
                let gen = ChunkGenerator::new(1);
                let (jobs, job_rx) = unbounded();
                let (done, done_rx) = unbounded();
                let mut p = Populator::new(&gen, jobs, done);
                let pump = |p: &mut Populator| {
                    while let Ok(Job::Generate(q)) = job_rx.try_recv() {
                        p.handle(PopMsg::Raw(q, gen.generate(q.0, q.1)));
                    }
                };
                p.handle(PopMsg::Ensure((0, 0)));
                pump(&mut p);
                while done_rx.try_recv().is_ok() {}
                let stone = state(id::STONE, 0);
                // A loaded chunk: applied at once and reported as another world's change.
                // A chunk nobody generated yet: kept until it is.
                p.handle(PopMsg::Remote(vec![((8, 200, 8), stone), ((165, 220, 5), stone)]));
                let Ok(Done::Edited(applied, false)) = done_rx.try_recv() else { panic!("no reply") };
                assert_eq!(applied, vec![((8, 200, 8), stone)]);
                p.handle(PopMsg::Ensure((10, 0)));
                pump(&mut p);
                let mut found = None;
                while let Ok(m) = done_rx.try_recv() {
                    if let Done::Ensured((10, 0), Some(c)) = m {
                        found = Some(c);
                    }
                }
                let c = found.expect("chunk (10, 0) answered");
                assert_eq!(c.blocks.get(165 & 15, 220, 5), stone);
            })
            .unwrap();
        t.join().unwrap();
    }
}
