//! `DamageSource`, `attackEntityFrom` and `damageEntity` for a player, `CombatRules`, the combat
//! tracker's death messages, death and respawn (`EntityPlayerMP.onDeath`,
//! `PlayerList.recreatePlayerEntity`).
//!
//! DROPPED across the file: shields, the anvil helmet wear, totems, the scoreboard
//! and statistics, teams (every death message goes to everyone), the held-item variants of the
//! death messages (`.item`), the `SPacketEntityVelocity` a plain hurt sends (this server does
//! not simulate the player's motion, so it only sends the knockback) and experience orbs.

use proto::{ON_GROUND, SNEAKING};
use serde_json::{json, Value};
use sim::lang::{Arg, Lang};

use super::players::{self, MAX_HEALTH, RESPAWN_INVULNERABLE};
use super::{net, terrain, Game};

/// The entity behind a damage source (`EntityDamageSource`'s `damageSourceEntity`).
#[derive(Clone, Debug, PartialEq)]
pub struct Attacker {
    pub name: String,
    pub pos: [f64; 3],
    /// An `EntityPlayer`: `pvp` decides whether it can hurt another player.
    pub player: bool,
    /// The attacker's entity id (a session id for a player), when it has one.
    pub id: Option<u32>,
}

/// `net.minecraft.util.DamageSource`
#[derive(Clone, Debug, PartialEq)]
pub struct DamageSource {
    pub kind: &'static str,
    pub unblockable: bool,
    pub hunger_damage: f32,
    pub absolute: bool,
    pub fire: bool,
    pub difficulty_scaled: bool,
    pub creative_ok: bool,
    /// `setProjectile`: an arrow or another thrown thing.
    pub projectile: bool,
    pub attacker: Option<Attacker>,
}

impl DamageSource {
    fn new(kind: &'static str) -> DamageSource {
        DamageSource { kind, unblockable: false, hunger_damage: 0.1, absolute: false, fire: false, difficulty_scaled: false, creative_ok: false, projectile: false, attacker: None }
    }

    /// `setDamageBypassesArmor`
    fn bypass_armor(mut self) -> DamageSource {
        self.unblockable = true;
        self.hunger_damage = 0.0;
        self
    }

    fn fire(mut self) -> DamageSource {
        self.fire = true;
        self
    }

    pub fn in_fire() -> DamageSource {
        DamageSource::new("inFire").fire()
    }
    pub fn on_fire() -> DamageSource {
        DamageSource::new("onFire").bypass_armor().fire()
    }
    pub fn lava() -> DamageSource {
        DamageSource::new("lava").fire()
    }
    pub fn in_wall() -> DamageSource {
        DamageSource::new("inWall").bypass_armor()
    }
    pub fn drown() -> DamageSource {
        DamageSource::new("drown").bypass_armor()
    }
    pub fn starve() -> DamageSource {
        let mut s = DamageSource::new("starve").bypass_armor();
        s.absolute = true;
        s
    }
    pub fn fall() -> DamageSource {
        DamageSource::new("fall").bypass_armor()
    }
    pub fn out_of_world() -> DamageSource {
        let mut s = DamageSource::new("outOfWorld").bypass_armor();
        s.creative_ok = true;
        s
    }
    pub fn generic() -> DamageSource {
        DamageSource::new("generic").bypass_armor()
    }
    /// `DamageSource.MAGIC`: poison and instant damage.
    pub fn magic() -> DamageSource {
        DamageSource::new("magic").bypass_armor()
    }
    /// `DamageSource.WITHER`
    pub fn wither() -> DamageSource {
        DamageSource::new("wither").bypass_armor()
    }
    pub fn cactus() -> DamageSource {
        DamageSource::new("cactus")
    }
    /// `causeMobDamage`
    pub fn mob(attacker: Attacker) -> DamageSource {
        let mut s = DamageSource::new("mob");
        s.difficulty_scaled = true;
        s.attacker = Some(attacker);
        s
    }
    /// `causeArrowDamage(arrow, shooter)`: an `EntityDamageSourceIndirect` named "arrow" with
    /// `setProjectile`. The attacker is the shooter (`getTrueSource`), if it is still around.
    pub fn arrow(attacker: Option<Attacker>) -> DamageSource {
        let mut s = DamageSource::new("arrow");
        s.projectile = true;
        s.attacker = attacker;
        s
    }
    /// `causeThornsDamage(source)`: an `EntityDamageSource` named "thorns", magic (bypasses armor).
    pub fn thorns(attacker: Attacker) -> DamageSource {
        let mut s = DamageSource::new("thorns").bypass_armor();
        s.attacker = Some(attacker);
        s
    }
    /// `causePlayerDamage`
    pub fn player(attacker: Attacker) -> DamageSource {
        let mut s = DamageSource::new("player");
        s.attacker = Some(Attacker { player: true, ..attacker });
        s
    }
}

