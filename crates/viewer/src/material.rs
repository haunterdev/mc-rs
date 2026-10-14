//! The chunk, sky, cloud, hand, item and distant-terrain materials, and the plugin that embeds
//! their shaders.
//!
//! Every material binds the environment texture (env.rs), so time of day, fog and the lightmap
//! change without touching a material asset: a material change re-specializes every mesh that
//! uses it.

use bevy::asset::embedded_asset;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, ColorWrites, CompareFunction, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use crate::env::EnvPlugin;
use crate::lod::LodPlugin;
use crate::vertex::{ATTRIBUTE_PACKED, ATTRIBUTE_PACKED_NORMAL};

const CHUNK_SHADER: &str = "embedded://viewer/shaders/chunk.wgsl";
const CHUNK_VERTEX_SHADER: &str = "embedded://viewer/shaders/chunk_vertex.wgsl";
const LOD_SHADER: &str = "embedded://viewer/shaders/lod.wgsl";
const LOD_VERTEX_SHADER: &str = "embedded://viewer/shaders/lod_vertex.wgsl";

/// The distant-terrain vertex's color and chunk (lod_mesh.rs `LodMesh::packed`).
pub const ATTRIBUTE_LOD: bevy::mesh::MeshVertexAttribute =
    bevy::mesh::MeshVertexAttribute::new("Vertex_LodPacked", 0x6d63_7273_0003, bevy::mesh::VertexFormat::Uint32x2);
const CHUNK_PREPASS_SHADER: &str = "embedded://viewer/shaders/chunk_prepass.wgsl";
const SKY_SHADER: &str = "embedded://viewer/shaders/sky.wgsl";
const HAND_SHADER: &str = "embedded://viewer/shaders/hand.wgsl";
const ITEM_SHADER: &str = "embedded://viewer/shaders/item.wgsl";
const ENTITY_SHADER: &str = "embedded://viewer/shaders/entity.wgsl";
const NAMEPLATE_SHADER: &str = "embedded://viewer/shaders/nameplate.wgsl";

/// The texture animation table, `BlockTextures::anim_table`.
#[derive(ShaderType, Clone, Debug)]
pub struct ChunkParams {
    pub anim: [UVec4; 256],
}

impl ChunkParams {
    pub fn new(table: &[[u32; 4]; 256]) -> ChunkParams {
        ChunkParams { anim: std::array::from_fn(|i| UVec4::from_array(table[i])) }
    }
}

/// One of the three terrain layers: opaque (`SOLID`), alpha tested at 0.5 (`CUTOUT_MIPPED`,
/// `CUTOUT`) or blended with an alpha test at 0.1 (`TRANSLUCENT`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(ChunkKey)]
pub struct ChunkMaterial {
    #[uniform(0)]
    pub params: ChunkParams,
    #[texture(1, dimension = "2d_array")]
    #[sampler(2)]
    pub blocks: Handle<Image>,
    #[texture(3, sample_type = "float", filterable = false)]
    pub env: Handle<Image>,
    pub alpha_mode: AlphaMode,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct ChunkKey {
    translucent: bool,
}

impl From<&ChunkMaterial> for ChunkKey {
    fn from(m: &ChunkMaterial) -> Self {
        ChunkKey { translucent: matches!(m.alpha_mode, AlphaMode::Blend) }
    }
}

impl Material for ChunkMaterial {
    fn fragment_shader() -> ShaderRef {
        CHUNK_SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        CHUNK_PREPASS_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn vertex_shader() -> ShaderRef {
        CHUNK_VERTEX_SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        CHUNK_VERTEX_SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The packed vertex (vertex.rs) has none of the standard attributes, so the pipeline
        // found no inputs and declared none of the varyings the fragment shaders read.
        descriptor.vertex.buffers =
            vec![layout.0.get_layout(&[ATTRIBUTE_PACKED.at_shader_location(0), ATTRIBUTE_PACKED_NORMAL.at_shader_location(1)])?];
        let defs = ["VERTEX_UVS", "VERTEX_UVS_A", "VERTEX_UVS_B", "VERTEX_COLORS"];
        descriptor.vertex.shader_defs.extend(defs.map(Into::into));
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.extend(defs.map(Into::into));
            if key.bind_group_data.translucent {
                fragment.shader_defs.push("ALPHA_TEST_TENTH".into());
            }
        }
        Ok(())
    }
}

/// What a `SkyMaterial` draws.
#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug)]
pub enum SkyKind {
    /// The sky mesh, drawn by the sky camera in submission order over the clear color.
    Sky,
    /// The clouds' color pass: only fragments at the depth the depth pass left.
    CloudColor,
    /// The clouds' depth pass (`colorMask(false)`), so only the nearest cloud face is colored.
    CloudDepth,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(SkyKey)]
pub struct SkyMaterial {
    #[texture(0, sample_type = "float", filterable = false)]
    pub env: Handle<Image>,
    #[texture(1)]
    #[sampler(2)]
    pub sun: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub moon: Handle<Image>,
    pub kind: SkyKind,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct SkyKey {
    kind: SkyKind,
}

impl From<&SkyMaterial> for SkyKey {
    fn from(m: &SkyMaterial) -> Self {
        SkyKey { kind: m.kind }
    }
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        match self.kind {
            // The shader writes premultiplied color, which covers opaque, alpha and additive.
            SkyKind::Sky => AlphaMode::Premultiplied,
            SkyKind::CloudColor | SkyKind::CloudDepth => AlphaMode::Blend,
        }
    }

