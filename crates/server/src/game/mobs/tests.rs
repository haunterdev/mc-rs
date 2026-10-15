//! Mobs (T9a) over a seeded world: a stone slab at y 200 to 201 under x, z 4..=13, the player at
//! the west edge and a cow summoned above the east edge.

use proto::{PlayerState, ON_GROUND};
use sim::kinds::COW;

use crate::game::mobs as gm;
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;
const ABOVE: [f64; 3] = [12.5, 206.0, 12.5];

fn setup() -> Harness {
    let mut h = Harness::new(7, 2);
    // These tests count the mobs they summon: natural spawning is spawner_tests' business.
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

fn tags(frames: &[(u8, Vec<u8>)]) -> Vec<u8> {
    frames.iter().map(|f| f.0).collect()
}

#[test]
fn a_summoned_cow_falls_and_comes_to_rest() {
    run(|| {
        let mut h = setup();
        let id = gm::summon(&mut h.game, &COW, ABOVE).expect("the chunk is loaded");
        assert_eq!(h.game.stats.mobs, 1);
        let mut rest = None;
        for n in 1..=100 {
            h.tick(1);
            let m = &h.game.mobs.mobs[0];
            if m.body.on_ground && rest.is_none() {
                rest = Some(n);
            }
        }
        let m = &h.game.mobs.mobs[0];
        assert_eq!(m.id, id);
        assert!(rest.is_some(), "the cow never landed");
        assert_eq!(m.body.pos.y, 201.0);
        // Each tick moves into the floor, zeroes motion, then gravity makes it (0 - 0.08) * 0.98.
        assert!((m.body.motion.y + 0.0784).abs() < 1e-6);
        assert_eq!((m.body.pos.x, m.body.pos.z), (12.5, 12.5), "the cow wanders only after a random delay");
        assert_eq!((m.body.width, m.body.height), (0.9, 1.4));
    });
}

#[test]
fn a_mob_below_the_world_is_removed() {
    run(|| {
        let mut h = setup();
        gm::summon(&mut h.game, &COW, ABOVE).unwrap();
        h.game.mobs.mobs[0].body.set_position(glam::DVec3::new(12.5, -70.0, 12.5));
        h.tick(1);
        assert!(h.game.mobs.mobs.is_empty());
    });
}

#[test]
fn mobs_survive_save_and_load() {
    run(|| {
        let mut h = setup();
        gm::summon(&mut h.game, &COW, ABOVE).unwrap();
        h.tick(60);
        h.game.mobs.mobs[0].health = 7.5;
        h.game.mobs.mobs[0].head_yaw = 33.0;
        let text = serde_json::to_string(&gm::save(&h.game)).unwrap();
        let before = h.game.mobs.mobs[0].clone();
        assert!(before.body.on_ground);

        let mut g = Harness::new(7, 2);
        gm::load(&mut g.game, serde_json::from_str(&text).unwrap());
        assert_eq!(g.game.mobs.mobs.len(), 1);
        assert_eq!(g.game.stats.mobs, 1);
        let m = &g.game.mobs.mobs[0];
        assert_eq!(m.def.kind.name, "cow");
        assert_eq!(m.body.pos, before.body.pos);
        assert_eq!(m.body.motion, before.body.motion);
        assert_eq!((m.health, m.head_yaw, m.body.on_ground), (7.5, 33.0, true));
        assert_eq!((m.body.bb.max_y - m.body.bb.min_y) as f32, 1.4);
    });
}

#[test]
fn the_tracker_spawns_moves_and_destroys() {
    run(|| {
        let mut h = setup();
        let id = gm::summon(&mut h.game, &COW, ABOVE).unwrap();
        h.tick(1);
        let first = h.frames_for(1);
        assert!(tags(&first).contains(&proto::S_SPAWN_MOB), "{:?}", tags(&first));
        let spawn = first.iter().find(|f| f.0 == proto::S_SPAWN_MOB).unwrap();
        let s = proto::SpawnMob::decode(&spawn.1).unwrap();
        assert_eq!((s.id, s.kind), (id, proto::MOB_COW));
        assert_eq!(s.pos, [12.5, h.game.mobs.mobs[0].body.pos.y, 12.5].map(|v| v));
        // The fall is moves, every third tick, and no second spawn.
        h.tick(40);
        let falling = h.frames_for(1);
        let moves: Vec<_> = falling.iter().filter(|f| f.0 == proto::S_ENTITY_MOVE).flat_map(|f| proto::decode_entity_moves(&f.1).unwrap()).collect();
        assert!(!moves.is_empty());
        assert!(moves.iter().all(|m| m.id == id));
        assert_eq!(moves.last().unwrap().pos[1], 201.0, "the last move is the rest");
        assert!(!tags(&falling).contains(&proto::S_SPAWN_MOB));
        // The player walks out of the range: the cow is destroyed for it.
        h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [400.0, 201.0, 400.0], flags: ON_GROUND, ..Default::default() });
        h.tick(2);
        let out = h.frames_for(1);
        let d = out.iter().find(|f| f.0 == proto::S_DESTROY).expect("a destroy");
        assert_eq!(proto::decode_destroy(&d.1).unwrap(), vec![id]);
    });
}

#[test]
fn changed_metadata_is_sent_to_trackers() {
    run(|| {
        let mut h = setup();
        gm::summon(&mut h.game, &COW, ABOVE).unwrap();
        h.tick(2);
        h.frames_for(1);
        h.game.mobs.mobs[0].set_fire(8);
        h.tick(1);
        let out = h.frames_for(1);
        let m = out.iter().find(|f| f.0 == proto::S_ENTITY_META).expect("meta frame");
        let meta = proto::EntityMeta::decode(&m.1).unwrap();
        assert_eq!(meta.payload, proto::MetaPayload::Mob(proto::MobMeta { flags: proto::MM_BURNING, ..Default::default() }));
    });
}
