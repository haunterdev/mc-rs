use super::*;
use crate::collision::tests::Blocks;
use worldgen::blocks::{STONE, WATER};

impl World for Blocks {
    fn edit(&mut self, edits: Vec<Edit>) {
        for e in edits {
            self.set(e.at.0, e.at.1, e.at.2, e.s);
        }
    }
}

struct Rig {
    w: Blocks,
    ctl: Controller,
    hand: Hand,
    inv: Inventory,
    player: Player,
}

impl Rig {
    fn new() -> Rig {
        let w = Blocks::floor_at(63, 8);
        let player = Player::new(DVec3::new(0.5, 64.0, 0.5), 0.0, 0.0);
        Rig { w, ctl: Controller::default(), hand: Hand::default(), inv: Inventory::default(), player }
    }
    fn hold(&mut self, block: u16, meta: u16) {
        let c = self.inv.current_item;
        self.inv.main[c] = block_stack(block, meta);
    }
    /// A right click with the crosshair on `pos`'s `face` at `vec`.
    fn use_on(&mut self, pos: BlockPos, face: Facing, vec: (f64, f64, f64)) {
        self.ctl.over = Some(Hit { kind: Kind::Block, vec: DVec3::new(vec.0, vec.1, vec.2), face, pos });
        right_click_mouse(&mut self.w, &mut self.ctl, &mut self.hand, &self.inv, &self.player);
    }
    fn at(&self, x: i32, y: i32, z: i32) -> u16 {
        self.w.state(x, y, z)
    }
}

#[test]
fn placing_on_a_face_and_not_into_the_player() {
    let mut r = Rig::new();
    r.use_on((2, 63, 0), Facing::Up, (2.5, 64.0, 0.5));
    assert_eq!(r.at(2, 64, 0), STONE);
    assert!(r.hand.swinging);
    // Into the player's own cell: refused before anything is sent, and no swing.
    let mut r = Rig::new();
    r.use_on((0, 63, 0), Facing::Up, (0.5, 64.0, 0.5));
    assert_eq!(r.at(0, 64, 0), 0);
    assert!(!r.hand.swinging);
    // Logs lie along the clicked face's axis.
    r.w.set(3, 64, 0, STONE);
    r.hold(id::LOG, 1);
    r.use_on((3, 64, 0), Facing::East, (4.0, 64.5, 0.5));
    assert_eq!(r.at(4, 64, 0), state(id::LOG, 1 | 4));
    // A vine hangs on the face it was put against.
    r.hold(id::VINE, 0);
    r.use_on((3, 64, 0), Facing::West, (3.0, 64.5, 0.5));
    assert_eq!(r.at(2, 64, 0), state(id::VINE, 8));
    // A flower needs soil.
    r.hold(id::RED_FLOWER, 3);
    r.use_on((5, 63, 0), Facing::Up, (5.5, 64.0, 0.5));
    assert_eq!(r.at(5, 64, 0), 0);
    r.w.set(5, 63, 0, state(id::GRASS, 0));
    r.use_on((5, 63, 0), Facing::Up, (5.5, 64.0, 0.5));
    assert_eq!(r.at(5, 64, 0), state(id::RED_FLOWER, 3));
    // Placing over the flower replaces nothing: it goes on top.
    r.hold(id::DIRT, 0);
    r.use_on((5, 64, 0), Facing::Up, (5.5, 64.6, 0.5));
    assert_eq!((r.at(5, 64, 0), r.at(5, 65, 0)), (state(id::RED_FLOWER, 3), state(id::DIRT, 0)));
}

#[test]
fn torches_stand_or_hang_by_the_clicked_face() {
    let mut r = Rig::new();
    r.hold(id::TORCH, 0);
    r.use_on((2, 63, 0), Facing::Up, (2.5, 64.0, 0.5));
    assert_eq!(r.at(2, 64, 0), state(id::TORCH, 5));
    // On a wall: meta 1 faces east.
    r.w.set(3, 65, 0, STONE);
    r.use_on((3, 65, 0), Facing::East, (4.0, 65.5, 0.5));
    assert_eq!(r.at(4, 65, 0), state(id::TORCH, 1));
    // Glass takes a standing torch on top but none on its side, so a click on the side
    // stands it on the floor instead.
    r.w.set(6, 64, 0, state(id::GLASS, 0));
    r.use_on((6, 64, 0), Facing::Up, (6.5, 65.0, 0.5));
    assert_eq!(r.at(6, 65, 0), state(id::TORCH, 5));
    r.use_on((6, 64, 0), Facing::East, (7.0, 64.5, 0.5));
    assert_eq!(r.at(7, 64, 0), state(id::TORCH, 5));
    // Nothing to stand on or hang from: refused.
    r.w.set(-3, 66, 0, STONE);
    r.use_on((-3, 66, 0), Facing::Up, (-2.5, 67.0, 0.5));
    assert_eq!(r.at(-3, 67, 0), state(id::TORCH, 5));
    r.use_on((-3, 67, 0), Facing::Up, (-2.5, 67.6, 0.5));
    assert_eq!(r.at(-3, 68, 0), 0);
}

