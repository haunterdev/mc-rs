//! Survival's own GUI bands: the HUD (`GuiIngame`) and the death screen (`GuiGameOver`) are
//! drawn as quads in GUI pixels on the item camera's layer 3, like the container screens in
//! `gui/draw.rs`, but in layers of their own so they can show next to an open screen and in
//! both native and streamed sessions.
//!
//! Each layer is one mesh with one texture sheet at its own depth, so the camera's back-to-front
//! sort draws them in vanilla's call order: the HUD, then the death screen's backdrop, buttons
//! and text over it. The HUD sits below every container band (gui/draw.rs starts at depth 1),
//! as vanilla draws the screen after the HUD. The systems that own a layer rebuild it each frame through [`Overlay`].

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::prelude::*;

use super::gui::draw::Quads;
use crate::hand::GuiIcons;
use crate::hotbar::GuiWidgets;
use crate::players::Font;

/// Which texture a layer samples.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sheet {
    Plain,
    Icons,
    Widgets,
    Font,
}

/// The layers, back to front: (depth, sheet).
const LAYERS: [(f32, Sheet); LAYER_COUNT] = [
    (0.2, Sheet::Icons),    // HUD_ICONS
    (0.4, Sheet::Font),     // HUD_TEXT
    (20.0, Sheet::Plain),   // DEATH_BACK
    (21.0, Sheet::Widgets), // DEATH_BUTTONS
    (22.0, Sheet::Font),    // DEATH_TEXT
    (150.0, Sheet::Font),   // HOTBAR_COUNTS
    (151.0, Sheet::Plain),  // HOTBAR_BARS
    (0.5, Sheet::Font),     // RECORD_TEXT
];
pub const LAYER_COUNT: usize = 8;
/// `GuiIngame`'s record "Now Playing" line over the hotbar.
pub const RECORD_TEXT: usize = 7;
/// `gui/icons.png` sprites: hearts, food, bubbles, the xp bar.
pub const HUD_ICONS: usize = 0;
/// The xp level.
pub const HUD_TEXT: usize = 1;
/// `GuiGameOver`'s gradient.
pub const DEATH_BACK: usize = 2;
/// `GuiButton` faces from `widgets.png`.
pub const DEATH_BUTTONS: usize = 3;
/// Its title, message, score and button labels.
pub const DEATH_TEXT: usize = 4;
/// The hotbar's stack sizes and durability bars, over the item icons (depth 100) as
/// `renderItemOverlayIntoGUI` draws them at `zLevel` 200.
pub const HOTBAR_COUNTS: usize = 5;
pub const HOTBAR_BARS: usize = 6;

/// What every layer draws this frame.
#[derive(Resource, Default)]
pub struct Overlay {
    pub layers: [Quads; LAYER_COUNT],
}

#[derive(Component)]
struct Layer(usize);

#[derive(Resource)]
struct Sheets {
    plain: Handle<StandardMaterial>,
    icons: Handle<StandardMaterial>,
    widgets: Handle<StandardMaterial>,
    font: Handle<StandardMaterial>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Overlay>()
        // A bare SurvivalPlugin (the net tests) has no images.
        .add_systems(PostStartup, spawn.run_if(resource_exists::<GuiIcons>.and_then(resource_exists::<GuiWidgets>).and_then(resource_exists::<Font>)))
        .add_systems(PostUpdate, present.run_if(resource_exists::<Sheets>).before(bevy::transform::TransformSystems::Propagate));
}

fn spawn(
    mut commands: Commands,
    icons: Res<GuiIcons>,
    widgets: Res<GuiWidgets>,
    font: Res<Font>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut material = |texture: Option<Handle<Image>>| {
        materials.add(StandardMaterial {
            base_color_texture: texture,
            unlit: true,
            fog_enabled: false,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        })
    };
    let sheets = Sheets {
        plain: material(None),
        icons: material(Some(icons.0.clone())),
        widgets: material(Some(widgets.0.clone())),
        font: material(Some(font.image.clone())),
    };
    for (i, &(z, sheet)) in LAYERS.iter().enumerate() {
        let mut q = Quads::default();
        q.rect(0, 0, 1, 1, 0);
        let m = match sheet {
            Sheet::Plain => sheets.plain.clone(),
            Sheet::Icons => sheets.icons.clone(),
            Sheet::Widgets => sheets.widgets.clone(),
            Sheet::Font => sheets.font.clone(),
        };
        commands.spawn((
            Layer(i),
            Mesh3d(meshes.add(q.mesh())),
            MeshMaterial3d(m),
            RenderLayers::layer(3),
            NoFrustumCulling,
            Transform::from_xyz(0.0, 0.0, z),
            Visibility::Hidden,
        ));
    }
    commands.insert_resource(sheets);
}

/// Puts each layer's quads on screen; an empty layer is hidden.
fn present(overlay: Res<Overlay>, mut meshes: ResMut<Assets<Mesh>>, mut layers: Query<(&Layer, &Mesh3d, &mut Visibility)>) {
    for (layer, mesh, mut visibility) in &mut layers {
        let q = &overlay.layers[layer.0];
        if q.is_empty() {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        let _ = meshes.insert(&mesh.0, q.mesh());
        visibility.set_if_neq(Visibility::Inherited);
    }
}
