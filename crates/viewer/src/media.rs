//! Media in the world, in the manner of the Immersive Paintings mod: a picture
//! or video dropped onto the window, or uploaded from the browser page (`S_HANG`), becomes a
//! painting on the block face the crosshair is on, a whole number of blocks wide and high and
//! snapped to the block grid like `EntityPainting`. The image stretches over the painting; GIFs
//! animate at their own frame delays and videos play looped and silent (media_load.rs). A
//! painting is lit as vanilla lights an entity: the lightmap of the block in front of it.
//!
//! With the crosshair on a painting: `=` and `-` change its width, `]` and `[` its height (1 to
//! 16 blocks), Delete takes it down, and a file dropped on it swaps the image and keeps the size.
//! A painting also comes down when the block it hangs on is broken.
//!
//! Not in vanilla 1.12.2. In a server session every painting is shared: its file goes to the
//! server once (`V_MEDIA`, named by its hash), then the painting (`V_PAINTING`); the server keeps
//! both and sends them to every player, now and on join. The server takes a painting down when
//! its block is broken, so a session only hides one whose block it sees as air. The local game
//! keeps paintings for the session, as it keeps its block edits.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::FileDragAndDrop;
use crossbeam_channel::{Receiver, Sender};
use proto::Painting;
use worldgen::populate::Facing;

use crate::chat::Chat;
use crate::env::Env;
use crate::input::Focus;
use crate::link::Link;
use crate::material::EntityMaterial;
use crate::media_load::{self, Content, Video};
use crate::player::{look, Player};
use crate::settings::Menu;
use crate::sky::neighbor_light;
use crate::stream::Streamer;
use crate::target::{mouse_over, BlockPos, Kind, REACH};
use crate::textures::{decode_png, flat_image, Argb};

/// The longer side of a new painting, in blocks.
const SIZE: i32 = 2;
/// The largest side of a painting, in blocks.
const MAX_BLOCKS: i32 = 16;
/// How far the painting floats off the face, against z-fighting.
const LIFT: f32 = 0.01;
/// The largest side a dropped image may have, in pixels.
pub const MAX_SIDE: u32 = 4096;
/// How long a picture uploaded from the page waits for the crosshair to be on a block.
const HANG_WAIT: Duration = Duration::from_secs(30);

/// A painting on a block face.
#[derive(Component)]
pub struct Picture {
    /// The painting's name on the server and in every session.
    uid: u64,
    /// The file it shows, as [`proto::media_id`] names it.
    media: String,
    pos: BlockPos,
    face: Facing,
    /// The painting's up direction, fixed when it was hung.
    up: Vec3,
    /// Size in blocks.
    w: i32,
    h: i32,
    material: Handle<EntityMaterial>,
    show: Show,
}

/// What a painting shows: still or GIF frames, or a playing video.
enum Show {
    Frames {
        frames: Vec<Handle<Image>>,
        /// Seconds each frame shows.
        delays: Vec<f32>,
        frame: usize,
        shown_for: f32,
    },
    Video(Video, Handle<Image>),
}

/// What a finished load is for.
enum Purpose {
    /// A new painting on this face, sized to the file.
    New { pos: BlockPos, face: Facing, up: Vec3 },
    /// The file of the painting `uid` changes; its size stays.
    Swap(u64),
    /// A painting another session shared.
    Shared(Painting),
}

type Loaded = (Purpose, String, io::Result<Content>);

/// The media files this session knows, the loads in flight, and pictures uploaded from the page
/// waiting to be hung.
#[derive(Resource)]
pub struct Media {
    /// Where each file is on disk: where it was dropped from, or the cache for shared ones.
    files: HashMap<String, PathBuf>,
    /// Files the server already has.
    shared: HashSet<String>,
    loads: (Sender<Loaded>, Receiver<Loaded>),
    hang: Vec<(String, Instant)>,
}

impl Default for Media {
    fn default() -> Media {
        Media { files: HashMap::new(), shared: HashSet::new(), loads: crossbeam_channel::unbounded(), hang: Vec::new() }
    }
}

