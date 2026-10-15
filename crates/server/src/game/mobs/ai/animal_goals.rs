//! The animal goals: `EntityAIMate`, `EntityAITempt`, `EntityAIFollowParent`.

use super::goals::{HORIZONTAL_FACE_SPEED, VERTICAL_FACE_SPEED};
use super::{dist_sq, Ctx, Effect, Goal};
use crate::game::mobs::animals::dye_damage;
use crate::game::mobs::Mob;

fn pos_of(m: &Mob) -> [f64; 3] {
    [m.body.pos.x, m.body.pos.y, m.body.pos.z]
}

/// `EntityAIMate`
#[derive(Clone, Debug)]
pub struct Mate {
    /// `targetMate`: its entity id.
    target: Option<u32>,
    spawn_baby_delay: i32,
    move_speed: f64,
}

impl Mate {
    pub fn new(speed: f64) -> Mate {
        Mate { target: None, spawn_baby_delay: 0, move_speed: speed }
    }

    /// `getNearbyMate`: the nearest animal of the same class in the box grown by 8 that
    /// `canMateWith` this one.
    fn get_nearby_mate(&self, c: &Ctx<'_>) -> Option<u32> {
        let bb = c.mob.body.bb.grow(8.0, 8.0, 8.0);
        let mut d0 = f64::MAX;
        let mut found = None;
        for o in c.others.iter() {
            if o.dead || !o.body.bb.intersects(&bb) {
                continue;
            }
            if c.mob.can_mate_with(o) {
                let d = dist_sq(pos_of(c.mob), pos_of(o));
                if d < d0 {
                    found = Some(o.id);
                    d0 = d;
                }
            }
        }
        found
    }

    /// `spawnBaby`. DROPPED: `StatList.ANIMALS_BRED` and the `BRED_ANIMALS` trigger, the heart
    /// particles (a server-side `spawnParticle` sends nothing in 1.12.2) and the experience orb.
    fn spawn_baby(&mut self, c: &mut Ctx<'_>) {
        let Some(id) = self.target else { return };
        let def = c.mob.def;
        let pos = pos_of(c.mob);
        let Some(mate) = c.others.get_mut(id) else { return };
        mate.set_growing_age(6000);
        c.mob.set_growing_age(6000);
        c.mob.reset_in_love();
        mate.reset_in_love();
        let dyes = (def.kind.name == "sheep").then(|| (dye_damage(c.mob.get_fleece_color()), dye_damage(mate.get_fleece_color())));
        c.fx.push(Effect::Child { def, pos, dyes });
    }
}

impl Goal for Mate {
    fn mutex_bits(&self) -> u32 {
        3
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if !c.mob.is_in_love() {
            return false;
        }
        self.target = self.get_nearby_mate(c);
        self.target.is_some()
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        let Some(m) = self.target.and_then(|id| c.others.get(id)) else { return false };
        !m.dead && m.health > 0.0 && m.is_in_love() && self.spawn_baby_delay < 60
    }

    fn reset_task(&mut self, _c: &mut Ctx<'_>) {
        self.target = None;
        self.spawn_baby_delay = 0;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        let Some((tp, eye)) = self.target.and_then(|id| c.others.get(id)).map(|m| (pos_of(m), m.eye_height() as f64)) else { return };
        c.mob.look_helper.set_look_position_with_entity([tp[0], tp[1] + eye, tp[2]], 10.0, VERTICAL_FACE_SPEED);
        c.mob.try_move_to_entity_living(c.w, tp, self.move_speed);
        self.spawn_baby_delay += 1;
        if self.spawn_baby_delay >= 60 && dist_sq(pos_of(c.mob), tp) < 9.0 {
            self.spawn_baby(c);
        }
    }
}

/// `EntityAITempt` for a creature with a ground navigator.
#[derive(Clone, Debug)]
pub struct Tempt {
    speed: f64,
    target: [f64; 3],
    pitch: f64,
    yaw: f64,
    /// `temptingPlayer`: its entity id.
    tempting: Option<u32>,
    delay_tempt_counter: i32,
    is_running: bool,
    tempt_items: Vec<u16>,
    scared_by_player_movement: bool,
}

impl Tempt {
    pub fn new(speed: f64, item: u16, scared_by_player_movement: bool) -> Tempt {
        Tempt {
            speed,
            target: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            tempting: None,
            delay_tempt_counter: 0,
            is_running: false,
            tempt_items: vec![item],
            scared_by_player_movement,
        }
    }

