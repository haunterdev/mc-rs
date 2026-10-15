//! Player experience: `EntityPlayer.experience`, `experienceLevel`, `experienceTotal`,
//! `addExperience`, `addExperienceLevel`, `xpBarCap`, the level-up sound, the `SPacketSetExperience`
//! ([`proto::S_XP`]) the client's bar reads, and what a death drops
//! (`EntityPlayer.getExperiencePoints`, spawned at `deathTime == 20`).
//!
//! DROPPED: `addScore` (the death screen's score reads the experience total), `onEnchant`'s level
//! cost (enchanting is not here yet), the `xpSeed`.

use super::players::Survival;
use super::{net, terrain, xp_orbs, Game};

/// `deathTime` at which `onDeathUpdate` spawns the orbs.
const DEATH_TICKS: i32 = 20;

impl Survival {
    /// `EntityPlayer.xpBarCap`: the points the level needs to the next.
    pub fn xp_bar_cap(&self) -> i32 {
        let l = self.xp.level;
        if l >= 30 {
            112 + (l - 30) * 9
        } else if l >= 15 {
            37 + (l - 15) * 5
        } else {
            7 + l * 2
        }
    }

    /// `EntityPlayer.addExperience`. Returns the volume of the level-up sound when one plays.
    pub fn add_experience(&mut self, amount: i32) -> Option<f32> {
        let mut sound = None;
        let amount = amount.min(i32::MAX - self.xp.total);
        self.xp.bar += amount as f32 / self.xp_bar_cap() as f32;
        self.xp.total += amount;
        while self.xp.bar >= 1.0 {
            self.xp.bar = (self.xp.bar - 1.0) * self.xp_bar_cap() as f32;
            sound = sound.or(self.add_experience_level(1));
            self.xp.bar /= self.xp_bar_cap() as f32;
        }
        sound
    }

    /// `EntityPlayer.addExperienceLevel`. Returns the volume of the level-up sound when one plays
    /// (every fifth level, at most once in 100 ticks).
    pub fn add_experience_level(&mut self, levels: i32) -> Option<f32> {
        self.xp.level += levels;
        if self.xp.level < 0 {
            self.xp.level = 0;
            self.xp.bar = 0.0;
            self.xp.total = 0;
        }
        if levels > 0 && self.xp.level % 5 == 0 && (self.last_xp_sound as f32) < (self.ticks_existed as f32) - 100.0 {
            let f = if self.xp.level > 30 { 1.0 } else { self.xp.level as f32 / 30.0 };
            self.last_xp_sound = self.ticks_existed;
            return Some(f * 0.75);
        }
        None
    }
}

/// `SPacketSetExperience` to the player.
pub fn send(game: &mut Game, id: u32) {
    let Some(p) = game.players.players.get(&id) else { return };
    let msg = p.xp.encode();
    net::send(game, id, proto::S_XP, &msg);
}

/// `EntityPlayerMP.addExperience`: the points, the level-up sound, and the bar to the client.
pub fn add_experience(game: &mut Game, id: u32, amount: i32) {
    let Some(p) = game.players.players.get_mut(&id) else { return };
    let sound = p.add_experience(amount);
    if let (Some(volume), Some(st)) = (sound, game.sessions.get(&id).and_then(|s| s.state)) {
        // World.playSound(null, ...): PLAYERS category, to everyone in range.
        let at = [st.pos[0] as f32, st.pos[1] as f32, st.pos[2] as f32];
        let s = proto::Sound { name: "entity.player.levelup".into(), category: 7, pos: at, volume, pitch: 1.0 };
        net::send_near(game, None, st.pos, 16.0, proto::S_SOUND, &s.encode());
    }
    send(game, id);
}

/// `EntityPlayer.getExperiencePoints`: 7 per level, at most 100, none with `keepInventory` or as a
/// spectator.
pub fn death_points(game: &Game, id: u32) -> i32 {
    if terrain::rule(game, "keepInventory", false) || super::players::mode(game, id) == proto::GM_SPECTATOR {
        return 0;
    }
    let level = game.players.players.get(&id).map_or(0, |p| p.xp.level);
    (level * 7).min(100)
}

/// `EntityLivingBase.onDeathUpdate` for a dead player: at tick 20 the orbs drop at the body.
/// The player entity stays until the respawn, so the counter runs on.
pub fn tick_dead(game: &mut Game) {
    let mut ids: Vec<u32> = game.players.players.iter().filter(|(_, p)| p.dead).map(|(&id, _)| id).collect();
    ids.sort_unstable();
    for id in ids {
        let Some(p) = game.players.players.get_mut(&id) else { continue };
        p.death_time += 1;
        if p.death_time != DEATH_TICKS {
            continue;
        }
        let Some(st) = game.sessions.get(&id).and_then(|s| s.state) else { continue };
        let points = death_points(game, id);
        xp_orbs::spawn_orbs(game, st.pos, points);
    }
}
