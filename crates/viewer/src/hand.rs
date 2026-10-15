//! The first-person arm and the crosshair: `ItemRenderer.renderItemInFirstPerson` with an empty
//! main hand (the arm alone), the swing, equip and sway state it reads, and `GuiIngame`'s
//! crosshair and attack indicator.
//!
//! The hand camera sits at the origin with no rotation, so its view space is vanilla's eye space
//! after `renderHand`'s `loadIdentity`, and the arm's transform is the GL matrix chain itself.

use bevy::asset::RenderAssetUsages;
use bevy::math::DVec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use worldgen::math_helper::{cos, sin, sqrt_f};

use crate::env::{self, Env};
use items::ItemStack;

use crate::interact::Controller;
use crate::player::Player;
use crate::sky::{neighbor_light, Clock, View};
use crate::stream::Streamer;

/// `EntityLivingBase.getArmSwingAnimationEnd` without haste or mining fatigue.
const SWING_END: i32 = 6;
/// The base value of `SharedMonsterAttributes.ATTACK_SPEED`.
const BASE_ATTACK_SPEED: f64 = 4.0;

/// `EntityLivingBase.activeItemStackUseCount` of the item in the main hand, for the first person
/// pose (`ItemRenderer.renderItemInFirstPerson`); survival/item_use.rs keeps it.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct InUse {
    /// `EnumAction.EAT` or `DRINK`: the item is raised to the mouth.
    pub eating: bool,
    /// `EnumAction.BOW`: the bow is drawn (`ItemRenderer.renderItemInFirstPerson`).
    pub bow: bool,
    /// `getItemInUseCount`: ticks left, 0 when no item is in use.
    pub count: i32,
    /// `getMaxItemUseDuration`
    pub max: i32,
}

/// Whether a slot's stack is still the stack the hand shows, the identity half of
/// `ForgeHooksClient.shouldCauseReequipAnimation` with `Item.shouldCauseReequipAnimation`
/// (`!oldStack.equals(newStack)`, Java's object identity). A use (`shrink`, `damageItem`) or NBT
/// write mutates the stack in place, so its identity holds through a count, wear or tag change and
/// the hand does not dip; only a different item or subtype is a different stack. The count is
/// always ignored, the damage only for a damageable item (where it is the wear), and the tag and
/// enchantments never count.
pub fn same_stack(shown: &ItemStack, held: &ItemStack) -> bool {
    let mut a = *shown;
    a.count = held.count;
    a.tag = held.tag;
    a.ench = held.ench;
    if a.is_item_stack_damageable() {
        a.damage = held.damage;
    }
    a == *held
}

/// The player state the arm is drawn from.
#[derive(Resource, Default)]
pub struct Hand {
    /// `isHandActive` with the item in use.
    pub in_use: InUse,
    /// `EntityPlayerSP.prevRenderArmPitch`, `renderArmPitch`, `prevRenderArmYaw`, `renderArmYaw`
    pub prev_arm_pitch: f32,
    pub arm_pitch: f32,
    pub prev_arm_yaw: f32,
    pub arm_yaw: f32,
    /// `EntityLivingBase.prevSwingProgress`, `swingProgress`, `swingProgressInt`,
    /// `isSwingInProgress`
    pub prev_swing: f32,
    pub swing: f32,
    pub swing_int: i32,
    pub swinging: bool,
    /// `EntityLivingBase.ticksSinceLastSwing`
    pub ticks_since_last_swing: i32,
    /// `ItemRenderer.prevEquippedProgressMainHand`, `equippedProgressMainHand`
    pub prev_equipped: f32,
    pub equipped: f32,
    /// `ItemRenderer.prevEquippedProgressOffHand`, `equippedProgressOffHand`
    pub prev_equipped_off: f32,
    pub equipped_off: f32,
    /// `EntityLivingBase.swingingHand`: which hand the one swing plays on ([`proto::HAND_MAIN`] or
    /// [`proto::HAND_OFF`]).
    pub swinging_hand: u8,
    /// Attack presses a tick has not handled yet (`KeyBinding.pressTime`).
    pub clicks: u32,
    /// `Minecraft.leftClickCounter`: set when the mouse is captured, so the capturing click does
    /// not swing; cleared when the attack key is up.
    pub left_click_counter: i32,
    /// `ItemRenderer.itemStackMainHand`: the stack drawn, which follows the held one while the
    /// hand is down.
    pub shown: ItemStack,
    /// The hotbar slot `shown` was taken from, half of the stand-in for its identity.
    pub shown_slot: usize,
    /// `EntityPlayer.itemStackMainHand`: the copy `EntityPlayer.onUpdate` keeps to notice a held
    /// item change, which restarts the attack cooldown.
    pub main_hand: ItemStack,
    /// `ItemRenderer.itemStackOffHand`: the off hand's drawn stack.
    pub shown_off: ItemStack,
    /// The sway starts from the first view instead of from 0, so the arm does not swing in.
    started: bool,
    /// The Haste and Mining Fatigue levels (amplifier + 1, 0 when absent) and the held item's
    /// attack speed modifier, which `getArmSwingAnimationEnd` and `getCooldownPeriod` read.
    pub haste: i32,
    pub fatigue: i32,
    pub speed_mod: f64,
    /// Counts started swings (wrapping), for other players' copies of this one (link.rs).
    pub swings: u8,
}

