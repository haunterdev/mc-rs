//! Drawing items: `GuiIngame.renderHotbar`'s nine item icons (`RenderItem.renderItemIntoGUI`) plus
//! the off hand's slot and the held items (`ItemRenderer.renderItemInFirstPerson`, both hands).
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
use crate::item::{item_color, model_key, pulling_key, ItemModel};
use crate::material::{ChunkMaterial, ItemMaterial, ItemParams};
use crate::sky::{BiomeColors, Clock, View};
use crate::stream::ChunkMaterials;
use crate::Atlas;

/// Draws the hotbar icons, after the UI.
#[derive(Component)]
pub struct ItemCamera;

/// The icon in hotbar slot `.0`; [`OFFHAND_SLOT`] is the off hand's icon.
#[derive(Component)]
pub struct HotbarIcon(pub usize);

/// The off hand is drawn as a tenth icon to the left of the hotbar (`GuiIngame.renderHotbar`), so it
/// shares the nine hotbar icons' machinery at this index.
pub const OFFHAND_SLOT: usize = 9;

/// The item in the main hand.
#[derive(Component)]
pub struct HeldItem;

/// The item in the off hand (first person).
#[derive(Component)]
pub struct OffHeldItem;

/// An item's mesh and its display transforms.
pub struct ItemMesh {
    pub mesh: Handle<Mesh>,
    pub gui: Mat4,
    pub first_person: Mat4,
    /// `FIRST_PERSON_LEFT_HAND`: the off hand (`ItemRenderer.renderItemInFirstPerson`).
    pub first_person_left: Mat4,
    /// In another player's hand (players/held.rs).
    pub third_person: Mat4,
    /// `THIRD_PERSON_LEFT_HAND`: the off hand of another player (players/held.rs).
    pub third_person_left: Mat4,
    /// `GROUND` as a [`crate::item::Display`], for `RenderEntityItem`.
    pub ground: crate::item::Display,
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
    /// The same three with the glint pass (`renderEffect`).
    pub glint_flat: Handle<ItemMaterial>,
    pub glint_3d: Handle<ItemMaterial>,
    pub glint_hand: Handle<ItemMaterial>,
}

impl ItemMaterials {
    pub fn all(&self) -> [&Handle<ItemMaterial>; 6] {
        [&self.gui_flat, &self.gui_3d, &self.hand, &self.glint_flat, &self.glint_3d, &self.glint_hand]
    }

    /// The glint pass of an item drawn with `base`.
    pub fn glint_of(&self, gui3d: bool, hand: bool) -> &Handle<ItemMaterial> {
        if hand {
            &self.glint_hand
        } else if gui3d {
            &self.glint_3d
        } else {
            &self.glint_flat
        }
    }
}

/// `textures/misc/enchanted_item_glint.png`, repeating.
#[derive(Resource)]
pub struct GlintImage(pub Handle<Image>);

/// The glint pass of `HotbarIcon(n)`.
#[derive(Component)]
pub struct HotbarGlint(pub usize);

/// The glint pass of the held item.
#[derive(Component)]
pub struct HeldGlint;

/// The glint pass of the off hand item.
#[derive(Component)]
pub struct OffHeldGlint;

/// `RenderItem.renderEffect`'s scroll: layer one `time % 3000 / 3000 / 8`, layer two the negative
/// of `time % 4873 / 4873 / 8`, for a time in milliseconds.
pub fn glint_scroll(ms: u64) -> (f32, f32) {
    ((ms % 3000) as f32 / 3000.0 / 8.0, -((ms % 4873) as f32 / 4873.0 / 8.0))
}

