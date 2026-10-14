//! Server to viewer: entities (spawn, move, meta and the rest) and world effects (block
//! actions, sounds, explosions).

use super::wire::{Rd, Wr};
use super::*;

/// What follows a [`SpawnObject`], by kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectData {
    /// [`OBJ_ITEM`]: the stack.
    Item(WireStack),
    /// [`OBJ_ARROW`]: the entity that shot it.
    Arrow { shooter: u32 },
    /// [`OBJ_FALLING_BLOCK`]: the block state.
    FallingBlock { state: u16 },
}

impl Default for ObjectData {
    fn default() -> ObjectData {
        ObjectData::Item(WireStack::EMPTY)
    }
}

impl ObjectData {
    /// The `SPacketSpawnObject` type id.
    pub fn kind(&self) -> u8 {
        match self {
            ObjectData::Item(_) => OBJ_ITEM,
            ObjectData::Arrow { .. } => OBJ_ARROW,
            ObjectData::FallingBlock { .. } => OBJ_FALLING_BLOCK,
        }
    }
}

/// [`S_SPAWN_OBJECT`]: `SPacketSpawnObject`. On the wire the kind byte follows the id.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpawnObject {
    pub id: u32,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub motion: [f32; 3],
    pub data: ObjectData,
}

impl SpawnObject {
    pub fn encode(&self) -> Vec<u8> {
        let w = Wr::new().u32(self.id).u8(self.data.kind()).f64x3(self.pos).f32(self.yaw).f32(self.pitch).f32x3(self.motion);
        let w = match self.data {
            ObjectData::Item(s) => w.stack(s),
            ObjectData::Arrow { shooter } => w.u32(shooter),
            ObjectData::FallingBlock { state } => w.u16(state),
        };
        w.done()
    }

    pub fn decode(b: &[u8]) -> Option<SpawnObject> {
        let mut r = Rd(b);
        let id = r.u32()?;
        let kind = r.u8()?;
        let (pos, yaw, pitch, motion) = (r.f64x3()?, r.f32()?, r.f32()?, r.f32x3()?);
        let data = match kind {
            OBJ_ITEM => ObjectData::Item(r.stack()?),
            OBJ_ARROW => ObjectData::Arrow { shooter: r.u32()? },
            OBJ_FALLING_BLOCK => ObjectData::FallingBlock { state: r.u16()? },
            _ => return None,
        };
        r.end(SpawnObject { id, pos, yaw, pitch, motion, data })
    }
}

/// [`S_SPAWN_MOB`]: `SPacketSpawnMob`. `kind` is the `EntityList` id ([`MOB_PIG`] and the
/// rest); the game decides which kinds it knows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpawnMob {
    pub id: u32,
    pub kind: u16,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    pub motion: [f32; 3],
    pub meta: MobMeta,
}

impl SpawnMob {
    pub const SIZE: usize = 58;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new()
            .u32(self.id)
            .u16(self.kind)
            .f64x3(self.pos)
            .f32(self.yaw)
            .f32(self.pitch)
            .f32(self.head_yaw)
            .f32x3(self.motion)
            .put(&self.meta.encode())
            .done()
    }

    pub fn decode(b: &[u8]) -> Option<SpawnMob> {
        let mut r = Rd(b);
        let m = SpawnMob {
            id: r.u32()?,
            kind: r.u16()?,
            pos: r.f64x3()?,
            yaw: r.f32()?,
            pitch: r.f32()?,
            head_yaw: r.f32()?,
            motion: r.f32x3()?,
            meta: MobMeta::decode(r.bytes(MobMeta::SIZE)?)?,
        };
        r.end(m)
    }
}

/// One entry of [`S_ENTITY_MOVE`]: `SPacketEntityTeleport` and `SPacketEntityHeadLook`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EntityMove {
    pub id: u32,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    pub on_ground: bool,
}

impl EntityMove {
    pub const SIZE: usize = 41;
}

/// [`S_ENTITY_MOVE`]: a `u16` count, then each [`EntityMove`]. At most 65,535 are sent.
pub fn encode_entity_moves(moves: &[EntityMove]) -> Vec<u8> {
    let n = moves.len().min(u16::MAX as usize);
    let mut w = Wr::new().u16(n as u16);
    for m in &moves[..n] {
        w = w.u32(m.id).f64x3(m.pos).f32(m.yaw).f32(m.pitch).f32(m.head_yaw).u8(m.on_ground as u8);
    }
    w.done()
}

