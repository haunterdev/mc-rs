//! `S_WORLD_EVENT` 1010 (`RenderGlobal.playEvent`, `playRecord`): a disc id plays
//! `record.<name>` at the jukebox (`PositionedSoundRecord.getRecordSoundRecord`: volume 4.0,
//! pitch 1.0, linear attenuation, so the mixer's `16 * volume` is the 64 blocks) and sets the
//! "Now Playing: C418 - <name>" line over the hotbar (`GuiIngame.setRecordPlayingMessage`, 60
//! ticks, rainbow colour, fading out). Data 0 stops it, and so does the block not being a
//! jukebox any more (`playRecord` first stops the sound already playing at the spot).
//!
//! DROPPED: `MusicTicker` pausing the background music while a record plays (the viewer's music
//! has no such state to pause), and the sound of a disc that was already playing when this viewer
//! joined (vanilla keeps none either).

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::gui::draw::string_width;
use super::gui::slots::Gui;
use super::gui::GuiLang;
use super::overlay::{Overlay, RECORD_TEXT};
use super::WorldEvent;
use crate::audio::{Audio, Category, SoundId};
use crate::celestial::hsv_to_rgb;
use crate::players::Font;
use crate::sky::Clock;
use crate::stream::Streamer;

type At = (i32, i32, i32);

/// `World.playEvent`'s record event.
const EVENT_RECORD: i32 = 1010;
/// `GuiIngame.recordPlayingUpFor` after `setRecordPlayingMessage`.
const MESSAGE_TICKS: i32 = 60;

#[derive(Resource, Default)]
pub struct Jukeboxes {
    playing: HashMap<At, SoundId>,
    /// The line and the ticks it has left.
    message: Option<(String, i32)>,
}

impl Jukeboxes {
    /// One 1010 event: stop what plays at the spot, then start the disc when there is one.
    fn event(&mut self, audio: Option<&Audio>, lang: Option<&sim::lang::Lang>, at: At, data: i32) {
        if let Some(id) = self.playing.remove(&at) {
            if let Some(audio) = audio {
                audio.stop(id);
            }
        }
        let Some(name) = u16::try_from(data).ok().and_then(::items::tooltip::record_name) else { return };
        if let Some(audio) = audio {
            let pos = Vec3::new(at.0 as f32 + 0.5, at.1 as f32 + 0.5, at.2 as f32 + 0.5);
            if let Some(id) = audio.play(&format!("record.{name}"), Category::Music, Some(pos), 4.0, 1.0) {
                self.playing.insert(at, id);
            }
        }
        let desc = lang.map_or(format!("item.record.{name}.desc"), |l| l.translate(&format!("item.record.{name}.desc")).to_string());
        let text = match lang {
            Some(l) => l.format("record.nowPlaying", &[sim::lang::Arg::Str(desc)]),
            None => format!("Now Playing: {desc}"),
        };
        self.message = Some((text, MESSAGE_TICKS));
    }
}

/// `GuiIngame.renderGameOverlay`: alpha and colour of the line with `left` ticks to go.
/// `None` once it is too faint to draw.
fn fade(left: i32, partial: f32) -> Option<u32> {
    let f2 = left as f32 - partial;
    let a = ((f2 * 255.0 / 20.0) as i32).min(255);
    if a <= 8 {
        return None;
    }
    let rgb = hsv_to_rgb(f2 / 50.0, 0.7, 0.6) & 0xFF_FFFF;
    Some((a as u32) << 24 | rgb as u32)
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Jukeboxes>().add_systems(Update, (events, tick, stop_gone, draw).chain());
}

fn events(mut jb: ResMut<Jukeboxes>, mut events: MessageReader<WorldEvent>, audio: Option<Res<Audio>>, lang: Option<Res<GuiLang>>) {
    for WorldEvent(e) in events.read() {
        if e.event == EVENT_RECORD {
            jb.event(audio.as_deref(), lang.as_ref().map(|l| &l.0), (e.pos[0], e.pos[1], e.pos[2]), e.data);
        }
    }
}

/// `GuiIngame.updateTick`: the line counts down.
fn tick(mut jb: ResMut<Jukeboxes>, clock: Option<Res<Clock>>) {
    let n = clock.map_or(0, |c| c.ticks_this_frame as i32);
    if let Some((_, left)) = jb.message.as_mut() {
        *left -= n;
    }
    if jb.message.as_ref().is_some_and(|m| m.1 <= 0) {
        jb.message = None;
    }
}

/// A record whose block is not a jukebox any more is cut off.
fn stop_gone(mut jb: ResMut<Jukeboxes>, audio: Option<Res<Audio>>, streamer: Option<Res<Streamer>>) {
    let (Some(audio), Some(streamer)) = (audio, streamer) else { return };
    let gone: Vec<At> = jb
        .playing
        .iter()
        .filter(|(at, id)| !audio.playing(**id) || streamer.state_at(at.0, at.1, at.2).is_some_and(|s| s >> 4 != ::items::ids::block::JUKEBOX))
        .map(|(at, _)| *at)
        .collect();
    for at in gone {
        if let Some(id) = jb.playing.remove(&at) {
            audio.stop(id);
        }
    }
}

/// The line, centred at the bottom of the hotbar's headroom: `translate(w / 2, h - 68)`,
/// `drawString(text, -width / 2, -4, colour)`.
fn draw(jb: Res<Jukeboxes>, mut overlay: ResMut<Overlay>, clock: Option<Res<Clock>>, window: Single<&Window, With<PrimaryWindow>>, font: Option<Res<Font>>) {
    overlay.layers[RECORD_TEXT] = Default::default();
    let (Some((text, left)), Some(font)) = (jb.message.as_ref(), font) else { return };
    let Some(color) = fade(*left, clock.map_or(0.0, |c| c.partial)) else { return };
    let gui = Gui::new(window.physical_width(), window.physical_height(), 0, 0);
    let x = gui.w / 2 - string_width(&font, text, false) / 2;
    overlay.layers[RECORD_TEXT].text(&font, text, x, gui.h - 68 - 4, color, true, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: At = (3, 70, -4);

    #[test]
    fn a_disc_sets_the_line_and_zero_clears_the_sound() {
        let mut jb = Jukeboxes::default();
        let lang = sim::lang::Lang::parse("record.nowPlaying=Now Playing: %s\nitem.record.cat.desc=C418 - cat\n");
        jb.event(None, Some(&lang), AT, ::items::ids::item::RECORD_CAT as i32);
        assert_eq!(jb.message, Some(("Now Playing: C418 - cat".to_string(), 60)));
        // Data 0 plays nothing and leaves the line to run out.
        jb.event(None, Some(&lang), AT, 0);
        assert!(jb.playing.is_empty());
        assert_eq!(jb.message.as_ref().map(|m| m.1), Some(60));
    }

    #[test]
    fn an_unknown_id_is_ignored() {
        let mut jb = Jukeboxes::default();
        jb.event(None, None, AT, 1);
        assert!(jb.message.is_none());
    }

    #[test]
    fn the_line_fades_in_the_last_twenty_ticks() {
        assert_eq!(fade(60, 0.0).unwrap() >> 24, 255);
        assert_eq!(fade(10, 0.0).unwrap() >> 24, 127);
        assert!(fade(1, 0.5).is_none(), "half a tick left is under the alpha of 8");
    }
}
