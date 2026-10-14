//! Copies the compressed viewer that tools/dist.py names in MCRS_PAYLOAD into OUT_DIR for
//! payload.rs to include, and derives the id its install is stamped with. Without MCRS_PAYLOAD (a
//! plain `cargo build --workspace`) the payload is empty and Play says so. Links the icon, version
//! info and manifest (crates/brand).

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=MCRS_PAYLOAD");
    println!("cargo:rerun-if-env-changed=MCRS_UPDATE_URL");
    let data = match std::env::var_os("MCRS_PAYLOAD") {
        Some(p) => {
            println!("cargo:rerun-if-changed={}", PathBuf::from(&p).display());
            std::fs::read(&p).expect("reading MCRS_PAYLOAD")
        }
        None => Vec::new(),
    };
    // FNV-1a 64: a new build installs over the old one even at the same version number.
    let id = data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("viewer.zst");
    std::fs::write(out, &data).unwrap();
    println!("cargo:rustc-env=MCRS_PAYLOAD_ID={id:016x}");
    brand::embed("launcher", "mc-rs launcher", "mc-rs-launcher.exe");
}
