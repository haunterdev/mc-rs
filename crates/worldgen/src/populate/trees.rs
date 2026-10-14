//! `WorldGenAbstractTree` and the small trees: `WorldGenTrees` (oak and jungle, with vines and
//! cocoa), `WorldGenBirchTree`, `WorldGenTaiga1`, `WorldGenTaiga2`, `WorldGenSwamp` and
//! `WorldGenShrub`. Every biome tree feature is constructed with notify false, so
//! `setBlockAndNotifyAdequately` is `setBlockState(pos, state, 2)` throughout.

use super::behavior;
use super::props::material;
use super::state::{self, Facing, Pos};
use super::world::PopWorld;
use super::{trees2, trees3};
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;

pub const OAK_LOG: u16 = blocks::state(id::LOG, 0);
pub const SPRUCE_LOG: u16 = blocks::state(id::LOG, 1);
pub const BIRCH_LOG: u16 = blocks::state(id::LOG, 2);
pub const JUNGLE_LOG: u16 = blocks::state(id::LOG, 3);
pub const ACACIA_LOG: u16 = blocks::state(id::LOG2, 0);
pub const DARK_OAK_LOG: u16 = blocks::state(id::LOG2, 1);
/// Leaves with CHECK_DECAY false and DECAYABLE true: the metadata is the variant.
pub const OAK_LEAF: u16 = blocks::state(id::LEAVES, 0);
pub const SPRUCE_LEAF: u16 = blocks::state(id::LEAVES, 1);
pub const BIRCH_LEAF: u16 = blocks::state(id::LEAVES, 2);
pub const JUNGLE_LEAF: u16 = blocks::state(id::LEAVES, 3);
pub const ACACIA_LEAF: u16 = blocks::state(id::LEAVES2, 0);
pub const DARK_OAK_LEAF: u16 = blocks::state(id::LEAVES2, 1);

/// The tree features biomes hand the decorator (`Biome.getRandomTreeFeature`).
#[derive(Clone, Copy, Debug)]
pub enum Tree {
    /// `WorldGenTrees(false, minTreeHeight, wood, leaves, vinesGrow)`
    Trees { min_height: i32, wood: u16, leaves: u16, vines: bool },
    /// `Biome.BIG_TREE_FEATURE`, the shared `WorldGenBigTree(false)`.
    Big,
    /// `WorldGenBirchTree(false, useExtraRandomHeight)`
    Birch { extra: bool },
    Taiga1,
    Taiga2,
    Swamp,
    /// `WorldGenCanopyTree(false)`
    Canopy,
    /// `WorldGenSavannaTree(false)`
    Savanna,
    /// `WorldGenMegaJungle(false, base, extra, wood, leaves)`
    MegaJungle { base: i32, extra: i32, wood: u16, leaves: u16 },
    /// `WorldGenMegaPineTree(false, useBaseHeight)`
    MegaPine { use_base_height: bool },
    /// `WorldGenShrub(wood, leaves)`
    Shrub { wood: u16, leaves: u16 },
}

/// `Biome.TREE_FEATURE`: `WorldGenTrees(false)`.
pub const TREE_FEATURE: Tree = Tree::Trees { min_height: 4, wood: OAK_LOG, leaves: OAK_LEAF, vines: false };

impl Tree {
    /// `WorldGenerator.setDecorationDefaults`; only `WorldGenBigTree` overrides it.
    pub fn set_decoration_defaults(&self, w: &mut PopWorld) {
        if let Tree::Big = self {
            w.statics.big_tree_leaf_distance_limit = 5;
        }
    }

    pub fn generate(&self, w: &mut PopWorld, rand: &mut JavaRandom, p: Pos) -> bool {
        match *self {
            Tree::Trees { min_height, wood, leaves, vines } => trees(w, rand, p, min_height, wood, leaves, vines),
            Tree::Big => trees2::big_tree(w, rand, p),
            Tree::Birch { extra } => birch(w, rand, p, extra),
            Tree::Taiga1 => taiga1(w, rand, p),
            Tree::Taiga2 => taiga2(w, rand, p),
            Tree::Swamp => swamp(w, rand, p),
            Tree::Canopy => trees2::canopy(w, rand, p),
            Tree::Savanna => trees2::savanna(w, rand, p),
            Tree::MegaJungle { base, extra, wood, leaves } => trees3::mega_jungle(w, rand, p, base, extra, wood, leaves),
            Tree::MegaPine { use_base_height } => trees3::mega_pine(w, rand, p, use_base_height),
            Tree::Shrub { wood, leaves } => shrub(w, rand, p, wood, leaves),
        }
    }