impl Hand {
    /// Reads the attributes `getCooldownPeriod` and `getArmSwingAnimationEnd` depend on: the held
    /// item's attack speed modifier and the Haste and Mining Fatigue effects.
    pub fn set_attack_stats(&mut self, held: &ItemStack, effects: &items::potion::Effects) {
        use items::potion::Effect;
        self.speed_mod = if held.is_empty() { 0.0 } else { items::tools::attack_modifiers(held.get_item()).map_or(0.0, |m| m.1) };
        self.haste = effects.amplifier(Effect::Haste).map_or(0, |a| a + 1);
        self.fatigue = effects.amplifier(Effect::MiningFatigue).map_or(0, |a| a + 1);
    }

    /// `EntityLivingBase.getArmSwingAnimationEnd`
    pub fn swing_end(&self) -> i32 {
        if self.haste > 0 {
            SWING_END - self.haste
        } else if self.fatigue > 0 {
            SWING_END + self.fatigue * 2
        } else {
            SWING_END
        }
    }

    /// `EntityPlayer.getCooldownPeriod`: `1 / attackSpeed * 20`, the attribute being the base 4
    /// plus the item's modifier, times the operation 2 modifiers of Haste and Mining Fatigue.
    pub fn cooldown_period(&self) -> f32 {
        let mut factor = 1.0f64;
        if self.haste > 0 {
            factor *= 1.0 + 0.100_000_001_490_116_12 * self.haste as f64;
        }
        if self.fatigue > 0 {
            factor *= 1.0 + -0.100_000_001_490_116_12 * self.fatigue as f64;
        }
        (1.0 / ((BASE_ATTACK_SPEED + self.speed_mod) * factor) * 20.0) as f32
    }

    /// `EntityPlayer.getCooledAttackStrength`
    pub fn cooled_attack_strength(&self, adjust: f32) -> f32 {
        ((self.ticks_since_last_swing as f32 + adjust) / self.cooldown_period()).clamp(0.0, 1.0)
    }

    /// `EntityLivingBase.swingArm`
    pub(crate) fn swing_arm(&mut self, hand: u8) {
        if !self.swinging || self.swing_int >= self.swing_end() / 2 || self.swing_int < 0 {
            self.swing_int = -1;
            self.swinging = true;
            self.swinging_hand = hand;
            self.swings = self.swings.wrapping_add(1);
        }
    }

    /// The swing progress the hand `right` draws with: `EntityLivingBase.getSwingProgress` for the
    /// hand `swingingHand` names (`ItemRenderer.renderItemInFirstPerson`'s `f3` and `f4`), 0 for the
    /// other hand.
    pub fn swing_progress_for(&self, right: bool, partial: f32) -> f32 {
        let mine = if right { proto::HAND_MAIN } else { proto::HAND_OFF };
        if self.swinging_hand == mine {
            self.swing_progress(partial)
        } else {
            0.0
        }
    }

    /// `EntityLivingBase.updateArmSwingProgress`
    fn update_arm_swing_progress(&mut self) {
        if self.swinging {
            self.swing_int += 1;
            if self.swing_int >= self.swing_end() {
                self.swing_int = 0;
                self.swinging = false;
            }
        } else {
            self.swing_int = 0;
        }
        self.swing = self.swing_int as f32 / self.swing_end() as f32;
    }

    /// `EntityLivingBase.getSwingProgress`
    pub fn swing_progress(&self, partial: f32) -> f32 {
        let mut f = self.swing - self.prev_swing;
        if f < 0.0 {
            f += 1.0;
        }
        self.prev_swing + f * partial
    }

    /// The sway starts from the first view instead of from 0, so the arm does not swing in.
    pub fn start(&mut self, pitch: f32, yaw: f32) {
        if !self.started {
            self.started = true;
            (self.prev_arm_pitch, self.arm_pitch) = (pitch, pitch);
            (self.prev_arm_yaw, self.arm_yaw) = (yaw, yaw);
        }
    }

    /// `EntityPlayer.resetCooldown`
    pub fn reset_cooldown(&mut self) {
        self.ticks_since_last_swing = 0;
    }

    /// The `EntityPlayer.onUpdate` block that restarts the cooldown when the held item changes: a
    /// different item or subtype (not a count, wear or tag change, which `areItemsEqualIgnoreDurability`
    /// sees through) resets `ticksSinceLastSwing`.
    pub fn update_main_hand(&mut self, held: ItemStack) {
        if !items::stack::are_item_stacks_equal(&self.main_hand, &held) {
            if !items::stack::are_items_equal_ignore_durability(&self.main_hand, &held) {
                self.reset_cooldown();
            }
            self.main_hand = held;
        }
    }

    /// `Minecraft.runTick`, after `runTickMouse`.
    pub fn begin_tick(&mut self) {
        if self.left_click_counter > 0 {
            self.left_click_counter -= 1;
        }
    }