/// `CombatRules.getDamageAfterAbsorb`
pub fn damage_after_absorb(damage: f32, total_armor: f32, toughness: f32) -> f32 {
    let f = 2.0 + toughness / 4.0;
    let f1 = (total_armor - damage / f).clamp(total_armor * 0.2, 20.0);
    damage * (1.0 - f1 / 25.0)
}

/// `CombatEntry`
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: &'static str,
    pub attacker: Option<String>,
    pub damage: f32,
    pub fall_distance: f32,
    pub fall_suffix: Option<&'static str>,
}

impl Entry {
    /// `getDamageAmount`
    fn damage_amount(&self) -> f32 {
        if self.kind == "outOfWorld" {
            f32::MAX
        } else {
            self.fall_distance
        }
    }
}

/// `CombatTracker`: the damage entries a death message is made from.
#[derive(Clone, Debug, Default)]
pub struct Combat {
    entries: Vec<Entry>,
    last_damage_time: i32,
    taking_damage: bool,
    in_combat: bool,
}

impl Combat {
    /// `reset`
    pub fn reset(&mut self, ticks: i32, alive: bool) {
        let i = if self.in_combat { 300 } else { 100 };
        if self.taking_damage && (!alive || ticks - self.last_damage_time > i) {
            self.taking_damage = false;
            self.in_combat = false;
            self.entries.clear();
        }
    }

    /// `trackDamage`
    fn track(&mut self, src: &DamageSource, damage: f32, fall_distance: f32, suffix: Option<&'static str>, ticks: i32, alive: bool) {
        self.reset(ticks, alive);
        let attacker = src.attacker.as_ref().map(|a| a.name.clone());
        self.entries.push(Entry { kind: src.kind, attacker: attacker.clone(), damage, fall_distance, fall_suffix: suffix });
        self.last_damage_time = ticks;
        self.taking_damage = true;
        if attacker.is_some() && !self.in_combat && alive {
            self.in_combat = true;
        }
    }

    /// `getBestCombatEntry`
    fn best(&self) -> Option<&Entry> {
        let (mut best, mut best1): (Option<&Entry>, Option<&Entry>) = (None, None);
        let (mut f, mut f1) = (0.0f32, 0.0f32);
        for (i, e) in self.entries.iter().enumerate() {
            let prev = if i > 0 { Some(&self.entries[i - 1]) } else { None };
            if (e.kind == "fall" || e.kind == "outOfWorld") && e.damage_amount() > 0.0 && (best.is_none() || e.damage_amount() > f1) {
                best = if i > 0 { prev } else { Some(e) };
                f1 = e.damage_amount();
            }
            if e.fall_suffix.is_some() && (best1.is_none() || e.damage > f) {
                best1 = Some(e);
                f = e.damage;
            }
        }
        if f1 > 5.0 && best.is_some() {
            best
        } else if f > 5.0 && best1.is_some() {
            best1
        } else {
            None
        }
    }
}

