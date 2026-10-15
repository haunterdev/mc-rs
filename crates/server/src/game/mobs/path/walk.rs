//! `WalkNodeProcessor` (1.12.2): ground nodes, step-up and drop heights, and the
//! `PathNodeType` classification of every block.

use sim::collision::{collides_with_any_block, floor, Aabb, BlockAccess};
use worldgen::blocks::id;

use super::blocks;
use super::node::{offered, NavEntity, NodeBase, NodeProcessor, PathNodeType, NODE_TYPE_COUNT};

/// `WalkNodeProcessor`
#[derive(Clone, Debug, Default)]
pub struct WalkNodeProcessor {
    base: NodeBase,
    /// `avoidsWater`: the entity's water priority at `init`, put back by `postProcess`.
    avoids_water: f32,
}

/// `Vec3i.hashCode`, for the iteration order of the `HashSet<BlockPos>` in `getStart`.
fn block_pos_hash(p: (i32, i32, i32)) -> i32 {
    p.1.wrapping_add(p.2.wrapping_mul(31)).wrapping_mul(31).wrapping_add(p.0)
}

impl WalkNodeProcessor {
    pub fn new() -> WalkNodeProcessor {
        WalkNodeProcessor::default()
    }

    /// `getPathNodeType(EntityLiving, x, y, z)`: the entity's own footprint and door flags.
    fn node_type_for(&self, w: &impl BlockAccess, e: &NavEntity, x: i32, y: i32, z: i32) -> PathNodeType {
        let b = &self.base;
        self.get_path_node_type_sized(w, x, y, z, e, (b.entity_size_x, b.entity_size_y, b.entity_size_z), b.can_open_doors, b.can_enter_doors)
    }

    /// `WalkNodeProcessor.getSafePoint`
    #[allow(clippy::too_many_arguments)]
    fn get_safe_point(&mut self, w: &impl BlockAccess, e: &NavEntity, x: i32, mut y: i32, z: i32, p4: i32, p5: f64, facing: (i32, i32)) -> Option<usize> {
        let mut pathpoint: Option<usize> = None;
        let d0 = y as f64 - (1.0 - blocks::bounding_max_y(w, x, y - 1, z));
        if d0 - p5 > 1.125 {
            return None;
        }
        let mut node_type = self.node_type_for(w, e, x, y, z);
        let mut f = e.priorities.get(node_type);
        let d1 = e.width as f64 / 2.0;
        if f >= 0.0 {
            let p = self.base.open_point(x, y, z);
            self.base.arena[p].node_type = node_type;
            self.base.arena[p].cost_malus = self.base.arena[p].cost_malus.max(f);
            pathpoint = Some(p);
        }
        if node_type == PathNodeType::Walkable {
            return pathpoint;
        }
        if pathpoint.is_none() && p4 > 0 && node_type != PathNodeType::Fence && node_type != PathNodeType::Trapdoor {
            pathpoint = self.get_safe_point(w, e, x, y + 1, z, p4 - 1, p5, facing);
            if let Some(p) = pathpoint {
                let t = self.base.arena[p].node_type;
                if (t == PathNodeType::Open || t == PathNodeType::Walkable) && e.width < 1.0 {
                    let d2 = (x - facing.0) as f64 + 0.5;
                    let d3 = (z - facing.1) as f64 + 0.5;
                    let bb = Aabb::new(d2 - d1, y as f64 + 0.001, d3 - d1, d2 + d1, (y as f32 + e.height) as f64, d3 + d1);
                    let max_y = blocks::bounding_max_y(w, x, y, z);
                    let bb2 = bb.expand(0.0, max_y - 0.002, 0.0);
                    if collides_with_any_block(w, &bb2) {
                        pathpoint = None;
                    }
                }
            }
        }
        if node_type == PathNodeType::Open {
            let bb3 = Aabb::new(x as f64 - d1 + 0.5, y as f64 + 0.001, z as f64 - d1 + 0.5, x as f64 + d1 + 0.5, (y as f32 + e.height) as f64, z as f64 + d1 + 0.5);
            if collides_with_any_block(w, &bb3) {
                return None;
            }
            if e.width >= 1.0 {
                let below = self.node_type_for(w, e, x, y - 1, z);
                if below == PathNodeType::Blocked {
                    let p = self.base.open_point(x, y, z);
                    self.base.arena[p].node_type = PathNodeType::Walkable;
                    self.base.arena[p].cost_malus = self.base.arena[p].cost_malus.max(f);
                    return Some(p);
                }
            }
            let mut i = 0;
            while y > 0 && node_type == PathNodeType::Open {
                y -= 1;
                if i >= e.max_fall_height {
                    return None;
                }
                i += 1;
                node_type = self.node_type_for(w, e, x, y, z);
                f = e.priorities.get(node_type);
                if node_type != PathNodeType::Open && f >= 0.0 {
                    let p = self.base.open_point(x, y, z);
                    self.base.arena[p].node_type = node_type;
                    self.base.arena[p].cost_malus = self.base.arena[p].cost_malus.max(f);
                    pathpoint = Some(p);
                    break;
                }
                if f < 0.0 {
                    return None;
                }
            }
        }
        pathpoint
    }