impl Media {
    /// Decodes on a thread of its own: a large picture or ffmpeg's probe would stall the frame.
    fn load(&self, purpose: Purpose, id: String) {
        let Some(path) = self.files.get(&id).cloned() else {
            warn!("media: {id} is not here");
            return;
        };
        let tx = self.loads.0.clone();
        std::thread::spawn(move || {
            let r = media_load::load(&path);
            let _ = tx.send((purpose, id, r));
        });
    }

    /// Names a file on disk by its content; `None` when it cannot be read.
    fn add_file(&mut self, path: &Path) -> Option<String> {
        let bytes = std::fs::read(path).map_err(|e| warn!("media: {}: {e}", path.display())).ok()?;
        let id = proto::media_id(&bytes, &media_load::ext_of(path));
        self.files.insert(id.clone(), path.to_path_buf());
        Some(id)
    }

    /// Keeps a file another session shared, in the cache under its name.
    fn add_shared(&mut self, id: String, bytes: &[u8]) {
        self.shared.insert(id.clone());
        if self.files.contains_key(&id) {
            return;
        }
        let dir = paths::data_dir().map(|d| d.join("media-cache")).unwrap_or_else(|| std::env::temp_dir().join("mc-rs-media"));
        let path = dir.join(&id);
        if !path.exists() {
            let tmp = dir.join(format!("{id}.{}.tmp", std::process::id()));
            if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&tmp, bytes)).and_then(|_| std::fs::rename(&tmp, &path)) {
                warn!("media: could not keep {id}: {e}");
                return;
            }
        }
        self.files.insert(id, path);
    }

    /// Sends a painting to the server, its file first when the server does not have it. A file
    /// past [`proto::MAX_MEDIA`] stays on this screen.
    fn share(&mut self, link: Option<&Link>, p: &Painting) {
        let Some(link) = link else { return };
        if !p.gone && !self.shared.contains(&p.media) {
            match self.files.get(&p.media).and_then(|f| std::fs::read(f).ok()) {
                Some(b) if b.len() <= proto::MAX_MEDIA => {
                    link.send(proto::V_MEDIA, &proto::encode_media(&p.media, &b));
                    self.shared.insert(p.media.clone());
                }
                _ => {
                    warn!("media: {} is over {} MB or unreadable; it shows on this screen only", p.media, proto::MAX_MEDIA >> 20);
                    return;
                }
            }
        }
        link.send(proto::V_PAINTING, &p.encode());
    }
}

/// Shared painting messages from the server, gathered by link.rs.
#[derive(Resource, Default)]
pub struct Inbox {
    pub media: Vec<(String, Vec<u8>)>,
    pub paintings: Vec<Painting>,
    pub hang: Vec<String>,
}

fn vec_of(f: Facing) -> Vec3 {
    let (x, y, z) = f.offset();
    Vec3::new(x as f32, y as f32, z as f32)
}

fn facing_of(v: Vec3) -> Facing {
    Facing::VALUES.into_iter().find(|&f| vec_of(f).dot(v) > 0.5).unwrap_or(Facing::Up)
}

impl Picture {
    fn painting(&self) -> Painting {
        Painting {
            uid: self.uid,
            pos: [self.pos.0, self.pos.1, self.pos.2],
            face: self.face as u8,
            up: facing_of(self.up) as u8,
            w: self.w as u8,
            h: self.h as u8,
            gone: false,
            media: self.media.clone(),
        }
    }
}

fn new_uid() -> u64 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    h.finish()
}

fn linear() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor { mag_filter: ImageFilterMode::Linear, min_filter: ImageFilterMode::Linear, ..default() })
}

/// A painting's picture: linear filtering, as a photo or video wants; the entity shader reads
/// the texels as the gamma values vanilla's textures hold.
fn picture_image(img: &Argb) -> Image {
    let mut image = flat_image(img, false);
    image.sampler = linear();
    image
}