#[derive(Default)]
pub struct State {
    /// `assets/minecraft/lang/en_us.lang` from the client jar, read on the first death.
    lang: Option<Option<Lang>>,
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    if tag == proto::V_RESPAWN && proto::decode_empty(body).is_some() {
        respawn(game, id);
        return true;
    }
    false
}

pub fn tick(_game: &mut Game) {}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

/// `/kill` on a session player: `EntityLivingBase.onKillCommand`.
pub fn on_kill(game: &mut Game, id: u32) {
    attack_entity_from(game, id, &DamageSource::out_of_world(), f32::MAX);
}

/// `SPacketEntityStatus` for the player: to the player and to everyone near.
fn entity_state(game: &Game, id: u32, status: u8) {
    let body = proto::EntityEvent { id, status }.encode();
    net::send(game, id, proto::S_ENTITY_EVENT, &body);
    if let Some(st) = game.sessions.get(&id).and_then(|s| s.state) {
        net::send_near(game, Some(id), st.pos, 64.0, proto::S_ENTITY_EVENT, &body);
    }
}

/// `EntityPlayerMP.attackEntityFrom`, `EntityPlayer.attackEntityFrom` and
/// `EntityLivingBase.attackEntityFrom`. True when the damage landed.
pub fn attack_entity_from(game: &mut Game, id: u32, src: &DamageSource, mut amount: f32) -> bool {
    let Some(s) = game.sessions.get(&id) else { return false };
    let mode = s.mode;
    let st = s.state;
    let Some(p) = game.players.players.get(&id) else { return false };
    // EntityPlayerMP: respawn protection, except for void damage (and fall damage with pvp on).
    let fall_with_pvp = game.cfg.pvp && src.kind == "fall";
    if !fall_with_pvp && p.respawn_invulnerable > 0 && src.kind != "outOfWorld" {
        return false;
    }
    if let Some(a) = &src.attacker {
        // canAttackPlayer
        if a.player && !game.cfg.pvp {
            return false;
        }
    }
    // EntityPlayer
    if players::disable_damage(mode) && !src.creative_ok {
        return false;
    }
    if p.health <= 0.0 {
        return false;
    }
    // EntityLivingBase: fire damage does nothing to a player with Fire Resistance.
    if src.fire && p.effects.is_active(items::potion::Effect::FireResistance) {
        return false;
    }
    if src.difficulty_scaled {
        match players::difficulty(game) {
            super::food::Difficulty::Peaceful => amount = 0.0,
            super::food::Difficulty::Easy => amount = amount.min(amount / 2.0 + 1.0),
            super::food::Difficulty::Hard => amount = amount * 3.0 / 2.0,
            super::food::Difficulty::Normal => {}
        }
    }
    if amount == 0.0 {
        return false;
    }
    // EntityLivingBase: the 20 tick hurt window lets only a stronger hit through.
    let (resistant, last) = (p.hurt_resistant, p.last_damage);
    let fresh;
    if resistant as f32 > 20.0 / 2.0 {
        if amount <= last {
            return false;
        }
        damage_entity(game, id, src, amount - last);
        game.players.players.get_mut(&id).unwrap().last_damage = amount;
        fresh = false;
    } else {
        let p = game.players.players.get_mut(&id).unwrap();
        p.last_damage = amount;
        p.hurt_resistant = 20;
        damage_entity(game, id, src, amount);
        fresh = true;
    }
    if fresh {
        let status = if src.kind == "drown" {
            proto::EV_HURT_DROWN
        } else if src.fire {
            proto::EV_HURT_BURN
        } else {
            proto::EV_HURT
        };
        entity_state(game, id, status);
        // EntityLivingBase.attackEntityFrom plays getHurtSound or getDeathSound; EntityPlayer's
        // playSound skips the player itself, whose viewer plays it from the status.
        if let Some(st) = game.sessions.get(&id).and_then(|s| s.state) {
            let dead = game.players.players.get(&id).is_some_and(|p| p.health <= 0.0);
            let name = if dead {
                "entity.player.death"
            } else {
                match status {
                    proto::EV_HURT_DROWN => "entity.player.hurt_drown",
                    proto::EV_HURT_BURN => "entity.player.hurt_on_fire",
                    _ => "entity.player.hurt",
                }
            };
            let pitch = super::mobs::sounds::sound_pitch(false, &mut game.mobs.rand);
            super::mobs::sounds::play_sound_except(game, Some(id), st.pos, name, super::mobs::sounds::PLAYERS, 1.0, pitch);
        }
        if let Some(a) = &src.attacker {
            knock_back(game, id, a, 0.4);
        }
    } else if let Some(a) = &src.attacker {
        knock_back(game, id, a, 0.4);
    }
    if game.players.players.get(&id).is_some_and(|p| p.health <= 0.0) {
        on_death(game, id);
    }
    let _ = st;
    true
}