    /// `ItemRenderer.updateEquippedItem` for the main hand, with
    /// `ForgeHooksClient.shouldCauseReequipAnimation` and `Item.shouldCauseReequipAnimation`: the
    /// hand re-equips (drops and rises) only when one side is empty and the other is not, when the
    /// hotbar slot changed, or when the item or subtype changed (see [`same_stack`]); a count,
    /// durability or NBT change never does. `ItemStack` is a value here, so the identity is the
    /// slot and the value: two empty hands are the one `ItemStack.EMPTY`, and a stack is the same
    /// stack while it stays in the same slot unchanged.
    pub fn update_equipped(&mut self, held: ItemStack, slot: usize) {
        self.prev_equipped = self.equipped;
        let f = self.cooled_attack_strength(1.0);
        // ForgeHooksClient.shouldCauseReequipAnimation: both empty, no change; one empty, a change.
        let same = (self.shown.is_empty() && held.is_empty()) || (self.shown_slot == slot && same_stack(&self.shown, &held));
        if same {
            // Java's `itemStackMainHand` is the very object the slot holds, so it follows the
            // count, the wear and the tag as they change.
            self.shown = held;
        }
        self.equipped += ((if same { f * f * f } else { 0.0 }) - self.equipped).clamp(-0.4, 0.4);
        if self.equipped < 0.1 {
            self.shown = held;
            self.shown_slot = slot;
        }
    }

    /// `ItemRenderer.updateEquippedItem` for the off hand: the target is 1 where the main hand's is
    /// the cooled attack strength cubed.
    pub fn update_equipped_off(&mut self, held: ItemStack) {
        self.prev_equipped_off = self.equipped_off;
        let same = (self.shown_off.is_empty() && held.is_empty()) || same_stack(&self.shown_off, &held);
        if same {
            self.shown_off = held;
        }
        self.equipped_off += ((if same { 1.0 } else { 0.0 }) - self.equipped_off).clamp(-0.4, 0.4);
        if self.equipped_off < 0.1 {
            self.shown_off = held;
        }
    }

    /// `ItemRenderer.resetEquippedProgress`: the named hand drops to the bottom at once.
    pub fn reset_equipped(&mut self, hand: u8) {
        if hand == proto::HAND_OFF {
            self.equipped_off = 0.0;
        } else {
            self.equipped = 0.0;
        }
    }

    /// The player's share of the tick: `EntityPlayer.onUpdate`, the swing, and the arm sway.
    pub fn end_tick(&mut self, pitch: f32, yaw: f32) {
        // EntityPlayer.onUpdate
        self.ticks_since_last_swing += 1;
        // Entity.onEntityUpdate, then EntityPlayer.updateEntityActionState.
        self.prev_swing = self.swing;
        self.update_arm_swing_progress();
        // EntityPlayerSP.onUpdate
        self.prev_arm_yaw = self.arm_yaw;
        self.prev_arm_pitch = self.arm_pitch;
        self.arm_pitch = (self.arm_pitch as f64 + (pitch - self.arm_pitch) as f64 * 0.5) as f32;
        self.arm_yaw = (self.arm_yaw as f64 + (yaw - self.arm_yaw) as f64 * 0.5) as f32;
    }

    /// A tick with an empty hand where every attack press is a MISS (`clickMouse` resets the
    /// cooldown and swings).
    #[cfg(test)]
    pub fn tick(&mut self, pitch: f32, yaw: f32, attack_held: bool) {
        self.begin_tick();
        while self.clicks > 0 {
            self.clicks -= 1;
            if self.left_click_counter <= 0 {
                self.reset_cooldown();
                self.swing_arm(proto::HAND_MAIN);
            }
        }
        if !attack_held {
            self.left_click_counter = 0;
        }
        self.update_equipped(ItemStack::EMPTY, 0);
        self.end_tick(pitch, yaw);
    }
}

/// F1: `GameSettings.hideGUI`, which hides the hand, the crosshair and the stats.
#[derive(Resource, Default)]
pub struct HideGui(pub bool);

/// F1 toggles `hideGUI`.
pub fn toggle_gui(keys: Res<ButtonInput<KeyCode>>, mut hide: ResMut<HideGui>) {
    if keys.just_pressed(KeyCode::F1) {
        hide.0 = !hide.0;
    }
}

