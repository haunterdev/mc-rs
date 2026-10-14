//! Drawing items: `GuiIngame.renderHotbar`'s nine item icons (`RenderItem.renderItemIntoGUI`)
//! and the held item (`ItemRenderer.renderItemInFirstPerson` with a stack in the main hand).
//! Each stack draws the model `ItemModelMesher` gives it ([`model_key`]).
//!
//! The icons draw on layer 3 with their own orthographic camera after the UI (dimmed under the
//! options menu), which spans the
//! window in GUI units (`ScaledResolution`) with y up, so GUI point (x, y) is world (x, -y). The
//! held item draws on the hand camera's layer 2.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::ScalingMode;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::env::Env;
use crate::hand::{gui_scale, held_matrix, standard_lights, Hand, HideGui};
use crate::hotbar::Inventory;
use crate::item::{item_color, model_key, ItemModel};
use crate::material::{ChunkMaterial, ItemMaterial, ItemParams};
use crate::settings::MenuRoot;
use crate::sky::{BiomeColors, Clock, View};
use crate::stream::ChunkMaterials;
use crate::Atlas;

/// Draws the hotbar icons, after the UI.
#[derive(Component)]
pub struct ItemCamera;

/// The icon in hotbar slot `.0`.
#[derive(Component)]
pub struct HotbarIcon(pub usize);

/// The item in the main hand.
#[derive(Component)]
pub struct HeldItem;

/// An item's mesh and its two display transforms.
pub struct ItemMesh {
    pub mesh: Handle<Mesh>,
    pub gui: Mat4,
    pub first_person: Mat4,
    pub gui3d: bool,
}

#[derive(Resource, Default)]
pub struct ItemMeshes(pub HashMap<(u16, u16), ItemMesh>);

/// The three ways an item is drawn: flat in the GUI, lit in the GUI (`isGui3d`), in the hand.
#[derive(Resource)]
pub struct ItemMaterials {
    pub gui_flat: Handle<ItemMaterial>,
    pub gui_3d: Handle<ItemMaterial>,
    pub hand: Handle<ItemMaterial>,
}

impl ItemMaterials {
    pub fn all(&self) -> [&Handle<ItemMaterial>; 3] {
        [&self.gui_flat, &self.gui_3d, &self.hand]
    }
}

