//! `PathNavigate` and `PathNavigateGround` merged: the ground navigator is the only subclass
//! ported, so the abstract methods (`canNavigate`, `getEntityPosition`,
//! `isDirectPathBetweenPoints`, `getPathFinder`) are the ground ones.
//!
//! DROPPED: the `ChunkCache` snapshot (the world is read directly), the profiler, `debugPathFinding`, and `isRiding`.
//! The wall clock in `checkForStuck` and the world time in `updatePath` both read this
//! navigator's own tick counter (50 ms a tick), which is what they measure on a 20 tps server.

use glam::DVec3;
use sim::collision::{ceil, floor, BlockAccess};
use worldgen::blocks::id;

use super::blocks;
use super::finder::PathFinder;
use super::helpers::EntityMoveHelper;
use super::node::{NavEntity, NodeProcessor, PathNodeType};
use super::point::Path;
use super::walk::WalkNodeProcessor;

/// `World.getHeight()`
const WORLD_HEIGHT: i32 = 256;

/// `PathNavigateGround`
#[derive(Clone, Debug)]
pub struct PathNavigateGround {
    finder: PathFinder<WalkNodeProcessor>,
    current_path: Option<Path>,
    speed: f64,
    total_ticks: i32,
    ticks_at_last_pos: i32,
    last_pos_check: DVec3,
    timeout_cached_node: DVec3,
    timeout_timer: i64,
    last_timeout_check: i64,
    timeout_limit: f64,
    max_distance_to_waypoint: f32,
    try_update_path: bool,
    last_time_updated: i64,
    target_pos: Option<(i32, i32, i32)>,
    /// `PathNavigateGround.shouldAvoidSun`, set by `EntityAIRestrictSun`.
    should_avoid_sun: bool,
}

impl Default for PathNavigateGround {
    fn default() -> Self {
        Self::new()
    }
}

impl PathNavigateGround {
    /// `new PathNavigateGround(entity, world)`: `getPathFinder` builds the walk processor and
    /// turns `canEnterDoors` on.
    pub fn new() -> PathNavigateGround {
        let mut p = WalkNodeProcessor::new();
        p.set_can_enter_doors(true);
        PathNavigateGround {
            finder: PathFinder::new(p),
            current_path: None,
            speed: 0.0,
            total_ticks: 0,
            ticks_at_last_pos: 0,
            last_pos_check: DVec3::ZERO,
            timeout_cached_node: DVec3::ZERO,
            timeout_timer: 0,
            last_timeout_check: 0,
            timeout_limit: 0.0,
            max_distance_to_waypoint: 0.5,
            try_update_path: false,
            last_time_updated: 0,
            target_pos: None,
            should_avoid_sun: false,
        }
    }

    /// `PathNavigateGround.setAvoidSun`
    pub fn set_avoid_sun(&mut self, avoid: bool) {
        self.should_avoid_sun = avoid;
    }

    /// `PathNavigate.getNodeProcessor`
    pub fn get_node_processor(&self) -> &WalkNodeProcessor {
        &self.finder.node_processor
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed;
    }

    /// `PathNavigate.getPathSearchRange`
    pub fn get_path_search_range(e: &NavEntity) -> f32 {
        e.follow_range
    }

    pub fn can_update_path_on_timeout(&self) -> bool {
        self.try_update_path
    }

    /// The navigator's clock in ms: `getTotalWorldTime` is `total_ticks`, `currentTimeMillis`
    /// is `total_ticks * 50`.
    fn now_ms(&self) -> i64 {
        self.total_ticks as i64 * 50
    }

    /// `PathNavigate.updatePath`
    pub fn update_path(&mut self, w: &impl BlockAccess, e: &mut NavEntity) {
        if self.total_ticks as i64 - self.last_time_updated > 20 {
            if let Some(t) = self.target_pos {
                self.current_path = None;
                self.current_path = self.get_path_to_pos(w, e, t);
                self.last_time_updated = self.total_ticks as i64;
                self.try_update_path = false;
            }
        } else {
            self.try_update_path = true;
        }
    }