/// The arm's model matrix in eye space: `ItemRenderer.rotateArm`, `renderArmFirstPerson` for the
/// right side, and `ModelRenderer.render(0.0625)` of `bipedRightArm` as
/// `RenderPlayer.renderRightArm` poses it. All degrees, as `GlStateManager.rotate` takes them.
pub fn arm_matrix(hand: &Hand, pitch: f32, yaw: f32, partial: f32) -> Mat4 {
    let rx = |d: f32| Mat4::from_rotation_x(d.to_radians());
    let ry = |d: f32| Mat4::from_rotation_y(d.to_radians());
    let rz = |d: f32| Mat4::from_rotation_z(d.to_radians());
    let t = |x: f32, y: f32, z: f32| Mat4::from_translation(Vec3::new(x, y, z));

    let mut m = rotate_arm(hand, pitch, yaw, partial);

    // renderItemInFirstPerson: the swing plays on `swingingHand`; the equip progress is inverted.
    let p2 = hand.swing_progress_for(true, partial);
    let p1 = 1.0 - (hand.prev_equipped + (hand.equipped - hand.prev_equipped) * partial);

    // renderArmFirstPerson, EnumHandSide.RIGHT.
    let f = 1.0f32;
    let f1 = sqrt_f(p2);
    let f2 = -0.3 * sin(f1 * 3.141_592_7);
    let f3 = 0.4 * sin(f1 * 6.283_185_5);
    let f4 = -0.4 * sin(p2 * 3.141_592_7);
    m = m * t(f * (f2 + 0.640_000_05), f3 + -0.6 + p1 * -0.6, f4 + -0.719_999_97);
    m = m * ry(f * 45.0);
    let f5 = sin(p2 * p2 * 3.141_592_7);
    let f6 = sin(f1 * 3.141_592_7);
    m = m * ry(f * f6 * 70.0);
    m = m * rz(f * f5 * -20.0);
    m = m * t(f * -1.0, 3.6, 3.5);
    m = m * rz(f * 120.0);
    m = m * rx(200.0);
    m = m * ry(f * -135.0);
    m = m * t(f * 5.6, 0.0, 0.0);

    // ModelRenderer.render: setRotationAngles with no motion and ageInTicks 0 leaves the right
    // arm at rotateAngleZ = cos(0) * 0.05 + 0.05 about the point (-5, 2, 0); renderRightArm zeroes
    // rotateAngleX, and Y is zero. The armwear copies the same angles and point.
    let scale = 0.0625;
    m = m * t(-5.0 * scale, 2.0 * scale, 0.0);
    m = m * rz(0.1 * 57.295_776);
    m * Mat4::from_scale(Vec3::splat(scale))
}

/// `ItemRenderer.rotateArm`: the sway of the arm behind the view.
fn rotate_arm(hand: &Hand, pitch: f32, yaw: f32, partial: f32) -> Mat4 {
    let f = hand.prev_arm_pitch + (hand.arm_pitch - hand.prev_arm_pitch) * partial;
    let f1 = hand.prev_arm_yaw + (hand.arm_yaw - hand.prev_arm_yaw) * partial;
    Mat4::from_rotation_x(((pitch - f) * 0.1).to_radians()) * Mat4::from_rotation_y(((yaw - f1) * 0.1).to_radians())
}

/// The held item's model matrix in eye space: `rotateArm`, then `renderItemInFirstPerson` for a
/// stack in one hand (`transformSideFirstPerson`, `transformFirstPerson`), the model's
/// `firstperson_righthand` or `firstperson_lefthand` transform, and `RenderItem.renderItem`'s
/// centering. `right` is the side (`EnumHandSide`); the off hand is the left.
pub fn held_matrix(hand: &Hand, right: bool, pitch: f32, yaw: f32, partial: f32, display: Mat4) -> Mat4 {
    let rx = |d: f32| Mat4::from_rotation_x(d.to_radians());
    let ry = |d: f32| Mat4::from_rotation_y(d.to_radians());
    let rz = |d: f32| Mat4::from_rotation_z(d.to_radians());
    let t = |x: f32, y: f32, z: f32| Mat4::from_translation(Vec3::new(x, y, z));
    let i = if right { 1.0 } else { -1.0 };

    let mut m = rotate_arm(hand, pitch, yaw, partial);
    let s = hand.swing_progress_for(right, partial);
    let (prev_eq, eq) = if right { (hand.prev_equipped, hand.equipped) } else { (hand.prev_equipped_off, hand.equipped_off) };
    let p = 1.0 - (prev_eq + (eq - prev_eq) * partial);
    // `renderItemInFirstPerson`: the use pose is for `getActiveHand() == hand`; this port's
    // `in_use` is the main hand's, so only the right hand takes these branches.
    if right && hand.in_use.eating && hand.in_use.count > 0 {
        // renderItemInFirstPerson, EAT and DRINK: transformEatFirstPerson, then
        // transformSideFirstPerson; the swing and transformFirstPerson are the other branch.
        m = m * eat_matrix(&hand.in_use, partial, right);
        m = m * t(i * 0.56, -0.52 + p * -0.6, -0.72);
        return m * display * t(-0.5, -0.5, -0.5);
    }
    if right && hand.in_use.bow && hand.in_use.count > 0 {
        // renderItemInFirstPerson, BOW: transformSideFirstPerson, then the draw.
        m = m * t(i * 0.56, -0.52 + p * -0.6, -0.72);
        m = m * bow_matrix(&hand.in_use, partial, right);
        return m * display * t(-0.5, -0.5, -0.5);
    }
    let f = -0.4 * sin(sqrt_f(s) * 3.141_592_7);
    let f1 = 0.2 * sin(sqrt_f(s) * 6.283_185_5);
    let f2 = -0.2 * sin(s * 3.141_592_7);
    m = m * t(i * f, f1, f2);
    // transformSideFirstPerson
    m = m * t(i * 0.56, -0.52 + p * -0.6, -0.72);
    // transformFirstPerson
    let f = sin(s * s * 3.141_592_7);
    m = m * ry(i * (45.0 + f * -20.0));
    let f1 = sin(sqrt_f(s) * 3.141_592_7);
    m = m * rz(i * f1 * -20.0);
    m = m * rx(f1 * -80.0);
    m = m * ry(i * -45.0);
    m * display * t(-0.5, -0.5, -0.5)
}