/// `EntityLivingBase.attackEntityFrom`'s attacker branch and `knockBack`, from a standstill: the
/// server keeps no motion for the player, so the push is what the hit alone gives.
fn knock_back(game: &mut Game, id: u32, a: &Attacker, strength: f32) {
    let Some(st) = game.sessions.get(&id).and_then(|s| s.state) else { return };
    let mut d1 = a.pos[0] - st.pos[0];
    let mut d0 = a.pos[2] - st.pos[2];
    while d1 * d1 + d0 * d0 < 1.0e-4 {
        let r = &mut game.players.rand;
        d1 = (r.next_double() - r.next_double()) * 0.01;
        d0 = (r.next_double() - r.next_double()) * 0.01;
    }
    let f = ((d1 * d1 + d0 * d0).sqrt()) as f32;
    let mx = -(d1 / f as f64) * strength as f64;
    let mz = -(d0 / f as f64) * strength as f64;
    let my = if st.flags & ON_GROUND != 0 { (strength as f64).min(0.400_000_005_960_464_5) } else { 0.0 };
    let v = proto::Velocity { id, motion: [mx as f32, my as f32, mz as f32] };
    net::send(game, id, proto::S_VELOCITY, &v.encode());
}

/// `EntityPlayer.damageEntity`
fn damage_entity(game: &mut Game, id: u32, src: &DamageSource, mut amount: f32) {
    // applyArmorCalculations: the worn pieces wear and soak up their share (armor.rs).
    if !src.unblockable {
        amount = super::armor::apply_armor_calculations(game, id, amount);
    }
    // applyPotionDamageCalculations: Resistance, then the enchantment protection total.
    if !src.absolute && src.kind != "outOfWorld" {
        if let Some(a) = game.players.players.get(&id).and_then(|p| p.effects.amplifier(items::potion::Effect::Resistance)) {
            let j = 25 - (a + 1) * 5;
            amount = (amount * j as f32 / 25.0).max(0.0);
        }
    }
    if !src.absolute && amount > 0.0 {
        let flags = items::enchant::DamageFlags {
            can_harm_in_creative: src.creative_ok,
            fire: src.fire,
            fall: src.kind == "fall",
            explosion: src.kind.starts_with("explosion"),
            projectile: src.projectile,
        };
        let k = game.players.players.get(&id).map_or(0, |p| items::enchant::get_enchantment_modifier_damage(p.inv.armor.iter(), &flags));
        if k > 0 {
            amount = items::enchant::damage_after_magic_absorb(amount, k);
        }
    }
    // Absorption soaks up the damage first.
    if let Some(p) = game.players.players.get_mut(&id) {
        let before = amount;
        amount = (amount - p.absorption).max(0.0);
        p.absorption -= before - amount;
    }
    if amount == 0.0 {
        return;
    }
    players::add_exhaustion(game, id, src.hunger_damage);
    let mode = players::mode(game, id);
    let pos = game.sessions.get(&id).and_then(|s| s.state).map(|s| s.pos);
    // CombatTracker.calculateFallSuffix, before the health changes.
    let suffix = match pos {
        Some(p) => match players::ladder_block(game, p, mode) {
            Some(65) => Some("ladder"),
            Some(_) => Some("vines"),
            None if game.players.players.get(&id).is_some_and(|p| p.in_water) => Some("water"),
            None => None,
        },
        None => None,
    };
    let Some(p) = game.players.players.get_mut(&id) else { return };
    let before = p.health;
    p.set_health(before - amount);
    let (fall, ticks, alive) = (p.fall_distance, p.ticks_existed, p.health > 0.0);
    // `isEntityAlive` is read before the health drops in trackDamage's caller chain: damageEntity
    // tracks after setHealth, so the fighter is already dead when the damage was lethal.
    p.combat.track(src, amount, fall, suffix, ticks, alive);
}