    /// `PathNavigate.getPathToXYZ`
    pub fn get_path_to_xyz(&mut self, w: &impl BlockAccess, e: &mut NavEntity, x: f64, y: f64, z: f64) -> Option<Path> {
        self.get_path_to_pos(w, e, (floor(x), floor(y), floor(z)))
    }

    /// `PathNavigateGround.getPathToPos`: lifts the target out of air and out of solid blocks,
    /// then `PathNavigate.getPathToPos`.
    pub fn get_path_to_pos(&mut self, w: &impl BlockAccess, e: &mut NavEntity, mut pos: (i32, i32, i32)) -> Option<Path> {
        if blocks::is_air(w.state(pos.0, pos.1, pos.2)) {
            let mut y = pos.1 - 1;
            while y > 0 && blocks::is_air(w.state(pos.0, y, pos.2)) {
                y -= 1;
            }
            if y > 0 {
                return self.get_path_to_pos_base(w, e, (pos.0, y + 1, pos.2));
            }
            while y < WORLD_HEIGHT && blocks::is_air(w.state(pos.0, y, pos.2)) {
                y += 1;
            }
            pos = (pos.0, y, pos.2);
        }
        if !blocks::is_solid(w.state(pos.0, pos.1, pos.2)) {
            self.get_path_to_pos_base(w, e, pos)
        } else {
            let mut y = pos.1 + 1;
            while y < WORLD_HEIGHT && blocks::is_solid(w.state(pos.0, y, pos.2)) {
                y += 1;
            }
            self.get_path_to_pos_base(w, e, (pos.0, y, pos.2))
        }
    }

    /// `PathNavigate.getPathToPos`
    fn get_path_to_pos_base(&mut self, w: &impl BlockAccess, e: &mut NavEntity, pos: (i32, i32, i32)) -> Option<Path> {
        if !self.can_navigate(e) {
            return None;
        }
        if let Some(p) = &self.current_path {
            if !p.is_finished() && Some(pos) == self.target_pos {
                return self.current_path.clone();
            }
        }
        self.target_pos = Some(pos);
        let f = Self::get_path_search_range(e);
        self.finder.find_path_to_pos(w, e, pos, f)
    }

    /// `PathNavigateGround.getPathToEntityLiving`, for a target standing at `pos`.
    pub fn get_path_to_entity_living(&mut self, w: &impl BlockAccess, e: &mut NavEntity, target: DVec3) -> Option<Path> {
        self.get_path_to_pos(w, e, (floor(target.x), floor(target.y), floor(target.z)))
    }

    /// `PathNavigate.tryMoveToXYZ`
    pub fn try_move_to_xyz(&mut self, w: &impl BlockAccess, e: &mut NavEntity, x: f64, y: f64, z: f64, speed: f64) -> bool {
        let p = self.get_path_to_xyz(w, e, x, y, z);
        self.set_path(w, e, p, speed)
    }

    /// `PathNavigate.tryMoveToEntityLiving`, for a target standing at `target`.
    pub fn try_move_to_entity_living(&mut self, w: &impl BlockAccess, e: &mut NavEntity, target: DVec3, speed: f64) -> bool {
        match self.get_path_to_entity_living(w, e, target) {
            Some(p) => self.set_path(w, e, Some(p), speed),
            None => false,
        }
    }

    /// `PathNavigate.setPath`
    pub fn set_path(&mut self, w: &impl BlockAccess, e: &NavEntity, path: Option<Path>, speed: f64) -> bool {
        let Some(path) = path else {
            self.current_path = None;
            return false;
        };
        if !path.is_same_path(self.current_path.as_ref()) {
            self.current_path = Some(path);
        }
        self.remove_sunny_path(w, e);
        if self.current_path.as_ref().map_or(0, |p| p.get_current_path_length()) <= 0 {
            return false;
        }
        self.speed = speed;
        let v = self.get_entity_position(w, e);
        self.ticks_at_last_pos = self.total_ticks;
        self.last_pos_check = v;
        true
    }

    pub fn get_path(&self) -> Option<&Path> {
        self.current_path.as_ref()
    }

