//! `PathFinder`: A* over the node processor's options, at most 200 dequeued nodes a search.

use sim::collision::BlockAccess;

use super::heap::PathHeap;
use super::node::{NavEntity, NodeProcessor};
use super::point::{Path, PathPoint};

/// The cap on dequeued nodes (`if (i >= 200) break;`).
pub const NODE_BUDGET: i32 = 200;

#[derive(Clone, Debug, Default)]
pub struct PathFinder<P: NodeProcessor> {
    path: PathHeap,
    path_options: Vec<usize>,
    pub node_processor: P,
}

impl<P: NodeProcessor> PathFinder<P> {
    pub fn new(processor: P) -> PathFinder<P> {
        PathFinder { path: PathHeap::default(), path_options: Vec::with_capacity(32), node_processor: processor }
    }

    /// `findPath(world, entity, targetEntity, maxDistance)`: the target's feet.
    pub fn find_path_to_entity(&mut self, w: &impl BlockAccess, e: &mut NavEntity, target_pos: (f64, f64, f64), max_distance: f32) -> Option<Path> {
        self.find_path(w, e, target_pos.0, target_pos.1, target_pos.2, max_distance)
    }

    /// `findPath(world, entity, BlockPos, maxDistance)`: the cell's centre.
    pub fn find_path_to_pos(&mut self, w: &impl BlockAccess, e: &mut NavEntity, target: (i32, i32, i32), max_distance: f32) -> Option<Path> {
        self.find_path(w, e, (target.0 as f32 + 0.5) as f64, (target.1 as f32 + 0.5) as f64, (target.2 as f32 + 0.5) as f64, max_distance)
    }

    /// The private `findPath(world, entity, x, y, z, maxDistance)`.
    pub fn find_path(&mut self, w: &impl BlockAccess, e: &mut NavEntity, x: f64, y: f64, z: f64, max_distance: f32) -> Option<Path> {
        self.path.clear_path();
        self.node_processor.init(w, e);
        let start = self.node_processor.get_start(w, e);
        let target = self.node_processor.get_path_point_to_coords(e, x, y, z);
        let path = self.search(w, e, start, target, max_distance);
        self.node_processor.post_process(e);
        path
    }

    /// The private `findPath(pathFrom, pathTo, maxDistance)`.
    fn search(&mut self, w: &impl BlockAccess, e: &NavEntity, from: usize, to: usize, max_distance: f32) -> Option<Path> {
        {
            let arena = &mut self.node_processor.base_mut().arena;
            arena[from].total_path_distance = 0.0;
            arena[from].distance_to_next = arena[from].distance_manhattan(&arena[to]);
            arena[from].distance_to_target = arena[from].distance_to_next;
        }
        self.path.clear_path();
        // DROPPED: `closedSet`, which Java clears and never reads.
        self.path.add_point(&mut self.node_processor.base_mut().arena, from);
        let mut best = from;
        let mut i = 0;
        while !self.path.is_path_empty() {
            i += 1;
            if i >= NODE_BUDGET {
                break;
            }
            let cur = self.path.dequeue(&mut self.node_processor.base_mut().arena);
            {
                let arena = &mut self.node_processor.base_mut().arena;
                if arena[cur].same_as(&arena[to]) {
                    best = to;
                    break;
                }
                if arena[cur].distance_manhattan(&arena[to]) < arena[best].distance_manhattan(&arena[to]) {
                    best = cur;
                }
                arena[cur].visited = true;
            }
            let n = self.node_processor.find_path_options(w, e, &mut self.path_options, cur, to, max_distance);
            for k in 0..n {
                let p2 = self.path_options[k];
                let arena = &mut self.node_processor.base_mut().arena;
                let f = arena[cur].distance_manhattan(&arena[p2]);
                arena[p2].distance_from_origin = arena[cur].distance_from_origin + f;
                arena[p2].cost = f + arena[p2].cost_malus;
                let f1 = arena[cur].total_path_distance + arena[p2].cost;
                if arena[p2].distance_from_origin < max_distance && (!arena[p2].is_assigned() || f1 < arena[p2].total_path_distance) {
                    arena[p2].previous = Some(cur);
                    arena[p2].total_path_distance = f1;
                    arena[p2].distance_to_next = arena[p2].distance_manhattan(&arena[to]) + arena[p2].cost_malus;
                    if arena[p2].is_assigned() {
                        let d = arena[p2].total_path_distance + arena[p2].distance_to_next;
                        self.path.change_distance(arena, p2, d);
                    } else {
                        arena[p2].distance_to_target = arena[p2].total_path_distance + arena[p2].distance_to_next;
                        self.path.add_point(arena, p2);
                    }
                }
            }
        }
        if best == from {
            None
        } else {
            Some(self.create_path(best))
        }
    }

    /// `PathFinder.createPath`: the chain of `previous` links, start first.
    fn create_path(&self, end: usize) -> Path {
        let arena = &self.node_processor.base().arena;
        let mut points: Vec<PathPoint> = Vec::new();
        let mut cur = Some(end);
        while let Some(i) = cur {
            points.push(arena[i].clone());
            cur = arena[i].previous;
        }
        points.reverse();
        Path::new(points)
    }
}
