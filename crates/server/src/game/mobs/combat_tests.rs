//! Mob combat (T9c) over the same slab world as the mob tests: the player at the west edge, a cow
//! or zombie a block east of him.

use items::ids::item;
use items::ItemStack;
use proto::{PlayerState, ANIM_CRIT, EV_DEATH, EV_HURT, ON_GROUND};
use sim::kinds::{COW, ZOMBIE};

use super::{attack_entity_as_mob, attack_target, hurt_mob};
use crate::game::damage::{Attacker, DamageSource};
use crate::game::mobs::{summon, Mob};
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;

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

/// A cow a block east of the player, landed.
fn cow(h: &mut Harness) -> u32 {
    let id = summon(&mut h.game, &COW, [6.5, 201.0, 5.5]).expect("loaded");
    h.tick(3);
    id
}

fn mob(h: &Harness, id: u32) -> &Mob {
    h.game.mobs.mobs.iter().find(|m| m.id == id).expect("the mob")
}

fn player_src(pos: [f64; 3]) -> DamageSource {
    DamageSource::player(Attacker { name: "Xy".into(), pos, player: true, id: Some(1) })
}

fn statuses(frames: &[(u8, Vec<u8>)], id: u32) -> Vec<u8> {
    frames
        .iter()
        .filter(|f| f.0 == proto::S_ENTITY_EVENT)
        .filter_map(|f| proto::EntityEvent::decode(&f.1))
        .filter(|e| e.id == id)
        .map(|e| e.status)
        .collect()
}

fn hold(h: &mut Harness, stack: ItemStack) {
    h.game.players.players.get_mut(&1).unwrap().inv.main[0] = stack;
}

#[test]
fn a_full_strength_sword_hit_takes_its_damage_and_wears_the_sword() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        hold(&mut h, ItemStack::of(item::IRON_SWORD));
        // 20 ticks cover the 12.5 tick cooldown period of attack speed 1.6.
        h.tick(20);
        h.frames_for(1);
        h.push_frame(1, proto::V_ATTACK, &proto::Attack { target: id }.encode());
        h.tick(1);
        // 1 + (3 + 2) at full strength.
        assert_eq!(mob(&h, id).health, 10.0 - 6.0);
        assert_eq!(h.game.players.players[&1].inv.main[0].get_metadata(), 1, "ItemSword.hitEntity: 1 durability");
        let f = h.frames_for(1);
        assert_eq!(statuses(&f, id), vec![EV_HURT]);
        assert_eq!(h.game.players.players[&1].ticks_since_last_swing, 1, "reset by the attack, then one player tick");
    });
}

#[test]
fn a_swing_before_the_cooldown_is_weak() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        h.game.players.players.get_mut(&1).unwrap().ticks_since_last_swing = 0;
        attack_target(&mut h.game, 1, id);
        // Bare hand: 1.0 * (0.2 + f2^2 * 0.8) with f2 = 0.5 / 5.
        let f2 = 0.5f32 / 5.0;
        let want = 1.0 * (0.2 + f2 * f2 * 0.8);
        assert!((mob(&h, id).last_damage - want).abs() < 1e-6, "{}", mob(&h, id).last_damage);
    });
}

#[test]
fn switching_slots_restarts_the_attack_cooldown() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        // EntityPlayer.onUpdate: a different item in hand restarts ticksSinceLastSwing.
        hold(&mut h, ItemStack::of(item::IRON_SWORD));
        h.tick(1);
        assert_eq!(h.game.players.players[&1].ticks_since_last_swing, 0);
        attack_target(&mut h.game, 1, id);
        let weak = mob(&h, id).last_damage;
        // 13 ticks later the sword's 12.5 tick cooldown is full: the same attack is stronger.
        h.tick(13);
        attack_target(&mut h.game, 1, id);
        let strong = mob(&h, id).last_damage;
        assert!(weak < strong, "weak {weak}, strong {strong}");
        assert_eq!(strong, 6.0);
    });
}

#[test]
fn out_of_reach_does_nothing() {
    run(|| {
        let mut h = setup();
        let id = summon(&mut h.game, &COW, [12.5, 201.0, 5.5]).unwrap();
        h.tick(25);
        h.push_frame(1, proto::V_ATTACK, &proto::Attack { target: id }.encode());
        h.tick(1);
        assert_eq!(mob(&h, id).health, 10.0, "7 blocks is past 6");
    });
}

