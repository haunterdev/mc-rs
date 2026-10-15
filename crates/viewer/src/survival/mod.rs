//! Survival in the viewer: the server owns health, inventories,
//! entities and windows, and this module shows them and sends the player's actions.
//!
//! Every survival frame from the server (S_ 160..219) reaches link.rs's `receive`, which leaves
//! it in the [`Inbox`]; [`net::dispatch`] decodes it into one typed Bevy message per frame kind
//! ([`Health`] and the rest below), and the files that later tasks fill read those messages. The
//! send helpers for the V_ messages are in `net.rs`.
//!
//! The game mode stays `Player::game_mode`, set by `S_GAMEMODE` in teleport.rs; there is no
//! second copy here.
//!
//! Survival runs only with a server (streamed or native). The viewer alone keeps its creative
//! inventory, and this plugin then has nothing to read.

use bevy::prelude::*;

pub mod arrows;
pub mod xp_orbs;
mod attack;
pub mod crit;
pub mod chest_render;
pub mod crack;
pub mod death;
pub mod effects;
pub mod explosion;
mod falling;
pub mod gui;
mod hud;
pub mod hurt;
mod items;
pub mod item_use;
mod jukebox;
pub mod mining;
pub(crate) mod mobs;
mod sound;
pub mod net;
pub mod overlay;

pub struct SurvivalPlugin;

impl Plugin for SurvivalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inbox>()
            .init_resource::<SurvivalState>()
            .add_message::<Health>()
            .add_message::<Xp>()
            .add_message::<Death>()
            .add_message::<Held>()
            .add_message::<Slots>()
            .add_message::<Confirm>()
            .add_message::<OpenWindow>()
            .add_message::<CloseWindow>()
            .add_message::<WindowProp>()
            .add_message::<SpawnObject>()
            .add_message::<SpawnMob>()
            .add_message::<EntityMoves>()
            .add_message::<Velocity>()
            .add_message::<Destroy>()
            .add_message::<EntityEvent>()
            .add_message::<EntityMeta>()
            .add_message::<Animation>()
            .add_message::<Equipment>()
            .add_message::<Collect>()
            .add_message::<BreakAnim>()
            .add_message::<BlockAction>()
            .add_message::<Sound>()
            .add_message::<Explosion>()
            .add_message::<EffectIn>()
            .add_message::<EffectOut>()
            .add_message::<WorldEvent>()
            .add_systems(PreUpdate, (net::dispatch, apply_state).chain());
        overlay::plugin(app);
        hud::plugin(app);
        death::plugin(app);
        effects::plugin(app);
        hurt::plugin(app);
        items::plugin(app);
        sound::plugin(app);
        mining::plugin(app);
        crack::plugin(app);
        gui::plugin(app);
        mobs::plugin(app);
        attack::plugin(app);
        crit::plugin(app);
        chest_render::plugin(app);
        falling::plugin(app);
        arrows::plugin(app);
        xp_orbs::plugin(app);
        explosion::plugin(app);
        item_use::plugin(app);
        jukebox::plugin(app);
    }
}

/// Survival frames link.rs received this frame, waiting for [`net::dispatch`].
#[derive(Resource, Default)]
pub struct Inbox(pub Vec<(u8, Vec<u8>)>);

/// The session player's survival stats as the server last sent them.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct SurvivalState {
    pub health: f32,
    pub food: u8,
    pub saturation: f32,
    pub air: i16,
    pub burning: bool,
    pub xp: proto::Xp,
    /// Set by `S_DEATH`, cleared by the next `S_HEALTH` with health above 0 (the server sends
    /// one after a respawn, as `EntityPlayerMP` does with `SPacketUpdateHealth`).
    pub dead: bool,
}

impl Default for SurvivalState {
    /// A new player: `EntityLivingBase.getMaxHealth` 20, `FoodStats` 20 and 5.0, `Entity`
    /// air 300, no experience.
    fn default() -> SurvivalState {
        SurvivalState { health: 20.0, food: 20, saturation: 5.0, air: 300, burning: false, xp: proto::Xp::default(), dead: false }
    }
}

// One message per S_ frame. The later tasks read them (T4 the stats and death, T5 items, T6 the
// break animation, T7 and T8 windows, T10 entities), so until then most are only written.
#[allow(dead_code)]
mod messages {
    use bevy::prelude::*;

    /// `S_HEALTH`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Health(pub proto::Health);
    /// `S_XP`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Xp(pub proto::Xp);
    /// `S_DEATH`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Death(pub proto::Death);
    /// `S_HELD`: the server moved the hotbar selection.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Held(pub proto::HeldSlot);
    /// `S_SLOTS`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Slots(pub proto::WindowSlots);
    /// `S_CONFIRM`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Confirm(pub proto::Confirm);
    /// `S_OPEN_WINDOW`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct OpenWindow(pub proto::OpenWindow);
    /// `S_CLOSE_WINDOW`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct CloseWindow(pub proto::CloseWindow);
    /// `S_WINDOW_PROP`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct WindowProp(pub proto::WindowProp);
    /// `S_SPAWN_OBJECT`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct SpawnObject(pub proto::SpawnObject);
    /// `S_SPAWN_MOB`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct SpawnMob(pub proto::SpawnMob);
    /// `S_ENTITY_MOVE`: one tick's moves, batched.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct EntityMoves(pub Vec<proto::EntityMove>);
    /// `S_VELOCITY`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Velocity(pub proto::Velocity);
    /// `S_DESTROY`: the entity ids gone.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Destroy(pub Vec<u32>);
    /// `S_ENTITY_EVENT`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct EntityEvent(pub proto::EntityEvent);
    /// `S_ENTITY_META`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct EntityMeta(pub proto::EntityMeta);
    /// `S_ANIMATION`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Animation(pub proto::Animation);
    /// `S_EQUIPMENT`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Equipment(pub proto::Equipment);
    /// `S_COLLECT`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Collect(pub proto::Collect);
    /// `S_BREAK_ANIM`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct BreakAnim(pub proto::BreakAnim);
    /// `S_BLOCK_ACTION`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct BlockAction(pub proto::BlockAction);
    /// `S_SOUND`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Sound(pub proto::Sound);
    /// `S_EXPLOSION`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct Explosion(pub proto::Explosion);
    /// `S_EFFECT`: `SPacketEntityEffect`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct EffectIn(pub proto::EntityEffect);
    /// `S_REMOVE_EFFECT`: `SPacketRemoveEntityEffect`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct EffectOut(pub proto::RemoveEffect);
    /// `S_WORLD_EVENT`: `SPacketEffect`.
    #[derive(Message, Clone, Debug, PartialEq)]
    pub struct WorldEvent(pub proto::WorldEvent);
}
pub use messages::*;
// Bevy's prelude has an `EntityEvent` trait; this module's name is the message.
pub use messages::EntityEvent;

/// Keeps [`SurvivalState`] to the server's `S_HEALTH`, `S_XP` and `S_DEATH`.
fn apply_state(mut state: ResMut<SurvivalState>, mut health: MessageReader<Health>, mut xp: MessageReader<Xp>, mut death: MessageReader<Death>) {
    for Health(h) in health.read() {
        state.health = h.health;
        state.food = h.food;
        state.saturation = h.saturation;
        state.air = h.air;
        state.burning = h.flags & proto::HEALTH_BURNING != 0;
        if h.health > 0.0 {
            state.dead = false;
        }
    }
    if let Some(Xp(x)) = xp.read().last() {
        state.xp = *x;
    }
    if death.read().next().is_some() {
        state.dead = true;
    }
}
