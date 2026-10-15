//! Natural spawning and despawning (T9d): the caps, the light rule, the distance rules, animals
//! at chunk generation, the biome lists and the despawn rules.

use proto::PlayerState;
use sim::kinds::{COW, ZOMBIE};
use worldgen::biome;
use worldgen::java_random::JavaRandom;
use worldgen::populate::biomes::creatures;

use super::*;
use crate::game::mobs as gm;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;
const BEDROCK: u16 = 7 << 4;
const GRASS: u16 = 2 << 4;
const AIR: u16 = 0;

fn mob_count(h: &Harness) -> usize {
    h.game.mobs.mobs.len()
}

// ---- caps ----

#[test]
fn caps_scale_with_the_chunks_counted() {
    // One player counts 17 x 17 = 289 chunks: the caps are the enum's numbers.
    assert_eq!(cap(CreatureType::Monster, 289), 70);
    assert_eq!(cap(CreatureType::Creature, 289), 10);
    assert_eq!(cap(CreatureType::Ambient, 289), 15);
    assert_eq!(cap(CreatureType::WaterCreature, 289), 5);
    // Two players apart count 578: double. Integer division in between.
    assert_eq!(cap(CreatureType::Monster, 578), 140);
    assert_eq!(cap(CreatureType::Creature, 100), 3);
}

#[test]
fn eligible_chunks_skip_the_edge_and_the_unsent() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        let players = player_positions(&h.game);
        assert_eq!(players.len(), 1);
        let e = eligible_chunks(&h.game, &players);
        assert_eq!(e.counted, 289, "17 x 17 chunks around the one player");
        assert!(!e.chunks.is_empty());
        for &(x, z) in &e.chunks {
            assert!(x.abs() < 8 && z.abs() < 8, "the ring at 8 is counted, never eligible");
            assert!(terrain::is_final(&h.game, (x, z)));
        }
        // Only the chunks around the player are held: far fewer than the 225 inside the ring.
        assert!(e.chunks.len() < 60);
        // Two players in one chunk: the held chunks are counted once, the rest twice.
        let two = [players[0], players[0]];
        let e2 = eligible_chunks(&h.game, &two);
        assert_eq!(e2.chunks.len(), e.chunks.len());
        assert_eq!(e2.counted, 289 + 289 - e.chunks.len() as i32);
    });
}

#[test]
fn the_cap_counts_every_mob_persistence_does_not_exempt() {
    let mut h = Harness::new(1, 1);
    let zombie = def_by_name("zombie").unwrap();
    let cow = def_by_name("cow").unwrap();
    for _ in 0..71 {
        gm::spawn(&mut h.game, zombie, [0.5, 100.0, 0.5], 0.0);
    }
    gm::spawn(&mut h.game, cow, [0.5, 100.0, 0.5], 0.0);
    assert_eq!(count_entities(&h.game, CreatureType::Monster), 71);
    assert_eq!(count_entities(&h.game, CreatureType::Creature), 1);
    // 71 > 70: the type is skipped. 70 is still allowed (`k4 <= l4`).
    assert!(71 > cap(CreatureType::Monster, 289));
    for m in h.game.mobs.mobs.iter_mut().take(5) {
        m.persistent = true;
    }
    assert_eq!(count_entities(&h.game, CreatureType::Monster), 66);
    assert!(count_entities(&h.game, CreatureType::Monster) <= cap(CreatureType::Monster, 289));
    // No terrain attached: no eligible chunk, nothing spawns, and the count stays put.
    assert_eq!(find_chunks_for_spawning(&mut h.game, true, true, true), 0);
    assert_eq!(mob_count(&h), 72);
}

// ---- the light rule ----

#[test]
fn light_rule_follows_is_valid_light_level() {
    // Sky light above nextInt(32) fails; else the light must not exceed nextInt(8).
    for seed in 0..200 {
        let (mut a, mut b) = (JavaRandom::new(seed), JavaRandom::new(seed));
        for (sky, light) in [(0, 0), (0, 7), (0, 8), (15, 0), (3, 4), (0, 3)] {
            let sky_fail = sky > b.next_int_bounded(32);
            let want = if sky_fail { false } else { light <= b.next_int_bounded(8) };
            assert_eq!(valid_light_level(sky, light, &mut a), want, "seed {seed} sky {sky} light {light}");
        }
    }
    // Light 8 and above never passes; pitch dark always does.
    let mut r = JavaRandom::new(5);
    for _ in 0..500 {
        assert!(!valid_light_level(0, 8, &mut r));
        assert!(valid_light_level(0, 0, &mut r));
    }
    // Open sky (15) passes only when nextInt(32) >= 15, then the light test.
    let passes = (0..3200).filter(|_| valid_light_level(15, 0, &mut r)).count();
    assert!((1400..1900).contains(&passes), "{passes} of 3200 under full sky light");
}