#[test]
fn the_hurt_window_lets_only_a_stronger_hit_through() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        let src = player_src([5.5, 201.0, 5.5]);
        assert!(hurt_mob(&mut h.game, id, &src, 4.0));
        assert_eq!(mob(&h, id).health, 6.0);
        assert!(!hurt_mob(&mut h.game, id, &src, 4.0), "no stronger than the last");
        assert!(!hurt_mob(&mut h.game, id, &src, 3.0));
        assert!(hurt_mob(&mut h.game, id, &src, 6.0), "6 - 4 more goes through");
        assert_eq!(mob(&h, id).health, 4.0);
        assert_eq!(mob(&h, id).last_damage, 6.0);
        // hurtResistantTime 20 counts down; over half it still blocks, at 10 it is open.
        h.tick(9);
        assert!(!hurt_mob(&mut h.game, id, &src, 3.0), "3 is not above lastDamage 6");
        h.tick(1);
        assert_eq!(mob(&h, id).hurt_resistant, 10);
        assert!(hurt_mob(&mut h.game, id, &src, 1.0));
        assert_eq!(mob(&h, id).health, 3.0);
    });
}

#[test]
fn a_hit_knocks_the_mob_away_from_the_attacker() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        let before = mob(&h, id).body.motion;
        assert!(mob(&h, id).body.on_ground);
        assert!(hurt_mob(&mut h.game, id, &player_src([5.5, 201.0, 5.5]), 1.0));
        let m = mob(&h, id);
        // d1 = 5.5 - 6.5 = -1, d0 = 0; motion = motion / 2 - ratio / f * 0.4.
        let s = 0.4f32 as f64;
        assert!((m.body.motion.x - (before.x / 2.0 + s)).abs() < 1e-12);
        assert!((m.body.motion.z - before.z / 2.0).abs() < 1e-12);
        assert!((m.body.motion.y - (before.y / 2.0 + s).min(0.400_000_005_960_464_5)).abs() < 1e-12);
        assert!(m.air_borne && m.velocity_changed);
        assert_eq!(m.hurt_time, 10);
        // The tracker carries it: a velocity frame goes to the player.
        h.frames_for(1);
        h.tick(1);
        assert!(h.frames_for(1).iter().any(|f| f.0 == proto::S_VELOCITY));
    });
}

#[test]
fn a_sprinting_full_strength_hit_adds_knockback_and_a_fall_hit_is_critical() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        h.tick(20);
        h.frames_for(1);
        // Sprinting: a knockback level (0.5) on top of the hit's 0.4, both along +x.
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND | proto::SPRINTING, yaw: -90.0, ..Default::default() });
        h.game.players.players.get_mut(&1).unwrap().ticks_since_last_swing = 100;
        attack_target(&mut h.game, 1, id);
        // The hit's 0.4 is halved by the second knockBack, then 0.5 is added.
        let x = mob(&h, id).body.motion.x;
        assert!((x - (0.4000000059604645 / 2.0 + 0.5)).abs() < 1e-6, "{x}");
        // A falling player who is not sprinting crits for x1.5.
        let id2 = summon(&mut h.game, &COW, [7.5, 201.0, 5.5]).unwrap();
        h.tick(3);
        h.frames_for(1);
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.5, 5.5], flags: 0, ..Default::default() });
        h.game.players.players.get_mut(&1).unwrap().fall_distance = 1.0;
        h.game.players.players.get_mut(&1).unwrap().ticks_since_last_swing = 100;
        attack_target(&mut h.game, 1, id2);
        assert_eq!(mob(&h, id2).last_damage, 1.5);
        let f = h.frames_for(1);
        let a = f.iter().find(|f| f.0 == proto::S_ANIMATION).map(|f| proto::Animation::decode(&f.1).unwrap());
        assert_eq!(a, Some(proto::Animation { id: id2, animation: ANIM_CRIT }));
    });
}

#[test]
fn a_killed_mob_dies_for_twenty_ticks_and_is_then_removed() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        h.frames_for(1);
        assert!(hurt_mob(&mut h.game, id, &player_src([5.5, 201.0, 5.5]), 100.0));
        assert_eq!(mob(&h, id).health, 0.0);
        let f = h.frames_for(1);
        assert_eq!(statuses(&f, id), vec![EV_HURT, EV_DEATH]);
        for n in 1..=19 {
            h.tick(1);
            assert_eq!(mob(&h, id).death_time, n);
        }
        let gone: Vec<u32> = h.frames_for(1).iter().filter(|f| f.0 == proto::S_DESTROY).flat_map(|f| proto::decode_destroy(&f.1).unwrap()).collect();
        assert!(!gone.contains(&id), "still there at deathTime 19");
        h.tick(1);
        assert!(h.game.mobs.mobs.iter().all(|m| m.id != id), "removed at deathTime 20");
        h.tick(1);
        let f = h.frames_for(1);
        let destroyed: Vec<u32> = f.iter().filter(|f| f.0 == proto::S_DESTROY).flat_map(|f| proto::decode_destroy(&f.1).unwrap()).collect();
        assert!(destroyed.contains(&id));
        // A dead mob takes no more damage.
        assert!(!hurt_mob(&mut h.game, id, &player_src([5.5, 201.0, 5.5]), 1.0));
    });
}

