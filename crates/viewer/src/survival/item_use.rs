//! Using an item in the air: a right click with an item that has a use (`Item.onItemRightClick`).
//! Holding right click with an item whose `getMaxItemUseDuration` is above 0 (food when hungry or
//! always edible, a milk bucket, the bow) sends `V_USE_ITEM` use once at the press
//! (`PlayerControllerMP.processRightClick`, `CPacketPlayerTryUseItem`), and `V_USE_ITEM` release
//! when the button is let go or the held item changes (`Minecraft.processKeyBinds`:
//! `onStoppedUsingItem`, `CPacketPlayerDigging` RELEASE_USE_ITEM). A press with an instant item
//! (a worn piece of armor, a bucket) sends one use. While an item is in use the player is slowed
//! as `EntityPlayerSP.onLivingUpdate` slows it (`Player::using_item`, movement input times 0.2,
//! player.rs), and food and drink raise the item to the mouth (`Hand::in_use`, hand.rs).
//!
//! A food or drink that runs its 32 ticks out is finished by the server; the button still held
//! uses the next one after 4 ticks (`Minecraft.rightClickDelayTimer`).
//!
//! The server owns what the use does (server/src/game/use_item.rs).
//!
//! The bow draws as `ItemBow` does: the `pull` and `pulling` item properties pick the
//! `bow_pulling_N` model (item.rs `pulling_key`) and `renderItemInFirstPerson`'s BOW case pulls
//! the item back and shakes it (hand.rs `bow_matrix`). Other players do not see the draw: the
//! server sends no `HAND_STATES` (`ArmPose.BOW_AND_ARROW` has nothing to read).
//!
//! DROPPED: the FOV zoom of `EntityPlayerSP.getFovModifier`; the off hand's air use (food, the
//! bow, a bucket: `ItemUse` tracks one hand, the server's `V_USE_ITEM` already reads the hand);
//! the item crack particles of eating (the particle
//! system has no item-crack particle); a block under the crosshair that takes the click first
//! (`Block.onBlockActivated`) does not stop the use.

use bevy::prelude::*;
use items::food;
use items::ids::item::{ARROW, BOW, BUCKET, LAVA_BUCKET, MILK_BUCKET, SPECTRAL_ARROW, TIPPED_ARROW, WATER_BUCKET};
use items::{ItemStack, Kind};

use crate::hand::{Hand, InUse};
use crate::hotbar::Inventory;
use crate::interact::Controller;
use crate::link::Link;
use crate::player::Player;
use crate::settings::Menu;
use crate::sky::Clock;

/// `ItemBow.getMaxItemUseDuration`
const BOW_DURATION: i32 = 72000;
/// `Minecraft.rightClickDelayTimer`
const RIGHT_CLICK_DELAY: i32 = 4;

/// The pressed state of the item in use.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct ItemUse {
    /// `EntityPlayerSP.isHandActive`
    pub using: bool,
    /// The item being used, to see it change.
    item: u16,
    /// `getMaxItemUseDuration` of it.
    pub max: i32,
    /// `getItemInUseCount`
    pub remaining: i32,
    /// An item ran its duration out with the button still down.
    finished: bool,
    /// Ticks until a held button uses again.
    delay: i32,
}

/// What the item use reads each frame.
#[derive(Clone, Copy, Debug)]
pub struct UseInput {
    pub press: bool,
    pub held_down: bool,
    /// The held item.
    pub held: ItemStack,
    /// `getMaxItemUseDuration` if the item can be used now (food when it can be eaten, a bow with
    /// arrows), else 0.
    pub duration: i32,
    /// A worn piece or a bucket: one use per press.
    pub instant: bool,
    /// A screen has the mouse, the player is dead, or the game mode is spectator.
    pub blocked: bool,
    /// An entity is under the crosshair (the click is an interaction, not an item use).
    pub entity: bool,
    /// Client ticks since the last frame.
    pub ticks: i32,
}

fn msg(phase: u8) -> Option<proto::UseItem> {
    Some(proto::UseItem { hand: proto::HAND_MAIN, phase })
}

impl ItemUse {
    /// One frame: the `V_USE_ITEM` to send, if any.
    pub fn update(&mut self, i: UseInput) -> Option<proto::UseItem> {
        if !i.held_down {
            self.finished = false;
        }
        if self.using {
            self.remaining -= i.ticks;
            if !i.held_down || i.blocked || i.held.get_item() != self.item {
                self.using = false;
                self.remaining = 0;
                return msg(proto::USE_RELEASE);
            }
            if self.remaining <= 0 && self.max < BOW_DURATION {
                // The server finished it; nothing is sent.
                self.using = false;
                self.remaining = 0;
                self.finished = true;
                self.delay = RIGHT_CLICK_DELAY;
            }
            return None;
        }
        self.delay = (self.delay - i.ticks).max(0);
        if i.blocked || i.entity || i.held.is_empty() {
            return None;
        }
        if i.instant {
            return if i.press { msg(proto::USE_START) } else { None };
        }
        if i.duration > 0 && (i.press || (i.held_down && self.finished && self.delay == 0)) {
            self.using = true;
            self.finished = false;
            self.item = i.held.get_item();
            self.max = i.duration;
            self.remaining = i.duration;
            return msg(proto::USE_START);
        }
        None
    }
}

