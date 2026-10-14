//! What the game makes sound and particles for: `RenderGlobal.playEvent(2001)` (break sound and
//! debris), `ItemBlock`'s place sound, `Entity.playStepSound`, the swim and splash sounds, the
//! sprint dust, `MusicTicker`, and the menu's button click. The sound engine is audio.rs, the
//! particles particles.rs.

use bevy::prelude::*;
use worldgen::blocks::id;
use worldgen::populate::props::SOUND;

use crate::audio::{Audio, Category, MusicTicker};
use crate::colors::Colors;
use crate::player::{Player, PlayerSound};
use crate::particles::Particles;
use crate::settings::Settings;
use crate::sky::{BiomeColors, Clock, View};
use crate::stream::{Streamer, WorldEvent};
use crate::Atlas;

/// `SoundType`: the event prefix, volume and pitch, by `sound_type` index.
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

/// `Block.getSoundType` for a state: prefix, volume, pitch.
pub fn sound_of(s: u16) -> (&'static str, f32, f32) {
    let t = SOUND[(s >> 4) as usize & 255][(s & 15) as usize] as usize;
    TYPES[t.min(TYPES.len() - 1)]
}

fn center(p: (i32, i32, i32)) -> Vec3 {
    Vec3::new(p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5)
}

/// Follows the options' volumes and the listener; once per frame.
pub fn update_listener(audio: Res<Audio>, settings: Res<Settings>, view: Res<View>) {
    if settings.is_changed() {
        audio.set_volume(Category::Master, settings.master_volume);
        audio.set_volume(Category::Music, settings.music_volume);
        for c in [Category::Blocks, Category::Players, Category::Ambient] {
            audio.set_volume(c, settings.sound_volume);
        }
    }
    audio.set_listener(view.eye, view.yaw);
}

/// The world's and the player's sounds and particles for this frame's ticks.
#[allow(clippy::too_many_arguments)]
pub fn play(
    clock: Res<Clock>,
    audio: Res<Audio>,
    atlas: Res<Atlas>,
    colors: Res<BiomeColors>,
    mut music: ResMut<MusicTicker>,
    mut streamer: ResMut<Streamer>,
    mut player: ResMut<Player>,
    mut particles: ResMut<Particles>,
) {
    let colors: &Colors = &colors.0;
    for e in std::mem::take(&mut streamer.events) {
        match e {
            WorldEvent::Broke(p, s) => {
                // RenderGlobal.playEvent 2001
                let (name, v, pitch) = sound_of(s);
                audio.play(&format!("{name}.break"), Category::Blocks, Some(center(p)), (v + 1.0) / 2.0, pitch * 0.8);
                if s >> 4 != id::FIRE {
                    particles.block_destroyed(&atlas, colors, &streamer, p, s);
                }
            }
            WorldEvent::Placed(p, s) => {
                let (name, v, pitch) = sound_of(s);
                audio.play(&format!("{name}.place"), Category::Blocks, Some(center(p)), (v + 1.0) / 2.0, pitch * 0.8);
            }
        }
    }
    let feet = player.pos.as_vec3();
    let jitter = |a: &Audio| 1.0 + (a.next_float() - a.next_float()) * 0.4;
    for s in std::mem::take(&mut player.sounds) {
        match s {
            PlayerSound::Step(x, y, z) => {
                // Entity.playStepSound: a snow layer on top steps as snow; liquids are silent.
                let block = streamer.state_at(x, y, z).unwrap_or(0);
                let above = streamer.state_at(x, y + 1, z).unwrap_or(0);
                let state = if above >> 4 == id::SNOW_LAYER { above } else { block };
                let liquid = matches!(state >> 4, id::WATER | id::FLOWING_WATER | id::LAVA | id::FLOWING_LAVA);
                if !liquid {
                    let (name, v, pitch) = sound_of(state);
                    audio.play(&format!("{name}.step"), Category::Players, Some(feet), v * 0.15, pitch);
                }
            }
            PlayerSound::Swim(v) => {
                audio.play("entity.player.swim", Category::Players, Some(feet), v, jitter(&audio));
            }
            PlayerSound::Splash(v) => {
                audio.play("entity.player.splash", Category::Players, Some(feet), v, jitter(&audio));
            }
        }
    }
    for _ in 0..clock.ticks_this_frame {
        music.update(&audio);
        // Entity.spawnRunningParticles
        if player.spawned && player.sprinting && !player.in_water {
            let (pos, motion, width) = (player.pos, player.motion, player.width as f64);
            particles.running(&atlas, colors, &streamer, pos, motion, width);
        }
    }
}

/// `GuiButton.playPressSound` for the menu buttons: one click per mouse press over a button, as
/// `GuiScreen.mouseClicked` plays it, rather than per `Interaction` change (the streamed
/// pointer and the UI focus system can both write it in one press). Runs before
/// `settings::toggle_menu`, so the Done button clicks before the menu goes.
pub fn button_clicks(audio: Res<Audio>, mouse: Res<ButtonInput<MouseButton>>, buttons: Query<&Interaction, With<Button>>) {
    // GuiScreen.mouseClicked acts on mouse button 0 only.
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if buttons.iter().any(|i| *i != Interaction::None) {
        audio.play("ui.button.click", Category::Master, None, 0.25, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen::blocks::state;

    #[test]
    fn sound_types_follow_the_block_table() {
        assert_eq!(sound_of(state(id::STONE, 0)).0, "block.stone");
        assert_eq!(sound_of(state(id::GRASS, 0)).0, "block.grass");
        assert_eq!(sound_of(state(id::DIRT, 0)).0, "block.gravel");
        assert_eq!(sound_of(state(id::LOG, 0)).0, "block.wood");
        assert_eq!(sound_of(state(id::SAND, 0)).0, "block.sand");
    }
}