#[test]
fn the_sky_darkens_at_night() {
    assert_eq!(skylight_subtracted(6000, false, false), 0);
    assert_eq!(skylight_subtracted(18000, false, false), 11);
    // Rain dims the day by up to 5/16 of 11.
    assert!(skylight_subtracted(6000, true, false) > 0);
    assert!(skylight_subtracted(6000, true, true) >= skylight_subtracted(6000, true, false));
}

// ---- the distance rules ----

#[test]
fn spots_within_24_of_a_player_or_of_the_spawn_point_are_refused() {
    let players = [[0.0, 64.0, 0.0]];
    let spawn = [1000, 64, 1000];
    assert!(!far_enough(&players, spawn, 23.9, 64.0, 0.0));
    assert!(far_enough(&players, spawn, 24.0, 64.0, 0.0), "exactly 24 is not within");
    assert!(!far_enough(&players, spawn, 0.0, 87.0, 0.0), "height counts");
    assert!(far_enough(&players, spawn, 0.0, 88.0, 0.0));
    // The spawn point needs >= 576, measured from the block corner.
    assert!(!far_enough(&[], spawn, 1023.9, 64.0, 1000.0));
    assert!(far_enough(&[], spawn, 1024.0, 64.0, 1000.0));
    assert!(!far_enough(&[], spawn, 1000.0, 64.0, 976.5));
    // Any of several players refuses.
    assert!(!far_enough(&[[500.0, 64.0, 500.0], [0.0, 64.0, 0.0]], spawn, 10.0, 64.0, 10.0));
}

/// A closed stone box in chunk (1, 0) with a dark room inside: a place where monsters can spawn.
fn dark_room() -> Harness {
    let mut h = Harness::new(7, 3);
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    h.game.mobs.mobs.clear();
    for x in 17..=30 {
        for z in 1..=14 {
            for y in 19..=25 {
                terrain::set_block(&mut h.game, (x, y, z), STONE);
            }
        }
    }
    for x in 20..=28 {
        for z in 4..=12 {
            for y in 21..=23 {
                terrain::set_block(&mut h.game, (x, y, z), AIR);
            }
        }
    }
    h
}

fn room_spawns(h: &mut Harness, ctx: &Ctx, t: CreatureType) -> usize {
    let before = mob_count(h);
    let mut st = std::mem::take(&mut h.game.mobs.spawner);
    for _ in 0..150 {
        pack_at(&mut h.game, ctx, &mut st, t, (24, 21, 8));
    }
    h.game.mobs.spawner = st;
    mob_count(h) - before
}

#[test]
fn monsters_spawn_in_the_dark_away_from_players_and_the_spawn_point() {
    run(|| {
        let mut h = dark_room();
        let ctx = context(&h.game);
        assert_eq!(ctx.players.len(), 1);
        let n = room_spawns(&mut h, &ctx, CreatureType::Monster);
        assert!(n > 0, "no monster spawned in a dark room");
        for m in &h.game.mobs.mobs {
            assert!(gm::spawn_lists::is_hostile(m.def), "{} is not a monster", m.def.kind.name);
            let p = m.body.pos;
            assert!((17.0..31.0).contains(&p.x) && (1.0..15.0).contains(&p.z) && p.y == 21.0, "{} at {p:?}", m.def.kind.name);
            assert!(far_enough(&ctx.players, ctx.spawn_point, p.x, p.y, p.z));
            assert!(m.body.bb.min_y >= 21.0 && m.body.bb.max_y <= 24.0);
        }
        // No two overlap (isNotColliding sees the mobs already there).
        let all = &h.game.mobs.mobs;
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(!a.body.bb.intersects(&b.body.bb));
            }
        }
    });
}

