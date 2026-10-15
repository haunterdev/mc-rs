use super::*;
use crate::collision::tests::Blocks;
use worldgen::blocks::{state, STONE, WATER};

fn standing(w: &Blocks, y: f64) -> Player {
    let mut p = Player::new(DVec3::new(0.5, y, 0.5), 0.0, 0.0);
    for _ in 0..40 {
        p.tick(w, &Keys::default());
    }
    p
}

#[test]
fn falls_and_lands_on_the_floor() {
    let w = Blocks::floor_at(63, 8);
    let p = standing(&w, 70.0);
    assert!(p.on_ground);
    assert_eq!(p.pos.y, 64.0);
    // onLanded zeroes it, then travel's gravity runs after the move, so a standing player
    // carries one tick of it.
    assert_eq!(p.motion.y, -0.08 * 0.9800000190734863);
}

#[test]
fn jump_starts_at_042_and_peaks_near_125() {
    let w = Blocks::floor_at(63, 8);
    let mut p = standing(&w, 64.0);
    let jump = Keys { jump: true, ..Keys::default() };
    p.tick(&w, &jump);
    // The jump's 0.42 is applied and moved in the same tick, then gravity takes its share.
    assert!((p.pos.y - (64.0 + 0.42f32 as f64)).abs() < 1e-9, "{}", p.pos.y);
    let mut peak = p.pos.y;
    for _ in 0..20 {
        p.tick(&w, &Keys::default());
        peak = peak.max(p.pos.y);
    }
    assert!((peak - 64.0 - 1.252).abs() < 0.01, "{peak}");
    assert!(p.on_ground && p.pos.y == 64.0);
}

#[test]
fn steps_up_half_a_block_but_not_a_full_one() {
    let mut w = Blocks::floor_at(63, 16);
    // A bottom slab's height: snow meta 4 is five layers, and its collision box is one layer lower, 0.5.
    for x in 2..=16 {
        for z in -16..=16 {
            w.set(x, 64, z, state(worldgen::blocks::id::SNOW_LAYER, 4));
        }
    }
    let mut p = standing(&w, 64.0);
    p.auto_jump = false;
    // Yaw -90 faces +x.
    p.yaw = -90.0;
    let walk = Keys { forward: true, ..Keys::default() };
    for _ in 0..30 {
        p.tick(&w, &walk);
    }
    assert!(p.pos.x > 3.0 && p.pos.y == 64.5, "{} {}", p.pos.x, p.pos.y);

    let mut wall = Blocks::floor_at(63, 8);
    for z in -8..=8 {
        wall.set(2, 64, z, STONE);
    }
    let mut q = standing(&wall, 64.0);
    q.auto_jump = false;
    q.yaw = -90.0;
    for _ in 0..30 {
        q.tick(&wall, &walk);
    }
    assert!((q.pos.x - (2.0 - 0.3)).abs() < 1e-6 && q.pos.y == 64.0, "{} {}", q.pos.x, q.pos.y);
    assert!(q.collided_horizontally);
}

#[test]
fn auto_jump_climbs_a_full_block() {
    let mut w = Blocks::floor_at(63, 16);
    for x in 2..=16 {
        for z in -16..=16 {
            w.set(x, 64, z, STONE);
        }
    }
    let mut p = standing(&w, 64.0);
    p.yaw = -90.0;
    let walk = Keys { forward: true, ..Keys::default() };
    for _ in 0..40 {
        p.tick(&w, &walk);
    }
    assert!(p.pos.x > 2.5 && p.pos.y == 65.0, "{} {}", p.pos.x, p.pos.y);
}

#[test]
fn sneaking_stops_at_the_edge() {
    // A floor that ends at x = 1.
    let mut w = Blocks::default();
    for x in -8..1 {
        for z in -8..=8 {
            w.set(x, 63, z, STONE);
        }
    }
    let mut p = standing(&w, 64.0);
    p.yaw = -90.0;
    let sneak = Keys { forward: true, sneak: true, ..Keys::default() };
    for _ in 0..60 {
        p.tick(&w, &sneak);
    }
    assert!(p.on_ground && p.pos.y == 64.0);
    // The box may hang out by less than its half width plus one 0.05 step.
    assert!(p.pos.x > 1.0 && p.pos.x < 1.0 + 0.3, "{}", p.pos.x);
    assert_eq!(p.height, 1.65);
    assert!((p.eye_height() - 1.54).abs() < 1e-6);
}