/// The images for what a load produced, and the first one to show.
fn show_for(content: Content, images: &mut Assets<Image>) -> (Show, Handle<Image>) {
    match content {
        Content::Frames(frames) => {
            let delays = frames.iter().map(|f| f.1).collect();
            let frames: Vec<_> = frames.iter().map(|(img, _)| images.add(picture_image(img))).collect();
            let first = frames[0].clone();
            (Show::Frames { frames, delays, frame: 0, shown_for: 0.0 }, first)
        }
        Content::Video(v) => {
            // Black until the first frame; kept in the main world too, so each frame can be
            // written into it.
            let mut image = Image::new(
                Extent3d { width: v.width, height: v.height, depth_or_array_layers: 1 },
                TextureDimension::D2,
                [0, 0, 0, 255].repeat((v.width * v.height) as usize),
                TextureFormat::Rgba8Unorm,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            );
            image.sampler = linear();
            let h = images.add(image);
            (Show::Video(v, h.clone()), h)
        }
    }
}

/// The frames of a PNG or GIF and their delays in seconds; a still image is one frame.
pub fn decode(path: &Path) -> io::Result<Vec<(Argb, f32)>> {
    let bytes = std::fs::read(path)?;
    let ext = media_load::ext_of(path);
    let frames = match ext.as_str() {
        "png" => vec![(decode_png(&bytes)?, 0.0)],
        "gif" => gif_frames(&bytes)?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("{ext:?} files are not supported; use a PNG, GIF, JPEG, WebP or BMP picture or an MP4, WebM, MKV or MOV video"),
            ))
        }
    };
    let first = &frames[0].0;
    if first.width == 0 || first.height == 0 || first.width > MAX_SIDE || first.height > MAX_SIDE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("{}x{} is outside 1 to {MAX_SIDE} pixels a side", first.width, first.height)));
    }
    Ok(frames)
}

/// Every GIF frame composited onto the full canvas, honoring each frame's disposal.
pub fn gif_frames(bytes: &[u8]) -> io::Result<Vec<(Argb, f32)>> {
    let bad = |e: gif::DecodingError| io::Error::new(io::ErrorKind::InvalidData, format!("gif: {e}"));
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts.read_info(Cursor::new(bytes)).map_err(bad)?;
    let (w, h) = (dec.width() as u32, dec.height() as u32);
    let mut canvas = vec![0u32; (w * h) as usize];
    let mut out = Vec::new();
    while let Some(f) = dec.read_next_frame().map_err(bad)? {
        let before = (f.dispose == gif::DisposalMethod::Previous).then(|| canvas.clone());
        let rect = |x: u32, y: u32| (f.left as u32 + x, f.top as u32 + y);
        for y in 0..f.height as u32 {
            for x in 0..f.width as u32 {
                let (cx, cy) = rect(x, y);
                let i = ((y * f.width as u32 + x) * 4) as usize;
                let p = &f.buffer[i..i + 4];
                if cx < w && cy < h && p[3] != 0 {
                    canvas[(cy * w + cx) as usize] = (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
                }
            }
        }
        // Browsers show a delay of 0 or 1 hundredths as 10.
        let delay = if f.delay <= 1 { 0.1 } else { f.delay as f32 / 100.0 };
        out.push((Argb { width: w, height: h, pixels: canvas.clone() }, delay));
        match f.dispose {
            gif::DisposalMethod::Background => {
                for y in 0..f.height as u32 {
                    for x in 0..f.width as u32 {
                        let (cx, cy) = rect(x, y);
                        if cx < w && cy < h {
                            canvas[(cy * w + cx) as usize] = 0;
                        }
                    }
                }
            }
            gif::DisposalMethod::Previous => canvas = before.unwrap_or(canvas),
            _ => {}
        }
    }
    if out.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "gif: no frames"));
    }
    Ok(out)
}

/// The size in blocks of a new painting for a `w` by `h` pixel image: the longer side `SIZE`
/// blocks, the shorter side the nearest whole number that keeps the aspect, at least 1.
pub fn fit(w: u32, h: u32) -> (i32, i32) {
    let short = |a: u32, b: u32| ((SIZE as f32 * a as f32 / b as f32).round() as i32).max(1);
    if w >= h {
        (SIZE, short(h, w))
    } else {
        (short(w, h), SIZE)
    }
}

