//! ESM-10: the day arithmetic, the monsters a hard-day night places, and the refusals.
//!
//! The step is called directly instead of through the tick: the natural spawner shares the tick and
//! spawns in the same dark hours, so a ticked world would not show what the hard days placed by
//! themselves. The harness reseeds the three generators the step draws from, so a run of calls is
//! the same in every test run.

use super::*;
use crate::game::testing::{run, Harness};

/// Calls the step `n` times, the number of rolls a night gives it.
const NIGHT_ROLLS: usize = 20_000;

#[test]
fn hard_days_come_every_eighth_day() {
    assert!(hard_day(8, HARD_DAY_CYCLE), "day 8 is a hard day");
    assert!(!hard_day(7, HARD_DAY_CYCLE), "day 7 is not");
    assert!(!hard_day(0, HARD_DAY_CYCLE), "day 0 is never one");
    assert!(hard_day(16, HARD_DAY_CYCLE), "and day 16 is");
    assert!(!hard_day(8, NO_HARD_DAYS), "a cycle of 0 turns them off");
    // The state comes with the flag on and the default interval.
    let st = State::default();
    assert!(st.hard_days);
    assert_eq!(st.cycle, HARD_DAY_CYCLE);
    // The bed rule follows the same arithmetic.
    assert!(sleep_denied(8));
    assert!(!sleep_denied(7));
    assert!(!sleep_denied(0));
}

/// The first column near the origin a monster could stand on whose surroundings are land: the step
/// draws its spot from a 48 block box, so a coast would leave it water on most draws. The candidate
/// whose box holds the most valid columns is taken.
fn stand_point(h: &mut Harness) -> [f64; 3] {
    let mut sites: Vec<(i32, i32, i32)> = Vec::new();
    for x in -40..40 {
        for z in -40..40 {
            let Some(y) = terrain::top_solid_or_liquid(&mut h.game, x, z) else { continue };
            if (2..=250).contains(&y)
                && spawner::can_creature_type_spawn_at_location(&h.game, Placement::OnGround, x, y, z)
            {
                sites.push((x, y, z));
            }
        }
    }
    assert!(sites.len() > 300, "the origin is not land: {} spawn sites", sites.len());
    let mut best = (0usize, sites[sites.len() / 2]);
    for &(x, y, z) in sites.iter().step_by(3) {
        let n = sites.iter().filter(|&&(sx, _, sz)| (sx - x).abs() <= SPAWN_BOX && (sz - z).abs() <= SPAWN_BOX).count();
        if n > best.0 {
            best = (n, (x, y, z));
        }
    }
    [best.1 .0 as f64, best.1 .1 as f64, best.1 .2 as f64]
}

/// A survival player standing on land at night on `day`, with the mob list cleared. The player
/// position the step saw comes back with the harness.
fn night_on(day: i64) -> (Harness, [f64; 3]) {
    let mut h = Harness::new(7, 3);
    h.join(1, "Xy", [8.5, 100.0, 8.5]);
    h.settle();
    let p = stand_point(&mut h);
    h.move_to(1, p);
    // The box is 48 blocks wide, so the chunks around the new spot settle before it is measured.
    h.settle();
    // 18000 is dusk: skylightSubtracted 11, so only lit ground refuses a monster.
    h.world.lock().unwrap().set_time(day * 24000 + 18000);
    // Whatever the tick placed belongs to the natural spawner; the step is measured alone.
    h.game.mobs.mobs.clear();
    let seen = spawner::player_positions(&h.game)[0];
    (h, seen)
}

/// Runs the step `NIGHT_ROLLS` times and asserts it placed nothing.
fn assert_no_spawns(h: &mut Harness) {
    for _ in 0..NIGHT_ROLLS {
        step(&mut h.game);
    }
    assert!(h.game.mobs.mobs.is_empty(), "{} mobs were placed", h.game.mobs.mobs.len());
}

