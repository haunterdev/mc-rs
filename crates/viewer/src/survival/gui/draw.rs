//! How a container screen reaches the window: `Gui.drawRect`, `drawGradientRect`,
//! `drawTexturedModalRect`, `FontRenderer.drawString` and `RenderItem.renderItemIntoGUI`, all
//! on the item camera's layer 3 in GUI pixels (GUI point (x, y) is world (x, -y)).
//!
//! Java draws in call order with the depth test off; here each screen fills a [`Frame`] of
//! bands, one mesh per band, and [`present`] puts each band at its own depth so the camera's
//! back-to-front sort draws them in the order vanilla does. The items are a pool of
//! [`ScreenIcon`] entities using the hotbar's item meshes and materials. While no screen is
//! open every band and icon is hidden.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use items::ItemStack;

use crate::item::model_key;
use crate::item_render::{gui_matrix, ItemMaterials, ItemMeshes};
use crate::material::ItemMaterial;
use crate::players::Font;

/// `textures/gui/container/creative_inventory/tabs.png` and the backgrounds: the three `tab_*.png`
/// (`Tab::background()` order: items, item_search, inventory), then `container/inventory.png` and
/// `container/crafting_table.png`, `generic_54.png` and `furnace.png`, as GUI (sRGB) images.
/// `empty_slots` is the empty armor and shield slot sprites side by side
/// ([`EMPTY_SLOT_NAMES`]), 16 pixels each.
#[derive(Resource)]
pub struct ContainerImages {
    pub tabs: Handle<Image>,
    pub backgrounds: [Handle<Image>; 8],
    pub empty_slots: Handle<Image>,
}

/// The `getSlotTexture` sprites, in the strip's order: the armor slots head to feet
/// (`ContainerPlayer`'s `EMPTY_SLOT_NAMES`) and the off hand.
pub const EMPTY_SLOT_NAMES: [&str; EMPTY_SLOT_SPRITES] =
    ["empty_armor_slot_helmet", "empty_armor_slot_chestplate", "empty_armor_slot_leggings", "empty_armor_slot_boots", "empty_armor_slot_shield"];
pub const EMPTY_SLOT_SPRITES: usize = 5;

/// What a band is drawn with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Plain,
    Tabs,
    Background,
    Font,
    EmptySlots,
    Galactic,
}

/// The bands, back to front: (depth, look).
const BANDS: [(f32, Look); BAND_COUNT] = [
    (1.0, Look::Plain),        // BACKDROP
    (2.0, Look::Tabs),         // TABS
    (3.0, Look::Background),   // BACKGROUND
    (4.0, Look::Tabs),         // FRONT
    (5.0, Look::Font),         // TEXT
    (6.0, Look::Plain),        // UNDER
    (200.0, Look::Font),       // COUNTS
    (201.0, Look::Plain),      // BARS
    (210.0, Look::Plain),      // HIGHLIGHT
    (400.0, Look::Font),       // CURSOR_COUNT
    (401.0, Look::Plain),      // CURSOR_BARS
    (500.0, Look::Plain),      // TIP_BACK
    (510.0, Look::Font),       // TIP_TEXT
    (5.5, Look::EmptySlots),   // EMPTY_SLOTS
    (5.2, Look::Galactic),     // GALACTIC
];
pub const BAND_COUNT: usize = 15;

/// The clue text of the enchanting table in the standard galactic alphabet.
pub const GALACTIC: usize = 14;
/// `drawDefaultBackground`
pub const BACKDROP: usize = 0;
/// The tabs behind the background.
pub const TABS: usize = 1;
pub const BACKGROUND: usize = 2;
/// The scrollbar and the selected tab.
pub const FRONT: usize = 3;
/// The title and the search field.
pub const TEXT: usize = 4;
/// The drag-split marks under the items, and the search field's bar cursor.
pub const UNDER: usize = 5;
/// The slots' stack sizes, and their durability bars.
pub const COUNTS: usize = 6;
pub const BARS: usize = 7;
/// The hovered slot's white wash, over its item.
pub const HIGHLIGHT: usize = 8;
/// The cursor stack's size and bar.
pub const CURSOR_COUNT: usize = 9;
pub const CURSOR_BARS: usize = 10;
pub const TIP_BACK: usize = 11;
pub const TIP_TEXT: usize = 12;
/// An empty slot's `getSlotTexture` sprite, where its item would be.
pub const EMPTY_SLOTS: usize = 13;