    /// `WalkNodeProcessor.getPathNodeTypeRaw`
    pub fn get_path_node_type_raw(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32) -> PathNodeType {
        let s = w.state(x, y, z);
        let b = s >> 4;
        if blocks::is_air(s) {
            PathNodeType::Open
        } else if blocks::is_trapdoor(s) || b == id::WATERLILY {
            PathNodeType::Trapdoor
        } else if b == blocks::FIRE {
            PathNodeType::DamageFire
        } else if b == id::CACTUS {
            PathNodeType::DamageCactus
        } else if worldgen::blocks::is_door(b) && !blocks::is_iron_door(s) && !blocks::door_open(w, s, x, y, z) {
            PathNodeType::DoorWoodClosed
        } else if blocks::is_iron_door(s) && !blocks::door_open(w, s, x, y, z) {
            PathNodeType::DoorIronClosed
        } else if worldgen::blocks::is_door(b) && blocks::door_open(w, s, x, y, z) {
            PathNodeType::DoorOpen
        } else if blocks::is_rail(s) {
            PathNodeType::Rail
        } else if blocks::is_fence_or_wall(s) || (blocks::is_fence_gate(s) && s & 4 == 0) {
            PathNodeType::Fence
        } else {
            match blocks::material(s) {
                sim::region::Mat::Water => PathNodeType::Water,
                sim::region::Mat::Lava => PathNodeType::Lava,
                _ => {
                    if blocks::is_passable(w, x, y, z) {
                        PathNodeType::Open
                    } else {
                        PathNodeType::Blocked
                    }
                }
            }
        }
    }

    /// `WalkNodeProcessor.checkNeighborBlocks`
    pub fn check_neighbor_blocks(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32, mut t: PathNodeType) -> PathNodeType {
        if t == PathNodeType::Walkable {
            for i in -1..=1 {
                for j in -1..=1 {
                    if i != 0 || j != 0 {
                        let b = w.state(i + x, y, j + z) >> 4;
                        if b == id::CACTUS {
                            t = PathNodeType::DangerCactus;
                        } else if b == blocks::FIRE {
                            t = PathNodeType::DangerFire;
                        }
                    }
                }
            }
        }
        t
    }

