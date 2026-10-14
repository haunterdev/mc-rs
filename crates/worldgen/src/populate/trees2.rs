//! `WorldGenBigTree`, `WorldGenCanopyTree` and `WorldGenSavannaTree`.
//!
//! `Biome.BIG_TREE_FEATURE` is one static `WorldGenBigTree`, and its `heightLimit` and
//! `leafDistanceLimit` fields are never reset between calls, so they live in `PopStatics` and
//! carry from tree to tree exactly as the Java instance fields do.

use super::props::material;
use super::state::{self, Facing, Pos};
use super::trees::{self, ACACIA_LEAF, ACACIA_LOG, DARK_OAK_LEAF, DARK_OAK_LOG, OAK_LEAF};
use super::world::PopWorld;
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;
use crate::math_helper;

const HEIGHT_ATTENUATION: f64 = 0.618;
const BRANCH_SLOPE: f64 = 0.381;
const SCALE_WIDTH: f64 = 1.0;
const LEAF_DENSITY: f64 = 1.0;
const HEIGHT_LIMIT_LIMIT: i32 = 12;
// DROPPED: trunkSize is the constant 1 for every caller, so the trunkSize == 2 limbs in
// generateTrunk never run.

/// `BlockPos.add(double, double, double)`: `new BlockPos(getX() + x, ...)`, each floored.
fn add_d(p: Pos, x: f64, y: f64, z: f64) -> Pos {
    Pos::new(
        math_helper::floor(p.x as f64 + x),
        math_helper::floor(p.y as f64 + y),
        math_helper::floor(p.z as f64 + z),
    )
}

/// `Math.pow(x, 2.0D)`
fn sq(x: f64) -> f64 {
    x * x
}

struct BigTree {
    rand: JavaRandom,
    base: Pos,
    height_limit: i32,
    height: i32,
    leaf_distance_limit: i32,
    /// `FoliageCoordinates`: the node and its branch base y.
    foliage: Vec<(Pos, i32)>,
}

/// `WorldGenBigTree.generate`
pub fn big_tree(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    let mut t = BigTree {
        rand: JavaRandom::new(rand.next_long()),
        base: position,
        height_limit: w.statics.big_tree_height_limit,
        height: 0,
        leaf_distance_limit: w.statics.big_tree_leaf_distance_limit,
        foliage: Vec::new(),
    };
    if t.height_limit == 0 {
        t.height_limit = 5 + t.rand.next_int_bounded(HEIGHT_LIMIT_LIMIT);
    }
    let ok = t.valid_tree_location(w);
    if ok {
        t.generate_leaf_node_list(w);
        t.generate_leaves(w);
        t.generate_trunk(w);
        t.generate_leaf_node_bases(w);
    }
    w.statics.big_tree_height_limit = t.height_limit;
    ok
}

impl BigTree {
    /// `WorldGenBigTree.generateLeafNodeList`
    fn generate_leaf_node_list(&mut self, w: &mut PopWorld) {
        self.height = (self.height_limit as f64 * HEIGHT_ATTENUATION) as i32;
        if self.height >= self.height_limit {
            self.height = self.height_limit - 1;
        }
        let mut i = (1.382 + sq(LEAF_DENSITY * self.height_limit as f64 / 13.0)) as i32;
        if i < 1 {
            i = 1;
        }
        let j = self.base.y + self.height;
        let mut k = self.height_limit - self.leaf_distance_limit;
        self.foliage.clear();
        self.foliage.push((self.base.up_n(k), j));
        while k >= 0 {
            let f = self.layer_size(k);
            if f >= 0.0 {
                for _ in 0..i {
                    let d0 = SCALE_WIDTH * f as f64 * (self.rand.next_float() as f64 + 0.328);
                    let d1 = (self.rand.next_float() * 2.0) as f64 * 3.141592653589793;
                    // Math.sin and Math.cos: std f64, see DECOMP_NOTES.
                    let d2 = d0 * d1.sin() + 0.5;
                    let d3 = d0 * d1.cos() + 0.5;
                    let bp = add_d(self.base, d2, (k - 1) as f64, d3);
                    let bp1 = bp.up_n(self.leaf_distance_limit);
                    if self.check_block_line(w, bp, bp1) == -1 {
                        let i1 = self.base.x - bp.x;
                        let j1 = self.base.z - bp.z;
                        let d4 = bp.y as f64 - ((i1 * i1 + j1 * j1) as f64).sqrt() * BRANCH_SLOPE;
                        let k1 = if d4 > j as f64 { j } else { d4 as i32 };
                        let bp2 = Pos::new(self.base.x, k1, self.base.z);
                        if self.check_block_line(w, bp2, bp) == -1 {
                            self.foliage.push((bp, bp2.y));
                        }
                    }
                }
            }
            k -= 1;
        }
    }

