//! Pig, sheep and chicken (T9i) over the slab world of tests.rs: stone at y 200 under x, z 4..=13,
//! the player at the west edge.

use std::sync::Arc;

use items::ids::item;
use items::{ItemStack, Recipes};
use proto::{PlayerState, EV_EAT_GRASS, ON_GROUND};
use sim::kinds::{CHICKEN, COW, PIG, SHEEP};
use worldgen::java_random::JavaRandom;

use super::ai::{self, eat_grass::EatGrass, Goal};
use super::animals::{dye_damage, get_random_sheep_color, meta_of_dye_damage};
use super::combat::hurt_mob;
use super::interact::process_mob_interact;
use super::{def_of, summon, Mob};
use crate::game::damage::{Attacker, DamageSource};
use crate::game::terrain::{self, Blocks};
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;
const GRASS: u16 = 2 << 4;
const DIRT: u16 = 3 << 4;
const TALL_GRASS: u16 = 31 << 4 | 1;

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    h.world.lock().unwrap().set_rule("doMobSpawning", "false");
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

fn hold(h: &mut Harness, stack: ItemStack) {
    h.game.players.players.get_mut(&1).unwrap().inv.main[0] = stack;
}

fn held(h: &Harness) -> ItemStack {
    h.game.players.players[&1].inv.main[0]
}

fn at(h: &mut Harness, kind: &'static sim::kinds::Kind, x: f64, z: f64) -> u32 {
    let id = summon(&mut h.game, kind, [x, 201.0, z]).expect("loaded");
    h.tick(3);
    id
}

fn mob(h: &Harness, id: u32) -> &Mob {
    h.game.mobs.mobs.iter().find(|m| m.id == id).expect("the mob")
}

fn mob_mut(h: &mut Harness, id: u32) -> &mut Mob {
    h.game.mobs.mobs.iter_mut().find(|m| m.id == id).expect("the mob")
}

fn new_mob(kind: &sim::kinds::Kind) -> Mob {
    Mob::new(1, def_of(kind).unwrap(), [0.0; 3], 0.0)
}

fn lists(m: &Mob) -> (Vec<i32>, Vec<u32>) {
    (m.tasks.entries.iter().map(|e| e.priority).collect(), m.tasks.entries.iter().map(|e| e.action.mutex_bits()).collect())
}

fn player_src() -> DamageSource {
    DamageSource::player(Attacker { name: "Xy".into(), pos: [5.5, 201.0, 5.5], player: true, id: Some(1) })
}

/// Item entities on the ground, summed by item and metadata.
fn dropped(h: &Harness, it: u16) -> (i32, Vec<i32>) {
    let es: Vec<_> = h.game.items.entities.iter().filter(|e| e.stack.item == it).collect();
    (es.iter().map(|e| e.stack.count).sum(), es.iter().map(|e| e.stack.damage).collect())
}

#[test]
fn each_kind_has_the_vanilla_goal_list_and_size() {
    let pig = new_mob(&PIG);
    assert_eq!(lists(&pig), (vec![0, 1, 3, 4, 4, 5, 6, 7, 8], vec![4, 1, 3, 3, 3, 0, 1, 2, 3]));
    assert!(pig.navigator.get_can_swim());
    assert_eq!((pig.def.max_health, pig.def.speed, pig.body.width, pig.body.height), (10.0, 0.25, 0.9, 0.9));

    let sheep = new_mob(&SHEEP);
    assert_eq!(lists(&sheep), (vec![0, 1, 2, 3, 4, 5, 6, 7, 8], vec![4, 1, 3, 3, 0, 7, 1, 2, 3]));
    assert_eq!((sheep.def.max_health, sheep.def.speed, sheep.body.width, sheep.body.height), (8.0, 0.23, 0.9, 1.3));

    let chicken = new_mob(&CHICKEN);
    assert_eq!(lists(&chicken), (vec![0, 1, 2, 3, 4, 5, 6, 7], vec![4, 1, 3, 3, 0, 1, 2, 3]));
    assert_eq!((chicken.def.max_health, chicken.def.speed, chicken.body.width, chicken.body.height), (4.0, 0.25, 0.4, 0.7));
    assert_eq!(chicken.path_priority.get(super::path::node::PathNodeType::Water), 0.0);
}

