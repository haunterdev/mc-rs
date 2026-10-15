//! Potion effects as data: `Potion.registerPotions` (the 27 registry ids, names, colours, status
//! icons), `PotionEffect` (`combine`, `onUpdate`, the NBT fields) and the effect map every living
//! entity keeps (`EntityLivingBase.activePotionsMap`), with the attribute modifiers
//! `Potion.registerPotionAttributeModifier` registers and `PotionUtils.getPotionColorFromEffectList`.
//!
//! What an effect does to the entity that has it (`Potion.performEffect`, the damage and health
//! changes) needs the entity, so the server does it: this file says which effects are ready on
//! which tick and what their modifiers add up to.
//!
//! DROPPED: `PotionType` and the potion items' effect lists (brewing and splash potions are a later
//! task), `isSplashPotion`, `Potion.affectEntity` (thrown potions), and the luck attribute
//! (`LUCK` has no consumer yet).

/// A `Potion.REGISTRY` entry. The discriminant is `Potion.getIdFromPotion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Effect {
    Speed = 1,
    Slowness,
    Haste,
    MiningFatigue,
    Strength,
    InstantHealth,
    InstantDamage,
    JumpBoost,
    Nausea,
    Regeneration,
    Resistance,
    FireResistance,
    WaterBreathing,
    Invisibility,
    Blindness,
    NightVision,
    Hunger,
    Weakness,
    Poison,
    Wither,
    HealthBoost,
    Absorption,
    Saturation,
    Glowing,
    Levitation,
    Luck,
    Unluck,
}

/// Every effect, in id order.
pub const ALL: [Effect; 27] = [
    Effect::Speed,
    Effect::Slowness,
    Effect::Haste,
    Effect::MiningFatigue,
    Effect::Strength,
    Effect::InstantHealth,
    Effect::InstantDamage,
    Effect::JumpBoost,
    Effect::Nausea,
    Effect::Regeneration,
    Effect::Resistance,
    Effect::FireResistance,
    Effect::WaterBreathing,
    Effect::Invisibility,
    Effect::Blindness,
    Effect::NightVision,
    Effect::Hunger,
    Effect::Weakness,
    Effect::Poison,
    Effect::Wither,
    Effect::HealthBoost,
    Effect::Absorption,
    Effect::Saturation,
    Effect::Glowing,
    Effect::Levitation,
    Effect::Luck,
    Effect::Unluck,
];

/// `PotionUtils.getPotionColorFromEffectList` of an empty list.
pub const DEFAULT_COLOR: i32 = 3_694_022;

impl Effect {
    /// `Potion.getIdFromPotion`
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// `Potion.getPotionById`
    pub fn from_id(id: i32) -> Option<Effect> {
        if (1..=27).contains(&id) {
            Some(ALL[(id - 1) as usize])
        } else {
            None
        }
    }