    /// `PathNavigate.onUpdateNavigation`
    pub fn on_update_navigation(&mut self, w: &impl BlockAccess, e: &mut NavEntity, mover: &mut EntityMoveHelper) {
        self.total_ticks += 1;
        if self.try_update_path {
            self.update_path(w, e);
        }
        if !self.no_path() {
            if self.can_navigate(e) {
                self.path_follow(w, e);
            } else if self.current_path.is_some() {
                let v = self.get_entity_position(w, e);
                let p = self.current_path.as_mut().expect("checked");
                if p.get_current_path_index() < p.get_current_path_length() {
                    let v1 = p.get_vector_from_index(e.width, p.get_current_path_index());
                    if v.y > v1.y && !e.on_ground && floor(v.x) == floor(v1.x) && floor(v.z) == floor(v1.z) {
                        p.set_current_path_index(p.get_current_path_index() + 1);
                    }
                }
            }
            if !self.no_path() {
                let mut v2 = self.current_path.as_ref().expect("has a path").get_position(e.width);
                let (bx, by, bz) = (floor(v2.x), floor(v2.y) - 1, floor(v2.z));
                let max_y = blocks::bounding_max_y(w, bx, by, bz);
                v2.y -= 1.0 - max_y;
                mover.set_move_to(v2.x, v2.y, v2.z, self.speed);
            }
        }
    }

    /// `PathNavigate.pathFollow`
    fn path_follow(&mut self, w: &impl BlockAccess, e: &NavEntity) {
        let v = self.get_entity_position(w, e);
        let path = self.current_path.as_mut().expect("has a path");
        let mut i = path.get_current_path_length();
        for j in path.get_current_path_index()..path.get_current_path_length() {
            if path.get_path_point_from_index(j).y as f64 != v.y.floor() {
                i = j;
                break;
            }
        }
        self.max_distance_to_waypoint = if e.width > 0.75 { e.width / 2.0 } else { 0.75 - e.width / 2.0 };
        let v1 = path.get_current_pos();
        if ((e.pos.x - (v1.x + 0.5)) as f32).abs() < self.max_distance_to_waypoint && ((e.pos.z - (v1.z + 0.5)) as f32).abs() < self.max_distance_to_waypoint && (e.pos.y - v1.y).abs() < 1.0 {
            path.set_current_path_index(path.get_current_path_index() + 1);
        }
        let k = ceil(e.width as f64);
        let l = ceil(e.height as f64);
        let i1 = k;
        let mut j1 = i - 1;
        while j1 >= path.get_current_path_index() {
            let target = path.get_vector_from_index(e.width, j1);
            if Self::is_direct_path_between_points(w, e, &self.finder.node_processor, v, target, k, l, i1) {
                path.set_current_path_index(j1);
                break;
            }
            j1 -= 1;
        }
        self.check_for_stuck(v, e.ai_move_speed);
    }

    /// `PathNavigate.checkForStuck`
    fn check_for_stuck(&mut self, pos: DVec3, ai_speed: f32) {
        if self.total_ticks - self.ticks_at_last_pos > 100 {
            if pos.distance_squared(self.last_pos_check) < 2.25 {
                self.clear_path();
            }
            self.ticks_at_last_pos = self.total_ticks;
            self.last_pos_check = pos;
        }
        if let Some(p) = &self.current_path {
            if !p.is_finished() {
                let v = p.get_current_pos();
                if v == self.timeout_cached_node {
                    self.timeout_timer += self.now_ms() - self.last_timeout_check;
                } else {
                    self.timeout_cached_node = v;
                    let d0 = pos.distance(self.timeout_cached_node);
                    self.timeout_limit = if ai_speed > 0.0 { d0 / ai_speed as f64 * 1000.0 } else { 0.0 };
                }
                if self.timeout_limit > 0.0 && self.timeout_timer as f64 > self.timeout_limit * 3.0 {
                    self.timeout_cached_node = DVec3::ZERO;
                    self.timeout_timer = 0;
                    self.timeout_limit = 0.0;
                    self.clear_path();
                }
                self.last_timeout_check = self.now_ms();
            }
        }
    }

    /// `PathNavigate.noPath`
    pub fn no_path(&self) -> bool {
        self.current_path.as_ref().is_none_or(|p| p.is_finished())
    }