#[test]
fn breeding_items_are_each_kinds_own() {
    let (pig, sheep, chicken, cow) = (new_mob(&PIG), new_mob(&SHEEP), new_mob(&CHICKEN), new_mob(&COW));
    for it in [item::CARROT, item::POTATO, item::BEETROOT] {
        assert!(pig.is_breeding_item(it));
        assert!(!sheep.is_breeding_item(it) && !chicken.is_breeding_item(it));
    }
    assert!(!pig.is_breeding_item(item::WHEAT));
    for it in [item::WHEAT_SEEDS, item::MELON_SEEDS, item::PUMPKIN_SEEDS, item::BEETROOT_SEEDS] {
        assert!(chicken.is_breeding_item(it));
        assert!(!pig.is_breeding_item(it) && !sheep.is_breeding_item(it));
    }
    assert!(!chicken.is_breeding_item(item::WHEAT));
    assert!(sheep.is_breeding_item(item::WHEAT) && cow.is_breeding_item(item::WHEAT));
    assert!(!sheep.is_breeding_item(item::WHEAT_SEEDS));
}

#[test]
fn a_pig_follows_a_carrot_and_loves_it() {
    run(|| {
        let mut h = setup();
        hold(&mut h, ItemStack::new(item::CARROT, 5, 0));
        let id = at(&mut h, &PIG, 11.5, 5.5);
        let d0 = (mob(&h, id).body.pos.x - 5.5).abs();
        let mut tempted = false;
        for _ in 0..120 {
            h.tick(1);
            tempted |= mob(&h, id).tasks.is_executing(4);
        }
        assert!(tempted, "the food EntityAITempt never started");
        assert!((mob(&h, id).body.pos.x - 5.5).abs() < d0 - 2.0);
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(mob(&h, id).in_love, 600);
        assert_eq!(held(&h).get_count(), 4);
        // Wheat is not its food.
        hold(&mut h, ItemStack::new(item::WHEAT, 1, 0));
        mob_mut(&mut h, id).in_love = 0;
        assert!(!process_mob_interact(&mut h.game, 1, id, 0));
    });
}

#[test]
fn a_saddle_goes_on_an_adult_pig_once() {
    run(|| {
        let mut h = setup();
        let id = at(&mut h, &PIG, 6.5, 5.5);
        hold(&mut h, ItemStack::new(item::SADDLE, 2, 0));
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert!(mob(&h, id).get_saddled());
        assert_eq!(held(&h).get_count(), 1);
        // A saddled pig keeps its saddle and the second one is not used (riding is DROPPED).
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(held(&h).get_count(), 1);
        // A piglet takes none.
        let baby = at(&mut h, &PIG, 7.5, 5.5);
        mob_mut(&mut h, baby).set_growing_age(-100);
        assert!(process_mob_interact(&mut h.game, 1, baby, 0));
        assert!(!mob(&h, baby).get_saddled());
        assert_eq!(held(&h).get_count(), 1);
    });
}

/// `EntityAIEatGrass` run by hand on a clone of the sheep, its effects applied to the game.
fn eat(h: &mut Harness, id: u32, seed_hit: bool) -> Mob {
    let mut m = mob(h, id).clone();
    let mut rand = JavaRandom::new(1);
    if seed_hit {
        // A generator whose first nextInt(1000) is 0.
        let seed = (0..1_000_000i64).find(|&s| JavaRandom::new(s).next_int_bounded(1000) == 0).unwrap();
        rand = JavaRandom::new(seed);
    }
    let mut fx = Vec::new();
    let mut goal = EatGrass::new();
    {
        let blocks = Blocks(&h.game);
        let (mut a, mut b): (Vec<Mob>, Vec<Mob>) = (Vec::new(), Vec::new());
        let mut c = ai::Ctx { w: &blocks, mob: &mut m, others: ai::Others { before: &mut a, after: &mut b }, players: &[], rand: &mut rand, fx: &mut fx, sky_subtracted: 0 };
        if !goal.should_execute(&mut c) {
            return m;
        }
        goal.start_executing(&mut c);
        assert_eq!(goal.get_eating_grass_timer(), 40);
        let mut ticks = 0;
        while goal.should_continue_executing(&mut c) {
            goal.update_task(&mut c);
            ticks += 1;
        }
        goal.reset_task(&mut c);
        assert_eq!(ticks, 40);
    }
    super::apply_effects(&mut h.game, fx);
    m
}

