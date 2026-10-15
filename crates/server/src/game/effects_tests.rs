//! Potion effects on a player: poison and regeneration timing, absorption, resistance, fire
//! resistance, health boost, the packets and the save file.

use ::items::ids::{block, item};
use ::items::potion::{Effect, PotionEffect};
use ::items::ItemStack;
use proto::{PlayerState, ON_GROUND};
use sim::kinds::COW;

use super::damage::{self, DamageSource};
use super::effects;
use super::mobs::summon;
use super::terrain;
use super::testing::{run, Harness};
use super::GameMsg;

fn setup() -> Harness {
    let mut h = Harness::new(11, 2);
    {
        let mut w = h.world.lock().unwrap();
        w.set_rule("doMobSpawning", "false");
        w.set_rule("doDaylightCycle", "false");
        w.set_rule("naturalRegeneration", "false");
    }
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
    h.tick(1);
    h.frames_for(1);
    h
}

fn health(h: &Harness) -> f32 {
    h.game.players.players[&1].health
}

fn give(h: &mut Harness, e: Effect, ticks: i32, amp: i32) {
    effects::add(&mut h.game, 1, PotionEffect::new(e, ticks, amp));
}

#[test]
fn poison_stops_at_one_health() {
    run(|| {
        let mut h = setup();
        give(&mut h, Effect::Poison, 2000, 0);
        h.tick(1);
        assert_eq!(health(&h), 19.0);
        h.tick(1000);
        assert_eq!(health(&h), 1.0);
        assert!(!h.game.players.players[&1].dead);
    });
}

#[test]
fn regeneration_two_heals_one_every_25_ticks() {
    run(|| {
        let mut h = setup();
        {
            let p = h.game.players.players.get_mut(&1).unwrap();
            p.set_health(10.0);
            p.food.level = 6;
            p.food.saturation = 0.0;
        }
        give(&mut h, Effect::Regeneration, 1000, 1);
        h.tick(1);
        assert_eq!(health(&h), 11.0);
        h.tick(24);
        assert_eq!(health(&h), 11.0);
        h.tick(1);
        assert_eq!(health(&h), 12.0);
    });
}

#[test]
fn absorption_takes_the_hit_and_goes_with_the_effect() {
    run(|| {
        let mut h = setup();
        give(&mut h, Effect::Absorption, 2400, 0);
        assert_eq!(h.game.players.players[&1].absorption, 4.0);
        damage::attack_entity_from(&mut h.game, 1, &DamageSource::generic(), 3.0);
        assert_eq!(health(&h), 20.0);
        assert_eq!(h.game.players.players[&1].absorption, 1.0);
        effects::remove(&mut h.game, 1, Effect::Absorption);
        assert_eq!(h.game.players.players[&1].absorption, 0.0);
    });
}

#[test]
fn resistance_and_fire_resistance_cut_damage() {
    run(|| {
        let mut h = setup();
        give(&mut h, Effect::Resistance, 2400, 1);
        damage::attack_entity_from(&mut h.game, 1, &DamageSource::generic(), 10.0);
        // Resistance II: 10 * (25 - 10) / 25 = 6.
        assert_eq!(health(&h), 14.0);
        give(&mut h, Effect::FireResistance, 2400, 0);
        h.tick(30);
        assert!(!damage::attack_entity_from(&mut h.game, 1, &DamageSource::lava(), 4.0));
        assert_eq!(health(&h), 14.0);
    });
}

#[test]
fn health_boost_raises_the_maximum() {
    run(|| {
        let mut h = setup();
        give(&mut h, Effect::HealthBoost, 2400, 0);
        assert_eq!(h.game.players.players[&1].max_health, 24.0);
        super::players::heal(&mut h.game, 1, 10.0);
        assert_eq!(health(&h), 24.0);
        effects::remove(&mut h.game, 1, Effect::HealthBoost);
        assert_eq!(health(&h), 20.0);
    });
}

#[test]
fn instant_damage_and_instant_health() {
    run(|| {
        let mut h = setup();
        give(&mut h, Effect::InstantDamage, 1, 0);
        h.tick(1);
        assert_eq!(health(&h), 14.0);
        give(&mut h, Effect::InstantHealth, 1, 0);
        h.tick(1);
        assert_eq!(health(&h), 18.0);
    });
}

#[test]
fn effects_reach_the_client_and_expire() {
    run(|| {
        let mut h = setup();
        give(&mut h, Effect::Speed, 5, 1);
        let f = h.frames_for(1);
        let got: Vec<proto::EntityEffect> = f.iter().filter(|(t, _)| *t == proto::S_EFFECT).filter_map(|(_, b)| proto::EntityEffect::decode(b)).collect();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].effect, got[0].amplifier, got[0].duration), (1, 1, 5));
        h.tick(6);
        let f = h.frames_for(1);
        assert!(f.iter().any(|(t, _)| *t == proto::S_REMOVE_EFFECT));
        assert!(h.game.players.players[&1].effects.is_empty());
    });
}

