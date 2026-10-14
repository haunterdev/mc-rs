//! The larger population features: huge mushrooms, springs, lakes, dungeons, ice spikes, desert
//! wells and fossils.

use super::behavior;
use super::props::material;
use super::state::{self, Facing, Pos};
use super::world::PopWorld;
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;
use crate::math_helper;

/// `BlockHugeMushroom.EnumType` metadata.
mod huge {
    pub const ALL_INSIDE: u16 = 0;
    pub const NORTH_WEST: u16 = 1;
    pub const NORTH_EAST: u16 = 3;
    pub const CENTER: u16 = 5;
    pub const SOUTH_WEST: u16 = 7;
    pub const SOUTH_EAST: u16 = 9;
    pub const STEM: u16 = 10;
}

/// `WorldGenBigMushroom.generate`. `kind` is the constructor's block, None for the no-argument
/// constructor the decorator and `BiomeForest.addMushrooms` use. Both construct with notify false,
/// so `setBlockAndNotifyAdequately` is flags 2.
pub fn big_mushroom(w: &mut PopWorld, rand: &mut JavaRandom, kind: Option<u16>, position: Pos) -> bool {
    let block = match kind {
        Some(b) => b,
        None => {
            if rand.next_boolean() {
                id::BROWN_MUSHROOM_BLOCK
            } else {
                id::RED_MUSHROOM_BLOCK
            }
        }
    };
    let mut i = rand.next_int_bounded(3) + 4;
    if rand.next_int_bounded(12) == 0 {
        i *= 2;
    }
    let mut flag = true;
    if !(position.y >= 1 && position.y + i + 1 < 256) {
        return false;
    }
    for j in position.y..=position.y + 1 + i {
        let mut k = 3;
        if j <= position.y + 3 {
            k = 0;
        }
        let mut l = position.x - k;
        while l <= position.x + k && flag {
            let mut i1 = position.z - k;
            while i1 <= position.z + k && flag {
                if (0..256).contains(&j) {
                    let m = state::mat(w.get(Pos::new(l, j, i1)));
                    if m != material::AIR && m != material::LEAVES {
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
    if !flag {
        return false;
    }
    let block1 = blocks::block_of(w.get(position.down()));
    if block1 != id::DIRT && block1 != id::GRASS && block1 != id::MYCELIUM {
        return false;
    }
    let mut k2 = position.y + i;
    if block == id::RED_MUSHROOM_BLOCK {
        k2 = position.y + i - 3;
    }
    for l2 in k2..=position.y + i {
        let mut j3 = 1;
        if l2 < position.y + i {
            j3 += 1;
        }
        if block == id::BROWN_MUSHROOM_BLOCK {
            j3 = 3;
        }
        let k3 = position.x - j3;
        let l3 = position.x + j3;
        let j1 = position.z - j3;
        let k1 = position.z + j3;
        for l1 in k3..=l3 {
            for i2 in j1..=k1 {
                let mut j2 = 5;
                if l1 == k3 {
                    j2 -= 1;
                } else if l1 == l3 {
                    j2 += 1;
                }
                if i2 == j1 {
                    j2 -= 3;
                } else if i2 == k1 {
                    j2 += 3;
                }
                // byMetadata(j2): j2 is always 1..=9 here, the metadata of the same type.
                let mut t = j2 as u16;
                if block == id::BROWN_MUSHROOM_BLOCK || l2 < position.y + i {
                    if (l1 == k3 || l1 == l3) && (i2 == j1 || i2 == k1) {
                        continue;
                    }
                    if l1 == position.x - (j3 - 1) && i2 == j1 {
                        t = huge::NORTH_WEST;
                    }
                    if l1 == k3 && i2 == position.z - (j3 - 1) {
                        t = huge::NORTH_WEST;
                    }
                    if l1 == position.x + (j3 - 1) && i2 == j1 {
                        t = huge::NORTH_EAST;
                    }
                    if l1 == l3 && i2 == position.z - (j3 - 1) {
                        t = huge::NORTH_EAST;
                    }
                    if l1 == position.x - (j3 - 1) && i2 == k1 {
                        t = huge::SOUTH_WEST;
                    }
                    if l1 == k3 && i2 == position.z + (j3 - 1) {
                        t = huge::SOUTH_WEST;
                    }
                    if l1 == position.x + (j3 - 1) && i2 == k1 {
                        t = huge::SOUTH_EAST;
                    }
                    if l1 == l3 && i2 == position.z + (j3 - 1) {
                        t = huge::SOUTH_EAST;
                    }
                }
                if t == huge::CENTER && l2 < position.y + i {
                    t = huge::ALL_INSIDE;
                }
                if position.y >= position.y + i - 1 || t != huge::ALL_INSIDE {
                    let bp = Pos::new(l1, l2, i2);
                    if !state::is_full_block(w.get(bp)) {
                        w.set(bp, blocks::state(block, t), 2);
                    }
                }
            }
        }
    }
    for i3 in 0..i {
        let s = w.get(position.up_n(i3));
        if !state::is_full_block(s) {
            w.set(position.up_n(i3), blocks::state(block, huge::STEM), 2);
        }
    }
    true
}

/// `WorldGenLiquids.generate`: `block` is FLOWING_WATER or FLOWING_LAVA.
pub fn liquids(w: &mut PopWorld, rand: &mut JavaRandom, block: u16, position: Pos) -> bool {
    if blocks::block_of(w.get(position.up())) != id::STONE {
        return false;
    }
    if blocks::block_of(w.get(position.down())) != id::STONE {
        return false;
    }
    let s = w.get(position);
    if state::mat(s) != material::AIR && blocks::block_of(s) != id::STONE {
        return false;
    }
    let mut i = 0;
    for q in [position.west(), position.east(), position.north(), position.south()] {
        if blocks::block_of(w.get(q)) == id::STONE {
            i += 1;
        }
    }
    let mut j = 0;
    for q in [position.west(), position.east(), position.north(), position.south()] {
        if w.is_air(q) {
            j += 1;
        }
    }
    if i == 3 && j == 1 {
        let s1 = blocks::state(block, 0);
        w.set(position, s1, 2);
        w.immediate_block_tick(position, s1, rand);
    }
    true
}

/// `WorldGenLakes.generate`: `block` is WATER or LAVA (the still blocks `populate` passes).
pub fn lakes(w: &mut PopWorld, rand: &mut JavaRandom, block: u16, position: Pos) -> bool {
    let mut position = position.add(-8, 0, -8);
    while position.y > 5 && w.is_air(position) {
        position = position.down();
    }
    if position.y <= 4 {
        return false;
    }
    position = position.down_n(4);
    let mut a = [false; 2048];
    let i = rand.next_int_bounded(4) + 4;
    for _ in 0..i {
        let d0 = rand.next_double() * 6.0 + 3.0;
        let d1 = rand.next_double() * 4.0 + 2.0;
        let d2 = rand.next_double() * 6.0 + 3.0;
        let d3 = rand.next_double() * (16.0 - d0 - 2.0) + 1.0 + d0 / 2.0;
        let d4 = rand.next_double() * (8.0 - d1 - 4.0) + 2.0 + d1 / 2.0;
        let d5 = rand.next_double() * (16.0 - d2 - 2.0) + 1.0 + d2 / 2.0;
        for l in 1..15 {
            for i1 in 1..15 {
                for j1 in 1..7 {
                    let d6 = (l as f64 - d3) / (d0 / 2.0);
                    let d7 = (j1 as f64 - d4) / (d1 / 2.0);
                    let d8 = (i1 as f64 - d5) / (d2 / 2.0);
                    let d9 = d6 * d6 + d7 * d7 + d8 * d8;
                    if d9 < 1.0 {
                        a[((l * 16 + i1) * 8 + j1) as usize] = true;
                    }
                }
            }
        }
    }
    let at = |x: i32, z: i32, y: i32| a[((x * 16 + z) * 8 + y) as usize];
    let border = |x: i32, z: i32, y: i32| {
        !at(x, z, y)
            && (x < 15 && at(x + 1, z, y)
                || x > 0 && at(x - 1, z, y)
                || z < 15 && at(x, z + 1, y)
                || z > 0 && at(x, z - 1, y)
                || y < 7 && at(x, z, y + 1)
                || y > 0 && at(x, z, y - 1))
    };
    for k1 in 0..16 {
        for l2 in 0..16 {
            for k in 0..8 {
                if border(k1, l2, k) {
                    let s = w.get(position.add(k1, k, l2));
                    if k >= 4 && state::mat_liquid(s) {
                        return false;
                    }
                    if k < 4 && !state::mat_solid(s) && blocks::block_of(w.get(position.add(k1, k, l2))) != block {
                        return false;
                    }
                }
            }
        }
    }
    for l1 in 0..16 {
        for i3 in 0..16 {
            for i4 in 0..8 {
                if at(l1, i3, i4) {
                    let s = if i4 >= 4 { blocks::AIR } else { blocks::state(block, 0) };
                    w.set(position.add(l1, i4, i3), s, 2);
                }
            }
        }
    }
    for i2 in 0..16 {
        for j3 in 0..16 {
            for j4 in 4..8 {
                if at(i2, j3, j4) {
                    let bp = position.add(i2, j4 - 1, j3);
                    if blocks::block_of(w.get(bp)) == id::DIRT && w.get_sky_light(position.add(i2, j4, j3)) > 0 {
                        let b = w.biome(bp);
                        if blocks::block_of(b.top_block) == id::MYCELIUM {
                            w.set(bp, blocks::MYCELIUM, 2);
                        } else {
                            w.set(bp, blocks::GRASS, 2);
                        }
                    }
                }
            }
        }
    }
    let m = state::mat(blocks::state(block, 0));
    if m == material::LAVA {
        for j2 in 0..16 {
            for k3 in 0..16 {
                for k4 in 0..8 {
                    if border(j2, k3, k4)
                        && (k4 < 4 || rand.next_int_bounded(2) != 0)
                        && state::mat_solid(w.get(position.add(j2, k4, k3)))
                    {
                        w.set(position.add(j2, k4, k3), blocks::STONE, 2);
                    }
                }
            }
        }
    }
    if m == material::WATER {
        for k2 in 0..16 {
            for l3 in 0..16 {
                if w.can_block_freeze(position.add(k2, 4, l3), false) {
                    w.set(position.add(k2, 4, l3), blocks::ICE, 2);
                }
            }
        }
    }
    true
}

/// `WorldGenDungeons.generate`
pub fn dungeons(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    let j = rand.next_int_bounded(2) + 2;
    let k = -j - 1;
    let l = j + 1;
    let k1 = rand.next_int_bounded(2) + 2;
    let l1 = -k1 - 1;
    let i2 = k1 + 1;
    let mut j2 = 0;
    for k2 in k..=l {
        for l2 in -1..=4 {
            for i3 in l1..=i2 {
                let bp = position.add(k2, l2, i3);
                let flag = state::mat_solid(w.get(bp));
                if l2 == -1 && !flag {
                    return false;
                }
                if l2 == 4 && !flag {
                    return false;
                }
                if (k2 == k || k2 == l || i3 == l1 || i3 == i2) && l2 == 0 && w.is_air(bp) && w.is_air(bp.up()) {
                    j2 += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&j2) {
        return false;
    }
    for k3 in k..=l {
        for i4 in (-1..=3).rev() {
            for k4 in l1..=i2 {
                let bp1 = position.add(k3, i4, k4);
                if k3 != k && i4 != -1 && k4 != l1 && k3 != l && i4 != 4 && k4 != i2 {
                    if blocks::block_of(w.get(bp1)) != id::CHEST {
                        w.set_air(bp1);
                    }
                } else if bp1.y >= 0 && !state::mat_solid(w.get(bp1.down())) {
                    w.set_air(bp1);
                } else if state::mat_solid(w.get(bp1)) && blocks::block_of(w.get(bp1)) != id::CHEST {
                    if i4 == -1 && rand.next_int_bounded(4) != 0 {
                        w.set(bp1, blocks::state(id::MOSSY_COBBLESTONE, 0), 2);
                    } else {
                        w.set(bp1, blocks::state(id::COBBLESTONE, 0), 2);
                    }
                }
            }
        }
    }
    for _l3 in 0..2 {
        for _j4 in 0..3 {
            let l4 = position.x + rand.next_int_bounded(j * 2 + 1) - j;
            let i5 = position.y;
            let j5 = position.z + rand.next_int_bounded(k1 * 2 + 1) - k1;
            let bp2 = Pos::new(l4, i5, j5);
            if w.is_air(bp2) {
                let mut j3 = 0;
                for f in Facing::HORIZONTAL {
                    if state::mat_solid(w.get(bp2.offset(f))) {
                        j3 += 1;
                    }
                }
                if j3 == 1 {
                    // CHEST default state: FACING NORTH, meta 2.
                    let s = behavior::chest_correct_facing(w, bp2, blocks::state(id::CHEST, 2));
                    w.set(bp2, s, 2);
                    // TileEntityChest.setLootTable(CHESTS_SIMPLE_DUNGEON, rand.nextLong())
                    let _ = rand.next_long();
                    // DROPPED: the loot table itself lives on the tile entity.
                    break;
                }
            }
        }
    }
    w.set(position, blocks::state(id::MOB_SPAWNER, 0), 2);
    // pickMobSpawner: SPAWNERTYPES[rand.nextInt(4)]
    let _ = rand.next_int_bounded(4);
    // DROPPED: setEntityId on the spawner tile entity.
    true
}

/// `WorldGenIceSpike.generate` (notify false, so flags 2).
pub fn ice_spike(w: &mut PopWorld, rand: &mut JavaRandom, mut position: Pos) -> bool {
    while w.is_air(position) && position.y > 2 {
        position = position.down();
    }
    if blocks::block_of(w.get(position)) != id::SNOW {
        return false;
    }
    position = position.up_n(rand.next_int_bounded(4));
    let i = rand.next_int_bounded(4) + 7;
    let j = i / 4 + rand.next_int_bounded(2);
    if j > 1 && rand.next_int_bounded(60) == 0 {
        position = position.up_n(10 + rand.next_int_bounded(30));
    }
    let packed = blocks::state(id::PACKED_ICE, 0);
    let spike_ok = |s: u16| {
        let b = blocks::block_of(s);
        state::mat(s) == material::AIR || b == id::DIRT || b == id::SNOW || b == id::ICE
    };
    for k in 0..i {
        let f = (1.0 - k as f32 / i as f32) * j as f32;
        let l = math_helper::ceil_f(f);
        for i1 in -l..=l {
            let f1 = i1.abs() as f32 - 0.25;
            for j1 in -l..=l {
                let f2 = j1.abs() as f32 - 0.25;
                if (i1 == 0 && j1 == 0 || f1 * f1 + f2 * f2 <= f * f)
                    && (i1 != -l && i1 != l && j1 != -l && j1 != l || rand.next_float() <= 0.75)
                {
                    let p = position.add(i1, k, j1);
                    if spike_ok(w.get(p)) {
                        w.set(p, packed, 2);
                    }
                    if k != 0 && l > 1 {
                        let p = position.add(i1, -k, j1);
                        if spike_ok(w.get(p)) {
                            w.set(p, packed, 2);
                        }
                    }
                }
            }
        }
    }
    let mut k1 = j - 1;
    if k1 < 0 {
        k1 = 0;
    } else if k1 > 1 {
        k1 = 1;
    }
    for l1 in -k1..=k1 {
        for i2 in -k1..=k1 {
            let mut bp = position.add(l1, -1, i2);
            let mut j2 = 50;
            if l1.abs() == 1 && i2.abs() == 1 {
                j2 = rand.next_int_bounded(5);
            }
            while bp.y > 50 {
                let s = w.get(bp);
                let b = blocks::block_of(s);
                if state::mat(s) != material::AIR && b != id::DIRT && b != id::SNOW && b != id::ICE && b != id::PACKED_ICE {
                    break;
                }
                w.set(bp, packed, 2);
                bp = bp.down();
                j2 -= 1;
                if j2 <= 0 {
                    bp = bp.down_n(rand.next_int_bounded(5) + 1);
                    j2 = rand.next_int_bounded(5);
                }
            }
        }
    }
    true
}

/// `WorldGenDesertWells.generate`
pub fn desert_wells(w: &mut PopWorld, _rand: &mut JavaRandom, mut position: Pos) -> bool {
    while w.is_air(position) && position.y > 2 {
        position = position.down();
    }
    // IS_SAND: SAND with variant SAND.
    if w.get(position) != blocks::SAND {
        return false;
    }
    for i in -2..=2 {
        for j in -2..=2 {
            if w.is_air(position.add(i, -1, j)) && w.is_air(position.add(i, -2, j)) {
                return false;
            }
        }
    }
    let sandstone = blocks::SANDSTONE;
    // STONE_SLAB, variant SAND (1), half BOTTOM.
    let slab = blocks::state(id::STONE_SLAB, 1);
    let water = blocks::FLOWING_WATER;
    for l in -1..=0 {
        for l1 in -2..=2 {
            for k in -2..=2 {
                w.set(position.add(l1, l, k), sandstone, 2);
            }
        }
    }
    w.set(position, water, 2);
    for f in Facing::HORIZONTAL {
        w.set(position.offset(f), water, 2);
    }
    for i1 in -2..=2 {
        for i2 in -2..=2 {
            if i1 == -2 || i1 == 2 || i2 == -2 || i2 == 2 {
                w.set(position.add(i1, 1, i2), sandstone, 2);
            }
        }
    }
    w.set(position.add(2, 1, 0), slab, 2);
    w.set(position.add(-2, 1, 0), slab, 2);
    w.set(position.add(0, 1, 2), slab, 2);
    w.set(position.add(0, 1, -2), slab, 2);
    for j1 in -1..=1 {
        for j2 in -1..=1 {
            if j1 == 0 && j2 == 0 {
                w.set(position.add(j1, 4, j2), sandstone, 2);
            } else {
                w.set(position.add(j1, 4, j2), slab, 2);
            }
        }
    }
    for k1 in 1..=3 {
        w.set(position.add(-1, k1, -1), sandstone, 2);
        w.set(position.add(-1, k1, 1), sandstone, 2);
        w.set(position.add(1, k1, -1), sandstone, 2);
        w.set(position.add(1, k1, 1), sandstone, 2);
    }
    true
}

/// `WorldGenFossils.generate`. The fossil draws from its own `Chunk.getRandomWithSeed(987234911)`,
/// so the generator's random is untouched.
pub fn fossils(_w: &mut PopWorld, _rand: &mut JavaRandom, _position: Pos) -> bool {
    // DROPPED: the whole body. It places two structure templates (fossils/fossil_spine_01 and the
    // rest) that are Mojang assets; the fixture harness has no templates either, so nothing lands.
    true
}