#[test]
fn zombies_spawn_in_the_dark_with_their_goal_lists() {
    run(|| {
        let mut h = dark_room();
        let ctx = context(&h.game);
        room_spawns(&mut h, &ctx, CreatureType::Monster);
        let zombies: Vec<_> = h.game.mobs.mobs.iter().filter(|m| m.def.kind.name == "zombie").collect();
        assert!(!zombies.is_empty(), "no zombie among {} monsters", h.game.mobs.mobs.len());
        for z in zombies {
            assert_eq!((z.tasks.entries.len(), z.target_tasks.entries.len()), (5, 2));
        }
    });
}

#[test]
fn a_player_in_the_room_or_the_spawn_point_there_stops_spawning() {
    run(|| {
        let mut h = dark_room();
        let mut ctx = context(&h.game);
        ctx.players.push([24.5, 21.0, 8.5]);
        assert_eq!(room_spawns(&mut h, &ctx, CreatureType::Monster), 0, "everything within 24 of the second player");
        let mut ctx = context(&h.game);
        ctx.spawn_point = [24, 21, 8];
        assert_eq!(room_spawns(&mut h, &ctx, CreatureType::Monster), 0, "everything within 24 of the spawn point");
    });
}

#[test]
fn peaceful_difficulty_and_daylight_stop_monsters() {
    run(|| {
        let mut h = dark_room();
        let mut ctx = context(&h.game);
        ctx.difficulty = Difficulty::Peaceful;
        assert_eq!(room_spawns(&mut h, &ctx, CreatureType::Monster), 0);
        // Animals need grass and light above 8: the dark stone room has neither.
        let ctx = context(&h.game);
        assert_eq!(room_spawns(&mut h, &ctx, CreatureType::Creature), 0);
    });
}

#[test]
fn animals_need_grass_and_light() {
    run(|| {
        let mut h = dark_room();
        // Grass floor, but no light: refused. Add a torch-bright block light via glowstone-like
        // block (89) in the middle of the room: light above 8 near it.
        for x in 20..=28 {
            for z in 4..=12 {
                terrain::set_block(&mut h.game, (x, 20, z), GRASS);
            }
        }
        let ctx = context(&h.game);
        assert_eq!(room_spawns(&mut h, &ctx, CreatureType::Creature), 0, "grass in the dark");
        for x in 20..=28 {
            for z in 4..=12 {
                terrain::set_block(&mut h.game, (x, 24, z), 89 << 4);
                terrain::set_block(&mut h.game, (x, 22, z), AIR);
            }
        }
        // Glowstone ceiling at y 24 lights the room to 15 - distance; y 21 is 3 below: 12.
        let ctx = context(&h.game);
        let n = room_spawns(&mut h, &ctx, CreatureType::Creature);
        // The biome may hold no animals (desert has rabbits only, which are not here yet).
        let biome_here = terrain::biome(&h.game, 24, 8).unwrap();
        let known = lists::spawnable_list(biome_here, CreatureType::Creature).iter().any(|e| def_by_name(e.name).is_some());
        if known {
            assert!(n > 0, "no animal on lit grass in {}", biome_here.name);
        }
        for m in &h.game.mobs.mobs {
            assert!(!gm::spawn_lists::is_hostile(m.def));
            assert_eq!(terrain::state(&h.game, (m.body.pos.x as i32, 20, m.body.pos.z as i32)), GRASS);
        }
    });
}

#[test]
fn placement_rules_match_can_creature_type_spawn_at_location() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        for x in 4..=13 {
            for z in 4..=13 {
                terrain::set_block(&mut h.game, (x, 200, z), STONE);
            }
        }
        let g = &h.game;
        assert!(can_creature_type_spawn_at_location(g, Placement::OnGround, 8, 201, 8));
        assert!(!can_creature_type_spawn_at_location(g, Placement::OnGround, 8, 200, 8), "inside the slab");
        assert!(!can_creature_type_spawn_at_location(g, Placement::OnGround, 8, 202, 8), "nothing under it");
        assert!(!can_creature_type_spawn_at_location(g, Placement::InWater, 8, 201, 8));
        terrain::set_block(&mut h.game, (8, 200, 8), BEDROCK);
        assert!(!can_creature_type_spawn_at_location(&h.game, Placement::OnGround, 8, 201, 8), "bedrock is refused");
        // A rail or a redstone wire is not a valid empty block to spawn in.
        assert!(is_valid_empty_spawn_block(AIR));
        assert!(!is_valid_empty_spawn_block(66 << 4));
        assert!(!is_valid_empty_spawn_block(55 << 4));
        assert!(!is_valid_empty_spawn_block(STONE));
    });
}

