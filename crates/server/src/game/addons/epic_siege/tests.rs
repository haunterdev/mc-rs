//! The ESM tests over the mob tests' slab world: stone at y 200 from x 4, z 4..=13, the player on
//! it at the west edge. A zombie is given the ESM setting under test by hand (`apply_with`), since
//! the mod's own settings all ship off.

use items::ids::item;
use proto::{PlayerState, ON_GROUND};
use sim::kinds::ZOMBIE;

use crate::game::mobs::{def_of, summon, Mob};
use super::config::{self, Config};
use crate::game::terrain;
use crate::game::testing::{run, Harness};

const STONE: u16 = 1 << 4;
const COBBLESTONE: u16 = 4 << 4;
const NIGHT: i64 = 18000;

/// The slab world at `time` of day, the clock stopped and natural spawning off. The slab runs to
/// x 45 so a test can put a mob 40 blocks from the player.
fn setup(time: i64) -> Harness {
    let mut h = Harness::new(7, 4);
    {
        let mut w = h.world.lock().unwrap();
        w.set_rule("doMobSpawning", "false");
        w.set_rule("doDaylightCycle", "false");
        w.set_time(time);
    }
    h.join(1, "Xy", [5.5, 230.0, 5.5]);
    h.settle();
    for x in 4..=45 {
        for z in 4..=13 {
            terrain::set_block(&mut h.game, (x, 200, z), STONE);
        }
    }
    h.game.sessions.get_mut(&1).unwrap().state =
        Some(PlayerState { pos: [5.5, 201.0, 5.5], flags: ON_GROUND, ..Default::default() });
    h.tick(1);
    h.frames_for(1);
    h
}

/// The state at a position in the running game.
fn state(h: &Harness, at: (i32, i32, i32)) -> u16 {
    terrain::state(&h.game, at)
}

fn mob(h: &Harness, id: u32) -> &Mob {
    h.game.mobs.mobs.iter().find(|m| m.id == id).expect("the mob")
}

#[test]
fn a_zombie_targets_a_player_40_blocks_away_through_a_wall() {
    run(|| {
        let mut h = setup(NIGHT);
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        // A wall straight across the slab between them: the zombie never sees the player.
        for y in 201..=203 {
            for z in 4..=13 {
                terrain::set_block(&mut h.game, (25, y, z), STONE);
            }
        }
        let z = summon(&mut h.game, &ZOMBIE, [45.5, 201.0, 5.5]).unwrap();
        {
            // 40 blocks is past the despawn range, and the target roll can take a few hundred ticks:
            // the zombie is made persistent so the wait is for the target and not the despawner.
            let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == z).unwrap();
            m.persistent = true;
        }
        let mut targeted = false;
        // The target roll is 1 in 10 on a goal pass every few ticks, so a few hundred ticks is the
        // normal wait; the loop is long enough that a run with no target at all means the zombie
        // cannot reach this far, which is what the test is for.
        for _ in 0..3000 {
            h.tick(1);
            if mob(&h, z).attack_target == Some(1) {
                targeted = true;
                break;
            }
        }
        assert!(targeted, "the zombie never took the player 40 blocks away behind the wall");
    });
}

