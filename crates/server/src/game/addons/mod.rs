//! Addons: gameplay layers the server owner turns on or off, so the base game stays vanilla 1.12.2.
//!
//! `server.json` `"addons": ["epic-siege"]` names the addons that run (`Config::addons`); with the
//! list empty every mob behaves as vanilla. An addon is an [`Addon`] row in [`ADDONS`] holding its
//! hook functions, and the base game calls through [`mob_init`], [`on_spawn`] and [`tick`] only:
//! no base file carries an addon's logic. Each addon's settings load from
//! `<data>/addons/<name>.json`, written with the addon's defaults on the first start when the file
//! is missing.
//!
//! To add an addon: a folder here, an `ADDON` const, a row in [`ADDONS`] and a field in [`State`].

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::mobs::Mob;
use super::Game;
use crate::config::Config;

pub mod epic_siege;

/// One addon's hooks. Every hook runs only while the addon is [`enabled`].
pub struct Addon {
    /// The name in `server.json` and the settings file's name.
    pub name: &'static str,
    /// Where a mob's goal and target tasks have just been built (`Mob::init_entity_ai`): the
    /// addon edits the lists in place. Also runs for a mob loaded from a save.
    pub mob_init: fn(&mut Game, &mut Mob),
    /// A mob has joined the world (`EntityJoinWorldEvent`), after `mob_init`, before its first
    /// tick: the rolls that need the mobs' generator.
    pub on_spawn: fn(&mut Game, &mut Mob),
    /// Once a tick, beside the natural spawner.
    pub tick: fn(&mut Game),
}

/// Every addon this build carries.
pub static ADDONS: &[Addon] = &[epic_siege::ADDON];

/// The addons switched on, and each one's loaded settings and running state.
#[derive(Default)]
pub struct State {
    pub enabled: Vec<String>,
    pub epic_siege: epic_siege::State,
}

impl State {
    /// `server.json`'s list, then each enabled addon's settings file under `<dir>/addons/`.
    pub fn load(cfg: &Config, dir: &Path) -> State {
        let enabled: Vec<String> = cfg.addons.iter().map(|n| n.trim().to_ascii_lowercase()).collect();
        let on = |name: &str| enabled.iter().any(|n| n == name);
        let epic_siege = if on(epic_siege::ADDON.name) {
            epic_siege::State::new(load_settings(dir, epic_siege::ADDON.name))
        } else {
            epic_siege::State::default()
        };
        State { enabled, epic_siege }
    }
}

/// Whether the addon `name` is switched on in `server.json`.
pub fn enabled(game: &Game, name: &str) -> bool {
    game.addons.enabled.iter().any(|n| n == name)
}

/// `<dir>/addons/<name>.json` as `T`, written with `T::default()` when missing. A file that does
/// not parse is reported and the defaults run, the file left for its owner to fix.
pub fn load_settings<T: Serialize + DeserializeOwned + Default>(dir: &Path, name: &str) -> T {
    let path = dir.join("addons").join(format!("{name}.json"));
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            println!("addons: {} does not parse ({e}), running the defaults", path.display());
            T::default()
        }),
        Err(_) => {
            let defaults = T::default();
            if let Ok(text) = serde_json::to_string_pretty(&defaults) {
                let _ = std::fs::create_dir_all(path.parent().unwrap_or(dir));
                let _ = std::fs::write(&path, text + "\n");
            }
            defaults
        }
    }
}

/// `mob_init` of every enabled addon.
pub fn mob_init(game: &mut Game, m: &mut Mob) {
    for a in ADDONS {
        if enabled(game, a.name) {
            (a.mob_init)(game, m);
        }
    }
}

/// `on_spawn` of every enabled addon.
pub fn on_spawn(game: &mut Game, m: &mut Mob) {
    for a in ADDONS {
        if enabled(game, a.name) {
            (a.on_spawn)(game, m);
        }
    }
}

/// `tick` of every enabled addon.
pub fn tick(game: &mut Game) {
    for a in ADDONS {
        if enabled(game, a.name) {
            (a.tick)(game);
        }
    }
}
