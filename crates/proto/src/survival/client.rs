//! Viewer to server: the V_ messages.

use super::wire::{Rd, Wr};
use super::*;

/// The body of [`V_RESPAWN`] and [`V_SWAP_HANDS`].
pub fn encode_empty() -> Vec<u8> {
    Vec::new()
}

pub fn decode_empty(b: &[u8]) -> Option<()> {
    b.is_empty().then_some(())
}

/// [`V_CLIENT_INFO`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClientInfo {
    pub version: u16,
}

impl ClientInfo {
    pub const SIZE: usize = 2;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u16(self.version).done()
    }

    pub fn decode(b: &[u8]) -> Option<ClientInfo> {
        let mut r = Rd(b);
        let m = ClientInfo { version: r.u16()? };
        r.end(m)
    }
}

/// [`V_DIG`]: `CPacketPlayerDigging` START, ABORT and STOP_DESTROY_BLOCK. `state` is the block
/// state the client saw there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dig {
    /// [`DIG_START`], [`DIG_ABORT`] or [`DIG_FINISH`].
    pub action: u8,
    pub pos: [i32; 3],
    /// `EnumFacing` index, 0 to 5.
    pub face: u8,
    pub state: u16,
}

impl Dig {
    pub const SIZE: usize = 16;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.action).i32x3(self.pos).u8(self.face).u16(self.state).done()
    }

    pub fn decode(b: &[u8]) -> Option<Dig> {
        let mut r = Rd(b);
        let m = Dig { action: r.below(3)?, pos: r.i32x3()?, face: r.below(6)?, state: r.u16()? };
        r.end(m)
    }
}

/// [`V_DROP`]: one held item, or the whole held stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropItem {
    pub all: bool,
}

impl DropItem {
    pub const SIZE: usize = 1;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.all as u8).done()
    }

    pub fn decode(b: &[u8]) -> Option<DropItem> {
        let mut r = Rd(b);
        let m = DropItem { all: r.bool()? };
        r.end(m)
    }
}

/// [`V_HELD`] and [`S_HELD`]: hotbar slot 0 to 8.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeldSlot {
    pub slot: u8,
}

impl HeldSlot {
    pub const SIZE: usize = 1;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.slot).done()
    }

    pub fn decode(b: &[u8]) -> Option<HeldSlot> {
        let mut r = Rd(b);
        let m = HeldSlot { slot: r.below(9)? };
        r.end(m)
    }
}

/// [`V_USE_ITEM`]: `CPacketPlayerTryUseItem` and RELEASE_USE_ITEM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UseItem {
    /// [`HAND_MAIN`] or [`HAND_OFF`].
    pub hand: u8,
    /// [`USE_START`] or [`USE_RELEASE`].
    pub phase: u8,
}

impl UseItem {
    pub const SIZE: usize = 2;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.hand).u8(self.phase).done()
    }

    pub fn decode(b: &[u8]) -> Option<UseItem> {
        let mut r = Rd(b);
        let m = UseItem { hand: r.below(2)?, phase: r.below(2)? };
        r.end(m)
    }
}

/// [`V_USE_ON`]: `CPacketPlayerTryUseItemOnBlock`. `state` is the block state the client placed
/// (0 none).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UseOn {
    pub pos: [i32; 3],
    pub face: u8,
    /// Where on the block face, 0 to 1 in each axis.
    pub hit: [f32; 3],
    pub hand: u8,
    pub state: u16,
}

impl UseOn {
    pub const SIZE: usize = 28;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().i32x3(self.pos).u8(self.face).f32x3(self.hit).u8(self.hand).u16(self.state).done()
    }

    pub fn decode(b: &[u8]) -> Option<UseOn> {
        let mut r = Rd(b);
        let m = UseOn { pos: r.i32x3()?, face: r.below(6)?, hit: r.f32x3()?, hand: r.below(2)?, state: r.u16()? };
        r.end(m)
    }
}

/// [`V_ATTACK`]: `CPacketUseEntity` ATTACK.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attack {
    pub target: u32,
}

impl Attack {
    pub const SIZE: usize = 4;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.target).done()
    }

    pub fn decode(b: &[u8]) -> Option<Attack> {
        let mut r = Rd(b);
        let m = Attack { target: r.u32()? };
        r.end(m)
    }
}

/// [`V_INTERACT`]: `CPacketUseEntity` INTERACT.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Interact {
    pub target: u32,
    pub hand: u8,
}

impl Interact {
    pub const SIZE: usize = 5;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.target).u8(self.hand).done()
    }

    pub fn decode(b: &[u8]) -> Option<Interact> {
        let mut r = Rd(b);
        let m = Interact { target: r.u32()?, hand: r.below(2)? };
        r.end(m)
    }
}

/// [`V_CLICK`]: `CPacketClickWindow`. Slot numbers are vanilla's per container; `action` is the
/// number [`Confirm`] answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Click {
    pub window: u8,
    pub slot: i16,
    pub button: u8,
    /// `ClickType` ordinal, [`CLICK_PICKUP`] to [`CLICK_PICKUP_ALL`].
    pub mode: u8,
    pub action: u16,
}

impl Click {
    pub const SIZE: usize = 7;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.window).i16(self.slot).u8(self.button).u8(self.mode).u16(self.action).done()
    }

    pub fn decode(b: &[u8]) -> Option<Click> {
        let mut r = Rd(b);
        let m = Click { window: r.u8()?, slot: r.i16()?, button: r.u8()?, mode: r.below(CLICK_PICKUP_ALL + 1)?, action: r.u16()? };
        r.end(m)
    }
}