/// The painting's up direction on `face`. On a wall its top points up; on a floor or ceiling it
/// takes the block axis nearest to `forward`, the way the player looks, so it reads upright from
/// where they stand.
pub fn up_for(face: Facing, forward: Vec3) -> Vec3 {
    let dy = face.offset().1;
    if dy == 0 {
        return Vec3::Y;
    }
    let up = if forward.x.abs() > forward.z.abs() { Vec3::X * forward.x.signum() } else { Vec3::Z * if forward.z == 0.0 { 1.0 } else { forward.z.signum() } };
    if dy > 0 {
        up
    } else {
        -up
    }
}

/// Where a `w` by `h` block painting hangs on `face` of the block at `pos`. As `EntityHanging`
/// places a painting (`updateFacingWithBoundingBox`, `offs`): the clicked cell is the painting's
/// center cell, and an even side shifts half a block to the right or up so the painting covers
/// whole cells.
pub fn placement(pos: BlockPos, face: Facing, up: Vec3, w: i32, h: i32) -> Transform {
    let normal = vec_of(face);
    // Seen from the front (looking along -normal), right is up x normal.
    let right = up.cross(normal);
    let half = |n: i32| if n % 2 == 0 { 0.5 } else { 0.0 };
    let center = Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5) + normal * (0.5 + LIFT) + right * half(w) + up * half(h);
    // A `Rectangle` faces +Z; turning -Z toward -normal turns its face out of the block.
    Transform::from_translation(center).looking_to(-normal, up).with_scale(Vec3::new(w as f32, h as f32, 1.0))
}

/// How far along the ray from `origin` in direction `dir` (unit length) it meets the painting
/// with this transform, if it does from the front.
pub fn ray_hits(t: &Transform, origin: Vec3, dir: Vec3) -> Option<f32> {
    let inv = t.compute_affine().inverse();
    let (o, d) = (inv.transform_point3(origin), inv.transform_vector3(dir));
    if o.z <= 0.0 || d.z >= 0.0 {
        return None;
    }
    let s = -o.z / d.z;
    let p = o + d * s;
    (p.x.abs() <= 0.5 && p.y.abs() <= 0.5).then_some(s)
}

/// The painting under the crosshair, unless a block is nearer.
fn pointed<'a>(streamer: &Streamer, player: &Player, pictures: impl Iterator<Item = (Entity, &'a Transform)>) -> Option<Entity> {
    let eye = player.pos + DVec3::Y * player.eye_height() as f64;
    let dir = look(player.pitch, player.yaw).normalize();
    let block = mouse_over(streamer, player, 1.0).filter(|h| h.kind == Kind::Block).map(|h| (h.vec - eye).length() as f32).unwrap_or(f32::MAX);
    pictures
        .filter_map(|(e, t)| ray_hits(t, eye.as_vec3(), dir).map(|s| (e, s)))
        .filter(|&(_, s)| s <= REACH as f32 && s <= block + 0.05)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e)
}

/// Where a file goes: onto the painting under the crosshair, or as a new painting on the face
/// under it; `None` when the crosshair is on neither.
fn aim(streamer: &Streamer, player: &Player, pictures: &Query<(Entity, &mut Picture, &mut Transform)>) -> Option<Purpose> {
    if let Some(e) = pointed(streamer, player, pictures.iter().map(|(e, _, t)| (e, t))) {
        return Some(Purpose::Swap(pictures.get(e).unwrap().1.uid));
    }
    let hit = mouse_over(streamer, player, 1.0).filter(|h| h.kind == Kind::Block)?;
    Some(Purpose::New { pos: hit.pos, face: hit.face, up: up_for(hit.face, look(0.0, player.yaw)) })
}

/// Starts loading dropped files, wherever the crosshair puts them.
pub fn drop_files(
    mut dropped: MessageReader<FileDragAndDrop>,
    mut media: ResMut<Media>,
    streamer: Res<Streamer>,
    player: Res<Player>,
    pictures: Query<(Entity, &mut Picture, &mut Transform)>,
) {
    for event in dropped.read() {
        let FileDragAndDrop::DroppedFile { path_buf, .. } = event else { continue };
        let Some(id) = media.add_file(path_buf) else { continue };
        match aim(&streamer, &player, &pictures) {
            Some(purpose) => {
                info!("media: loading {}", path_buf.display());
                media.load(purpose, id);
            }
            None => info!("media: point at a block face to hang {}", path_buf.display()),
        }
    }
}

