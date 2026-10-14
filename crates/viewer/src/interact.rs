//! The creative player's hands on the world: `Minecraft.clickMouse`, `rightClickMouse`,
//! `middleClickMouse` and `sendClickBlockToController`, the creative half of
//! `PlayerControllerMP`, and the `ItemBlock` placement rules behind them (`World.mayPlace`, the
//! blocks' `canPlaceBlockAt`, `getStateForPlacement`, and the slab, snow and lily pad items).
//!
//! The client guesses each change at once; the populator thread (the server) checks the
//! placements it can and answers with what the world ends up with (`Streamer::edit`).

use bevy::math::DVec3;
use bevy::prelude::*;
use items::{ItemStack, Kind as ItemKind};
use worldgen::blocks::{id, state, WATERLILY};
use worldgen::populate::behavior::{can_sustain_bush, torch_can_place_at, torch_meta, vine_bit, vine_except, TORCH_FACINGS};
use worldgen::populate::props::material;
use worldgen::populate::state as st;
use worldgen::populate::Facing;

use crate::blockstates::{double_slab, half_slab};
use crate::collision::{collision_box, floor, Aabb, BlockAccess};
use crate::hand::Hand;
use crate::hotbar::{block_stack, placed_block, Inventory};
use crate::player::{look, Player};
use crate::populator::Edit;
use crate::region::{props, Mat};
use crate::stream::Streamer;
use crate::target::{mouse_over, offset, ray_trace_blocks, BlockPos, Hit, Kind};

/// The client's world: what it reads, and the changes it guesses.
pub trait World: BlockAccess {
    fn edit(&mut self, edits: Vec<Edit>);
}

impl World for Streamer {
    fn edit(&mut self, edits: Vec<Edit>) {
        Streamer::edit(self, edits)
    }
}

/// `Minecraft.objectMouseOver`, `rightClickDelayTimer`, `PlayerControllerMP.blockHitDelay`, and
/// the use and pick-block presses not yet handled (`KeyBinding.pressTime`). The attack presses
/// are `Hand::clicks`.
#[derive(Resource, Default)]
pub struct Controller {
    pub over: Option<Hit>,
    pub right_click_delay: i32,
    pub block_hit_delay: i32,
    pub uses: u32,
    pub picks: u32,
    /// The sign just placed, for the edit screen (`EntityPlayer.openEditSign`, sign.rs).
    pub sign: Option<BlockPos>,
}

/// `EnumActionResult`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Success,
    Pass,
    Fail,
}

fn get(w: &impl BlockAccess, p: BlockPos) -> u16 {
    w.state(p.0, p.1, p.2)
}

fn up(p: BlockPos) -> BlockPos {
    (p.0, p.1 + 1, p.2)
}

fn down(p: BlockPos) -> BlockPos {
    (p.0, p.1 - 1, p.2)
}

fn edit(at: BlockPos, s: u16, flags: i32, place: bool) -> Edit {
    Edit { at, s, flags, place }
}

fn air(s: u16) -> bool {
    props(s).mat == Mat::Air
}