    /// `WorldGenBigTree.crosSection`
    fn cros_section(&self, w: &mut PopWorld, pos: Pos, size: f32, s: u16) {
        let i = (size as f64 + 0.618) as i32;
        for j in -i..=i {
            for k in -i..=i {
                if sq(j.abs() as f64 + 0.5) + sq(k.abs() as f64 + 0.5) <= (size * size) as f64 {
                    let bp = pos.add(j, 0, k);
                    if trees::air_or_leaves(w, bp) {
                        w.set(bp, s, 2);
                    }
                }
            }
        }
    }

    /// `WorldGenBigTree.layerSize`
    fn layer_size(&self, y: i32) -> f32 {
        if (y as f32) < self.height_limit as f32 * 0.3 {
            return -1.0;
        }
        let f = self.height_limit as f32 / 2.0;
        let f1 = f - y as f32;
        let mut f2 = math_helper::sqrt_f(f * f - f1 * f1);
        if f1 == 0.0 {
            f2 = f;
        } else if f1.abs() >= f {
            return 0.0;
        }
        f2 * 0.5
    }

    /// `WorldGenBigTree.leafSize`
    fn leaf_size(&self, y: i32) -> f32 {
        if y >= 0 && y < self.leaf_distance_limit {
            if y != 0 && y != self.leaf_distance_limit - 1 { 3.0 } else { 2.0 }
        } else {
            -1.0
        }
    }

    /// `WorldGenBigTree.generateLeafNode`
    fn generate_leaf_node(&self, w: &mut PopWorld, pos: Pos) {
        for i in 0..self.leaf_distance_limit {
            self.cros_section(w, pos.up_n(i), self.leaf_size(i), OAK_LEAF);
        }
    }

    /// `WorldGenBigTree.limb` with `Blocks.LOG`
    fn limb(&self, w: &mut PopWorld, p1: Pos, p2: Pos) {
        let d = Pos::new(p2.x - p1.x, p2.y - p1.y, p2.z - p1.z);
        let i = greatest_distance(d);
        let f = d.x as f32 / i as f32;
        let f1 = d.y as f32 / i as f32;
        let f2 = d.z as f32 / i as f32;
        for j in 0..=i {
            let bp1 = add_d(
                p1,
                (0.5 + j as f32 * f) as f64,
                (0.5 + j as f32 * f1) as f64,
                (0.5 + j as f32 * f2) as f64,
            );
            let axis = log_axis(p1, bp1);
            w.set(bp1, blocks::state(id::LOG, axis << 2), 2);
        }
    }

    /// `WorldGenBigTree.generateLeaves`
    fn generate_leaves(&self, w: &mut PopWorld) {
        for &(p, _) in &self.foliage {
            self.generate_leaf_node(w, p);
        }
    }

    /// `WorldGenBigTree.leafNodeNeedsBase`
    fn leaf_node_needs_base(&self, y: i32) -> bool {
        y as f64 >= self.height_limit as f64 * 0.2
    }