    pub fn clear_path(&mut self) {
        self.current_path = None;
    }

    /// `PathNavigateGround.canNavigate`
    pub fn can_navigate(&self, e: &NavEntity) -> bool {
        e.on_ground || (self.get_can_swim() && Self::is_in_liquid(e))
    }

    /// `PathNavigate.isInLiquid`
    fn is_in_liquid(e: &NavEntity) -> bool {
        e.in_water || e.in_lava
    }

    /// `PathNavigateGround.getEntityPosition`
    fn get_entity_position(&self, w: &impl BlockAccess, e: &NavEntity) -> DVec3 {
        DVec3::new(e.pos.x, self.get_pathable_pos_y(w, e) as f64, e.pos.z)
    }

    fn get_pathable_pos_y(&self, w: &impl BlockAccess, e: &NavEntity) -> i32 {
        if e.in_water && self.get_can_swim() {
            Self::pathable_pos_y_swimming(w, e)
        } else {
            (e.bb.min_y + 0.5) as i32
        }
    }

    fn pathable_pos_y_swimming(w: &impl BlockAccess, e: &NavEntity) -> i32 {
        let mut i = e.bb.min_y as i32;
        let (x, z) = (floor(e.pos.x), floor(e.pos.z));
        let mut block = w.state(x, i, z) >> 4;
        let mut j = 0;
        while block == id::FLOWING_WATER || block == id::WATER {
            i += 1;
            block = w.state(x, i, z) >> 4;
            j += 1;
            if j > 16 {
                return e.bb.min_y as i32;
            }
        }
        i
    }

    /// `PathNavigate.removeSunnyPath`: lifts a path point in a cauldron by one block.
    /// Then `PathNavigateGround.removeSunnyPath`: with `shouldAvoidSun` the path is cut before
    /// its first point under open sky, and left alone when the mob already stands under it.
    fn remove_sunny_path(&mut self, w: &impl BlockAccess, e: &NavEntity) {
        let Some(path) = &mut self.current_path else { return };
        for i in 0..path.get_current_path_length() {
            let p = path.get_path_point_from_index(i).clone();
            let p1 = if i + 1 < path.get_current_path_length() { Some(path.get_path_point_from_index(i + 1).clone()) } else { None };
            if w.state(p.x, p.y, p.z) >> 4 == id::CAULDRON {
                path.set_point(i, p.clone_move(p.x, p.y + 1, p.z));
                if let Some(p1) = p1 {
                    if p.y >= p1.y {
                        path.set_point(i + 1, p1.clone_move(p1.x, p.y + 1, p1.z));
                    }
                }
            }
        }
        if self.should_avoid_sun {
            if blocks::can_see_sky(w, floor(e.pos.x), (e.bb.min_y + 0.5) as i32, floor(e.pos.z)) {
                return;
            }
            let Some(path) = &mut self.current_path else { return };
            for i in 0..path.get_current_path_length() {
                let p = path.get_path_point_from_index(i);
                if blocks::can_see_sky(w, p.x, p.y, p.z) {
                    path.set_current_path_length(i - 1);
                    return;
                }
            }
        }
    }

