//! Mob sounds the server plays, ported from `World.playSound(null, ...)` and the sound
//! parts of `EntityLiving`, `Entity.move`, `EntityCreeper`, `AbstractSkeleton` and `EntityArrow`.
//! The viewer plays any `S_SOUND`; it plays the mobs' hurt and death sounds itself from the
//! entity status, so those are not sent from here.
//!
//! DROPPED: the swim and splash sounds of `Entity.move` and `handleWaterMovement`, `isSilent`
//! (no mob is silenced), and the chicken egg and sheep shear sounds.

use sim::collision::BlockAccess;

use super::ai::Effect;
use super::{spawn_lists, Mob};
use crate::game::net;
use crate::game::terrain::Blocks;
use crate::game::Game;
use worldgen::java_random::JavaRandom;

/// `SoundCategory.HOSTILE`, `NEUTRAL` and `PLAYERS` ordinals.
pub const HOSTILE: u8 = 5;
pub const NEUTRAL: u8 = 6;
pub const PLAYERS: u8 = 7;

/// `World.playSound(null, x, y, z, sound, category, volume, pitch)`: every player within
/// `16 * max(volume, 1)` blocks (`PlayerList.sendToAllNearExcept` with `volume > 1 ? 16 * volume : 16`).
pub fn play_sound(game: &Game, pos: [f64; 3], name: &str, category: u8, volume: f32, pitch: f32) {
    play_sound_except(game, None, pos, name, category, volume, pitch);
}

/// The same, skipping the session `except` (`World.playSound(player, ...)`).
pub fn play_sound_except(game: &Game, except: Option<u32>, pos: [f64; 3], name: &str, category: u8, volume: f32, pitch: f32) {
    let sound = proto::Sound { name: name.into(), category, pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32], volume, pitch };
    net::send_near(game, except, pos, 16.0 * volume.max(1.0) as f64, proto::S_SOUND, &sound.encode());
}

/// `Entity.getSoundCategory` of a mob: `EntityMob` is HOSTILE, the rest NEUTRAL.
pub fn category_of(mob: &Mob) -> u8 {
    if spawn_lists::is_hostile(mob.def) {
        HOSTILE
    } else {
        NEUTRAL
    }
}

/// `EntityLivingBase.getSoundPitch`: `(rand - rand) * 0.2 + 1.0`, 1.5 for a child.
pub fn sound_pitch(child: bool, rand: &mut JavaRandom) -> f32 {
    let a = rand.next_float();
    let b = rand.next_float();
    (a - b) * 0.2 + if child { 1.5 } else { 1.0 }
}

/// The 1.12.2 `SoundType` of a block state: event prefix, volume and pitch (`Block.getSoundType`
/// through `worldgen`'s `SOUND` table). Same order as that table's indices.
const TYPES: [(&str, f32, f32); 12] = [
    ("block.wood", 1.0, 1.0),
    ("block.gravel", 1.0, 1.0),
    ("block.grass", 1.0, 1.0),
    ("block.stone", 1.0, 1.0),
    ("block.metal", 1.0, 1.5),
    ("block.glass", 1.0, 1.0),
    ("block.cloth", 1.0, 1.0),
    ("block.sand", 1.0, 1.0),
    ("block.snow", 1.0, 1.0),
    ("block.ladder", 1.0, 1.0),
    ("block.anvil", 0.3, 1.0),
    ("block.slime", 1.0, 1.0),
];

const SNOW_LAYER: u16 = 78;

fn sound_type(state: u16) -> (&'static str, f32, f32) {
    let t = worldgen::populate::props::SOUND[(state >> 4) as usize & 255][(state & 15) as usize] as usize;
    TYPES[t.min(TYPES.len() - 1)]
}