#[test]
fn a_night_on_a_hard_day_spawns_monsters_around_the_player() {
    run(|| {
        let (mut h, p) = night_on(8);
        for _ in 0..NIGHT_ROLLS {
            step(&mut h.game);
        }
        let spawned: Vec<(&'static str, [f64; 3])> =
            h.game.mobs.mobs.iter().map(|m| (m.def.kind.name, [m.body.pos.x, m.body.pos.y, m.body.pos.z])).collect();
        assert!(!spawned.is_empty(), "a hard-day night placed no monster in {NIGHT_ROLLS} rolls");
        let (bx, by, bz) = (floor(p[0]), floor(p[1]), floor(p[2]));
        for (name, q) in &spawned {
            let def = def_by_name(name).expect("a mob the table holds");
            assert!(lists::is_hostile(def), "{name} is not a monster");
            // The spot is drawn from the box around the player.
            for (v, b) in [(q[0], bx), (q[1], by), (q[2], bz)] {
                let i = v as i32;
                assert!((b - SPAWN_BOX..=b + SPAWN_BOX - 1).contains(&i), "{name} at {q:?} is outside the box around {p:?}");
            }
            // Not within 8 blocks of the player.
            let (dx, dy, dz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
            assert!(
                dx * dx + dy * dy + dz * dz >= PLAYER_CLEARANCE * PLAYER_CLEARANCE,
                "{name} at {q:?} is within {PLAYER_CLEARANCE} blocks of {p:?}"
            );
            // On ground the placement rules accept.
            assert!(
                spawner::can_creature_type_spawn_at_location(&h.game, Placement::OnGround, q[0] as i32, q[1] as i32, q[2] as i32),
                "{name} at {q:?} does not stand on a spawn site"
            );
        }
    });
}

#[test]
fn a_night_off_a_hard_day_spawns_nothing() {
    run(|| {
        let (mut h, _) = night_on(7);
        assert_no_spawns(&mut h);
    });
}

/// The addon's tick hook is what runs the step: with `server.json` listing no addons, day 8 at night
/// spawns nothing, and with it listed the same rolls place monsters.
#[test]
fn addon_off_day_8_night_spawns_no_hard_day_mobs() {
    run(|| {
        let (mut h, _) = night_on(8);
        h.game.addons.enabled.clear();
        for _ in 0..NIGHT_ROLLS {
            crate::game::addons::tick(&mut h.game);
        }
        assert!(h.game.mobs.mobs.is_empty(), "{} mobs were placed", h.game.mobs.mobs.len());
        h.game.addons.enabled.push("epic-siege".into());
        for _ in 0..NIGHT_ROLLS {
            crate::game::addons::tick(&mut h.game);
        }
        assert!(!h.game.mobs.mobs.is_empty(), "the addon on places monsters");
    });
}

#[test]
fn peaceful_spawns_nothing_on_a_hard_day() {
    run(|| {
        let (mut h, _) = night_on(8);
        h.game.cfg.difficulty = "peaceful".into();
        assert_no_spawns(&mut h);
    });
}

#[test]
fn the_flag_turns_the_hard_days_off() {
    run(|| {
        let (mut h, _) = night_on(8);
        h.game.addons.epic_siege.hardday.hard_days = false;
        assert_no_spawns(&mut h);
    });
}

#[test]
fn the_512_entity_cap_stops_the_step() {
    run(|| {
        let (mut h, _) = night_on(8);
        // The cap counts every entity the world ticks, the player included.
        let cow = def_by_name("cow").unwrap();
        for _ in 0..ENTITY_CAP - 1 {
            spawn(&mut h.game, cow, [0.5, 250.0, 0.5], 0.0);
        }
        assert_eq!(entity_count(&h.game), ENTITY_CAP);
        for _ in 0..NIGHT_ROLLS {
            step(&mut h.game);
        }
        assert_eq!(h.game.mobs.mobs.len(), ENTITY_CAP - 1, "the cap left no room");
    });
}