#[test]
fn a_sheep_eats_grass_and_regrows_its_wool() {
    run(|| {
        let mut h = setup();
        let id = at(&mut h, &SHEEP, 9.5, 9.5);
        terrain::set_block(&mut h.game, (9, 200, 9), GRASS);
        mob_mut(&mut h, id).set_sheared(true);
        mob_mut(&mut h, id).set_fleece_color(11);
        // Most draws do nothing (1 in 1000).
        assert!(eat(&mut h, id, false).get_sheared());
        assert!(mob(&h, id).get_sheared());
        h.frames_for(1);
        let m = eat(&mut h, id, true);
        assert!(!m.get_sheared(), "eatGrassBonus");
        assert_eq!(m.get_fleece_color(), 11, "the colour stays");
        assert_eq!(terrain::state(&h.game, (9, 200, 9)), DIRT, "grass turned to dirt");
        // Status 10 went to the viewer at the start.
        let ev: Vec<_> = h.frames_for(1).iter().filter(|f| f.0 == proto::S_ENTITY_EVENT).filter_map(|f| proto::EntityEvent::decode(&f.1)).collect();
        assert!(ev.iter().any(|e| e.id == id && e.status == EV_EAT_GRASS), "{ev:?}");
    });
}

#[test]
fn a_sheep_eats_tall_grass_and_respects_mobgriefing() {
    run(|| {
        let mut h = setup();
        let id = at(&mut h, &SHEEP, 9.5, 9.5);
        terrain::set_block(&mut h.game, (9, 201, 9), TALL_GRASS);
        mob_mut(&mut h, id).set_sheared(true);
        let m = eat(&mut h, id, true);
        assert!(!m.get_sheared());
        assert_eq!(terrain::state(&h.game, (9, 201, 9)), 0, "the tall grass is gone");
        // With mobGriefing off the grass stays but the wool still grows back.
        h.world.lock().unwrap().set_rule("mobGriefing", "false");
        terrain::set_block(&mut h.game, (9, 200, 9), GRASS);
        mob_mut(&mut h, id).set_sheared(true);
        let m = eat(&mut h, id, true);
        assert!(!m.get_sheared());
        assert_eq!(terrain::state(&h.game, (9, 200, 9)), GRASS);
        // Bare stone: the goal never starts.
        terrain::set_block(&mut h.game, (9, 200, 9), STONE);
        mob_mut(&mut h, id).set_sheared(true);
        assert!(eat(&mut h, id, true).get_sheared());
    });
}

#[test]
fn a_lamb_eating_grass_grows_up_a_minute() {
    run(|| {
        let mut h = setup();
        let id = at(&mut h, &SHEEP, 9.5, 9.5);
        terrain::set_block(&mut h.game, (9, 200, 9), GRASS);
        mob_mut(&mut h, id).set_growing_age(-24000);
        let m = eat(&mut h, id, true);
        assert_eq!(m.growing_age, -24000 + 60 * 20, "addGrowth(60)");
    });
}

