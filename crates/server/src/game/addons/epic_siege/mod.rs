//! Epic Siege (ESM 13.169) as a server addon, ported from the decompiled mod at
//! `EpicSiegeMod-13.169-src/funwayguy/epicsiegemod/`. Switched on by `"epic-siege"` in
//! `server.json`'s `addons`; its settings are `<data>/addons/epic-siege.json`
//! ([`config::Settings`]). With it off, no file here runs.
//!
//! | File | Source |
//! |---|---|
//! | `config.rs` | `config/props/CfgProps.java`, `config/EsmConfigGlobal.java` |
//! | `attrs.rs` | `handlers/entities/GeneralEntityHandler.java`, `handlers/MainHandler.java` |
//! | `senses.rs` | `ai/hooks/EntitySensesProxy.java` |
//! | `target.rs` | `ai/ESM_EntityAITarget.java`, `ai/ESM_EntityAINearestAttackableTarget.java` |
//! | `melee.rs` | `ai/ESM_EntityAIAttackMelee.java`, `ai/ESM_EntityAIZombieAttack.java` |
//! | `dig.rs` | `ai/ESM_EntityAIDigging.java`, `ai/utils/AiUtils.java`, `ai/additions/AdditionDigger.java` |
//! | `pillar.rs` | `ai/ESM_EntityAIPillarUp.java`, `ai/additions/AdditionPillaring.java` |
//! | `grief.rs` | `ai/ESM_EntityAIGrief.java`, `ai/additions/AdditionGrief.java` |
//! | `creeper.rs` | `ai/ESM_EntityAICreeperSwell.java`, `handlers/entities/CreeperHandler.java` |
//! | `hardday.rs` | `handlers/entities/PlayerHandler.java` (hard-day spawner) |
//!
//! The mod's `handlers/MainHandler.java` walks a mob's task lists after `initEntityAI` and hands
//! every goal to the registered task modifiers and additions; a modifier replaces the vanilla goal
//! it claims, an addition is a new goal of its own. [`apply_with`] is that pass, run from the
//! addon's `mob_init` hook.
//!
//! DROPPED: every modifier and addition whose system this port does not carry (`ModifierBowAttack`,
//! `ModifierRangedAttack`, `ModifierSwimming`, `ModifierWander`, `ModifierNoPanic`,
//! `ModifierVillagerAvoid`, `AdditionAnimalAttack`, `AdditionAnimalRetaliate`,
//! `AdditionAvoidExplosives`, `AdditionDemolition`), the `Neutral Mob` setting (the port's one
//! profile is the zombie, which is hostile), `Attack Everything`'s mob targets, and the mod's
//! `ModifiedHandler` capability: `Mob` carries its `Attrs` directly.

use crate::game::mobs::ai::{melee as vanilla_melee, target as vanilla_target};
use crate::game::mobs::Mob;
use crate::game::Game;

pub mod attrs;
pub mod config;
pub mod creeper;
pub mod dig;
pub mod grief;
pub mod hardday;
pub mod melee;
pub mod pillar;
pub mod senses;
pub mod target;
#[cfg(test)]
mod tests;

use super::Addon;

pub const ADDON: Addon = Addon { name: "epic-siege", mob_init, on_spawn, tick };

/// What the addon keeps: its settings and the hard-day spawner's state.
pub struct State {
    pub settings: config::Settings,
    pub hardday: hardday::State,
}

impl Default for State {
    fn default() -> State {
        State::new(config::Settings::default())
    }
}

impl State {
    pub fn new(settings: config::Settings) -> State {
        let hardday = hardday::State { hard_days: true, cycle: settings.hardcore_day_cycle };
        State { settings, hardday }
    }
}

/// `MainHandler`'s task pass for the kinds this port carries: the zombie's profile, and the
/// creeper's swell goal (`ModifierCreeperSwell`).
fn mob_init(game: &mut Game, m: &mut Mob) {
    let s = &game.addons.epic_siege.settings;
    match m.def.kind.name {
        "zombie" => apply_with(m, &s.zombie()),
        "creeper" => creeper::replace_swell(m, s.creeper_breaching),
        _ => {}
    }
}

/// `EntityJoinWorldEvent` rolls that need the generator: the creeper's powered roll
/// (`CreeperHandler.onSpawn`) and `AdditionDigger.getAdditionalAI`'s iron pickaxe.
fn on_spawn(game: &mut Game, m: &mut Mob) {
    let s = &game.addons.epic_siege.settings;
    match m.def.kind.name {
        "creeper" => creeper::on_spawn(m, &mut game.mobs.rand, s.creeper_powered),
        "zombie" => {
            let cfg = s.zombie();
            equip(m, &cfg, &mut game.mobs.rand);
        }
        _ => {}
    }
}

fn tick(game: &mut Game) {
    hardday::step(game);
}

/// The task pass and the attribute pass, with the settings in hand (a test turns one on).
pub fn apply_with(m: &mut Mob, cfg: &config::Config) {
    attrs::apply(m, cfg);
    // ModifierAttackMelee / ModifierZombieAttack: the vanilla melee goal in place becomes the ESM
    // one, at the same priority. The mod reads the vanilla `speedTowardsTarget` reflectively; every
    // ported kind builds its melee goal with 1.0, which is what the replacement carries.
    for e in m.tasks.entries.iter_mut() {
        if e.action.as_any().is::<vanilla_melee::AttackMelee>() {
            e.action = Box::new(melee::EsAttackMelee::new(cfg, 1.0, true));
        } else if e.action.as_any().is::<vanilla_melee::ZombieAttack>() {
            e.action = Box::new(melee::EsZombieAttack::new(cfg, 1.0, true));
        }
    }
    // ModifierNearestAttackable: the vanilla nearest-attackable goal becomes the ESM one.
    for e in m.target_tasks.entries.iter_mut() {
        if e.action.as_any().is::<vanilla_target::NearestAttackableTarget>() {
            e.action = Box::new(target::EsNearestAttackableTarget::new(cfg));
        }
    }
    // The additions, in the order `CommonProxy.registerTasks` registers them (the digger, the
    // demolition which has no port, the pillaring, the griefing), each at task priority 4.
    if cfg.digging {
        m.tasks.add_task(4, dig::Digging::new(cfg));
    }
    if cfg.pillaring {
        m.tasks.add_task(4, pillar::PillarUp::new(cfg));
    }
    if cfg.griefing {
        m.tasks.add_task(4, grief::Grief::new(cfg));
    }
}

/// `AdditionDigger.getAdditionalAI`: the one-time roll for an iron pickaxe in the main hand, which
/// a digging mob needs for a block that requires a tool. Nothing is rolled unless `digging` is on,
/// so the default settings add no draw to a zombie's stream.
pub fn equip(m: &mut Mob, cfg: &config::Config, rand: &mut worldgen::java_random::JavaRandom) {
    if !cfg.digging || !m.main_hand.is_empty() {
        return;
    }
    if rand.next_int_bounded(20) == 0 {
        m.main_hand = items::ItemStack::of(items::ids::item::IRON_PICKAXE);
    }
}