    /// Clouds draw after every chunk, water included (`RenderGlobal.renderClouds` follows the
    /// translucent layer), and the depth pass before the color pass.
    fn depth_bias(&self) -> f32 {
        match self.kind {
            SkyKind::Sky => 0.0,
            SkyKind::CloudDepth => 1.0e6,
            SkyKind::CloudColor => 2.0e6,
        }
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // GlStateManager.disableCull in renderSky and renderCloudsFancy.
        descriptor.primitive.cull_mode = None;
        let kind = key.bind_group_data.kind;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            match kind {
                // The sky is drawn with depth writes off and nothing in front of it yet.
                SkyKind::Sky => {
                    ds.depth_write_enabled = Some(false);
                    ds.depth_compare = Some(CompareFunction::Always);
                }
                SkyKind::CloudDepth => ds.depth_write_enabled = Some(true),
                SkyKind::CloudColor => {}
            }
        }
        if let Some(fragment) = descriptor.fragment.as_mut() {
            if kind != SkyKind::Sky {
                fragment.shader_defs.push("CLOUDS".into());
            }
            if kind == SkyKind::CloudDepth {
                for target in fragment.targets.iter_mut().flatten() {
                    target.write_mask = ColorWrites::empty();
                }
            }
        }
        Ok(())
    }
}

/// The first-person arm, drawn by the hand camera after a depth clear (`renderHand`'s
/// `clear(256)`), blended with depth writes on as `RenderPlayer.renderRightArm` leaves them.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct HandMaterial {
    #[texture(0, sample_type = "float", filterable = false)]
    pub env: Handle<Image>,
    #[texture(1)]
    #[sampler(2)]
    pub skin: Handle<Image>,
}

impl Material for HandMaterial {
    fn fragment_shader() -> ShaderRef {
        HAND_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // ItemRenderer.renderArmFirstPerson: GlStateManager.disableCull.
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(true);
        }
        Ok(())
    }
}

/// What an `ItemMaterial` lights with.
#[derive(ShaderType, Clone, Debug)]
pub struct ItemParams {
    pub w0: Vec4,
    pub w1: Vec4,
    /// x: lighting on (`isGui3d` in the GUI, always in the hand); y: in the hand; z: dimmed under
    /// the options menu.
    pub flags: UVec4,
}

/// A block item in the GUI or the hand (item_render.rs): the block texture array at its top
/// level, blended with depth writes on as `RenderItem.renderItemModel` leaves them.
/// DROPPED: the enchantment glint (`renderEffect`) and the front-face cull for a negative scale.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct ItemMaterial {
    #[uniform(0)]
    pub params: ChunkParams,
    #[texture(1, dimension = "2d_array")]
    #[sampler(2)]
    pub blocks: Handle<Image>,
    #[texture(3, sample_type = "float", filterable = false)]
    pub env: Handle<Image>,
    #[uniform(4)]
    pub item: ItemParams,
}

