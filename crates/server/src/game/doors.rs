//! Doors: `ItemDoor.onItemUse` and `placeDoor` (the item in the `USES` table of use_on.rs) and
//! `BlockDoor.onBlockActivated` (a right click on a wooden door swings it). The viewer predicts
//! both and sends a `V_USE_ON`; this is the answer the world keeps.
//!
//! DROPPED: `canPlayerEdit`, `isBlockPowered` and the redstone that swings or powers a door (an
//! iron door ignores the hand and nothing opens it, as nothing gives power here), the neighbour
//! notifications, and `markBlockRangeForRenderUpdate`.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{UseOn, SNEAKING};
use sim::door::{lower_meta, upper_meta};
use sim::placement::is_replaceable;
use worldgen::blocks::{block_of, id, is_door, state};
use worldgen::populate::state as st;
use worldgen::populate::Facing;

use super::mobs::sounds;
use super::terrain::{self, Blocks};
use super::use_on::Used;
use super::Game;

/// `SoundCategory.BLOCKS`
const BLOCKS: u8 = 4;
/// `World.setBlockState` flags of the two halves of a placed door, and of a swing.
const PLACE_FLAGS: i32 = 2;
const SWING_FLAGS: i32 = 10;

/// The door block an `ItemDoor` places.
pub fn door_block(it: u16) -> Option<u16> {
    Some(match it {
        item::OAK_DOOR => id::WOODEN_DOOR,
        item::IRON_DOOR => id::IRON_DOOR,
        item::SPRUCE_DOOR => id::SPRUCE_DOOR,
        item::BIRCH_DOOR => id::BIRCH_DOOR,
        item::JUNGLE_DOOR => id::JUNGLE_DOOR,
        item::ACACIA_DOOR => id::ACACIA_DOOR,
        item::DARK_OAK_DOOR => id::DARK_OAK_DOOR,
        _ => return None,
    })
}

pub fn is_door_item(s: &ItemStack) -> bool {
    !s.is_empty() && door_block(s.item).is_some()
}

type Pos = (i32, i32, i32);

fn up(p: Pos) -> Pos {
    (p.0, p.1 + 1, p.2)
}

fn offset(p: Pos, f: Facing) -> Pos {
    let (x, y, z) = f.offset();
    (p.0 + x, p.1 + y, p.2 + z)
}

/// `EnumFacing.fromAngle`: `getHorizontal(floor(angle / 90 + 0.5) & 3)`.
fn from_angle(yaw: f32) -> Facing {
    Facing::BY_HORIZONTAL[((yaw as f64 / 90.0 + 0.5).floor() as i64 & 3) as usize]
}

/// `BlockDoor.canPlaceBlockAt`: below 255, a solid top underneath, and both cells replaceable.
fn can_place_at(game: &Game, p: Pos) -> bool {
    p.1 < 255 && st::is_top_solid(terrain::state(game, (p.0, p.1 - 1, p.2))) && is_replaceable(&Blocks(game), p) && is_replaceable(&Blocks(game), up(p))
}

/// `ItemDoor.onItemUse`: only on a top face; the door faces the way the player looks, hinged on
/// the side of the face the click was nearer.
/// DROPPED: `canPlayerEdit`.
pub fn item_use(game: &mut Game, pid: u32, m: &UseOn, stack: ItemStack) -> Used {
    let Some(door) = door_block(stack.item) else { return Used::No };
    let Some(face) = Facing::VALUES.get(m.face as usize).copied() else { return Used::No };
    let Some(yaw) = game.sessions.get(&pid).and_then(|s| s.state).map(|s| s.yaw) else { return Used::No };
    if face != Facing::Up {
        return Used::No;
    }
    let mut p = (m.pos[0], m.pos[1], m.pos[2]);
    if !is_replaceable(&Blocks(game), p) {
        p = offset(p, face);
    }
    if !can_place_at(game, p) {
        return Used::No;
    }
    let facing = from_angle(yaw);
    let (i, _, j) = facing.offset();
    let (hit_x, hit_z) = (m.hit[0], m.hit[2]);
    let right = i < 0 && hit_z < 0.5 || i > 0 && hit_z > 0.5 || j < 0 && hit_x > 0.5 || j > 0 && hit_x < 0.5;
    place_door(game, p, facing, door, right);
    // SoundType.getPlaceSound at ((volume + 1) / 2, pitch * 0.8): wood is 1.0 and 1.0, metal 1.0 and 1.5.
    let (name, pitch) = if door == id::IRON_DOOR { ("block.metal.place", 1.5 * 0.8) } else { ("block.wood.place", 0.8) };
    sounds::play_sound_except(game, Some(pid), [p.0 as f64 + 0.5, p.1 as f64 + 0.5, p.2 as f64 + 0.5], name, BLOCKS, 1.0, pitch);
    Used::Success { consume: true }
}

