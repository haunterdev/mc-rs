//! The pieces every survival body is built from: the reader and writer, [`WireStack`] and
//! [`MobMeta`].

/// Reads a body front to back; every read is `None` past the end or on a non-finite float.
pub(super) struct Rd<'a>(pub(super) &'a [u8]);

impl<'a> Rd<'a> {
    pub(super) fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }

    pub(super) fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N)?.try_into().ok()
    }

    pub(super) fn u8(&mut self) -> Option<u8> {
        Some(self.arr::<1>()?[0])
    }

    pub(super) fn i8(&mut self) -> Option<i8> {
        Some(self.u8()? as i8)
    }

    pub(super) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.arr()?))
    }

    pub(super) fn i16(&mut self) -> Option<i16> {
        Some(i16::from_le_bytes(self.arr()?))
    }

    pub(super) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.arr()?))
    }

    pub(super) fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.arr()?))
    }

    pub(super) fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.arr()?)).filter(|v| v.is_finite())
    }

    pub(super) fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.arr()?)).filter(|v| v.is_finite())
    }

    pub(super) fn i32x3(&mut self) -> Option<[i32; 3]> {
        Some([self.i32()?, self.i32()?, self.i32()?])
    }

    pub(super) fn f32x3(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }

    pub(super) fn f64x3(&mut self) -> Option<[f64; 3]> {
        Some([self.f64()?, self.f64()?, self.f64()?])
    }

    pub(super) fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// A byte that must be below `n`.
    pub(super) fn below(&mut self, n: u8) -> Option<u8> {
        self.u8().filter(|&v| v < n)
    }

    pub(super) fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.bytes(n)?.to_vec()).ok()
    }

    pub(super) fn stack(&mut self) -> Option<WireStack> {
        WireStack::decode(self.bytes(WireStack::SIZE)?)
    }

    /// `v`, if the body held nothing more.
    pub(super) fn end<T>(self, v: T) -> Option<T> {
        self.0.is_empty().then_some(v)
    }
}

/// Builds a body.
pub(super) struct Wr(Vec<u8>);

impl Wr {
    pub(super) fn new() -> Wr {
        Wr(Vec::with_capacity(64))
    }

    pub(super) fn put(mut self, b: &[u8]) -> Wr {
        self.0.extend_from_slice(b);
        self
    }

    pub(super) fn u8(self, v: u8) -> Wr {
        self.put(&[v])
    }

    pub(super) fn i8(self, v: i8) -> Wr {
        self.put(&[v as u8])
    }

    pub(super) fn u16(self, v: u16) -> Wr {
        self.put(&v.to_le_bytes())
    }

    pub(super) fn i16(self, v: i16) -> Wr {
        self.put(&v.to_le_bytes())
    }

    pub(super) fn u32(self, v: u32) -> Wr {
        self.put(&v.to_le_bytes())
    }

    pub(super) fn i32(self, v: i32) -> Wr {
        self.put(&v.to_le_bytes())
    }

    pub(super) fn f32(self, v: f32) -> Wr {
        self.put(&v.to_le_bytes())
    }

    pub(super) fn i32x3(self, v: [i32; 3]) -> Wr {
        self.i32(v[0]).i32(v[1]).i32(v[2])
    }

    pub(super) fn f32x3(self, v: [f32; 3]) -> Wr {
        self.f32(v[0]).f32(v[1]).f32(v[2])
    }

    pub(super) fn f64x3(self, v: [f64; 3]) -> Wr {
        self.put(&v[0].to_le_bytes()).put(&v[1].to_le_bytes()).put(&v[2].to_le_bytes())
    }

    /// A `u16` length and the UTF-8, cut at a character boundary past 65,535 bytes.
    pub(super) fn str(self, s: &str) -> Wr {
        let mut n = s.len().min(u16::MAX as usize);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.u16(n as u16).put(&s.as_bytes()[..n])
    }

    pub(super) fn stack(self, s: WireStack) -> Wr {
        self.put(&s.encode())
    }

    pub(super) fn done(self) -> Vec<u8> {
        self.0
    }
}

/// An item stack on the wire: 1.12.2 numeric item id (-1 empty; under 256 means the `ItemBlock`
/// of that block), count, meta (damage for tools) and the reduced tag compound `items::Tag` as
/// three bytes: kind (0 none, 1 potion, 2 stored enchantment, 3 spawn egg entity) and two
/// values. `sim` converts to `items::ItemStack`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireStack {
    pub id: i16,
    pub count: u8,
    pub meta: u16,
    pub tag: [u8; 3],
}