impl Material for ItemMaterial {
    fn fragment_shader() -> ShaderRef {
        ITEM_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(true);
        }
        Ok(())
    }
}

/// Distant terrain (lod.rs): vertex colors, the environment texture and the shown-chunk mask.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct LodMaterial {
    #[texture(3, sample_type = "float", filterable = false)]
    pub env: Handle<Image>,
    /// Read by the vertex shader, which drops the quads over shown chunks.
    #[texture(4, sample_type = "float", filterable = false, visibility(vertex, fragment))]
    pub mask: Handle<Image>,
}

impl Material for LodMaterial {
    fn vertex_shader() -> ShaderRef {
        LOD_VERTEX_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        LOD_SHADER.into()
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0), ATTRIBUTE_LOD.at_shader_location(1)])?];
        descriptor.vertex.shader_defs.push("VERTEX_COLORS".into());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.push("VERTEX_COLORS".into());
        }
        Ok(())
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// Other players (players.rs): the skin under vanilla's entity lighting and the lightmap at the
/// entity's eyes, fogged.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct EntityMaterial {
    #[texture(0, sample_type = "float", filterable = false)]
    pub env: Handle<Image>,
    #[texture(1)]
    #[sampler(2)]
    pub skin: Handle<Image>,
    /// Lightmap coordinates (block, sky) in 0..240, in x and y.
    #[uniform(3)]
    pub light: Vec4,
}

impl Material for EntityMaterial {
    fn fragment_shader() -> ShaderRef {
        ENTITY_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Mask(0.1)
    }

    fn enable_prepass() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The hat and jacket layers show their inner faces through gaps, as with culling off.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// How a name tag piece draws: `EntityRenderer.drawNameplate` draws the background and a faint
/// copy of the text with the depth test off (seen through walls), then the text with it on.
#[derive(Eq, PartialEq, Hash, Copy, Clone, Debug)]
pub enum PlateKind {
    /// The 25% black quad, through walls.
    Background,
    /// The quad of a sneaking player, depth tested.
    BackgroundDepth,
    /// The text at alpha 0x20, through walls.
    TextThrough,
    /// The text, depth tested.
    Text,
}

#[derive(ShaderType, Clone, Debug)]
pub struct PlateParams {
    pub color: Vec4,
    pub mode: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(PlateKey)]
pub struct NameplateMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub font: Handle<Image>,
    #[uniform(2)]
    pub params: PlateParams,
    pub kind: PlateKind,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct PlateKey {
    kind: PlateKind,
}

impl From<&NameplateMaterial> for PlateKey {
    fn from(m: &NameplateMaterial) -> Self {
        PlateKey { kind: m.kind }
    }
}

impl Material for NameplateMaterial {
    fn fragment_shader() -> ShaderRef {
        NAMEPLATE_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
            if matches!(key.bind_group_data.kind, PlateKind::Background | PlateKind::TextThrough) {
                ds.depth_compare = Some(CompareFunction::Always);
            }
        }
        Ok(())
    }
}

pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "shaders/chunk_common.wgsl");
        embedded_asset!(app, "shaders/chunk.wgsl");
        embedded_asset!(app, "shaders/chunk_vertex.wgsl");
        embedded_asset!(app, "shaders/chunk_prepass.wgsl");
        embedded_asset!(app, "shaders/sky.wgsl");
        embedded_asset!(app, "shaders/hand.wgsl");
        embedded_asset!(app, "shaders/item.wgsl");
        embedded_asset!(app, "shaders/lod.wgsl");
        embedded_asset!(app, "shaders/lod_vertex.wgsl");
        embedded_asset!(app, "shaders/entity.wgsl");
        embedded_asset!(app, "shaders/nameplate.wgsl");
        app.add_plugins((
            EnvPlugin,
            LodPlugin,
            MaterialPlugin::<ChunkMaterial>::default(),
            MaterialPlugin::<SkyMaterial>::default(),
            MaterialPlugin::<HandMaterial>::default(),
            MaterialPlugin::<ItemMaterial>::default(),
            MaterialPlugin::<LodMaterial>::default(),
            MaterialPlugin::<EntityMaterial>::default(),
            MaterialPlugin::<NameplateMaterial>::default(),
        ));
    }
}
