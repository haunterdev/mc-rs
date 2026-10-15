//! Mob sounds over a stone slab at y 200: a player at the west edge, another far away.

use proto::{PlayerState, ON_GROUND, S_SOUND};
use sim::kinds::{COW, CREEPER, ZOMBIE};

use super::ai::Tasks;
use super::summon;
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;

fn setup() -> Harness {
    let mut h = Harness::new(11, 2);
    {
        let mut w = h.world.lock().unwrap();
        w.set_rule("doMobSpawning", "false");
        w.set_rule("doDaylightCycle", "false");
        w.set_time(18000);
    }
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.join(2, "Far", [45.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=30 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state = Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.game.sessions.get_mut(&2).unwrap().state = Some(PlayerState { pos: [45.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    for id in [1, 2] {
        h.game.players.players.get_mut(&id).unwrap().respawn_invulnerable = 0;
    }
    h.tick(1);
    h.frames_for(1);
    h.frames_for(2);
    h
}

fn sounds(frames: &[(u8, Vec<u8>)]) -> Vec<proto::Sound> {
    frames.iter().filter(|f| f.0 == S_SOUND).map(|f| proto::Sound::decode(&f.1).expect("a sound")).collect()
}

#[test]
fn zombie_ambient_sound_reaches_a_near_player_only() {
    run(|| {
        let mut h = setup();
        let id = summon(&mut h.game, &ZOMBIE, [8.5, 201.0, 5.5]).unwrap();
        let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == id).unwrap();
        m.tasks = Tasks::new();
        m.target_tasks = Tasks::new();
        let (mut near, mut far) = (Vec::new(), Vec::new());
        for _ in 0..2000 {
            h.tick(1);
            near.extend(sounds(&h.frames_for(1)));
            far.extend(sounds(&h.frames_for(2)));
            if near.iter().any(|s| s.name == "entity.zombie.ambient") {
                break;
            }
        }
        let s = near.iter().find(|s| s.name == "entity.zombie.ambient").expect("an ambient sound");
        assert_eq!(s.category, 5);
        assert_eq!(s.volume, 1.0);
        assert!((0.8..=1.2).contains(&s.pitch));
        assert!(far.is_empty(), "the player 40 blocks away heard {far:?}");
    });
}

#[test]
fn ignited_creeper_primes_once_at_half_pitch() {
    run(|| {
        let mut h = setup();
        let id = summon(&mut h.game, &CREEPER, [8.5, 201.0, 5.5]).unwrap();
        let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == id).unwrap();
        m.tasks = Tasks::new();
        m.target_tasks = Tasks::new();
        m.ignite();
        let (mut near, mut far) = (Vec::new(), Vec::new());
        for _ in 0..40 {
            h.tick(1);
            near.extend(sounds(&h.frames_for(1)));
            far.extend(sounds(&h.frames_for(2)));
        }
        let primed: Vec<_> = near.iter().filter(|s| s.name == "entity.creeper.primed").collect();
        assert_eq!(primed.len(), 1);
        assert_eq!(primed[0].pitch, 0.5);
        assert_eq!(primed[0].volume, 1.0);
        assert!(far.iter().all(|s| s.name != "entity.creeper.primed"));
    });
}

#[test]
fn walking_cow_plays_step_sounds() {
    run(|| {
        let mut h = setup();
        let id = summon(&mut h.game, &COW, [8.5, 201.0, 5.5]).unwrap();
        let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == id).unwrap();
        m.tasks = Tasks::new();
        m.target_tasks = Tasks::new();
        let mut all = Vec::new();
        let mut far = Vec::new();
        for t in 0..200 {
            if t == 5 {
                let mut m = h.game.mobs.mobs.pop().unwrap();
                let w = crate::game::terrain::Blocks(&h.game);
                m.try_move_to(&w, 17.5, 201.0, 8.5, 1.0);
                h.game.mobs.mobs.push(m);
            }
            h.tick(1);
            all.extend(sounds(&h.frames_for(1)));
            far.extend(sounds(&h.frames_for(2)));
        }
        let steps = all.iter().filter(|s| s.name == "entity.cow.step").count();
        assert!(steps >= 3, "only {steps} step sounds in {all:?}");
        let s = all.iter().find(|s| s.name == "entity.cow.step").unwrap();
        assert_eq!((s.volume, s.pitch, s.category), (0.15, 1.0, 6));
        assert!(far.iter().all(|s| s.name != "entity.cow.step"));
    });
}
