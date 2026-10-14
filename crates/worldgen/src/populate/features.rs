//! The small `WorldGenerator`s the decorator and the biomes call: ore veins, disks, plants and
//! patches. Each `generate` is ported line by line from the class it names.

use super::behavior;
use super::props::material;
use super::state::{self, Facing, Pos};
use super::world::PopWorld;
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;
use crate::math_helper;

/// `position.add(rand.nextInt(h) - rand.nextInt(h), rand.nextInt(v) - rand.nextInt(v), rand.nextInt(h) - rand.nextInt(h))`,
/// drawn left to right as Java evaluates the arguments.
pub fn scatter(rand: &mut JavaRandom, p: Pos, h: i32, v: i32) -> Pos {
    let dx = rand.next_int_bounded(h) - rand.next_int_bounded(h);
    let dy = rand.next_int_bounded(v) - rand.next_int_bounded(v);
    let dz = rand.next_int_bounded(h) - rand.next_int_bounded(h);
    p.add(dx, dy, dz)
}

/// The descent `WorldGenTallGrass` and `WorldGenDeadBush` start with: down through air and leaves.
fn sink_through_air_and_leaves(w: &mut PopWorld, mut p: Pos) -> Pos {
    loop {
        let m = state::mat(w.get(p));
        if !((m == material::AIR || m == material::LEAVES) && p.y > 0) {
            return p;
        }
        p = p.down();
    }
}

/// `WorldGenMinable.StonePredicate`: stone whose variant `isNatural()` (stone, granite, diorite,
/// andesite; the polished variants are not).
fn stone_predicate(s: u16) -> bool {
    blocks::block_of(s) == id::STONE && matches!(s & 15, 0 | 1 | 3 | 5)
}