/// Item depths: the slots and tab icons at the hotbar's, the cursor stack over the counts
/// (`drawItemStack`'s `zLevel` 200 and `translate(0, 0, 32)`).
pub const SLOT_Z: f32 = 0.0;
pub const CURSOR_Z: f32 = 200.0;

/// One band's quads in GUI pixels.
#[derive(Default)]
pub struct Quads {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    color: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

/// An ARGB color as linear vertex color (the GUI blends in sRGB, the vertex color is linear).
fn vertex_color(c: u32) -> [f32; 4] {
    let l = |b: u32| Color::srgb_u8(b as u8, 0, 0).to_linear().red;
    [l(c >> 16 & 255), l(c >> 8 & 255), l(c & 255), (c >> 24 & 255) as f32 / 255.0]
}

impl Quads {
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    /// A quad from GUI (x0, y0) to (x1, y1) with texture rect `uv` (0..1), `top` along its top
    /// edge and `bottom` along its bottom.
    pub fn quad(&mut self, [x0, y0, x1, y1]: [f32; 4], [u0, v0, u1, v1]: [f32; 4], top: u32, bottom: u32) {
        let base = self.pos.len() as u32;
        self.pos.extend_from_slice(&[[x0, -y0, 0.0], [x0, -y1, 0.0], [x1, -y1, 0.0], [x1, -y0, 0.0]]);
        self.uv.extend_from_slice(&[[u0, v0], [u0, v1], [u1, v1], [u1, v0]]);
        let (t, b) = (vertex_color(top), vertex_color(bottom));
        self.color.extend_from_slice(&[t, b, b, t]);
        self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// `Gui.drawRect(left, top, right, bottom, color)`: the corners in either order.
    pub fn rect(&mut self, left: i32, top: i32, right: i32, bottom: i32, color: u32) {
        self.gradient(left.min(right), top.min(bottom), left.max(right), top.max(bottom), color, color);
    }

    /// `Gui.drawGradientRect`: `start` along the top, `end` along the bottom.
    pub fn gradient(&mut self, left: i32, top: i32, right: i32, bottom: i32, start: u32, end: u32) {
        self.quad([left as f32, top as f32, right as f32, bottom as f32], [0.0; 4], start, end);
    }

    /// `Gui.drawTexturedModalRect(x, y, u, v, w, h)` from a 256 x 256 texture.
    pub fn sprite(&mut self, x: i32, y: i32, u: i32, v: i32, w: i32, h: i32) {
        let s = 1.0 / 256.0;
        self.quad(
            [x as f32, y as f32, (x + w) as f32, (y + h) as f32],
            [u as f32 * s, v as f32 * s, (u + w) as f32 * s, (v + h) as f32 * s],
            0xFFFF_FFFF,
            0xFFFF_FFFF,
        );
    }

    /// `FontRenderer.renderString` for one style: each glyph `renderDefaultChar` draws
    /// (`charWidth - 1.01` wide, 7.99 tall), and again one pixel right when bold. Returns the
    /// x after the last glyph.
    fn glyphs(&mut self, font: &Font, s: &str, mut x: f32, y: f32, color: u32, bold: bool) -> f32 {
        let t = 1.0 / 128.0;
        for b in s.bytes() {
            let w = font.widths[b as usize] as f32;
            let f = w - 0.01;
            let (gx, gy) = ((b % 16) as f32 * 8.0, (b / 16) as f32 * 8.0);
            let uv = [gx * t, gy * t, (gx + f - 1.0) * t, (gy + 7.99) * t];
            self.quad([x, y, x + f - 1.0, y + 7.99], uv, color, color);
            if bold {
                self.quad([x + 1.0, y, x + f, y + 7.99], uv, color, color);
            }
            x += w + bold as u8 as f32;
        }
        x
    }

    /// `FontRenderer.drawString(text, x, y, color, dropShadow)`: a color with no alpha is
    /// opaque, and the shadow is the color at a quarter one pixel down and right, drawn first.
    /// Returns the x after the text, as `drawString` does (plus one with a shadow).
    pub fn text(&mut self, font: &Font, s: &str, x: i32, y: i32, color: u32, shadow: bool, bold: bool) -> i32 {
        let color = if color & 0xFC00_0000 == 0 { color | 0xFF00_0000 } else { color };
        let (x, y) = (x as f32, y as f32);
        let mut end = x;
        if shadow {
            let dark = (color & 0x00FC_FCFC) >> 2 | color & 0xFF00_0000;
            end = self.glyphs(font, s, x + 1.0, y + 1.0, dark, bold);
        }
        end = end.max(self.glyphs(font, s, x, y, color, bold));
        end as i32
    }

    /// `GlStateManager.scale(k, k, k)` around the origin, for the vertices added since
    /// `from` (a [`Quads::vertices`] count taken before drawing).
    pub fn scale_from(&mut self, from: usize, k: f32) {
        for p in &mut self.pos[from..] {
            p[0] *= k;
            p[1] *= k;
        }
    }

    /// How many vertices the band holds.
    pub fn vertices(&self) -> usize {
        self.pos.len()
    }

    pub fn mesh(&self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.color.clone())
            .with_inserted_indices(Indices::U32(self.idx.clone()))
    }
}

/// `FontRenderer.getStringWidth`, plus one a character when bold.
pub fn string_width(font: &Font, s: &str, bold: bool) -> i32 {
    font.string_width(s) + if bold { s.len() as i32 } else { 0 }
}

/// `FontRenderer.trimStringToWidth(text, width, reverse)`: the most characters from the start
/// (or the end) whose widths fit in `width`.
pub fn trim_to_width<'a>(font: &Font, s: &'a str, width: i32, reverse: bool) -> &'a str {
    let mut j = 0;
    if reverse {
        let mut start = s.len();
        for (i, b) in s.bytes().enumerate().rev() {
            j += font.widths[b as usize] as i32;
            if j > width {
                break;
            }
            start = i;
        }
        &s[start..]
    } else {
        let mut end = 0;
        for (i, b) in s.bytes().enumerate() {
            j += font.widths[b as usize] as i32;
            if j > width {
                break;
            }
            end = i + 1;
        }
        &s[..end]
    }
}

