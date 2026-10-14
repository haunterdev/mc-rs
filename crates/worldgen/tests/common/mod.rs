//! Fixture loading and first-difference reporting for the per-stage diff tests.
//! Fixtures come from `tools/fixtures.py gen` and live in `fixtures/` (gitignored).
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Debug;
use std::io::Read;
use std::path::PathBuf;

pub enum Record {
    I32(Vec<i32>),
    U16(Vec<u16>),
    F64(Vec<f64>),
    F32(Vec<f32>),
    I64(Vec<i64>),
}

pub struct Fixture {
    pub name: String,
    records: HashMap<String, Record>,
}

pub fn dir() -> PathBuf {
    match std::env::var_os("MCRS_FIXTURES") {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"),
    }
}

pub fn json(name: &str) -> serde_json::Value {
    let path = dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}. Generate fixtures with `python tools/fixtures.py gen`.", path.display()));
    serde_json::from_str(&text).unwrap()
}

pub fn manifest() -> serde_json::Value {
    json("manifest.json")
}

/// Every seed listed in the manifest.
pub fn seeds() -> Vec<i64> {
    manifest()["seeds"].as_array().unwrap().iter().map(|s| s["seed"].as_i64().unwrap()).collect()
}

impl Fixture {
    pub fn load(file: &str) -> Fixture {
        let path = dir().join(file);
        let f = std::fs::File::open(&path)
            .unwrap_or_else(|e| panic!("{}: {e}. Generate fixtures with `python tools/fixtures.py gen`.", path.display()));
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(f).read_to_end(&mut bytes).unwrap();
        assert_eq!(&bytes[..8], b"MCRSFIX1", "{}: bad magic", path.display());
        let mut pos = 8;
        let mut records = HashMap::new();
        let u32_at = |p: usize| u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
        while pos < bytes.len() {
            let nlen = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
            pos += 2;
            let name = String::from_utf8(bytes[pos..pos + nlen].to_vec()).unwrap();
            pos += nlen;
            let dtype = bytes[pos];
            pos += 1;
            let count = u32_at(pos) as usize;
            pos += 4;
            let size = match dtype {
                1 | 4 => 4,
                2 => 2,
                3 | 5 => 8,
                d => panic!("unknown dtype {d}"),
            };
            let body = &bytes[pos..pos + count * size];
            pos += count * size;
            let rec = match dtype {
                1 => Record::I32(body.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()),
                2 => Record::U16(body.chunks_exact(2).map(|c| u16::from_le_bytes(c.try_into().unwrap())).collect()),
                3 => Record::F64(body.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()),
                4 => Record::F32(body.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()),
                _ => Record::I64(body.chunks_exact(8).map(|c| i64::from_le_bytes(c.try_into().unwrap())).collect()),
            };
            records.insert(name, rec);
        }
        Fixture { name: file.to_string(), records }
    }

    fn get(&self, name: &str) -> &Record {
        self.records.get(name).unwrap_or_else(|| panic!("{}: no record {name}", self.name))
    }

    pub fn i32(&self, name: &str) -> &[i32] {
        match self.get(name) {
            Record::I32(v) => v,
            _ => panic!("{name}: not i32"),
        }
    }

    pub fn u16(&self, name: &str) -> &[u16] {
        match self.get(name) {
            Record::U16(v) => v,
            _ => panic!("{name}: not u16"),
        }
    }

    pub fn f64(&self, name: &str) -> &[f64] {
        match self.get(name) {
            Record::F64(v) => v,
            _ => panic!("{name}: not f64"),
        }
    }

    pub fn f32(&self, name: &str) -> &[f32] {
        match self.get(name) {
            Record::F32(v) => v,
            _ => panic!("{name}: not f32"),
        }
    }

    pub fn i64(&self, name: &str) -> &[i64] {
        match self.get(name) {
            Record::I64(v) => v,
            _ => panic!("{name}: not i64"),
        }
    }
}

/// Values compared by representation, so -0.0 vs 0.0 counts as a difference and NaN equals NaN.
pub trait Exact: Copy + Debug {
    fn same(self, other: Self) -> bool;
}

impl Exact for i32 {
    fn same(self, o: Self) -> bool {
        self == o
    }
}
impl Exact for i64 {
    fn same(self, o: Self) -> bool {
        self == o
    }
}
impl Exact for u16 {
    fn same(self, o: Self) -> bool {
        self == o
    }
}
impl Exact for f64 {
    fn same(self, o: Self) -> bool {
        self.to_bits() == o.to_bits() || (self.is_nan() && o.is_nan())
    }
}
impl Exact for f32 {
    fn same(self, o: Self) -> bool {
        self.to_bits() == o.to_bits() || (self.is_nan() && o.is_nan())
    }
}

/// Index of the first differing element, with both values.
pub fn first_diff<T: Exact>(expected: &[T], actual: &[T]) -> Option<(usize, T, T)> {
    assert_eq!(expected.len(), actual.len(), "length mismatch");
    expected.iter().zip(actual).enumerate().find(|(_, (e, a))| !e.same(**a)).map(|(i, (e, a))| (i, *e, *a))
}

/// Panics with the first difference; `locate` turns the flat index into a coordinate.
pub fn assert_same<T: Exact>(label: &str, expected: &[T], actual: &[T], locate: impl Fn(usize) -> String) {
    if let Some((i, e, a)) = first_diff(expected, actual) {
        let total = expected.iter().zip(actual).filter(|(e, a)| !e.same(**a)).count();
        panic!("{label}: first difference at {} (index {i}): expected {e:?}, got {a:?} ({total} differ)", locate(i));
    }
}

pub fn flat(i: usize) -> String {
    format!("[{i}]")
}

/// ChunkPrimer index `x << 12 | z << 8 | y` to a coordinate.
pub fn block_at(cx: i32, cz: i32) -> impl Fn(usize) -> String {
    move |i| {
        let (x, z, y) = ((i >> 12) & 15, (i >> 8) & 15, i & 255);
        format!("chunk ({cx}, {cz}) local x={x} y={y} z={z} (block {}, {y}, {})", cx * 16 + x as i32, cz * 16 + z as i32)
    }
}

/// Formats a ChunkPrimer state id as `id:meta`.
pub fn state(v: u16) -> String {
    format!("{}:{}", v >> 4, v & 15)
}

/// Like assert_same for primer data, printing states as `id:meta`.
pub fn assert_blocks(label: &str, cx: i32, cz: i32, expected: &[u16], actual: &[u16]) {
    if let Some((i, e, a)) = first_diff(expected, actual) {
        let total = expected.iter().zip(actual).filter(|(e, a)| e != a).count();
        panic!(
            "{label}: first difference at {}: expected {}, got {} ({total} blocks differ)",
            block_at(cx, cz)(i),
            state(e),
            state(a)
        );
    }
}
