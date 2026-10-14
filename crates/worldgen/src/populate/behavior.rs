//! Per-block behaviour population can trigger: `onBlockAdded`, `breakBlock`, `neighborChanged`,
//! `updateTick`, the support checks the features ask (`canPlaceBlockAt`, `canBlockStay`) and the
//! item drops whose `World.rand` draws shift everything after them.
//!
//! Only the blocks population places or touches are here. Every other block takes the `Block` base
//! behaviour, which does nothing for all four hooks.

use super::liquid;
use super::props::material;
use super::state::{self, Facing, Pos};
use super::world::{PopWorld, TickRand};
use crate::blocks::{self, block_of, id};

fn is_liquid_block(b: u16) -> bool {
    (id::FLOWING_WATER..=id::LAVA).contains(&b)
}

/// `Block.onBlockAdded`, dispatched on the new block.
pub fn on_block_added(w: &mut PopWorld, p: Pos, s: u16) {
    match block_of(s) {
        b if is_liquid_block(b) => liquid::on_block_added(w, p, s),
        // BlockFalling.onBlockAdded
        id::SAND | id::GRAVEL => w.schedule_update(p, block_of(s)),
        id::CHEST => chest_on_block_added(w, p, s),
        // BlockTorch.onBlockAdded
        id::TORCH => {
            torch_check_for_drop(w, p, s);
        }
        // DROPPED: BlockPumpkin.trySpawnGolem. Population never builds the snow or iron block
        // pattern under a pumpkin, so the match always fails.
        _ => {}
    }
}

/// `Block.breakBlock`, dispatched on the old block.
pub fn break_block(w: &mut PopWorld, p: Pos, old: u16) {
    match block_of(old) {
        id::LEAVES | id::LEAVES2 => leaves_break_block(w, p),
        id::LOG | id::LOG2 => log_break_block(w, p),
        // DROPPED: BlockChest.breakBlock. A dungeon chest's loot is still a loot table reference,
        // so dropInventoryItems walks empty slots and draws nothing; the comparator update and the
        // tile entity removal have no block effect.
        _ => {}
    }
}

/// `Block.neighborChanged`, dispatched on the block at `p`.
pub fn neighbor_changed(w: &mut PopWorld, p: Pos, s: u16, _block: u16, _from: Pos) {
    match block_of(s) {
        b if is_liquid_block(b) => liquid::neighbor_changed(w, p, s),
        // BlockFalling.neighborChanged
        id::SAND | id::GRAVEL => w.schedule_update(p, block_of(s)),
        id::SAPLING
        | id::TALLGRASS
        | id::DEADBUSH
        | id::YELLOW_FLOWER
        | id::RED_FLOWER
        | id::BROWN_MUSHROOM
        | id::RED_MUSHROOM
        | id::WATERLILY
        | id::DOUBLE_PLANT => bush_check_and_drop_block(w, p, s),
        id::REEDS => {
            reed_check_for_drop(w, p, s);
        }
        id::CACTUS => {
            if !cactus_can_block_stay(w, p) {
                w.destroy_block(p, true);
            }
        }
        id::SNOW_LAYER => {
            if !snow_can_place_at(w, p) {
                drop_block_as_item(w, p, s);
                w.set_air(p);
            }
        }
        id::VINE => {
            if !vine_recheck_grown_sides(w, p, s) {
                drop_block_as_item(w, p, s);
                w.set_air(p);
            }
        }
        id::COCOA => {
            if !cocoa_can_block_stay(w, p, s) {
                w.set3(p, blocks::AIR);
                drop_block_as_item(w, p, s);
            }
        }
        // DROPPED: BlockChest.neighborChanged only refreshes the tile entity's cached neighbours.
        id::TORCH => {
            torch_on_neighbor_change_internal(w, p, s);
        }
        // BlockStandingSign.neighborChanged
        id::STANDING_SIGN => {
            if !state::mat_solid(w.get(p.down())) {
                drop_block_as_item(w, p, s);
                w.set_air(p);
            }
        }
        // BlockWallSign.neighborChanged
        id::WALL_SIGN => {
            if !state::mat_solid(w.get(p.offset(wall_sign_facing(s).opposite()))) {
                drop_block_as_item(w, p, s);
                w.set_air(p);
            }
        }
        _ => {}
    }
}

