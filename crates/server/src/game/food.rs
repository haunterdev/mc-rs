//! `net.minecraft.util.FoodStats`, ported 1:1. The player-side calls (`heal`, `attackEntityFrom`)
//! come back as a [`FoodTick`] for players.rs to carry out, so the stats stay a plain value.

use serde_json::{json, Value};

/// `EnumDifficulty`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    /// The `difficulty` of server.json: a name or the id 0 to 3; anything else is normal.
    pub fn parse(s: &str) -> Difficulty {
        match s.trim().to_ascii_lowercase().as_str() {
            "peaceful" | "0" => Difficulty::Peaceful,
            "easy" | "1" => Difficulty::Easy,
            "hard" | "3" => Difficulty::Hard,
            _ => Difficulty::Normal,
        }
    }
}

/// What `FoodStats.onUpdate` asked the player to do.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FoodTick {
    /// `player.heal(amount)`
    pub heal: f32,
    /// `player.attackEntityFrom(DamageSource.STARVE, 1.0F)`
    pub starve: bool,
}

/// `FoodStats`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoodStats {
    pub level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub timer: i32,
    pub prev_level: i32,
}

impl Default for FoodStats {
    fn default() -> FoodStats {
        FoodStats { level: 20, saturation: 5.0, exhaustion: 0.0, timer: 0, prev_level: 20 }
    }
}

impl FoodStats {
    /// `addStats(int, float)`
    pub fn add_stats(&mut self, food: i32, saturation_modifier: f32) {
        self.level = (food + self.level).min(20);
        self.saturation = (self.saturation + food as f32 * saturation_modifier * 2.0).min(self.level as f32);
    }

    /// `addExhaustion`
    pub fn add_exhaustion(&mut self, exhaustion: f32) {
        self.exhaustion = (self.exhaustion + exhaustion).min(40.0);
    }

    /// `needFood`
    pub fn need_food(&self) -> bool {
        self.level < 20
    }