impl Default for WireStack {
    fn default() -> WireStack {
        WireStack::EMPTY
    }
}

impl WireStack {
    pub const SIZE: usize = 8;
    pub const EMPTY: WireStack = WireStack { id: -1, count: 0, meta: 0, tag: [0; 3] };

    /// An untagged stack.
    pub const fn new(id: i16, count: u8, meta: u16) -> WireStack {
        WireStack { id, count, meta, tag: [0; 3] }
    }

    pub fn is_empty(&self) -> bool {
        self.id < 0 || self.count == 0
    }

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let s = if self.is_empty() { WireStack::EMPTY } else { *self };
        let mut b = [0u8; Self::SIZE];
        b[..2].copy_from_slice(&s.id.to_le_bytes());
        b[2] = s.count;
        b[3..5].copy_from_slice(&s.meta.to_le_bytes());
        b[5..8].copy_from_slice(&s.tag);
        b
    }

    /// Exactly [`Self::SIZE`] bytes. Ids below -1 and tag kinds above 3 are refused; an empty
    /// stack decodes as [`WireStack::EMPTY`].
    pub fn decode(b: &[u8]) -> Option<WireStack> {
        if b.len() != Self::SIZE {
            return None;
        }
        let s = WireStack { id: i16::from_le_bytes([b[0], b[1]]), count: b[2], meta: u16::from_le_bytes([b[3], b[4]]), tag: [b[5], b[6], b[7]] };
        (s.id >= -1 && s.tag[0] <= 3).then(|| if s.is_empty() { WireStack::EMPTY } else { s })
    }
}

/// What a mob looks like beyond its kind: [`MM_BURNING`](super::MM_BURNING) and the other flags, sheep wool color
/// (`EnumDyeColor` meta, 0 to 15), creeper swell direction (-1, 0 or 1). The fourth byte is
/// reserved and sent as 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MobMeta {
    pub flags: u8,
    pub wool: u8,
    pub swell: i8,
}

impl MobMeta {
    pub const SIZE: usize = 4;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        [self.flags, self.wool, self.swell as u8, 0]
    }

    pub fn decode(b: &[u8]) -> Option<MobMeta> {
        let mut r = Rd(b);
        let m = MobMeta { flags: r.u8()?, wool: r.below(16)?, swell: r.i8().filter(|s| (-1..=1).contains(s))? };
        r.u8()?;
        r.end(m)
    }
}

#[cfg(test)]
mod tests {
    use crate::survival::tests::{COBBLE, DIAMOND_SWORD, SHEEP};
    use crate::survival::*;

    #[test]
    fn stacks_and_mob_meta_round_trip() {
        for s in [DIAMOND_SWORD, COBBLE, WireStack::EMPTY] {
            assert_eq!(WireStack::decode(&s.encode()), Some(s));
        }
        assert_eq!(WireStack::EMPTY.encode(), [0xFF, 0xFF, 0, 0, 0, 0, 0, 0]);
        // Any empty stack goes out and comes back as EMPTY.
        assert_eq!(WireStack::decode(&WireStack::new(4, 0, 3).encode()), Some(WireStack::EMPTY));
        assert_eq!(WireStack::decode(&[0xFE, 0xFF, 1, 0, 0, 0, 0, 0]), None);
        let potion = WireStack { id: 373, count: 1, meta: 0, tag: [1, 5, 0] };
        assert_eq!(WireStack::decode(&potion.encode()), Some(potion));
        assert_eq!(WireStack::decode(&[0x75, 0x01, 1, 0, 0, 4, 0, 0]), None);
        assert_eq!(WireStack::decode(&COBBLE.encode()[..4]), None);
        assert_eq!(MobMeta::decode(&SHEEP.encode()), Some(SHEEP));
        let creeper = MobMeta { flags: MM_IGNITED | MM_POWERED, wool: 0, swell: -1 };
        assert_eq!(MobMeta::decode(&creeper.encode()), Some(creeper));
        assert_eq!(MobMeta::decode(&[0, 16, 0, 0]), None);
        assert_eq!(MobMeta::decode(&[0, 0, 2, 0]), None);
    }

    #[test]
    fn long_strings_cut_at_a_character_boundary() {
        let m = Death { message: "\u{e9}".repeat(40_000) };
        let d = Death::decode(&m.encode()).unwrap();
        assert_eq!(d.message.len(), 65_534);
        assert!(m.message.starts_with(&d.message));
    }
}