/// The `BOW` case of `ItemRenderer.renderItemInFirstPerson` after `transformSideFirstPerson`: the
/// fixed tilt, the shake once the draw passes a tenth, the pull back along z and the stretch. The
/// side `j` mirrors the x translation and the y and z rotations.
pub fn bow_matrix(u: &InUse, partial: f32, right: bool) -> Mat4 {
    let j = if right { 1.0 } else { -1.0 };
    let mut m = Mat4::from_translation(Vec3::new(j * -0.278_568_2, 0.183_443_87, 0.157_315_31));
    m *= Mat4::from_rotation_x((-13.935f32).to_radians());
    m *= Mat4::from_rotation_y(j * 35.3f32.to_radians());
    m *= Mat4::from_rotation_z(j * -9.785f32.to_radians());
    let f5 = u.max as f32 - (u.count as f32 - partial + 1.0);
    let mut f6 = f5 / 20.0;
    f6 = (f6 * f6 + f6 * 2.0) / 3.0;
    if f6 > 1.0 {
        f6 = 1.0;
    }
    if f6 > 0.1 {
        let f7 = sin((f5 - 0.1) * 1.3);
        let f3 = f6 - 0.1;
        let f4 = f7 * f3;
        m *= Mat4::from_translation(Vec3::new(f4 * 0.0, f4 * 0.004, f4 * 0.0));
    }
    m *= Mat4::from_translation(Vec3::new(f6 * 0.0, f6 * 0.0, f6 * 0.04));
    m *= Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0 + f6 * 0.2));
    m * Mat4::from_axis_angle(Vec3::NEG_Y, j * 45.0f32.to_radians())
}

/// `ItemRenderer.transformEatFirstPerson`: the side `i` mirrors the x translation and the y and z
/// rotations.
pub fn eat_matrix(u: &InUse, partial: f32, right: bool) -> Mat4 {
    let i = if right { 1.0 } else { -1.0 };
    let f = u.count as f32 - partial + 1.0;
    let f1 = f / u.max as f32;
    let mut m = Mat4::IDENTITY;
    if f1 < 0.8 {
        let f2 = (cos(f / 4.0 * 3.141_592_7) * 0.1).abs();
        m *= Mat4::from_translation(Vec3::new(0.0, f2, 0.0));
    }
    let f3 = 1.0 - (f1 as f64).powf(27.0) as f32;
    m *= Mat4::from_translation(Vec3::new(i * f3 * 0.6, f3 * -0.5, 0.0));
    m *= Mat4::from_rotation_y((i * f3 * 90.0).to_radians());
    m *= Mat4::from_rotation_x((f3 * 10.0).to_radians());
    m * Mat4::from_rotation_z((i * f3 * 30.0).to_radians())
}

/// `RenderHelper.LIGHT0_POS` and `LIGHT1_POS`.
pub fn standard_lights() -> [Vec3; 2] {
    let l0 = DVec3::new(0.200_000_002_980_232_24, 1.0, -0.699_999_988_079_071).normalize().as_vec3();
    let l1 = DVec3::new(-0.200_000_002_980_232_24, 1.0, 0.699_999_988_079_071).normalize().as_vec3();
    [l0, l1]
}

/// `RenderHelper.enableStandardItemLighting`'s light directions under
/// `ItemRenderer.rotateArroundXAndY(pitch, yaw)`, in eye space.
pub fn item_lights(pitch: f32, yaw: f32) -> [Vec3; 2] {
    let [l0, l1] = standard_lights();
    let m = Mat3::from_rotation_x(pitch.to_radians()) * Mat3::from_rotation_y(yaw.to_radians());
    [m * l0, m * l1]
}

#[derive(Default)]
pub(crate) struct BoxMesh {
    pub(crate) pos: Vec<[f32; 3]>,
    pub(crate) normal: Vec<[f32; 3]>,
    pub(crate) uv: Vec<[f32; 2]>,
    pub(crate) idx: Vec<u32>,
}

