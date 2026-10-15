//! `V_USE_ON` through use_on.rs over a seeded world: a stone slab at y 200 under x, z 4..=13 and
//! the player at (5.5, 201, 5.5).

use ::items::ids::{block, item};
use ::items::{ItemStack, Tag};
use proto::{PlayerState, ON_GROUND};

use crate::game::testing::{run, Harness};
use crate::game::{terrain, GameMsg};

const STONE: u16 = block::STONE << 4;
const SAND: u16 = block::SAND << 4;

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.tick(1);
    h.frames_for(1);
    h
}

fn give(h: &mut Harness, s: ItemStack) {
    let p = h.game.players.players.get_mut(&1).unwrap();
    let c = p.inv.current_item;
    p.inv.main[c] = s;
}

fn held(h: &Harness) -> ItemStack {
    let p = &h.game.players.players[&1];
    p.inv.main[p.inv.current_item]
}

fn click(h: &mut Harness, pos: [i32; 3], face: u8, state: u16) {
    h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos, face, hit: [0.5; 3], hand: 0, state }.encode());
    h.tick(1);
}

#[test]
fn a_survival_player_places_sand_and_uses_one() {
    run(|| {
        let mut h = setup();
        give(&mut h, ItemStack::new(block::SAND, 3, 0));
        click(&mut h, [8, 200, 8], 1, SAND);
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), SAND);
        assert_eq!(held(&h).count, 2);
        let frames = h.frames_for(1);
        assert!(frames.iter().any(|f| f.0 == proto::S_BLOCKS), "the clicked cells go back to the viewer");
    });
}

#[test]
fn a_creative_player_keeps_the_stack() {
    run(|| {
        let mut h = setup();
        h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
        give(&mut h, ItemStack::new(block::SAND, 3, 0));
        click(&mut h, [8, 200, 8], 1, SAND);
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), SAND);
        assert_eq!(held(&h).count, 3);
    });
}

#[test]
fn placing_into_the_players_own_body_is_refused() {
    run(|| {
        let mut h = setup();
        give(&mut h, ItemStack::new(block::SAND, 3, 0));
        click(&mut h, [5, 200, 5], 1, SAND);
        assert_eq!(terrain::state(&h.game, (5, 201, 5)), 0);
        assert_eq!(held(&h).count, 3);
    });
}

#[test]
fn a_click_that_placed_nothing_on_the_viewer_places_nothing() {
    run(|| {
        let mut h = setup();
        give(&mut h, ItemStack::new(block::SAND, 3, 0));
        click(&mut h, [8, 200, 8], 1, 0);
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), 0);
        assert_eq!(held(&h).count, 3);
    });
}

#[test]
fn a_placement_out_of_reach_is_refused() {
    run(|| {
        let mut h = setup();
        give(&mut h, ItemStack::new(block::SAND, 3, 0));
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5 - 20.0], ..Default::default() });
        click(&mut h, [8, 200, 8], 1, SAND);
        assert_eq!(terrain::state(&h.game, (8, 201, 8)), 0);
        assert_eq!(held(&h).count, 3);
    });
}

#[test]
fn a_zombie_egg_on_grass_spawns_a_zombie_and_is_used_up() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (8, 200, 8), block::GRASS << 4);
        let i = ::items::creative_table::EGGS.iter().position(|e| e.entity == "minecraft:zombie").unwrap();
        give(&mut h, ItemStack::of(item::SPAWN_EGG).with_tag(Tag::Entity(i as u8)));
        let before = h.game.mobs.mobs.len();
        click(&mut h, [8, 200, 8], 1, 0);
        assert_eq!(h.game.mobs.mobs.len(), before + 1);
        let m = h.game.mobs.mobs.last().unwrap();
        assert_eq!(m.def.kind.name, "zombie");
        assert_eq!((m.body.pos.x, m.body.pos.y, m.body.pos.z), (8.5, 201.0, 8.5));
        assert!(held(&h).is_empty());
    });
}

#[test]
fn a_chest_still_opens_through_the_module() {
    run(|| {
        let mut h = setup();
        terrain::set_block(&mut h.game, (7, 201, 6), block::CHEST << 4 | 2);
        h.tick(1);
        h.frames_for(1);
        click(&mut h, [7, 201, 6], 1, 0);
        let frames = h.frames_for(1);
        let open = frames.iter().find(|f| f.0 == proto::S_OPEN_WINDOW).expect("the chest opens");
        assert_eq!(proto::OpenWindow::decode(&open.1).unwrap().kind, proto::WK_CHEST);
    });
}
