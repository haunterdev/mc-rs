//! Server to viewer: the session player's own state and its windows. Entities and world effects
//! are in `server_entity.rs`.

use super::wire::{Rd, Wr};
use super::*;

/// [`S_HEALTH`]: `SPacketUpdateHealth`, with air from the player's metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Health {
    pub health: f32,
    pub food: u8,
    pub saturation: f32,
    pub air: i16,
    /// [`HEALTH_BURNING`].
    pub flags: u8,
    /// `EntityLivingBase.getAbsorptionAmount`.
    pub absorption: f32,
}

impl Health {
    pub const SIZE: usize = 16;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().f32(self.health).u8(self.food).f32(self.saturation).i16(self.air).u8(self.flags).f32(self.absorption).done()
    }

    pub fn decode(b: &[u8]) -> Option<Health> {
        let mut r = Rd(b);
        let m = Health { health: r.f32()?, food: r.u8()?, saturation: r.f32()?, air: r.i16()?, flags: r.u8()?, absorption: r.f32()? };
        r.end(m)
    }
}

/// [`S_XP`]: `SPacketSetExperience`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xp {
    /// The bar, 0 to 1.
    pub bar: f32,
    pub level: i32,
    pub total: i32,
}

impl Xp {
    pub const SIZE: usize = 12;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().f32(self.bar).i32(self.level).i32(self.total).done()
    }

    pub fn decode(b: &[u8]) -> Option<Xp> {
        let mut r = Rd(b);
        let m = Xp { bar: r.f32()?, level: r.i32()?, total: r.i32()? };
        r.end(m)
    }
}

/// [`S_DEATH`]: the death message, already translated.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Death {
    pub message: String,
}

impl Death {
    pub fn encode(&self) -> Vec<u8> {
        Wr::new().str(&self.message).done()
    }

    pub fn decode(b: &[u8]) -> Option<Death> {
        let mut r = Rd(b);
        let m = Death { message: r.str()? };
        r.end(m)
    }
}

/// [`S_SLOTS`]: `SPacketWindowItems` and `SPacketSetSlot`. Each entry is a slot number (-1 the
/// cursor) and its stack.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowSlots {
    pub window: u8,
    pub slots: Vec<(i16, WireStack)>,
}

impl WindowSlots {
    /// At most 65,535 entries are sent.
    pub fn encode(&self) -> Vec<u8> {
        let n = self.slots.len().min(u16::MAX as usize);
        let mut w = Wr::new().u8(self.window).u16(n as u16);
        for &(slot, stack) in &self.slots[..n] {
            w = w.i16(slot).stack(stack);
        }
        w.done()
    }

    pub fn decode(b: &[u8]) -> Option<WindowSlots> {
        let mut r = Rd(b);
        let window = r.u8()?;
        let n = r.u16()? as usize;
        if r.0.len() != n * (2 + WireStack::SIZE) {
            return None;
        }
        let slots = (0..n).map(|_| Some((r.i16()?, r.stack()?))).collect::<Option<Vec<_>>>()?;
        r.end(WindowSlots { window, slots })
    }
}

/// [`S_CONFIRM`]: `SPacketConfirmTransaction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Confirm {
    pub window: u8,
    pub action: u16,
    pub accepted: bool,
}

impl Confirm {
    pub const SIZE: usize = 4;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.window).u16(self.action).u8(self.accepted as u8).done()
    }

    pub fn decode(b: &[u8]) -> Option<Confirm> {
        let mut r = Rd(b);
        let m = Confirm { window: r.u8()?, action: r.u16()?, accepted: r.bool()? };
        r.end(m)
    }
}

/// [`S_OPEN_WINDOW`]: `SPacketOpenWindow`. The window id is 1 or more (0 is the inventory,
/// always open); `title` is a translation key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenWindow {
    pub window: u8,
    /// [`WK_CRAFTING_TABLE`], [`WK_CHEST`], [`WK_FURNACE`] or [`WK_ENCHANTING`].
    pub kind: u8,
    pub rows: u8,
    pub pos: [i32; 3],
    pub title: String,
}