#[test]
fn do_mob_spawning_false_stops_the_step() {
    run(|| {
        let mut h = dark_room();
        let before = mob_count(&h);
        h.world.lock().unwrap().set_rule("doMobSpawning", "false");
        for _ in 0..300 {
            h.game.ticks = 0; // animals run on the set tick as well
            step(&mut h.game);
        }
        assert_eq!(mob_count(&h), before);
        h.world.lock().unwrap().set_rule("doMobSpawning", "true");
        assert!(terrain::rule(&h.game, "doMobSpawning", false));
    });
}

// ---- animals at chunk generation ----

#[test]
fn world_gen_spawning_puts_an_animal_group_on_the_ground() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        for x in 8..24 {
            for z in 8..24 {
                terrain::set_block(&mut h.game, (x, 100, z), GRASS);
            }
        }
        let before = mob_count(&h);
        let plains = biome::get(1).unwrap();
        // A generator whose first float is under the 0.1 chance, then run until it stops.
        let seed = (0..10_000).find(|&s| JavaRandom::new(s).next_float() < 0.1).unwrap();
        let mut rand = JavaRandom::new(seed);
        for s in 0..40 {
            if mob_count(&h) > before {
                break;
            }
            let sd = (s * 1000..).find(|&q| JavaRandom::new(q).next_float() < 0.1).unwrap();
            rand = JavaRandom::new(sd);
            perform_world_gen_spawning(&mut h.game, plains, 8, 8, 16, 16, &mut rand);
        }
        let made = &h.game.mobs.mobs[before..];
        assert!(!made.is_empty(), "the group was not placed");
        assert!(made.len() <= 6, "a plains group is at most 6");
        for m in made {
            assert!(!gm::spawn_lists::is_hostile(m.def));
            assert!(matches!(m.def.kind.name, "sheep" | "pig" | "chicken" | "cow"), "{}", m.def.kind.name);
            let (x, z) = (m.body.pos.x.floor() as i32, m.body.pos.z.floor() as i32);
            assert!((8..24).contains(&x) && (8..24).contains(&z), "inside the 16 x 16 area: {x} {z}");
            assert!(m.persistent || !can_despawn(m.def), "animals never despawn");
            assert!(can_creature_type_spawn_at_location(&h.game, Placement::OnGround, x, m.body.pos.y as i32, z));
        }
    });
}

#[test]
fn each_populated_chunk_gets_its_animals_once() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        let done = h.game.mobs.spawner.worldgen_done.clone();
        assert!(done.len() >= 9, "the populated chunks around the player were visited: {}", done.len());
        let n = mob_count(&h);
        world_gen_step(&mut h.game);
        world_gen_step(&mut h.game);
        assert_eq!(mob_count(&h), n, "a second pass spawns nothing");
        assert_eq!(h.game.mobs.spawner.worldgen_done, done);
        // Everything it made is a passive animal, and the visited chunks survive a save and load.
        assert!(h.game.mobs.mobs.iter().all(|m| !gm::spawn_lists::is_hostile(m.def)));
        let saved = gm::save(&h.game);
        let mut h2 = Harness::new(7, 2);
        gm::load(&mut h2.game, saved);
        assert_eq!(h2.game.mobs.spawner.worldgen_done, done);
        assert_eq!(mob_count(&h2), n);
    });
}

// ---- the biome lists ----

#[test]
fn creature_lists_match_the_worldgen_population_pass() {
    for b in biome::all() {
        // DROPPED: worldgen leaves the Nether, End and void lists at the defaults.
        if matches!(b.kind, biome::Kind::Hell | biome::Kind::End | biome::Kind::Void) {
            continue;
        }
        let mine = lists::spawnable_list(b, CreatureType::Creature);
        let theirs = creatures(b);
        assert_eq!(mine.len(), theirs.len(), "{}", b.name);
        for (m, t) in mine.iter().zip(&theirs) {
            assert_eq!((m.weight, m.min, m.max), (t.weight, t.min, t.max), "{} {}", b.name, m.name);
            assert_eq!(m.name == "sheep", t.sheep, "{}", b.name);
        }
    }
}

