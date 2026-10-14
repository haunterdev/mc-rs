//! Survival messages: digging, items, inventories and
//! windows, health and experience, and the server's entities.
//!
//! Tags V_ 60..99 and S_ 160..219 are reserved for these. Bodies are fixed little-endian layouts
//! like the rest of proto. Every message has an `encode` returning the body and a `decode` that
//! returns `None` for a body of the wrong length, a non-finite float, an enum value outside its
//! range, or a string that is not UTF-8. Strings are a `u16` byte length, then UTF-8.
//!
//! This file holds the tags and enum values; `wire.rs` the body reader and writer, `client.rs`
//! the V_ messages, `server.rs` and `server_entity.rs` the S_ messages.

mod client;
mod server;
mod server_entity;
mod wire;

pub use client::*;
pub use server::*;
pub use server_entity::*;
pub use wire::{MobMeta, WireStack};

/// The survival protocol version a viewer announces in [`V_CLIENT_INFO`]. 2: [`WireStack`]
/// carries the tag (8 bytes). 3: sign text ([`V_SIGN`], [`S_SIGN`]).
pub const SURVIVAL_PROTOCOL: u16 = 3;

/// Viewer to server: [`ClientInfo`], once right after the welcome.
pub const V_CLIENT_INFO: u8 = 60;
/// [`Dig`]: start, abort or finish breaking a block.
pub const V_DIG: u8 = 61;
/// [`DropItem`]: throw one held item or the whole held stack.
pub const V_DROP: u8 = 62;
/// [`HeldSlot`]: the hotbar slot in hand.
pub const V_HELD: u8 = 63;
/// [`UseItem`]: use the held item in the air, or release it (bow, food).
pub const V_USE_ITEM: u8 = 64;
/// [`UseOn`]: use the held item on a block face.
pub const V_USE_ON: u8 = 65;
/// [`Attack`]: hit an entity.
pub const V_ATTACK: u8 = 66;
/// [`Interact`]: right-click an entity.
pub const V_INTERACT: u8 = 67;
/// [`Click`]: a click in an open window (`CPacketClickWindow`).
pub const V_CLICK: u8 = 68;
/// [`CloseWindow`]: the viewer closed a window.
pub const V_CLOSE: u8 = 69;
/// [`CreativeSlot`]: creative mode sets a slot outright.
pub const V_CREATIVE_SLOT: u8 = 70;
/// Empty body ([`encode_empty`]): respawn after death.
pub const V_RESPAWN: u8 = 71;
/// Empty body ([`encode_empty`]): swap the main and off hand.
pub const V_SWAP_HANDS: u8 = 72;
/// [`SignText`]: the four lines the edit screen closed with (`CPacketUpdateSign`).
pub const V_SIGN: u8 = 73;

/// Server to viewer: [`Health`].
pub const S_HEALTH: u8 = 160;
/// [`Xp`].
pub const S_XP: u8 = 161;
/// [`Death`]: the session player died.
pub const S_DEATH: u8 = 162;
/// [`HeldSlot`]: the server moved the hotbar selection.
pub const S_HELD: u8 = 163;
/// [`WindowSlots`]: slot contents; slot -1 is the cursor.
pub const S_SLOTS: u8 = 164;
/// [`Confirm`]: whether a [`Click`] was accepted.
pub const S_CONFIRM: u8 = 165;
/// [`OpenWindow`].
pub const S_OPEN_WINDOW: u8 = 166;
/// [`CloseWindow`]: the server closed a window.
pub const S_CLOSE_WINDOW: u8 = 167;
/// [`WindowProp`]: a furnace's progress bars and the like.
pub const S_WINDOW_PROP: u8 = 168;
/// [`SpawnObject`]: an item, arrow or falling block.
pub const S_SPAWN_OBJECT: u8 = 169;
/// [`SpawnMob`].
pub const S_SPAWN_MOB: u8 = 170;
/// [`encode_entity_moves`]: where entities are now, batched.
pub const S_ENTITY_MOVE: u8 = 171;
/// [`Velocity`]: an entity's motion; for the session player's own id, knockback.
pub const S_VELOCITY: u8 = 172;
/// [`encode_destroy`]: entities gone.
pub const S_DESTROY: u8 = 173;
/// [`EntityEvent`]: hurt, death, love hearts and the like.
pub const S_ENTITY_EVENT: u8 = 174;
/// [`EntityMeta`]: a mob's [`MobMeta`] or an item entity's stack changed.
pub const S_ENTITY_META: u8 = 175;
/// [`Animation`]: an arm swing or crit particles.
pub const S_ANIMATION: u8 = 176;
/// [`Equipment`]: what an entity holds or wears.
pub const S_EQUIPMENT: u8 = 177;
/// [`Collect`]: an item entity flies into a collector.
pub const S_COLLECT: u8 = 178;
/// [`BreakAnim`]: another player's crack overlay on a block.
pub const S_BREAK_ANIM: u8 = 179;
/// [`BlockAction`]: a chest lid and the like.
pub const S_BLOCK_ACTION: u8 = 180;
/// [`Sound`].
pub const S_SOUND: u8 = 181;
/// [`Explosion`]: changed blocks arrive as `S_BLOCKS`.
pub const S_EXPLOSION: u8 = 182;
/// [`SignText`]: a sign's lines, on join for every sign and after each edit
/// (`SPacketUpdateTileEntity` for a sign).
pub const S_SIGN: u8 = 183;

