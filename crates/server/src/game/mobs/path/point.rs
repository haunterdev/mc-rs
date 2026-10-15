//! `PathPoint` and `Path`. DROPPED: the client-only debug sets and `createFromBuffer` / `read`.
//! A point's `previous` is an index into the node processor's arena (Java holds the reference);
//! a finished [`Path`] owns copies and never follows it.

use glam::DVec3;

use super::math::sqrt_f;
use super::node::PathNodeType;

/// `PathPoint`
#[derive(Clone, Debug)]
pub struct PathPoint {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    hash: i32,
    /// Position in the `PathHeap`, -1 when not queued.
    pub index: i32,
    pub total_path_distance: f32,
    pub distance_to_next: f32,
    pub distance_to_target: f32,
    pub previous: Option<usize>,
    pub visited: bool,
    pub distance_from_origin: f32,
    pub cost: f32,
    pub cost_malus: f32,
    pub node_type: PathNodeType,
}

impl PathPoint {
    pub fn new(x: i32, y: i32, z: i32) -> PathPoint {
        PathPoint {
            x,
            y,
            z,
            hash: Self::make_hash(x, y, z),
            index: -1,
            total_path_distance: 0.0,
            distance_to_next: 0.0,
            distance_to_target: 0.0,
            previous: None,
            visited: false,
            distance_from_origin: 0.0,
            cost: 0.0,
            cost_malus: 0.0,
            node_type: PathNodeType::Blocked,
        }
    }

    /// `PathPoint.cloneMove`
    pub fn clone_move(&self, x: i32, y: i32, z: i32) -> PathPoint {
        PathPoint { x, y, z, hash: Self::make_hash(x, y, z), ..self.clone() }
    }

    /// `PathPoint.makeHash`
    pub fn make_hash(x: i32, y: i32, z: i32) -> i32 {
        (y & 255) | ((x & 32767) << 8) | ((z & 32767) << 24) | if x < 0 { i32::MIN } else { 0 } | if z < 0 { 0x8000 } else { 0 }
    }

    pub fn distance_to(&self, o: &PathPoint) -> f32 {
        sqrt_f(self.distance_to_squared(o))
    }

    pub fn distance_to_squared(&self, o: &PathPoint) -> f32 {
        let f = (o.x - self.x) as f32;
        let f1 = (o.y - self.y) as f32;
        let f2 = (o.z - self.z) as f32;
        f * f + f1 * f1 + f2 * f2
    }

    /// `PathPoint.distanceManhattan`
    pub fn distance_manhattan(&self, o: &PathPoint) -> f32 {
        (o.x - self.x).abs() as f32 + (o.y - self.y).abs() as f32 + (o.z - self.z).abs() as f32
    }

    /// `PathPoint.equals`
    pub fn same_as(&self, o: &PathPoint) -> bool {
        self.hash == o.hash && self.x == o.x && self.y == o.y && self.z == o.z
    }

    pub fn is_assigned(&self) -> bool {
        self.index >= 0
    }
}

/// `Path`
#[derive(Clone, Debug)]
pub struct Path {
    points: Vec<PathPoint>,
    current_path_index: i32,
    path_length: i32,
}

impl Path {
    pub fn new(points: Vec<PathPoint>) -> Path {
        let path_length = points.len() as i32;
        Path { points, current_path_index: 0, path_length }
    }

    pub fn increment_path_index(&mut self) {
        self.current_path_index += 1;
    }

    pub fn is_finished(&self) -> bool {
        self.current_path_index >= self.path_length
    }

    pub fn get_final_path_point(&self) -> Option<&PathPoint> {
        if self.path_length > 0 {
            self.points.get(self.path_length as usize - 1)
        } else {
            None
        }
    }

    pub fn get_path_point_from_index(&self, index: i32) -> &PathPoint {
        &self.points[index as usize]
    }

    pub fn set_point(&mut self, index: i32, point: PathPoint) {
        self.points[index as usize] = point;
    }

    pub fn get_current_path_length(&self) -> i32 {
        self.path_length
    }

    pub fn set_current_path_length(&mut self, length: i32) {
        self.path_length = length;
    }

    pub fn get_current_path_index(&self) -> i32 {
        self.current_path_index
    }

    pub fn set_current_path_index(&mut self, i: i32) {
        self.current_path_index = i;
    }

    /// `Path.getVectorFromIndex`: the point's cell, shifted by half the entity's cell footprint.
    pub fn get_vector_from_index(&self, width: f32, index: i32) -> DVec3 {
        let p = &self.points[index as usize];
        let half = (width + 1.0) as i32 as f64 * 0.5;
        DVec3::new(p.x as f64 + half, p.y as f64, p.z as f64 + half)
    }

    pub fn get_position(&self, width: f32) -> DVec3 {
        self.get_vector_from_index(width, self.current_path_index)
    }

    pub fn get_current_pos(&self) -> DVec3 {
        let p = &self.points[self.current_path_index as usize];
        DVec3::new(p.x as f64, p.y as f64, p.z as f64)
    }

    /// `Path.isSamePath`
    pub fn is_same_path(&self, other: Option<&Path>) -> bool {
        let Some(o) = other else { return false };
        o.points.len() == self.points.len() && self.points.iter().zip(&o.points).all(|(a, b)| a.x == b.x && a.y == b.y && a.z == b.z)
    }

    /// The cells of the path, for tests and logs.
    pub fn cells(&self) -> Vec<(i32, i32, i32)> {
        self.points.iter().map(|p| (p.x, p.y, p.z)).collect()
    }
}