/// The server's painting messages, and pictures uploaded from the page, hung once the crosshair
/// is on a block.
pub fn receive_shared(
    mut commands: Commands,
    mut inbox: ResMut<Inbox>,
    mut media: ResMut<Media>,
    streamer: Res<Streamer>,
    player: Res<Player>,
    mut pictures: Query<(Entity, &mut Picture, &mut Transform)>,
) {
    for (id, bytes) in std::mem::take(&mut inbox.media) {
        media.add_shared(id, &bytes);
    }
    for p in std::mem::take(&mut inbox.paintings) {
        let found = pictures.iter_mut().find(|(_, q, _)| q.uid == p.uid);
        match found {
            Some((e, _, _)) if p.gone => commands.entity(e).despawn(),
            Some((_, mut q, mut t)) if q.media == p.media => {
                (q.w, q.h) = (p.w as i32, p.h as i32);
                *t = placement(q.pos, q.face, q.up, q.w, q.h);
            }
            _ if p.gone => {}
            _ => {
                let id = p.media.clone();
                media.load(Purpose::Shared(p), id);
            }
        }
    }
    let now = Instant::now();
    for id in std::mem::take(&mut inbox.hang) {
        media.hang.push((id, now + HANG_WAIT));
    }
    if media.hang.is_empty() {
        return;
    }
    for (id, until) in std::mem::take(&mut media.hang) {
        match aim(&streamer, &player, &pictures) {
            Some(purpose) => media.load(purpose, id),
            None if now < until => media.hang.push((id, until)),
            None => warn!("media: {id} was uploaded but the crosshair never came to a block"),
        }
    }
}

/// Spawns, swaps or replaces paintings as their files finish loading, and shares the ones this
/// session made.
#[allow(clippy::too_many_arguments)]
pub fn finish_loads(
    mut commands: Commands,
    mut media: ResMut<Media>,
    link: Option<Res<Link>>,
    mut chat: Option<ResMut<Chat>>,
    env: Res<Env>,
    mut pictures: Query<(Entity, &mut Picture, &mut Transform)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
) {
    while let Ok((purpose, id, result)) = media.loads.1.try_recv() {
        let content = match result {
            Ok(c) => c,
            Err(e) => {
                warn!("media: {id}: {e}");
                if let Some(chat) = chat.as_mut() {
                    chat.push(format!("Could not show that file: {e}"), Color::srgb(1.0, 0.33, 0.33));
                }
                continue;
            }
        };
        let (pw, ph) = content.size();
        let (show, first) = show_for(content, &mut images);
        let (pos, face, up, w, h, uid, share) = match purpose {
            Purpose::Swap(uid) => {
                let Some((_, mut p, _)) = pictures.iter_mut().find(|(_, p, _)| p.uid == uid) else { continue };
                if let Some(mut m) = materials.get_mut(&p.material) {
                    m.skin = first;
                }
                p.show = show;
                p.media = id;
                info!("media: {} on the painting at {:?}", p.media, p.pos);
                let painting = p.painting();
                media.share(link.as_deref(), &painting);
                continue;
            }
            Purpose::New { pos, face, up } => {
                let (w, h) = fit(pw, ph);
                (pos, face, up, w, h, new_uid(), true)
            }
            Purpose::Shared(p) => {
                if let Some((e, _, _)) = pictures.iter().find(|(_, q, _)| q.uid == p.uid) {
                    commands.entity(e).despawn();
                }
                let face = Facing::VALUES[p.face as usize % 6];
                ((p.pos[0], p.pos[1], p.pos[2]), face, vec_of(Facing::VALUES[p.up as usize % 6]), p.w as i32, p.h as i32, p.uid, false)
            }
        };
        let material = materials.add(EntityMaterial::new(env.image.clone(), first));
        info!("media: hung {id} ({pw}x{ph} px, {w}x{h} blocks) at {pos:?} {face:?}");
        let picture = Picture { uid, media: id, pos, face, up, w, h, material: material.clone(), show };
        if share {
            let painting = picture.painting();
            media.share(link.as_deref(), &painting);
        }
        commands.spawn((picture, Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))), MeshMaterial3d(material), NotShadowCaster, placement(pos, face, up, w, h)));
    }
}