impl Mob {
    /// `getAmbientSound`: None for the creeper (it has none).
    pub fn ambient_sound(&self) -> Option<&'static str> {
        Some(match self.def.kind.name {
            "pig" => "entity.pig.ambient",
            "sheep" => "entity.sheep.ambient",
            "cow" => "entity.cow.ambient",
            "chicken" => "entity.chicken.ambient",
            "zombie" => "entity.zombie.ambient",
            "skeleton" => "entity.skeleton.ambient",
            "spider" => "entity.spider.ambient",
            _ => return None,
        })
    }

    /// `getTalkInterval`: 80, and `EntityAnimal` says 120.
    pub fn talk_interval(&self) -> i32 {
        if matches!(self.def.kind.name, "pig" | "sheep" | "cow" | "chicken") {
            120
        } else {
            80
        }
    }

    /// `getSoundVolume`: 1.0, and `EntityCow` says 0.4.
    pub fn sound_volume(&self) -> f32 {
        if self.def.kind.name == "cow" {
            0.4
        } else {
            1.0
        }
    }

    /// `EntityLiving.onEntityUpdate`'s tail: `rand.nextInt(1000) < livingSoundTime++` plays the
    /// living sound and resets the timer to `-getTalkInterval()`.
    pub(super) fn living_sound_update(&mut self, rand: &mut JavaRandom, fx: &mut Vec<Effect>) {
        if self.dead || self.health <= 0.0 {
            return;
        }
        let r = rand.next_int_bounded(1000);
        let t = self.living_sound_time;
        self.living_sound_time += 1;
        if r < t {
            self.living_sound_time = -self.talk_interval();
            // playLivingSound
            if let Some(name) = self.ambient_sound() {
                let pitch = sound_pitch(self.is_child(), rand);
                self.sound(fx, name, self.sound_volume(), pitch);
            }
        }
    }

    /// `Entity.playSound` at the mob's feet in its category.
    pub fn sound(&self, fx: &mut Vec<Effect>, name: &str, volume: f32, pitch: f32) {
        let p = self.body.pos;
        fx.push(Effect::Sound { pos: [p.x, p.y, p.z], name: name.to_string(), category: category_of(self), volume, pitch });
    }

    /// `playStepSound`: the kinds below override it with their own event at 0.15 and 1.0; any
    /// other kind plays the block's `SoundType` step (nothing on a liquid, snow when a snow layer
    /// lies on the block).
    pub(super) fn play_step_sounds(&mut self, w: &Blocks, fx: &mut Vec<Effect>) {
        let steps: Vec<(i32, i32, i32)> = self
            .body
            .sounds
            .drain(..)
            .filter_map(|s| match s {
                sim::entity::Sound::Step(x, y, z) => Some((x, y, z)),
                // DROPPED: the swim and splash sounds.
                _ => None,
            })
            .collect();
        for (x, y, z) in steps {
            let own = match self.def.kind.name {
                "pig" => Some("entity.pig.step"),
                "sheep" => Some("entity.sheep.step"),
                "cow" => Some("entity.cow.step"),
                "chicken" => Some("entity.chicken.step"),
                "zombie" => Some("entity.zombie.step"),
                "skeleton" => Some("entity.skeleton.step"),
                "spider" => Some("entity.spider.step"),
                _ => None,
            };
            if let Some(name) = own {
                self.sound(fx, name, 0.15, 1.0);
                continue;
            }
            let state = w.state(x, y, z);
            let (name, vol, pitch) = if w.state(x, y + 1, z) >> 4 == SNOW_LAYER {
                sound_type(SNOW_LAYER << 4)
            } else if matches!(state >> 4, 8..=11) {
                continue;
            } else {
                sound_type(state)
            };
            self.sound(fx, &format!("{name}.step"), vol * 0.15, pitch);
        }
    }

    /// `EntityCreeper.processInteract` with flint and steel: the sound for everyone (the
    /// client's own copy of the click does not play it here) at `rand * 0.4 + 0.8`.
    pub fn flint_and_steel_sound(game: &mut Game, id: u32) {
        let Some(i) = game.mobs.mobs.iter().position(|m| m.id == id) else { return };
        let p = game.mobs.mobs[i].body.pos;
        let pitch = game.mobs.rand.next_float() * 0.4 + 0.8;
        play_sound(game, [p.x, p.y, p.z], "item.flintandsteel.use", category_of(&game.mobs.mobs[i]), 1.0, pitch);
    }
}