pub fn decode_entity_moves(b: &[u8]) -> Option<Vec<EntityMove>> {
    let mut r = Rd(b);
    let n = r.u16()? as usize;
    if r.0.len() != n * EntityMove::SIZE {
        return None;
    }
    let moves = (0..n)
        .map(|_| Some(EntityMove { id: r.u32()?, pos: r.f64x3()?, yaw: r.f32()?, pitch: r.f32()?, head_yaw: r.f32()?, on_ground: r.bool()? }))
        .collect::<Option<Vec<_>>>()?;
    r.end(moves)
}

/// [`S_VELOCITY`]: `SPacketEntityVelocity`, blocks a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Velocity {
    pub id: u32,
    pub motion: [f32; 3],
}

impl Velocity {
    pub const SIZE: usize = 16;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.id).f32x3(self.motion).done()
    }

    pub fn decode(b: &[u8]) -> Option<Velocity> {
        let mut r = Rd(b);
        let m = Velocity { id: r.u32()?, motion: r.f32x3()? };
        r.end(m)
    }
}

/// [`S_DESTROY`]: a `u16` count, then the entity ids. At most 65,535 are sent.
pub fn encode_destroy(ids: &[u32]) -> Vec<u8> {
    let n = ids.len().min(u16::MAX as usize);
    let mut w = Wr::new().u16(n as u16);
    for &id in &ids[..n] {
        w = w.u32(id);
    }
    w.done()
}

pub fn decode_destroy(b: &[u8]) -> Option<Vec<u32>> {
    let mut r = Rd(b);
    let n = r.u16()? as usize;
    if r.0.len() != n * 4 {
        return None;
    }
    let ids = (0..n).map(|_| r.u32()).collect::<Option<Vec<_>>>()?;
    r.end(ids)
}

/// [`S_ENTITY_EVENT`]: `SPacketEntityStatus`, [`EV_HURT`] and the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntityEvent {
    pub id: u32,
    pub status: u8,
}

impl EntityEvent {
    pub const SIZE: usize = 5;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.id).u8(self.status).done()
    }

    pub fn decode(b: &[u8]) -> Option<EntityEvent> {
        let mut r = Rd(b);
        let m = EntityEvent { id: r.u32()?, status: r.u8()? };
        r.end(m)
    }
}

/// What an [`EntityMeta`] carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaPayload {
    /// Which 0: a mob's [`MobMeta`].
    Mob(MobMeta),
    /// Which 1: an item entity's stack.
    Item(WireStack),
}

impl Default for MetaPayload {
    fn default() -> MetaPayload {
        MetaPayload::Mob(MobMeta::default())
    }
}

/// [`S_ENTITY_META`]: `SPacketEntityMetadata`, as a `u8` which, then the payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntityMeta {
    pub id: u32,
    pub payload: MetaPayload,
}

impl EntityMeta {
    pub fn encode(&self) -> Vec<u8> {
        let w = Wr::new().u32(self.id);
        let w = match self.payload {
            MetaPayload::Mob(m) => w.u8(0).put(&m.encode()),
            MetaPayload::Item(s) => w.u8(1).stack(s),
        };
        w.done()
    }

    pub fn decode(b: &[u8]) -> Option<EntityMeta> {
        let mut r = Rd(b);
        let id = r.u32()?;
        let payload = match r.u8()? {
            0 => MetaPayload::Mob(MobMeta::decode(r.bytes(MobMeta::SIZE)?)?),
            1 => MetaPayload::Item(r.stack()?),
            _ => return None,
        };
        r.end(EntityMeta { id, payload })
    }
}

/// [`S_ANIMATION`]: `SPacketAnimation`, 0 to 5 ([`ANIM_SWING_MAIN`], [`ANIM_CRIT`],
/// [`ANIM_MAGIC_CRIT`] and vanilla's others).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Animation {
    pub id: u32,
    pub animation: u8,
}

