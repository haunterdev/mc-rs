//! `--shot`: the smoke run's screenshot, saved as PNG, so a rendering change can be looked at
//! without opening a window.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::ScreenshotCaptured;

/// Writes the captured frame to `path` as an 8-bit RGB PNG, then ends the run.
pub fn save_png(path: PathBuf) -> impl FnMut(On<ScreenshotCaptured>, MessageWriter<AppExit>) {
    move |captured, mut exit| {
        let image = &captured.image;
        let (w, h) = (image.width(), image.height());
        let bgra = matches!(image.texture_descriptor.format, TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb);
        let rgb: Vec<u8> = image
            .data
            .as_deref()
            .unwrap_or_default()
            .chunks_exact(4)
            .flat_map(|p| if bgra { [p[2], p[1], p[0]] } else { [p[0], p[1], p[2]] })
            .collect();
        let result = std::fs::File::create(&path).map_err(|e| e.to_string()).and_then(|file| {
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&rgb).map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => println!("SHOT {} {w}x{h} {:?}", path.display(), image.texture_descriptor.format),
            Err(e) => eprintln!("could not save {}: {e}", path.display()),
        }
        exit.write(AppExit::Success);
    }
}