/// `BlockWallSign.getStateFromMeta`: `EnumFacing.byIndex(meta)`, NORTH for up and down.
pub fn wall_sign_facing(s: u16) -> Facing {
    match Facing::VALUES[(s & 15) as usize % 6] {
        Facing::Up | Facing::Down => Facing::North,
        f => f,
    }
}

/// `BlockContainer.hasInvalidNeighbor`: a cactus beside it on any side.
pub fn has_invalid_neighbor(w: &mut PopWorld, p: Pos) -> bool {
    [Facing::North, Facing::South, Facing::West, Facing::East].iter().any(|&f| state::mat(w.get(p.offset(f))) == material::CACTUS)
}

/// `Block.updateTick`. Only liquids and falling blocks schedule ticks during population, so only
/// they can get here.
pub fn update_tick(w: &mut PopWorld, p: Pos, s: u16, rand: TickRand) {
    match block_of(s) {
        b if is_liquid_block(b) => liquid::update_tick(w, p, s, rand),
        id::SAND | id::GRAVEL => falling_check_fallable(w, p, s),
        _ => {}
    }
}

// ---- BlockFalling ----

/// `BlockFalling.canFallThrough`
fn can_fall_through(s: u16) -> bool {
    let m = state::mat(s);
    block_of(s) == id::FIRE || m == material::AIR || m == material::WATER || m == material::LAVA
}

/// `BlockFalling.checkFallable`
fn falling_check_fallable(w: &mut PopWorld, p: Pos, s: u16) {
    let down = p.down();
    if (w.is_air(down) || can_fall_through(w.get(down))) && p.y >= 0 {
        // Ticks only run during population, where fallInstantly is set, so vanilla always takes
        // the instant branch.
        debug_assert!(w.fall_instantly);
        // DROPPED: the EntityFallingBlock branch (fallInstantly is false).
        w.set_air(p);
        let mut bp = p.down();
        while (w.is_air(bp) || can_fall_through(w.get(bp))) && bp.y > 0 {
            bp = bp.down();
        }
        if bp.y > 0 {
            // getDefaultState(): red sand lands as plain sand.
            w.set3(bp.up(), blocks::default_state(s));
        }
    }
}

// ---- BlockBush and its subclasses ----

/// `BlockBush.canSustainBush`, with the overrides in `BlockDeadBush`, `BlockMushroom` and
/// `BlockLilyPad`.
pub fn can_sustain_bush(block: u16, below: u16) -> bool {
    let b = block_of(below);
    match block {
        id::DEADBUSH => {
            b == id::SAND || b == id::HARDENED_CLAY || b == id::STAINED_HARDENED_CLAY || b == id::DIRT
        }
        id::BROWN_MUSHROOM | id::RED_MUSHROOM => state::is_full_block(below),
        id::WATERLILY => b == id::WATER || state::mat(below) == material::ICE,
        _ => b == id::GRASS || b == id::DIRT || b == id::FARMLAND,
    }
}

/// `canBlockStay` for the `BlockBush` family, `BlockReed` and `BlockCactus`.
pub fn can_block_stay(w: &mut PopWorld, p: Pos, s: u16) -> bool {
    match block_of(s) {
        id::BROWN_MUSHROOM | id::RED_MUSHROOM => {
            if p.y >= 0 && p.y < 256 {
                let below = w.get(p.down());
                if block_of(below) == id::MYCELIUM {
                    true
                } else if below == blocks::PODZOL {
                    true
                } else {
                    w.get_light(p) < 13 && can_sustain_bush(block_of(s), below)
                }
            } else {
                false
            }
        }
        id::WATERLILY => {
            if p.y >= 0 && p.y < 256 {
                let below = w.get(p.down());
                let m = state::mat(below);
                m == material::WATER && below & 15 == 0 || m == material::ICE
            } else {
                false
            }
        }
        id::DOUBLE_PLANT => {
            if s & 8 != 0 {
                block_of(w.get(p.down())) == id::DOUBLE_PLANT
            } else {
                let up = w.get(p.up());
                block_of(up) == id::DOUBLE_PLANT && {
                    let below = w.get(p.down());
                    can_sustain_bush(id::DOUBLE_PLANT, below)
                }
            }
        }
        id::REEDS => reed_can_place_block_at(w, p),
        id::CACTUS => cactus_can_block_stay(w, p),
        b => {
            let below = w.get(p.down());
            can_sustain_bush(b, below)
        }
    }
}

