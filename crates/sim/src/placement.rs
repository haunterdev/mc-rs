//! The block placement rules the viewer's prediction and the server's `ItemBlock.onItemUse` share:
//! `Block.isReplaceable`, `canPlaceBlockAt`, `canPlaceBlockOnSide` and `World.mayPlace`.

use worldgen::blocks::{id, state};
use worldgen::populate::behavior::{can_sustain_bush, torch_can_place_at, vine_except, TORCH_FACINGS};
use worldgen::populate::props::material;
use worldgen::populate::state as st;
use worldgen::populate::Facing;

use crate::collision::{collision_box, Aabb, BlockAccess};
use crate::raytrace::{offset, BlockPos};
use crate::region::{props, Mat};

fn get(w: &impl BlockAccess, p: BlockPos) -> u16 {
    w.state(p.0, p.1, p.2)
}

fn up(p: BlockPos) -> BlockPos {
    (p.0, p.1 + 1, p.2)
}

fn down(p: BlockPos) -> BlockPos {
    (p.0, p.1 - 1, p.2)
}

/// `BlockDoublePlant.getActualState`'s variant: the upper half takes the lower half's.
pub fn double_plant_variant(w: &impl BlockAccess, s: u16, p: BlockPos) -> u16 {
    if s & 8 != 0 {
        let below = get(w, down(p));
        if below >> 4 == id::DOUBLE_PLANT {
            below & 7
        } else {
            0
        }
    } else {
        s & 7
    }
}

/// `Block.isReplaceable`, with the overrides in `BlockSnow`, `BlockDoublePlant`,
/// `BlockTallGrass`, `BlockDeadBush` and `BlockVine`.
pub fn is_replaceable(w: &impl BlockAccess, p: BlockPos) -> bool {
    let s = get(w, p);
    match s >> 4 {
        id::SNOW_LAYER => s & 7 == 0,
        // Double grass and the large fern.
        id::DOUBLE_PLANT => matches!(double_plant_variant(w, s, p), 2 | 3),
        id::TALLGRASS | id::DEADBUSH | id::VINE => true,
        _ => st::mat_replaceable(s),
    }
}

/// `BlockVine.isAcceptableNeighbor`
pub fn vine_acceptable(w: &impl BlockAccess, p: BlockPos, f: Facing) -> bool {
    let s = get(w, p);
    st::solid_face(s, f) && !vine_except(s >> 4)
}

/// `BlockVine.canAttachTo`
pub fn vine_can_attach_to(w: &impl BlockAccess, p: BlockPos, side: Facing) -> bool {
    let above = get(w, up(p)) >> 4;
    vine_acceptable(w, offset(p, side.opposite()), side) && (above == id::AIR || above == id::VINE || vine_acceptable(w, up(p), Facing::Up))
}

/// `Block.canPlaceBlockAt` for the blocks the generator places, as the client sees it.
/// DROPPED: `BlockMushroom.canBlockStay`'s light below 13; the client has no light to read, and
/// the populator checks it.
pub fn can_place_block_at(w: &impl BlockAccess, p: BlockPos, block: u16) -> bool {
    let below = get(w, down(p));
    match block {
        // BlockBush, BlockMushroom
        id::SAPLING | id::TALLGRASS | id::DEADBUSH | id::YELLOW_FLOWER | id::RED_FLOWER | id::WATERLILY | id::BROWN_MUSHROOM | id::RED_MUSHROOM => {
            is_replaceable(w, p) && can_sustain_bush(block, below)
        }
        id::DOUBLE_PLANT => is_replaceable(w, p) && can_sustain_bush(block, below) && get(w, up(p)) == 0,
        // BlockReed: on reeds, or on grass, dirt or sand beside water.
        id::REEDS => {
            let b = below >> 4;
            b == id::REEDS
                || (matches!(b, id::GRASS | id::DIRT | id::SAND)
                    && Facing::HORIZONTAL.iter().any(|&f| {
                        let n = get(w, offset(down(p), f));
                        props(n).mat == Mat::Water || n >> 4 == id::FROSTED_ICE
                    }))
        }
        // BlockCactus.canBlockStay
        id::CACTUS => {
            is_replaceable(w, p)
                && Facing::HORIZONTAL.iter().all(|&f| {
                    let n = get(w, offset(p, f));
                    !st::mat_solid(n) && props(n).mat != Mat::Lava
                })
                && matches!(below >> 4, id::CACTUS | id::SAND)
                && !st::mat_liquid(get(w, up(p)))
        }
        id::PUMPKIN => st::mat_replaceable(get(w, p)) && st::is_top_solid(below),
        // BlockSign: no cactus beside it (BlockContainer.hasInvalidNeighbor), and Block's rule.
        id::STANDING_SIGN | id::WALL_SIGN => {
            !Facing::HORIZONTAL.iter().any(|&f| st::mat(get(w, offset(p, f))) == material::CACTUS) && is_replaceable(w, p)
        }
        // BlockTorch.canPlaceBlockAt
        id::TORCH => TORCH_FACINGS.iter().any(|&f| torch_can_place_at(get(w, offset(p, f.opposite())), f)),
        id::SNOW_LAYER => {
            let b = below >> 4;
            !matches!(b, id::ICE | id::PACKED_ICE | id::BARRIER)
                && (st::solid_face(below, Facing::Up) || matches!(b, id::LEAVES | id::LEAVES2) || below == state(id::SNOW_LAYER, 7))
        }
        _ => is_replaceable(w, p),
    }
}

/// `Block.canPlaceBlockOnSide`, with `BlockVine`'s.
pub fn can_place_block_on_side(w: &impl BlockAccess, p: BlockPos, block: u16, side: Facing) -> bool {
    if block == id::VINE {
        side.is_horizontal() && vine_can_attach_to(w, p, side)
    } else {
        can_place_block_at(w, p, block)
    }
}

pub fn clear_of(b: Aabb, p: BlockPos, player: &Aabb) -> bool {
    !b.offset(p.0 as f64, p.1 as f64, p.2 as f64).intersects(player)
}

/// `World.mayPlace(block, pos, false, side, null)`: the block's default collision box clear of
/// the player, the block there replaceable, and the block's own rule.
/// DROPPED: the anvil over a circuit.
pub fn may_place(w: &impl BlockAccess, bb: &Aabb, block: u16, p: BlockPos, side: Facing) -> bool {
    may_place_among(w, |b| !b.intersects(bb), block, p, side)
}

/// [`may_place`] with `World.checkNoEntityCollision` as `clear`: true when no entity stands in the
/// box (the block's collision box, already offset to `p`).
pub fn may_place_among(w: &impl BlockAccess, clear: impl Fn(Aabb) -> bool, block: u16, p: BlockPos, side: Facing) -> bool {
    if let Some(b) = collision_box(state(block, 0)) {
        if !clear(b.offset(p.0 as f64, p.1 as f64, p.2 as f64)) {
            return false;
        }
    }
    st::mat_replaceable(get(w, p)) && can_place_block_on_side(w, p, block, side)
}