#[test]
fn shears_take_one_to_three_wool_and_wear() {
    run(|| {
        let mut h = setup();
        let id = at(&mut h, &SHEEP, 6.5, 5.5);
        mob_mut(&mut h, id).set_fleece_color(14);
        hold(&mut h, ItemStack::of(item::SHEARS));
        let mut counts = Vec::new();
        for n in 0..40 {
            let before = dropped(&h, 35).0;
            mob_mut(&mut h, id).set_sheared(false);
            hold(&mut h, ItemStack::of(item::SHEARS));
            h.game.items.entities.truncate(h.game.items.entities.len());
            assert!(!process_mob_interact(&mut h.game, 1, id, 0), "the shear branch does not answer true itself");
            assert!(mob(&h, id).get_sheared());
            assert_eq!(held(&h).get_metadata(), 1, "damageItem(1)");
            let (total, metas) = dropped(&h, 35);
            counts.push(total - before);
            assert!(metas.iter().all(|&m| m == 14), "wool of the fleece colour {metas:?}");
            if n == 0 {
                let e = h.game.items.entities.last().unwrap();
                assert!((e.body.pos.y - 202.0).abs() < 1e-9, "entityDropItem(stack, 1.0)");
            }
            // Already sheared: nothing more.
            let again = dropped(&h, 35).0;
            process_mob_interact(&mut h.game, 1, id, 0);
            assert_eq!(dropped(&h, 35).0, again);
            assert_eq!(held(&h).get_metadata(), 1);
        }
        assert!(counts.iter().all(|&c| (1..=3).contains(&c)), "{counts:?}");
        assert!(counts.contains(&1) && counts.contains(&3), "{counts:?}");
    });
}

#[test]
fn a_lamb_cannot_be_sheared_and_creative_shears_do_not_wear() {
    run(|| {
        let mut h = setup();
        let id = at(&mut h, &SHEEP, 6.5, 5.5);
        hold(&mut h, ItemStack::of(item::SHEARS));
        mob_mut(&mut h, id).set_growing_age(-100);
        process_mob_interact(&mut h.game, 1, id, 0);
        assert!(!mob(&h, id).get_sheared());
        assert_eq!(dropped(&h, 35).0, 0);
        mob_mut(&mut h, id).set_growing_age(0);
        h.game.sessions.get_mut(&1).unwrap().mode = proto::GM_CREATIVE;
        process_mob_interact(&mut h.game, 1, id, 0);
        assert!(mob(&h, id).get_sheared());
        assert_eq!(held(&h).get_metadata(), 0);
        assert!(dropped(&h, 35).0 >= 1);
    });
}

#[test]
fn dye_recolours_an_unsheared_sheep() {
    run(|| {
        let mut h = setup();
        let id = at(&mut h, &SHEEP, 6.5, 5.5);
        // Dye damage 1 is red (colour 14).
        hold(&mut h, ItemStack::new(item::DYE, 3, 1));
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(mob(&h, id).get_fleece_color(), 14);
        assert_eq!(held(&h).get_count(), 2);
        // The same colour is not used up, the click still counts.
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(held(&h).get_count(), 2);
        // Sheared: no change.
        mob_mut(&mut h, id).set_sheared(true);
        hold(&mut h, ItemStack::new(item::DYE, 3, 4));
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(mob(&h, id).get_fleece_color(), 14);
        assert_eq!(held(&h).get_count(), 3);
        assert!(mob(&h, id).get_sheared(), "dyeing keeps the sheared bit");
        // Creative keeps the stack.
        mob_mut(&mut h, id).set_sheared(false);
        h.game.sessions.get_mut(&1).unwrap().mode = proto::GM_CREATIVE;
        assert!(process_mob_interact(&mut h.game, 1, id, 0));
        assert_eq!(mob(&h, id).get_fleece_color(), 11);
        assert_eq!(held(&h).get_count(), 3);
        // A cow takes no dye.
        let cow = at(&mut h, &COW, 7.5, 5.5);
        assert!(!process_mob_interact(&mut h.game, 1, cow, 0));
    });
}

const MIX_RECIPES: &[(&str, &str)] = &[(
    "orange_dye_from_red_yellow",
    r#"{"type":"crafting_shapeless","group":"orange_dye","ingredients":[{"item":"minecraft:dye","data":1},{"item":"minecraft:dye","data":11}],"result":{"item":"minecraft:dye","data":14,"count":2}}"#,
)];