/// `Block.canPlaceBlockAt`: the block there now has a replaceable material.
fn base_can_place(w: &mut PopWorld, p: Pos) -> bool {
    state::mat_replaceable(w.get(p))
}

/// `Block.canPlaceBlockAt(world, pos)` for `block`, with its overrides.
pub fn can_place_block_at(w: &mut PopWorld, p: Pos, block: u16) -> bool {
    match block {
        id::SAPLING
        | id::TALLGRASS
        | id::DEADBUSH
        | id::YELLOW_FLOWER
        | id::RED_FLOWER
        | id::WATERLILY => {
            // BlockBush.canPlaceBlockAt
            base_can_place(w, p) && {
                let below = w.get(p.down());
                can_sustain_bush(block, below)
            }
        }
        id::BROWN_MUSHROOM | id::RED_MUSHROOM => {
            // BlockMushroom: super (BlockBush) && canBlockStay(default state)
            base_can_place(w, p)
                && {
                    let below = w.get(p.down());
                    can_sustain_bush(block, below)
                }
                && can_block_stay(w, p, block << 4)
        }
        id::DOUBLE_PLANT => {
            base_can_place(w, p)
                && {
                    let below = w.get(p.down());
                    can_sustain_bush(block, below)
                }
                && w.is_air(p.up())
        }
        id::REEDS => reed_can_place_block_at(w, p),
        id::CACTUS => base_can_place(w, p) && cactus_can_block_stay(w, p),
        id::PUMPKIN => {
            // BlockPumpkin.canPlaceBlockAt
            state::mat_replaceable(w.get(p)) && state::is_top_solid(w.get(p.down()))
        }
        id::SNOW_LAYER => snow_can_place_at(w, p),
        // BlockSign.canPlaceBlockAt
        id::STANDING_SIGN | id::WALL_SIGN => !has_invalid_neighbor(w, p) && base_can_place(w, p),
        // BlockTorch.canPlaceBlockAt: any facing but DOWN.
        id::TORCH => TORCH_FACINGS.iter().any(|&f| torch_can_place_at(w.get(p.offset(f.opposite())), f)),
        _ => base_can_place(w, p),
    }
}

/// `BlockTorch.FACING.getAllowedValues()`: every facing but DOWN, in index order.
pub const TORCH_FACINGS: [Facing; 5] = [Facing::Up, Facing::North, Facing::South, Facing::West, Facing::East];

/// `BlockTorch.getStateFromMeta`: EAST 1, WEST 2, SOUTH 3, NORTH 4, anything else UP.
pub fn torch_facing(s: u16) -> Facing {
    match s & 15 {
        1 => Facing::East,
        2 => Facing::West,
        3 => Facing::South,
        4 => Facing::North,
        _ => Facing::Up,
    }
}

/// `BlockTorch.getMetaFromState`
pub fn torch_meta(f: Facing) -> u16 {
    match f {
        Facing::East => 1,
        Facing::West => 2,
        Facing::South => 3,
        Facing::North => 4,
        Facing::Down | Facing::Up => 5,
    }
}

/// `Block.isExceptBlockForAttachWithPiston`
pub fn except_with_piston(b: u16) -> bool {
    (id::WHITE_SHULKER_BOX..=id::BLACK_SHULKER_BOX).contains(&b)
        || matches!(
            b,
            id::LEAVES
                | id::LEAVES2
                | id::TRAPDOOR
                | id::IRON_TRAPDOOR
                | id::BEACON
                | id::CAULDRON
                | id::GLASS
                | id::GLOWSTONE
                | id::ICE
                | id::SEA_LANTERN
                | id::STAINED_GLASS
                | id::PISTON
                | id::STICKY_PISTON
                | id::PISTON_HEAD
        )
}