// ---- translation ----

/// The `death.*` lines of `en_us.lang`, for a server with no client jar to read.
fn fallback(key: &str) -> Option<&'static str> {
    Some(match key {
        "death.attack.generic" => "%1$s died",
        "death.attack.magic" => "%1$s was killed by magic",
        "death.attack.wither" => "%1$s withered away",
        "death.attack.inFire" => "%1$s went up in flames",
        "death.attack.onFire" => "%1$s burned to death",
        "death.attack.lava" => "%1$s tried to swim in lava",
        "death.attack.inWall" => "%1$s suffocated in a wall",
        "death.attack.drown" => "%1$s drowned",
        "death.attack.starve" => "%1$s starved to death",
        "death.attack.cactus" => "%1$s was pricked to death",
        "death.attack.fall" => "%1$s hit the ground too hard",
        "death.attack.outOfWorld" => "%1$s fell out of the world",
        "death.attack.mob" => "%1$s was slain by %2$s",
        "death.attack.player" => "%1$s was slain by %2$s",
        "death.fell.accident.generic" => "%1$s fell from a high place",
        "death.fell.accident.ladder" => "%1$s fell off a ladder",
        "death.fell.accident.vines" => "%1$s fell off some vines",
        "death.fell.accident.water" => "%1$s fell out of the water",
        "death.fell.assist" => "%1$s was doomed to fall by %2$s",
        "death.fell.finish" => "%1$s fell too far and was finished by %2$s",
        "death.fell.killer" => "%1$s was doomed to fall",
        _ => return None,
    })
}

/// The client jar's `en_us.lang`, once. Tests use the built-in lines.
fn load_lang(game: &mut Game) -> Option<&Lang> {
    if game.damage.lang.is_none() {
        #[cfg(not(test))]
        let lang = {
            let jar = if game.cfg.jar.is_empty() { jar::Jar::find() } else { jar::Jar::open(std::path::Path::new(&game.cfg.jar)) };
            jar.ok().and_then(|j| j.asset("lang/en_us.lang").ok()).map(|b| Lang::from_bytes(&b))
        };
        #[cfg(test)]
        let lang = None;
        game.damage.lang = Some(lang);
    }
    game.damage.lang.as_ref().and_then(|l| l.as_ref())
}

fn tr(game: &mut Game, key: &str, args: &[Arg]) -> String {
    if let Some(l) = load_lang(game) {
        if l.has_key(key) {
            return l.format(key, args);
        }
    }
    match fallback(key) {
        Some(f) => sim::lang::format(f, args).unwrap_or_else(|| f.to_string()),
        None => key.to_string(),
    }
}