fn is_arrow(s: &ItemStack) -> bool {
    matches!(s.get_item(), ARROW | SPECTRAL_ARROW | TIPPED_ARROW)
}

/// `ItemStack.getMaxItemUseDuration` where the use would start: `ItemFood.onItemRightClick`
/// (`canEat`), `ItemBucketMilk`, `ItemBow.onItemRightClick` (ammo or creative). 0 otherwise.
pub fn use_duration(held: &ItemStack, food_level: u8, mode: u8, has_arrow: bool) -> i32 {
    if held.is_empty() || mode == proto::GM_SPECTATOR {
        return 0;
    }
    let creative = mode == proto::GM_CREATIVE;
    match held.get_item() {
        BOW if has_arrow || creative => BOW_DURATION,
        MILK_BUCKET => 32,
        it if food::food(it).is_some() => {
            let always = food::food(it).is_some_and(|f| f.always_edible);
            // EntityPlayer.canEat
            // CHANGED: creative eats at any hunger, by request.
            if always || food_level < 20 || creative {
                food::max_item_use_duration(held)
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// An item with a use in the air that ends at once: `ItemArmor` and `ItemBucket`.
pub fn is_instant(held: &ItemStack) -> bool {
    !held.is_empty() && (matches!(held.get_item(), BUCKET | WATER_BUCKET | LAVA_BUCKET) || matches!(held.def().kind, Kind::Armor(..)))
}

#[allow(clippy::too_many_arguments)]
fn run(
    mouse: Res<ButtonInput<MouseButton>>,
    inv: Res<Inventory>,
    ctl: Res<Controller>,
    menu: Res<Menu>,
    state: Res<super::SurvivalState>,
    clock: Res<Clock>,
    link: Option<Res<Link>>,
    mut player: ResMut<Player>,
    mut hand: ResMut<Hand>,
    mut using: ResMut<ItemUse>,
) {
    let held = inv.get_current_item();
    let has_arrow = (0..inv.get_size_inventory()).any(|i| is_arrow(&inv.get_stack_in_slot(i)));
    let input = UseInput {
        press: mouse.just_pressed(MouseButton::Right),
        held_down: mouse.pressed(MouseButton::Right),
        held,
        duration: use_duration(&held, state.food, player.game_mode, has_arrow),
        instant: is_instant(&held),
        blocked: menu.open || state.dead || player.game_mode == proto::GM_SPECTATOR,
        entity: ctl.entity.is_some(),
        ticks: clock.ticks_this_frame as i32,
    };
    if let Some(m) = using.update(input) {
        if let Some(link) = &link {
            super::net::send_use_item(link, &m);
        }
    }
    if player.using_item != using.using {
        player.using_item = using.using;
    }
    // ItemRenderer: EAT and DRINK (a duration of 32) raise the item.
    let in_use = InUse { eating: using.using && using.max < BOW_DURATION, bow: using.using && using.max >= BOW_DURATION, count: if using.using { using.remaining.max(0) } else { 0 }, max: using.max };
    if hand.in_use != in_use {
        hand.in_use = in_use;
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ItemUse>().add_systems(
        Update,
        run.before(crate::player::run_ticks)
            .run_if(resource_exists::<Inventory>)
            .run_if(resource_exists::<Controller>)
            .run_if(resource_exists::<Menu>)
            .run_if(resource_exists::<ButtonInput<MouseButton>>)
            .run_if(resource_exists::<Clock>)
            .run_if(resource_exists::<Hand>)
            .run_if(resource_exists::<Player>),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::ids::item;

    fn held(it: u16) -> ItemStack {
        ItemStack::of(it)
    }

    fn input(it: u16, duration: i32) -> UseInput {
        UseInput { press: false, held_down: false, held: held(it), duration, instant: is_instant(&held(it)), blocked: false, entity: false, ticks: 1 }
    }

    const START: Option<proto::UseItem> = Some(proto::UseItem { hand: proto::HAND_MAIN, phase: proto::USE_START });
    const RELEASE: Option<proto::UseItem> = Some(proto::UseItem { hand: proto::HAND_MAIN, phase: proto::USE_RELEASE });

    #[test]
    fn press_sends_use_once_and_release_sends_release() {
        let mut b = ItemUse::default();
        let idle = input(item::BOW, BOW_DURATION);
        let start = b.update(UseInput { press: true, held_down: true, ..idle });
        assert_eq!(start, START);
        assert_eq!(start.unwrap().encode(), [0, 0]);
        assert!(b.using);
        for _ in 0..5 {
            assert_eq!(b.update(UseInput { held_down: true, ..idle }), None);
        }
        let end = b.update(idle);
        assert_eq!(end, RELEASE);
        assert_eq!(end.unwrap().encode(), [0, 1]);
        assert!(!b.using);
        assert_eq!(b.update(idle), None);
    }

    #[test]
    fn only_a_usable_item_with_no_screen_starts_a_use() {
        let press = UseInput { press: true, held_down: true, ..input(item::BOW, BOW_DURATION) };
        assert_eq!(ItemUse::default().update(UseInput { duration: 0, ..press }), None);
        assert_eq!(ItemUse::default().update(UseInput { blocked: true, ..press }), None);
        assert_eq!(ItemUse::default().update(UseInput { entity: true, ..press }), None);
        // Switching away from the item mid use releases it.
        let mut b = ItemUse::default();
        b.update(press);
        let r = b.update(UseInput { held_down: true, held: held(item::APPLE), ..press });
        assert_eq!(r.map(|m| m.phase), Some(proto::USE_RELEASE));
    }

    #[test]
    fn food_held_with_right_click_sends_use_then_release() {
        let mut b = ItemUse::default();
        let bread = input(item::BREAD, 32);
        assert_eq!(b.update(UseInput { press: true, held_down: true, ..bread }), START);
        for _ in 0..10 {
            assert_eq!(b.update(UseInput { held_down: true, ..bread }), None);
        }
        assert!(b.using);
        assert_eq!(b.remaining, 32 - 10);
        assert_eq!(b.update(bread), RELEASE);
    }

    #[test]
    fn a_finished_food_uses_the_next_one_while_the_button_is_held() {
        let mut b = ItemUse::default();
        let bread = input(item::BREAD, 32);
        b.update(UseInput { press: true, held_down: true, ..bread });
        // 32 ticks run out: nothing is sent, the server finished the bread.
        let r = b.update(UseInput { held_down: true, ticks: 32, ..bread });
        assert_eq!(r, None);
        assert!(!b.using);
        // The 4 tick delay, then the held button starts again.
        for _ in 0..3 {
            assert_eq!(b.update(UseInput { held_down: true, ..bread }), None);
        }
        assert_eq!(b.update(UseInput { held_down: true, ..bread }), START);
    }

    #[test]
    fn a_bucket_click_sends_one_use() {
        let mut b = ItemUse::default();
        let bucket = input(item::WATER_BUCKET, 0);
        assert_eq!(b.update(UseInput { press: true, held_down: true, ..bucket }), START);
        assert!(!b.using, "an instant item is not held");
        assert_eq!(b.update(UseInput { held_down: true, ..bucket }), None);
        assert_eq!(b.update(bucket), None);
        assert_eq!(b.update(UseInput { press: true, held_down: true, ..bucket }), START);
    }

    #[test]
    fn what_can_be_used() {
        let s = proto::GM_SURVIVAL;
        assert_eq!(use_duration(&held(item::BREAD), 19, s, false), 32);
        assert_eq!(use_duration(&held(item::BREAD), 20, s, false), 0);
        assert_eq!(use_duration(&held(item::GOLDEN_APPLE), 20, s, false), 32);
        assert_eq!(use_duration(&held(item::MILK_BUCKET), 20, s, false), 32);
        assert_eq!(use_duration(&held(item::BOW), 20, s, false), 0);
        assert_eq!(use_duration(&held(item::BOW), 20, s, true), BOW_DURATION);
        // CHANGED: creative eats at any hunger, by request.
        assert_eq!(use_duration(&held(item::BREAD), 20, proto::GM_CREATIVE, false), 32);
        assert_eq!(use_duration(&held(item::BREAD), 5, proto::GM_CREATIVE, false), 32);
    }

    #[test]
    fn eating_raises_the_held_item() {
        // f1 = 1 at the start: the item has not moved. Near the end it is at the mouth.
        let start = crate::hand::eat_matrix(&InUse { eating: true, bow: false, count: 32, max: 32 }, 1.0, true);
        let end = crate::hand::eat_matrix(&InUse { eating: true, bow: false, count: 4, max: 32 }, 0.0, true);
        assert!(start.abs_diff_eq(Mat4::IDENTITY, 1e-4));
        assert!(!end.abs_diff_eq(Mat4::IDENTITY, 0.1));
    }
}