    /// `Potion.getPotionFromResourceLocation`: `speed` or `minecraft:speed`.
    pub fn from_name(name: &str) -> Option<Effect> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        ALL.iter().copied().find(|e| e.name() == n)
    }

    /// The registry name (`minecraft:<name>`).
    pub const fn name(self) -> &'static str {
        match self {
            Effect::Speed => "speed",
            Effect::Slowness => "slowness",
            Effect::Haste => "haste",
            Effect::MiningFatigue => "mining_fatigue",
            Effect::Strength => "strength",
            Effect::InstantHealth => "instant_health",
            Effect::InstantDamage => "instant_damage",
            Effect::JumpBoost => "jump_boost",
            Effect::Nausea => "nausea",
            Effect::Regeneration => "regeneration",
            Effect::Resistance => "resistance",
            Effect::FireResistance => "fire_resistance",
            Effect::WaterBreathing => "water_breathing",
            Effect::Invisibility => "invisibility",
            Effect::Blindness => "blindness",
            Effect::NightVision => "night_vision",
            Effect::Hunger => "hunger",
            Effect::Weakness => "weakness",
            Effect::Poison => "poison",
            Effect::Wither => "wither",
            Effect::HealthBoost => "health_boost",
            Effect::Absorption => "absorption",
            Effect::Saturation => "saturation",
            Effect::Glowing => "glowing",
            Effect::Levitation => "levitation",
            Effect::Luck => "luck",
            Effect::Unluck => "unluck",
        }
    }

    /// `Potion.getName` (`setPotionName`): the translation key.
    pub const fn lang_key(self) -> &'static str {
        match self {
            Effect::Speed => "effect.moveSpeed",
            Effect::Slowness => "effect.moveSlowdown",
            Effect::Haste => "effect.digSpeed",
            Effect::MiningFatigue => "effect.digSlowDown",
            Effect::Strength => "effect.damageBoost",
            Effect::InstantHealth => "effect.heal",
            Effect::InstantDamage => "effect.harm",
            Effect::JumpBoost => "effect.jump",
            Effect::Nausea => "effect.confusion",
            Effect::Regeneration => "effect.regeneration",
            Effect::Resistance => "effect.resistance",
            Effect::FireResistance => "effect.fireResistance",
            Effect::WaterBreathing => "effect.waterBreathing",
            Effect::Invisibility => "effect.invisibility",
            Effect::Blindness => "effect.blindness",
            Effect::NightVision => "effect.nightVision",
            Effect::Hunger => "effect.hunger",
            Effect::Weakness => "effect.weakness",
            Effect::Poison => "effect.poison",
            Effect::Wither => "effect.wither",
            Effect::HealthBoost => "effect.healthBoost",
            Effect::Absorption => "effect.absorption",
            Effect::Saturation => "effect.saturation",
            Effect::Glowing => "effect.glowing",
            Effect::Levitation => "effect.levitation",
            Effect::Luck => "effect.luck",
            Effect::Unluck => "effect.unluck",
        }
    }

    /// The `en_us.lang` text of [`Effect::lang_key`], for a viewer that has no lang file.
    pub const fn display_name(self) -> &'static str {
        match self {
            Effect::Speed => "Speed",
            Effect::Slowness => "Slowness",
            Effect::Haste => "Haste",
            Effect::MiningFatigue => "Mining Fatigue",
            Effect::Strength => "Strength",
            Effect::InstantHealth => "Instant Health",
            Effect::InstantDamage => "Instant Damage",
            Effect::JumpBoost => "Jump Boost",
            Effect::Nausea => "Nausea",
            Effect::Regeneration => "Regeneration",
            Effect::Resistance => "Resistance",
            Effect::FireResistance => "Fire Resistance",
            Effect::WaterBreathing => "Water Breathing",
            Effect::Invisibility => "Invisibility",
            Effect::Blindness => "Blindness",
            Effect::NightVision => "Night Vision",
            Effect::Hunger => "Hunger",
            Effect::Weakness => "Weakness",
            Effect::Poison => "Poison",
            Effect::Wither => "Wither",
            Effect::HealthBoost => "Health Boost",
            Effect::Absorption => "Absorption",
            Effect::Saturation => "Saturation",
            Effect::Glowing => "Glowing",
            Effect::Levitation => "Levitation",
            Effect::Luck => "Luck",
            Effect::Unluck => "Bad Luck",
        }
    }

    /// `Potion.getLiquidColor`
    pub const fn color(self) -> i32 {
        match self {
            Effect::Speed => 8_171_462,
            Effect::Slowness => 5_926_017,
            Effect::Haste => 14_270_531,
            Effect::MiningFatigue => 4_866_583,
            Effect::Strength => 9_643_043,
            Effect::InstantHealth => 16_262_179,
            Effect::InstantDamage => 4_393_481,
            Effect::JumpBoost => 2_293_580,
            Effect::Nausea => 5_578_058,
            Effect::Regeneration => 13_458_603,
            Effect::Resistance => 10_044_730,
            Effect::FireResistance => 14_981_690,
            Effect::WaterBreathing => 3_035_801,
            Effect::Invisibility => 8_356_754,
            Effect::Blindness => 2_039_587,
            Effect::NightVision => 2_039_713,
            Effect::Hunger => 5_797_459,
            Effect::Weakness => 4_738_376,
            Effect::Poison => 5_149_489,
            Effect::Wither => 3_484_199,
            Effect::HealthBoost => 16_284_963,
            Effect::Absorption => 2_445_989,
            Effect::Saturation => 16_262_179,
            Effect::Glowing => 9_740_385,
            Effect::Levitation => 13_565_951,
            Effect::Luck => 3_381_504,
            Effect::Unluck => 12_624_973,
        }
    }

    /// `Potion.isBadEffect`
    pub const fn is_bad(self) -> bool {
        matches!(
            self,
            Effect::Slowness
                | Effect::MiningFatigue
                | Effect::InstantDamage
                | Effect::Nausea
                | Effect::Blindness
                | Effect::Hunger
                | Effect::Weakness
                | Effect::Poison
                | Effect::Wither
                | Effect::Levitation
                | Effect::Unluck
        )
    }

    /// `Potion.getStatusIconIndex` (`setIconIndex(x, y)` is `x + y * 8`); `None` for the instant
    /// effects and saturation, which have no icon.
    pub const fn status_icon(self) -> Option<u8> {
        Some(match self {
            Effect::Speed => 0,
            Effect::Slowness => 1,
            Effect::Haste => 2,
            Effect::MiningFatigue => 3,
            Effect::Strength => 4,
            Effect::JumpBoost => 2 + 8,
            Effect::Nausea => 3 + 8,
            Effect::Regeneration => 7,
            Effect::Resistance => 6 + 8,
            Effect::FireResistance => 7 + 8,
            Effect::WaterBreathing => 16,
            Effect::Invisibility => 8,
            Effect::Blindness => 5 + 8,
            Effect::NightVision => 4 + 8,
            Effect::Hunger => 1 + 8,
            Effect::Weakness => 5,
            Effect::Poison => 6,
            Effect::Wither => 1 + 16,
            Effect::HealthBoost => 7 + 16,
            Effect::Absorption => 2 + 16,
            Effect::Glowing => 4 + 16,
            Effect::Levitation => 3 + 16,
            Effect::Luck => 5 + 16,
            Effect::Unluck => 6 + 16,
            Effect::InstantHealth | Effect::InstantDamage | Effect::Saturation => return None,
        })
    }

    /// `Potion.isInstant`: `PotionHealth` (instant health, instant damage and saturation).
    pub const fn is_instant(self) -> bool {
        matches!(self, Effect::InstantHealth | Effect::InstantDamage | Effect::Saturation)
    }

    /// `Potion.isReady(duration, amplifier)` and the `PotionHealth` override.
    pub fn is_ready(self, duration: i32, amplifier: i32) -> bool {
        let every = |base: i32| {
            // Java shifts by `amplifier & 31`.
            let k = base >> (amplifier & 31);
            if k > 0 {
                duration % k == 0
            } else {
                true
            }
        };
        match self {
            Effect::Regeneration => every(50),
            Effect::Poison => every(25),
            Effect::Wither => every(40),
            Effect::Hunger => true,
            e if e.is_instant() => duration >= 1,
            _ => false,
        }
    }
}