impl BoxMesh {
    /// `ModelBox` and its six `TexturedQuad`s on a 64 x 64 texture, unmirrored.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn model_box(&mut self, u: i32, v: i32, x: f32, y: f32, z: f32, dx: i32, dy: i32, dz: i32, delta: f32) {
        self.model_box_on(64.0, 64.0, u, v, x, y, z, dx, dy, dz, delta);
    }

    /// [`Self::model_box`] on a `tw` x `th` texture (`ModelBase.textureWidth` and `textureHeight`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn model_box_on(&mut self, tw: f32, th: f32, u: i32, v: i32, x: f32, y: f32, z: f32, dx: i32, dy: i32, dz: i32, delta: f32) {
        self.model_box_mirrored(tw, th, u, v, x, y, z, dx, dy, dz, delta, false);
    }

    /// [`Self::model_box_on`] of a `ModelRenderer` with `mirror` set: the box's x extent swapped and
    /// every quad `flipFace`d (its vertices reversed with their texture coordinates).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn model_box_mirrored(&mut self, tw: f32, th: f32, u: i32, v: i32, x: f32, y: f32, z: f32, dx: i32, dy: i32, dz: i32, delta: f32, mirror: bool) {
        let (mut x0, y0, z0) = (x - delta, y - delta, z - delta);
        let (mut x1, y1, z1) = (x + dx as f32 + delta, y + dy as f32 + delta, z + dz as f32 + delta);
        if mirror {
            std::mem::swap(&mut x0, &mut x1);
        }
        let v7 = Vec3::new(x0, y0, z0);
        let v0 = Vec3::new(x1, y0, z0);
        let v1 = Vec3::new(x1, y1, z0);
        let v2 = Vec3::new(x0, y1, z0);
        let v3 = Vec3::new(x0, y0, z1);
        let v4 = Vec3::new(x1, y0, z1);
        let v5 = Vec3::new(x1, y1, z1);
        let v6 = Vec3::new(x0, y1, z1);
        let quads = [
            ([v4, v0, v1, v5], u + dz + dx, v + dz, u + dz + dx + dz, v + dz + dy),
            ([v7, v3, v6, v2], u, v + dz, u + dz, v + dz + dy),
            ([v4, v3, v7, v0], u + dz, v, u + dz + dx, v + dz),
            ([v1, v2, v6, v5], u + dz + dx, v + dz, u + dz + dx + dx, v),
            ([v0, v7, v2, v1], u + dz, v + dz, u + dz + dx, v + dz + dy),
            ([v3, v4, v5, v6], u + dz + dx + dz, v + dz, u + dz + dx + dz + dx, v + dz + dy),
        ];
        for (mut p, u1, v1, u2, v2) in quads {
            let (u1, v1, u2, v2) = (u1 as f32 / tw, v1 as f32 / th, u2 as f32 / tw, v2 as f32 / th);
            let mut uvs = [[u2, v1], [u1, v1], [u1, v2], [u2, v2]];
            if mirror {
                p.reverse();
                uvs.reverse();
            }
            // TexturedQuad.draw: (v2 - v1) x (v0 - v1).
            let n = (p[2] - p[1]).cross(p[0] - p[1]).normalize();
            let base = self.pos.len() as u32;
            for (k, uv) in uvs.into_iter().enumerate() {
                self.pos.push(p[k].to_array());
                self.normal.push(n.to_array());
                self.uv.push(uv);
            }
            self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}

/// `bipedRightArm` and `bipedRightArmwear` of the wide-armed `ModelPlayer`, in that order.
pub fn arm_mesh() -> Mesh {
    let mut b = BoxMesh::default();
    // ModelBiped: new ModelRenderer(this, 40, 16).addBox(-3, -2, -2, 4, 12, 4, 0).
    b.model_box(40, 16, -3.0, -2.0, -2.0, 4, 12, 4, 0.0);
    // ModelPlayer: new ModelRenderer(this, 40, 32).addBox(-3, -2, -2, 4, 12, 4, 0.25).
    b.model_box(40, 32, -3.0, -2.0, -2.0, 4, 12, 4, 0.25);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
        .with_inserted_indices(Indices::U32(b.idx))
}

/// Marks the arm entity.
#[derive(Component)]
pub struct Arm;

/// Poses the arm under the view bobbing `renderHand` applies first, and writes its light:
/// `ItemRenderer.setLightmap` reads `getCombinedLight(BlockPos(posX, posY + eyeHeight, posZ), 0)`.
#[allow(clippy::too_many_arguments)]
pub fn update_hand(
    clock: Res<Clock>,
    view: Res<View>,
    hand: Res<Hand>,
    hide: Res<HideGui>,
    streamer: Res<Streamer>,
    player: Res<Player>,
    mut env: ResMut<Env>,
    arm: Single<(&mut Transform, &mut Visibility), With<Arm>>,
) {
    let (mut transform, mut visibility) = arm.into_inner();
    // renderItemInFirstPerson draws the arm only for an empty hand (item_render.rs draws a stack).
    // renderHand runs in first person only.
    *visibility = if hide.0 || !hand.shown.is_empty() || view.third_person != 0 { Visibility::Hidden } else { Visibility::Inherited };
    *transform = Transform::from_matrix(view.bob * arm_matrix(&hand, view.pitch, view.yaw, clock.partial));
    let [w0, w1] = item_lights(view.pitch, view.yaw);
    env.set(env::P_HAND_W0, [w0.x, w0.y, w0.z, 0.0]);
    env.set(env::P_HAND_W1, [w1.x, w1.y, w1.z, 0.0]);
    let p = player.pos;
    let y = (p.y + player.eye_height() as f64).floor() as i32;
    let (sky, block) = neighbor_light(&streamer, p.x.floor() as i32, y, p.z.floor() as i32);
    env.set(env::P_HAND_LIGHT, [(block as u32 * 16) as f32, (sky as u32 * 16) as f32, 0.0, 0.0]);
}