impl Animation {
    pub const SIZE: usize = 5;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.id).u8(self.animation).done()
    }

    pub fn decode(b: &[u8]) -> Option<Animation> {
        let mut r = Rd(b);
        let m = Animation { id: r.u32()?, animation: r.below(6)? };
        r.end(m)
    }
}

/// [`S_EQUIPMENT`]: `SPacketEntityEquipment`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Equipment {
    pub id: u32,
    /// [`EQ_MAIN_HAND`] to [`EQ_HEAD`].
    pub slot: u8,
    pub stack: WireStack,
}

impl Equipment {
    pub const SIZE: usize = 10;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.id).u8(self.slot).stack(self.stack).done()
    }

    pub fn decode(b: &[u8]) -> Option<Equipment> {
        let mut r = Rd(b);
        let m = Equipment { id: r.u32()?, slot: r.below(EQ_HEAD + 1)?, stack: r.stack()? };
        r.end(m)
    }
}

/// [`S_COLLECT`]: `SPacketCollectItem`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Collect {
    pub item: u32,
    pub collector: u32,
    pub count: u8,
}

impl Collect {
    pub const SIZE: usize = 9;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.item).u32(self.collector).u8(self.count).done()
    }

    pub fn decode(b: &[u8]) -> Option<Collect> {
        let mut r = Rd(b);
        let m = Collect { item: r.u32()?, collector: r.u32()?, count: r.u8()? };
        r.end(m)
    }
}

/// [`S_BREAK_ANIM`]: `SPacketBlockBreakAnim`, stage 0 to 9, -1 clears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BreakAnim {
    pub breaker: u32,
    pub pos: [i32; 3],
    pub stage: i8,
}

impl BreakAnim {
    pub const SIZE: usize = 17;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().u32(self.breaker).i32x3(self.pos).i8(self.stage).done()
    }

    pub fn decode(b: &[u8]) -> Option<BreakAnim> {
        let mut r = Rd(b);
        let m = BreakAnim { breaker: r.u32()?, pos: r.i32x3()?, stage: r.i8().filter(|s| (-1..=9).contains(s))? };
        r.end(m)
    }
}

/// [`S_BLOCK_ACTION`]: `SPacketBlockAction` (chest lid: `a` 1, `b` the players using it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockAction {
    pub pos: [i32; 3],
    pub block: u16,
    pub a: u8,
    pub b: u8,
}

impl BlockAction {
    pub const SIZE: usize = 16;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().i32x3(self.pos).u16(self.block).u8(self.a).u8(self.b).done()
    }

    pub fn decode(b: &[u8]) -> Option<BlockAction> {
        let mut r = Rd(b);
        let m = BlockAction { pos: r.i32x3()?, block: r.u16()?, a: r.u8()?, b: r.u8()? };
        r.end(m)
    }
}

/// [`S_SOUND`]: `SPacketSoundEffect`, by sound event name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sound {
    pub name: String,
    /// `SoundCategory` ordinal, below [`SOUND_CATEGORIES`].
    pub category: u8,
    pub pos: [f32; 3],
    pub volume: f32,
    pub pitch: f32,
}

impl Sound {
    pub fn encode(&self) -> Vec<u8> {
        Wr::new().str(&self.name).u8(self.category).f32x3(self.pos).f32(self.volume).f32(self.pitch).done()
    }

    pub fn decode(b: &[u8]) -> Option<Sound> {
        let mut r = Rd(b);
        let m = Sound { name: r.str()?, category: r.below(SOUND_CATEGORIES)?, pos: r.f32x3()?, volume: r.f32()?, pitch: r.f32()? };
        r.end(m)
    }
}

/// [`S_EXPLOSION`]: `SPacketExplosion`; `motion` is the push on the receiving player.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Explosion {
    pub pos: [f64; 3],
    pub strength: f32,
    pub motion: [f32; 3],
}

impl Explosion {
    pub const SIZE: usize = 40;

    pub fn encode(&self) -> Vec<u8> {
        Wr::new().f64x3(self.pos).f32(self.strength).f32x3(self.motion).done()
    }

    pub fn decode(b: &[u8]) -> Option<Explosion> {
        let mut r = Rd(b);
        let m = Explosion { pos: r.f64x3()?, strength: r.f32()?, motion: r.f32x3()? };
        r.end(m)
    }
}