/// `EnumHand`.
pub const HAND_MAIN: u8 = 0;
pub const HAND_OFF: u8 = 1;

/// [`Dig`] actions.
pub const DIG_START: u8 = 0;
pub const DIG_ABORT: u8 = 1;
pub const DIG_FINISH: u8 = 2;

/// [`UseItem`] phases.
pub const USE_START: u8 = 0;
pub const USE_RELEASE: u8 = 1;

/// `ClickType` ordinals, [`Click::mode`].
pub const CLICK_PICKUP: u8 = 0;
pub const CLICK_QUICK_MOVE: u8 = 1;
pub const CLICK_SWAP: u8 = 2;
pub const CLICK_CLONE: u8 = 3;
pub const CLICK_THROW: u8 = 4;
pub const CLICK_QUICK_CRAFT: u8 = 5;
pub const CLICK_PICKUP_ALL: u8 = 6;

/// Window id 0: the player inventory, always open.
pub const WINDOW_INVENTORY: u8 = 0;

/// [`OpenWindow::kind`].
pub const WK_CRAFTING_TABLE: u8 = 1;
pub const WK_CHEST: u8 = 2;
pub const WK_FURNACE: u8 = 3;

/// [`Health::flags`]: the player is on fire.
pub const HEALTH_BURNING: u8 = 1;

/// [`SpawnObject`] kinds (`SPacketSpawnObject` type ids).
pub const OBJ_ITEM: u8 = 2;
pub const OBJ_ARROW: u8 = 60;
pub const OBJ_FALLING_BLOCK: u8 = 70;

/// [`SpawnMob::kind`]: `EntityList` ids.
pub const MOB_CREEPER: u16 = 50;
pub const MOB_SKELETON: u16 = 51;
pub const MOB_SPIDER: u16 = 52;
pub const MOB_ZOMBIE: u16 = 54;
pub const MOB_PIG: u16 = 90;
pub const MOB_SHEEP: u16 = 91;
pub const MOB_COW: u16 = 92;
pub const MOB_CHICKEN: u16 = 93;

/// [`MobMeta::flags`]. New flags take free bits.
pub const MM_BURNING: u8 = 1;
pub const MM_CHILD: u8 = 2;
/// Arms raised (zombie) or aggressive.
pub const MM_AGGRESSIVE: u8 = 4;
pub const MM_SHEARED: u8 = 8;
pub const MM_IGNITED: u8 = 16;
pub const MM_POWERED: u8 = 32;

/// [`EntityEvent::status`] values survival sends (`SPacketEntityStatus`).
pub const EV_HURT: u8 = 2;
pub const EV_DEATH: u8 = 3;
pub const EV_USE_FINISHED: u8 = 9;
pub const EV_EAT_GRASS: u8 = 10;
pub const EV_LOVE: u8 = 18;

/// [`Animation::animation`] (`SPacketAnimation`, 0 to 5).
pub const ANIM_SWING_MAIN: u8 = 0;
pub const ANIM_CRIT: u8 = 4;
pub const ANIM_MAGIC_CRIT: u8 = 5;

/// [`Equipment::slot`]: hands, then armor from feet to head.
pub const EQ_MAIN_HAND: u8 = 0;
pub const EQ_OFF_HAND: u8 = 1;
pub const EQ_FEET: u8 = 2;
pub const EQ_LEGS: u8 = 3;
pub const EQ_CHEST: u8 = 4;
pub const EQ_HEAD: u8 = 5;

/// `SoundCategory` has this many values; [`Sound::category`] is below it.
pub const SOUND_CATEGORIES: u8 = 10;

/// The fixtures and round-trip helper every side's tests share, and the bad-body test that
/// covers all of them.
#[cfg(test)]
mod tests {
    use super::*;