#[test]
fn slabs_join_and_take_the_clicked_half() {
    let mut r = Rig::new();
    r.hold(id::STONE_SLAB, 0);
    r.use_on((2, 63, 0), Facing::Up, (2.5, 64.0, 0.5));
    assert_eq!(r.at(2, 64, 0), state(id::STONE_SLAB, 0));
    r.use_on((2, 64, 0), Facing::Up, (2.5, 64.5, 0.5));
    assert_eq!(r.at(2, 64, 0), state(43, 0));
    // The upper half of a side face makes a top slab.
    r.w.set(4, 65, 0, STONE);
    r.use_on((4, 65, 0), Facing::West, (4.0, 65.75, 0.5));
    assert_eq!(r.at(3, 65, 0), state(id::STONE_SLAB, 8));
    // Another variant does not join.
    r.hold(id::STONE_SLAB, 1);
    r.use_on((3, 65, 0), Facing::Down, (3.5, 65.5, 0.5));
    assert_eq!((r.at(3, 65, 0), r.at(3, 64, 0)), (state(id::STONE_SLAB, 8), state(id::STONE_SLAB, 9)));
    // Wooden slabs join into the wooden double slab.
    r.hold(id::WOODEN_SLAB, 2);
    r.use_on((6, 63, 0), Facing::Up, (6.5, 64.0, 0.5));
    assert_eq!(r.at(6, 64, 0), state(id::WOODEN_SLAB, 2));
    r.use_on((6, 64, 0), Facing::Up, (6.5, 64.5, 0.5));
    assert_eq!(r.at(6, 64, 0), state(id::DOUBLE_WOODEN_SLAB, 2));
}

#[test]
fn snow_stacks_and_a_single_layer_is_replaced() {
    let mut r = Rig::new();
    r.hold(id::SNOW_LAYER, 0);
    r.use_on((2, 63, 0), Facing::Up, (2.5, 64.0, 0.5));
    assert_eq!(r.at(2, 64, 0), state(id::SNOW_LAYER, 0));
    r.use_on((2, 64, 0), Facing::Up, (2.5, 64.125, 0.5));
    assert_eq!(r.at(2, 64, 0), state(id::SNOW_LAYER, 1));
    r.w.set(5, 64, 0, state(id::SNOW_LAYER, 0));
    r.hold(id::STONE, 0);
    r.use_on((5, 64, 0), Facing::Up, (5.5, 64.125, 0.5));
    assert_eq!(r.at(5, 64, 0), STONE);
}

#[test]
fn a_lily_pad_goes_on_the_water_the_eye_sees() {
    let mut r = Rig::new();
    r.w = Blocks::floor_at(61, 4);
    for x in -4..=4 {
        for z in -4..=4 {
            r.w.set(x, 62, z, WATER);
            r.w.set(x, 63, z, WATER);
        }
    }
    r.player = Player::new(DVec3::new(0.5, 66.0, 0.5), 0.0, 90.0);
    r.hold(id::WATERLILY, 0);
    r.ctl.uses = 1;
    tick(&mut r.w, &mut r.ctl, &mut r.hand, &mut r.inv, &r.player, false, false);
    assert_eq!(r.at(0, 64, 0), WATERLILY);
    assert_eq!(r.hand.equipped, 0.0);
}

#[test]
fn breaking_takes_both_halves_and_a_held_attack_repeats() {
    let mut r = Rig::new();
    r.w.set(1, 64, 0, state(id::DOUBLE_PLANT, 2));
    r.w.set(1, 65, 0, state(id::DOUBLE_PLANT, 10));
    r.ctl.over = Some(Hit { kind: Kind::Block, vec: DVec3::new(1.5, 65.5, 0.0), face: Facing::North, pos: (1, 65, 0) });
    click_mouse(&mut r.w, &mut r.ctl, &mut r.hand);
    assert_eq!((r.at(1, 64, 0), r.at(1, 65, 0)), (0, 0));
    assert_eq!(r.ctl.block_hit_delay, 5);
    // Looking straight down with the attack held: the block under the feet goes on the
    // first tick and the next one six ticks later.
    r.player = Player::new(DVec3::new(0.5, 64.0, 0.5), 0.0, 90.0);
    r.ctl.block_hit_delay = 0;
    let mut broken = Vec::new();
    for t in 0..8 {
        tick(&mut r.w, &mut r.ctl, &mut r.hand, &mut r.inv, &r.player, true, false);
        if r.at(0, 63, 0) == 0 {
            broken.push(t);
            r.w.set(0, 63, 0, STONE);
        }
    }
    assert_eq!(broken, vec![0, 6]);
}

