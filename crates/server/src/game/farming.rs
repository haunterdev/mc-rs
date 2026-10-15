//! The farming items in [`super::use_on`]'s table: `ItemHoe`, `ItemSeeds` / `ItemSeedFood` and
//! bone meal (`ItemDye.applyBonemeal`). What grows afterwards is blockticks.rs.
//!
//! DROPPED: `canPlayerEdit`, `ForgeEventFactory.onHoeUse`, the sound and the `playEvent(2005)`
//! particles, the cocoa bean planting by bone meal on a jungle log, bone meal on a mushroom or
//! a flower.

use ::items::ids::block as bid;
use ::items::{ItemStack, Kind};
use proto::UseOn;
use worldgen::populate::Facing;

use super::use_on::{self, Used};
use super::{blockticks, players, terrain, Game};

const DYE: u16 = ::items::ids::item::DYE;
/// `EnumDyeColor.WHITE.getDyeDamage()`: bone meal.
const BONE_MEAL: i32 = 15;

pub fn is_hoe(s: &ItemStack) -> bool {
    !s.is_empty() && matches!(s.def().kind, Kind::Hoe(_))
}

pub fn is_seed(s: &ItemStack) -> bool {
    !s.is_empty() && matches!(s.def().kind, Kind::Seeds { .. })
}

pub fn is_bone_meal(s: &ItemStack) -> bool {
    !s.is_empty() && s.item == DYE && s.damage == BONE_MEAL
}

/// `ItemHoe.onItemUse`: grass, a grass path and dirt become farmland, coarse dirt becomes dirt;
/// the block above must be air and the face not the bottom. One point of wear.
pub fn hoe_use(game: &mut Game, id: u32, m: &UseOn, _stack: ItemStack) -> Used {
    if Facing::VALUES.get(m.face as usize).copied() == Some(Facing::Down) {
        return Used::No;
    }
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    if terrain::state(game, (at.0, at.1 + 1, at.2)) >> 4 != 0 {
        return Used::No;
    }
    let s = terrain::state(game, at);
    let to = match (s >> 4, s & 15) {
        (bid::GRASS | bid::GRASS_PATH, _) | (bid::DIRT, 0) => bid::FARMLAND << 4,
        (bid::DIRT, 1) => bid::DIRT << 4,
        _ => return Used::No,
    };
    terrain::set_block(game, at, to);
    let creative = players::mode(game, id) == proto::GM_CREATIVE;
    if let Some(slot) = use_on::slot(game, id, m.hand) {
        if slot.damage_item(1, creative, &mut players::wear_rand()) {
            *slot = ItemStack::EMPTY;
        }
    }
    Used::Success { consume: false }
}

/// `ItemSeeds.onItemUse` (and `ItemSeedFood`): on the top face of the crop's soil, with air above,
/// the crop's first stage goes in. Farmland for everything but the nether wart's soul sand.
pub fn seed_use(game: &mut Game, _id: u32, m: &UseOn, stack: ItemStack) -> Used {
    let Kind::Seeds { crop } = stack.def().kind else { return Used::No };
    if Facing::VALUES.get(m.face as usize).copied() != Some(Facing::Up) {
        return Used::No;
    }
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let soil = if crop == bid::NETHER_WART { bid::SOUL_SAND } else { bid::FARMLAND };
    if terrain::state(game, at) >> 4 != soil || terrain::state(game, (at.0, at.1 + 1, at.2)) >> 4 != 0 {
        return Used::No;
    }
    terrain::set_block(game, (at.0, at.1 + 1, at.2), crop << 4);
    Used::Success { consume: true }
}

/// `ItemDye.onItemUse` with bone meal: the block clicked grows if it can; the dye is used up
/// whenever it could, the growth itself can still fail its roll.
pub fn bone_meal_use(game: &mut Game, _id: u32, m: &UseOn, _stack: ItemStack) -> Used {
    if blockticks::bonemeal(game, (m.pos[0], m.pos[1], m.pos[2])) {
        Used::Success { consume: true }
    } else {
        Used::No
    }
}
