//! The survival HUD, `GuiIngame.renderPlayerStats` and `renderExpBar`: the hearts (low-health
//! shake, the flash after a change), the food (hunger jitter at zero saturation), the air
//! bubbles under water and the xp bar with its level, from the jar's `gui/icons.png`. Shown in
//! survival and adventure only (`PlayerControllerMP.shouldDrawHUD`, `gameIsSurvivalOrAdventure`),
//! in a native and a streamed session alike: it is drawn into the viewer's own frame.
//!
//! [`StatsDisplay::stats`] is the pure half: sprite placements in GUI pixels from the player's
//! numbers, so the layout is tested without a window.
//!
//! The armor row above the hearts counts the worn pieces' armor points
//! (`EntityLivingBase.getTotalArmorValue`) from the inventory the server keeps in sync.
//!
//! DROPPED: the absorption hearts, the
//! poison and wither heart tints, the hunger effect's food tint and the regeneration wave's
//! trigger (no potion effects reach the viewer; [`StatsInput::regeneration`] is the switch),
//! hardcore hearts, and the mount health that replaces the food row on a horse.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use worldgen::java_random::JavaRandom;

use super::gui::slots::Gui;
use crate::hotbar::Inventory;
use super::hurt::Vitals;
use super::gui::draw::{durability_bar, stack_count};
use super::overlay::{Overlay, HOTBAR_BARS, HOTBAR_COUNTS, HUD_ICONS, HUD_TEXT};
use super::SurvivalState;
use crate::celestial::Fluid;
use crate::hand::HideGui;
use crate::player::Player;
use crate::players::Font;
use crate::sky::{Clock, View};

pub fn plugin(app: &mut App) {
    app.init_resource::<StatsDisplay>().add_systems(
        Update,
        (tick_display.after(crate::player::run_ticks), draw.after(tick_display), hotbar_counts)
            .run_if(resource_exists::<Player>.and_then(resource_exists::<View>).and_then(resource_exists::<Clock>)),
    );
}

/// One `drawTexturedModalRect` from `icons.png`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sprite {
    pub x: i32,
    pub y: i32,
    pub u: i32,
    pub v: i32,
    pub w: i32,
    pub h: i32,
}

fn sprite(x: i32, y: i32, u: i32, v: i32) -> Sprite {
    Sprite { x, y, u, v, w: 9, h: 9 }
}

/// What `renderPlayerStats` reads from the player.
#[derive(Clone, Copy, Debug)]
pub struct StatsInput {
    pub health: f32,
    pub food: i32,
    /// `getTotalArmorValue`
    pub armor: i32,
    pub saturation: f32,
    pub air: i32,
    /// `Entity.isInsideOfMaterial(WATER)`
    pub in_water: bool,
    /// `EntityLivingBase.hurtResistantTime`
    pub hurt_resistant_time: i32,
    /// `isPotionActive(REGENERATION)`
    pub regeneration: bool,
    /// `Minecraft.getSystemTime`, milliseconds.
    pub now_ms: i64,
}

/// `GuiIngame`'s counters: `updateCounter`, the health flash and the random source.
#[derive(Resource)]
pub struct StatsDisplay {
    pub update_counter: i32,
    health_update_counter: i64,
    player_health: i32,
    last_player_health: i32,
    last_system_time: i64,
    rand: JavaRandom,
}

impl Default for StatsDisplay {
    fn default() -> StatsDisplay {
        StatsDisplay { update_counter: 0, health_update_counter: 0, player_health: 0, last_player_health: 0, last_system_time: 0, rand: JavaRandom::new(0) }
    }
}

/// `EntityLivingBase.getTotalArmorValue`: the armor points of the four worn pieces.
pub fn total_armor(inv: &Inventory) -> i32 {
    inv.armor
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| match s.def().kind {
            items::Kind::Armor(m, slot) => m.damage_reduction_amount(slot),
            _ => 0,
        })
        .sum()
}

/// `MathHelper.ceil`
fn ceil(f: f32) -> i32 {
    let i = f as i32;
    if f > i as f32 {
        i + 1
    } else {
        i
    }
}

