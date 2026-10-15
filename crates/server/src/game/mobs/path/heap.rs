//! `PathHeap`: the binary min-heap on `distanceToTarget`. The points live in the node
//! processor's arena; the heap holds their indices. DROPPED: the array growth (a `Vec`).

use super::point::PathPoint;

#[derive(Clone, Debug, Default)]
pub struct PathHeap {
    path_points: Vec<usize>,
}

impl PathHeap {
    /// `PathHeap.addPoint`
    pub fn add_point(&mut self, arena: &mut [PathPoint], point: usize) -> usize {
        assert!(arena[point].index < 0, "OW KNOWS!");
        self.path_points.push(point);
        arena[point].index = self.path_points.len() as i32 - 1;
        self.sort_back(arena, self.path_points.len() - 1);
        point
    }

    pub fn clear_path(&mut self) {
        self.path_points.clear();
    }

    /// `PathHeap.dequeue`
    pub fn dequeue(&mut self, arena: &mut [PathPoint]) -> usize {
        let point = self.path_points[0];
        let last = self.path_points.pop().expect("non-empty");
        if !self.path_points.is_empty() {
            self.path_points[0] = last;
            self.sort_forward(arena, 0);
        }
        arena[point].index = -1;
        point
    }

    /// `PathHeap.changeDistance`
    pub fn change_distance(&mut self, arena: &mut [PathPoint], point: usize, distance: f32) {
        let f = arena[point].distance_to_target;
        arena[point].distance_to_target = distance;
        let index = arena[point].index as usize;
        if distance < f {
            self.sort_back(arena, index);
        } else {
            self.sort_forward(arena, index);
        }
    }

    fn sort_back(&mut self, arena: &mut [PathPoint], mut index: usize) {
        let point = self.path_points[index];
        let f = arena[point].distance_to_target;
        while index > 0 {
            let i = (index - 1) >> 1;
            let p1 = self.path_points[i];
            if f >= arena[p1].distance_to_target {
                break;
            }
            self.path_points[index] = p1;
            arena[p1].index = index as i32;
            index = i;
        }
        self.path_points[index] = point;
        arena[point].index = index as i32;
    }

    fn sort_forward(&mut self, arena: &mut [PathPoint], mut index: usize) {
        let point = self.path_points[index];
        let f = arena[point].distance_to_target;
        let count = self.path_points.len();
        loop {
            let i = 1 + (index << 1);
            let j = i + 1;
            if i >= count {
                break;
            }
            let p1 = self.path_points[i];
            let f1 = arena[p1].distance_to_target;
            let (p2, f2) = if j >= count {
                (None, f32::INFINITY)
            } else {
                let p = self.path_points[j];
                (Some(p), arena[p].distance_to_target)
            };
            if f1 < f2 {
                if f1 >= f {
                    break;
                }
                self.path_points[index] = p1;
                arena[p1].index = index as i32;
                index = i;
            } else {
                if f2 >= f {
                    break;
                }
                let p2 = p2.expect("f2 finite");
                self.path_points[index] = p2;
                arena[p2].index = index as i32;
                index = j;
            }
        }
        self.path_points[index] = point;
        arena[point].index = index as i32;
    }

    pub fn is_path_empty(&self) -> bool {
        self.path_points.is_empty()
    }
}