#[test]
fn a_cow_drops_what_its_loot_table_gives() {
    run(|| {
        let mut h = setup();
        let (mut leather, mut beef) = (Vec::new(), Vec::new());
        for n in 0..60 {
            let id = summon(&mut h.game, &COW, [6.5 + (n % 5) as f64, 201.0, 6.5]).unwrap();
            let before = h.game.items.entities.len();
            assert!(hurt_mob(&mut h.game, id, &player_src([5.5, 201.0, 5.5]), 100.0));
            let drops = &h.game.items.entities[before..];
            let count = |it: u16| drops.iter().filter(|e| e.stack.item == it).map(|e| e.stack.count).sum::<i32>();
            let (l, b) = (count(item::LEATHER), count(item::BEEF));
            assert_eq!(drops.iter().map(|e| e.stack.count).sum::<i32>(), l + b, "only leather and beef");
            leather.push(l);
            beef.push(b);
            assert!(drops.iter().all(|e| e.pickup_delay == 10 && e.body.pos.y == 201.0));
        }
        // set_count 0..2 leather (an empty stack is not dropped) and 1..3 beef.
        assert!(leather.iter().all(|&l| (0..=2).contains(&l)) && leather.contains(&0) && leather.contains(&2));
        assert!(beef.iter().all(|&b| (1..=3).contains(&b)) && beef.contains(&1) && beef.contains(&3));
    });
}

#[test]
fn a_mob_hurts_a_player_through_the_damage_path() {
    run(|| {
        let mut h = setup();
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        let z = summon(&mut h.game, &ZOMBIE, [6.5, 201.0, 5.5]).unwrap();
        h.tick(1);
        h.frames_for(1);
        assert!(attack_entity_as_mob(&mut h.game, z, 1));
        // EntityZombie attack damage 3, scaled by the difficulty setting.
        let hp = h.game.players.players[&1].health;
        assert!((15.5..20.0).contains(&hp), "{hp}");
        let f = h.frames_for(1);
        assert_eq!(statuses(&f, 1), vec![EV_HURT]);
        assert!(f.iter().any(|f| f.0 == proto::S_VELOCITY), "knocked away from the zombie");
        assert_eq!(h.game.players.players[&1].hurt_resistant, 20);
        assert!(!attack_entity_as_mob(&mut h.game, z, 1), "inside the hurt window");
        // A cow has no attack damage.
        let c = cow(&mut h);
        assert!(!attack_entity_as_mob(&mut h.game, c, 1));
    });
}

fn enchanted(it: u16, e: u8, level: i32) -> ItemStack {
    let mut s = ItemStack::of(it);
    s.add_enchantment(e, level);
    s
}

#[test]
fn sharpness_five_adds_three_to_an_iron_sword_hit() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        hold(&mut h, enchanted(item::IRON_SWORD, items::enchant::id::SHARPNESS, 5));
        h.tick(20);
        h.frames_for(1);
        h.push_frame(1, proto::V_ATTACK, &proto::Attack { target: id }.encode());
        h.tick(1);
        // 1 + (3 + 2) plus EnchantmentDamage 1 + (5 - 1) * 0.5 = 3 (not 1.25 * 5).
        assert_eq!(mob(&h, id).health, 10.0 - 9.0);
    });
}

#[test]
fn looting_three_raises_the_beef_a_cow_drops() {
    run(|| {
        let mut h = setup();
        hold(&mut h, enchanted(item::IRON_SWORD, items::enchant::id::LOOTING, 3));
        let mut best = 0;
        for n in 0..60 {
            let id = summon(&mut h.game, &COW, [6.5 + (n % 5) as f64, 201.0, 6.5]).unwrap();
            let before = h.game.items.entities.len();
            assert!(hurt_mob(&mut h.game, id, &player_src([5.5, 201.0, 5.5]), 100.0));
            let beef: i32 = h.game.items.entities[before..].iter().filter(|e| e.stack.item == item::BEEF).map(|e| e.stack.count).sum();
            best = best.max(beef);
        }
        // looting_enchant adds up to 3 * 1 beef on top of the 1..3
        assert!(best > 3, "best beef {best}");
    });
}

#[test]
fn fire_aspect_sets_the_mob_alight() {
    run(|| {
        let mut h = setup();
        let id = cow(&mut h);
        hold(&mut h, enchanted(item::IRON_SWORD, items::enchant::id::FIRE_ASPECT, 2));
        h.tick(20);
        h.push_frame(1, proto::V_ATTACK, &proto::Attack { target: id }.encode());
        h.tick(1);
        assert!(mob(&h, id).fire >= 7 * 20, "{}", mob(&h, id).fire);
    });
}