    /// The 13-argument `getPathNodeType` that fills the `EnumSet`: returns the type of the
    /// first cell and sets a flag per type seen.
    #[allow(clippy::too_many_arguments)]
    fn collect_types(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32, size: (i32, i32, i32), can_open_doors: bool, can_enter_doors: bool, set: &mut [bool; NODE_TYPE_COUNT], mut first: PathNodeType, entity_pos: (i32, i32, i32)) -> PathNodeType {
        for i in 0..size.0 {
            for j in 0..size.1 {
                for k in 0..size.2 {
                    let (l, i1, j1) = (i + x, j + y, k + z);
                    let mut t = self.get_path_node_type(w, l, i1, j1);
                    if t == PathNodeType::DoorWoodClosed && can_open_doors && can_enter_doors {
                        t = PathNodeType::Walkable;
                    }
                    if t == PathNodeType::DoorOpen && !can_enter_doors {
                        t = PathNodeType::Blocked;
                    }
                    if t == PathNodeType::Rail && !blocks::is_rail(w.state(entity_pos.0, entity_pos.1, entity_pos.2)) && !blocks::is_rail(w.state(entity_pos.0, entity_pos.1 - 1, entity_pos.2)) {
                        t = PathNodeType::Fence;
                    }
                    if i == 0 && j == 0 && k == 0 {
                        first = t;
                    }
                    set[t as usize] = true;
                }
            }
        }
        first
    }
}

impl NodeProcessor for WalkNodeProcessor {
    fn base(&self) -> &NodeBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut NodeBase {
        &mut self.base
    }

    fn init(&mut self, _w: &impl BlockAccess, e: &mut NavEntity) {
        self.base.init(e);
        self.avoids_water = e.priorities.get(PathNodeType::Water);
    }

    fn post_process(&mut self, e: &mut NavEntity) {
        e.priorities.set(PathNodeType::Water, self.avoids_water);
    }

    fn get_start(&mut self, w: &impl BlockAccess, e: &NavEntity) -> usize {
        let (bx, by, bz) = e.block_pos();
        let i: i32;
        if self.get_can_swim() && e.in_water {
            let mut yy = e.bb.min_y as i32;
            let (fx, fz) = (floor(e.pos.x), floor(e.pos.z));
            let mut block = w.state(fx, yy, fz) >> 4;
            while block == id::FLOWING_WATER || block == id::WATER {
                yy += 1;
                block = w.state(fx, yy, fz) >> 4;
            }
            i = yy;
        } else if e.on_ground {
            i = floor(e.bb.min_y + 0.5);
        } else {
            let mut y = by;
            while (blocks::is_air(w.state(bx, y, bz)) || blocks::is_passable(w, bx, y, bz)) && y > 0 {
                y -= 1;
            }
            i = y + 1;
        }
        let t1 = self.node_type_for(w, e, bx, i, bz);
        if e.priorities.get(t1) < 0.0 {
            let (min_x, max_x, min_z, max_z) = (e.bb.min_x, e.bb.max_x, e.bb.min_z, e.bb.max_z);
            let mut set: Vec<(i32, i32, i32)> = Vec::new();
            for (a, b) in [(min_x, min_z), (min_x, max_z), (max_x, min_z), (max_x, max_z)] {
                let p = (floor(a), i, floor(b));
                if !set.contains(&p) {
                    set.push(p);
                }
            }
            // HashSet<BlockPos> iterates by bucket (hash ^ hash >>> 16) & 15, then insertion order.
            set.sort_by_key(|&p| {
                let h = block_pos_hash(p) as u32;
                (h ^ (h >> 16)) & 15
            });
            for p in set {
                let t = self.node_type_for(w, e, p.0, p.1, p.2);
                if e.priorities.get(t) >= 0.0 {
                    return self.base.open_point(p.0, p.1, p.2);
                }
            }
        }
        self.base.open_point(bx, i, bz)
    }

    fn get_path_point_to_coords(&mut self, _e: &NavEntity, x: f64, y: f64, z: f64) -> usize {
        self.base.open_point(floor(x), floor(y), floor(z))
    }