/// The keys on the painting under the crosshair: `=` and `-` width, `]` and `[` height, Delete
/// takes it down.
#[allow(clippy::too_many_arguments)]
pub fn edit_keys(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<Focus>,
    menu: Res<Menu>,
    chat: Option<Res<Chat>>,
    streamer: Res<Streamer>,
    player: Res<Player>,
    mut media: ResMut<Media>,
    link: Option<Res<Link>>,
    mut pictures: Query<(Entity, &mut Picture, &mut Transform)>,
) {
    if !focus.in_game || menu.open || chat.is_some_and(|c| c.open) {
        return;
    }
    let (dw, dh) = (
        keys.just_pressed(KeyCode::Equal) as i32 - keys.just_pressed(KeyCode::Minus) as i32,
        keys.just_pressed(KeyCode::BracketRight) as i32 - keys.just_pressed(KeyCode::BracketLeft) as i32,
    );
    let delete = keys.just_pressed(KeyCode::Delete);
    if dw == 0 && dh == 0 && !delete {
        return;
    }
    let Some(e) = pointed(&streamer, &player, pictures.iter().map(|(e, _, t)| (e, t))) else { return };
    let (_, mut p, mut t) = pictures.get_mut(e).unwrap();
    if delete {
        media.share(link.as_deref(), &Painting { gone: true, ..p.painting() });
        commands.entity(e).despawn();
        return;
    }
    p.w = (p.w + dw).clamp(1, MAX_BLOCKS);
    p.h = (p.h + dh).clamp(1, MAX_BLOCKS);
    *t = placement(p.pos, p.face, p.up, p.w, p.h);
    let painting = p.painting();
    media.share(link.as_deref(), &painting);
}