/// `BlockTorch.canPlaceOn`, given the state at the support.
pub fn torch_can_place_on(support: u16) -> bool {
    let b = block_of(support);
    let flag = b == id::END_GATEWAY || b == id::LIT_PUMPKIN;
    if state::is_top_solid(support) {
        !flag
    } else {
        // `block instanceof BlockFence`: oak, nether brick, and spruce through acacia.
        let fence = matches!(b, id::OAK_FENCE | id::NETHER_BRICK_FENCE) || (id::SPRUCE_FENCE..=id::ACACIA_FENCE).contains(&b);
        (fence || b == id::GLASS || b == id::COBBLESTONE_WALL || b == id::STAINED_GLASS) && !flag
    }
}

/// `BlockTorch.canPlaceAt`, given the state at `pos.offset(facing.getOpposite())`.
pub fn torch_can_place_at(support: u16, facing: Facing) -> bool {
    if facing == Facing::Up && torch_can_place_on(support) {
        true
    } else if facing.is_horizontal() {
        !except_with_piston(block_of(support)) && state::solid_face(support, facing)
    } else {
        false
    }
}

/// `BlockTorch.checkForDrop`
fn torch_check_for_drop(w: &mut PopWorld, p: Pos, s: u16) -> bool {
    let f = torch_facing(s);
    if block_of(s) == id::TORCH && torch_can_place_at(w.get(p.offset(f.opposite())), f) {
        true
    } else {
        if block_of(w.get(p)) == id::TORCH {
            drop_block_as_item(w, p, s);
            w.set_air(p);
        }
        false
    }
}

/// `BlockTorch.onNeighborChangeInternal`
fn torch_on_neighbor_change_internal(w: &mut PopWorld, p: Pos, s: u16) -> bool {
    if !torch_check_for_drop(w, p, s) {
        return true;
    }
    let f = torch_facing(s);
    let support = w.get(p.offset(f.opposite()));
    let flag = if f.is_horizontal() { !state::solid_face(support, f) } else { !torch_can_place_on(support) };
    if flag {
        drop_block_as_item(w, p, s);
        w.set_air(p);
    }
    flag
}

/// `BlockBush.checkAndDropBlock`, and the `BlockDoublePlant` override.
fn bush_check_and_drop_block(w: &mut PopWorld, p: Pos, s: u16) {
    if block_of(s) == id::DOUBLE_PLANT {
        double_plant_check_and_drop_block(w, p, s);
        return;
    }
    if !can_block_stay(w, p, s) {
        drop_block_as_item(w, p, s);
        w.set(p, blocks::AIR, 3);
    }
}

/// `BlockDoublePlant.checkAndDropBlock`
fn double_plant_check_and_drop_block(w: &mut PopWorld, p: Pos, s: u16) {
    if !can_block_stay(w, p, s) {
        let flag = s & 8 != 0;
        let blockpos = if flag { p } else { p.up() };
        let blockpos1 = if flag { p.down() } else { p };
        let block = if flag { id::DOUBLE_PLANT } else { block_of(w.get(blockpos)) };
        let block1 = if flag { block_of(w.get(blockpos1)) } else { id::DOUBLE_PLANT };
        if block == id::DOUBLE_PLANT {
            w.set(blockpos, blocks::AIR, 2);
        }
        if block1 == id::DOUBLE_PLANT {
            w.set(blockpos1, blocks::AIR, 3);
            if !flag {
                drop_block_as_item(w, blockpos1, s);
            }
        }
    }
}

// ---- BlockReed ----

/// `BlockReed.canPlaceBlockAt`, which `canBlockStay` also is.
pub fn reed_can_place_block_at(w: &mut PopWorld, p: Pos) -> bool {
    let block = block_of(w.get(p.down()));
    if block == id::REEDS {
        true
    } else if block != id::GRASS && block != id::DIRT && block != id::SAND {
        false
    } else {
        let blockpos = p.down();
        for f in Facing::HORIZONTAL {
            let s = w.get(blockpos.offset(f));
            if state::mat(s) == material::WATER || block_of(s) == id::FROSTED_ICE {
                return true;
            }
        }
        false
    }
}

/// `BlockReed.checkForDrop`
fn reed_check_for_drop(w: &mut PopWorld, p: Pos, s: u16) -> bool {
    if reed_can_place_block_at(w, p) {
        true
    } else {
        drop_block_as_item(w, p, s);
        w.set_air(p);
        false
    }
}

