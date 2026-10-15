//! `S_SOUND`: `NetHandlerPlayClient.handleSoundEffect`, which calls
//! `World.playSound(player, x, y, z, sound, category, volume, pitch)`: a positioned sound at the
//! packet's spot with its own volume and pitch. The mixer attenuates linearly over
//! `16 * max(volume, 1)` blocks. Native and streamed sessions share the `Audio` resource.

use bevy::prelude::*;

use super::Sound as SoundMsg;
use crate::audio::{Audio, Category};

/// `SoundCategory` by ordinal (MASTER, MUSIC, RECORDS, WEATHER, BLOCKS, HOSTILE, NEUTRAL,
/// PLAYERS, AMBIENT, VOICE) onto the mixer's categories; the options file has no slider for
/// the rest, so records follow music and the others follow ambient.
pub fn category_of(ordinal: u8) -> Category {
    match ordinal {
        0 => Category::Master,
        1 | 2 => Category::Music,
        4 => Category::Blocks,
        7 => Category::Players,
        _ => Category::Ambient,
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, play);
}

fn play(mut sounds: MessageReader<SoundMsg>, audio: Option<Res<Audio>>) {
    for SoundMsg(s) in sounds.read() {
        let Some(audio) = &audio else { continue };
        let at = Vec3::new(s.pos[0], s.pos[1], s.pos[2]);
        audio.play(&s.name, category_of(s.category), Some(at), s.volume, s.pitch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_follow_the_enum_order() {
        assert_eq!(category_of(4), Category::Blocks);
        assert_eq!(category_of(7), Category::Players);
        assert_eq!(category_of(2), Category::Music);
        assert_eq!(category_of(5), Category::Ambient);
    }

    #[test]
    fn a_sound_message_plays_without_audio_assets() {
        let mut app = App::new();
        app.add_message::<SoundMsg>().insert_resource(Audio::silent()).add_systems(Update, play);
        app.world_mut().write_message(SoundMsg(proto::Sound { name: "entity.zombie.ambient".into(), category: 5, pos: [1.0, 2.0, 3.0], volume: 1.0, pitch: 0.9 }));
        app.update();
    }
}