fn with_mix_recipes(h: &mut Harness) {
    h.game.windows.recipes = Some(Arc::new(Recipes::from_files(MIX_RECIPES.iter().map(|(n, j)| (*n, j.as_bytes()))).unwrap()));
}

#[test]
fn a_lamb_takes_the_mixed_colour_of_its_parents() {
    run(|| {
        let mut h = setup();
        with_mix_recipes(&mut h);
        // Red (dye damage 1) and yellow (11) craft orange (dye damage 14, colour 1), in either order.
        assert_eq!(meta_of_dye_damage(14), 1);
        assert_eq!(ai::sheep_dye_mix(&mut h.game, 1, 11), 1);
        assert_eq!(ai::sheep_dye_mix(&mut h.game, 11, 1), 1);
        // No recipe: one of the parents, by nextBoolean.
        let (red, blue) = (meta_of_dye_damage(1), meta_of_dye_damage(4));
        let seen: Vec<u8> = (0..40).map(|_| ai::sheep_dye_mix(&mut h.game, 1, 4)).collect();
        assert!(seen.iter().all(|&c| c == red || c == blue));
        assert!(seen.contains(&red) && seen.contains(&blue), "{seen:?}");
        // The effect spawns a calf of that colour.
        let before = h.game.mobs.mobs.len();
        super::apply_effects(&mut h.game, vec![ai::Effect::Child { def: def_of(&SHEEP).unwrap(), pos: [8.5, 201.0, 8.5], dyes: Some((1, 11)) }]);
        assert_eq!(h.game.mobs.mobs.len(), before + 1);
        let calf = h.game.mobs.mobs.last().unwrap();
        assert!(calf.is_child());
        assert_eq!(calf.get_fleece_color(), 1);
    });
}

#[test]
fn two_red_sheep_breed_a_red_lamb() {
    run(|| {
        let mut h = setup();
        let (a, b) = (at(&mut h, &SHEEP, 10.5, 10.5), at(&mut h, &SHEEP, 11.5, 10.5));
        for id in [a, b] {
            mob_mut(&mut h, id).set_fleece_color(14);
            hold(&mut h, ItemStack::new(item::WHEAT, 1, 0));
            assert!(process_mob_interact(&mut h.game, 1, id, 0));
        }
        for _ in 0..200 {
            h.tick(1);
            if h.game.mobs.mobs.len() == 3 {
                break;
            }
        }
        assert_eq!(h.game.mobs.mobs.len(), 3, "no lamb");
        let lamb = h.game.mobs.mobs.last().unwrap();
        assert!(lamb.is_child());
        assert_eq!(lamb.get_fleece_color(), 14);
        assert!(!lamb.get_sheared());
    });
}

#[test]
fn a_new_sheep_gets_a_vanilla_random_colour() {
    let mut rand = JavaRandom::new(42);
    let mut n = [0u32; 16];
    for _ in 0..100_000 {
        n[get_random_sheep_color(&mut rand) as usize] += 1;
    }
    let pct = |c: usize| n[c] as f64 / 1000.0;
    // black 5, gray 5, silver 5, brown 3, pink 0.164, white the rest.
    assert!((pct(15) - 5.0).abs() < 0.5 && (pct(7) - 5.0).abs() < 0.5 && (pct(8) - 5.0).abs() < 0.5, "{n:?}");
    assert!((pct(12) - 3.0).abs() < 0.4, "{n:?}");
    assert!(n[6] > 50 && n[6] < 300, "{n:?}");
    assert!(pct(0) > 80.0);
    assert_eq!(n.iter().sum::<u32>(), 100_000);
    assert_eq!([0, 6, 15].map(dye_damage), [15, 9, 0]);
    // /summon and the spawner run onInitialSpawn.
    run(|| {
        let mut h = setup();
        let mut seen = std::collections::HashSet::new();
        for n in 0..60 {
            let id = summon(&mut h.game, &SHEEP, [6.5 + (n % 5) as f64, 201.0, 6.5]).unwrap();
            seen.insert(mob(&h, id).get_fleece_color());
        }
        assert!(seen.len() >= 3, "{seen:?}");
    });
}

