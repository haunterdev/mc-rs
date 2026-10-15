//! Epic Siege's settings as a const table (ESM-1): the defaults of the mod's
//! `config/props/CfgProps.java` and `config/EsmConfigGlobal.java`, one field per key.
//!
//! [`DEFAULT`] is the mod's defaults and the values the addon writes to its settings file on the
//! first start ([`Settings`]); a test that wants one setting on builds a goal with its own
//! [`Config`]. The mod's reload command and the per-mob cap in `ESM.dat` have no port.
//!
//! DROPPED: the reload command, `ESM.dat`, and every key whose system is not
//! ported: the ranged attack (`RANGE_DIST`, `RANGE_ERR`, `ARROW_POT`, `BOW_DMG`), `SP_WEB`,
//! `EN_TELE`, `INFECTIONS`, `WITHER_SKEL`, the creeper keys, `DEMOLITION`, `BOSS_*`, `DEBUG_TARGET`
//! and the anti-griefing keys.

use items::ids::block;

/// One kind's ESM settings. Field names are the config keys of the mod.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// `Awareness Radius`: `EntityMob`'s `FOLLOW_RANGE` is raised to at least this when it joins.
    pub awareness: f64,
    /// `Xray Distance`: a target inside this range is seen with no line of sight.
    pub xray: f64,
    /// `Pathing Cap`: the attackers one target may have at once.
    pub target_cap: usize,
    /// `Attack Everything`
    pub attack_all: bool,
    /// `Attack Pets`
    pub attack_pets: bool,
    /// `Attack Villagers`
    pub attack_villagers: bool,
    /// `Neutral Mob`: a kind that is not hostile keeps its vanilla goal list.
    pub neutral: bool,
    /// `Strafe Evade`
    pub strafe: bool,
    /// `Animal Retaliate`
    pub animal_retaliation: bool,
    /// `Digging`
    pub digging: bool,
    /// `Digging Blacklist`: block registry names that are never dug.
    pub dig_blacklist: &'static [&'static str],
    /// `Invert Blacklist`: dig only what the blacklist names.
    pub dig_blacklist_inverted: bool,
    /// `Requires Tools`: a block that needs a tool is left alone by an empty hand.
    pub dig_requires_tools: bool,
    /// `Pillaring`
    pub pillaring: bool,
    /// `Pillar Block`, `mod:name[:meta]`; the default cobblestone. The mod stores the name and looks
    /// the default state up (`Block.getBlockFromName`), so this is the packed state (`id << 4 | meta`)
    /// and not the block id.
    pub pillar_block: u16,
    /// `Griefing`
    pub griefing: bool,
    /// `Grief Targets`
    pub grief_blocks: &'static [&'static str],
    /// `Health`: `MAX_HEALTH` times this.
    pub mod_health: f64,
    /// `Damage`: `ATTACK_DAMAGE` times this.
    pub mod_damage: f64,
    /// `Speed`: `MOVEMENT_SPEED` times this.
    pub mod_speed: f64,
    /// `Knockback`: `KNOCKBACK_RESISTANCE` times this.
    pub mod_knockback: f64,
}

/// The mod's defaults.
pub const DEFAULT: Config = Config {
    awareness: 64.0,
    xray: 64.0,
    target_cap: 16,
    attack_all: false,
    attack_pets: true,
    attack_villagers: true,
    neutral: false,
    strafe: false,
    animal_retaliation: true,
    digging: false,
    dig_blacklist: &[],
    dig_blacklist_inverted: false,
    dig_requires_tools: true,
    pillaring: false,
    pillar_block: block::COBBLESTONE << 4,
    griefing: false,
    // CfgProps.GRF_DEF, the 25 grief targets.
    grief_blocks: &[
        "minecraft:chest",
        "minecraft:furnace",
        "minecraft:crafting_table",
        "minecraft:melon_stem",
        "minecraft:pumpkin_stem",
        "minecraft:fence_gate",
        "minecraft:melon_block",
        "minecraft:pumpkin",
        "minecraft:glass",
        "minecraft:glass_pane",
        "minecraft:stained_glass",
        "minecraft:stained_glass_pane",
        "minecraft:carrots",
        "minecraft:potatoes",
        "minecraft:brewing_stand",
        "minecraft:enchanting_table",
        "minecraft:cake",
        "minecraft:ladder",
        "minecraft:wooden_door",
        "minecraft:farmland",
        "minecraft:bookshelf",
        "minecraft:sapling",
        "minecraft:bed",
        "minecraft:fence",
        "minecraft:planks",
    ],
    mod_health: 1.0,
    mod_damage: 1.0,
    mod_speed: 1.0,
    mod_knockback: 1.0,
};