    /// `PathNavigateGround.isDirectPathBetweenPoints`
    #[allow(clippy::too_many_arguments)]
    fn is_direct_path_between_points(w: &impl BlockAccess, e: &NavEntity, nodes: &WalkNodeProcessor, v1: DVec3, v2: DVec3, size_x: i32, size_y: i32, size_z: i32) -> bool {
        let mut i = floor(v1.x);
        let mut j = floor(v1.z);
        let mut d0 = v2.x - v1.x;
        let mut d1 = v2.z - v1.z;
        let d2 = d0 * d0 + d1 * d1;
        if d2 < 1.0e-8 {
            return false;
        }
        let d3 = 1.0 / d2.sqrt();
        d0 *= d3;
        d1 *= d3;
        let (sx, sz) = (size_x + 2, size_z + 2);
        if !Self::is_safe_to_stand_at(w, e, nodes, i, v1.y as i32, j, sx, size_y, sz, v1, d0, d1) {
            return false;
        }
        let (sx, sz) = (sx - 2, sz - 2);
        let d4 = 1.0 / d0.abs();
        let d5 = 1.0 / d1.abs();
        let mut d6 = i as f64 - v1.x;
        let mut d7 = j as f64 - v1.z;
        if d0 >= 0.0 {
            d6 += 1.0;
        }
        if d1 >= 0.0 {
            d7 += 1.0;
        }
        d6 /= d0;
        d7 /= d1;
        let k = if d0 < 0.0 { -1 } else { 1 };
        let l = if d1 < 0.0 { -1 } else { 1 };
        let i1 = floor(v2.x);
        let j1 = floor(v2.z);
        let mut k1 = i1 - i;
        let mut l1 = j1 - j;
        while k1 * k > 0 || l1 * l > 0 {
            if d6 < d7 {
                d6 += d4;
                i += k;
                k1 = i1 - i;
            } else {
                d7 += d5;
                j += l;
                l1 = j1 - j;
            }
            if !Self::is_safe_to_stand_at(w, e, nodes, i, v1.y as i32, j, sx, size_y, sz, v1, d0, d1) {
                return false;
            }
        }
        true
    }

    /// `PathNavigateGround.isSafeToStandAt`
    #[allow(clippy::too_many_arguments)]
    fn is_safe_to_stand_at(w: &impl BlockAccess, e: &NavEntity, nodes: &WalkNodeProcessor, x: i32, y: i32, z: i32, size_x: i32, size_y: i32, size_z: i32, v1: DVec3, p8: f64, p10: f64) -> bool {
        let i = x - size_x / 2;
        let j = z - size_z / 2;
        if !Self::is_position_clear(w, i, y, j, size_x, size_y, size_z, v1, p8, p10) {
            return false;
        }
        for k in i..i + size_x {
            for l in j..j + size_z {
                let d0 = k as f64 + 0.5 - v1.x;
                let d1 = l as f64 + 0.5 - v1.z;
                if d0 * p8 + d1 * p10 >= 0.0 {
                    let mut t = nodes.get_path_node_type_sized(w, k, y - 1, l, e, (size_x, size_y, size_z), true, true);
                    if matches!(t, PathNodeType::Water | PathNodeType::Lava | PathNodeType::Open) {
                        return false;
                    }
                    t = nodes.get_path_node_type_sized(w, k, y, l, e, (size_x, size_y, size_z), true, true);
                    let f = e.priorities.get(t);
                    if !(0.0..8.0).contains(&f) {
                        return false;
                    }
                    if matches!(t, PathNodeType::DamageFire | PathNodeType::DangerFire | PathNodeType::DamageOther) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// `PathNavigateGround.isPositionClear`
    #[allow(clippy::too_many_arguments)]
    fn is_position_clear(w: &impl BlockAccess, x: i32, y: i32, z: i32, size_x: i32, size_y: i32, size_z: i32, v: DVec3, p8: f64, p10: f64) -> bool {
        for bx in x..x + size_x {
            for by in y..y + size_y {
                for bz in z..z + size_z {
                    let d0 = bx as f64 + 0.5 - v.x;
                    let d1 = bz as f64 + 0.5 - v.z;
                    if d0 * p8 + d1 * p10 >= 0.0 && !blocks::is_passable(w, bx, by, bz) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// `PathNavigate.canEntityStandOnPos`: the block below is a full block.
    pub fn can_entity_stand_on_pos(w: &impl BlockAccess, pos: (i32, i32, i32)) -> bool {
        sim::collision::is_normal_cube(w.state(pos.0, pos.1 - 1, pos.2))
    }

    pub fn set_break_doors(&mut self, v: bool) {
        self.finder.node_processor.set_can_open_doors(v);
    }

    pub fn set_enter_doors(&mut self, v: bool) {
        self.finder.node_processor.set_can_enter_doors(v);
    }

    pub fn get_enter_doors(&self) -> bool {
        self.finder.node_processor.get_can_enter_doors()
    }

    pub fn set_can_swim(&mut self, v: bool) {
        self.finder.node_processor.set_can_swim(v);
    }

    pub fn get_can_swim(&self) -> bool {
        self.finder.node_processor.get_can_swim()
    }
}
