//! Jar data for the tests: `MCRS_JAR_DATA` points at a folder holding the client jar's
//! `assets/minecraft` (recipes, lang) extracted for testing. Tests that need it skip without it,
//! the way the viewer's jar tests skip without a jar. Nothing read here is written anywhere.

#![allow(dead_code)]

use std::path::PathBuf;

pub fn jar_data() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("MCRS_JAR_DATA")?);
    p.join("assets/minecraft").is_dir().then_some(p)
}

/// Every recipe file as `(jar entry name, bytes)`, as a caller reading the jar would pass them.
pub fn recipe_entries() -> Option<Vec<(String, Vec<u8>)>> {
    let root = jar_data()?;
    let dir = root.join("assets/minecraft/recipes");
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).ok()? {
        let e = e.ok()?;
        let name = e.file_name().to_string_lossy().into_owned();
        out.push((format!("assets/minecraft/recipes/{name}"), std::fs::read(e.path()).ok()?));
    }
    Some(out)
}

pub fn lang() -> Option<Vec<u8>> {
    std::fs::read(jar_data()?.join("assets/minecraft/lang/en_us.lang")).ok()
}