// ---- BlockCactus ----

/// `BlockCactus.canBlockStay`
pub fn cactus_can_block_stay(w: &mut PopWorld, p: Pos) -> bool {
    for f in Facing::HORIZONTAL {
        let s = w.get(p.offset(f));
        if state::mat_solid(s) || state::mat(s) == material::LAVA {
            return false;
        }
    }
    let block = block_of(w.get(p.down()));
    block == id::CACTUS || block == id::SAND && !state::mat_liquid(w.get(p.up()))
}

// ---- BlockSnow ----

/// `BlockSnow.canPlaceBlockAt`
pub fn snow_can_place_at(w: &mut PopWorld, p: Pos) -> bool {
    let below = w.get(p.down());
    let block = block_of(below);
    if block != id::ICE && block != id::PACKED_ICE && block != id::BARRIER {
        state::solid_face(below, Facing::Up)
            || state::mat(below) == material::LEAVES
            || block == id::SNOW_LAYER && below & 15 == 7
    } else {
        false
    }
}

// ---- BlockVine ----

/// The `BlockVine` metadata bit for a horizontal facing: SOUTH 1, WEST 2, NORTH 4, EAST 8.
pub fn vine_bit(f: Facing) -> u16 {
    match f {
        Facing::South => 1,
        Facing::West => 2,
        Facing::North => 4,
        Facing::East => 8,
        _ => 0,
    }
}

/// `BlockVine.isExceptBlockForAttaching`
pub fn vine_except(b: u16) -> bool {
    (id::WHITE_SHULKER_BOX..=id::BLACK_SHULKER_BOX).contains(&b)
        || matches!(
            b,
            id::BEACON | id::CAULDRON | id::GLASS | id::STAINED_GLASS | id::PISTON | id::STICKY_PISTON | id::PISTON_HEAD | id::TRAPDOOR
        )
}

/// `BlockVine.isAcceptableNeighbor`
fn vine_acceptable_neighbor(w: &mut PopWorld, p: Pos, f: Facing) -> bool {
    let s = w.get(p);
    state::solid_face(s, f) && !vine_except(block_of(s))
}

/// `BlockVine.canAttachTo`
pub fn vine_can_attach_to(w: &mut PopWorld, p: Pos, f: Facing) -> bool {
    let block = block_of(w.get(p.up()));
    vine_acceptable_neighbor(w, p.offset(f.opposite()), f)
        && (block == id::AIR || block == id::VINE || vine_acceptable_neighbor(w, p.up(), Facing::Up))
}

/// `BlockVine.canPlaceBlockOnSide`
pub fn vine_can_place_on_side(w: &mut PopWorld, p: Pos, side: Facing) -> bool {
    side != Facing::Down && side != Facing::Up && vine_can_attach_to(w, p, side)
}

/// `BlockVine.recheckGrownSides`. The stored state never has UP set (it is an actual-state
/// property), so `getNumGrownFaces` counts the four meta bits.
fn vine_recheck_grown_sides(w: &mut PopWorld, p: Pos, s: u16) -> bool {
    let original = s;
    let mut st = s;
    for f in Facing::HORIZONTAL {
        let bit = vine_bit(f);
        if st & bit != 0 && !vine_can_attach_to(w, p, f.opposite()) {
            let up = w.get(p.up());
            if block_of(up) != id::VINE || up & bit == 0 {
                st &= !bit;
            }
        }
    }
    if st & 15 == 0 {
        false
    } else {
        if original != st {
            w.set(p, st, 2);
        }
        true
    }
}

// ---- BlockCocoa ----

/// `BlockCocoa.canBlockStay`: the block it faces is a jungle log.
fn cocoa_can_block_stay(w: &mut PopWorld, p: Pos, s: u16) -> bool {
    let f = Facing::BY_HORIZONTAL[(s & 3) as usize];
    let t = w.get(p.offset(f));
    block_of(t) == id::LOG && t & 3 == 3
}

// ---- BlockChest ----

/// The `BlockChest.FACING` of a stored chest state; vertical metadata reads as NORTH.
fn chest_facing(s: u16) -> Facing {
    match s & 15 {
        3 => Facing::South,
        4 => Facing::West,
        5 => Facing::East,
        _ => Facing::North,
    }
}

