//! Potion effects on the client: `NetHandlerPlayClient.handleEntityEffect` and
//! `handleRemoveEntityEffect` for the session's own player (the effects live on [`Player`], whose
//! tick counts the durations down and whose movement, jump and dig speed read them), and
//! `InventoryEffectRenderer`'s list beside the survival inventory.
//!
//! The screen effects are in sky.rs (night vision brightness in the lightmap, blindness fog) and
//! the speed FOV is `Player::speed_attribute`.
//!
//! DROPPED: the nausea wobble (`EntityRenderer.setupCameraTransform`'s portal-style warp), the
//! swirl particles `EntityLivingBase.updatePotionMetadata` makes (the particle system has no spell
//! particle), `Potion.renderInventoryEffect` (nothing draws extra in 1.12.2 vanilla), the
//! health boost hearts and the absorption row in the HUD, and `GuiIngame.renderPotionEffects`
//! (the top right icons of 1.12.2 are drawn by the inventory screen alone here).

use bevy::prelude::*;
use items::potion::{duration_string, Effect, PotionEffect};

use super::gui::draw::{Frame, BACKGROUND, TEXT};
use super::{Death, EffectIn, EffectOut};
use crate::link::Link;
use crate::player::Player;
use crate::players::Font;
use sim::lang::Lang;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, effects_in.before(crate::player::run_ticks).run_if(resource_exists::<Player>));
}

/// The frames about the session's own player into [`Player::effects`]; a death clears them.
fn effects_in(mut player: ResMut<Player>, mut put: MessageReader<EffectIn>, mut out: MessageReader<EffectOut>, mut death: MessageReader<Death>, link: Option<Res<Link>>) {
    let me = link.as_deref().map(|l| l.welcome.id);
    for EffectIn(e) in put.read() {
        if Some(e.id) != me {
            continue;
        }
        let Some(effect) = Effect::from_id(e.effect as i32) else { continue };
        // handleEntityEffect: the new effect replaces the old one outright.
        let mut pe = PotionEffect::new(effect, e.duration, e.amplifier as i32);
        pe.ambient = e.flags & proto::EntityEffect::AMBIENT != 0;
        pe.show_particles = e.flags & proto::EntityEffect::HIDE_PARTICLES == 0;
        player.effects.remove(effect);
        player.effects.add(pe);
    }
    for EffectOut(e) in out.read() {
        if Some(e.id) == me {
            if let Some(effect) = Effect::from_id(e.effect as i32) {
                player.effects.remove(effect);
            }
        }
    }
    if death.read().next().is_some() {
        player.effects.clear();
    }
}

/// `InventoryEffectRenderer.updateActivePotionEffects`: with effects on, the window sits at
/// `160 + (width - xSize - 200) / 2`.
pub fn shifted_left(width: i32, x_size: i32) -> i32 {
    160 + (width - x_size - 200) / 2
}

/// The y offset of each row of the list: 33 apart, or `132 / (n - 1)` past five effects.
pub fn row_offsets(n: usize) -> Vec<i32> {
    let step = if n > 5 { 132 / (n as i32 - 1) } else { 33 };
    (0..n as i32).map(|i| i * step).collect()
}

/// The level suffix `enchantment.level.2` to `.4` for an amplifier of 1 to 3.
pub fn level_suffix(amplifier: i32) -> Option<&'static str> {
    match amplifier {
        1 => Some("II"),
        2 => Some("III"),
        3 => Some("IV"),
        _ => None,
    }
}

/// The name line: the translated effect name and its level.
pub fn title(lang: &Lang, e: &PotionEffect) -> String {
    let key = e.effect.lang_key();
    let name = if lang.has_key(key) { lang.translate(key).to_string() } else { e.effect.display_name().to_string() };
    match level_suffix(e.amplifier) {
        Some(l) => format!("{name} {l}"),
        None => name,
    }
}