    /// `onUpdate`. `health` is the player's, `max_health` 20; `natural_regeneration` the game rule.
    pub fn on_update(&mut self, difficulty: Difficulty, natural_regeneration: bool, health: f32, max_health: f32) -> FoodTick {
        let mut out = FoodTick::default();
        // shouldHeal: alive and hurt.
        let should_heal = health > 0.0 && health < max_health;
        self.prev_level = self.level;
        if self.exhaustion > 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else if difficulty != Difficulty::Peaceful {
                self.level = (self.level - 1).max(0);
            }
        }
        if natural_regeneration && self.saturation > 0.0 && should_heal && self.level >= 20 {
            self.timer += 1;
            if self.timer >= 10 {
                let f = self.saturation.min(6.0);
                out.heal = f / 6.0;
                self.add_exhaustion(f);
                self.timer = 0;
            }
        } else if natural_regeneration && self.level >= 18 && should_heal {
            self.timer += 1;
            if self.timer >= 80 {
                out.heal = 1.0;
                self.add_exhaustion(6.0);
                self.timer = 0;
            }
        } else if self.level <= 0 {
            self.timer += 1;
            if self.timer >= 80 {
                if health > 10.0 || difficulty == Difficulty::Hard || health > 1.0 && difficulty == Difficulty::Normal {
                    out.starve = true;
                }
                self.timer = 0;
            }
        } else {
            self.timer = 0;
        }
        out
    }

    /// `writeNBT`, as the JSON keys of the same names.
    pub fn to_json(&self) -> Value {
        json!({"foodLevel": self.level, "foodTickTimer": self.timer, "foodSaturationLevel": self.saturation, "foodExhaustionLevel": self.exhaustion})
    }

    /// `readNBT`: nothing changes unless `foodLevel` is there.
    pub fn read_json(&mut self, v: &Value) {
        if let Some(level) = v.get("foodLevel").and_then(|x| x.as_i64()) {
            self.level = level as i32;
            self.timer = v.get("foodTickTimer").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
            self.saturation = v.get("foodSaturationLevel").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
            self.exhaustion = v.get("foodExhaustionLevel").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `n` ticks against a player who heals as told, returning the health.
    fn run(f: &mut FoodStats, d: Difficulty, mut health: f32, n: u32) -> f32 {
        for _ in 0..n {
            let t = f.on_update(d, true, health, 20.0);
            health += t.heal;
            if t.starve {
                health -= 1.0;
            }
        }
        health
    }

    #[test]
    fn food_18_heals_one_every_80_ticks_for_6_exhaustion() {
        let mut f = FoodStats { level: 18, saturation: 0.0, ..Default::default() };
        assert_eq!(run(&mut f, Difficulty::Normal, 10.0, 79), 10.0);
        assert_eq!(f.exhaustion, 0.0);
        let h = run(&mut f, Difficulty::Normal, 10.0, 1);
        assert_eq!(h, 11.0);
        // 6 exhaustion added, then 4 taken off at the next tick for one food point (no saturation).
        assert_eq!(f.exhaustion, 6.0);
        run(&mut f, Difficulty::Normal, 11.0, 1);
        assert_eq!((f.exhaustion, f.level), (2.0, 17));
    }

    #[test]
    fn food_20_with_saturation_heals_fast() {
        let mut f = FoodStats::default();
        let h = run(&mut f, Difficulty::Normal, 10.0, 10);
        // min(saturation 5, 6) / 6 every 10 ticks, and the 5 exhaustion it costs.
        assert!((h - (10.0 + 5.0 / 6.0)).abs() < 1e-6, "{h}");
        assert_eq!(f.exhaustion, 5.0);
        // A full heart bar does not heal.
        let mut g = FoodStats::default();
        assert_eq!(run(&mut g, Difficulty::Normal, 20.0, 100), 20.0);
    }

    #[test]
    fn exhaustion_over_4_costs_saturation_then_food() {
        let mut f = FoodStats::default();
        f.add_exhaustion(4.5);
        f.on_update(Difficulty::Normal, true, 20.0, 20.0);
        assert_eq!((f.saturation, f.exhaustion, f.level), (4.0, 0.5, 20));
        let mut f = FoodStats { saturation: 0.0, ..Default::default() };
        f.add_exhaustion(4.5);
        f.on_update(Difficulty::Normal, true, 20.0, 20.0);
        assert_eq!(f.level, 19);
        // Peaceful never loses food to exhaustion.
        let mut f = FoodStats { saturation: 0.0, ..Default::default() };
        f.add_exhaustion(4.5);
        f.on_update(Difficulty::Peaceful, true, 20.0, 20.0);
        assert_eq!(f.level, 20);
        // Exhaustion stops at 40.
        f.add_exhaustion(100.0);
        assert_eq!(f.exhaustion, 40.0);
    }

    #[test]
    fn starvation_stops_at_the_difficulty_floor() {
        let starve = |d, start| {
            let mut f = FoodStats { level: 0, saturation: 0.0, ..Default::default() };
            run(&mut f, d, start, 80 * 30)
        };
        // Easy stops at 10, normal at 1, hard goes on to 0.
        assert_eq!(starve(Difficulty::Easy, 20.0), 10.0);
        assert_eq!(starve(Difficulty::Normal, 20.0), 1.0);
        assert_eq!(starve(Difficulty::Hard, 20.0), -10.0);
    }

    #[test]
    fn add_stats_caps_food_and_saturation() {
        let mut f = FoodStats { level: 10, saturation: 0.0, ..Default::default() };
        f.add_stats(6, 0.6);
        assert_eq!(f.level, 16);
        assert!((f.saturation - 7.2).abs() < 1e-5);
        f.add_stats(20, 1.0);
        assert_eq!((f.level, f.saturation), (20, 20.0));
    }

    #[test]
    fn json_round_trips_and_ignores_a_record_without_food() {
        let f = FoodStats { level: 7, saturation: 1.5, exhaustion: 2.25, timer: 33, prev_level: 20 };
        let mut g = FoodStats::default();
        g.read_json(&f.to_json());
        assert_eq!((g.level, g.saturation, g.exhaustion, g.timer), (7, 1.5, 2.25, 33));
        g.read_json(&json!({}));
        assert_eq!(g.level, 7);
    }
}