/// `ItemDoor.placeDoor`: the hinge goes to the side with more solid blocks, or away from a door
/// already standing beside this one, so two doors pair up.
pub fn place_door(game: &mut Game, p: Pos, facing: Facing, door: u16, right_hinge: bool) {
    let right_of = offset(p, facing.rotate_y());
    let left_of = offset(p, facing.rotate_y_ccw());
    let normal = |g: &Game, q: Pos| st::is_normal_cube(terrain::state(g, q)) as i32;
    let is_door_at = |g: &Game, q: Pos| block_of(terrain::state(g, q)) == door;
    let i = normal(game, left_of) + normal(game, up(left_of));
    let j = normal(game, right_of) + normal(game, up(right_of));
    let flag = is_door_at(game, left_of) || is_door_at(game, up(left_of));
    let flag1 = is_door_at(game, right_of) || is_door_at(game, up(right_of));
    let mut right = right_hinge;
    if (!flag || flag1) && j <= i {
        if flag1 && !flag || j < i {
            right = false;
        }
    } else {
        right = true;
    }
    // POWERED and OPEN start false: DROPPED isBlockPowered.
    terrain::set_flags(game, p, state(door, lower_meta(facing, false)), PLACE_FLAGS);
    terrain::set_flags(game, up(p), state(door, upper_meta(right, false)), PLACE_FLAGS);
}

/// `BlockDoor.onBlockActivated`. True when the click was a door's: a wooden door swings (OPEN
/// lives on the lower half) and plays the open or close sound; an iron door is not used, so its
/// click falls through to the item. A sneaking player with an item in hand uses the item.
pub fn use_on(game: &mut Game, id: u32, m: &UseOn) -> bool {
    let p = (m.pos[0], m.pos[1], m.pos[2]);
    let s = terrain::state(game, p);
    let b = block_of(s);
    if !is_door(b) || b == block::IRON_DOOR {
        return false;
    }
    let held_any = game.players.players.get(&id).is_some_and(|pl| !pl.inv.get_current_item().is_empty() || !pl.inv.off_hand[0].is_empty());
    let sneaking = game.sessions.get(&id).and_then(|s| s.state).is_some_and(|s| s.flags & SNEAKING != 0);
    if sneaking && held_any {
        return false;
    }
    let lower = if s & 8 == 0 { p } else { (p.0, p.1 - 1, p.2) };
    let ls = if lower == p { s } else { terrain::state(game, lower) };
    if block_of(ls) != b {
        return false;
    }
    let now = ls ^ 4;
    terrain::set_flags(game, lower, now, SWING_FLAGS);
    // playEvent 1006 / 1012 (wooden door open / close), pitch rand * 0.1 + 0.9.
    let pitch = game.mobs.rand.next_float() * 0.1 + 0.9;
    let name = if now & 4 != 0 { "block.wooden_door.open" } else { "block.wooden_door.close" };
    sounds::play_sound(game, [p.0 as f64 + 0.5, p.1 as f64 + 0.5, p.2 as f64 + 0.5], name, BLOCKS, 1.0, pitch);
    true
}