/// `new PotionEffect(potion, duration, amplifier, ambient, showParticles)`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionEffect {
    pub effect: Effect,
    /// ticks
    pub duration: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub show_particles: bool,
}

impl PotionEffect {
    /// `new PotionEffect(potion, duration, amplifier)`
    pub const fn new(effect: Effect, duration: i32, amplifier: i32) -> PotionEffect {
        PotionEffect { effect, duration, amplifier, ambient: false, show_particles: true }
    }

    /// `PotionEffect.combine`: a stronger effect replaces this one, a longer one of the same
    /// strength extends it.
    pub fn combine(&mut self, other: &PotionEffect) {
        if other.amplifier > self.amplifier {
            self.amplifier = other.amplifier;
            self.duration = other.duration;
        } else if other.amplifier == self.amplifier && self.duration < other.duration {
            self.duration = other.duration;
        } else if !other.ambient && self.ambient {
            self.ambient = other.ambient;
        }
        self.show_particles = other.show_particles;
    }

    /// `PotionEffect.compareTo`, the order the inventory lists them in.
    pub fn compare(&self, o: &PotionEffect) -> std::cmp::Ordering {
        let mut c = self.ambient.cmp(&o.ambient);
        if (self.duration <= 32147 || o.duration <= 32147) && (!self.ambient || !o.ambient) && c.is_eq() {
            c = self.duration.cmp(&o.duration);
        }
        if c.is_eq() {
            c = self.effect.color().cmp(&o.effect.color());
        }
        c
    }
}