impl StatsDisplay {
    /// `GuiIngame.updateTick`'s counter.
    pub fn tick(&mut self) {
        self.update_counter += 1;
    }

    /// `GuiIngame.renderPlayerStats` on a `width` x `height` scaled screen, in draw order.
    pub fn stats(&mut self, p: &StatsInput, width: i32, height: i32) -> Vec<Sprite> {
        let mut out = Vec::new();
        let i = ceil(p.health);
        let counter = self.update_counter as i64;
        let flash = self.health_update_counter > counter && (self.health_update_counter - counter) / 3 % 2 == 1;
        if i < self.player_health && p.hurt_resistant_time > 0 {
            self.last_system_time = p.now_ms;
            self.health_update_counter = counter + 20;
        } else if i > self.player_health && p.hurt_resistant_time > 0 {
            self.last_system_time = p.now_ms;
            self.health_update_counter = counter + 10;
        }
        if p.now_ms - self.last_system_time > 1000 {
            self.player_health = i;
            self.last_player_health = i;
            self.last_system_time = p.now_ms;
        }
        self.player_health = i;
        let j = self.last_player_health;
        // setSeed((long)(updateCounter * 312871)): the product wraps as an int.
        self.rand.set_seed(self.update_counter.wrapping_mul(312871) as i64);
        let k = p.food;
        let l = width / 2 - 91;
        let i1 = width / 2 + 91;
        let j1 = height - 39;
        // MAX_HEALTH 20 and no absorption.
        let f = 20.0f32;
        let k1 = 0;
        let l1 = ceil((f + k1 as f32) / 2.0 / 10.0);
        let i2 = (10 - (l1 - 2)).max(3);
        let k2 = j1 - 10;
        let l2 = k1;
        let j3 = if p.regeneration { self.update_counter % ceil(f + 5.0) } else { -1 };

        // armor
        let j2 = j1 - (l1 - 1) * i2 - 10;
        for k3 in 0..10 {
            if p.armor > 0 {
                let l3 = l + k3 * 8;
                if k3 * 2 + 1 < p.armor {
                    out.push(sprite(l3, j2, 34, 9));
                }
                if k3 * 2 + 1 == p.armor {
                    out.push(sprite(l3, j2, 25, 9));
                }
                if k3 * 2 + 1 > p.armor {
                    out.push(sprite(l3, j2, 16, 9));
                }
            }
        }

        // health
        for j5 in (0..ceil((f + k1 as f32) / 2.0)).rev() {
            let k5 = 16;
            let i4 = flash as i32;
            let j4 = ceil((j5 + 1) as f32 / 10.0) - 1;
            let k4 = l + j5 % 10 * 8;
            let mut l4 = j1 - j4 * i2;
            if i <= 4 {
                l4 += self.rand.next_int_bounded(2);
            }
            if l2 <= 0 && j5 == j3 {
                l4 -= 2;
            }
            out.push(sprite(k4, l4, 16 + i4 * 9, 0));
            if flash {
                if j5 * 2 + 1 < j {
                    out.push(sprite(k4, l4, k5 + 54, 0));
                }
                if j5 * 2 + 1 == j {
                    out.push(sprite(k4, l4, k5 + 63, 0));
                }
            }
            if j5 * 2 + 1 < i {
                out.push(sprite(k4, l4, k5 + 36, 0));
            }
            if j5 * 2 + 1 == i {
                out.push(sprite(k4, l4, k5 + 45, 0));
            }
        }

        // food
        for l5 in 0..10 {
            let mut j6 = j1;
            let l6 = 16;
            if p.saturation <= 0.0 && self.update_counter % (k * 3 + 1) == 0 {
                j6 = j1 + (self.rand.next_int_bounded(3) - 1);
            }
            let l7 = i1 - l5 * 8 - 9;
            out.push(sprite(l7, j6, 16, 27));
            if l5 * 2 + 1 < k {
                out.push(sprite(l7, j6, l6 + 36, 27));
            }
            if l5 * 2 + 1 == k {
                out.push(sprite(l7, j6, l6 + 45, 27));
            }
        }

        // air
        if p.in_water {
            let i6 = p.air as f64;
            let k6 = ((i6 - 2.0) * 10.0 / 300.0).ceil() as i32;
            let i7 = (i6 * 10.0 / 300.0).ceil() as i32 - k6;
            for k7 in 0..k6 + i7 {
                let x = i1 - k7 * 8 - 9;
                out.push(if k7 < k6 { sprite(x, k2, 16, 18) } else { sprite(x, k2, 25, 18) });
            }
        }
        out
    }
}