/// [`V_CLOSE`] and [`S_CLOSE_WINDOW`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloseWindow {
    pub window: u8,
}

impl CloseWindow {
    pub const SIZE: usize = 1;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.window).done()
    }

    pub fn decode(b: &[u8]) -> Option<CloseWindow> {
        let mut r = Rd(b);
        let m = CloseWindow { window: r.u8()? };
        r.end(m)
    }
}

/// [`V_ENCHANT`]: `CPacketEnchantItem`, the window and the button (0 to 2) pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnchantItem {
    pub window: u8,
    pub button: u8,
}

impl EnchantItem {
    pub const SIZE: usize = 2;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.window).u8(self.button).done()
    }

    pub fn decode(b: &[u8]) -> Option<EnchantItem> {
        let mut r = Rd(b);
        let m = EnchantItem { window: r.u8()?, button: r.u8().filter(|&b| b < 3)? };
        r.end(m)
    }
}

/// [`V_CREATIVE_SLOT`]: `CPacketCreativeInventoryAction`, `ContainerPlayer` numbering, -1 throws
/// the stack out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CreativeSlot {
    pub slot: i16,
    pub stack: WireStack,
}

impl CreativeSlot {
    pub const SIZE: usize = 2 + WireStack::SIZE;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().i16(self.slot).stack(self.stack).done()
    }

    pub fn decode(b: &[u8]) -> Option<CreativeSlot> {
        let mut r = Rd(b);
        let m = CreativeSlot { slot: r.i16()?, stack: r.stack()? };
        r.end(m)
    }
}

/// The most characters a sign line may hold on the wire (`CPacketUpdateSign` reads each line with
/// `readString(384)`).
pub const SIGN_LINE_MAX: usize = 384;

/// [`V_SIGN`] and [`S_SIGN`]: a sign's position and its four lines (`CPacketUpdateSign`, and the
/// `Text1` to `Text4` of the sign's update packet).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignText {
    pub pos: [i32; 3],
    pub lines: [String; 4],
}

impl SignText {
    /// The longest body: the position and four lines of [`SIGN_LINE_MAX`] characters of up to
    /// four UTF-8 bytes each.
    pub const MAX_SIZE: usize = 12 + 4 * (2 + SIGN_LINE_MAX * 4);

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Wr::new().i32x3(self.pos);
        for l in &self.lines {
            w = w.str(l);
        }
        w.done()
    }

    pub fn decode(b: &[u8]) -> Option<SignText> {
        let mut r = Rd(b);
        let pos = r.i32x3()?;
        let mut line = || r.str().filter(|s| s.chars().count() <= SIGN_LINE_MAX);
        let lines = [line()?, line()?, line()?, line()?];
        r.end(SignText { pos, lines })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survival::tests::{trip, COBBLE};

    #[test]
    fn viewer_messages_round_trip() {
        assert_eq!(decode_empty(&encode_empty()), Some(()));
        assert_eq!(decode_empty(&[0]), None);
        trip(&ClientInfo { version: SURVIVAL_PROTOCOL }, ClientInfo::encode, ClientInfo::decode, Some(ClientInfo::SIZE));
        trip(&Dig { action: DIG_FINISH, pos: [-5, 64, 1_000_000], face: 5, state: 1 << 4 }, Dig::encode, Dig::decode, Some(Dig::SIZE));
        trip(&DropItem { all: true }, DropItem::encode, DropItem::decode, Some(DropItem::SIZE));
        trip(&DropItem { all: false }, DropItem::encode, DropItem::decode, Some(DropItem::SIZE));
        trip(&HeldSlot { slot: 8 }, HeldSlot::encode, HeldSlot::decode, Some(HeldSlot::SIZE));
        trip(&UseItem { hand: HAND_OFF, phase: USE_RELEASE }, UseItem::encode, UseItem::decode, Some(UseItem::SIZE));
        let use_on = UseOn { pos: [3, 70, -9], face: 1, hit: [0.5, 1.0, 0.25], hand: HAND_MAIN, state: 4 << 4 };
        trip(&use_on, UseOn::encode, UseOn::decode, Some(UseOn::SIZE));
        trip(&Attack { target: 1_000_007 }, Attack::encode, Attack::decode, Some(Attack::SIZE));
        trip(&Interact { target: 1_000_008, hand: HAND_OFF }, Interact::encode, Interact::decode, Some(Interact::SIZE));
        let click = Click { window: 3, slot: -999, button: 1, mode: CLICK_QUICK_CRAFT, action: 512 };
        trip(&click, Click::encode, Click::decode, Some(Click::SIZE));
        trip(&CloseWindow { window: 100 }, CloseWindow::encode, CloseWindow::decode, Some(CloseWindow::SIZE));
        trip(&EnchantItem { window: 5, button: 2 }, EnchantItem::encode, EnchantItem::decode, Some(EnchantItem::SIZE));
        assert_eq!(EnchantItem::decode(&[5, 3]), None);
        trip(&CreativeSlot { slot: 36, stack: COBBLE }, CreativeSlot::encode, CreativeSlot::decode, Some(CreativeSlot::SIZE));
        trip(&CreativeSlot { slot: -1, stack: WireStack::EMPTY }, CreativeSlot::encode, CreativeSlot::decode, Some(CreativeSlot::SIZE));
        let sign = SignText { pos: [-4, 70, 12], lines: ["Hello".into(), String::new(), "\u{e9}t\u{e9}".into(), "x".repeat(SIGN_LINE_MAX)] };
        trip(&sign, SignText::encode, SignText::decode, None);
        assert!(sign.encode().len() <= SignText::MAX_SIZE);
    }
}