    /// `WorldGenBigTree.generateTrunk`
    fn generate_trunk(&self, w: &mut PopWorld) {
        self.limb(w, self.base, self.base.up_n(self.height));
    }

    /// `WorldGenBigTree.generateLeafNodeBases`
    fn generate_leaf_node_bases(&self, w: &mut PopWorld) {
        for &(p, i) in &self.foliage {
            let bp = Pos::new(self.base.x, i, self.base.z);
            if bp != p && self.leaf_node_needs_base(i - self.base.y) {
                self.limb(w, bp, p);
            }
        }
    }

    /// `WorldGenBigTree.checkBlockLine`
    fn check_block_line(&self, w: &mut PopWorld, one: Pos, two: Pos) -> i32 {
        let d = Pos::new(two.x - one.x, two.y - one.y, two.z - one.z);
        let i = greatest_distance(d);
        if i == 0 {
            return -1;
        }
        let f = d.x as f32 / i as f32;
        let f1 = d.y as f32 / i as f32;
        let f2 = d.z as f32 / i as f32;
        for j in 0..=i {
            let bp1 = add_d(
                one,
                (0.5 + j as f32 * f) as f64,
                (0.5 + j as f32 * f1) as f64,
                (0.5 + j as f32 * f2) as f64,
            );
            if !trees::grows_at(w, bp1) {
                return j;
            }
        }
        -1
    }

    /// `WorldGenBigTree.validTreeLocation`
    fn valid_tree_location(&mut self, w: &mut PopWorld) -> bool {
        let block = blocks::block_of(w.get(self.base.down()));
        if block != id::DIRT && block != id::GRASS && block != id::FARMLAND {
            return false;
        }
        let i = self.check_block_line(w, self.base, self.base.up_n(self.height_limit - 1));
        if i == -1 {
            true
        } else if i < 6 {
            false
        } else {
            self.height_limit = i;
            true
        }
    }
}

/// `WorldGenBigTree.getGreatestDistance`
fn greatest_distance(p: Pos) -> i32 {
    let i = p.x.abs();
    let j = p.y.abs();
    let k = p.z.abs();
    if k > i && k > j {
        k
    } else if j > i {
        j
    } else {
        i
    }
}

/// `WorldGenBigTree.getLogAxis`: 0 Y, 1 X, 2 Z (the log metadata's axis bits).
fn log_axis(a: Pos, b: Pos) -> u16 {
    let mut axis = 0;
    let i = (b.x - a.x).abs();
    let j = (b.z - a.z).abs();
    let k = i.max(j);
    if k > 0 {
        if i == k {
            axis = 1;
        } else if j == k {
            axis = 2;
        }
    }
    axis
}