/// `textures/gui/icons.png`
#[derive(Resource)]
pub struct GuiIcons(pub Handle<Image>);

/// One sprite `GuiIngame` draws over the view.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum GuiPart {
    Crosshair,
    AttackBack,
    AttackFill,
    /// The full indicator over a pointed living entity once the attack is charged (`renderAttackIndicator`).
    AttackReady,
}

pub fn spawn_gui(mut commands: Commands, icons: Res<GuiIcons>) {
    for part in [GuiPart::Crosshair, GuiPart::AttackBack, GuiPart::AttackFill, GuiPart::AttackReady] {
        commands.spawn((
            part,
            ImageNode::new(icons.0.clone()),
            Node { position_type: PositionType::Absolute, ..default() },
            Visibility::Hidden,
        ));
    }
}

/// `ScaledResolution` with the GUI scale on auto: the largest factor that keeps 320 x 240.
pub fn gui_scale(width: u32, height: u32) -> u32 {
    let mut k = 1;
    while k < 1000 && width / (k + 1) >= 320 && height / (k + 1) >= 240 {
        k += 1;
    }
    k
}

/// `GuiIngame.renderAttackIndicator`: the crosshair, the cooldown bar under it while the attack
/// strength is below 1, and the full indicator while a living entity is pointed at and the charged
/// attack (every entity in `Controller.entities` is alive, so `isEntityAlive` is true).
/// DROPPED: the crosshair's inverting blend (`ONE_MINUS_DST_COLOR, ONE_MINUS_SRC_COLOR`); the
/// UI blends normally, so the crosshair is drawn white.
pub fn update_gui(
    window: Single<&Window, With<PrimaryWindow>>,
    hand: Res<Hand>,
    ctl: Res<Controller>,
    hide: Res<HideGui>,
    mut parts: Query<(&GuiPart, &mut Node, &mut ImageNode, &mut Visibility)>,
) {
    let (w, h) = (window.physical_width(), window.physical_height());
    let k = gui_scale(w, h);
    let sw = (w as f64 / k as f64).ceil() as i32;
    let sh = (h as f64 / k as f64).ceil() as i32;
    // Scaled GUI units to logical pixels.
    let unit = k as f32 / window.scale_factor();
    let f = hand.cooled_attack_strength(0.0);
    let fill = (f * 17.0) as i32;
    // renderAttackIndicator: the full indicator needs a pointed living entity, a charged attack and
    // a cooldown period above five (a sword's 12.5, not a block's or an empty hand's 5).
    let ready = ctl.entity.is_some() && f >= 1.0 && hand.cooldown_period() > 5.0;
    for (part, mut node, mut image, mut visibility) in &mut parts {
        let (x, y, u, v, width, height, shown) = match part {
            GuiPart::Crosshair => (sw / 2 - 7, sh / 2 - 7, 0, 0, 16, 16, true),
            GuiPart::AttackBack => (sw / 2 - 8, sh / 2 - 7 + 16, 36, 94, 16, 4, !ready && f < 1.0),
            GuiPart::AttackFill => (sw / 2 - 8, sh / 2 - 7 + 16, 52, 94, fill, 4, !ready && f < 1.0 && fill > 0),
            GuiPart::AttackReady => (sw / 2 - 8, sh / 2 - 7 + 16, 68, 94, 16, 16, ready),
        };
        *visibility = if shown && !hide.0 { Visibility::Inherited } else { Visibility::Hidden };
        node.left = px(x as f32 * unit);
        node.top = px(y as f32 * unit);
        node.width = px(width as f32 * unit);
        node.height = px(height as f32 * unit);
        let rect = Rect::new(u as f32, v as f32, (u + width) as f32, (v + height) as f32);
        if image.rect != Some(rect) {
            image.rect = Some(rect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_runs_six_ticks() {
        let mut h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, ..default() };
        h.clicks = 1;
        h.tick(0.0, 0.0, false);
        // The click resets the cooldown and starts the swing at -1, advanced to 0 the same tick.
        assert!(h.swinging);
        assert_eq!(h.swing_int, 0);
        assert_eq!(h.ticks_since_last_swing, 1);
        let mut progress = vec![h.swing];
        for _ in 0..6 {
            h.tick(0.0, 0.0, false);
            progress.push(h.swing);
        }
        let want = [0.0, 1.0 / 6.0, 2.0 / 6.0, 3.0 / 6.0, 4.0 / 6.0, 5.0 / 6.0, 0.0];
        for (a, b) in progress.iter().zip(want) {
            assert!((a - b).abs() < 1e-6, "{progress:?}");
        }
        assert!(!h.swinging);
        // Wrapping back to 0 interpolates forward, not backward.
        h.prev_swing = 5.0 / 6.0;
        h.swing = 0.0;
        assert!((h.swing_progress(0.5) - (5.0 / 6.0 + 1.0 / 12.0)).abs() < 1e-6);
    }

    #[test]
    fn a_count_change_on_the_held_stack_does_not_dip_the_hand() {
        let stone = items::ItemStack { item: 1, count: 64, ..default() };
        let mut h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, shown: stone, shown_slot: 3, ..default() };
        for count in [63, 62, 1] {
            h.update_equipped(items::ItemStack { count, ..stone }, 3);
            assert_eq!(h.equipped, 1.0, "count {count}");
            assert_eq!(h.shown.count, count);
        }
        // Another item in the same slot, or the same stack in another slot, still dips.
        h.update_equipped(items::ItemStack { item: 2, count: 1, ..default() }, 3);
        assert!(h.equipped < 1.0);
        let mut h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, shown: stone, shown_slot: 3, ..default() };
        h.update_equipped(stone, 4);
        assert!(h.equipped < 1.0);
    }

    #[test]
    fn a_damage_change_on_a_sword_does_not_dip_the_hand() {
        let sword = items::ItemStack::of(items::ids::item::IRON_SWORD);
        let mut h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, shown: sword, shown_slot: 3, ..default() };
        for damage in [1, 2, 100] {
            h.update_equipped(items::ItemStack { damage, ..sword }, 3);
            assert_eq!(h.equipped, 1.0, "damage {damage}");
            assert_eq!(h.shown.damage, damage);
        }
    }

    #[test]
    fn progress_after_a_slot_change_follows_the_cooldown() {
        let sword = items::ItemStack::of(items::ids::item::IRON_SWORD);
        let block = items::ItemStack { item: 1, count: 64, ..default() };
        let mut h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, shown: block, shown_slot: 3, ..default() };
        h.set_attack_stats(&sword, &items::potion::Effects::default());
        // An iron sword's attack speed 1.6 gives a 12.5 tick cooldown.
        assert!((h.cooldown_period() - 12.5).abs() < 1e-3, "{}", h.cooldown_period());
        // Switching to the sword restarts the cooldown.
        h.update_main_hand(sword);
        assert_eq!(h.ticks_since_last_swing, 0);
        // The slot changed: the hand falls over three ticks.
        for _ in 0..3 {
            h.update_equipped(sword, 4);
            h.ticks_since_last_swing += 1;
        }
        assert!(h.equipped < 0.1, "{}", h.equipped);
        assert_eq!(h.shown_slot, 4);
        // Then it rises, tracking f * f * f with f = (ticks + 1) / 12.5.
        for _ in 0..12 {
            h.ticks_since_last_swing += 1;
            h.update_equipped(sword, 4);
            let f = h.cooled_attack_strength(1.0);
            assert!((h.equipped - f * f * f).abs() < 1e-5, "{} vs {}", h.equipped, f * f * f);
        }
        assert!((h.equipped - 1.0).abs() < 1e-6, "{}", h.equipped);
    }

    #[test]
    fn equip_dips_and_recovers_after_a_click() {
        let mut h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, ..default() };
        h.clicks = 1;
        h.tick(0.0, 0.0, false);
        // Strength (0 + 1) / 5 = 0.2, cubed 0.008: the step is clamped to -0.4.
        assert!((h.equipped - 0.6).abs() < 1e-6, "{}", h.equipped);
        for _ in 0..10 {
            h.tick(0.0, 0.0, false);
        }
        assert!((h.equipped - 1.0).abs() < 1e-6, "{}", h.equipped);
        assert_eq!(h.cooled_attack_strength(0.0), 1.0);
    }

    #[test]
    fn the_click_that_captures_the_mouse_does_not_swing() {
        let mut h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, ..default() };
        // setIngameFocus, with the click queued and the button still down.
        h.left_click_counter = 10000;
        h.clicks = 1;
        h.tick(0.0, 0.0, true);
        h.tick(0.0, 0.0, true);
        assert!(!h.swinging && h.left_click_counter == 9998);
        // Let go, then click again: that one swings.
        h.tick(0.0, 0.0, false);
        assert_eq!(h.left_click_counter, 0);
        h.clicks = 1;
        h.tick(0.0, 0.0, true);
        assert!(h.swinging);
    }

    #[test]
    fn the_resting_arm_sits_low_right_in_front() {
        let h = Hand { equipped: 1.0, prev_equipped: 1.0, ticks_since_last_swing: 100, ..default() };
        let m = arm_matrix(&h, 0.0, 0.0, 0.0);
        // The arm box's center, in model units: (-1, 4, 0).
        let c = m.transform_point3(Vec3::new(-1.0, 4.0, 0.0));
        assert!(c.x > 0.0 && c.y < 0.0 && c.z < 0.0, "{c}");
        assert!(c.length() < 2.0, "{c}");
        // The box has 12 quads, two boxes of six.
        assert_eq!(arm_mesh().indices().unwrap().len(), 12 * 6);
    }

    #[test]
    fn gui_scale_matches_scaled_resolution() {
        assert_eq!(gui_scale(854, 480), 2);
        assert_eq!(gui_scale(1920, 1080), 4);
        assert_eq!(gui_scale(639, 480), 1);
    }
}