/// `InventoryEffectRenderer.drawActivePotionEffects`: for each effect (in `PotionEffect.compareTo`
/// order) the 140 x 32 panel from the inventory texture at (`left` - 124, `top` + offset), the
/// status icon, the name with its level and the time left.
pub fn draw_list(frame: &mut Frame, font: &Font, lang: &Lang, left: i32, top: i32, list: &[PotionEffect]) {
    let x = left - 124;
    for (e, dy) in list.iter().zip(row_offsets(list.len())) {
        let y = top + dy;
        frame.bands[BACKGROUND].sprite(x, y, 0, 166, 140, 32);
        if let Some(i) = e.effect.status_icon() {
            let i = i as i32;
            frame.bands[BACKGROUND].sprite(x + 6, y + 7, i % 8 * 18, 198 + i / 8 * 18, 18, 18);
        }
        frame.bands[TEXT].text(font, &title(lang, e), x + 10 + 18, y + 6, 0xFFFFFF, true, false);
        frame.bands[TEXT].text(font, &duration_string(e.duration), x + 10 + 18, y + 6 + 10, 0x7F7F7F, true, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_has_one_row_per_effect_and_squeezes_past_five() {
        assert_eq!(row_offsets(0), Vec::<i32>::new());
        assert_eq!(row_offsets(3), vec![0, 33, 66]);
        assert_eq!(row_offsets(5), vec![0, 33, 66, 99, 132]);
        // Six effects share 132 pixels: 132 / 5 = 26 apart.
        assert_eq!(row_offsets(6), vec![0, 26, 52, 78, 104, 130]);
    }

    #[test]
    fn levels_are_roman_from_the_second() {
        let lang = Lang::default();
        let mut e = PotionEffect::new(Effect::Speed, 600, 0);
        assert_eq!(title(&lang, &e), "Speed");
        e.amplifier = 1;
        assert_eq!(title(&lang, &e), "Speed II");
        e.amplifier = 3;
        assert_eq!(title(&lang, &e), "Speed IV");
        e.amplifier = 4;
        assert_eq!(title(&lang, &e), "Speed");
    }

    #[test]
    fn the_window_moves_right_while_effects_show() {
        assert_eq!(shifted_left(320, 176), 160 + (320 - 176 - 200) / 2);
    }

    #[test]
    fn a_frame_lays_out_a_panel_icon_and_two_lines_per_effect() {
        let mut frame = Frame::default();
        let font = Font { image: Handle::default(), widths: [6; 256] };
        let list = [PotionEffect::new(Effect::Speed, 1200, 1), PotionEffect::new(Effect::Regeneration, 100, 0)];
        draw_list(&mut frame, &font, &Lang::default(), 200, 40, &list);
        // Two panels and two icons in the background band.
        assert!(frame.bands[BACKGROUND].vertices() >= 4 * 4);
        assert!(frame.bands[TEXT].vertices() > 0);
    }

    #[test]
    fn frames_for_the_own_player_reach_the_effect_list_and_the_dig_context() {
        use crate::link::Welcome;
        use crate::survival::{Inbox, SurvivalPlugin};
        use bevy::math::DVec3;
        use crossbeam_channel::unbounded;
        use std::sync::{atomic::AtomicBool, Arc};
        let (out, _rx) = unbounded();
        let (_tx, inbox) = unbounded();
        let link = Link::over(out, inbox, Arc::new(AtomicBool::new(false)), Welcome { id: 3, ..Default::default() });
        let mut app = App::new();
        app.add_plugins(SurvivalPlugin).insert_resource(link).insert_resource(Player::new(DVec3::ZERO, 0.0, 0.0));
        let eff = |id: u32, effect: u8, amplifier: u8| proto::EntityEffect { id, effect, amplifier, duration: 600, flags: 0 };
        app.world_mut().resource_mut::<Inbox>().0.extend([
            // Haste (3) and Speed (1) for us; Slowness (2) for someone else.
            (proto::S_EFFECT, eff(3, 3, 1).encode()),
            (proto::S_EFFECT, eff(3, 1, 0).encode()),
            (proto::S_EFFECT, eff(9, 2, 0).encode()),
        ]);
        app.update();
        let p = app.world().resource::<Player>();
        assert_eq!(p.effects.len(), 2);
        assert_eq!(p.effects.amplifier(Effect::Haste), Some(1));
        assert!(p.effects.movement_speed_factor() > 1.1);
        app.world_mut().resource_mut::<Inbox>().0.push((proto::S_REMOVE_EFFECT, proto::RemoveEffect { id: 3, effect: 1 }.encode()));
        app.update();
        assert_eq!(app.world().resource::<Player>().effects.len(), 1);
        app.world_mut().resource_mut::<Inbox>().0.push((proto::S_DEATH, proto::Death { message: "x".into() }.encode()));
        app.update();
        assert!(app.world().resource::<Player>().effects.is_empty());
    }
}