impl OpenWindow {
    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.window).u8(self.kind).u8(self.rows).i32x3(self.pos).str(&self.title).done()
    }

    pub fn decode(b: &[u8]) -> Option<OpenWindow> {
        let mut r = Rd(b);
        let m = OpenWindow {
            window: r.u8().filter(|&w| w != WINDOW_INVENTORY)?,
            kind: r.u8().filter(|k| (WK_CRAFTING_TABLE..=WK_ENCHANTING).contains(k))?,
            rows: r.u8()?,
            pos: r.i32x3()?,
            title: r.str()?,
        };
        r.end(m)
    }
}

/// [`S_WINDOW_PROP`]: `SPacketWindowProperty`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowProp {
    pub window: u8,
    pub property: u16,
    pub value: i16,
}

impl WindowProp {
    pub const SIZE: usize = 5;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u8(self.window).u16(self.property).i16(self.value).done()
    }

    pub fn decode(b: &[u8]) -> Option<WindowProp> {
        let mut r = Rd(b);
        let m = WindowProp { window: r.u8()?, property: r.u16()?, value: r.i16()? };
        r.end(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survival::tests::{trip, COBBLE, DIAMOND_SWORD, SHEEP};

    #[test]
    fn server_messages_round_trip() {
        let health = Health { health: 13.5, food: 17, saturation: 2.25, air: 300, flags: HEALTH_BURNING, absorption: 4.0 };
        trip(&health, Health::encode, Health::decode, Some(Health::SIZE));
        trip(&Xp { bar: 0.75, level: 30, total: 1395 }, Xp::encode, Xp::decode, Some(Xp::SIZE));
        trip(&Death { message: "xy was slain by Zombie".into() }, Death::encode, Death::decode, None);
        trip(&Death { message: String::new() }, Death::encode, Death::decode, Some(2));
        trip(&HeldSlot { slot: 0 }, HeldSlot::encode, HeldSlot::decode, Some(HeldSlot::SIZE));
        let slots = WindowSlots { window: 0, slots: vec![(-1, COBBLE), (36, DIAMOND_SWORD), (45, WireStack::EMPTY)] };
        trip(&slots, WindowSlots::encode, WindowSlots::decode, Some(3 + 3 * (2 + WireStack::SIZE)));
        trip(&WindowSlots { window: 2, slots: vec![] }, WindowSlots::encode, WindowSlots::decode, Some(3));
        trip(&Confirm { window: 1, action: 7, accepted: true }, Confirm::encode, Confirm::decode, Some(Confirm::SIZE));
        let open = OpenWindow { window: 1, kind: WK_CHEST, rows: 6, pos: [10, 64, -20], title: "container.chestDouble".into() };
        trip(&open, OpenWindow::encode, OpenWindow::decode, None);
        trip(&CloseWindow { window: 1 }, CloseWindow::encode, CloseWindow::decode, Some(CloseWindow::SIZE));
        trip(&WindowProp { window: 2, property: 3, value: -200 }, WindowProp::encode, WindowProp::decode, Some(WindowProp::SIZE));
        let base = SpawnObject { id: 1_000_000, pos: [0.5, 65.0, -0.5], yaw: 45.0, pitch: -5.0, motion: [0.1, 0.2, -0.1], data: ObjectData::Item(COBBLE) };
        trip(&base, SpawnObject::encode, SpawnObject::decode, Some(49 + WireStack::SIZE));
        let arrow = SpawnObject { data: ObjectData::Arrow { shooter: 1 }, ..base };
        trip(&arrow, SpawnObject::encode, SpawnObject::decode, Some(49 + 4));
        let sand = SpawnObject { data: ObjectData::FallingBlock { state: 12 << 4 }, ..base };
        trip(&sand, SpawnObject::encode, SpawnObject::decode, Some(49 + 2));
        let orb = SpawnObject { data: ObjectData::XpOrb { value: 2477 }, ..base };
        trip(&orb, SpawnObject::encode, SpawnObject::decode, Some(49 + 2));
        let mob = SpawnMob { id: 1_000_001, kind: MOB_SHEEP, pos: [8.5, 70.0, 8.5], yaw: 10.0, pitch: 0.0, head_yaw: 20.0, motion: [0.0, -0.08, 0.0], meta: SHEEP };
        trip(&mob, SpawnMob::encode, SpawnMob::decode, Some(SpawnMob::SIZE));
        let moves = vec![
            EntityMove { id: 1_000_001, pos: [8.6, 70.0, 8.4], yaw: 12.0, pitch: 0.0, head_yaw: 25.0, on_ground: true, teleport: false },
            EntityMove { id: 2, pos: [-100.25, 80.5, 3.0], yaw: -90.0, pitch: 30.0, head_yaw: -90.0, on_ground: false, teleport: true },
        ];
        trip(&moves, |m| encode_entity_moves(m), decode_entity_moves, Some(2 + 2 * EntityMove::SIZE));
        trip(&vec![], |m: &Vec<EntityMove>| encode_entity_moves(m), decode_entity_moves, Some(2));
        trip(&Velocity { id: 1, motion: [0.4, 0.36, -0.4] }, Velocity::encode, Velocity::decode, Some(Velocity::SIZE));
        trip(&vec![1_000_000u32, 1_000_005, 3], |m: &Vec<u32>| encode_destroy(m), decode_destroy, Some(2 + 3 * 4));
        trip(&EntityEvent { id: 1_000_001, status: EV_LOVE }, EntityEvent::encode, EntityEvent::decode, Some(EntityEvent::SIZE));
        let meta = EntityMeta { id: 1_000_001, payload: MetaPayload::Mob(SHEEP) };
        trip(&meta, EntityMeta::encode, EntityMeta::decode, Some(4 + 1 + MobMeta::SIZE));
        let meta = EntityMeta { id: 1_000_000, payload: MetaPayload::Item(DIAMOND_SWORD) };
        trip(&meta, EntityMeta::encode, EntityMeta::decode, Some(4 + 1 + WireStack::SIZE));
        trip(&Animation { id: 1, animation: ANIM_MAGIC_CRIT }, Animation::encode, Animation::decode, Some(Animation::SIZE));
        trip(&Equipment { id: 1_000_002, slot: EQ_HEAD, stack: DIAMOND_SWORD }, Equipment::encode, Equipment::decode, Some(Equipment::SIZE));
        trip(&Collect { item: 1_000_000, collector: 1, count: 64 }, Collect::encode, Collect::decode, Some(Collect::SIZE));
        trip(&BreakAnim { breaker: 2, pos: [1, 2, 3], stage: -1 }, BreakAnim::encode, BreakAnim::decode, Some(BreakAnim::SIZE));
        trip(&BreakAnim { breaker: 2, pos: [1, 2, 3], stage: 9 }, BreakAnim::encode, BreakAnim::decode, Some(BreakAnim::SIZE));
        trip(&BlockAction { pos: [5, 64, 5], block: 54, a: 1, b: 2 }, BlockAction::encode, BlockAction::decode, Some(BlockAction::SIZE));
        trip(&WorldEvent { event: 1010, pos: [5, 64, -5], data: 2257 }, WorldEvent::encode, WorldEvent::decode, Some(WorldEvent::SIZE));
        let sound = Sound { name: "entity.zombie.ambient".into(), category: 5, pos: [1.0, 2.0, 3.0], volume: 1.0, pitch: 0.9 };
        trip(&sound, Sound::encode, Sound::decode, None);
        trip(&EntityEffect { id: 1, effect: 10, amplifier: 1, duration: 100, flags: EntityEffect::AMBIENT }, EntityEffect::encode, EntityEffect::decode, Some(EntityEffect::SIZE));
        trip(&RemoveEffect { id: 1, effect: 10 }, RemoveEffect::encode, RemoveEffect::decode, Some(RemoveEffect::SIZE));
        trip(&Explosion { pos: [0.0, 64.0, 0.0], strength: 3.0, motion: [0.5, 0.25, 0.0] }, Explosion::encode, Explosion::decode, Some(Explosion::SIZE));
    }
}