/// `GuiIngame.renderExpBar`: the bar's sprites, and the level's text and place when above 0.
/// `bar` is `EntityPlayer.experience`, the fill from 0 to 1.
pub fn exp_bar(bar: f32, level: i32, width: i32, height: i32, text_width: impl Fn(&str) -> i32) -> (Vec<Sprite>, Option<(String, i32, i32)>) {
    let x = width / 2 - 91;
    let mut out = Vec::new();
    // xpBarCap is above 0 for every level.
    let k = (bar * 183.0) as i32;
    let l = height - 32 + 3;
    out.push(Sprite { x, y: l, u: 0, v: 64, w: 182, h: 5 });
    if k > 0 {
        out.push(Sprite { x, y: l, u: 0, v: 69, w: k, h: 5 });
    }
    let text = (level > 0).then(|| {
        let s = level.to_string();
        let i1 = (width - text_width(&s)) / 2;
        let j1 = height - 31 - 4;
        (s, i1, j1)
    });
    (out, text)
}

/// The tick counter and the client vitals step once per client tick, after the player's.
fn tick_display(clock: Res<Clock>, mut display: ResMut<StatsDisplay>, mut vitals: ResMut<Vitals>) {
    for _ in 0..clock.ticks_this_frame {
        display.tick();
        vitals.tick();
    }
}

/// `GuiIngame.renderHotbar` -> `renderHotbarItem` -> `RenderItem.renderItemOverlayIntoGUI`: the
/// stack size and durability bar of each hotbar slot, every frame in every game mode.
/// Nothing is cached: the sizes come from the inventory as it is this frame. The icons move
/// behind an open screen's backdrop (item_render.rs), so the counts leave with them.
fn hotbar_counts(
    mut overlay: ResMut<Overlay>,
    inv: Option<Res<Inventory>>,
    hide: Option<Res<HideGui>>,
    font: Option<Res<Font>>,
    screens: crate::item_render::Screens,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    for layer in [HOTBAR_COUNTS, HOTBAR_BARS] {
        overlay.layers[layer] = Default::default();
    }
    let (Some(inv), Some(font)) = (inv, font) else { return };
    if hide.is_some_and(|h| h.0) || screens.open() {
        return;
    }
    let gui = Gui::new(window.physical_width(), window.physical_height(), 0, 0);
    for i in 0..9 {
        let (x, y) = (gui.w / 2 - 90 + i as i32 * 20 + 2, gui.h - 16 - 3);
        stack_count(&mut overlay.layers[HOTBAR_COUNTS], &font, &inv.main[i], x, y, None);
        durability_bar(&mut overlay.layers[HOTBAR_BARS], &inv.main[i], x, y);
    }
}

#[allow(clippy::too_many_arguments)]
/// `PlayerControllerMP.shouldDrawHUD`: the health, food and experience bars are drawn in survival
/// and adventure, not in creative or spectator.
pub fn draws_stats(game_mode: u8) -> bool {
    matches!(game_mode, proto::GM_SURVIVAL | proto::GM_ADVENTURE)
}

