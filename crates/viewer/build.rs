//! Links the icon, version info and manifest (crates/brand).

fn main() {
    brand::embed("viewer", "mc-rs viewer", "mc-rs-viewer.exe");
}