/// `Potion.getPotionDurationString` / `StringUtils.ticksToElapsedTime`: `m:ss`.
pub fn duration_string(ticks: i32) -> String {
    let mut i = ticks / 20;
    let j = i / 60;
    i %= 60;
    if i < 10 {
        format!("{j}:0{i}")
    } else {
        format!("{j}:{i}")
    }
}

/// What `addPotionEffect` did: which hook of `EntityLivingBase` runs next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Added {
    /// `onNewPotionEffect`
    New(PotionEffect),
    /// `onChangedPotionEffect(effect, true)`, with the merged effect.
    Changed(PotionEffect),
}

/// What one `updatePotionEffects` pass found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tick {
    /// `performEffect` calls, in order: the effect and its amplifier.
    pub perform: Vec<(Effect, i32)>,
    /// `onChangedPotionEffect(effect, false)`: the 600-tick resync.
    pub changed: Vec<PotionEffect>,
    /// `onFinishedPotionEffect`: expired and removed.
    pub finished: Vec<PotionEffect>,
}

/// `EntityLivingBase.activePotionsMap`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Effects {
    list: Vec<PotionEffect>,
}

impl Effects {
    pub fn new() -> Effects {
        Effects::default()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// `getActivePotionEffects`
    pub fn iter(&self) -> impl Iterator<Item = &PotionEffect> {
        self.list.iter()
    }

    /// `getActivePotionEffect`
    pub fn get(&self, e: Effect) -> Option<&PotionEffect> {
        self.list.iter().find(|p| p.effect == e)
    }

    /// `isPotionActive`
    pub fn is_active(&self, e: Effect) -> bool {
        self.get(e).is_some()
    }

    /// The amplifier of an active effect.
    pub fn amplifier(&self, e: Effect) -> Option<i32> {
        self.get(e).map(|p| p.amplifier)
    }

    /// `addPotionEffect` without the `isPotionApplicable` check (the caller makes it).
    pub fn add(&mut self, new: PotionEffect) -> Added {
        match self.list.iter_mut().find(|p| p.effect == new.effect) {
            None => {
                self.list.push(new);
                Added::New(new)
            }
            Some(old) => {
                old.combine(&new);
                Added::Changed(*old)
            }
        }
    }

    /// `removeActivePotionEffect`
    pub fn remove(&mut self, e: Effect) -> Option<PotionEffect> {
        let i = self.list.iter().position(|p| p.effect == e)?;
        Some(self.list.remove(i))
    }

    /// `clearActivePotions`: everything that was active, for `onFinishedPotionEffect`.
    pub fn clear(&mut self) -> Vec<PotionEffect> {
        std::mem::take(&mut self.list)
    }

    /// `updatePotionEffects`' loop: `PotionEffect.onUpdate` on each. An effect that runs out is
    /// removed and reported in `finished`.
    pub fn tick(&mut self) -> Tick {
        let mut t = Tick::default();
        self.list.retain_mut(|p| {
            if p.duration > 0 {
                if p.effect.is_ready(p.duration, p.amplifier) {
                    t.perform.push((p.effect, p.amplifier));
                }
                p.duration -= 1;
            }
            if p.duration > 0 {
                if p.duration % 600 == 0 {
                    t.changed.push(*p);
                }
                true
            } else {
                t.finished.push(*p);
                false
            }
        });
        t
    }

    /// `PotionUtils.getPotionColorFromEffectList` of the active effects.
    pub fn color(&self) -> i32 {
        color_of(self.list.iter())
    }

    /// `EntityLivingBase.areAllPotionsAmbient`
    pub fn all_ambient(&self) -> bool {
        self.list.iter().all(|p| p.ambient)
    }

    /// The effects sorted as the inventory lists them (`Ordering.natural().sortedCopy`).
    pub fn sorted(&self) -> Vec<PotionEffect> {
        let mut v = self.list.clone();
        v.sort_by(|a, b| a.compare(b));
        v
    }

    // ---- the attribute modifiers of `Potion.registerPotionAttributeModifier` ----

    /// `MOVEMENT_SPEED` multiplier: operation 2 modifiers, each `1 + amount * (amplifier + 1)`
    /// (speed +0.2, slowness -0.15).
    pub fn movement_speed_factor(&self) -> f64 {
        let mut f = 1.0f64;
        if let Some(a) = self.amplifier(Effect::Speed) {
            f *= 1.0 + 0.200_000_002_980_232_24 * (a + 1) as f64;
        }
        if let Some(a) = self.amplifier(Effect::Slowness) {
            f *= 1.0 + -0.150_000_005_960_464_48 * (a + 1) as f64;
        }
        f
    }

    /// `ATTACK_DAMAGE` operation 0 total: strength `3 * (amp + 1)`, weakness `-4 * (amp + 1)`
    /// (`PotionAttackDamage.getAttributeModifierAmount`).
    pub fn attack_damage_bonus(&self) -> f64 {
        let mut d = 0.0;
        if let Some(a) = self.amplifier(Effect::Strength) {
            d += 3.0 * (a + 1) as f64;
        }
        if let Some(a) = self.amplifier(Effect::Weakness) {
            d += -4.0 * (a + 1) as f64;
        }
        d
    }

    /// `ATTACK_SPEED` multiplier: haste `+0.1 * (amp + 1)`, mining fatigue `-0.1 * (amp + 1)`
    /// (operation 2).
    pub fn attack_speed_factor(&self) -> f64 {
        let mut f = 1.0f64;
        if let Some(a) = self.amplifier(Effect::Haste) {
            f *= 1.0 + 0.100_000_001_490_116_12 * (a + 1) as f64;
        }
        if let Some(a) = self.amplifier(Effect::MiningFatigue) {
            f *= 1.0 + -0.100_000_001_490_116_12 * (a + 1) as f64;
        }
        f
    }

    /// `MAX_HEALTH` operation 0 total: health boost `4 * (amp + 1)`.
    pub fn max_health_bonus(&self) -> f64 {
        self.amplifier(Effect::HealthBoost).map_or(0.0, |a| 4.0 * (a + 1) as f64)
    }

    /// `EntityLivingBase.fall`'s `(amplifier + 1)` of jump boost, 0 without it.
    pub fn jump_boost_level(&self) -> f32 {
        self.amplifier(Effect::JumpBoost).map_or(0.0, |a| (a + 1) as f32)
    }
}

/// `PotionUtils.getPotionColorFromEffectList`
pub fn color_of<'a>(effects: impl Iterator<Item = &'a PotionEffect>) -> i32 {
    let (mut f, mut f1, mut f2) = (0.0f32, 0.0f32, 0.0f32);
    let mut j = 0;
    let mut any = false;
    for e in effects {
        any = true;
        if e.show_particles {
            let k = e.effect.color();
            let l = e.amplifier + 1;
            f += (l * (k >> 16 & 255)) as f32 / 255.0;
            f1 += (l * (k >> 8 & 255)) as f32 / 255.0;
            f2 += (l * (k & 255)) as f32 / 255.0;
            j += l;
        }
    }
    if !any {
        return DEFAULT_COLOR;
    }
    if j == 0 {
        return 0;
    }
    f = f / j as f32 * 255.0;
    f1 = f1 / j as f32 * 255.0;
    f2 = f2 / j as f32 * 255.0;
    (f as i32) << 16 | (f1 as i32) << 8 | f2 as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pe(e: Effect, d: i32, a: i32) -> PotionEffect {
        PotionEffect::new(e, d, a)
    }

    #[test]
    fn ids_names_and_icons_match_the_registry() {
        for (i, e) in ALL.iter().enumerate() {
            assert_eq!(e.id(), i as i32 + 1);
            assert_eq!(Effect::from_id(e.id()), Some(*e));
            assert_eq!(Effect::from_name(e.name()), Some(*e));
        }
        assert_eq!(Effect::from_id(0), None);
        assert_eq!(Effect::from_id(28), None);
        assert_eq!(Effect::from_name("minecraft:wither"), Some(Effect::Wither));
        assert_eq!(Effect::Speed.lang_key(), "effect.moveSpeed");
        assert_eq!(Effect::Absorption.status_icon(), Some(18));
        assert_eq!(Effect::InstantHealth.status_icon(), None);
    }

    #[test]
    fn readiness_follows_the_shifted_interval() {
        // Regeneration II: 50 >> 1 = 25
        assert!(Effect::Regeneration.is_ready(50, 1));
        assert!(!Effect::Regeneration.is_ready(49, 1));
        assert!(Effect::Regeneration.is_ready(25, 1));
        // Poison I: every 25 ticks, poison V: 25 >> 5 = 0 so every tick
        assert!(Effect::Poison.is_ready(25, 0) && !Effect::Poison.is_ready(24, 0));
        assert!(Effect::Poison.is_ready(7, 5));
        assert!(Effect::Hunger.is_ready(3, 0));
        assert!(Effect::InstantHealth.is_ready(1, 0) && !Effect::InstantHealth.is_ready(0, 0));
        assert!(!Effect::Speed.is_ready(20, 0));
    }

    #[test]
    fn combine_keeps_the_stronger_or_longer() {
        let mut a = pe(Effect::Speed, 100, 0);
        a.combine(&pe(Effect::Speed, 50, 1));
        assert_eq!((a.amplifier, a.duration), (1, 50));
        a.combine(&pe(Effect::Speed, 200, 0));
        assert_eq!((a.amplifier, a.duration), (1, 50), "a weaker effect does not replace it");
        a.combine(&pe(Effect::Speed, 90, 1));
        assert_eq!(a.duration, 90);
    }

    #[test]
    fn tick_counts_down_and_reports_the_end() {
        let mut fx = Effects::new();
        assert!(matches!(fx.add(pe(Effect::Regeneration, 3, 1)), Added::New(_)));
        assert!(matches!(fx.add(pe(Effect::Regeneration, 5, 1)), Added::Changed(e) if e.duration == 5));
        let mut performed = 0;
        let mut finished = 0;
        for _ in 0..10 {
            let t = fx.tick();
            performed += t.perform.len();
            finished += t.finished.len();
        }
        // 5 ticks, ready at 50 >> 1 = 25 only at multiples of 25: none in 5, 4, 3, 2, 1
        assert_eq!((performed, finished), (0, 1));
        assert!(fx.is_empty());
    }

    #[test]
    fn modifiers_add_up() {
        let mut fx = Effects::new();
        fx.add(pe(Effect::Speed, 100, 1));
        fx.add(pe(Effect::Slowness, 100, 0));
        let f = fx.movement_speed_factor();
        assert!((f - (1.0 + 0.4) * (1.0 - 0.15)).abs() < 1e-6, "{f}");
        fx.add(pe(Effect::Strength, 100, 0));
        fx.add(pe(Effect::Weakness, 100, 1));
        assert_eq!(fx.attack_damage_bonus(), 3.0 - 8.0);
        fx.add(pe(Effect::HealthBoost, 100, 1));
        assert_eq!(fx.max_health_bonus(), 8.0);
    }

    #[test]
    fn colour_and_duration_text() {
        assert_eq!(Effects::new().color(), DEFAULT_COLOR);
        let mut fx = Effects::new();
        fx.add(pe(Effect::Speed, 100, 0));
        assert_eq!(fx.color(), Effect::Speed.color());
        assert_eq!(duration_string(1200), "1:00");
        assert_eq!(duration_string(100), "0:05");
        assert_eq!(duration_string(2400), "2:00");
    }
}