    /// `EntityAITempt(creature, speed, scaredByPlayerMovement, Set<Item>)`
    pub fn with_items(speed: f64, items: &[u16], scared_by_player_movement: bool) -> Tempt {
        Tempt { tempt_items: items.to_vec(), ..Tempt::new(speed, 0, scared_by_player_movement) }
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }

    fn is_tempting(&self, item: u16) -> bool {
        item != 0 && self.tempt_items.contains(&item)
    }
}

impl Goal for Tempt {
    fn mutex_bits(&self) -> u32 {
        3
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if self.delay_tempt_counter > 0 {
            self.delay_tempt_counter -= 1;
            return false;
        }
        // getClosestPlayerToEntity(entity, 10.0)
        let p = c.mob.body.pos;
        let player = c.closest_player([p.x, p.y, p.z], 10.0);
        self.tempting = player.map(|p| p.id);
        match player {
            None => false,
            Some(p) => self.is_tempting(p.held[0]) || self.is_tempting(p.held[1]),
        }
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        if self.scared_by_player_movement {
            let Some(p) = self.tempting.and_then(|id| c.player(id)) else { return false };
            if dist_sq(pos_of(c.mob), p.pos) < 36.0 {
                if dist_sq(p.pos, self.target) > 0.010_000_000_000_000_002 {
                    return false;
                }
                if (p.pitch as f64 - self.pitch).abs() > 5.0 || (p.yaw as f64 - self.yaw).abs() > 5.0 {
                    return false;
                }
            } else {
                self.target = p.pos;
            }
            self.pitch = p.pitch as f64;
            self.yaw = p.yaw as f64;
        }
        self.should_execute(c)
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        if let Some(p) = self.tempting.and_then(|id| c.player(id)) {
            self.target = p.pos;
        }
        self.is_running = true;
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        self.tempting = None;
        c.mob.navigator.clear_path();
        self.delay_tempt_counter = 100;
        self.is_running = false;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        let Some(p) = self.tempting.and_then(|id| c.player(id)) else { return };
        let e = p.eye_pos();
        c.mob.look_helper.set_look_position_with_entity(e, HORIZONTAL_FACE_SPEED + 20.0, VERTICAL_FACE_SPEED);
        if dist_sq(pos_of(c.mob), p.pos) < 6.25 {
            c.mob.navigator.clear_path();
        } else {
            c.mob.try_move_to_entity_living(c.w, p.pos, self.speed);
        }
    }
}

/// `EntityAIFollowParent`
#[derive(Clone, Debug)]
pub struct FollowParent {
    parent: Option<u32>,
    move_speed: f64,
    delay_counter: i32,
}

impl FollowParent {
    pub fn new(speed: f64) -> FollowParent {
        FollowParent { parent: None, move_speed: speed, delay_counter: 0 }
    }
}

impl Goal for FollowParent {
    fn mutex_bits(&self) -> u32 {
        0
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if c.mob.get_growing_age() >= 0 {
            return false;
        }
        let bb = c.mob.body.bb.grow(8.0, 4.0, 8.0);
        let mut found = None;
        let mut d0 = f64::MAX;
        for o in c.others.iter() {
            if o.def.kind.name != c.mob.def.kind.name || o.dead || !o.body.bb.intersects(&bb) {
                continue;
            }
            if o.get_growing_age() >= 0 {
                let d1 = dist_sq(pos_of(c.mob), pos_of(o));
                if d1 <= d0 {
                    d0 = d1;
                    found = Some(o.id);
                }
            }
        }
        match found {
            None => false,
            Some(_) if d0 < 9.0 => false,
            Some(id) => {
                self.parent = Some(id);
                true
            }
        }
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        if c.mob.get_growing_age() >= 0 {
            return false;
        }
        let Some(p) = self.parent.and_then(|id| c.others.get(id)) else { return false };
        if p.dead || p.health <= 0.0 {
            return false;
        }
        let d0 = dist_sq(pos_of(c.mob), pos_of(p));
        (9.0..=256.0).contains(&d0)
    }

    fn start_executing(&mut self, _c: &mut Ctx<'_>) {
        self.delay_counter = 0;
    }

    fn reset_task(&mut self, _c: &mut Ctx<'_>) {
        self.parent = None;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        self.delay_counter -= 1;
        if self.delay_counter <= 0 {
            self.delay_counter = 10;
            if let Some(p) = self.parent.and_then(|id| c.others.get(id)).map(pos_of) {
                c.mob.try_move_to_entity_living(c.w, p, self.move_speed);
            }
        }
    }
}
