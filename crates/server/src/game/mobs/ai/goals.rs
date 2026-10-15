//! The goals every animal shares: `EntityAISwimming`, `EntityAIPanic`, `EntityAIWander` with its
//! `EntityAIWanderAvoidWater` subclass, `EntityAIWatchClosest` and `EntityAILookIdle`.

use sim::collision::{floor, BlockAccess};
use sim::region::Mat;

use super::random_pos::{find_random_target, get_land_pos};
use super::{dist_sq, Ctx, Goal};
use crate::game::mobs::path::blocks::material;
use crate::game::mobs::Mob;

/// `EntityLiving.getHorizontalFaceSpeed` and `getVerticalFaceSpeed`.
pub const HORIZONTAL_FACE_SPEED: f32 = 10.0;
pub const VERTICAL_FACE_SPEED: f32 = 40.0;

/// `EntityAISwimming`
#[derive(Clone, Debug)]
pub struct Swimming;

impl Swimming {
    /// The constructor also calls `PathNavigateGround.setCanSwim(true)`.
    /// DROPPED: the `PathNavigateFlying.setCanFloat` branch (no flying mob yet).
    pub fn new(mob: &mut Mob) -> Swimming {
        mob.navigator.set_can_swim(true);
        Swimming
    }
}

impl Goal for Swimming {
    fn mutex_bits(&self) -> u32 {
        4
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        c.mob.body.in_water || c.mob.body.is_in_lava(c.w)
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        if c.rand.next_float() < 0.8 {
            c.mob.jump_helper.set_jumping();
        }
    }
}

/// `EntityAIPanic`
#[derive(Clone, Debug)]
pub struct Panic {
    speed: f64,
    rand_pos: [f64; 3],
}

impl Panic {
    pub fn new(speed: f64) -> Panic {
        Panic { speed, rand_pos: [0.0; 3] }
    }

    /// `findRandomPosition`
    fn find_random_position(&mut self, c: &mut Ctx<'_>) -> bool {
        match find_random_target(c.mob, c.w, c.rand, 5, 4) {
            None => false,
            Some(v) => {
                self.rand_pos = v;
                true
            }
        }
    }
}

/// `EntityAIPanic.getRandPos`: the nearest water block within the box.
fn get_rand_pos(w: &impl BlockAccess, mob: &Mob, horizontal: i32, vertical: i32) -> Option<(i32, i32, i32)> {
    let (i, j, k) = (floor(mob.body.pos.x), floor(mob.body.pos.y), floor(mob.body.pos.z));
    let mut f = (horizontal * horizontal * vertical * 2) as f32;
    let mut found = None;
    for l in i - horizontal..=i + horizontal {
        for i1 in j - vertical..=j + vertical {
            for j1 in k - horizontal..=k + horizontal {
                if material(w.state(l, i1, j1)) == Mat::Water {
                    let f1 = ((l - i) * (l - i) + (i1 - j) * (i1 - j) + (j1 - k) * (j1 - k)) as f32;
                    if f1 < f {
                        f = f1;
                        found = Some((l, i1, j1));
                    }
                }
            }
        }
    }
    found
}

impl Goal for Panic {
    fn mutex_bits(&self) -> u32 {
        1
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        let burning = c.mob.is_burning();
        if c.mob.revenge_target.is_none() && !burning {
            return false;
        }
        if burning {
            if let Some(p) = get_rand_pos(c.w, c.mob, 5, 4) {
                self.rand_pos = [p.0 as f64, p.1 as f64, p.2 as f64];
                return true;
            }
        }
        self.find_random_position(c)
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        let [x, y, z] = self.rand_pos;
        c.mob.try_move_to(c.w, x, y, z, self.speed);
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        !c.mob.navigator.no_path()
    }
}

/// `EntityAIWander`, and `EntityAIWanderAvoidWater` when `avoid_water` holds its probability.
#[derive(Clone, Debug)]
pub struct Wander {
    pos: [f64; 3],
    speed: f64,
    execution_chance: i32,
    must_update: bool,
    /// `EntityAIWanderAvoidWater.probability`; `None` for the plain `EntityAIWander`.
    avoid_water: Option<f32>,
}

impl Wander {
    /// `new EntityAIWander(creature, speed)`: chance 120.
    pub fn new(speed: f64) -> Wander {
        Wander { pos: [0.0; 3], speed, execution_chance: 120, must_update: false, avoid_water: None }
    }

