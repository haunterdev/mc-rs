//! `WorldGenHugeTrees` and its population subclasses `WorldGenMegaJungle` and
//! `WorldGenMegaPineTree`.

use super::props::material;
use super::state::{self, Facing, Pos};
use super::trees::{self, SPRUCE_LEAF, SPRUCE_LOG};
use super::world::PopWorld;
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;
use crate::math_helper;

/// `WorldGenHugeTrees.getHeight`
fn huge_height(rand: &mut JavaRandom, base: i32, extra: i32) -> i32 {
    let mut i = rand.next_int_bounded(3) + base;
    if extra > 1 {
        i += rand.next_int_bounded(extra);
    }
    i
}

/// `WorldGenHugeTrees.isSpaceAt`
fn is_space_at(w: &mut PopWorld, leaves_pos: Pos, height: i32) -> bool {
    let mut flag = true;
    if !(leaves_pos.y >= 1 && leaves_pos.y + height + 1 <= 256) {
        return false;
    }
    for i in 0..=1 + height {
        let mut j = 2;
        if i == 0 {
            j = 1;
        } else if i >= 1 + height - 2 {
            j = 2;
        }
        let mut k = -j;
        while k <= j && flag {
            let mut l = -j;
            while l <= j && flag {
                if leaves_pos.y + i < 0 || leaves_pos.y + i >= 256 || !trees::grows_at(w, leaves_pos.add(k, i, l)) {
                    flag = false;
                }
                l += 1;
            }
            k += 1;
        }
    }
    flag
}

/// `WorldGenHugeTrees.ensureDirtsUnderneath`
fn ensure_dirts_underneath(w: &mut PopWorld, pos: Pos) -> bool {
    let bp = pos.down();
    let block = blocks::block_of(w.get(bp));
    if (block == id::GRASS || block == id::DIRT) && pos.y >= 2 {
        trees::set_dirt_at(w, bp);
        trees::set_dirt_at(w, bp.east());
        trees::set_dirt_at(w, bp.south());
        trees::set_dirt_at(w, bp.south().east());
        true
    } else {
        false
    }
}

/// `WorldGenHugeTrees.ensureGrowable`
fn ensure_growable(w: &mut PopWorld, tree_pos: Pos, height: i32) -> bool {
    is_space_at(w, tree_pos, height) && ensure_dirts_underneath(w, tree_pos)
}

/// `WorldGenHugeTrees.growLeavesLayerStrict`
fn grow_leaves_layer_strict(w: &mut PopWorld, center: Pos, width: i32, leaves: u16) {
    let i = width * width;
    for j in -width..=width + 1 {
        for k in -width..=width + 1 {
            let l = j - 1;
            let i1 = k - 1;
            if j * j + k * k <= i || l * l + i1 * i1 <= i || j * j + i1 * i1 <= i || l * l + k * k <= i {
                let bp = center.add(j, 0, k);
                if trees::air_or_leaves(w, bp) {
                    w.set(bp, leaves, 2);
                }
            }
        }
    }
}

/// `WorldGenHugeTrees.growLeavesLayer`
fn grow_leaves_layer(w: &mut PopWorld, center: Pos, width: i32, leaves: u16) {
    let i = width * width;
    for j in -width..=width {
        for k in -width..=width {
            if j * j + k * k <= i {
                let bp = center.add(j, 0, k);
                if trees::air_or_leaves(w, bp) {
                    w.set(bp, leaves, 2);
                }
            }
        }
    }
}

/// `WorldGenMegaJungle.generate`
pub fn mega_jungle(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos, base: i32, extra: i32, wood: u16, leaves: u16) -> bool {
    let i = huge_height(rand, base, extra);
    if !ensure_growable(w, position, i) {
        return false;
    }
    // createCrown(position.up(i), 2)
    let crown = position.up_n(i);
    for j in -2..=0 {
        grow_leaves_layer_strict(w, crown.up_n(j), 2 + 1 - j, leaves);
    }
    let mut j = position.y + i - 2 - rand.next_int_bounded(4);
    while j > position.y + i / 2 {
        let f = rand.next_float() * 6.2831855;
        let mut k = position.x + (0.5 + math_helper::cos(f) * 4.0) as i32;
        let mut l = position.z + (0.5 + math_helper::sin(f) * 4.0) as i32;
        for i1 in 0..5 {
            k = position.x + (1.5 + math_helper::cos(f) * i1 as f32) as i32;
            l = position.z + (1.5 + math_helper::sin(f) * i1 as f32) as i32;
            w.set(Pos::new(k, j - 3 + i1 / 2, l), wood, 2);
        }
        let j2 = 1 + rand.next_int_bounded(2);
        let j1 = j;
        for k1 in j - j2..=j1 {
            let l1 = k1 - j1;
            grow_leaves_layer(w, Pos::new(k, k1, l), 1 - l1, leaves);
        }
        j -= 2 + rand.next_int_bounded(4);
    }
    for i2 in 0..i {
        let bp = position.up_n(i2);
        if trees::grows_at(w, bp) {
            w.set(bp, wood, 2);
            if i2 > 0 {
                place_vine(w, rand, bp.west(), Facing::East);
                place_vine(w, rand, bp.north(), Facing::South);
            }
        }
        if i2 < i - 1 {
            let bp1 = bp.east();
            if trees::grows_at(w, bp1) {
                w.set(bp1, wood, 2);
                if i2 > 0 {
                    place_vine(w, rand, bp1.east(), Facing::West);
                    place_vine(w, rand, bp1.north(), Facing::South);
                }
            }
            let bp2 = bp.south().east();
            if trees::grows_at(w, bp2) {
                w.set(bp2, wood, 2);
                if i2 > 0 {
                    place_vine(w, rand, bp2.east(), Facing::West);
                    place_vine(w, rand, bp2.south(), Facing::North);
                }
            }
            let bp3 = bp.south();
            if trees::grows_at(w, bp3) {
                w.set(bp3, wood, 2);
                if i2 > 0 {
                    place_vine(w, rand, bp3.west(), Facing::East);
                    place_vine(w, rand, bp3.south(), Facing::North);
                }
            }
        }
    }
    true
}