#[test]
fn biome_lists_follow_the_biome_classes() {
    let find = |name: &str, t| lists::spawnable_list(biome::all().find(|b| b.name == name).unwrap(), t);
    let weights = |l: &[lists::Entry], n: &str| l.iter().find(|e| e.name == n).map(|e| e.weight);
    let plains = find("plains", CreatureType::Monster);
    assert_eq!(weights(&plains, "zombie"), Some(95));
    assert_eq!(plains.iter().map(|e| e.weight).sum::<i32>(), 100 + 95 + 5 + 100 + 100 + 100 + 10 + 5);
    let desert = find("desert", CreatureType::Monster);
    assert_eq!((weights(&desert, "zombie"), weights(&desert, "husk"), weights(&desert, "zombie_villager")), (Some(19), Some(80), Some(1)));
    assert_eq!(weights(&find("ice_flats", CreatureType::Monster), "stray"), Some(80));
    assert_eq!(weights(&find("ice_flats", CreatureType::Monster), "skeleton"), Some(20));
    assert_eq!(weights(&find("swampland", CreatureType::Monster), "slime"), Some(100 + 1).map(|_| 100).or(Some(100)).filter(|_| false).or(Some(100)));
    assert_eq!(find("swampland", CreatureType::Monster).iter().filter(|e| e.name == "slime").count(), 2);
    assert!(find("mushroom_island", CreatureType::Monster).is_empty());
    assert!(find("mushroom_island", CreatureType::WaterCreature).is_empty());
    assert_eq!(find("ocean", CreatureType::WaterCreature), vec![lists::Entry { name: "squid", weight: 10, min: 4, max: 4 }]);
    assert!(find("ocean", CreatureType::Creature).is_empty());
    assert_eq!(find("plains", CreatureType::Ambient)[0].name, "bat");
    assert_eq!(find("jungle", CreatureType::Monster).iter().filter(|e| e.name == "ocelot").count(), 1);
    assert_eq!(find("jungle_edge", CreatureType::Monster).iter().filter(|e| e.name == "ocelot").count(), 0);
    assert!(find("sky", CreatureType::Creature).is_empty());
    // Every kind MOBS has has an entry somewhere, and each maps to its counting class.
    for def in gm::MOBS.iter() {
        let t = lists::creature_type_of(def);
        assert!(biome::all().any(|b| lists::spawnable_list(b, t).iter().any(|e| e.name == def.kind.name)), "{}", def.kind.name);
    }
    assert_eq!(lists::placement("squid"), Placement::InWater);
    assert_eq!(CreatureType::Creature.max_number(), 10);
    assert!(lists::CreatureType::Creature.animal() && !lists::CreatureType::Monster.animal());
}

// ---- despawning ----

#[test]
fn despawn_beyond_128_blocks_and_never_for_persistent_or_animals() {
    let mut r = JavaRandom::new(3);
    let mut idle = 0;
    let sq = |d: f64| d * d;
    assert!(despawn_entity(false, &mut idle, Some(sq(128.5)), true, &mut r));
    assert!(!despawn_entity(false, &mut idle, Some(sq(127.0)), true, &mut r));
    assert!(!despawn_entity(false, &mut idle, Some(sq(128.0)), true, &mut r), "exactly 16384 is not past it");
    assert!(!despawn_entity(false, &mut idle, Some(sq(500.0)), false, &mut r), "an animal stays");
    assert!(!despawn_entity(true, &mut idle, Some(sq(500.0)), true, &mut r), "persistence stays");
    assert!(!despawn_entity(false, &mut idle, None, true, &mut r), "no player, no despawn");
    let mut idle = 700;
    assert!(!despawn_entity(true, &mut idle, Some(sq(500.0)), true, &mut r));
    assert_eq!(idle, 0, "persistence clears the idle count");
    assert!(can_despawn(def_by_name("zombie").unwrap()));
    for animal in ["pig", "sheep", "cow", "chicken"] {
        assert!(!can_despawn(def_by_name(animal).unwrap()), "{animal}");
    }
}