#[test]
fn a_chicken_lays_an_egg_inside_the_vanilla_window() {
    run(|| {
        let mut h = setup();
        // The constructor draws nextInt(6000) + 6000.
        let mut timers = Vec::new();
        for n in 0..30 {
            let id = summon(&mut h.game, &CHICKEN, [6.5 + (n % 5) as f64, 201.0, 6.5]).unwrap();
            timers.push(mob(&h, id).egg_timer);
        }
        assert!(timers.iter().all(|&t| (6000..12000).contains(&t)), "{timers:?}");
        assert!(timers.iter().min() != timers.iter().max());
        let id = at(&mut h, &CHICKEN, 9.5, 9.5);
        let eggs0 = dropped(&h, item::EGG).0;
        mob_mut(&mut h, id).egg_timer = 3;
        h.tick(1);
        h.tick(1);
        assert_eq!(dropped(&h, item::EGG).0, eggs0, "not yet");
        h.tick(1);
        assert_eq!(dropped(&h, item::EGG).0, eggs0 + 1, "the egg at timer 0");
        let t = mob(&h, id).egg_timer;
        assert!((6000..12000).contains(&t), "{t}");
        // A chick lays nothing and its timer holds.
        mob_mut(&mut h, id).set_growing_age(-24000);
        mob_mut(&mut h, id).egg_timer = 1;
        h.tick(5);
        assert_eq!(dropped(&h, item::EGG).0, eggs0 + 1);
        assert_eq!(mob(&h, id).egg_timer, 1);
    });
}

#[test]
fn a_chicken_falls_slowly() {
    run(|| {
        let mut h = setup();
        let chicken = summon(&mut h.game, &CHICKEN, [12.5, 206.0, 12.5]).unwrap();
        let cow = summon(&mut h.game, &COW, [11.5, 206.0, 12.5]).unwrap();
        h.tick(15);
        // v = 0.6 * 0.98 * (v - 0.08) has the fixed point -0.0470 / 0.412.
        let want = -0.6 * 0.98 * 0.08 / (1.0 - 0.6 * 0.98);
        let v = mob(&h, chicken).body.motion.y;
        assert!(!mob(&h, chicken).body.on_ground);
        assert!((v - want).abs() < 1e-3, "{v} vs {want}");
        let c = mob(&h, cow).body.motion.y;
        assert!(c < -0.5 || mob(&h, cow).body.on_ground, "the cow falls normally: {c}");
        // It lands without being hurt.
        h.tick(60);
        assert!(mob(&h, chicken).body.on_ground);
        assert_eq!(mob(&h, chicken).health, 4.0);
    });
}

#[test]
fn loot_follows_each_kinds_table() {
    run(|| {
        let mut h = setup();
        // Pig: 1 to 3 porkchops.
        let mut pork = Vec::new();
        for n in 0..50 {
            let id = summon(&mut h.game, &PIG, [6.5 + (n % 5) as f64, 201.0, 6.5]).unwrap();
            let before = dropped(&h, item::PORKCHOP).0;
            assert!(hurt_mob(&mut h.game, id, &player_src(), 100.0));
            pork.push(dropped(&h, item::PORKCHOP).0 - before);
        }
        assert!(pork.iter().all(|&p| (1..=3).contains(&p)) && pork.contains(&1) && pork.contains(&3), "{pork:?}");
        assert_eq!(dropped(&h, item::SADDLE).0, 0);
        // A saddled pig also drops the saddle.
        let id = summon(&mut h.game, &PIG, [8.5, 201.0, 8.5]).unwrap();
        mob_mut(&mut h, id).saddled = true;
        assert!(hurt_mob(&mut h.game, id, &player_src(), 100.0));
        assert_eq!(dropped(&h, item::SADDLE).0, 1);
        // Chicken: 0 to 2 feathers and one raw chicken.
        let (mut feathers, mut meat) = (Vec::new(), Vec::new());
        for n in 0..50 {
            let id = summon(&mut h.game, &CHICKEN, [6.5 + (n % 5) as f64, 201.0, 7.5]).unwrap();
            let (f0, m0) = (dropped(&h, item::FEATHER).0, dropped(&h, item::CHICKEN).0);
            assert!(hurt_mob(&mut h.game, id, &player_src(), 100.0));
            feathers.push(dropped(&h, item::FEATHER).0 - f0);
            meat.push(dropped(&h, item::CHICKEN).0 - m0);
        }
        assert!(feathers.iter().all(|&f| (0..=2).contains(&f)) && feathers.contains(&0) && feathers.contains(&2), "{feathers:?}");
        assert!(meat.iter().all(|&m| m == 1), "{meat:?}");
    });
}