/// `WorldGenMegaJungle.placeVine`
fn place_vine(w: &mut PopWorld, rand: &mut JavaRandom, p: Pos, side: Facing) {
    if rand.next_int_bounded(3) > 0 && w.is_air(p) {
        w.set(p, blocks::state(id::VINE, super::behavior::vine_bit(side)), 2);
    }
}

/// `WorldGenMegaPineTree.generate` (baseHeight 13, extraRandomHeight 15)
pub fn mega_pine(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos, use_base_height: bool) -> bool {
    let i = huge_height(rand, 13, 15);
    if !ensure_growable(w, position, i) {
        return false;
    }
    mega_pine_crown(w, position.x, position.z, position.y + i, 0, rand, use_base_height);
    for j in 0..i {
        if trees::air_or_leaves(w, position.up_n(j)) {
            w.set(position.up_n(j), SPRUCE_LOG, 2);
        }
        if j < i - 1 {
            for (dx, dz) in [(1, 0), (1, 1), (0, 1)] {
                let p = position.add(dx, j, dz);
                if trees::air_or_leaves(w, p) {
                    w.set(p, SPRUCE_LOG, 2);
                }
            }
        }
    }
    true
}

/// `WorldGenMegaPineTree.createCrown`
fn mega_pine_crown(w: &mut PopWorld, x: i32, z: i32, y: i32, p5: i32, rand: &mut JavaRandom, use_base_height: bool) {
    let i = rand.next_int_bounded(5) + if use_base_height { 13 } else { 3 };
    let mut j = 0;
    for k in y - i..=y {
        let l = y - k;
        let i1 = p5 + math_helper::floor_f(l as f32 / i as f32 * 3.5);
        let extra = if l > 0 && i1 == j && (k & 1) == 0 { 1 } else { 0 };
        grow_leaves_layer_strict(w, Pos::new(x, k, z), i1 + extra, SPRUCE_LEAF);
        j = i1;
    }
}

/// `WorldGenMegaPineTree.generateSaplings`
pub fn mega_pine_saplings(w: &mut PopWorld, rand: &mut JavaRandom, pos: Pos) {
    podzol_circle(w, pos.west().north());
    podzol_circle(w, pos.offset_n(Facing::East, 2).north());
    podzol_circle(w, pos.west().offset_n(Facing::South, 2));
    podzol_circle(w, pos.offset_n(Facing::East, 2).offset_n(Facing::South, 2));
    for _ in 0..5 {
        let j = rand.next_int_bounded(64);
        let k = j % 8;
        let l = j / 8;
        if k == 0 || k == 7 || l == 0 || l == 7 {
            podzol_circle(w, pos.add(-3 + k, 0, -3 + l));
        }
    }
}

/// `WorldGenMegaPineTree.placePodzolCircle`
fn podzol_circle(w: &mut PopWorld, center: Pos) {
    for i in -2i32..=2 {
        for j in -2i32..=2 {
            if i.abs() != 2 || j.abs() != 2 {
                podzol_at(w, center.add(i, 0, j));
            }
        }
    }
}

/// `WorldGenMegaPineTree.placePodzolAt`
fn podzol_at(w: &mut PopWorld, pos: Pos) {
    let mut i = 2;
    while i >= -3 {
        let bp = pos.up_n(i);
        let s = w.get(bp);
        let block = blocks::block_of(s);
        if block == id::GRASS || block == id::DIRT {
            w.set(bp, blocks::PODZOL, 2);
            break;
        }
        if state::mat(s) != material::AIR && i < 0 {
            break;
        }
        i -= 1;
    }
}