/// `BlockDoublePlant.getActualState`'s variant: the upper half takes the lower half's.
fn double_plant_variant(w: &impl BlockAccess, s: u16, p: BlockPos) -> u16 {
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
fn is_replaceable(w: &impl BlockAccess, p: BlockPos) -> bool {
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
fn vine_acceptable(w: &impl BlockAccess, p: BlockPos, f: Facing) -> bool {
    let s = get(w, p);
    st::solid_face(s, f) && !vine_except(s >> 4)
}

/// `BlockVine.canAttachTo`
fn vine_can_attach_to(w: &impl BlockAccess, p: BlockPos, side: Facing) -> bool {
    let above = get(w, up(p)) >> 4;
    vine_acceptable(w, offset(p, side.opposite()), side) && (above == id::AIR || above == id::VINE || vine_acceptable(w, up(p), Facing::Up))
}

/// `Block.canPlaceBlockAt` for the blocks the generator places, as the client sees it.
/// DROPPED: `BlockMushroom.canBlockStay`'s light below 13; the client has no light to read, and
/// the populator checks it.
fn can_place_block_at(w: &impl BlockAccess, p: BlockPos, block: u16) -> bool {
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
fn can_place_block_on_side(w: &impl BlockAccess, p: BlockPos, block: u16, side: Facing) -> bool {
    if block == id::VINE {
        side.is_horizontal() && vine_can_attach_to(w, p, side)
    } else {
        can_place_block_at(w, p, block)
    }
}

fn clear_of(b: Aabb, p: BlockPos, player: &Aabb) -> bool {
    !b.offset(p.0 as f64, p.1 as f64, p.2 as f64).intersects(player)
}

/// `World.mayPlace(block, pos, false, side, null)`: the block's default collision box clear of
/// the player, the block there replaceable, and the block's own rule.
/// DROPPED: the anvil over a circuit.
fn may_place(w: &impl BlockAccess, bb: &Aabb, block: u16, p: BlockPos, side: Facing) -> bool {
    if let Some(b) = collision_box(state(block, 0)) {
        if !clear_of(b, p, bb) {
            return false;
        }
    }
    st::mat_replaceable(get(w, p)) && can_place_block_on_side(w, p, block, side)
}

/// `ItemBlock.canPlaceBlockOnSide`, with `ItemSlab`'s: the check the client makes before it
/// sends a placement at all.
fn item_can_place_block_on_side(w: &impl BlockAccess, bb: &Aabb, stack: ItemStack, p: BlockPos, side: Facing) -> bool {
    let (block, meta) = block_meta(&stack);
    if double_slab(block).is_some() {
        let variant = meta & 7;
        let s = get(w, p);
        if s >> 4 == block {
            let top = s & 8 != 0;
            if ((side == Facing::Up && !top) || (side == Facing::Down && top)) && s & 7 == variant {
                return true;
            }
        }
        let s1 = get(w, offset(p, side));
        if s1 >> 4 == block && s1 & 7 == variant {
            return true;
        }
    }
    let (mut p, mut side) = (p, side);
    if get(w, p) >> 4 == id::SNOW_LAYER {
        side = Facing::Up;
    } else if !is_replaceable(w, p) {
        p = offset(p, side);
    }
    may_place(w, bb, block, p, side)
}

/// The block a block item places and the stack's metadata. Only stacks [`placed_block`] knows
/// reach the placement code.
fn block_meta(stack: &ItemStack) -> (u16, u16) {
    placed_block(stack).unwrap_or((id::AIR, 0))
}

/// `Entity.getHorizontalFacing`
fn horizontal_facing(yaw: f32) -> Facing {
    Facing::BY_HORIZONTAL[(floor((yaw * 4.0 / 360.0) as f64 + 0.5) & 3) as usize]
}

/// `ItemBlock.getMetadata` and `Block.getStateForPlacement`, with the facing `onBlockPlacedBy`
/// gives a chest.
/// DROPPED: a chest joining the chest beside it.
fn state_for_placement(stack: ItemStack, facing: Facing, hit_y: f32, yaw: f32) -> u16 {
    let (b, m) = block_meta(&stack);
    let meta = match b {
        // BlockLog.LOG_AXIS from the clicked face's axis: Y 0, X 4, Z 8.
        id::LOG | id::LOG2 => {
            (m & 3)
                | match facing {
                    Facing::Down | Facing::Up => 0,
                    Facing::West | Facing::East => 4,
                    Facing::North | Facing::South => 8,
                }
        }
        id::PUMPKIN => horizontal_facing(yaw).opposite().horizontal_index() as u16,
        // BlockSlab: the top half from below, or from the upper half of a side.
        id::STONE_SLAB | id::WOODEN_SLAB | id::STONE_SLAB2 | id::PURPUR_SLAB => (m & 7) | if facing == Facing::Down || (facing != Facing::Up && hit_y > 0.5) { 8 } else { 0 },
        id::VINE => {
            if facing.is_horizontal() {
                vine_bit(facing.opposite())
            } else {
                0
            }
        }
        id::CHEST => horizontal_facing(yaw).opposite() as u16,
        // The default state: every side the cap.
        id::BROWN_MUSHROOM_BLOCK | id::RED_MUSHROOM_BLOCK => 14,
        // ItemLeaves: placed leaves never decay.
        id::LEAVES | id::LEAVES2 => (m & 3) | 4,
        _ => m,
    };
    state(b, meta)
}

/// `BlockTorch.getStateForPlacement`: the clicked face if the torch can hang there, else the
/// first horizontal facing that can, else standing.
fn torch_for_placement(w: &impl BlockAccess, p: BlockPos, side: Facing) -> u16 {
    let fits = |f: Facing| torch_can_place_at(get(w, offset(p, f.opposite())), f);
    let f = if fits(side) { side } else { Facing::HORIZONTAL.into_iter().find(|&f| fits(f)).unwrap_or(Facing::Up) };
    state(id::TORCH, torch_meta(f))
}

/// `ItemBlock.onItemUse`, and `ItemBlockSpecial`'s for reeds, which comes to the same here.
/// DROPPED: the build height check `NetHandlerPlayServer` makes; the populator ignores what is
/// out of the world.
fn item_block_use(w: &mut impl World, player: &Player, stack: ItemStack, p: BlockPos, side: Facing, hit_y: f32) -> Action {
    let block = block_meta(&stack).0;
    let q = if is_replaceable(w, p) { p } else { offset(p, side) };
    if !may_place(w, &player.bb, block, q, side) {
        return Action::Fail;
    }
    let s = if block == id::TORCH { torch_for_placement(w, q, side) } else { state_for_placement(stack, side, hit_y, player.yaw) };
    let mut edits = vec![edit(q, s, 11, true)];
    // BlockDoublePlant.onBlockPlacedBy: the upper half, facing north.
    if block == id::DOUBLE_PLANT {
        edits.push(edit(up(q), state(id::DOUBLE_PLANT, 10), 2, false));
    }
    w.edit(edits);
    Action::Success
}

/// The `setBlockState(pos, state, flags)` the slab and snow items make when the grown block's
/// collision box is clear of the player.
fn grow(w: &mut impl World, player: &Player, p: BlockPos, s: u16, flags: i32) -> bool {
    let clear = collision_box(s).is_some_and(|b| clear_of(b, p, &player.bb));
    if clear {
        w.edit(vec![edit(p, s, flags, false)]);
    }
    clear
}

/// `ItemStack.onItemUse` for the block items: `ItemSlab` and `ItemSnow` grow the block clicked,
/// everything else is `item_block_use`.
fn on_item_use(w: &mut impl World, player: &Player, stack: ItemStack, p: BlockPos, side: Facing, hit_y: f32) -> Action {
    let (block, meta) = block_meta(&stack);
    if let Some(double) = double_slab(block) {
        let variant = meta & 7;
        let s = get(w, p);
        if s >> 4 == block {
            let top = s & 8 != 0;
            if ((side == Facing::Up && !top) || (side == Facing::Down && top)) && s & 7 == variant {
                grow(w, player, p, state(double, variant), 11);
                return Action::Success;
            }
        }
        // ItemSlab.tryPlace
        let q = offset(p, side);
        let s1 = get(w, q);
        if s1 >> 4 == block && s1 & 7 == variant {
            grow(w, player, q, state(double, variant), 11);
            return Action::Success;
        }
        return item_block_use(w, player, stack, p, side, hit_y);
    }
    match block {
        id::SNOW_LAYER => {
            let mut q = p;
            let mut s = get(w, p);
            if (side != Facing::Up || s >> 4 != id::SNOW_LAYER) && !is_replaceable(w, p) {
                q = offset(p, side);
                s = get(w, q);
            }
            // One layer more, up to eight.
            if s >> 4 == id::SNOW_LAYER && s & 7 < 7 && grow(w, player, q, state(id::SNOW_LAYER, (s & 7) + 1), 10) {
                return Action::Success;
            }
            item_block_use(w, player, stack, p, side, hit_y)
        }
        _ => item_block_use(w, player, stack, p, side, hit_y),
    }
}

/// `ItemSign.onItemUse`: a standing sign facing the player on top of a block, a wall sign on its
/// side, and its edit screen (`openEditSign`), which sign.rs opens from `opened`.
/// DROPPED: `canPlayerEdit`, true for a creative player; `ItemBlock.setTileEntityNBT`, as no
/// stack carries tile data; the placed-block trigger; and `shrink`, as creative keeps the stack.
fn item_sign_use(w: &mut impl World, player: &Player, p: BlockPos, facing: Facing, opened: &mut Option<BlockPos>) -> Action {
    let s = get(w, p);
    let flag = is_replaceable(w, p);
    if facing != Facing::Down && (st::mat_solid(s) || flag) && (!flag || facing == Facing::Up) {
        let q = offset(p, facing);
        if can_place_block_at(w, q, id::STANDING_SIGN) {
            let q = if flag { down(q) } else { q };
            let s = if facing == Facing::Up {
                let i = floor(((player.yaw + 180.0) * 16.0 / 360.0) as f64 + 0.5) & 15;
                state(id::STANDING_SIGN, i as u16)
            } else {
                state(id::WALL_SIGN, facing as u16)
            };
            w.edit(vec![edit(q, s, 11, true)]);
            *opened = Some(q);
            return Action::Success;
        }
    }
    Action::Fail
}

/// `PlayerControllerMP.processRightClickBlock` in creative.
/// DROPPED: every `onBlockActivated` but the chest's, which on the client only answers true (its
/// screen is not ported).
fn process_right_click_block(w: &mut impl World, player: &Player, held: ItemStack, hit: Hit, opened: &mut Option<BlockPos>) -> Action {
    let p = hit.pos;
    let hit_y = (hit.vec.y - p.1 as f64) as f32;
    let activated = (!player.is_sneaking() || held.is_empty()) && get(w, p) >> 4 == id::CHEST;
    if !activated {
        // Reeds are an ItemBlockSpecial, not an ItemBlock, and skip the check.
        if !held.is_empty() && matches!(held.def().kind, ItemKind::Block { .. }) && !item_can_place_block_on_side(w, &player.bb, held, p, hit.face) {
            return Action::Fail;
        }
    }
    if activated {
        return Action::Success;
    }
    if held.is_empty() {
        return Action::Pass;
    }
    match placed_block(&held) {
        Some(_) => on_item_use(w, player, held, p, hit.face, hit_y),
        None if held.item == items::ids::item::SIGN => item_sign_use(w, player, p, hit.face, opened),
        // DROPPED: Item.onItemUse of the other items that place no block.
        None => Action::Pass,
    }
}

/// `ItemLilyPad.onItemRightClick`: `Item.rayTrace` with liquids, and the pad on the still water
/// it finds.
fn use_lily_pad(w: &mut impl World, player: &Player) -> Action {
    let start = player.pos + DVec3::Y * player.eye_height() as f64;
    let l = look(player.pitch, player.yaw).as_dvec3();
    match ray_trace_blocks(w, start, start + DVec3::new(l.x * 5.0, l.y * 5.0, l.z * 5.0), true, false) {
        None => Action::Pass,
        Some(h) if h.kind == Kind::Block => {
            let s = get(w, h.pos);
            if props(s).mat == Mat::Water && s & 15 == 0 && get(w, up(h.pos)) == 0 {
                w.edit(vec![edit(up(h.pos), WATERLILY, 11, false)]);
                Action::Success
            } else {
                Action::Fail
            }
        }
        Some(_) => Action::Fail,
    }
}

/// `PlayerControllerMP.processRightClick`: `Item.onItemRightClick`, which only the lily pad among
/// the block items overrides.
fn process_right_click(w: &mut impl World, player: &Player, stack: ItemStack) -> Action {
    if placed_block(&stack).is_some_and(|(b, _)| b == id::WATERLILY) {
        use_lily_pad(w, player)
    } else {
        Action::Pass
    }
}

/// `Minecraft.rightClickMouse` for the main hand.
/// DROPPED: the off hand.
fn right_click_mouse(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, inv: &Inventory, player: &Player) {
    ctl.right_click_delay = 4;
    let held = inv.get_current_item();
    if let Some(h) = ctl.over.filter(|h| h.kind == Kind::Block) {
        if !air(get(w, h.pos)) && process_right_click_block(w, player, held, h, &mut ctl.sign) == Action::Success {
            hand.swing_arm();
            // ItemRenderer.resetEquippedProgress: always in creative.
            if !held.is_empty() {
                hand.equipped = 0.0;
            }
            return;
        }
    }
    if !held.is_empty() && process_right_click(w, player, held) == Action::Success {
        hand.equipped = 0.0;
    }
}

/// `PlayerControllerMP.onPlayerDestroyBlock` in creative, with `BlockDoublePlant.onBlockHarvested`
/// taking the other half.
/// DROPPED: the sword that cannot break blocks in creative, and the break particles and sound.
fn on_player_destroy_block(w: &mut impl World, p: BlockPos) {
    let s = get(w, p);
    if air(s) {
        return;
    }
    let mut edits = Vec::new();
    if s >> 4 == id::DOUBLE_PLANT {
        if s & 8 != 0 {
            if get(w, down(p)) >> 4 == id::DOUBLE_PLANT {
                edits.push(edit(down(p), 0, 3, false));
            }
        } else if get(w, up(p)) >> 4 == id::DOUBLE_PLANT {
            edits.push(edit(up(p), 0, 2, false));
        }
    }
    edits.push(edit(p, 0, 11, false));
    w.edit(edits);
}

/// `PlayerControllerMP.clickBlockCreative`: the fire on the clicked face goes out first
/// (`World.extinguishFire`), else the block breaks.
fn click_block_creative(w: &mut impl World, p: BlockPos, face: Facing) {
    let f = offset(p, face);
    if get(w, f) >> 4 == id::FIRE {
        w.edit(vec![edit(f, 0, 3, false)]);
    } else {
        on_player_destroy_block(w, p);
    }
}

/// `Minecraft.clickMouse` with `PlayerControllerMP.clickBlock` in creative.
/// DROPPED: the entity hit.
fn click_mouse(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand) {
    if hand.left_click_counter > 0 {
        return;
    }
    let Some(h) = ctl.over else { return };
    if h.kind == Kind::Block && !air(get(w, h.pos)) {
        click_block_creative(w, h.pos, h.face);
        ctl.block_hit_delay = 5;
    } else {
        hand.reset_cooldown();
    }
    hand.swing_arm();
}

/// `Minecraft.sendClickBlockToController` with `PlayerControllerMP.onPlayerDamageBlock` in
/// creative: a held attack breaks a block every sixth tick.
/// DROPPED: the hit particles, and `resetBlockRemoving`, which has no progress to reset in creative.
fn send_click_block_to_controller(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, attack_held: bool) {
    if !attack_held {
        hand.left_click_counter = 0;
    }
    if hand.left_click_counter > 0 || !attack_held {
        return;
    }
    let Some(h) = ctl.over.filter(|h| h.kind == Kind::Block) else { return };
    if air(get(w, h.pos)) {
        return;
    }
    if ctl.block_hit_delay > 0 {
        ctl.block_hit_delay -= 1;
    } else {
        ctl.block_hit_delay = 5;
        click_block_creative(w, h.pos, h.face);
    }
    hand.swing_arm();
}

/// `Block.getItem`: the stack the middle click picks, as a block and metadata.
/// DROPPED: the item-only results (cocoa beans, doors) and the stored tile entity data. The sign's
/// item is `middle_click_mouse`'s.
fn pick(w: &impl BlockAccess, s: u16, p: BlockPos) -> Option<(u16, u16)> {
    let (b, m) = (s >> 4, s & 15);
    Some(match b {
        id::AIR | id::FLOWING_WATER | id::WATER | id::FLOWING_LAVA | id::LAVA | id::FIRE | id::MOB_SPAWNER | id::PORTAL | id::FROSTED_ICE => {
            return None
        }
        id::COCOA | id::WOODEN_DOOR | id::IRON_DOOR | id::SPRUCE_DOOR | id::DARK_OAK_DOOR | id::STANDING_SIGN | id::WALL_SIGN | id::PISTON_HEAD => return None,
        // damageDropped: the variant.
        id::STONE
        | id::DIRT
        | id::PLANKS
        | id::SAND
        | id::SANDSTONE
        | id::RED_SANDSTONE
        | id::TALLGRASS
        | id::RED_FLOWER
        | id::STONEBRICK
        | id::STAINED_GLASS
        | id::STAINED_HARDENED_CLAY
        | id::MONSTER_EGG => (b, m),
        id::SAPLING => (b, m & 7),
        id::LOG | id::LOG2 | id::LEAVES | id::LEAVES2 => (b, m & 3),
        // A double slab picks its half.
        id::STONE_SLAB | id::WOODEN_SLAB | id::STONE_SLAB2 | id::PURPUR_SLAB => (b, m & 7),
        _ if half_slab(b).is_some() => (half_slab(b).unwrap(), m & 7),
        id::DOUBLE_PLANT => (b, double_plant_variant(w, s, p)),
        // Lit redstone ore.
        74 => (id::REDSTONE_ORE, 0),
        id::BROWN_MUSHROOM_BLOCK => (id::BROWN_MUSHROOM, 0),
        id::RED_MUSHROOM_BLOCK => (id::RED_MUSHROOM, 0),
        id::FARMLAND => (id::DIRT, 0),
        _ => (b, 0),
    })
}

/// [`pick`] with nothing around the block, for the hotbar's block keys (`item::block_keys`). Wrong
/// for a double plant's upper half, which reads the half below.
#[cfg(test)]
pub fn picked_item(s: u16) -> Option<(u16, u16)> {
    struct NoWorld;
    impl BlockAccess for NoWorld {
        fn state(&self, _: i32, _: i32, _: i32) -> u16 {
            0
        }
        fn loaded(&self, _: i32, _: i32) -> bool {
            true
        }
    }
    pick(&NoWorld, s, (0, 64, 0))
}

/// `Minecraft.middleClickMouse` in creative: the pointed block into the hotbar.
fn middle_click_mouse(w: &impl BlockAccess, ctl: &Controller, inv: &mut Inventory) {
    let Some(h) = ctl.over.filter(|h| h.kind == Kind::Block) else { return };
    let s = get(w, h.pos);
    if air(s) {
        return;
    }
    // BlockSign.getItem: the sign item.
    let stack = if matches!(s >> 4, id::STANDING_SIGN | id::WALL_SIGN) {
        ItemStack::of(items::ids::item::SIGN)
    } else {
        let Some((b, m)) = pick(w, s, h.pos) else { return };
        block_stack(b, m)
    };
    if stack.is_empty() {
        return;
    }
    inv.set_picked_item_stack(stack);
    // DROPPED: playerController.sendSlotPacket; no server keeps the creative hotbar.
}

/// One tick of `Minecraft.runTick` up to the world update: the pointed block
/// (`getMouseOver(1.0F)`), the wheel (`runTickMouse`), the hotbar keys and the queued presses
/// (`processKeyBinds`), the held attack, and the hand's equip progress.
#[allow(clippy::too_many_arguments)]
pub fn tick(w: &mut impl World, ctl: &mut Controller, hand: &mut Hand, inv: &mut Inventory, player: &Player, attack_held: bool, use_held: bool) {
    if ctl.right_click_delay > 0 {
        ctl.right_click_delay -= 1;
    }
    ctl.over = mouse_over(w, player, 1.0);
    // One InventoryPlayer.changeCurrentItem per wheel event.
    while inv.scroll != 0 {
        let d = inv.scroll.signum();
        inv.change_current_item(d);
        inv.scroll -= d;
    }
    hand.begin_tick();
    // KeyBinding.isPressed takes one press per tick.
    for i in 0..9 {
        if inv.digits[i] > 0 {
            inv.digits[i] -= 1;
            inv.current_item = i;
        }
    }
    while hand.clicks > 0 {
        hand.clicks -= 1;
        click_mouse(w, ctl, hand);
    }
    while ctl.uses > 0 {
        ctl.uses -= 1;
        right_click_mouse(w, ctl, hand, inv, player);
    }
    while ctl.picks > 0 {
        ctl.picks -= 1;
        middle_click_mouse(w, ctl, inv);
    }
    // Held use repeats every fourth tick.
    if use_held && ctl.right_click_delay == 0 {
        right_click_mouse(w, ctl, hand, inv, player);
    }
    send_click_block_to_controller(w, ctl, hand, attack_held);
    hand.update_equipped(inv.get_current_item(), inv.current_item);
}

#[cfg(test)]
mod tests;