/// The settings file `<data>/addons/epic-siege.json`: the keys of the mod's config this port
/// carries, named as the mod names them, with [`DEFAULT`] as the values written on the first start.
/// The zombie is the one kind with a profile; the creeper has its two keys; the hard days are the
/// mod's global `Hardcore Day Cycle`.
///
/// DROPPED: the list keys (`Digging Blacklist`, `Grief Targets`, `Pillar Block`), which stay at the
/// mod's defaults here.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(rename = "Awareness Radius")]
    pub awareness: f64,
    #[serde(rename = "Xray Distance")]
    pub xray: f64,
    #[serde(rename = "Pathing Cap")]
    pub target_cap: usize,
    #[serde(rename = "Strafe Evade")]
    pub strafe: bool,
    #[serde(rename = "Digging")]
    pub digging: bool,
    #[serde(rename = "Requires Tools")]
    pub dig_requires_tools: bool,
    #[serde(rename = "Pillaring")]
    pub pillaring: bool,
    #[serde(rename = "Griefing")]
    pub griefing: bool,
    #[serde(rename = "Health")]
    pub mod_health: f64,
    #[serde(rename = "Damage")]
    pub mod_damage: f64,
    #[serde(rename = "Speed")]
    pub mod_speed: f64,
    /// `Hardcore Day Cycle`: every Nth day is a hard day; 0 turns the hard days off.
    #[serde(rename = "Hardcore Day Cycle")]
    pub hardcore_day_cycle: i64,
    /// `Powered` (`CfgProps.CR_POWERED`): the share of creepers that spawn powered.
    #[serde(rename = "Creeper Powered")]
    pub creeper_powered: f64,
    /// `Breaching` (`CfgProps.CR_BREACH`)
    #[serde(rename = "Creeper Breaching")]
    pub creeper_breaching: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        let d = DEFAULT;
        Settings {
            awareness: d.awareness,
            xray: d.xray,
            target_cap: d.target_cap,
            strafe: d.strafe,
            digging: d.digging,
            dig_requires_tools: d.dig_requires_tools,
            pillaring: d.pillaring,
            griefing: d.griefing,
            mod_health: d.mod_health,
            mod_damage: d.mod_damage,
            mod_speed: d.mod_speed,
            hardcore_day_cycle: super::hardday::HARD_DAY_CYCLE,
            creeper_powered: super::creeper::POWERED_CHANCE,
            creeper_breaching: true,
        }
    }
}

impl Settings {
    /// The zombie's profile: [`DEFAULT`] with these keys over it.
    pub fn zombie(&self) -> Config {
        Config {
            awareness: self.awareness,
            xray: self.xray,
            target_cap: self.target_cap,
            strafe: self.strafe,
            digging: self.digging,
            dig_requires_tools: self.dig_requires_tools,
            pillaring: self.pillaring,
            griefing: self.griefing,
            mod_health: self.mod_health,
            mod_damage: self.mod_damage,
            mod_speed: self.mod_speed,
            ..DEFAULT
        }
    }
}

/// The registry path of a block state's block (`Block.registryName`, the string the mod matches a
/// block list against), `minecraft:` left off so it compares with either spelling.
pub fn block_name(state: u16) -> Option<&'static str> {
    items::registry::item(state >> 4).map(|d| strip(d.name))
}

/// A config list entry (`minecraft:chest`) and a registry path (`chest`) name the same block.
pub fn strip(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}