#[test]
fn double_jump_toggles_flight_and_landing_ends_it() {
    let w = Blocks::floor_at(63, 8);
    let mut p = standing(&w, 64.0);
    let jump = Keys { jump: true, ..Keys::default() };
    let none = Keys::default();
    p.tick(&w, &jump);
    p.tick(&w, &none);
    p.tick(&w, &jump);
    assert!(p.flying);
    for _ in 0..10 {
        p.tick(&w, &jump);
    }
    assert!(p.flying && p.pos.y > 66.0, "{}", p.pos.y);
    let down = Keys { sneak: true, ..Keys::default() };
    for _ in 0..80 {
        p.tick(&w, &down);
    }
    assert!(!p.flying && p.on_ground);
    assert!((p.fov_modifier() - 1.0).abs() < 1e-6);
}

#[test]
fn walking_bobs_and_standing_does_not() {
    let w = Blocks::floor_at(63, 8);
    let mut p = standing(&w, 64.0);
    // cameraPitch decays from the landing by 0.2 a tick and never reaches exactly zero.
    assert!(p.bobbing(0.5).abs_diff_eq(Mat4::IDENTITY, 1e-6));
    let walk = Keys { forward: true, ..Keys::default() };
    for _ in 0..10 {
        p.tick(&w, &walk);
    }
    assert!(p.distance_walked_modified > 0.5 && p.camera_yaw > 0.05, "{} {}", p.distance_walked_modified, p.camera_yaw);
    assert!(!p.bobbing(0.5).abs_diff_eq(Mat4::IDENTITY, 1e-3));
}