/// `WorldGenMinable.generate` with the stone predicate.
pub fn minable(w: &mut PopWorld, rand: &mut JavaRandom, ore: u16, n: i32, position: Pos) -> bool {
    let f = rand.next_float() * 3.1415927_f32;
    let nf = n as f32;
    let d0 = ((position.x + 8) as f32 + math_helper::sin(f) * nf / 8.0) as f64;
    let d1 = ((position.x + 8) as f32 - math_helper::sin(f) * nf / 8.0) as f64;
    let d2 = ((position.z + 8) as f32 + math_helper::cos(f) * nf / 8.0) as f64;
    let d3 = ((position.z + 8) as f32 - math_helper::cos(f) * nf / 8.0) as f64;
    let d4 = (position.y + rand.next_int_bounded(3) - 2) as f64;
    let d5 = (position.y + rand.next_int_bounded(3) - 2) as f64;
    for i in 0..n {
        let f1 = i as f32 / nf;
        let d6 = d0 + (d1 - d0) * f1 as f64;
        let d7 = d4 + (d5 - d4) * f1 as f64;
        let d8 = d2 + (d3 - d2) * f1 as f64;
        let d9 = rand.next_double() * n as f64 / 16.0;
        let d10 = (math_helper::sin(3.1415927_f32 * f1) + 1.0) as f64 * d9 + 1.0;
        let d11 = (math_helper::sin(3.1415927_f32 * f1) + 1.0) as f64 * d9 + 1.0;
        let j = math_helper::floor(d6 - d10 / 2.0);
        let k = math_helper::floor(d7 - d11 / 2.0);
        let l = math_helper::floor(d8 - d10 / 2.0);
        let i1 = math_helper::floor(d6 + d10 / 2.0);
        let j1 = math_helper::floor(d7 + d11 / 2.0);
        let k1 = math_helper::floor(d8 + d10 / 2.0);
        for l1 in j..=i1 {
            let d12 = (l1 as f64 + 0.5 - d6) / (d10 / 2.0);
            if d12 * d12 < 1.0 {
                for i2 in k..=j1 {
                    let d13 = (i2 as f64 + 0.5 - d7) / (d11 / 2.0);
                    if d12 * d12 + d13 * d13 < 1.0 {
                        for j2 in l..=k1 {
                            let d14 = (j2 as f64 + 0.5 - d8) / (d10 / 2.0);
                            if d12 * d12 + d13 * d13 + d14 * d14 < 1.0 {
                                let bp = Pos::new(l1, i2, j2);
                                if stone_predicate(w.get(bp)) {
                                    w.set(bp, ore, 2);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenSand.generate`: `block` is SAND or GRAVEL.
pub fn sand(w: &mut PopWorld, rand: &mut JavaRandom, block: u16, radius: i32, position: Pos) -> bool {
    if state::mat(w.get(position)) != material::WATER {
        return false;
    }
    let i = rand.next_int_bounded(radius - 2) + 2;
    for k in position.x - i..=position.x + i {
        for l in position.z - i..=position.z + i {
            let i1 = k - position.x;
            let j1 = l - position.z;
            if i1 * i1 + j1 * j1 <= i * i {
                for k1 in position.y - 2..=position.y + 2 {
                    let bp = Pos::new(k, k1, l);
                    let b = blocks::block_of(w.get(bp));
                    if b == id::DIRT || b == id::GRASS {
                        w.set(bp, blocks::state(block, 0), 2);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenClay.generate`
pub fn clay(w: &mut PopWorld, rand: &mut JavaRandom, n: i32, position: Pos) -> bool {
    if state::mat(w.get(position)) != material::WATER {
        return false;
    }
    let i = rand.next_int_bounded(n - 2) + 2;
    for k in position.x - i..=position.x + i {
        for l in position.z - i..=position.z + i {
            let i1 = k - position.x;
            let j1 = l - position.z;
            if i1 * i1 + j1 * j1 <= i * i {
                for k1 in position.y - 1..=position.y + 1 {
                    let bp = Pos::new(k, k1, l);
                    let b = blocks::block_of(w.get(bp));
                    if b == id::DIRT || b == id::CLAY {
                        w.set(bp, blocks::state(id::CLAY, 0), 2);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenFlowers.generate`: `flower` is the full state (YELLOW_FLOWER or RED_FLOWER with its type).
pub fn flowers(w: &mut PopWorld, rand: &mut JavaRandom, flower: u16, position: Pos) -> bool {
    for _ in 0..64 {
        let bp = scatter(rand, position, 8, 4);
        // `!provider.isNether() || y < 255` is true in the overworld.
        if w.is_air(bp) && behavior::can_block_stay(w, bp, flower) {
            w.set(bp, flower, 2);
        }
    }
    true
}

/// `WorldGenTallGrass.generate`: `grass` is TALLGRASS with its type (1 grass, 2 fern).
pub fn tall_grass(w: &mut PopWorld, rand: &mut JavaRandom, grass: u16, position: Pos) -> bool {
    let position = sink_through_air_and_leaves(w, position);
    for _ in 0..128 {
        let bp = scatter(rand, position, 8, 4);
        if w.is_air(bp) && behavior::can_block_stay(w, bp, grass) {
            w.set(bp, grass, 2);
        }
    }
    true
}

/// `BlockDoublePlant.placeAt(world, lowerPos, variant, flags)`: the upper half is the default state
/// with HALF UPPER, meta `8 | NORTH.getHorizontalIndex()`.
pub fn place_double_plant(w: &mut PopWorld, lower: Pos, variant: u16, flags: i32) {
    w.set(lower, blocks::state(id::DOUBLE_PLANT, variant), flags);
    w.set(lower.up(), blocks::state(id::DOUBLE_PLANT, 10), flags);
}

/// `BlockDoublePlant.EnumPlantType` metadata.
pub mod plant {
    pub const SUNFLOWER: u16 = 0;
    pub const SYRINGA: u16 = 1;
    pub const GRASS: u16 = 2;
    pub const FERN: u16 = 3;
    pub const ROSE: u16 = 4;
    pub const PAEONIA: u16 = 5;
}

/// `WorldGenDoublePlant.generate`
pub fn double_plant(w: &mut PopWorld, rand: &mut JavaRandom, variant: u16, position: Pos) -> bool {
    let mut flag = false;
    for _ in 0..64 {
        let bp = scatter(rand, position, 8, 4);
        // `!provider.isNether() || y < 254` is true in the overworld.
        if w.is_air(bp) && behavior::can_place_block_at(w, bp, id::DOUBLE_PLANT) {
            place_double_plant(w, bp, variant, 2);
            flag = true;
        }
    }
    flag
}

/// `WorldGenDeadBush.generate`
pub fn dead_bush(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    let position = sink_through_air_and_leaves(w, position);
    let s = blocks::state(id::DEADBUSH, 0);
    for _ in 0..4 {
        let bp = scatter(rand, position, 8, 4);
        if w.is_air(bp) && behavior::can_block_stay(w, bp, s) {
            w.set(bp, s, 2);
        }
    }
    true
}

/// `WorldGenBush.generate`: `block` is BROWN_MUSHROOM or RED_MUSHROOM.
pub fn bush(w: &mut PopWorld, rand: &mut JavaRandom, block: u16, position: Pos) -> bool {
    let s = blocks::state(block, 0);
    for _ in 0..64 {
        let bp = scatter(rand, position, 8, 4);
        if w.is_air(bp) && behavior::can_block_stay(w, bp, s) {
            w.set(bp, s, 2);
        }
    }
    true
}

/// `WorldGenReed.generate`
pub fn reed(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    for _ in 0..20 {
        let dx = rand.next_int_bounded(4) - rand.next_int_bounded(4);
        let dz = rand.next_int_bounded(4) - rand.next_int_bounded(4);
        let bp = position.add(dx, 0, dz);
        if w.is_air(bp) {
            let bp1 = bp.down();
            if state::mat(w.get(bp1.west())) == material::WATER
                || state::mat(w.get(bp1.east())) == material::WATER
                || state::mat(w.get(bp1.north())) == material::WATER
                || state::mat(w.get(bp1.south())) == material::WATER
            {
                let inner = rand.next_int_bounded(3) + 1;
                let j = 2 + rand.next_int_bounded(inner);
                for k in 0..j {
                    // BlockReed.canBlockStay(world, pos) is canPlaceBlockAt, asked of the base position.
                    if behavior::reed_can_place_block_at(w, bp) {
                        w.set(bp.up_n(k), blocks::state(id::REEDS, 0), 2);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenCactus.generate`
pub fn cactus(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    for _ in 0..10 {
        let bp = scatter(rand, position, 8, 4);
        if w.is_air(bp) {
            let inner = rand.next_int_bounded(3) + 1;
            let j = 1 + rand.next_int_bounded(inner);
            for k in 0..j {
                // BlockCactus.canBlockStay is asked of the base position, as in vanilla.
                if behavior::cactus_can_block_stay(w, bp) {
                    w.set(bp.up_n(k), blocks::state(id::CACTUS, 0), 2);
                }
            }
        }
    }
    true
}

/// `WorldGenPumpkin.generate`
pub fn pumpkin(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    for _ in 0..64 {
        let bp = scatter(rand, position, 8, 4);
        if w.is_air(bp)
            && blocks::block_of(w.get(bp.down())) == id::GRASS
            && behavior::can_place_block_at(w, bp, id::PUMPKIN)
        {
            let f = Facing::random_horizontal(rand);
            w.set(bp, blocks::state(id::PUMPKIN, f.horizontal_index() as u16), 2);
        }
    }
    true
}

/// `WorldGenMelon.generate`
pub fn melon(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    for _ in 0..64 {
        let bp = scatter(rand, position, 8, 4);
        if behavior::can_place_block_at(w, bp, id::MELON_BLOCK) && blocks::block_of(w.get(bp.down())) == id::GRASS {
            w.set(bp, blocks::state(id::MELON_BLOCK, 0), 2);
        }
    }
    true
}

/// `WorldGenWaterlily.generate`
pub fn waterlily(w: &mut PopWorld, rand: &mut JavaRandom, position: Pos) -> bool {
    for _ in 0..10 {
        let j = position.x + rand.next_int_bounded(8) - rand.next_int_bounded(8);
        let k = position.y + rand.next_int_bounded(4) - rand.next_int_bounded(4);
        let l = position.z + rand.next_int_bounded(8) - rand.next_int_bounded(8);
        let bp = Pos::new(j, k, l);
        if w.is_air(bp) && behavior::can_place_block_at(w, bp, id::WATERLILY) {
            w.set(bp, blocks::WATERLILY, 2);
        }
    }
    true
}

/// `WorldGenVines.generate`
pub fn vines(w: &mut PopWorld, rand: &mut JavaRandom, mut position: Pos) -> bool {
    while position.y < 128 {
        if w.is_air(position) {
            for f in Facing::HORIZONTAL {
                if behavior::vine_can_place_on_side(w, position, f) {
                    w.set(position, blocks::state(id::VINE, behavior::vine_bit(f)), 2);
                    break;
                }
            }
        } else {
            let dx = rand.next_int_bounded(4) - rand.next_int_bounded(4);
            let dz = rand.next_int_bounded(4) - rand.next_int_bounded(4);
            position = position.add(dx, 0, dz);
        }
        position = position.up();
    }
    true
}

/// `WorldGenIcePath.generate`
pub fn ice_path(w: &mut PopWorld, rand: &mut JavaRandom, base_width: i32, mut position: Pos) -> bool {
    while w.is_air(position) && position.y > 2 {
        position = position.down();
    }
    if blocks::block_of(w.get(position)) != id::SNOW {
        return false;
    }
    let i = rand.next_int_bounded(base_width - 2) + 2;
    for k in position.x - i..=position.x + i {
        for l in position.z - i..=position.z + i {
            let i1 = k - position.x;
            let j1 = l - position.z;
            if i1 * i1 + j1 * j1 <= i * i {
                for k1 in position.y - 1..=position.y + 1 {
                    let bp = Pos::new(k, k1, l);
                    let b = blocks::block_of(w.get(bp));
                    if b == id::DIRT || b == id::SNOW || b == id::ICE {
                        w.set(bp, blocks::state(id::PACKED_ICE, 0), 2);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenBlockBlob.generate`
pub fn block_blob(w: &mut PopWorld, rand: &mut JavaRandom, block: u16, start_radius: i32, mut position: Pos) -> bool {
    loop {
        // label50: step down while the block below is air or not grass, dirt or stone.
        if position.y > 3 {
            let below = position.down();
            if w.is_air(below) {
                position = below;
                continue;
            }
            let b = blocks::block_of(w.get(below));
            if b != id::GRASS && b != id::DIRT && b != id::STONE {
                position = below;
                continue;
            }
        }
        if position.y <= 3 {
            return false;
        }
        let i1 = start_radius;
        let mut i = 0;
        while i1 >= 0 && i < 3 {
            let j = i1 + rand.next_int_bounded(2);
            let k = i1 + rand.next_int_bounded(2);
            let l = i1 + rand.next_int_bounded(2);
            let f = (j + k + l) as f32 * 0.333 + 0.5;
            let r = (f * f) as f64;
            // BlockPos.getAllInBox: x fastest, then y, then z.
            for bz in position.z - l..=position.z + l {
                for by in position.y - k..=position.y + k {
                    for bx in position.x - j..=position.x + j {
                        let bp = Pos::new(bx, by, bz);
                        if bp.distance_sq(position) <= r {
                            w.set(bp, blocks::state(block, 0), 4);
                        }
                    }
                }
            }
            let dx = -(i1 + 1) + rand.next_int_bounded(2 + i1 * 2);
            let dy = 0 - rand.next_int_bounded(2);
            let dz = -(i1 + 1) + rand.next_int_bounded(2 + i1 * 2);
            position = position.add(dx, dy, dz);
            i += 1;
        }
        return true;
    }
}