#[test]
fn a_zombie_digs_through_a_stone_wall_to_reach_a_player() {
    run(|| {
        let mut h = setup(NIGHT);
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        // A one block thick wall across the slab, two tall: what a zombie's dig rays reach.
        for y in 201..=202 {
            for z in 4..=13 {
                terrain::set_block(&mut h.game, (9, y, z), STONE);
            }
        }
        let z = summon(&mut h.game, &ZOMBIE, [10.5, 201.0, 5.5]).unwrap();
        {
            let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == z).unwrap();
            super::apply_with(m, &Config { digging: true, ..config::DEFAULT });
            // AdditionDigger's own pickaxe roll, taken by hand so the test does not depend on it.
            m.main_hand = items::ItemStack::of(item::IRON_PICKAXE);
            // The target the dig goal needs, taken by hand: the roll that takes it is the test above.
            m.attack_target = Some(1);
        }
        let mut hole = false;
        for _ in 0..1200 {
            h.tick(1);
            if state(&h, (9, 201, 5)) == 0 || state(&h, (9, 202, 5)) == 0 {
                hole = true;
                break;
            }
        }
        assert!(hole, "the zombie never broke into the wall");
        // It walks through the hole it made. The measure is the zombie's own side of the wall: the
        // player it reaches can be killed and respawn, so a distance to the player says nothing.
        let mut through = false;
        for _ in 0..900 {
            h.tick(1);
            if mob(&h, z).body.pos.x < 8.5 {
                through = true;
                break;
            }
        }
        assert!(through, "never came through the hole, still at x {}", mob(&h, z).body.pos.x);
    });
}

#[test]
fn a_zombie_pillars_up_to_a_player_on_a_ledge() {
    run(|| {
        let mut h = setup(NIGHT);
        h.game.players.players.get_mut(&1).unwrap().respawn_invulnerable = 0;
        // A ledge four blocks high at x 9, the player on its top, the zombie beside it.
        for y in 201..=204 {
            for z in 4..=6 {
                terrain::set_block(&mut h.game, (9, y, z), STONE);
            }
        }
        h.game.sessions.get_mut(&1).unwrap().state =
            Some(PlayerState { pos: [9.5, 205.0, 5.5], flags: ON_GROUND, ..Default::default() });
        let z = summon(&mut h.game, &ZOMBIE, [10.5, 201.0, 5.5]).unwrap();
        {
            let m = h.game.mobs.mobs.iter_mut().find(|m| m.id == z).unwrap();
            super::apply_with(m, &Config { pillaring: true, ..config::DEFAULT });
            // The target the goal needs, taken by hand: the roll that takes it is the test above, and
            // this one is about the tower the zombie builds once it has the player.
            m.attack_target = Some(1);
        }
        let mut placed = false;
        for _ in 0..1200 {
            h.tick(1);
            if state(&h, (10, 201, 5)) == COBBLESTONE {
                placed = true;
                break;
            }
        }
        assert!(placed, "the zombie never placed a pillar block");
        // Four blocks of tower take it to the player's level at y 205.
        let mut climbed = 0.0;
        let mut reached = false;
        for _ in 0..1800 {
            h.tick(1);
            climbed = mob(&h, z).body.pos.y;
            if climbed >= 204.5 {
                reached = true;
                break;
            }
        }
        assert!(reached, "climbed only to y {climbed}");
    });
}

#[test]
fn the_attribute_multipliers_scale_a_zombie() {
    let d = def_of(&ZOMBIE).unwrap();
    let mut m = Mob::new(1, d, [0.0, 0.0, 0.0], 0.0);
    // A bare `Mob::new` is vanilla: the kind's own health, follow range 35 and damage.
    assert_eq!((m.max_health(), m.follow_range(), m.attack_damage()), (20.0, 35.0, Some(3.0)));
    // The default settings leave health and damage at the kind's values, and the join raises
    // FOLLOW_RANGE to the awareness radius, 64 over the zombie's 35.
    super::apply_with(&mut m, &config::DEFAULT);
    assert_eq!((m.max_health(), m.follow_range(), m.attack_damage()), (20.0, 64.0, Some(3.0)));
    let cfg = Config { mod_health: 2.0, mod_damage: 1.5, mod_speed: 1.5, ..config::DEFAULT };
    super::apply_with(&mut m, &cfg);
    assert_eq!(m.max_health(), 40.0);
    assert_eq!(m.attack_damage(), Some(4.5));
    assert!((m.movement_speed() - 0.345).abs() < 1e-6, "{}", m.movement_speed());
    assert_eq!(m.follow_range(), 64.0);
    // `heal(getMaxHealth())`: the mob joins at its new maximum.
    assert_eq!(m.health, 40.0);
}