/// An item drawn at GUI (x, y), `z` over the slot depth.
#[derive(Clone, Copy)]
pub struct Icon {
    pub stack: ItemStack,
    pub x: i32,
    pub y: i32,
    pub z: f32,
}

/// What the open screen draws this frame. `open` false hides it all.
#[derive(Resource, Default)]
pub struct Frame {
    pub open: bool,
    pub bands: [Quads; BAND_COUNT],
    pub icons: Vec<Icon>,
    /// Which of [`ContainerImages::backgrounds`] the background band shows.
    pub background: usize,
    /// `GuiInventory.drawEntityOnScreen(posX, posY, scale, mouseX, mouseY, player)`: the player
    /// model this frame, drawn by `players::inventory_model`.
    pub player_model: Option<PlayerModel>,
}

/// The arguments of `GuiInventory.drawEntityOnScreen`: the feet at GUI pixel (`x`, `y`), `scale`
/// GUI pixels per block, and the `mouseX` and `mouseY` it turns toward (the model's centre minus
/// the pointer, as the callers pass them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerModel {
    pub x: i32,
    pub y: i32,
    pub scale: i32,
    pub mouse_x: f32,
    pub mouse_y: f32,
}

/// `renderItemOverlayIntoGUI`'s stack size (or `text`), bottom right of the slot at (x, y) with a
/// shadow. Shared by the container screens and the hotbar.
pub fn stack_count(q: &mut Quads, font: &Font, stack: &ItemStack, x: i32, y: i32, text: Option<(String, u32)>) {
    if stack.is_empty() {
        return;
    }
    if stack.get_count() != 1 || text.is_some() {
        let (s, color) = text.unwrap_or_else(|| (stack.get_count().to_string(), 0xFFFFFF));
        let w = string_width(font, &s, false);
        q.text(font, &s, x + 19 - 2 - w, y + 6 + 3, color, true, false);
    }
}

/// `renderItemOverlayIntoGUI`'s durability bar of a damaged item.
pub fn durability_bar(q: &mut Quads, stack: &ItemStack, x: i32, y: i32) {
    if stack.is_empty() || !stack.is_item_damaged() {
        return;
    }
    let (f, f1) = (stack.get_metadata() as f32, stack.get_max_damage() as f32);
    let f2 = ((f1 - f) / f1).max(0.0);
    let i = (13.0 - f * 13.0 / f1).round() as i32;
    let j = hsv_to_rgb(f2 / 3.0, 1.0, 1.0);
    q.rect(x + 2, y + 13, x + 2 + 13, y + 13 + 2, 0xFF00_0000);
    q.rect(x + 2, y + 13, x + 2 + i, y + 13 + 1, 0xFF00_0000 | j);
}

