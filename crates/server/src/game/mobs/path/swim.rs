//! `SwimNodeProcessor`: nodes through water in all six directions. Used with a swimming
//! navigator (`PathNavigateSwimmer`, not ported yet); here it is exercised through the
//! `PathFinder` directly.

use sim::collision::{floor, BlockAccess};

use super::blocks;
use super::node::{offered, NavEntity, NodeBase, NodeProcessor, PathNodeType};

#[derive(Clone, Debug, Default)]
pub struct SwimNodeProcessor {
    base: NodeBase,
}

/// `EnumFacing.values()`: DOWN, UP, NORTH, SOUTH, WEST, EAST.
const FACINGS: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

impl SwimNodeProcessor {
    pub fn new() -> SwimNodeProcessor {
        SwimNodeProcessor::default()
    }

    /// `getWaterNode`
    fn get_water_node(&mut self, w: &impl BlockAccess, x: i32, y: i32, z: i32) -> Option<usize> {
        if self.is_free(w, x, y, z) == PathNodeType::Water {
            Some(self.base.open_point(x, y, z))
        } else {
            None
        }
    }

    /// `isFree`: water through the whole footprint.
    fn is_free(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32) -> PathNodeType {
        for i in x..x + self.base.entity_size_x {
            for j in y..y + self.base.entity_size_y {
                for k in z..z + self.base.entity_size_z {
                    if blocks::material(w.state(i, j, k)) != sim::region::Mat::Water {
                        return PathNodeType::Blocked;
                    }
                }
            }
        }
        PathNodeType::Water
    }
}

impl NodeProcessor for SwimNodeProcessor {
    fn base(&self) -> &NodeBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut NodeBase {
        &mut self.base
    }

    fn get_start(&mut self, _w: &impl BlockAccess, e: &NavEntity) -> usize {
        self.base.open_point(floor(e.bb.min_x), floor(e.bb.min_y + 0.5), floor(e.bb.min_z))
    }

    fn get_path_point_to_coords(&mut self, e: &NavEntity, x: f64, y: f64, z: f64) -> usize {
        self.base.open_point(floor(x - (e.width / 2.0) as f64), floor(y + 0.5), floor(z - (e.width / 2.0) as f64))
    }

    fn find_path_options(&mut self, w: &impl BlockAccess, _e: &NavEntity, options: &mut Vec<usize>, cur: usize, target: usize, max_distance: f32) -> usize {
        options.clear();
        let (cx, cy, cz) = (self.base.arena[cur].x, self.base.arena[cur].y, self.base.arena[cur].z);
        for (dx, dy, dz) in FACINGS {
            if let Some(p) = self.get_water_node(w, cx + dx, cy + dy, cz + dz) {
                if offered(&self.base.arena, p, target, max_distance) {
                    options.push(p);
                }
            }
        }
        options.len()
    }

    fn get_path_node_type_sized(&self, _w: &impl BlockAccess, _x: i32, _y: i32, _z: i32, _e: &NavEntity, _size: (i32, i32, i32), _b: bool, _d: bool) -> PathNodeType {
        PathNodeType::Water
    }

    fn get_path_node_type(&self, _w: &impl BlockAccess, _x: i32, _y: i32, _z: i32) -> PathNodeType {
        PathNodeType::Water
    }
}