/// FNV-1a over the f64 bits, so the hash is the same on every build.
fn fnv(h: &mut u64, v: f64) {
    for b in v.to_bits().to_le_bytes() {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

/// Runs `ticks` ticks with the keys `script` gives for each, hashing `pos` and `motion` every tick.
fn trace(w: &Blocks, start: DVec3, yaw: f32, ticks: usize, script: impl Fn(usize) -> Keys) -> u64 {
    let mut p = Player::new(start, yaw, 0.0);
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for t in 0..ticks {
        p.tick(w, &script(t));
        for v in [p.pos.x, p.pos.y, p.pos.z, p.motion.x, p.motion.y, p.motion.z] {
            fnv(&mut h, v);
        }
    }
    h
}

/// Guards the movement code through the move into `crates/sim`: four scripted 300-tick runs
/// whose hashes were recorded on the code before it moved.
#[test]
fn golden_traces() {
    let walk = Keys { forward: true, ..Keys::default() };
    // Into a wall, up half-slab steps on the way.
    let mut steps = Blocks::floor_at(63, 24);
    for x in 3..=5 {
        for z in -24..=24 {
            steps.set(x, 64, z, state(worldgen::blocks::id::SNOW_LAYER, 4));
        }
    }
    for z in -24..=24 {
        steps.set(10, 64, z, STONE);
        steps.set(10, 65, z, STONE);
    }
    let a = trace(&steps, DVec3::new(0.5, 64.0, 0.3), -90.0, 300, |t| if t < 20 { Keys::default() } else { walk });
    // Sprint-jumping across a three-block gap.
    let mut gap = Blocks::default();
    for x in -24..=24 {
        for z in -4..=4 {
            if !(0..3).contains(&x) {
                gap.set(x, 63, z, STONE);
            }
        }
    }
    let b = trace(&gap, DVec3::new(-6.5, 64.0, 0.5), -90.0, 300, |t| Keys {
        forward: t >= 10,
        sprint: t >= 10,
        jump: (14..18).contains(&t) || (40..44).contains(&t),
        ..Keys::default()
    });
    // Swimming in a pool: in, along the bottom, up and out against the wall.
    let mut pool = Blocks::floor_at(58, 12);
    for x in -6..=6 {
        for z in -6..=6 {
            for y in 59..63 {
                pool.set(x, y, z, WATER);
            }
        }
    }
    for z in -12..=12 {
        for y in 59..64 {
            pool.set(7, y, z, STONE);
        }
    }
    let c = trace(&pool, DVec3::new(0.5, 64.0, 0.5), -90.0, 300, |t| Keys {
        forward: t >= 30,
        jump: (60..120).contains(&t) || t >= 200,
        ..Keys::default()
    });
    // Sneaking off an edge, then letting go and walking off it.
    let mut edge = Blocks::default();
    for x in -12..1 {
        for z in -12..=12 {
            edge.set(x, 63, z, STONE);
        }
    }
    for x in -12..=12 {
        for z in -12..=12 {
            edge.set(x, 58, z, STONE);
        }
    }
    let d = trace(&edge, DVec3::new(-3.5, 64.0, 0.5), -80.0, 300, |t| Keys {
        forward: t >= 10,
        sneak: t < 150,
        ..Keys::default()
    });
    let got = [a, b, c, d];
    assert_eq!(got, GOLDEN, "the movement traces changed: {got:#x?}");
}

const GOLDEN: [u64; 4] = [0x56ab_7a3d_8536_ea78, 0x9cb5_6661_d9c8_98cf, 0xed06_8cf0_4ddd_6024, 0x94ba_df1e_faad_add4];

#[test]
fn turn_clamps_pitch_and_carries_the_previous_angle() {
    let mut p = Player::new(DVec3::ZERO, 10.0, 0.0);
    p.turn(100.0, -1000.0);
    assert_eq!(p.yaw, 25.0);
    assert_eq!(p.pitch, 90.0);
    assert_eq!(p.prev_yaw, 25.0);
    assert_eq!(p.prev_pitch, 90.0);
    let l = look(0.0, -90.0);
    assert!((l - Vec3::X).length() < 1e-3, "{l}");
}

#[test]
fn survival_cannot_double_jump_into_flight() {
    let w = Blocks::floor_at(63, 8);
    let mut p = standing(&w, 64.0);
    p.game_mode = proto::GM_SURVIVAL;
    let jump = Keys { jump: true, ..Keys::default() };
    p.tick(&w, &jump);
    p.tick(&w, &Keys::default());
    p.tick(&w, &jump);
    assert!(!p.flying && !p.allow_flying());
}

#[test]
fn hunger_at_six_stops_the_sprint_unless_flying_is_allowed() {
    let w = Blocks::floor_at(63, 16);
    let run = Keys { forward: true, sprint: true, ..Keys::default() };
    for (mode, food, sprints) in [
        (proto::GM_SURVIVAL, 20, true),
        (proto::GM_SURVIVAL, 7, true),
        (proto::GM_SURVIVAL, 6, false),
        (proto::GM_ADVENTURE, 0, false),
        (proto::GM_CREATIVE, 0, true),
    ] {
        let mut p = standing(&w, 64.0);
        p.game_mode = mode;
        p.food_level = food;
        for _ in 0..5 {
            p.tick(&w, &run);
        }
        assert_eq!(p.sprinting, sprints, "mode {mode} food {food}");
    }
    // Running out of food mid-sprint ends it on the next tick.
    let mut p = standing(&w, 64.0);
    p.game_mode = proto::GM_SURVIVAL;
    for _ in 0..5 {
        p.tick(&w, &run);
    }
    assert!(p.sprinting);
    p.food_level = 6;
    p.tick(&w, &run);
    assert!(!p.sprinting);
}

#[test]
fn a_velocity_frame_sets_the_motion() {
    let mut p = Player::new(DVec3::new(0.5, 64.0, 0.5), 0.0, 0.0);
    p.motion = DVec3::new(1.0, 1.0, 1.0);
    p.set_velocity([-0.25, 0.4, 0.5]);
    assert_eq!(p.motion, DVec3::new(-0.25, 0.4f32 as f64, 0.5));
}