#[test]
fn a_sheeps_drop_is_its_fleece_and_mutton() {
    run(|| {
        let mut h = setup();
        // Red wool is metadata 14; the mutton table is nested in the colour's table.
        let id = summon(&mut h.game, &SHEEP, [6.5, 201.0, 6.5]).unwrap();
        mob_mut(&mut h, id).set_fleece_color(14);
        assert_eq!(mob(&h, id).loot_table_path(), "loot_tables/entities/sheep/red.json");
        assert!(hurt_mob(&mut h.game, id, &player_src(), 100.0));
        let (wool, metas) = dropped(&h, 35);
        assert_eq!((wool, metas), (1, vec![14]));
        let mutton = dropped(&h, item::MUTTON).0;
        assert!((1..=2).contains(&mutton), "{mutton}");
        // Sheared: mutton only.
        let id = summon(&mut h.game, &SHEEP, [7.5, 201.0, 6.5]).unwrap();
        mob_mut(&mut h, id).set_sheared(true);
        assert_eq!(mob(&h, id).loot_table_path(), "loot_tables/entities/sheep.json");
        assert!(hurt_mob(&mut h.game, id, &player_src(), 100.0));
        assert_eq!(dropped(&h, 35).0, 1, "no new wool");
        assert!(dropped(&h, item::MUTTON).0 > mutton);
        // Every colour has a table in the jar.
        for c in 0..16u8 {
            let mut m = new_mob(&SHEEP);
            m.set_fleece_color(c);
            let t = super::loot::table_at(&mut h.game, &m.loot_table_path()).unwrap_or_else(|| panic!("no table for {c}"));
            let mut rand = JavaRandom::new(1);
            let mut nested = super::loot::Nested::new();
            let inner = super::loot::table_at(&mut h.game, "loot_tables/entities/sheep.json").unwrap();
            nested.insert(super::loot::table_file("entities/sheep"), inner);
            let out = super::loot::generate_with(&t, &mut rand, &Default::default(), &nested);
            assert_eq!(out[0].get_item(), 35);
            assert_eq!(out[0].get_metadata(), c as i32);
        }
    });
}

#[test]
fn sheep_pig_and_chicken_state_survives_a_save() {
    run(|| {
        let mut h = setup();
        let s = at(&mut h, &SHEEP, 6.5, 6.5);
        let p = at(&mut h, &PIG, 8.5, 6.5);
        let c = at(&mut h, &CHICKEN, 10.5, 6.5);
        {
            let m = mob_mut(&mut h, s);
            m.set_fleece_color(5);
            m.set_sheared(true);
        }
        mob_mut(&mut h, p).saddled = true;
        mob_mut(&mut h, c).egg_timer = 7777;
        let v = super::save(&h.game);
        h.game.mobs.mobs.clear();
        super::load(&mut h.game, v);
        let find = |h: &Harness, k: &str| h.game.mobs.mobs.iter().find(|m| m.def.kind.name == k).unwrap().clone();
        let sheep = find(&h, "sheep");
        assert_eq!((sheep.get_fleece_color(), sheep.get_sheared()), (5, true));
        assert!(find(&h, "pig").saddled);
        assert_eq!(find(&h, "chicken").egg_timer, 7777);
    });
}