/// `WorldGenCanopyTree.generate`
pub fn canopy(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    let i = rand.next_int_bounded(3) + rand.next_int_bounded(2) + 6;
    let j = position.x;
    let k = position.y;
    let l = position.z;
    if !(k >= 1 && k + i + 1 < 256) {
        return false;
    }
    let bp = position.down();
    let block = blocks::block_of(w.get(bp));
    if block != id::GRASS && block != id::DIRT {
        return false;
    }
    if !canopy_place_tree_of_height(w, position, i) {
        return false;
    }
    trees::set_dirt_at(w, bp);
    trees::set_dirt_at(w, bp.east());
    trees::set_dirt_at(w, bp.south());
    trees::set_dirt_at(w, bp.south().east());
    let facing = Facing::random_horizontal(rand);
    let i1 = i - rand.next_int_bounded(4);
    let mut j1 = 2 - rand.next_int_bounded(3);
    let mut k1 = j;
    let mut l1 = l;
    let i2 = k + i - 1;
    for j2 in 0..i {
        if j2 >= i1 && j1 > 0 {
            let (dx, _, dz) = facing.offset();
            k1 += dx;
            l1 += dz;
            j1 -= 1;
        }
        let k2 = k + j2;
        let bp1 = Pos::new(k1, k2, l1);
        if trees::air_or_leaves(w, bp1) {
            canopy_log(w, bp1);
            canopy_log(w, bp1.east());
            canopy_log(w, bp1.south());
            canopy_log(w, bp1.east().south());
        }
    }
    for i3 in -2..=0 {
        for l3 in -2..=0 {
            let mut k4 = -1;
            canopy_leaf(w, k1 + i3, i2 + k4, l1 + l3);
            canopy_leaf(w, 1 + k1 - i3, i2 + k4, l1 + l3);
            canopy_leaf(w, k1 + i3, i2 + k4, 1 + l1 - l3);
            canopy_leaf(w, 1 + k1 - i3, i2 + k4, 1 + l1 - l3);
            if (i3 > -2 || l3 > -1) && (i3 != -1 || l3 != -2) {
                k4 = 1;
                canopy_leaf(w, k1 + i3, i2 + k4, l1 + l3);
                canopy_leaf(w, 1 + k1 - i3, i2 + k4, l1 + l3);
                canopy_leaf(w, k1 + i3, i2 + k4, 1 + l1 - l3);
                canopy_leaf(w, 1 + k1 - i3, i2 + k4, 1 + l1 - l3);
            }
        }
    }
    if rand.next_boolean() {
        canopy_leaf(w, k1, i2 + 2, l1);
        canopy_leaf(w, k1 + 1, i2 + 2, l1);
        canopy_leaf(w, k1 + 1, i2 + 2, l1 + 1);
        canopy_leaf(w, k1, i2 + 2, l1 + 1);
    }
    for j3 in -3i32..=4 {
        for i4 in -3i32..=4 {
            if (j3 != -3 || i4 != -3)
                && (j3 != -3 || i4 != 4)
                && (j3 != 4 || i4 != -3)
                && (j3 != 4 || i4 != 4)
                && (j3.abs() < 3 || i4.abs() < 3)
            {
                canopy_leaf(w, k1 + j3, i2, l1 + i4);
            }
        }
    }
    for k3 in -1..=2 {
        for j4 in -1..=2 {
            if (k3 < 0 || k3 > 1 || j4 < 0 || j4 > 1) && rand.next_int_bounded(3) <= 0 {
                let l4 = rand.next_int_bounded(3) + 2;
                for i5 in 0..l4 {
                    canopy_log(w, Pos::new(j + k3, i2 - i5 - 1, l + j4));
                }
                for j5 in -1..=1 {
                    for l2 in -1..=1 {
                        canopy_leaf(w, k1 + k3 + j5, i2, l1 + j4 + l2);
                    }
                }
                for k5 in -2i32..=2 {
                    for l5 in -2i32..=2 {
                        if k5.abs() != 2 || l5.abs() != 2 {
                            canopy_leaf(w, k1 + k3 + k5, i2 - 1, l1 + j4 + l5);
                        }
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenCanopyTree.placeTreeOfHeight`
fn canopy_place_tree_of_height(w: &mut PopWorld, pos: Pos, height: i32) -> bool {
    for l in 0..=height + 1 {
        let mut i1 = 1;
        if l == 0 {
            i1 = 0;
        }
        if l >= height - 1 {
            i1 = 2;
        }
        for j1 in -i1..=i1 {
            for k1 in -i1..=i1 {
                if !trees::grows_at(w, Pos::new(pos.x + j1, pos.y + l, pos.z + k1)) {
                    return false;
                }
            }
        }
    }
    true
}

/// `WorldGenCanopyTree.placeLogAt`
fn canopy_log(w: &mut PopWorld, p: Pos) {
    if trees::grows_at(w, p) {
        w.set(p, DARK_OAK_LOG, 2);
    }
}

/// `WorldGenCanopyTree.placeLeafAt`
fn canopy_leaf(w: &mut PopWorld, x: i32, y: i32, z: i32) {
    let bp = Pos::new(x, y, z);
    if state::mat(w.get(bp)) == material::AIR {
        w.set(bp, DARK_OAK_LEAF, 2);
    }
}

/// `WorldGenSavannaTree.placeLeafAt`
fn savanna_leaf(w: &mut PopWorld, p: Pos) {
    if trees::air_or_leaves(w, p) {
        w.set(p, ACACIA_LEAF, 2);
    }
}

/// `WorldGenSavannaTree.generate`
pub fn savanna(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    let i = rand.next_int_bounded(3) + rand.next_int_bounded(3) + 5;
    if !(position.y >= 1 && position.y + i + 1 <= 256) {
        return false;
    }
    if !trees::standard_space(w, position, i) {
        return false;
    }
    let block = blocks::block_of(w.get(position.down()));
    if !((block == id::GRASS || block == id::DIRT) && position.y < 256 - i - 1) {
        return false;
    }
    trees::set_dirt_at(w, position.down());
    let facing = Facing::random_horizontal(rand);
    let k2 = i - rand.next_int_bounded(4) - 1;
    let mut l2 = 3 - rand.next_int_bounded(3);
    let mut i3 = position.x;
    let mut j1 = position.z;
    let mut k1 = 0;
    for l1 in 0..i {
        let i2 = position.y + l1;
        if l1 >= k2 && l2 > 0 {
            let (dx, _, dz) = facing.offset();
            i3 += dx;
            j1 += dz;
            l2 -= 1;
        }
        let bp = Pos::new(i3, i2, j1);
        if trees::air_or_leaves(w, bp) {
            w.set(bp, ACACIA_LOG, 2);
            k1 = i2;
        }
    }
    let mut bp2 = Pos::new(i3, k1, j1);
    for j3 in -3i32..=3 {
        for i4 in -3i32..=3 {
            if j3.abs() != 3 || i4.abs() != 3 {
                savanna_leaf(w, bp2.add(j3, 0, i4));
            }
        }
    }
    bp2 = bp2.up();
    for k3 in -1..=1 {
        for j4 in -1..=1 {
            savanna_leaf(w, bp2.add(k3, 0, j4));
        }
    }
    savanna_leaf(w, bp2.offset_n(Facing::East, 2));
    savanna_leaf(w, bp2.offset_n(Facing::West, 2));
    savanna_leaf(w, bp2.offset_n(Facing::South, 2));
    savanna_leaf(w, bp2.offset_n(Facing::North, 2));
    i3 = position.x;
    j1 = position.z;
    let facing1 = Facing::random_horizontal(rand);
    if facing1 != facing {
        let l3 = k2 - rand.next_int_bounded(2) - 1;
        let mut k4 = 1 + rand.next_int_bounded(3);
        k1 = 0;
        let mut l4 = l3;
        while l4 < i && k4 > 0 {
            if l4 >= 1 {
                let j2 = position.y + l4;
                let (dx, _, dz) = facing1.offset();
                i3 += dx;
                j1 += dz;
                let bp1 = Pos::new(i3, j2, j1);
                if trees::air_or_leaves(w, bp1) {
                    w.set(bp1, ACACIA_LOG, 2);
                    k1 = j2;
                }
            }
            l4 += 1;
            k4 -= 1;
        }
        if k1 > 0 {
            let mut bp3 = Pos::new(i3, k1, j1);
            for i5 in -2i32..=2 {
                for k5 in -2i32..=2 {
                    if i5.abs() != 2 || k5.abs() != 2 {
                        savanna_leaf(w, bp3.add(i5, 0, k5));
                    }
                }
            }
            bp3 = bp3.up();
            for j5 in -1..=1 {
                for l5 in -1..=1 {
                    savanna_leaf(w, bp3.add(j5, 0, l5));
                }
            }
        }
    }
    true
}