/// `BlockChest.onBlockAdded`
fn chest_on_block_added(w: &mut PopWorld, p: Pos, s: u16) {
    check_for_surrounding_chests(w, p, s);
    for f in Facing::HORIZONTAL {
        let bp = p.offset(f);
        let st = w.get(bp);
        if block_of(st) == id::CHEST {
            check_for_surrounding_chests(w, bp, st);
        }
    }
}

/// `BlockChest.checkForSurroundingChests`
fn check_for_surrounding_chests(w: &mut PopWorld, p: Pos, s: u16) -> u16 {
    let chest = |st: u16| block_of(st) == id::CHEST;
    let full = state::is_full_block;
    let n = w.get(p.north());
    let so = w.get(p.south());
    let we = w.get(p.west());
    let ea = w.get(p.east());
    let mut facing = chest_facing(s);
    if !chest(n) && !chest(so) {
        let flag = full(n);
        let flag1 = full(so);
        if chest(we) || chest(ea) {
            let bp1 = if chest(we) { p.west() } else { p.east() };
            let s7 = w.get(bp1.north());
            let s6 = w.get(bp1.south());
            facing = Facing::South;
            let facing2 = if chest(we) { chest_facing(we) } else { chest_facing(ea) };
            if facing2 == Facing::North {
                facing = Facing::North;
            }
            if (flag || full(s7)) && !flag1 && !full(s6) {
                facing = Facing::South;
            }
            if (flag1 || full(s6)) && !flag && !full(s7) {
                facing = Facing::North;
            }
        }
    } else {
        let bp = if chest(n) { p.north() } else { p.south() };
        let s4 = w.get(bp.west());
        let s5 = w.get(bp.east());
        facing = Facing::East;
        let facing1 = if chest(n) { chest_facing(n) } else { chest_facing(so) };
        if facing1 == Facing::West {
            facing = Facing::West;
        }
        if (full(we) || full(s4)) && !full(ea) && !full(s5) {
            facing = Facing::East;
        }
        if (full(ea) || full(s5)) && !full(we) && !full(s4) {
            facing = Facing::West;
        }
    }
    let st = blocks::state(id::CHEST, facing as u16);
    w.set(p, st, 3);
    st
}

/// `BlockChest.correctFacing`, which `WorldGenDungeons` applies to the default state.
pub fn chest_correct_facing(w: &mut PopWorld, p: Pos, s: u16) -> u16 {
    let mut facing: Option<Facing> = None;
    for f in Facing::HORIZONTAL {
        let st = w.get(p.offset(f));
        if block_of(st) == id::CHEST {
            return s;
        }
        if state::is_full_block(st) {
            if facing.is_some() {
                facing = None;
                break;
            }
            facing = Some(f);
        }
    }
    let f = match facing {
        Some(f) => f.opposite(),
        None => {
            let mut f2 = chest_facing(s);
            if state::is_full_block(w.get(p.offset(f2))) {
                f2 = f2.opposite();
            }
            if state::is_full_block(w.get(p.offset(f2))) {
                f2 = f2.rotate_y();
            }
            if state::is_full_block(w.get(p.offset(f2))) {
                f2 = f2.opposite();
            }
            f2
        }
    };
    blocks::state(id::CHEST, f as u16)
}

// ---- BlockLeaves and BlockLog ----

/// `BlockLeaves.CHECK_DECAY` in the metadata.
const CHECK_DECAY: u16 = 8;

/// `BlockLeaves.breakBlock`
fn leaves_break_block(w: &mut PopWorld, p: Pos) {
    if w.is_area_loaded(p.add(-2, -2, -2), p.add(2, 2, 2)) {
        for j1 in -1..=1 {
            for k1 in -1..=1 {
                for l1 in -1..=1 {
                    let bp = p.add(j1, k1, l1);
                    let st = w.get(bp);
                    if state::mat(st) == material::LEAVES && st & CHECK_DECAY == 0 {
                        w.set(bp, st | CHECK_DECAY, 4);
                    }
                }
            }
        }
    }
}