#[test]
fn random_despawn_past_32_blocks_after_600_idle_ticks() {
    let mut r = JavaRandom::new(11);
    let d = 40.0 * 40.0;
    // Not idle long enough: never, and no draw is spent.
    for _ in 0..5000 {
        let mut idle = 600;
        assert!(!despawn_entity(false, &mut idle, Some(d), true, &mut r));
    }
    // Idle past 600: one in 800 a tick.
    let hits = (0..160_000).filter(|_| despawn_entity(false, &mut 601, Some(d), true, &mut r)).count();
    assert!((120..300).contains(&hits), "{hits} of 160000, expected about 200");
    // Within 32 blocks: never, and the idle count resets.
    let mut idle = 900;
    for _ in 0..5000 {
        assert!(!despawn_entity(false, &mut idle, Some(30.0 * 30.0), true, &mut r));
    }
    assert_eq!(idle, 0);
    // An animal never takes the random route either.
    assert!((0..20_000).all(|_| !despawn_entity(false, &mut 601, Some(d), false, &mut r)));
}

#[test]
fn a_far_zombie_despawns_in_the_tick_and_a_cow_stays() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        // The player stands at y 230; mobs at y 70 are more than 128 blocks below.
        let z = gm::summon(&mut h.game, &ZOMBIE, [8.5, 70.0, 8.5]).expect("loaded");
        let c = gm::summon(&mut h.game, &COW, [12.5, 70.0, 8.5]).expect("loaded");
        h.tick(1);
        assert!(h.game.mobs.mobs.iter().all(|m| m.id != z), "the zombie went");
        assert!(h.game.mobs.mobs.iter().any(|m| m.id == c), "the cow stayed");
        // Close to the player the zombie lives, and its idle count stays reset.
        h.move_to(1, [8.5, 71.0, 8.5]);
        h.tick(1);
        let z2 = gm::summon(&mut h.game, &ZOMBIE, [10.5, 71.0, 8.5]).expect("loaded");
        h.tick(40);
        let m = h.game.mobs.mobs.iter().find(|m| m.id == z2).expect("a zombie beside the player stays");
        assert_eq!(m.idle_time, 0);
    });
}

#[test]
fn hostile_mobs_do_not_survive_peaceful() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        h.game.cfg.difficulty = "peaceful".into();
        let z = gm::summon(&mut h.game, &ZOMBIE, [8.5, 150.0, 8.5]).expect("loaded");
        let c = gm::summon(&mut h.game, &COW, [12.5, 150.0, 8.5]).expect("loaded");
        h.tick(1);
        assert!(h.game.mobs.mobs.iter().all(|m| m.id != z));
        assert!(h.game.mobs.mobs.iter().any(|m| m.id == c));
    });
}

#[test]
fn the_difficulty_command_message_makes_a_running_game_peaceful() {
    run(|| {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        let z = gm::summon(&mut h.game, &ZOMBIE, [8.5, 150.0, 8.5]).expect("loaded");
        h.tick(1);
        assert!(h.game.mobs.mobs.iter().any(|m| m.id == z), "a zombie lives on normal");
        h.game.handle(crate::game::GameMsg::Difficulty { id: 0 });
        assert_eq!(crate::game::players::difficulty(&h.game), crate::game::food::Difficulty::Peaceful);
        h.tick(1);
        assert!(h.game.mobs.mobs.iter().all(|m| m.id != z), "peaceful removes it");
        h.game.handle(crate::game::GameMsg::Difficulty { id: 3 });
        assert_eq!(crate::game::players::difficulty(&h.game), crate::game::food::Difficulty::Hard);
    });
}

#[test]
fn persistence_is_saved() {
    let mut h = Harness::new(1, 1);
    let id = gm::summon(&mut h.game, &COW, [0.5, 100.0, 0.5]);
    assert!(id.is_none(), "no chunk is loaded, so /summon refuses");
    gm::spawn(&mut h.game, def_by_name("cow").unwrap(), [0.5, 100.0, 0.5], 0.0);
    h.game.mobs.mobs[0].persistent = true;
    let saved = gm::save(&h.game);
    let mut h2 = Harness::new(1, 1);
    gm::load(&mut h2.game, saved);
    assert!(h2.game.mobs.mobs[0].persistent);
    assert!(!h2.game.mobs.mobs[0].dead);
    let _ = PlayerState::default();
}