impl Frame {
    /// Empties the frame for a new draw.
    pub fn clear(&mut self) {
        self.open = false;
        for b in &mut self.bands {
            *b = Quads::default();
        }
        self.icons.clear();
        self.player_model = None;
    }

    /// `RenderItem.renderItemOverlayIntoGUI`: the stack size (or `text`) bottom right with a
    /// shadow, and the durability bar of a damaged item. DROPPED: the cooldown overlay
    /// (`CooldownTracker`), which no item here starts.
    pub fn overlay(&mut self, font: &Font, stack: &ItemStack, x: i32, y: i32, text: Option<(String, u32)>, text_band: usize, bar_band: usize) {
        stack_count(&mut self.bands[text_band], font, stack, x, y, text);
        durability_bar(&mut self.bands[bar_band], stack, x, y);
    }

    /// `GuiScreen.drawHoveringText(lines, x, y)` on a `width` x `height` screen. Each line is
    /// (text, color, bold).
    pub fn hovering_text(&mut self, font: &Font, lines: &[(String, u32, bool)], x: i32, y: i32, width: i32, height: i32) {
        if lines.is_empty() {
            return;
        }
        let i = lines.iter().map(|(s, _, b)| string_width(font, s, *b)).max().unwrap_or(0);
        let mut l1 = x + 12;
        let mut i2 = y - 12;
        let mut k = 8;
        if lines.len() > 1 {
            k += 2 + (lines.len() as i32 - 1) * 10;
        }
        if l1 + i > width {
            l1 -= 28 + i;
        }
        if i2 + k + 6 > height {
            i2 = height - k - 6;
        }
        let back = 0xF010_0010;
        let q = &mut self.bands[TIP_BACK];
        q.gradient(l1 - 3, i2 - 4, l1 + i + 3, i2 - 3, back, back);
        q.gradient(l1 - 3, i2 + k + 3, l1 + i + 3, i2 + k + 4, back, back);
        q.gradient(l1 - 3, i2 - 3, l1 + i + 3, i2 + k + 3, back, back);
        q.gradient(l1 - 4, i2 - 3, l1 - 3, i2 + k + 3, back, back);
        q.gradient(l1 + i + 3, i2 - 3, l1 + i + 4, i2 + k + 3, back, back);
        let (top, bottom) = (0x5050_00FF, 0x5028_007F);
        q.gradient(l1 - 3, i2 - 3 + 1, l1 - 3 + 1, i2 + k + 3 - 1, top, bottom);
        q.gradient(l1 + i + 2, i2 - 3 + 1, l1 + i + 3, i2 + k + 3 - 1, top, bottom);
        q.gradient(l1 - 3, i2 - 3, l1 + i + 3, i2 - 3 + 1, top, top);
        q.gradient(l1 - 3, i2 + k + 2, l1 + i + 3, i2 + k + 3, bottom, bottom);
        for (n, (s, color, bold)) in lines.iter().enumerate() {
            self.bands[TIP_TEXT].text(font, s, l1, i2, *color, true, *bold);
            if n == 0 {
                i2 += 2;
            }
            i2 += 10;
        }
    }
}

/// `MathHelper.hsvToRGB`
pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> u32 {
    let i = (hue * 6.0) as i32 % 6;
    let f = hue * 6.0 - i as f32;
    let f1 = value * (1.0 - saturation);
    let f2 = value * (1.0 - f * saturation);
    let f3 = value * (1.0 - (1.0 - f) * saturation);
    let (r, g, b) = match i {
        0 => (value, f3, f1),
        1 => (f2, value, f1),
        2 => (f1, value, f3),
        3 => (f1, f2, value),
        4 => (f3, f1, value),
        _ => (value, f1, f2),
    };
    let c = |v: f32| ((v * 255.0) as i32).clamp(0, 255) as u32;
    c(r) << 16 | c(g) << 8 | c(b)
}

/// A band's entity: its index in [`BANDS`].
#[derive(Component)]
pub struct Band(usize);