    pub(super) const DIAMOND_SWORD: WireStack = WireStack::new(276, 1, 17);
    pub(super) const COBBLE: WireStack = WireStack::new(4, 64, 0);
    pub(super) const SHEEP: MobMeta = MobMeta { flags: MM_CHILD | MM_SHEARED, wool: 14, swell: 0 };

    /// Round-trips `m`, checks the body is `size` bytes when given, and that the body one byte
    /// short and one byte long are both refused.
    pub(super) fn trip<T: PartialEq + std::fmt::Debug>(m: &T, enc: impl Fn(&T) -> Vec<u8>, dec: impl Fn(&[u8]) -> Option<T>, size: Option<usize>) {
        let b = enc(m);
        if let Some(n) = size {
            assert_eq!(b.len(), n, "{m:?}");
        }
        assert_eq!(dec(&b).as_ref(), Some(m));
        if !b.is_empty() {
            assert_eq!(dec(&b[..b.len() - 1]), None, "short {m:?}");
        }
        let mut long = b.clone();
        long.push(0);
        assert_eq!(dec(&long), None, "long {m:?}");
    }

    #[test]
    fn decode_refuses_bad_bodies() {
        // Non-finite floats.
        let mut b = Health { health: 20.0, ..Default::default() }.encode();
        b[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Health::decode(&b), None);
        let mut b = Explosion::default().encode();
        b[8..16].copy_from_slice(&f64::INFINITY.to_le_bytes());
        assert_eq!(Explosion::decode(&b), None);
        let mut b = UseOn::default().encode();
        b[13..17].copy_from_slice(&f32::NEG_INFINITY.to_le_bytes());
        assert_eq!(UseOn::decode(&b), None);
        // Short and empty bodies.
        assert_eq!(Dig::decode(&Dig::default().encode()[..15]), None);
        assert_eq!(SpawnMob::decode(&[]), None);
        assert_eq!(decode_destroy(&[2, 0, 1, 0, 0, 0]), None);
        assert_eq!(WindowSlots::decode(&[0, 1, 0, 0, 0]), None);
        // Unknown actions and enum values.
        assert_eq!(Dig::decode(&Dig { action: 3, ..Default::default() }.encode()), None);
        assert_eq!(Dig::decode(&Dig { face: 6, ..Default::default() }.encode()), None);
        assert_eq!(DropItem::decode(&[2]), None);
        assert_eq!(HeldSlot::decode(&[9]), None);
        assert_eq!(UseItem::decode(&[2, 0]), None);
        assert_eq!(UseItem::decode(&[0, 2]), None);
        assert_eq!(Interact::decode(&Interact { target: 1, hand: 2 }.encode()), None);
        assert_eq!(Click::decode(&Click { mode: 7, ..Default::default() }.encode()), None);
        assert_eq!(Confirm::decode(&[1, 0, 0, 2]), None);
        let open = OpenWindow { window: 1, kind: WK_FURNACE, rows: 0, pos: [0; 3], title: "container.furnace".into() };
        assert_eq!(OpenWindow::decode(&OpenWindow { kind: 4, ..open.clone() }.encode()), None);
        assert_eq!(OpenWindow::decode(&OpenWindow { window: WINDOW_INVENTORY, ..open.clone() }.encode()), None);
        let mut b = SpawnObject::default().encode();
        b[4] = 61;
        assert_eq!(SpawnObject::decode(&b), None);
        let mut b = EntityMeta::default().encode();
        b[4] = 2;
        assert_eq!(EntityMeta::decode(&b), None);
        assert_eq!(Animation::decode(&Animation { id: 1, animation: 6 }.encode()), None);
        assert_eq!(Equipment::decode(&Equipment { slot: 6, ..Default::default() }.encode()), None);
        assert_eq!(BreakAnim::decode(&BreakAnim { stage: 10, ..Default::default() }.encode()), None);
        assert_eq!(BreakAnim::decode(&BreakAnim { stage: -2, ..Default::default() }.encode()), None);
        assert_eq!(Sound::decode(&Sound { category: SOUND_CATEGORIES, ..Default::default() }.encode()), None);
        let mut b = encode_entity_moves(&[EntityMove::default()]);
        *b.last_mut().unwrap() = 2;
        assert_eq!(decode_entity_moves(&b), None);
        // A string that is not UTF-8, and one whose length runs past the body.
        assert_eq!(Death::decode(&[2, 0, 0xC3, 0x28]), None);
        assert_eq!(Death::decode(&[5, 0, b'a']), None);
        // A sign line longer than `readString(384)` takes.
        let long = SignText { pos: [0; 3], lines: ["x".repeat(SIGN_LINE_MAX + 1), String::new(), String::new(), String::new()] };
        assert_eq!(SignText::decode(&long.encode()), None);
    }
}