/// `RenderItem.renderModel`'s vertices: each quad with its face normal (`putQuadNormal`) and the
/// stack's tint as the whole color (`putColor4` replaces the baked shade). The array layer goes
/// in the color's alpha, as the mesher writes it.
pub fn item_mesh(model: &ItemModel, tint: impl Fn(i32) -> u32) -> Mesh {
    let n = model.quads.len() * 4;
    let (mut pos, mut normal, mut uv, mut color) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    let mut idx = Vec::with_capacity(model.quads.len() * 6);
    for q in &model.quads {
        let c = tint(q.tint);
        let rgb = [(c >> 16 & 255) as f32 / 255.0, (c >> 8 & 255) as f32 / 255.0, (c & 255) as f32 / 255.0];
        let v = q.face.vec();
        let base = pos.len() as u32;
        for k in 0..4 {
            pos.push(q.pos[k]);
            normal.push([v[0] as f32, v[1] as f32, v[2] as f32]);
            uv.push(q.uv[k]);
            color.push([rgb[0], rgb[1], rgb[2], q.sprite.layer as f32]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, color)
        .with_inserted_indices(Indices::U32(idx))
}

/// `RenderItem.setupGuiTransform` and `renderItem`'s centering for an icon at GUI (x, y), in the
/// item camera's world: `translate(x, y, 100)`, `translate(8, 8, 0)`, `scale(1, -1, 1)`,
/// `scale(16)`, the `gui` transform, `translate(-0.5)`. The y flip cancels against the camera's.
pub fn gui_matrix(x: i32, y: i32, display: Mat4) -> Mat4 {
    Mat4::from_translation(Vec3::new(x as f32 + 8.0, -(y as f32 + 8.0), 100.0))
        * Mat4::from_scale(Vec3::splat(16.0))
        * display
        * Mat4::from_translation(Vec3::splat(-0.5))
}

/// `RenderHelper.enableGUIStandardItemLighting`: the item lights under `rotate(-30, 0, 1, 0)` and
/// `rotate(165, 1, 0, 0)`, then into the item camera's world (GUI y points down).
pub fn gui_lights() -> [Vec3; 2] {
    let m = Mat3::from_diagonal(Vec3::new(1.0, -1.0, 1.0)) * Mat3::from_rotation_y((-30f32).to_radians()) * Mat3::from_rotation_x(165f32.to_radians());
    standard_lights().map(|l| m * l)
}

/// Builds every item mesh and the item materials, and spawns the icon camera, the nine icons and
/// the held item.
#[allow(clippy::too_many_arguments)]
pub fn spawn_items(
    mut commands: Commands,
    atlas: Res<Atlas>,
    biome_colors: Res<BiomeColors>,
    chunk_materials: Res<ChunkMaterials>,
    chunks: Res<Assets<ChunkMaterial>>,
    env: Res<Env>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ItemMaterial>>,
) {
    let mut out = ItemMeshes::default();
    for (&(item, meta), model) in &atlas.0.items {
        let mesh = meshes.add(item_mesh(model, |t| item_color(item, meta, t, &biome_colors.0)));
        out.0.insert((item, meta), ItemMesh { mesh, gui: model.gui.matrix(), first_person: model.first_person.matrix(), gui3d: model.gui3d });
    }
    commands.insert_resource(out);

    let chunk = chunks.get(&chunk_materials.solid).expect("chunk material exists");
    let [w0, w1] = gui_lights();
    let mut material = |lit: bool, hand: bool| {
        materials.add(ItemMaterial {
            params: chunk.params.clone(),
            blocks: chunk.blocks.clone(),
            env: env.image.clone(),
            item: ItemParams { w0: w0.extend(0.0), w1: w1.extend(0.0), flags: UVec4::new(lit as u32, hand as u32, 0, 0) },
        })
    };
    let item_materials = ItemMaterials { gui_flat: material(false, false), gui_3d: material(true, false), hand: material(true, true) };

    commands.spawn((
        ItemCamera,
        Camera3d::default(),
        Camera { order: 2, clear_color: ClearColorConfig::None, ..default() },
        Projection::Orthographic(OrthographicProjection {
            near: 0.0,
            far: 2000.0,
            viewport_origin: Vec2::new(0.0, 1.0),
            scaling_mode: ScalingMode::Fixed { width: 1.0, height: 1.0 },
            ..OrthographicProjection::default_3d()
        }),
        Tonemapping::None,
        DebandDither::Disabled,
        Msaa::Off,
        Transform::from_xyz(0.0, 0.0, 1000.0),
        RenderLayers::layer(3),
    ));
    for l in 0..9 {
        commands.spawn((
            HotbarIcon(l),
            Mesh3d::default(),
            MeshMaterial3d(item_materials.gui_flat.clone()),
            RenderLayers::layer(3),
            NoFrustumCulling,
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    commands.spawn((
        HeldItem,
        Mesh3d::default(),
        MeshMaterial3d(item_materials.hand.clone()),
        RenderLayers::layer(2),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.insert_resource(item_materials);
}

/// `GuiIngame.renderHotbar`'s items and `renderItemInFirstPerson`'s stack, each frame.
/// DROPPED: the pickup pop (`animationsToGo`), the stack count and the durability bar. Vanilla
/// draws the menu over the hotbar; here the icons draw after the UI, so under the options menu
/// they stay on screen dimmed as its backdrop dims the bar, and chat leaves them as they are.
/// Under the creative inventory they move back past the screen's backdrop, which covers them.
#[allow(clippy::too_many_arguments)]
pub fn update_items(
    window: Single<&Window, With<PrimaryWindow>>,
    inv: Res<Inventory>,
    hide: Res<HideGui>,
    options: Query<(), With<MenuRoot>>,
    creative: Option<Res<crate::survival::gui::creative::Creative>>,
    hand: Res<Hand>,
    view: Res<View>,
    clock: Res<Clock>,
    items: Res<ItemMeshes>,
    materials: Res<ItemMaterials>,
    mut item_assets: ResMut<Assets<ItemMaterial>>,
    mut dimmed: Local<bool>,
    mut camera: Single<&mut Projection, With<ItemCamera>>,
    mut icons: Query<(&HotbarIcon, &mut Mesh3d, &mut MeshMaterial3d<ItemMaterial>, &mut Transform, &mut Visibility), Without<HeldItem>>,
    held: Single<(&mut Mesh3d, &mut Transform, &mut Visibility), (With<HeldItem>, Without<HotbarIcon>)>,
) {
    let (w, h) = (window.physical_width(), window.physical_height());
    let k = gui_scale(w, h);
    // ScaledResolution: the GUI ortho spans the window over the unrounded scaled size.
    let (gw, gh) = (w as f32 / k as f32, h as f32 / k as f32);
    if let Projection::Orthographic(o) = &**camera {
        if !matches!(o.scaling_mode, ScalingMode::Fixed { width, height } if width == gw && height == gh) {
            if let Projection::Orthographic(o) = &mut **camera {
                o.scaling_mode = ScalingMode::Fixed { width: gw, height: gh };
            }
        }
    }
    let sw = (w as f64 / k as f64).ceil() as i32;
    let sh = (h as f64 / k as f64).ceil() as i32;
    let dim = !options.is_empty();
    if *dimmed != dim {
        *dimmed = dim;
        for m in [&materials.gui_flat, &materials.gui_3d] {
            if let Some(mut m) = item_assets.get_mut(m) {
                m.item.flags.z = dim as u32;
            }
        }
    }
    let under = creative.is_some_and(|c| c.screen.is_some());
    let behind = Mat4::from_translation(Vec3::Z * if under { -500.0 } else { 0.0 });
    for (icon, mut mesh, mut material, mut transform, mut visibility) in &mut icons {
        let item = model_key(&inv.main[icon.0]).and_then(|key| items.0.get(&key));
        let Some(item) = item.filter(|_| !hide.0) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let x = sw / 2 - 90 + icon.0 as i32 * 20 + 2;
        let y = sh - 16 - 3;
        *transform = Transform::from_matrix(behind * gui_matrix(x, y, item.gui));
        if mesh.0 != item.mesh {
            mesh.0 = item.mesh.clone();
        }
        let m = if item.gui3d { &materials.gui_3d } else { &materials.gui_flat };
        if material.0 != *m {
            material.0 = m.clone();
        }
        *visibility = Visibility::Inherited;
    }

    let (mut mesh, mut transform, mut visibility) = held.into_inner();
    let item = model_key(&hand.shown).and_then(|key| items.0.get(&key));
    match item.filter(|_| !hide.0 && view.third_person == 0) {
        Some(item) => {
            if mesh.0 != item.mesh {
                mesh.0 = item.mesh.clone();
            }
            *transform = Transform::from_matrix(view.bob * held_matrix(&hand, view.pitch, view.yaw, clock.partial, item.first_person));
            *visibility = Visibility::Inherited;
        }
        None => *visibility = Visibility::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_keeps_the_winding() {
        assert!(gui_matrix(10, 20, Mat4::IDENTITY).determinant() > 0.0);
        // block/block's gui transform.
        let d = crate::item::Display { rotation: [30.0, 225.0, 0.0], translation: [0.0; 3], scale: [0.625; 3] };
        let m = gui_matrix(0, 0, d.matrix());
        assert!(m.determinant() > 0.0);
        // The block's center lands on the slot's center.
        let c = m.transform_point3(Vec3::splat(0.5));
        assert!((c - Vec3::new(8.0, -8.0, 100.0)).length() < 1.0e-4);
    }

    #[test]
    fn gui_lights_point_up_the_screen() {
        // Light 0 starts mostly up; after rotate(165, x) it points down in GUI space, which is up
        // on the screen, so +y in the item camera's world.
        let [l0, l1] = gui_lights();
        assert!(l0.y > 0.0 && l1.y > 0.0, "{l0} {l1}");
    }
}