    /// `WorldGenAbstractTree.generateSaplings`; only `WorldGenMegaPineTree` overrides it.
    pub fn generate_saplings(&self, w: &mut PopWorld, rand: &mut JavaRandom, p: Pos) {
        if let Tree::MegaPine { .. } = self {
            trees3::mega_pine_saplings(w, rand, p);
        }
    }
}

/// `WorldGenAbstractTree.canGrowInto(Block)`
pub fn can_grow_into(block: u16) -> bool {
    let m = state::mat(block << 4);
    m == material::AIR
        || m == material::LEAVES
        || matches!(block, id::GRASS | id::DIRT | id::LOG | id::LOG2 | id::SAPLING | id::VINE)
}

/// `can_grow_into` for the block at `p`.
pub fn grows_at(w: &mut PopWorld, p: Pos) -> bool {
    can_grow_into(blocks::block_of(w.get(p)))
}

/// `WorldGenAbstractTree.setDirtAt`
pub fn set_dirt_at(w: &mut PopWorld, p: Pos) {
    if blocks::block_of(w.get(p)) != id::DIRT {
        w.set(p, blocks::DIRT, 2);
    }
}

/// The material is AIR or LEAVES.
pub fn air_or_leaves(w: &mut PopWorld, p: Pos) -> bool {
    let m = state::mat(w.get(p));
    m == material::AIR || m == material::LEAVES
}

/// The space check `WorldGenTrees`, `WorldGenBirchTree` and `WorldGenSavannaTree` share: radius 0
/// at the base, 2 for the top two layers, 1 between.
pub fn standard_space(w: &mut PopWorld, p: Pos, i: i32) -> bool {
    let mut flag = true;
    for j in p.y..=p.y + 1 + i {
        let mut k = 1;
        if j == p.y {
            k = 0;
        }
        if j >= p.y + 1 + i - 2 {
            k = 2;
        }
        let mut l = p.x - k;
        while l <= p.x + k && flag {
            let mut i1 = p.z - k;
            while i1 <= p.z + k && flag {
                if (0..256).contains(&j) {
                    if !grows_at(w, Pos::new(l, j, i1)) {
                        flag = false;
                    }
                } else {
                    flag = false;
                }
                i1 += 1;
            }
            l += 1;
        }
    }
    flag
}

fn vine(f: Facing) -> u16 {
    blocks::state(id::VINE, behavior::vine_bit(f))
}