#[test]
fn picking_keeps_the_variant() {
    let mut w = Blocks::default();
    w.set(0, 64, 0, state(id::DOUBLE_PLANT, 3));
    w.set(0, 65, 0, state(id::DOUBLE_PLANT, 10));
    assert_eq!(pick(&w, state(id::DOUBLE_PLANT, 10), (0, 65, 0)), Some((id::DOUBLE_PLANT, 3)));
    assert_eq!(pick(&w, state(id::LEAVES, 5), (0, 0, 0)), Some((id::LEAVES, 1)));
    assert_eq!(pick(&w, state(id::BROWN_MUSHROOM_BLOCK, 14), (0, 0, 0)), Some((id::BROWN_MUSHROOM, 0)));
    assert_eq!(pick(&w, state(43, 8), (0, 0, 0)), Some((id::STONE_SLAB, 0)));
    assert_eq!(pick(&w, WATER, (0, 0, 0)), None);
}

#[test]
fn a_picked_block_not_in_the_hotbar_takes_the_current_or_the_best_slot() {
    let mut r = Rig::new();
    r.ctl.over = Some(Hit { kind: Kind::Block, vec: DVec3::new(2.5, 64.5, 0.0), face: Facing::North, pos: (2, 64, 0) });
    // Each middle click does what InventoryPlayer.setPickedItemStack does with the picked stack.
    let click = |r: &mut Rig, block: u16| {
        r.w.set(2, 64, 0, state(block, 0));
        let mut want = r.inv.player.clone();
        want.set_picked_item_stack(block_stack(block, 0));
        middle_click_mouse(&r.w, &r.ctl, &mut r.inv);
        assert_eq!((r.inv.current_item, r.inv.main), (want.current_item, want.main));
    };
    // The current slot is empty: the block goes there.
    r.inv.current_item = 3;
    r.inv.main[3] = ItemStack::EMPTY;
    r.inv.main[1] = ItemStack::EMPTY;
    click(&mut r, id::SAND);
    assert_eq!(r.inv.current_item, 3);
    assert_eq!(r.inv.main[3], block_stack(id::SAND, 0));
    // The current slot is full: the first empty hotbar slot from it on, wrapping.
    click(&mut r, id::GRAVEL);
    assert_eq!(r.inv.current_item, 1);
    assert_eq!(r.inv.main[1], block_stack(id::GRAVEL, 0));
    assert_eq!(r.inv.main[3], block_stack(id::SAND, 0));
    // A full hotbar: the current slot, its stack moved to the first empty main slot.
    r.inv.current_item = 5;
    let planks = r.inv.main[5];
    click(&mut r, id::CLAY);
    assert_eq!(r.inv.current_item, 5);
    assert_eq!(r.inv.main[5], block_stack(id::CLAY, 0));
    assert_eq!(r.inv.main[9], planks);
}

#[test]
fn a_sign_stands_on_top_hangs_on_a_side_and_opens_its_screen() {
    let mut r = Rig::new();
    let c = r.inv.current_item;
    r.inv.main[c] = ItemStack::of(items::ids::item::SIGN);
    // On top: a standing sign turned to face the player (yaw 0 looks south, so rotation 8).
    r.use_on((2, 63, 0), Facing::Up, (2.5, 64.0, 0.5));
    assert_eq!(r.at(2, 64, 0), state(id::STANDING_SIGN, 8));
    assert_eq!(r.ctl.sign, Some((2, 64, 0)));
    // On a side: a wall sign facing out of that side.
    r.w.set(4, 64, 0, STONE);
    r.use_on((4, 64, 0), Facing::North, (4.5, 64.5, 0.0));
    assert_eq!(r.at(4, 64, 0 - 1), state(id::WALL_SIGN, 2));
    // Never on the bottom of a block, and not beside a cactus.
    r.ctl.sign = None;
    r.use_on((4, 64, 0), Facing::Down, (4.5, 64.0, 0.5));
    assert_eq!(r.at(4, 63, 0), STONE);
    r.w.set(7, 64, 1, state(id::CACTUS, 0));
    r.use_on((7, 63, 0), Facing::Up, (7.5, 64.0, 0.5));
    assert_eq!((r.at(7, 64, 0), r.ctl.sign), (0, None));
    // The middle click picks the sign item.
    r.inv.main[c] = ItemStack::EMPTY;
    r.ctl.over = Some(Hit { kind: Kind::Block, vec: DVec3::new(2.5, 64.5, 0.5), face: Facing::Up, pos: (2, 64, 0) });
    middle_click_mouse(&r.w, &r.ctl, &mut r.inv);
    assert_eq!(r.inv.get_current_item().item, items::ids::item::SIGN);
}