    fn find_path_options(&mut self, w: &impl BlockAccess, e: &NavEntity, options: &mut Vec<usize>, cur: usize, target: usize, max_distance: f32) -> usize {
        options.clear();
        let mut j = 0;
        let (cx, cy, cz) = (self.base.arena[cur].x, self.base.arena[cur].y, self.base.arena[cur].z);
        let t = self.node_type_for(w, e, cx, cy + 1, cz);
        if e.priorities.get(t) >= 0.0 {
            j = floor(e.step_height.max(1.0) as f64);
        }
        let d0 = cy as f64 - (1.0 - blocks::bounding_max_y(w, cx, cy - 1, cz));
        // SOUTH, WEST, EAST, NORTH: (xOffset, zOffset)
        let south = self.get_safe_point(w, e, cx, cy, cz + 1, j, d0, (0, 1));
        let west = self.get_safe_point(w, e, cx - 1, cy, cz, j, d0, (-1, 0));
        let east = self.get_safe_point(w, e, cx + 1, cy, cz, j, d0, (1, 0));
        let north = self.get_safe_point(w, e, cx, cy, cz - 1, j, d0, (0, -1));
        for p in [south, west, east, north].into_iter().flatten() {
            if offered(&self.base.arena, p, target, max_distance) {
                options.push(p);
            }
        }
        let open_or_costly = |s: &Self, p: Option<usize>| match p {
            None => true,
            Some(p) => s.base.arena[p].node_type == PathNodeType::Open || s.base.arena[p].cost_malus != 0.0,
        };
        let flag = open_or_costly(self, north);
        let flag1 = open_or_costly(self, south);
        let flag2 = open_or_costly(self, east);
        let flag3 = open_or_costly(self, west);
        let diagonals = [
            (flag && flag3, -1, -1, (0, -1)),
            (flag && flag2, 1, -1, (0, -1)),
            (flag1 && flag3, -1, 1, (0, 1)),
            (flag1 && flag2, 1, 1, (0, 1)),
        ];
        for (cond, dx, dz, facing) in diagonals {
            if cond {
                if let Some(p) = self.get_safe_point(w, e, cx + dx, cy, cz + dz, j, d0, facing) {
                    if offered(&self.base.arena, p, target, max_distance) {
                        options.push(p);
                    }
                }
            }
        }
        options.len()
    }

    fn get_path_node_type_sized(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32, e: &NavEntity, size: (i32, i32, i32), can_break_doors: bool, can_enter_doors: bool) -> PathNodeType {
        let mut set = [false; NODE_TYPE_COUNT];
        let first = self.collect_types(w, x, y, z, size, can_break_doors, can_enter_doors, &mut set, PathNodeType::Blocked, e.block_pos());
        if set[PathNodeType::Fence as usize] {
            return PathNodeType::Fence;
        }
        let mut best = PathNodeType::Blocked;
        for t in PathNodeType::ALL {
            if !set[t as usize] {
                continue;
            }
            if e.priorities.get(t) < 0.0 {
                return t;
            }
            if e.priorities.get(t) >= e.priorities.get(best) {
                best = t;
            }
        }
        if first == PathNodeType::Open && e.priorities.get(best) == 0.0 {
            PathNodeType::Open
        } else {
            best
        }
    }

    fn get_path_node_type(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32) -> PathNodeType {
        let mut t = self.get_path_node_type_raw(w, x, y, z);
        if t == PathNodeType::Open && y >= 1 {
            let block = w.state(x, y - 1, z) >> 4;
            let t1 = self.get_path_node_type_raw(w, x, y - 1, z);
            t = if t1 != PathNodeType::Walkable && t1 != PathNodeType::Open && t1 != PathNodeType::Water && t1 != PathNodeType::Lava { PathNodeType::Walkable } else { PathNodeType::Open };
            if t1 == PathNodeType::DamageFire || block == blocks::MAGMA {
                t = PathNodeType::DamageFire;
            }
            if t1 == PathNodeType::DamageCactus {
                t = PathNodeType::DamageCactus;
            }
        }
        self.check_neighbor_blocks(w, x, y, z, t)
    }
}
