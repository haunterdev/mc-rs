//! On-screen stats: FPS, chunks per second, chunk counts, process RAM, position, biome, time and
//! light.

use std::time::Instant;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin, SystemInformationDiagnosticsPlugin};
use bevy::prelude::*;

use crate::hand::HideGui;
use crate::player::Player;
use crate::sky::{neighbor_light, Clock, View};
use crate::stream::{chunk_of, Streamer};

#[derive(Component)]
pub struct HudText;

/// Last biome lookup, redone only when the player enters another block column.
#[derive(Default)]
pub struct BiomeCache {
    column: Option<(i32, i32)>,
    name: &'static str,
}

pub const CONTROLS: &str = "WASD walk, Space jump (double tap: fly), Shift sneak / down, Ctrl sprint\nleft click: capture mouse, then break; right click: place; middle click: pick block\nscroll or 1-9: hotbar slot\nEsc options, +/- render distance, F3 or H stats, F5 view, F1 hide GUI";

pub fn spawn_hud(mut commands: Commands) {
    commands.spawn((
        HudText,
        Text::new(""),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, top: px(10), left: px(12), ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
        Visibility::Hidden,
    ));
}

/// Memory reported by the sysinfo diagnostic, in MB.
pub fn process_mem_mb(diagnostics: &DiagnosticsStore) -> Option<f64> {
    diagnostics.get(&SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE).and_then(|d| d.value()).map(|gib| gib * 1024.0)
}

pub fn fps(diagnostics: &DiagnosticsStore) -> Option<f64> {
    diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed())
}

pub fn update_hud(
    mut streamer: ResMut<Streamer>,
    diagnostics: Res<DiagnosticsStore>,
    clock: Res<Clock>,
    view: Res<View>,
    player: Res<Player>,
    mut text: Single<&mut Text, With<HudText>>,
    mut biome: Local<BiomeCache>,
) {
    let pos = player.pos.as_vec3();
    let column = (pos.x.floor() as i32, pos.z.floor() as i32);
    if biome.column != Some(column) {
        biome.column = Some(column);
        biome.name = streamer.generator.provider.get_biomes(column.0, column.1, 1, 1)[0].name;
    }
    let chunk = chunk_of(pos);
    let s = &mut *streamer;
    let cps = s.stats.chunks_per_sec(Instant::now());
    let fps = fps(&diagnostics).map_or("-".into(), |v| format!("{v:.0}"));
    let mem = process_mem_mb(&diagnostics).map_or("-".into(), |v| format!("{v:.0} MB"));
    let e = view.eye;
    let (sky, block) = neighbor_light(s, e.x.floor() as i32, e.y.floor() as i32, e.z.floor() as i32);
    let mode = if player.flying {
        format!("flying, speed {:.3}", player.fly_speed)
    } else if player.is_sneaking() {
        "sneaking".into()
    } else if player.sprinting {
        "sprinting".into()
    } else {
        "walking".into()
    };
    let free = s.brake.available_mb;
    let brake = if s.brake.on { "   generation paused: low memory" } else { "" };
    text.0 = format!(
        "FPS {fps}   RAM {mem}   free {free} MB{brake}\n\
         chunks/s {cps}   avg gen {:.2} ms   avg mesh {:.2} ms\n\
         loaded {}   tracked {}   pending {}\n\
         generated {}   faces {}\n\
         seed {}   radius {}   workers {}\n\
         xyz {:.1} {:.1} {:.1}   chunk {} {}   {mode}\n\
         biome {}\n\
         time {} (day {})   light sky {sky} block {block}   {:?}\n\
         {CONTROLS}",
        s.stats.avg_gen_ms(),
        s.stats.avg_mesh_ms(),
        s.loaded(),
        s.known(),
        s.pending(),
        s.stats.generated_total,
        s.stats.faces,
        s.generator.seed(),
        s.radius,
        s.workers,
        pos.x,
        pos.y,
        pos.z,
        chunk.0,
        chunk.1,
        biome.name,
        clock.world_time.rem_euclid(24000),
        clock.world_time.div_euclid(24000),
        view.fluid,
    );
}

/// F3 (the game's debug screen key) or H shows and hides the stats, which start hidden; F1
/// (`HideGui`) hides them with the rest of the GUI.
pub fn toggle_hud(
    keys: Res<ButtonInput<KeyCode>>,
    hide: Res<HideGui>,
    mut shown: Local<bool>,
    mut hud: Single<&mut Visibility, With<HudText>>,
) {
    if keys.any_just_pressed([KeyCode::F3, KeyCode::KeyH]) {
        *shown = !*shown;
    }
    let v = if !*shown || hide.0 { Visibility::Hidden } else { Visibility::Inherited };
    if **hud != v {
        **hud = v;
    }
}
