//! Doors in the creative player's hands: `ItemDoor.onItemUse` and `placeDoor`, `BlockDoor`'s
//! `canPlaceBlockAt`, `onBlockActivated` and `onBlockHarvested`, and `getItem` for the middle click.

use items::ids::{block as iblock, item};
use items::ItemStack;
use worldgen::blocks::{block_of, id, is_door, state};
use worldgen::populate::state as st;
use worldgen::populate::Facing;

use super::{down, edit, get, horizontal_facing, is_replaceable, up, Action, World};
use crate::collision::BlockAccess;
use crate::player::Player;
use crate::target::{offset, BlockPos, Hit};
use sim::door::{lower_meta, upper_meta};

/// `BlockDoor.canPlaceBlockAt`: below 255, on a solid top, and room for both halves
/// (`Block.canPlaceBlockAt` at the block and the one above).
pub(super) fn can_place_door(w: &impl BlockAccess, p: BlockPos) -> bool {
    p.1 < 255 && st::is_top_solid(get(w, down(p))) && is_replaceable(w, p) && is_replaceable(w, up(p))
}

/// `ItemDoor.onItemUse`: only on a top face; the door faces the way the player looks, hinged on
/// the side of the face the click was nearer.
/// DROPPED: `canPlayerEdit`, true for a creative player; the place sound, which `Streamer::edit`
/// plays from the lower half; and `shrink`, as creative keeps the stack.
pub(super) fn item_door_use(w: &mut impl World, player: &Player, door: u16, hit: Hit) -> Action {
    if hit.face != Facing::Up {
        return Action::Fail;
    }
    let mut p = hit.pos;
    let (hit_x, hit_z) = ((hit.vec.x - p.0 as f64) as f32, (hit.vec.z - p.2 as f64) as f32);
    if !is_replaceable(w, p) {
        p = offset(p, hit.face);
    }
    if !can_place_door(w, p) {
        return Action::Fail;
    }
    // EnumFacing.fromAngle(rotationYaw)
    let facing = horizontal_facing(player.yaw);
    let (i, _, j) = facing.offset();
    let right = i < 0 && hit_z < 0.5 || i > 0 && hit_z > 0.5 || j < 0 && hit_x > 0.5 || j > 0 && hit_x < 0.5;
    place_door(w, p, facing, door, right);
    Action::Success
}

/// `ItemDoor.placeDoor`: the hinge goes to the side with more solid blocks, or away from a door
/// already standing beside this one, so two doors pair up.
/// DROPPED: `isBlockPowered`; nothing gives power, so POWERED and OPEN start false.
pub(super) fn place_door(w: &mut impl World, p: BlockPos, facing: Facing, door: u16, right_hinge: bool) {
    let right_of = offset(p, facing.rotate_y());
    let left_of = offset(p, facing.rotate_y_ccw());
    let normal = |q: BlockPos| st::is_normal_cube(get(w, q)) as i32;
    let i = normal(left_of) + normal(up(left_of));
    let j = normal(right_of) + normal(up(right_of));
    let flag = block_of(get(w, left_of)) == door || block_of(get(w, up(left_of))) == door;
    let flag1 = block_of(get(w, right_of)) == door || block_of(get(w, up(right_of))) == door;
    let mut right = right_hinge;
    if (!flag || flag1) && j <= i {
        if flag1 && !flag || j < i {
            right = false;
        }
    } else {
        right = true;
    }
    // Both halves with flags 2, then notifyNeighborsOfStateChange for each; the lower half
    // carries the placement check the server repeats.
    w.edit(vec![
        edit(p, state(door, lower_meta(facing, false)), 3, true),
        edit(up(p), state(door, upper_meta(right, false)), 3, false),
    ]);
}

/// `BlockDoor.onBlockActivated`: a wooden door swings open or shut (OPEN lives on the lower
/// half); an iron door ignores the hand.
/// DROPPED: the open and close sounds (`playEvent` 1006 and 1012).
pub(super) fn activate(w: &mut impl World, p: BlockPos) -> bool {
    let s = get(w, p);
    let b = block_of(s);
    if !is_door(b) || b == id::IRON_DOOR {
        return false;
    }
    let lower = if s & 8 == 0 { p } else { down(p) };
    let ls = if lower == p { s } else { get(w, lower) };
    if block_of(ls) != b {
        return false;
    }
    w.edit(vec![edit(lower, ls ^ 4, 10, false)]);
    true
}

/// `BlockDoor.onBlockHarvested` for a creative player: the other half goes with the one broken.
/// Pushed before the broken block's own removal.
pub(super) fn harvest_edits(w: &impl BlockAccess, p: BlockPos, s: u16, edits: &mut Vec<super::Edit>) {
    let b = block_of(s);
    if s & 8 != 0 {
        if block_of(get(w, down(p))) == b {
            edits.push(edit(down(p), 0, 3, false));
        }
    } else if block_of(get(w, up(p))) == b {
        edits.push(edit(p, 0, 3, false));
        edits.push(edit(up(p), 0, 3, false));
    }
}

/// The door block an `ItemDoor` places; `None` for any other item.
pub(super) fn door_block(it: u16) -> Option<u16> {
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

/// `BlockDoor.getItem`: the door item of the block.
pub(super) fn door_item(b: u16) -> ItemStack {
    ItemStack::of(match b {
        iblock::IRON_DOOR => item::IRON_DOOR,
        iblock::SPRUCE_DOOR => item::SPRUCE_DOOR,
        iblock::BIRCH_DOOR => item::BIRCH_DOOR,
        iblock::JUNGLE_DOOR => item::JUNGLE_DOOR,
        iblock::ACACIA_DOOR => item::ACACIA_DOOR,
        iblock::DARK_OAK_DOOR => item::DARK_OAK_DOOR,
        _ => item::OAK_DOOR,
    })
}