/// Lights each painting, steps GIFs, shows each video's newest frame, and hides a painting whose
/// block is air (the local game takes it down; in a server session the server does).
pub fn update_pictures(
    mut commands: Commands,
    time: Res<Time>,
    streamer: Res<Streamer>,
    link: Option<Res<Link>>,
    mut pictures: Query<(Entity, &mut Picture, &mut Visibility)>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
) {
    for (e, mut p, mut vis) in &mut pictures {
        let air = streamer.state_at(p.pos.0, p.pos.1, p.pos.2).is_some_and(|s| s >> 4 == 0);
        if air && link.is_none() {
            commands.entity(e).despawn();
            continue;
        }
        vis.set_if_neq(if air { Visibility::Hidden } else { Visibility::Inherited });
        // RenderPainting.setLightmap: the light where the painting hangs, in front of its block.
        // DROPPED: vanilla samples it per 16-pixel segment; one sample for the whole painting.
        let (dx, dy, dz) = p.face.offset();
        let (sky, block) = neighbor_light(&streamer, p.pos.0 + dx, p.pos.1 + dy, p.pos.2 + dz);
        let light = Vec4::new(block as f32 * 16.0, sky as f32 * 16.0, 0.0, 0.0);
        let mut skin = None;
        match &mut p.show {
            Show::Frames { frames, delays, frame, shown_for } if frames.len() > 1 => {
                *shown_for += time.delta_secs();
                let before = *frame;
                while *shown_for >= delays[*frame] {
                    *shown_for -= delays[*frame];
                    *frame = (*frame + 1) % frames.len();
                }
                if *frame != before {
                    skin = Some(frames[*frame].clone());
                }
            }
            Show::Video(v, image) => {
                if let Some(bytes) = v.latest.lock().unwrap().take() {
                    if let Some(mut img) = images.get_mut(&*image) {
                        img.data = Some(bytes);
                    }
                    // A new texture behind the same handle: touching the material rebinds it.
                    skin = Some(image.clone());
                }
            }
            _ => {}
        }
        if skin.is_some() || materials.get(&p.material).is_some_and(|m| m.light != light) {
            if let Some(mut m) = materials.get_mut(&p.material) {
                m.light = light;
                if let Some(s) = skin {
                    m.skin = s;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn hangs_out_of_the_face_upright() {
        assert_eq!(fit(200, 100), (2, 1));
        assert_eq!(fit(100, 300), (1, 2));
        assert_eq!(fit(64, 64), (2, 2));
        assert_eq!(fit(1000, 10), (2, 1));
        // East face of (10, 64, 0), 2 wide by 1 high: the front looks east, the top up, and the
        // even width shifts half a block to the viewer's right (north, -Z, looking west).
        let up = up_for(Facing::East, Vec3::Z);
        let t = placement((10, 64, 0), Facing::East, up, 2, 1);
        assert!(close(t.translation, Vec3::new(11.0 + LIFT, 64.5, 0.0)));
        assert!(close(t.rotation * Vec3::Z, Vec3::X));
        assert!(close(t.rotation * Vec3::Y, Vec3::Y));
        assert!(close(t.rotation * Vec3::X, -Vec3::Z));
        assert!(close(t.scale, Vec3::new(2.0, 1.0, 1.0)));
        // On a floor, looking mostly north (-Z), the top points north; 3x3 stays centered.
        let up = up_for(Facing::Up, Vec3::new(0.3, -0.5, -0.9));
        let t = placement((0, 63, 0), Facing::Up, up, 3, 3);
        assert!(close(t.rotation * Vec3::Z, Vec3::Y));
        assert!(close(t.rotation * Vec3::Y, -Vec3::Z));
        assert!(close(t.translation, Vec3::new(0.5, 64.0 + LIFT, 0.5)));
    }

    #[test]
    fn up_directions_round_trip_through_facings() {
        for f in Facing::VALUES {
            assert_eq!(facing_of(vec_of(f)), f);
        }
        let up = up_for(Facing::Up, Vec3::new(0.9, -0.5, 0.1));
        assert_eq!(facing_of(up), Facing::East);
    }

    #[test]
    fn rays_pick_paintings_from_the_front() {
        let t = placement((10, 64, 0), Facing::East, Vec3::Y, 2, 1);
        // From 3 blocks east, looking west at the painting's center.
        let s = ray_hits(&t, Vec3::new(14.0, 64.5, 0.0), -Vec3::X).unwrap();
        assert!((s - (3.0 - LIFT)).abs() < 1e-4);
        // Inside the 2-block width, outside it, and from behind.
        assert!(ray_hits(&t, Vec3::new(14.0, 64.5, -0.9), -Vec3::X).is_some());
        assert!(ray_hits(&t, Vec3::new(14.0, 64.5, 1.1), -Vec3::X).is_none());
        assert!(ray_hits(&t, Vec3::new(9.0, 64.5, 0.0), Vec3::X).is_none());
    }

    #[test]
    fn gif_frames_composite_and_dispose() {
        // A 2x1 GIF: frame 1 paints both pixels red, frame 2 paints only the right one blue over
        // it and is cleared to the background afterwards, frame 3 paints nothing new.
        let mut bytes = Vec::new();
        {
            let palette = [255, 0, 0, 0, 0, 255];
            let mut enc = gif::Encoder::new(&mut bytes, 2, 1, &palette).unwrap();
            let mut f = gif::Frame { width: 2, height: 1, buffer: vec![0, 0].into(), delay: 5, ..Default::default() };
            enc.write_frame(&f).unwrap();
            f = gif::Frame { left: 1, width: 1, height: 1, buffer: vec![1].into(), delay: 0, dispose: gif::DisposalMethod::Background, ..Default::default() };
            enc.write_frame(&f).unwrap();
            f = gif::Frame { width: 1, height: 1, buffer: vec![0].into(), delay: 20, ..Default::default() };
            enc.write_frame(&f).unwrap();
        }
        let frames = gif_frames(&bytes).unwrap();
        let px = |i: usize| frames[i].0.pixels.clone();
        assert_eq!(frames.len(), 3);
        assert_eq!(px(0), vec![0xffff_0000, 0xffff_0000]);
        assert_eq!(px(1), vec![0xffff_0000, 0xff00_00ff]);
        assert_eq!(px(2), vec![0xffff_0000, 0]);
        assert_eq!(frames.iter().map(|f| f.1).collect::<Vec<_>>(), vec![0.05, 0.1, 0.2]);
    }
}