fn draw(
    mut overlay: ResMut<Overlay>,
    mut display: ResMut<StatsDisplay>,
    state: Res<SurvivalState>,
    inv: Option<Res<Inventory>>,
    vitals: Res<Vitals>,
    player: Res<Player>,
    view: Res<View>,
    hide: Option<Res<HideGui>>,
    time: Res<Time>,
    font: Option<Res<Font>>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    for layer in [HUD_ICONS, HUD_TEXT] {
        overlay.layers[layer] = Default::default();
    }
    let survival = draws_stats(player.game_mode);
    let Some(font) = font else { return };
    if !survival || hide.is_some_and(|h| h.0) || !player.spawned {
        return;
    }
    let gui = Gui::new(window.physical_width(), window.physical_height(), 0, 0);
    let inv = inv.as_deref().map_or(0, |i| total_armor(i));
    let input = StatsInput {
        health: state.health,
        food: state.food as i32,
        armor: inv,
        saturation: state.saturation,
        air: state.air as i32,
        in_water: view.fluid == Fluid::Water,
        hurt_resistant_time: vitals.hurt_resistant_time,
        regeneration: false,
        now_ms: (time.elapsed_secs_f64() * 1000.0) as i64 + 1_000_000,
    };
    let sprites = display.stats(&input, gui.w, gui.h);
    let (bar, level) = exp_bar(state.xp.bar, state.xp.level, gui.w, gui.h, |s| font.string_width(s));
    let icons = &mut overlay.layers[HUD_ICONS];
    for s in sprites.iter().chain(&bar) {
        icons.sprite(s.x, s.y, s.u, s.v, s.w, s.h);
    }
    if let Some((s, x, y)) = level {
        // The level in green with a black outline, drawn without a shadow.
        let text = &mut overlay.layers[HUD_TEXT];
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            text.text(&font, &s, x + dx, y + dy, 0, false, false);
        }
        text.text(&font, &s, x, y, 8453920, false, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(health: f32, food: i32) -> StatsInput {
        StatsInput { health, food, armor: 0, saturation: 5.0, air: 300, in_water: false, hurt_resistant_time: 0, regeneration: false, now_ms: 1_000_000 }
    }

    fn at(s: &[Sprite], u: i32, v: i32) -> Vec<(i32, i32)> {
        s.iter().filter(|s| s.u == u && s.v == v).map(|s| (s.x, s.y)).collect()
    }

    #[test]
    fn the_armor_row_sits_a_row_above_the_hearts_and_counts_in_halves() {
        let mut d = StatsDisplay::default();
        let none = d.stats(&input(20.0, 20), 427, 240);
        assert!(at(&none, 34, 9).is_empty() && at(&none, 16, 9).is_empty(), "no armor, no row");
        let mut worn = input(20.0, 20);
        worn.armor = 7;
        let s = d.stats(&worn, 427, 240);
        // Three full icons, a half, six empty, from x 122 on the row at 240 - 39 - 10.
        assert_eq!(at(&s, 34, 9), vec![(122, 191), (130, 191), (138, 191)]);
        assert_eq!(at(&s, 25, 9), vec![(146, 191)]);
        assert_eq!(at(&s, 16, 9).len(), 6);
    }

    #[test]
    fn a_full_bar_is_ten_hearts_over_ten_food_on_the_hotbar_row() {
        let mut d = StatsDisplay::default();
        let s = d.stats(&input(20.0, 20), 427, 240);
        // 427 / 2 - 91 = 122, y = 240 - 39.
        let backs = at(&s, 16, 0);
        assert_eq!(backs.len(), 10);
        assert_eq!(backs[0], (122 + 9 * 8, 201), "drawn from the last heart to the first");
        assert_eq!(backs[9], (122, 201));
        assert_eq!(at(&s, 52, 0).len(), 10, "full hearts");
        assert_eq!(at(&s, 61, 0).len(), 0);
        // Food runs right to left from 427 / 2 + 91 - 9 = 295.
        let full = at(&s, 52, 27);
        assert_eq!(full.len(), 10);
        assert_eq!(full[0], (295, 201));
        assert_eq!(full[9], (295 - 72, 201));
        assert_eq!(at(&s, 16, 27).len(), 10);
        assert!(at(&s, 16, 18).is_empty(), "no bubbles in air");
    }

    #[test]
    fn half_hearts_and_half_food() {
        let mut d = StatsDisplay::default();
        let s = d.stats(&input(7.0, 5), 320, 240);
        assert_eq!(at(&s, 52, 0).len(), 3, "three full hearts");
        assert_eq!(at(&s, 61, 0), vec![(320 / 2 - 91 + 3 * 8, 201)], "one half heart");
        assert_eq!(at(&s, 52, 27).len(), 2);
        assert_eq!(at(&s, 61, 27).len(), 1);
        // Fractions round up: 6.2 health shows 7.
        let mut d = StatsDisplay::default();
        let s = d.stats(&input(6.2, 20), 320, 240);
        assert_eq!(at(&s, 52, 0).len(), 3);
        assert_eq!(at(&s, 61, 0).len(), 1);
    }

    #[test]
    fn four_hearts_or_less_shake_with_the_seeded_random() {
        let mut d = StatsDisplay::default();
        d.update_counter = 7;
        let s = d.stats(&input(4.0, 20), 320, 240);
        let ys: Vec<i32> = at(&s, 16, 0).iter().map(|p| p.1).collect();
        // A Java Random seeded 7 * 312871 gives one draw per heart, last heart first.
        let mut r = JavaRandom::new(7 * 312871);
        let expect: Vec<i32> = (0..10).map(|_| 201 + r.next_int_bounded(2)).collect();
        assert_eq!(ys, expect);
        assert!(ys.contains(&202), "some heart is down");
        // Five hearts do not shake.
        let mut d3 = StatsDisplay::default();
        d3.update_counter = 7;
        let calm = d3.stats(&input(5.0, 20), 320, 240);
        assert!(at(&calm, 16, 0).iter().all(|p| p.1 == 201));
    }

    #[test]
    fn empty_saturation_jitters_the_food_every_so_often() {
        let mut still = StatsInput { saturation: 0.0, ..input(20.0, 20) };
        let mut d = StatsDisplay::default();
        // updateCounter % (food * 3 + 1) == 0 at 0 and 61.
        d.update_counter = 61;
        let s = d.stats(&still, 320, 240);
        let ys: Vec<i32> = at(&s, 16, 27).iter().map(|p| p.1).collect();
        let mut r = JavaRandom::new(61 * 312871);
        let expect: Vec<i32> = (0..10).map(|_| 201 + r.next_int_bounded(3) - 1).collect();
        assert_eq!(ys, expect);
        d.update_counter = 62;
        let s = d.stats(&still, 320, 240);
        assert!(at(&s, 16, 27).iter().all(|p| p.1 == 201));
        // Saturation above zero never jitters.
        still.saturation = 0.5;
        d.update_counter = 61;
        assert!(at(&d.stats(&still, 320, 240), 16, 27).iter().all(|p| p.1 == 201));
    }

    #[test]
    fn a_hit_flashes_the_hearts_for_twenty_ticks_in_threes() {
        let mut d = StatsDisplay::default();
        let calm = input(20.0, 20);
        d.stats(&calm, 320, 240);
        // Hurt to 14 with the resistance window open.
        let hurt = StatsInput { hurt_resistant_time: 20, ..input(14.0, 20) };
        let first = d.stats(&hurt, 320, 240);
        assert_eq!(d.health_update_counter, 20);
        assert!(at(&first, 25, 0).is_empty(), "the flash starts the frame after");
        let mut flashing = Vec::new();
        for _ in 0..21 {
            d.tick();
            let s = d.stats(&hurt, 320, 240);
            flashing.push(!at(&s, 25, 0).is_empty());
        }
        // (20 - counter) / 3 % 2 == 1 while the counter is below 20.
        let expect: Vec<bool> = (1..=21).map(|c: i64| 20 > c && (20 - c) / 3 % 2 == 1).collect();
        assert_eq!(flashing, expect);
        assert!(flashing.iter().any(|&f| f) && flashing.iter().any(|&f| !f));
        // On a flashing frame the white hearts show the health before the hit, 20, under the 14.
        let mut d = StatsDisplay::default();
        d.stats(&calm, 320, 240);
        d.stats(&hurt, 320, 240);
        for _ in 0..4 {
            d.tick();
        }
        let s = d.stats(&hurt, 320, 240);
        assert_eq!(at(&s, 25, 0).len(), 10, "flashing backgrounds");
        assert_eq!(at(&s, 70, 0).len(), 10, "the old health, all ten");
        assert_eq!(at(&s, 52, 0).len(), 7, "the current 14");
    }

    #[test]
    fn health_that_stays_changed_for_a_second_stops_being_a_change() {
        let mut d = StatsDisplay::default();
        d.stats(&input(20.0, 20), 320, 240);
        let hurt = StatsInput { hurt_resistant_time: 20, ..input(10.0, 20) };
        d.stats(&hurt, 320, 240);
        assert_eq!(d.last_player_health, 20);
        let later = StatsInput { now_ms: hurt.now_ms + 1001, hurt_resistant_time: 0, ..hurt };
        d.stats(&later, 320, 240);
        assert_eq!(d.last_player_health, 10);
    }

    #[test]
    fn regeneration_lifts_one_heart_in_a_wave() {
        let mut d = StatsDisplay::default();
        d.update_counter = 3;
        let s = d.stats(&StatsInput { regeneration: true, ..input(20.0, 20) }, 320, 240);
        let lifted: Vec<_> = at(&s, 16, 0).into_iter().filter(|p| p.1 == 199).collect();
        // 3 % ceil(25) = 3: heart index 3 sits two pixels up.
        assert_eq!(lifted, vec![(320 / 2 - 91 + 3 * 8, 199)]);
    }

    #[test]
    fn bubbles_count_the_air_left() {
        let mut d = StatsDisplay::default();
        let wet = |air| StatsInput { in_water: true, air, ..input(20.0, 20) };
        // Full air: 10 whole bubbles, right to left over the food row.
        let s = d.stats(&wet(300), 427, 240);
        let whole = at(&s, 16, 18);
        assert_eq!(whole.len(), 10);
        assert_eq!(whole[0], (295, 191));
        assert!(at(&s, 25, 18).is_empty());
        // 150 air: five whole bubbles.
        let s = d.stats(&wet(150), 427, 240);
        assert_eq!((at(&s, 16, 18).len(), at(&s, 25, 18).len()), (5, 0));
        let s = d.stats(&wet(3), 427, 240);
        assert_eq!((at(&s, 16, 18).len(), at(&s, 25, 18).len()), (1, 0));
        // At 1 the last bubble is popping.
        let s = d.stats(&wet(1), 427, 240);
        assert_eq!((at(&s, 16, 18).len(), at(&s, 25, 18).len()), (0, 1));
        // A drowning player (air below 0) shows nothing.
        assert!(d.stats(&wet(-20), 427, 240).iter().all(|s| s.v != 18));
    }

    #[test]
    fn the_xp_bar_sits_above_the_hotbar_and_the_level_is_centered_above_it() {
        let (s, text) = exp_bar(0.5, 12, 427, 240, |t| t.len() as i32 * 6);
        assert_eq!(s[0], Sprite { x: 122, y: 211, u: 0, v: 64, w: 182, h: 5 });
        assert_eq!(s[1], Sprite { x: 122, y: 211, u: 0, v: 69, w: 91, h: 5 });
        assert_eq!(text, Some(("12".into(), (427 - 12) / 2, 205)));
        let (s, text) = exp_bar(0.0, 0, 427, 240, |_| 0);
        assert_eq!((s.len(), text), (1, None));
        // The fill reaches 183 pixels at a full bar, one past the frame, as vanilla draws it.
        assert_eq!(exp_bar(1.0, 0, 427, 240, |_| 0).0[1].w, 183);
    }

    #[test]
    fn the_xp_bar_draws_in_survival_and_adventure_and_not_in_creative_or_spectator() {
        assert!(draws_stats(proto::GM_SURVIVAL) && draws_stats(proto::GM_ADVENTURE));
        assert!(!draws_stats(proto::GM_CREATIVE) && !draws_stats(proto::GM_SPECTATOR));
    }
}
