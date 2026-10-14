//! The mc-rs icon and the Windows resources both exes carry.
//!
//! The icon is drawn here in code (an isometric grass-topped block on an 8x8 pixel grid), so the
//! repo holds no image file. `icon_rgba` serves the launcher's window icon at runtime; `embed`,
//! called from a build script, writes the `.ico`, the application manifest and a version-info
//! resource into `OUT_DIR` and links them into the exe. Antivirus heuristics and SmartScreen
//! treat an exe with no version info, no icon and no manifest as more suspect.

use std::path::Path;

/// Straight-alpha RGBA, `size` x `size`, row-major from the top.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let py = (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if let Some([cr, cg, cb]) = sample(px, py) {
                        r += cr;
                        g += cg;
                        b += cb;
                        a += 1.0;
                    }
                }
            }
            if a > 0.0 {
                out.extend([r / a, g / a, b / a].map(|c| c.round().clamp(0.0, 255.0) as u8));
                out.push((a / (SS * SS) as f32 * 255.0).round() as u8);
            } else {
                out.extend([0, 0, 0, 0]);
            }
        }
    }
    out
}

/// The block's color at a point of the unit square, or `None` outside it. Faces are found by
/// solving for the point's coordinates along each rhombus's two edges.
fn sample(px: f32, py: f32) -> Option<[f32; 3]> {
    const TOP: (f32, f32) = (0.5, 0.04);
    const UL: (f32, f32) = (0.07, 0.285);
    const UR: (f32, f32) = (0.93, 0.285);
    const MID: (f32, f32) = (0.5, 0.53);
    const DOWN: (f32, f32) = (0.0, 0.43);
    let face = |o: (f32, f32), e1: (f32, f32), e2: (f32, f32)| {
        let (dx, dy) = (px - o.0, py - o.1);
        let det = e1.0 * e2.1 - e1.1 * e2.0;
        let u = (dx * e2.1 - dy * e2.0) / det;
        let v = (e1.0 * dy - e1.1 * dx) / det;
        ((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v)).then_some((u, v))
    };
    let sub = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0, a.1 - b.1);
    if let Some((u, v)) = face(UL, sub(TOP, UL), sub(MID, UL)) {
        return Some(texel(u, v, Face::Top, 1.0));
    }
    if let Some((u, v)) = face(UL, sub(MID, UL), DOWN) {
        return Some(texel(u, v, Face::Side, 0.82));
    }
    if let Some((u, v)) = face(MID, sub(UR, MID), DOWN) {
        return Some(texel(u, v, Face::Side, 0.62));
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum Face {
    Top,
    Side,
}

/// One cell of the 8x8 grid on a face: grass on top, dirt on the sides under a grass fringe
/// with a few longer drips.
fn texel(u: f32, v: f32, face: Face, shade: f32) -> [f32; 3] {
    const GRASS: [f32; 3] = [96.0, 158.0, 56.0];
    const DIRT: [f32; 3] = [134.0, 96.0, 67.0];
    let (cx, cy) = ((u * 8.0) as u32, (v * 8.0) as u32);
    let h = hash(cx, cy, face as u32);
    let grass = face == Face::Top || cy < 2 || (cy == 2 && h % 3 == 0);
    let base = if grass { GRASS } else { DIRT };
    let jitter = 0.86 + (h % 7) as f32 * 0.045;
    base.map(|c| c * jitter * shade)
}

fn hash(x: u32, y: u32, salt: u32) -> u32 {
    let mut h = x.wrapping_mul(0x9e37_79b1) ^ y.wrapping_mul(0x85eb_ca77) ^ salt.wrapping_mul(0xc2b2_ae3d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// A `.ico` holding every size in `sizes` as 32-bit BMP images.
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| dib(s, &icon_rgba(s))).collect();
    let mut out = Vec::new();
    out.extend(0u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&s, img) in sizes.iter().zip(&images) {
        let dim = if s >= 256 { 0 } else { s as u8 };
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((img.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend(img);
    }
    out
}

/// One icon image: a BITMAPINFOHEADER with double height, BGRA rows bottom-up, then an all-zero
/// AND mask (the alpha channel does the masking).
fn dib(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::new();
    out.extend(40u32.to_le_bytes());
    out.extend((size as i32).to_le_bytes());
    out.extend((size as i32 * 2).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend((size * size * 4 + mask_row * size).to_le_bytes());
    out.extend([0u8; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            out.extend([rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    out
}

/// The application manifest: runs as the invoking user (no elevation prompt), declares Windows
/// 10 and 11, per-monitor DPI awareness, UTF-8 as the code page and the v6 common controls.
#[cfg_attr(not(feature = "build"), allow(dead_code))]
fn manifest(name: &str, description: &str, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="xy.mc-rs.{name}" version="{version}.0" processorArchitecture="amd64"/>
  <description>{description}</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#
    )
}

/// The resource script. Numeric ids stand in for the `winver.h` names so `rc` needs no include
/// path: 24 is RT_MANIFEST, 0x40004 VOS_NT_WINDOWS32, 1 VFT_APP.
#[cfg_attr(not(feature = "build"), allow(dead_code))]
fn rc(ico: &Path, manifest: &Path, description: &str, file_name: &str, version: &str) -> String {
    let esc = |p: &Path| p.display().to_string().replace('\\', "\\\\");
    let mut parts = version.split('.').map(|n| n.parse::<u16>().unwrap_or(0)).collect::<Vec<_>>();
    parts.resize(4, 0);
    let commas = parts.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    format!(
        r#"1 ICON "{ico}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "xy"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{file_name}"
      VALUE "LegalCopyright", "xy. Not an official Minecraft product; not approved by or associated with Mojang or Microsoft."
      VALUE "OriginalFilename", "{file_name}"
      VALUE "ProductName", "mc-rs"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = esc(ico),
        manifest = esc(manifest),
    )
}

#[cfg(feature = "build")]
/// From a build script: links the icon, manifest and version info into the crate's exe.
/// `name` is the manifest's short identity (`launcher`, `viewer`); `file_name` the exe's name on
/// disk. Does nothing when the target is not Windows.
pub fn embed(name: &str, description: &str, file_name: &str) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let ico_path = out.join("mc-rs.ico");
    let manifest_path = out.join("app.manifest");
    let rc_path = out.join("resources.rc");
    std::fs::write(&ico_path, ico(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();
    std::fs::write(&manifest_path, manifest(name, description, &version)).unwrap();
    std::fs::write(&rc_path, rc(&ico_path, &manifest_path, description, file_name, &version)).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_required().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_a_block_and_a_clear_border() {
        let px = icon_rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        let alpha = |x: usize, y: usize| px[(y * 32 + x) * 4 + 3];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(31, 31), 0);
        assert_eq!(alpha(16, 16), 255);
        // The top face is green, the left face brown under its fringe.
        let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..(y * 32 + x) * 4 + 3];
        assert!(at(16, 8)[1] > at(16, 8)[0]);
        assert!(at(8, 24)[0] > at(8, 24)[2]);
    }

    #[test]
    fn ico_directory_points_at_each_image() {
        let sizes = [16, 256];
        let f = ico(&sizes);
        assert_eq!(&f[0..6], &[0, 0, 1, 0, 2, 0]);
        let u32_at = |i: usize| u32::from_le_bytes(f[i..i + 4].try_into().unwrap());
        let (len0, off0) = (u32_at(6 + 8), u32_at(6 + 12));
        let (len1, off1) = (u32_at(22 + 8), u32_at(22 + 12));
        assert_eq!(f[22], 0, "256 is written as 0");
        assert_eq!(off0, 6 + 32);
        assert_eq!(off1, off0 + len0);
        assert_eq!((off1 + len1) as usize, f.len());
        assert_eq!(u32_at(off0 as usize), 40);
    }

    #[test]
    fn version_info_takes_four_parts() {
        let s = rc(Path::new(r"C:\a\b.ico"), Path::new("m"), "d", "x.exe", "0.2.0");
        assert!(s.contains("FILEVERSION 0,2,0,0"));
        assert!(s.contains(r#""C:\\a\\b.ico""#));
    }
}