/// `WorldGenTrees.generate`
fn trees(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos, min_height: i32, wood: u16, leaves: u16, vines: bool) -> bool {
    let i = rand.next_int_bounded(3) + min_height;
    if !(position.y >= 1 && position.y + i + 1 <= 256) {
        return false;
    }
    if !standard_space(w, position, i) {
        return false;
    }
    let block = blocks::block_of(w.get(position.down()));
    if !((block == id::GRASS || block == id::DIRT || block == id::FARMLAND) && position.y < 256 - i - 1) {
        return false;
    }
    set_dirt_at(w, position.down());
    for i3 in position.y - 3 + i..=position.y + i {
        let i4 = i3 - (position.y + i);
        let j1 = 1 - i4 / 2;
        for k1 in position.x - j1..=position.x + j1 {
            let l1 = k1 - position.x;
            for i2 in position.z - j1..=position.z + j1 {
                let j2 = i2 - position.z;
                if l1.abs() != j1 || j2.abs() != j1 || rand.next_int_bounded(2) != 0 && i4 != 0 {
                    let bp = Pos::new(k1, i3, i2);
                    let m = state::mat(w.get(bp));
                    if m == material::AIR || m == material::LEAVES || m == material::VINE {
                        w.set(bp, leaves, 2);
                    }
                }
            }
        }
    }
    for j3 in 0..i {
        let m = state::mat(w.get(position.up_n(j3)));
        if m == material::AIR || m == material::LEAVES || m == material::VINE {
            w.set(position.up_n(j3), wood, 2);
            if vines && j3 > 0 {
                if rand.next_int_bounded(3) > 0 && w.is_air(position.add(-1, j3, 0)) {
                    w.set(position.add(-1, j3, 0), vine(Facing::East), 2);
                }
                if rand.next_int_bounded(3) > 0 && w.is_air(position.add(1, j3, 0)) {
                    w.set(position.add(1, j3, 0), vine(Facing::West), 2);
                }
                if rand.next_int_bounded(3) > 0 && w.is_air(position.add(0, j3, -1)) {
                    w.set(position.add(0, j3, -1), vine(Facing::South), 2);
                }
                if rand.next_int_bounded(3) > 0 && w.is_air(position.add(0, j3, 1)) {
                    w.set(position.add(0, j3, 1), vine(Facing::North), 2);
                }
            }
        }
    }
    if vines {
        for k3 in position.y - 3 + i..=position.y + i {
            let j4 = k3 - (position.y + i);
            let k4 = 2 - j4 / 2;
            for l4 in position.x - k4..=position.x + k4 {
                for i5 in position.z - k4..=position.z + k4 {
                    let mp = Pos::new(l4, k3, i5);
                    if state::mat(w.get(mp)) == material::LEAVES {
                        let bp2 = mp.west();
                        let bp3 = mp.east();
                        let bp4 = mp.north();
                        let bp1 = mp.south();
                        if rand.next_int_bounded(4) == 0 && w.is_air(bp2) {
                            add_hanging_vine(w, bp2, Facing::East);
                        }
                        if rand.next_int_bounded(4) == 0 && w.is_air(bp3) {
                            add_hanging_vine(w, bp3, Facing::West);
                        }
                        if rand.next_int_bounded(4) == 0 && w.is_air(bp4) {
                            add_hanging_vine(w, bp4, Facing::South);
                        }
                        if rand.next_int_bounded(4) == 0 && w.is_air(bp1) {
                            add_hanging_vine(w, bp1, Facing::North);
                        }
                    }
                }
            }
        }
        if rand.next_int_bounded(5) == 0 && i > 5 {
            for l3 in 0..2 {
                for f in Facing::HORIZONTAL {
                    if rand.next_int_bounded(4 - l3) == 0 {
                        let f1 = f.opposite();
                        let age = rand.next_int_bounded(3);
                        let (dx, _, dz) = f1.offset();
                        // placeCocoa: COCOA with AGE and FACING, meta horizontalIndex | age << 2.
                        let s = blocks::state(id::COCOA, (f.horizontal_index() | age << 2) as u16);
                        w.set(position.add(dx, i - 5 + l3, dz), s, 2);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenTrees.addHangingVine`
fn add_hanging_vine(w: &mut PopWorld, p: Pos, f: Facing) {
    w.set(p, vine(f), 2);
    let mut i = 4;
    let mut bp = p.down();
    while w.is_air(bp) && i > 0 {
        w.set(bp, vine(f), 2);
        bp = bp.down();
        i -= 1;
    }
}

/// `WorldGenBirchTree.generate`
fn birch(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos, extra: bool) -> bool {
    let mut i = rand.next_int_bounded(3) + 5;
    if extra {
        i += rand.next_int_bounded(7);
    }
    if !(position.y >= 1 && position.y + i + 1 <= 256) {
        return false;
    }
    if !standard_space(w, position, i) {
        return false;
    }
    let block = blocks::block_of(w.get(position.down()));
    if !((block == id::GRASS || block == id::DIRT || block == id::FARMLAND) && position.y < 256 - i - 1) {
        return false;
    }
    set_dirt_at(w, position.down());
    for i2 in position.y - 3 + i..=position.y + i {
        let k2 = i2 - (position.y + i);
        let l2 = 1 - k2 / 2;
        for i3 in position.x - l2..=position.x + l2 {
            let j1 = i3 - position.x;
            for k1 in position.z - l2..=position.z + l2 {
                let l1 = k1 - position.z;
                if j1.abs() != l2 || l1.abs() != l2 || rand.next_int_bounded(2) != 0 && k2 != 0 {
                    let bp = Pos::new(i3, i2, k1);
                    if air_or_leaves(w, bp) {
                        w.set(bp, BIRCH_LEAF, 2);
                    }
                }
            }
        }
    }
    for j2 in 0..i {
        if air_or_leaves(w, position.up_n(j2)) {
            w.set(position.up_n(j2), BIRCH_LOG, 2);
        }
    }
    true
}

/// `WorldGenTaiga1.generate`
fn taiga1(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    let i = rand.next_int_bounded(5) + 7;
    let j = i - rand.next_int_bounded(2) - 3;
    let k = i - j;
    let l = 1 + rand.next_int_bounded(k + 1);
    if !(position.y >= 1 && position.y + i + 1 <= 256) {
        return false;
    }
    let mut flag = true;
    let mut i1 = position.y;
    while i1 <= position.y + 1 + i && flag {
        let j1 = if i1 - position.y < j { 0 } else { l };
        let mut k1 = position.x - j1;
        while k1 <= position.x + j1 && flag {
            let mut l1 = position.z - j1;
            while l1 <= position.z + j1 && flag {
                if (0..256).contains(&i1) {
                    if !grows_at(w, Pos::new(k1, i1, l1)) {
                        flag = false;
                    }
                } else {
                    flag = false;
                }
                l1 += 1;
            }
            k1 += 1;
        }
        i1 += 1;
    }
    if !flag {
        return false;
    }
    let block = blocks::block_of(w.get(position.down()));
    if !((block == id::GRASS || block == id::DIRT) && position.y < 256 - i - 1) {
        return false;
    }
    set_dirt_at(w, position.down());
    let mut k2 = 0;
    let mut l2 = position.y + i;
    while l2 >= position.y + j {
        for j3 in position.x - k2..=position.x + k2 {
            let k3 = j3 - position.x;
            for i2 in position.z - k2..=position.z + k2 {
                let j2 = i2 - position.z;
                if k3.abs() != k2 || j2.abs() != k2 || k2 <= 0 {
                    let bp = Pos::new(j3, l2, i2);
                    if !state::is_full_block(w.get(bp)) {
                        w.set(bp, SPRUCE_LEAF, 2);
                    }
                }
            }
        }
        if k2 >= 1 && l2 == position.y + j + 1 {
            k2 -= 1;
        } else if k2 < l {
            k2 += 1;
        }
        l2 -= 1;
    }
    for i3 in 0..i - 1 {
        if air_or_leaves(w, position.up_n(i3)) {
            w.set(position.up_n(i3), SPRUCE_LOG, 2);
        }
    }
    true
}

/// `WorldGenTaiga2.generate`
fn taiga2(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    let i = rand.next_int_bounded(4) + 6;
    let j = 1 + rand.next_int_bounded(2);
    let k = i - j;
    let l = 2 + rand.next_int_bounded(2);
    let mut flag = true;
    if !(position.y >= 1 && position.y + i + 1 <= 256) {
        return false;
    }
    let mut i1 = position.y;
    while i1 <= position.y + 1 + i && flag {
        let j1 = if i1 - position.y < j { 0 } else { l };
        let mut k1 = position.x - j1;
        while k1 <= position.x + j1 && flag {
            let mut l1 = position.z - j1;
            while l1 <= position.z + j1 && flag {
                if (0..256).contains(&i1) {
                    if !air_or_leaves(w, Pos::new(k1, i1, l1)) {
                        flag = false;
                    }
                } else {
                    flag = false;
                }
                l1 += 1;
            }
            k1 += 1;
        }
        i1 += 1;
    }
    if !flag {
        return false;
    }
    let block = blocks::block_of(w.get(position.down()));
    if !((block == id::GRASS || block == id::DIRT || block == id::FARMLAND) && position.y < 256 - i - 1) {
        return false;
    }
    set_dirt_at(w, position.down());
    let mut i3 = rand.next_int_bounded(2);
    let mut j3 = 1;
    let mut k3 = 0;
    for l3 in 0..=k {
        let j4 = position.y + i - l3;
        for i2 in position.x - i3..=position.x + i3 {
            let j2 = i2 - position.x;
            for k2 in position.z - i3..=position.z + i3 {
                let l2 = k2 - position.z;
                if j2.abs() != i3 || l2.abs() != i3 || i3 <= 0 {
                    let bp = Pos::new(i2, j4, k2);
                    if !state::is_full_block(w.get(bp)) {
                        w.set(bp, SPRUCE_LEAF, 2);
                    }
                }
            }
        }
        if i3 >= j3 {
            i3 = k3;
            k3 = 1;
            j3 += 1;
            if j3 > l {
                j3 = l;
            }
        } else {
            i3 += 1;
        }
    }
    let i4 = rand.next_int_bounded(3);
    for k4 in 0..i - i4 {
        if air_or_leaves(w, position.up_n(k4)) {
            w.set(position.up_n(k4), SPRUCE_LOG, 2);
        }
    }
    true
}

fn is_water_block(b: u16) -> bool {
    b == id::WATER || b == id::FLOWING_WATER
}

/// `WorldGenSwamp.generate`
fn swamp(w: &mut PopWorld, rand: &mut JavaRandom, mut position: Pos) -> bool {
    let i = rand.next_int_bounded(4) + 5;
    while state::mat(w.get(position.down())) == material::WATER {
        position = position.down();
    }
    let mut flag = true;
    if !(position.y >= 1 && position.y + i + 1 <= 256) {
        return false;
    }
    for j in position.y..=position.y + 1 + i {
        let mut k = 1;
        if j == position.y {
            k = 0;
        }
        if j >= position.y + 1 + i - 2 {
            k = 3;
        }
        let mut l = position.x - k;
        while l <= position.x + k && flag {
            let mut i1 = position.z - k;
            while i1 <= position.z + k && flag {
                if (0..256).contains(&j) {
                    let s = w.get(Pos::new(l, j, i1));
                    let m = state::mat(s);
                    if m != material::AIR && m != material::LEAVES {
                        if !is_water_block(blocks::block_of(s)) {
                            flag = false;
                        } else if j > position.y {
                            flag = false;
                        }
                    }
                } else {
                    flag = false;
                }
                i1 += 1;
            }
            l += 1;
        }
    }
    if !flag {
        return false;
    }
    let block1 = blocks::block_of(w.get(position.down()));
    if !((block1 == id::GRASS || block1 == id::DIRT) && position.y < 256 - i - 1) {
        return false;
    }
    set_dirt_at(w, position.down());
    for k1 in position.y - 3 + i..=position.y + i {
        let j2 = k1 - (position.y + i);
        let l2 = 2 - j2 / 2;
        for j3 in position.x - l2..=position.x + l2 {
            let k3 = j3 - position.x;
            for i4 in position.z - l2..=position.z + l2 {
                let j1 = i4 - position.z;
                if k3.abs() != l2 || j1.abs() != l2 || rand.next_int_bounded(2) != 0 && j2 != 0 {
                    let bp = Pos::new(j3, k1, i4);
                    if !state::is_full_block(w.get(bp)) {
                        w.set(bp, OAK_LEAF, 2);
                    }
                }
            }
        }
    }
    for l1 in 0..i {
        let s = w.get(position.up_n(l1));
        let m = state::mat(s);
        if m == material::AIR || m == material::LEAVES || is_water_block(blocks::block_of(s)) {
            w.set(position.up_n(l1), OAK_LOG, 2);
        }
    }
    for i2 in position.y - 3 + i..=position.y + i {
        let k2 = i2 - (position.y + i);
        let i3 = 2 - k2 / 2;
        for l3 in position.x - i3..=position.x + i3 {
            for j4 in position.z - i3..=position.z + i3 {
                let mp = Pos::new(l3, i2, j4);
                if state::mat(w.get(mp)) == material::LEAVES {
                    let bp3 = mp.west();
                    let bp4 = mp.east();
                    let bp1 = mp.north();
                    let bp2 = mp.south();
                    if rand.next_int_bounded(4) == 0 && w.is_air(bp3) {
                        swamp_add_vine(w, bp3, Facing::East);
                    }
                    if rand.next_int_bounded(4) == 0 && w.is_air(bp4) {
                        swamp_add_vine(w, bp4, Facing::West);
                    }
                    if rand.next_int_bounded(4) == 0 && w.is_air(bp1) {
                        swamp_add_vine(w, bp1, Facing::South);
                    }
                    if rand.next_int_bounded(4) == 0 && w.is_air(bp2) {
                        swamp_add_vine(w, bp2, Facing::North);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenSwamp.addVine`
fn swamp_add_vine(w: &mut PopWorld, p: Pos, f: Facing) {
    let s = vine(f);
    w.set(p, s, 2);
    let mut i = 4;
    let mut bp = p.down();
    while w.is_air(bp) && i > 0 {
        w.set(bp, s, 2);
        bp = bp.down();
        i -= 1;
    }
}

/// `WorldGenShrub.generate`
fn shrub(w: &mut PopWorld, rand: &mut JavaRandom, mut position: Pos, wood: u16, leaves: u16) -> bool {
    while air_or_leaves(w, position) && position.y > 0 {
        position = position.down();
    }
    let block = blocks::block_of(w.get(position));
    if block == id::DIRT || block == id::GRASS {
        position = position.up();
        w.set(position, wood, 2);
        for i in position.y..=position.y + 2 {
            let j = i - position.y;
            let k = 2 - j;
            for l in position.x - k..=position.x + k {
                let i1 = l - position.x;
                for j1 in position.z - k..=position.z + k {
                    let k1 = j1 - position.z;
                    if i1.abs() != k || k1.abs() != k || rand.next_int_bounded(2) != 0 {
                        let bp = Pos::new(l, i, j1);
                        if air_or_leaves(w, bp) {
                            w.set(bp, leaves, 2);
                        }
                    }
                }
            }
        }
    }
    true
}