#[test]
fn effects_survive_save_and_load() {
    run(|| {
        let mut h = setup();
        give(&mut h, Effect::Speed, 3000, 2);
        give(&mut h, Effect::Absorption, 3000, 0);
        let v = super::playerdata::to_json(&h.game.players.players[&1]);
        let mut p = super::players::Survival::new();
        assert!(super::playerdata::from_json(&mut p, &v));
        assert_eq!(p.effects.amplifier(Effect::Speed), Some(2));
        assert_eq!(p.effects.get(Effect::Speed).unwrap().duration, 3000);
        assert_eq!(p.absorption, 4.0);
    });
}

/// A stone floor under the player at (5.5, 201, 5.5).
fn floor(h: &mut Harness) {
    for x in 4..=13 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), block::STONE << 4);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.tick(1);
    h.frames_for(1);
}

fn hold(h: &mut Harness, s: ItemStack) {
    h.game.players.players.get_mut(&1).unwrap().inv.main[0] = s;
}

fn eat(h: &mut Harness) {
    h.push_frame(1, proto::V_USE_ITEM, &proto::UseItem { hand: proto::HAND_MAIN, phase: proto::USE_START }.encode());
    h.tick(33);
}

#[test]
fn a_golden_apple_gives_regeneration_two_and_absorption() {
    run(|| {
        let mut h = setup();
        floor(&mut h);
        hold(&mut h, ItemStack::of(item::GOLDEN_APPLE));
        eat(&mut h);
        let p = &h.game.players.players[&1];
        let r = p.effects.get(Effect::Regeneration).expect("regeneration");
        assert_eq!(r.amplifier, 1);
        assert!(r.duration > 60 && r.duration <= 100, "5 seconds: {}", r.duration);
        let a = p.effects.get(Effect::Absorption).expect("absorption");
        assert!(a.duration > 2300 && a.duration <= 2400, "2 minutes: {}", a.duration);
        assert_eq!(p.absorption, 4.0);
    });
}

#[test]
fn a_poisonous_potato_can_poison_and_milk_clears_everything() {
    run(|| {
        let mut h = setup();
        floor(&mut h);
        give(&mut h, Effect::Slowness, 3000, 0);
        give(&mut h, Effect::Absorption, 3000, 1);
        assert_eq!(h.game.players.players[&1].absorption, 8.0);
        hold(&mut h, ItemStack::of(item::MILK_BUCKET));
        eat(&mut h);
        let p = &h.game.players.players[&1];
        assert!(p.effects.is_empty());
        assert_eq!(p.absorption, 0.0);
    });
}

#[test]
fn strength_one_adds_three_to_a_sword_hit() {
    run(|| {
        let mut h = setup();
        floor(&mut h);
        let id = summon(&mut h.game, &COW, [6.5, 201.0, 5.5]).expect("loaded");
        h.tick(3);
        hold(&mut h, ItemStack::of(item::IRON_SWORD));
        give(&mut h, Effect::Strength, 3000, 0);
        h.tick(20);
        h.push_frame(1, proto::V_ATTACK, &proto::Attack { target: id }.encode());
        h.tick(1);
        let m = h.game.mobs.mobs.iter().find(|m| m.id == id).unwrap();
        // 1 + 5 + 3 = 9 of the cow's 10.
        assert_eq!(m.health, 1.0);
    });
}

#[test]
fn haste_two_digs_faster() {
    run(|| {
        let mut h = setup();
        floor(&mut h);
        hold(&mut h, ItemStack::of(item::IRON_PICKAXE));
        let held = h.game.players.players[&1].inv.get_current_item();
        let stone = block::STONE << 4;
        let base = ::items::dig::player_relative_block_hardness(stone, &held, &super::mining::dig_context(&h.game, 1));
        give(&mut h, Effect::Haste, 3000, 1);
        let fast = ::items::dig::player_relative_block_hardness(stone, &held, &super::mining::dig_context(&h.game, 1));
        // Haste II multiplies the speed by 1 + 2 * 0.2.
        assert!(fast > base * 1.39 && fast < base * 1.41, "{base} -> {fast}");
    });
}

#[test]
fn the_effect_command_adds_and_clears() {
    run(|| {
        let mut h = setup();
        h.send(GameMsg::Potion { id: 1, op: crate::commands::PotionOp::Add(PotionEffect::new(Effect::Speed, 600, 1)) });
        h.tick(1);
        assert_eq!(h.game.players.players[&1].effects.amplifier(Effect::Speed), Some(1));
        h.send(GameMsg::Potion { id: 1, op: crate::commands::PotionOp::Clear });
        h.tick(1);
        assert!(h.game.players.players[&1].effects.is_empty());
    });
}