/// `BlockLog.breakBlock`. `BlockPos.getAllInBox` steps x fastest, then y, then z.
fn log_break_block(w: &mut PopWorld, p: Pos) {
    if w.is_area_loaded(p.add(-5, -5, -5), p.add(5, 5, 5)) {
        for z in -4..=4 {
            for y in -4..=4 {
                for x in -4..=4 {
                    let bp = p.add(x, y, z);
                    let st = w.get(bp);
                    if state::mat(st) == material::LEAVES && st & CHECK_DECAY == 0 {
                        w.set(bp, st | CHECK_DECAY, 4);
                    }
                }
            }
        }
    }
}

// ---- Drops ----

/// `Block.spawnAsEntity`: the item's offset draws three floats from `World.rand`.
fn spawn_as_entity(w: &mut PopWorld) {
    w.rand.next_float();
    w.rand.next_float();
    w.rand.next_float();
    // DROPPED: the EntityItem itself. Its constructor uses Math.random, and items never touch
    // blocks during population.
}

/// `quantityDroppedWithBonus(0, World.rand)`
fn quantity_dropped(w: &mut PopWorld, s: u16) -> i32 {
    match block_of(s) {
        b if is_liquid_block(b) => 0,
        id::SNOW_LAYER | id::VINE | id::FIRE | id::ICE | id::PACKED_ICE | id::PORTAL | id::MOB_SPAWNER => 0,
        id::MONSTER_EGG | id::GLASS | id::STAINED_GLASS | id::PISTON_HEAD => 0,
        // BlockTallGrass: 1 + nextInt(fortune * 2 + 1)
        id::TALLGRASS => 1 + w.rand.next_int_bounded(1),
        // BlockDeadBush
        id::DEADBUSH => w.rand.next_int_bounded(3),
        // BlockMelon: min(9, 3 + nextInt(5) + nextInt(1 + fortune))
        id::MELON_BLOCK => {
            let q = 3 + w.rand.next_int_bounded(5);
            (q + w.rand.next_int_bounded(1)).min(9)
        }
        id::CLAY | id::SNOW => 4,
        _ => 1,
    }
}

/// Whether `getItemDropped(state, World.rand, 0)` is something other than `Items.AIR`.
fn item_dropped(w: &mut PopWorld, s: u16) -> bool {
    match block_of(s) {
        // BlockTallGrass: seeds one time in 8
        id::TALLGRASS => w.rand.next_int_bounded(8) == 0,
        id::DOUBLE_PLANT => {
            if s & 8 != 0 {
                false
            } else {
                match s & 7 {
                    3 => false,
                    2 => w.rand.next_int_bounded(8) == 0,
                    _ => true,
                }
            }
        }
        // BlockGravel: flint or gravel, either way an item
        id::GRAVEL => {
            w.rand.next_int_bounded(10);
            true
        }
        _ => true,
    }
}

/// `Block.dropBlockAsItem(world, pos, state, 0)`, which is `dropBlockAsItemWithChance(.., 1.0, 0)`.
pub fn drop_block_as_item(w: &mut PopWorld, _p: Pos, s: u16) {
    match block_of(s) {
        id::LEAVES | id::LEAVES2 => {
            // BlockLeaves.dropBlockAsItemWithChance
            let variant = s & 3;
            let old = block_of(s) == id::LEAVES;
            let chance = if old && variant == 3 { 40 } else { 20 };
            if w.rand.next_int_bounded(chance) == 0 {
                spawn_as_entity(w);
            }
            // dropApple(200): BlockOldLeaf for OAK, BlockNewLeaf for DARK_OAK.
            if (old && variant == 0) || (!old && variant == 1) {
                if w.rand.next_int_bounded(200) == 0 {
                    spawn_as_entity(w);
                }
            }
        }
        id::COCOA => {
            // BlockCocoa.dropBlockAsItemWithChance: no chance roll, 3 beans when fully grown.
            let n = if (s & 15) >> 2 >= 2 { 3 } else { 1 };
            for _ in 0..n {
                spawn_as_entity(w);
            }
        }
        _ => {
            let n = quantity_dropped(w, s);
            for _ in 0..n {
                if w.rand.next_float() <= 1.0 && item_dropped(w, s) {
                    spawn_as_entity(w);
                }
            }
        }
    }
}
