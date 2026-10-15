//! Worn armor over the slab world of use_on_tests.rs: the damage it absorbs, the wear it takes,
//! the right click that equips a piece and the equipment the other players are sent.

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND};

use crate::game::damage::{attack_entity_from, DamageSource};
use crate::game::terrain;
use crate::game::testing::{run, Harness};

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.join(2, "Ab", [6.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), block::STONE << 4);
        }
    }
    for id in [1, 2] {
        h.game.sessions.get_mut(&id).unwrap().state = Some(PlayerState { pos: [5.5 + id as f64, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
        h.game.players.players.get_mut(&id).unwrap().respawn_invulnerable = 0;
    }
    h.tick(1);
    h.frames_for(1);
    h.frames_for(2);
    h
}

fn wear(h: &mut Harness, pieces: [u16; 4]) {
    let p = h.game.players.players.get_mut(&1).unwrap();
    for (i, it) in pieces.into_iter().enumerate() {
        p.inv.armor[i] = ItemStack::of(it);
    }
}

const IRON: [u16; 4] = [item::IRON_BOOTS, item::IRON_LEGGINGS, item::IRON_CHESTPLATE, item::IRON_HELMET];
const DIAMOND: [u16; 4] = [item::DIAMOND_BOOTS, item::DIAMOND_LEGGINGS, item::DIAMOND_CHESTPLATE, item::DIAMOND_HELMET];

fn health(h: &Harness) -> f32 {
    h.game.players.players[&1].health
}

#[test]
fn full_iron_armor_cuts_a_ten_damage_hit_to_six() {
    run(|| {
        let mut h = setup();
        wear(&mut h, IRON);
        // 15 armor points: f1 = clamp(15 - 10 / 2, 3, 20) = 10, 10 * (1 - 10 / 25) = 6.
        assert!(attack_entity_from(&mut h.game, 1, &DamageSource::cactus(), 10.0));
        assert_eq!(health(&h), 14.0);
    });
}

#[test]
fn diamond_toughness_cuts_deeper() {
    run(|| {
        let mut h = setup();
        wear(&mut h, DIAMOND);
        // 20 armor, 8 toughness (2 each): f = 4, f1 = clamp(20 - 2.5, 4, 20) = 17.5, 10 * 0.3 = 3.
        assert!(attack_entity_from(&mut h.game, 1, &DamageSource::cactus(), 10.0));
        assert!((health(&h) - 17.0).abs() < 1e-4, "{}", health(&h));
    });
}

#[test]
fn a_hit_that_bypasses_armor_takes_all_and_wears_nothing() {
    run(|| {
        let mut h = setup();
        wear(&mut h, IRON);
        assert!(attack_entity_from(&mut h.game, 1, &DamageSource::fall(), 10.0));
        assert_eq!(health(&h), 10.0);
        assert!(h.game.players.players[&1].inv.armor.iter().all(|s| s.damage == 0));
    });
}

#[test]
fn a_hit_takes_armor_durability_a_quarter_of_the_damage() {
    run(|| {
        let mut h = setup();
        wear(&mut h, IRON);
        attack_entity_from(&mut h.game, 1, &DamageSource::cactus(), 10.0);
        // 10 / 4 = 2.5, truncated to 2 on each of the four pieces.
        let armor = h.game.players.players[&1].inv.armor;
        assert!(armor.iter().all(|s| s.damage == 2), "{:?}", armor.map(|s| s.damage));
    });
}

#[test]
fn a_small_hit_still_wears_one_point_and_a_worn_out_piece_breaks() {
    run(|| {
        let mut h = setup();
        wear(&mut h, IRON);
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            // iron boots last 13 * 15 - 1 = 194 here (max damage 195 breaks at damage 196)
            p.inv.armor[0].damage = p.inv.armor[0].get_max_damage();
        }
        attack_entity_from(&mut h.game, 1, &DamageSource::cactus(), 1.0);
        let armor = h.game.players.players[&1].inv.armor;
        assert!(armor[0].is_empty(), "the boots wore out");
        assert_eq!(armor[2].damage, 1);
    });
}

fn use_item(h: &mut Harness) {
    h.push_frame(1, proto::V_USE_ITEM, &proto::UseItem { hand: proto::HAND_MAIN, phase: proto::USE_START }.encode());
    h.tick(1);
}

#[test]
fn right_click_with_a_chestplate_wears_it() {
    run(|| {
        let mut h = setup();
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            let c = p.inv.current_item;
            p.inv.main[c] = ItemStack::of(item::IRON_CHESTPLATE);
        }
        use_item(&mut h);
        let p = &h.game.players.players[&1];
        assert_eq!(p.inv.armor[2].get_item(), item::IRON_CHESTPLATE);
        assert!(p.inv.main[p.inv.current_item].is_empty());
        // A second chestplate stays in hand: the slot is taken.
        let c = p.inv.current_item;
        h.game.players.players.get_mut(&1).unwrap().inv.main[c] = ItemStack::of(item::DIAMOND_CHESTPLATE);
        use_item(&mut h);
        let p = &h.game.players.players[&1];
        assert_eq!(p.inv.armor[2].get_item(), item::IRON_CHESTPLATE);
        assert_eq!(p.inv.main[p.inv.current_item].get_item(), item::DIAMOND_CHESTPLATE);
    });
}

#[test]
fn other_players_are_sent_the_worn_piece() {
    run(|| {
        let mut h = setup();
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            let c = p.inv.current_item;
            p.inv.main[c] = ItemStack::of(item::IRON_HELMET);
        }
        use_item(&mut h);
        h.tick(1);
        let eq: Vec<proto::Equipment> = h.frames_for(2).iter().filter(|f| f.0 == proto::S_EQUIPMENT).filter_map(|f| proto::Equipment::decode(&f.1)).collect();
        assert!(eq.iter().any(|e| e.id == 1 && e.slot == proto::EQ_HEAD && e.stack.id == item::IRON_HELMET as i16), "{eq:?}");
        assert!(h.frames_for(1).iter().all(|f| f.0 != proto::S_EQUIPMENT), "not to oneself");
    });
}