/// `CombatTracker.getDeathMessage`
pub fn death_message(game: &mut Game, id: u32) -> String {
    let name = game.sessions.get(&id).map(|s| s.name.clone()).unwrap_or_default();
    let me = Arg::Str(name);
    let Some(p) = game.players.players.get(&id) else { return String::new() };
    let entries = p.combat.entries.clone();
    let best = p.combat.best().cloned();
    let Some(last) = entries.last().cloned() else {
        return tr(game, "death.attack.generic", &[me]);
    };
    if let (Some(best), "fall") = (&best, last.kind) {
        if best.kind != "fall" && best.kind != "outOfWorld" {
            // DROPPED: the `.item` variants for a named held item.
            return match (&best.attacker, &last.attacker) {
                (Some(b), l) if l.as_ref() != Some(b) => tr(game, "death.fell.assist", &[me, Arg::Str(b.clone())]),
                (_, Some(l)) => tr(game, "death.fell.finish", &[me, Arg::Str(l.clone())]),
                _ => tr(game, "death.fell.killer", &[me]),
            };
        }
        let key = format!("death.fell.accident.{}", best.fall_suffix.unwrap_or("generic"));
        return tr(game, &key, &[me]);
    }
    // DamageSource.getDeathMessage / EntityDamageSource.getDeathMessage
    let key = format!("death.attack.{}", last.kind);
    match &last.attacker {
        Some(a) => tr(game, &key, &[me, Arg::Str(a.clone())]),
        None => tr(game, &key, &[me]),
    }
}

// ---- death and respawn ----

/// `EntityPlayerMP.onDeath`: the death screen and message, the inventory on the ground, the
/// fire out. The player stays where they died until `V_RESPAWN`.
fn on_death(game: &mut Game, id: u32) {
    let show = terrain::rule(game, "showDeathMessages", true);
    let message = death_message(game, id);
    let shown = if show { message.clone() } else { String::new() };
    net::send(game, id, proto::S_DEATH, &proto::Death { message: shown }.encode());
    if show {
        println!("{message}");
        let chat = json!({"t": "chat", "from": "", "kind": "command", "text": message, "color": "white"});
        net::broadcast(game, None, proto::S_TEXT, chat.to_string().as_bytes());
    }
    let mode = players::mode(game, id);
    if !terrain::rule(game, "keepInventory", false) && mode != proto::GM_SPECTATOR {
        // DROPPED: destroyVanishingCursedItems (no enchantments).
        players::drop_inventory(game, id);
    }
    if let Some(p) = game.players.players.get_mut(&id) {
        p.fire = 0;
        p.dead = true;
        p.combat.reset(p.ticks_existed, false);
    }
    // The other players see the body fall over.
    if let Some(st) = game.sessions.get(&id).and_then(|s| s.state) {
        let body = proto::EntityEvent { id, status: proto::EV_DEATH }.encode();
        net::send_near(game, Some(id), st.pos, 64.0, proto::S_ENTITY_EVENT, &body);
    }
    players::sync(game, id);
}

/// `CPacketClientStatus` PERFORM_RESPAWN -> `PlayerList.recreatePlayerEntity`. A living player's
/// request is ignored (`NetHandlerPlayServer.processClientStatus`).
pub fn respawn(game: &mut Game, id: u32) {
    let Some(old) = game.players.players.get(&id) else { return };
    if !old.dead {
        return;
    }
    let mode = players::mode(game, id);
    let mut p = players::Survival::new();
    // EntityPlayerMP.copyFrom: keepInventory (or a spectator) keeps the inventory and experience.
    if terrain::rule(game, "keepInventory", false) || mode == proto::GM_SPECTATOR {
        let old = game.players.players.remove(&id).unwrap();
        p.inv = old.inv;
        p.xp = old.xp;
    }
    p.respawn_invulnerable = RESPAWN_INVULNERABLE;
    p.health = MAX_HEALTH;
    game.players.players.insert(id, p);
    let name = game.sessions[&id].name.clone();
    let mut tp = match terrain::world(game) {
        Some(w) => crate::commands::respawn_point(&w.lock().unwrap(), &name),
        None => proto::Teleport { pos: [0.5, 64.0, 0.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN | proto::TP_SURFACE },
    };
    // EntityPlayer.getBedSpawnLocation: a bed's exit, or the world spawn once the bed is gone.
    super::beds::respawn_adjust(game, &name, &mut tp);
    // The next state the viewer sends is the baseline at the new place.
    if let Some(s) = game.sessions.get_mut(&id) {
        s.state = None;
    }
    net::send(game, id, proto::S_TELEPORT, &tp.encode());
    players::send_all(game, id);
    let _ = SNEAKING;
}