/// One of the screen's item entities.
#[derive(Component)]
pub struct ScreenIcon(usize);

/// The glint pass over `ScreenIcon(.0)`.
#[derive(Component)]
pub struct ScreenGlint(usize);

/// The band materials.
#[derive(Resource)]
pub struct BandMaterials {
    plain: Handle<StandardMaterial>,
    tabs: Handle<StandardMaterial>,
    backgrounds: [Handle<StandardMaterial>; 8],
    galactic: Handle<StandardMaterial>,
    font: Handle<StandardMaterial>,
    empty_slots: Handle<StandardMaterial>,
}

/// Enough for a full container, the tab icons and the cursor stack.
const ICONS: usize = 96;
/// Frames the icon pool is kept warm after start, and where it is parked meanwhile.
const WARM_FRAMES: u32 = 240;
const PARKED: Vec3 = Vec3::new(-100_000.0, 0.0, 0.0);

/// Makes the band materials and spawns the hidden bands and icons. Runs after Startup, once
/// main.rs's `setup` has made the images.
pub fn spawn_screen(
    mut commands: Commands,
    images: Res<ContainerImages>,
    font: Res<Font>,
    galactic: Option<Res<super::enchanting::Galactic>>,
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
    let m = BandMaterials {
        plain: material(None),
        tabs: material(Some(images.tabs.clone())),
        backgrounds: images.backgrounds.clone().map(|b| material(Some(b))),
        font: material(Some(font.image.clone())),
        empty_slots: material(Some(images.empty_slots.clone())),
        galactic: material(Some(galactic.map_or_else(|| font.image.clone(), |g| g.0.image.clone()))),
    };
    for (i, &(z, look)) in BANDS.iter().enumerate() {
        let mut q = Quads::default();
        q.rect(0, 0, 1, 1, 0);
        let material = match look {
            Look::Plain => m.plain.clone(),
            Look::Tabs => m.tabs.clone(),
            Look::Background => m.backgrounds[0].clone(),
            Look::Font => m.font.clone(),
            Look::EmptySlots => m.empty_slots.clone(),
            Look::Galactic => m.galactic.clone(),
        };
        commands.spawn((
            Band(i),
            Mesh3d(meshes.add(q.mesh())),
            MeshMaterial3d(material),
            RenderLayers::layer(3),
            NoFrustumCulling,
            Transform::from_xyz(0.0, 0.0, z),
            Visibility::Hidden,
        ));
    }
    for i in 0..ICONS {
        commands.spawn((
            ScreenIcon(i),
            Mesh3d::default(),
            MeshMaterial3d::<ItemMaterial>::default(),
            RenderLayers::layer(3),
            NoFrustumCulling,
            Transform::default(),
            Visibility::Hidden,
        ));
        commands.spawn((
            ScreenGlint(i),
            Mesh3d::default(),
            MeshMaterial3d::<ItemMaterial>::default(),
            RenderLayers::layer(3),
            NoFrustumCulling,
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    commands.insert_resource(m);
}

/// Puts the [`Frame`] on screen: each band's mesh, the background's texture, and the items.
#[allow(clippy::type_complexity)]
pub fn present(
    frame: Res<Frame>,
    band_materials: Option<Res<BandMaterials>>,
    items: Res<ItemMeshes>,
    item_materials: Res<ItemMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut shown: Local<bool>,
    mut frames: Local<u32>,
    mut bands: Query<(&Band, &Mesh3d, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility), Without<ScreenIcon>>,
    mut icons: Query<(&ScreenIcon, &mut Mesh3d, &mut MeshMaterial3d<ItemMaterial>, &mut Transform, &mut Visibility), (Without<Band>, Without<ScreenGlint>)>,
    mut glints: Query<(&ScreenGlint, &mut Mesh3d, &mut MeshMaterial3d<ItemMaterial>, &mut Transform, &mut Visibility), (Without<Band>, Without<ScreenIcon>)>,
) {
    let Some(band_materials) = band_materials else { return };
    // The first open showed no items for a split second, because the icon pool's meshes,
    // materials and pipelines were first used on the frame the screen opened. For the first
    // WARM_FRAMES frames every icon and glint is drawn parked off screen with a real flat or 3D
    // mesh and its material, so the renderer has them ready before any screen opens.
    if *frames < WARM_FRAMES && !frame.open {
        *frames += 1;
        let flat = items.0.values().find(|m| !m.gui3d);
        let block = items.0.values().find(|m| m.gui3d);
        let parked = Transform::from_translation(PARKED);
        let (flat_material, block_material) = (&item_materials.gui_flat, &item_materials.gui_3d);
        for (n, (_, mut mesh, mut material, mut transform, mut visibility)) in icons.iter_mut().enumerate() {
            let (item, m) = if n % 2 == 0 { (flat, flat_material) } else { (block, block_material) };
            if let Some(item) = item {
                mesh.0 = item.mesh.clone();
                material.0 = m.clone();
                *transform = parked;
                *visibility = Visibility::Inherited;
            }
        }
        for (n, (_, mut mesh, mut material, mut transform, mut visibility)) in glints.iter_mut().enumerate() {
            if let Some(item) = [flat, block][n % 2] {
                mesh.0 = item.mesh.clone();
                material.0 = item_materials.glint_of(item.gui3d, false).clone();
                *transform = parked;
                *visibility = Visibility::Inherited;
            }
        }
        return;
    }
    if !frame.open && !*shown {
        if *frames == WARM_FRAMES {
            *frames += 1;
            for (_, _, _, _, mut visibility) in &mut icons {
                *visibility = Visibility::Hidden;
            }
            for (_, _, _, _, mut visibility) in &mut glints {
                *visibility = Visibility::Hidden;
            }
        }
        return;
    }
    *frames = WARM_FRAMES + 1;
    *shown = frame.open;
    for (band, mesh, mut material, mut visibility) in &mut bands {
        let q = &frame.bands[band.0];
        if !frame.open || q.is_empty() {
            *visibility = Visibility::Hidden;
            continue;
        }
        let _ = meshes.insert(&mesh.0, q.mesh());
        if band.0 == BACKGROUND {
            let m = &band_materials.backgrounds[frame.background.min(7)];
            if material.0 != *m {
                material.0 = m.clone();
            }
        }
        *visibility = Visibility::Inherited;
    }
    for (glint, mut mesh, mut material, mut transform, mut visibility) in &mut glints {
        let shown = frame.icons.get(glint.0).filter(|i| frame.open && i.stack.has_effect());
        let Some((i, item)) = shown.and_then(|i| model_key(&i.stack).and_then(|k| items.0.get(&k)).map(|item| (i, item))) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *transform = Transform::from_matrix(Mat4::from_translation(Vec3::Z * i.z) * gui_matrix(i.x, i.y, item.gui));
        if mesh.0 != item.mesh {
            mesh.0 = item.mesh.clone();
        }
        let m = item_materials.glint_of(item.gui3d, false);
        if material.0 != *m {
            material.0 = m.clone();
        }
        *visibility = Visibility::Inherited;
    }
    for (icon, mut mesh, mut material, mut transform, mut visibility) in &mut icons {
        let shown = frame.icons.get(icon.0).filter(|_| frame.open);
        let Some((i, item)) = shown.and_then(|i| model_key(&i.stack).and_then(|k| items.0.get(&k)).map(|item| (i, item))) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *transform = Transform::from_matrix(Mat4::from_translation(Vec3::Z * i.z) * gui_matrix(i.x, i.y, item.gui));
        if mesh.0 != item.mesh {
            mesh.0 = item.mesh.clone();
        }
        let m = if item.gui3d { &item_materials.gui_3d } else { &item_materials.gui_flat };
        if material.0 != *m {
            material.0 = m.clone();
        }
        *visibility = Visibility::Inherited;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_matches_the_durability_colors() {
        // Full health is green, none is red, half is yellow-ish.
        assert_eq!(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0), 0x00FF00);
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), 0xFF0000);
        assert_eq!(hsv_to_rgb(1.0 / 6.0, 1.0, 1.0), 0xFFFF00);
    }

    #[test]
    fn a_gradient_is_one_quad_with_its_colors() {
        let mut q = Quads::default();
        q.gradient(0, 0, 10, 5, 0xFFFF_FFFF, 0x8000_0000);
        assert_eq!(q.pos.len(), 4);
        assert_eq!(q.color[0], [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(q.color[1][3], 128.0 / 255.0);
        assert_eq!(q.pos[2], [10.0, -5.0, 0.0]);
    }
}