    /// `new EntityAIWanderAvoidWater(creature, speed)`: probability 0.001.
    pub fn avoid_water(speed: f64) -> Wander {
        Wander { avoid_water: Some(0.001), ..Wander::new(speed) }
    }

    pub fn make_update(&mut self) {
        self.must_update = true;
    }

    pub fn set_execution_chance(&mut self, chance: i32) {
        self.execution_chance = chance;
    }

    /// `getPosition`, with the `EntityAIWanderAvoidWater` override.
    fn get_position(&self, c: &mut Ctx<'_>) -> Option<[f64; 3]> {
        match self.avoid_water {
            None => find_random_target(c.mob, c.w, c.rand, 10, 7),
            Some(probability) => {
                if c.mob.body.in_water {
                    get_land_pos(c.mob, c.w, c.rand, 15, 7).or_else(|| find_random_target(c.mob, c.w, c.rand, 10, 7))
                } else if c.rand.next_float() >= probability {
                    get_land_pos(c.mob, c.w, c.rand, 10, 7)
                } else {
                    find_random_target(c.mob, c.w, c.rand, 10, 7)
                }
            }
        }
    }
}

impl Goal for Wander {
    fn mutex_bits(&self) -> u32 {
        1
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if !self.must_update {
            if c.mob.idle_time >= 100 {
                return false;
            }
            if c.rand.next_int_bounded(self.execution_chance) != 0 {
                return false;
            }
        }
        match self.get_position(c) {
            None => false,
            Some(v) => {
                self.pos = v;
                self.must_update = false;
                true
            }
        }
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        !c.mob.navigator.no_path()
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        let [x, y, z] = self.pos;
        c.mob.try_move_to(c.w, x, y, z, self.speed);
    }
}

/// `EntityAIWatchClosest` for `EntityPlayer.class` (the only watched class a cow has).
#[derive(Clone, Debug)]
pub struct WatchClosest {
    /// The watched player's entity id.
    closest: Option<u32>,
    max_distance: f32,
    look_time: i32,
    chance: f32,
}

impl WatchClosest {
    pub fn new(max_distance: f32) -> WatchClosest {
        WatchClosest { closest: None, max_distance, look_time: 0, chance: 0.02 }
    }
}

impl Goal for WatchClosest {
    fn mutex_bits(&self) -> u32 {
        2
    }

    /// DROPPED: the `getAttackTarget` branch (the player-class lookup overwrites it anyway),
    /// `EntitySelectors.notRiding` (nothing rides), and the non-player class lookup.
    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if c.rand.next_float() >= self.chance {
            return false;
        }
        let p = c.mob.body.pos;
        self.closest = c.closest_player([p.x, p.y, p.z], self.max_distance as f64).map(|p| p.id);
        self.closest.is_some()
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        let Some(p) = self.closest.and_then(|id| c.player(id)) else { return false };
        let m = c.mob.body.pos;
        if dist_sq([m.x, m.y, m.z], p.pos) > (self.max_distance * self.max_distance) as f64 {
            return false;
        }
        self.look_time > 0
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        self.look_time = 40 + c.rand.next_int_bounded(40);
    }

    fn reset_task(&mut self, _c: &mut Ctx<'_>) {
        self.closest = None;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        if let Some(p) = self.closest.and_then(|id| c.player(id)) {
            let e = p.eye_pos();
            c.mob.look_helper.set_look_position(e[0], e[1], e[2], HORIZONTAL_FACE_SPEED, VERTICAL_FACE_SPEED);
        }
        self.look_time -= 1;
    }
}

/// `EntityAILookIdle`
#[derive(Clone, Debug, Default)]
pub struct LookIdle {
    look_x: f64,
    look_z: f64,
    idle_time: i32,
}

impl LookIdle {
    pub fn new() -> LookIdle {
        LookIdle::default()
    }
}

impl Goal for LookIdle {
    fn mutex_bits(&self) -> u32 {
        3
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        c.rand.next_float() < 0.02
    }

    fn should_continue_executing(&mut self, _c: &mut Ctx<'_>) -> bool {
        self.idle_time >= 0
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        let d0 = 6.283_185_307_179_586 * c.rand.next_double();
        self.look_x = d0.cos();
        self.look_z = d0.sin();
        self.idle_time = 20 + c.rand.next_int_bounded(20);
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        self.idle_time -= 1;
        let (p, eye) = (c.mob.body.pos, c.mob.eye_height() as f64);
        c.mob.look_helper.set_look_position(p.x + self.look_x, p.y + eye, p.z + self.look_z, HORIZONTAL_FACE_SPEED, VERTICAL_FACE_SPEED);
    }
}
