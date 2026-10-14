//! The streamed session's frame grab: right after the render world submits a frame, its image is
//! copied to a mapped buffer and waited on, and the pixels go to the encoder (encode.rs) in the
//! same frame.
//!
//! Bevy's own `Readback` maps the buffer after the submit and delivers the data at the next
//! frame's extract, one to two frames later; at 60 fps that is 17 to 33 ms of delay for nothing.
//! Waiting here costs the render thread the GPU's time for the frame, which a session can spare.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, Extent3d, MapMode, TexelCopyBufferInfo, TexelCopyBufferLayout,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};

use crate::encode::{Frame, FrameSink};

/// The newest browser input the frame being built reflects, and when its update began; set each
/// frame in `First` (link.rs) and carried into the render world.
#[derive(Resource, Clone, Copy, ExtractResource)]
pub struct FrameStamp {
    pub seq: u32,
    pub at: Instant,
}

/// Time spent grabbing, summed for the stats line.
#[derive(Default)]
pub struct GrabStats {
    pub frames: u32,
    pub total: Duration,
    pub worst: Duration,
    /// From the frame's update start to the pixels handed to the encoder.
    pub update_to_grab_total: Duration,
}

/// What to grab and where it goes.
#[derive(Resource, Clone, ExtractResource)]
pub struct GrabTarget {
    pub image: Handle<Image>,
    pub width: u32,
    pub height: u32,
    pub sink: FrameSink,
    pub stats: Arc<Mutex<GrabStats>>,
}

pub struct GrabPlugin;

impl Plugin for GrabPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((ExtractResourcePlugin::<GrabTarget>::default(), ExtractResourcePlugin::<FrameStamp>::default()));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(Render, grab.in_set(RenderSystems::Cleanup));
        }
    }
}

/// Rows of `width * 4` bytes padded to the 256-byte copy alignment.
pub fn padded_row(width: u32) -> u32 {
    (width * 4).div_ceil(256) * 256
}

fn grab(
    target: Option<Res<GrabTarget>>,
    stamp: Option<Res<FrameStamp>>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut buffer: Local<Option<(Buffer, u32, u32)>>,
) {
    let (Some(t), Some(stamp)) = (target, stamp) else { return };
    let Some(gpu) = images.get(&t.image) else { return };
    // A picture change (link.rs `change_picture`) resizes the image; until its texture has the new
    // size, there is no frame of that size to grab.
    if (gpu.texture.width(), gpu.texture.height()) != (t.width, t.height) {
        return;
    }
    let start = Instant::now();
    let row = padded_row(t.width);
    if buffer.as_ref().is_none_or(|b| (b.1, b.2) != (t.width, t.height)) {
        let b = device.create_buffer(&BufferDescriptor {
            label: Some("stream-grab"),
            size: (row * t.height) as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        *buffer = Some((b, t.width, t.height));
    }
    let buffer = &buffer.as_ref().unwrap().0;
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: Some("stream-grab") });
    enc.copy_texture_to_buffer(
        gpu.texture.as_image_copy(),
        TexelCopyBufferInfo { buffer, layout: TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
        Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(bevy::render::render_resource::PollType::Wait { submission_index: None, timeout: None });
    if !matches!(rx.recv(), Ok(Ok(()))) {
        return;
    }
    let tight = (t.width * 4) as usize;
    let mut data = Vec::with_capacity(tight * t.height as usize);
    {
        let mapped = slice.get_mapped_range();
        for y in 0..t.height as usize {
            let from = y * row as usize;
            data.extend_from_slice(&mapped[from..from + tight]);
        }
    }
    buffer.unmap();
    t.sink.submit(Frame { data, seq: stamp.seq, at: stamp.at });
    let took = start.elapsed();
    if let Ok(mut s) = t.stats.lock() {
        s.update_to_grab_total += stamp.at.elapsed();
        s.frames += 1;
        s.total += took;
        s.worst = s.worst.max(took);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_pad_to_256_bytes() {
        assert_eq!(padded_row(1280), 5120);
        assert_eq!(padded_row(1000), 4096);
        assert_eq!(padded_row(64), 256);
    }
}