/// Moves the glint materials along.
pub fn scroll_glint(time: Res<Time<Real>>, materials: Res<ItemMaterials>, mut assets: ResMut<Assets<ItemMaterial>>) {
    let (a, b) = glint_scroll(time.elapsed().as_millis() as u64);
    for h in [&materials.glint_flat, &materials.glint_3d, &materials.glint_hand] {
        if let Some(mut m) = assets.get_mut(h) {
            m.item.glint.y = a;
            m.item.glint.z = b;
        }
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
    glint: Res<GlintImage>,
) {
    let mut out = ItemMeshes::default();
    for (&(item, meta), model) in &atlas.0.items {
        let mesh = meshes.add(item_mesh(model, |t| item_color(item, meta, t, &biome_colors.0)));
        out.0.insert(
            (item, meta),
            ItemMesh {
                mesh,
                gui: model.gui.matrix(),
                first_person: model.first_person.matrix(),
                first_person_left: model.first_person_left.matrix_left(),
                third_person: model.third_person.matrix(),
                third_person_left: model.third_person_left.matrix_left(),
                ground: model.ground,
                gui3d: model.gui3d,
            },
        );
    }
    commands.insert_resource(out);

    let chunk = chunks.get(&chunk_materials.solid).expect("chunk material exists");
    let [w0, w1] = gui_lights();
    let mut material = |lit: bool, hand: bool, pass: bool| {
        materials.add(ItemMaterial {
            params: chunk.params.clone(),
            blocks: chunk.blocks.clone(),
            env: env.image.clone(),
            item: ItemParams { w0: w0.extend(0.0), w1: w1.extend(0.0), flags: UVec4::new(lit as u32, hand as u32, 0, 0), glint: Vec4::new(pass as u32 as f32, 0.0, 0.0, 0.0) },
            glint: glint.0.clone(),
        })
    };
    let item_materials = ItemMaterials {
        gui_flat: material(false, false, false),
        gui_3d: material(true, false, false),
        hand: material(true, true, false),
        glint_flat: material(false, false, true),
        glint_3d: material(true, false, true),
        glint_hand: material(true, true, true),
    };

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
    for l in 0..9 {
        commands.spawn((
            HotbarGlint(l),
            Mesh3d::default(),
            MeshMaterial3d(item_materials.glint_flat.clone()),
            RenderLayers::layer(3),
            NoFrustumCulling,
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    // `GuiIngame.renderHotbar`: the off hand's slot icon, drawn like a tenth hotbar icon.
    commands.spawn((
        HotbarGlint(OFFHAND_SLOT),
        Mesh3d::default(),
        MeshMaterial3d(item_materials.glint_flat.clone()),
        RenderLayers::layer(3),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        HotbarIcon(OFFHAND_SLOT),
        Mesh3d::default(),
        MeshMaterial3d(item_materials.gui_flat.clone()),
        RenderLayers::layer(3),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        HeldGlint,
        Mesh3d::default(),
        MeshMaterial3d(item_materials.glint_hand.clone()),
        RenderLayers::layer(2),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        HeldItem,
        Mesh3d::default(),
        MeshMaterial3d(item_materials.hand.clone()),
        RenderLayers::layer(2),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        OffHeldGlint,
        Mesh3d::default(),
        MeshMaterial3d(item_materials.glint_hand.clone()),
        RenderLayers::layer(2),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.spawn((
        OffHeldItem,
        Mesh3d::default(),
        MeshMaterial3d(item_materials.hand.clone()),
        RenderLayers::layer(2),
        NoFrustumCulling,
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.insert_resource(item_materials);
}

/// The screens that cover the hotbar.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Screens<'w> {
    creative: Option<Res<'w, crate::survival::gui::creative::Creative>>,
    death: Option<Res<'w, crate::survival::death::DeathScreen>>,
    windows: Option<Res<'w, crate::survival::gui::window::Windows>>,
}

impl Screens<'_> {
    pub fn open(&self) -> bool {
        self.creative.as_ref().is_some_and(|c| c.screen.is_some())
            || self.death.as_ref().is_some_and(|d| d.open.is_some())
            || self.windows.as_ref().is_some_and(|w| w.screen.is_some())
    }
}

/// `GuiIngame.renderHotbar`'s items and `renderItemInFirstPerson`'s stack, each frame.
/// The stack count and durability bar are survival/hud.rs `hotbar_counts`. DROPPED: the pickup
/// pop (`animationsToGo`). The options menu draws on a camera over this one (chat.rs
/// `ScreenCamera`), so it covers the icons and its backdrop dims them. Under the creative inventory they move back past the screen's backdrop, which covers them.
#[allow(clippy::too_many_arguments)]
/// The model of hotbar slot `slot`: `renderItemInHotbar` passes the player, so the bow in use shows
/// its pulling model there too.
fn icon_key(inv: &Inventory, slot: usize, hand: &Hand) -> Option<(u16, u16)> {
    let stack = &inv.main[slot];
    let key = model_key(stack)?;
    if slot == inv.current_item {
        return Some(pulling_key(stack, &hand.in_use).unwrap_or(key));
    }
    Some(key)
}

pub fn update_items(
    window: Single<&Window, With<PrimaryWindow>>,
    inv: Res<Inventory>,
    hide: Res<HideGui>,
    screens: Screens,
    hand: Res<Hand>,
    view: Res<View>,
    clock: Res<Clock>,
    items: Res<ItemMeshes>,
    materials: Res<ItemMaterials>,
    mut camera: Single<&mut Projection, With<ItemCamera>>,
    mut icons: Query<
        (&HotbarIcon, &mut Mesh3d, &mut MeshMaterial3d<ItemMaterial>, &mut Transform, &mut Visibility),
        (Without<HeldItem>, Without<OffHeldItem>, Without<HotbarGlint>, Without<HeldGlint>, Without<OffHeldGlint>),
    >,
    held: Single<
        (&mut Mesh3d, &mut Transform, &mut Visibility),
        (With<HeldItem>, Without<OffHeldItem>, Without<HotbarIcon>, Without<HotbarGlint>, Without<HeldGlint>, Without<OffHeldGlint>),
    >,
    (mut glints, held_glint, off_held_glint): (
        Query<
            (&HotbarGlint, &mut Mesh3d, &mut MeshMaterial3d<ItemMaterial>, &mut Transform, &mut Visibility),
            (Without<HeldItem>, Without<OffHeldItem>, Without<HotbarIcon>, Without<HeldGlint>, Without<OffHeldGlint>),
        >,
        Single<
            (&mut Mesh3d, &mut Transform, &mut Visibility),
            (With<HeldGlint>, Without<HeldItem>, Without<OffHeldItem>, Without<HotbarIcon>, Without<HotbarGlint>, Without<OffHeldGlint>),
        >,
        Single<
            (&mut Mesh3d, &mut Transform, &mut Visibility),
            (With<OffHeldGlint>, Without<HeldItem>, Without<OffHeldItem>, Without<HotbarIcon>, Without<HotbarGlint>, Without<HeldGlint>),
        >,
    ),
    off_held: Single<
        (&mut Mesh3d, &mut Transform, &mut Visibility),
        (With<OffHeldItem>, Without<HeldItem>, Without<HotbarIcon>, Without<HotbarGlint>, Without<HeldGlint>, Without<OffHeldGlint>),
    >,
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
    let under = screens.open();
    let behind = Mat4::from_translation(Vec3::Z * if under { -500.0 } else { 0.0 });
    for (icon, mut mesh, mut material, mut transform, mut visibility) in &mut icons {
        let off = icon.0 == OFFHAND_SLOT;
        let item = if off { model_key(&inv.get_stack_in_slot(40)) } else { icon_key(&inv, icon.0, &hand) }.and_then(|key| items.0.get(&key));
        let Some(item) = item.filter(|_| !hide.0) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let x = if off { sw / 2 - 91 - 26 } else { sw / 2 - 90 + icon.0 as i32 * 20 + 2 };
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
    // `renderEffect` over each enchanted icon: the same mesh and place, the glint material.
    for (g, mut mesh, mut material, mut transform, mut visibility) in &mut glints {
        let off = g.0 == OFFHAND_SLOT;
        let stack = if off { inv.get_stack_in_slot(40) } else { inv.main[g.0] };
        let item = if off { model_key(&stack) } else { icon_key(&inv, g.0, &hand) }.and_then(|key| items.0.get(&key));
        match item.filter(|_| !hide.0 && stack.has_effect()) {
            Some(item) => {
                let x = if off { sw / 2 - 91 - 26 } else { sw / 2 - 90 + g.0 as i32 * 20 + 2 };
                *transform = Transform::from_matrix(behind * gui_matrix(x, sh - 16 - 3, item.gui));
                if mesh.0 != item.mesh {
                    mesh.0 = item.mesh.clone();
                }
                let m = materials.glint_of(item.gui3d, false);
                if material.0 != *m {
                    material.0 = m.clone();
                }
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }

    let (mut mesh, mut transform, mut visibility) = held.into_inner();
    let item = model_key(&hand.shown).map(|k| pulling_key(&hand.shown, &hand.in_use).unwrap_or(k)).and_then(|key| items.0.get(&key));
    match item.filter(|_| !hide.0 && view.third_person == 0) {
        Some(item) => {
            if mesh.0 != item.mesh {
                mesh.0 = item.mesh.clone();
            }
            *transform = Transform::from_matrix(view.bob * held_matrix(&hand, true, view.pitch, view.yaw, clock.partial, item.first_person));
            *visibility = Visibility::Inherited;
            let (mut gmesh, mut gtransform, mut gvisibility) = held_glint.into_inner();
            if hand.shown.has_effect() {
                if gmesh.0 != item.mesh {
                    gmesh.0 = item.mesh.clone();
                }
                *gtransform = *transform;
                *gvisibility = Visibility::Inherited;
            } else {
                *gvisibility = Visibility::Hidden;
            }
        }
        None => {
            *visibility = Visibility::Hidden;
            *held_glint.into_inner().2 = Visibility::Hidden;
        }
    }

    // The off hand, drawn from `itemStackOffHand` and its `firstperson_lefthand` transform.
    // `renderItemInFirstPerson`: a main-hand bow that is being drawn suppresses the other hand.
    let bow_active = hand.in_use.bow && hand.in_use.count > 0;
    let (mut mesh, mut transform, mut visibility) = off_held.into_inner();
    let item = model_key(&hand.shown_off).and_then(|key| items.0.get(&key));
    match item.filter(|_| !hide.0 && view.third_person == 0 && !bow_active) {
        Some(item) => {
            if mesh.0 != item.mesh {
                mesh.0 = item.mesh.clone();
            }
            *transform = Transform::from_matrix(view.bob * held_matrix(&hand, false, view.pitch, view.yaw, clock.partial, item.first_person_left));
            *visibility = Visibility::Inherited;
            let (mut gmesh, mut gtransform, mut gvisibility) = off_held_glint.into_inner();
            if hand.shown_off.has_effect() {
                if gmesh.0 != item.mesh {
                    gmesh.0 = item.mesh.clone();
                }
                *gtransform = *transform;
                *gvisibility = Visibility::Inherited;
            } else {
                *gvisibility = Visibility::Hidden;
            }
        }
        None => {
            *visibility = Visibility::Hidden;
            *off_held_glint.into_inner().2 = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glint_layers_scroll_at_vanillas_periods() {
        assert_eq!(glint_scroll(0), (0.0, 0.0));
        let (a, b) = glint_scroll(1500);
        assert!((a - 0.0625).abs() < 1.0e-6, "{a}");
        assert!(b < 0.0 && b > -0.125);
        // Layer one wraps every 3 s and layer two every 4.873 s.
        assert_eq!(glint_scroll(3000).0, 0.0);
        assert_eq!(glint_scroll(4873).1, -0.0);
    }

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
