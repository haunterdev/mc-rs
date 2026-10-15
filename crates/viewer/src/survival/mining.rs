//! Survival block breaking: the survival half of `PlayerControllerMP` (`clickBlock`,
//! `onPlayerDamageBlock`, `resetBlockRemoving`, `isHittingPosition`) and the sends it makes as
//! `CPacketPlayerDigging` (`V_DIG`: start, abort, finish, carrying the state the client saw).
//! The creative half stays in interact.rs. The server (game/mining.rs) checks every finish; a
//! refused one answers with `S_BLOCKS` holding its state, which the streamer applies.
//!
//! [`Mining`] holds the controller's fields and leaves what it wants sent in [`Mining::out`]:
//! [`flush`] puts the `V_DIG` frames on the link (streamed and native sessions both own a
//! [`Link`]), the break progress into the crack overlay (crack.rs) and the hit sounds on the
//! audio engine, so the logic itself needs no resources and is tested without a window.
//!
//! DROPPED: `Block.onBlockClicked` (no block of this world reacts to a punch on the client),
//! `syncCurrentPlayItem`'s `CPacketHeldItemChange` (`V_HELD` is the hotbar's), the hit
//! particles (`addBlockHitEffects`), the tool damage `onPlayerDestroyBlock` applies (the server
//! owns the stack and sends `S_SLOTS`), the adventure `canDestroy` check, the world border, and
//! the potion effects of `getDigSpeed` (the viewer tracks none, so haste and fatigue are
//! absent).

use bevy::prelude::*;
use items::{DigContext, ItemStack};
use sim::mining::player_relative_block_hardness;
use crate::audio::{Audio, Category};
use crate::interact::{on_player_destroy_block, World};
use crate::link::Link;
use crate::player::Player;
use crate::region::{props, Mat};
use crate::sfx::sound_of;
use crate::target::BlockPos;
use worldgen::populate::Facing;

use super::crack::Cracks;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, flush.after(crate::player::run_ticks).after(super::items::held_sync).before(super::crack::tick).run_if(resource_exists::<Link>).run_if(resource_exists::<crate::interact::Controller>));
}

/// What the controller asks the session to do, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// `CPacketPlayerDigging`.
    Dig(proto::Dig),
    /// `WorldClient.sendBlockBreakProgress(player id, pos, progress)`.
    Progress(BlockPos, i32),
    /// The hit sound of the block in this state, at the position.
    Hit(u16, BlockPos),
}

/// What the player holds and does that `getPlayerRelativeBlockHardness` reads.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hold {
    pub stack: ItemStack,
    pub dig: DigContext,
}

/// `PlayerControllerMP`'s survival fields.
#[derive(Clone, Debug, Default)]
pub struct Mining {
    /// `isHittingBlock`
    pub is_hitting_block: bool,
    /// `currentBlock`
    pub current_block: BlockPos,
    /// `currentItemHittingBlock`
    current_item: ItemStack,
    /// `curBlockDamageMP`
    pub cur_block_damage_mp: f32,
    /// `stepSoundTickCounter`
    step_sound_tick_counter: f32,
    pub out: Vec<Out>,
}

/// The progress `sendBlockBreakProgress` is given: `(int)(curBlockDamageMP * 10) - 1`.
pub fn progress_of(cur_block_damage_mp: f32) -> i32 {
    (cur_block_damage_mp * 10.0) as i32 - 1
}

fn dig(action: u8, p: BlockPos, face: Facing, seen: u16) -> Out {
    Out::Dig(proto::Dig { action, pos: [p.0, p.1, p.2], face: face as u8, state: seen })
}

fn is_air(s: u16) -> bool {
    props(s).mat == Mat::Air
}

impl Mining {
    /// `PlayerControllerMP.isHittingPosition`: the same block, and the same held item (its
    /// metadata too unless it is damageable).
    fn is_hitting_position(&self, p: BlockPos, held: &ItemStack) -> bool {
        let cur = &self.current_item;
        let same = if cur.is_empty() && held.is_empty() {
            true
        } else if !cur.is_empty() && !held.is_empty() {
            held.item == cur.item && (held.is_item_stack_damageable() || held.get_metadata() == cur.get_metadata())
        } else {
            false
        };
        p == self.current_block && same
    }

    /// `PlayerControllerMP.clickBlock` in survival.
    pub fn click_block(&mut self, w: &mut impl World, p: BlockPos, face: Facing, hold: &Hold) -> bool {
        if !self.is_hitting_block || !self.is_hitting_position(p, &hold.stack) {
            if self.is_hitting_block {
                self.out.push(dig(proto::DIG_ABORT, self.current_block, face, 0));
            }
            let s = w.state(p.0, p.1, p.2);
            self.out.push(dig(proto::DIG_START, p, face, s));
            let solid = !is_air(s);
            if solid && player_relative_block_hardness(s, &hold.stack, &hold.dig) >= 1.0 {
                on_player_destroy_block(w, p, true);
            } else {
                self.is_hitting_block = true;
                self.current_block = p;
                self.current_item = hold.stack;
                self.cur_block_damage_mp = 0.0;
                self.step_sound_tick_counter = 0.0;
                self.out.push(Out::Progress(p, progress_of(self.cur_block_damage_mp)));
            }
        }
        true
    }

    /// `PlayerControllerMP.resetBlockRemoving`.
    pub fn reset_block_removing(&mut self) {
        if self.is_hitting_block {
            self.out.push(dig(proto::DIG_ABORT, self.current_block, Facing::Down, 0));
            self.is_hitting_block = false;
            self.cur_block_damage_mp = 0.0;
            self.out.push(Out::Progress(self.current_block, -1));
        }
    }

    /// `PlayerControllerMP.onPlayerDamageBlock` in survival; `delay` is `blockHitDelay`.
    pub fn on_player_damage_block(&mut self, w: &mut impl World, delay: &mut i32, p: BlockPos, face: Facing, hold: &Hold) -> bool {
        if *delay > 0 {
            *delay -= 1;
            return true;
        }
        if !self.is_hitting_position(p, &hold.stack) {
            return self.click_block(w, p, face, hold);
        }
        let s = w.state(p.0, p.1, p.2);
        if is_air(s) {
            self.is_hitting_block = false;
            return false;
        }
        self.cur_block_damage_mp += player_relative_block_hardness(s, &hold.stack, &hold.dig);
        if self.step_sound_tick_counter % 4.0 == 0.0 {
            self.out.push(Out::Hit(s, p));
        }
        self.step_sound_tick_counter += 1.0;
        if self.cur_block_damage_mp >= 1.0 {
            self.is_hitting_block = false;
            self.out.push(dig(proto::DIG_FINISH, p, face, s));
            on_player_destroy_block(w, p, true);
            self.cur_block_damage_mp = 0.0;
            self.step_sound_tick_counter = 0.0;
            *delay = 5;
        }
        self.out.push(Out::Progress(self.current_block, progress_of(self.cur_block_damage_mp)));
        true
    }
}

/// `Entity.isInsideOfMaterial(Material.WATER)` at the eyes.
pub fn eyes_in_water(w: &impl crate::collision::BlockAccess, player: &Player) -> bool {
    let y = player.pos.y + player.eye_height() as f64;
    let (bx, by, bz) = (player.pos.x.floor() as i32, y.floor() as i32, player.pos.z.floor() as i32);
    let s = w.state(bx, by, bz);
    if props(s).mat != Mat::Water {
        return false;
    }
    // BlockLiquid.getLiquidHeightPercent(level) - 1/9
    let level = (s & 15) as i32;
    let level = if level >= 8 { 0 } else { level };
    let f = (level + 1) as f32 / 9.0 - 0.11111111;
    y < (by + 1) as f32 as f64 - f as f64
}

/// What `EntityPlayer.getDigSpeed` reads of the player: on the ground, eyes in water.
pub fn dig_context(w: &impl crate::collision::BlockAccess, player: &Player) -> DigContext {
    let haste = player.effects.amplifier(items::potion::Effect::Haste);
    let mining_fatigue = player.effects.amplifier(items::potion::Effect::MiningFatigue);
    DigContext { on_ground: player.on_ground, eyes_in_water: eyes_in_water(w, player), haste, mining_fatigue, ..DigContext::default() }
}

/// Puts the controller's output where it goes: `V_DIG` on the link, progress into the crack
/// overlay, the hit sound on the audio engine.
fn flush(mut ctl: ResMut<crate::interact::Controller>, link: Res<Link>, mut cracks: ResMut<Cracks>, audio: Option<Res<Audio>>) {
    if ctl.mining.out.is_empty() {
        return;
    }
    for o in std::mem::take(&mut ctl.mining.out) {
        match o {
            Out::Dig(d) => super::net::send_dig(&link, &d),
            Out::Progress(p, stage) => cracks.send_block_break_progress(link.welcome.id, p, stage),
            Out::Hit(s, p) => {
                // The block's hit sound at (volume + 1) / 8 and half the pitch.
                if let Some(audio) = &audio {
                    let (name, v, pitch) = sound_of(s);
                    let at = Vec3::new(p.0 as f32 + 0.5, p.1 as f32 + 0.5, p.2 as f32 + 0.5);
                    audio.play(&format!("{name}.hit"), Category::Blocks, Some(at), (v + 1.0) / 8.0, pitch * 0.5);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::Blocks;
    use crate::collision::BlockAccess;
    use items::ids::{block, item};
    use sim::mining::break_ticks;
    use worldgen::blocks::{id, state};

    fn rig(s: u16) -> (Blocks, Mining) {
        let mut w = Blocks::floor_at(63, 8);
        w.set(2, 64, 0, s);
        (w, Mining::default())
    }

    fn hold(tool: u16) -> Hold {
        Hold { stack: if tool == 0 { ItemStack::EMPTY } else { ItemStack::of(tool) }, dig: DigContext::default() }
    }

    /// Holds the attack on (2, 64, 0): the click tick, then one damage call per tick. Returns the
    /// number of damage calls up to and including the one that sent the finish.
    fn dig_it(w: &mut Blocks, m: &mut Mining, h: &Hold) -> Option<u32> {
        let p = (2, 64, 0);
        let mut delay = 0;
        m.click_block(w, p, Facing::West, h);
        for n in 1..=400 {
            m.on_player_damage_block(w, &mut delay, p, Facing::West, h);
            if m.out.iter().any(|o| matches!(o, Out::Dig(d) if d.action == proto::DIG_FINISH)) {
                return Some(n);
            }
        }
        None
    }

    #[test]
    fn dig_progress_takes_the_ticks_sim_mining_counts() {
        let d = DigContext::default();
        for (s, tool, expect) in [
            (state(block::STONE, 0), 0, 151),
            (state(block::DIRT, 0), 0, 15),
            (state(block::STONE, 0), item::WOODEN_PICKAXE, 23),
            (state(block::OBSIDIAN, 0), item::DIAMOND_PICKAXE, 188),
        ] {
            let (mut w, mut m) = rig(s);
            let h = hold(tool);
            assert_eq!(break_ticks(s, &h.stack, &d), Some(expect));
            assert_eq!(dig_it(&mut w, &mut m, &h), Some(expect), "state {s:#x} tool {tool}");
            assert_eq!(w.state(2, 64, 0), 0, "the client predicts the break");
        }
    }

    #[test]
    fn the_sends_are_start_then_finish_with_the_seen_state() {
        let s = state(block::DIRT, 0);
        let (mut w, mut m) = rig(s);
        dig_it(&mut w, &mut m, &hold(0));
        let digs: Vec<_> = m.out.iter().filter_map(|o| if let Out::Dig(d) = o { Some(*d) } else { None }).collect();
        assert_eq!(digs.len(), 2);
        assert_eq!((digs[0].action, digs[0].pos, digs[0].state), (proto::DIG_START, [2, 64, 0], s));
        assert_eq!((digs[1].action, digs[1].face, digs[1].state), (proto::DIG_FINISH, Facing::West as u8, s));
        // Break sound every fourth tick: ticks 0, 4, 8, 12 of the 15.
        assert_eq!(m.out.iter().filter(|o| matches!(o, Out::Hit(..))).count(), 4);
        // The delay after a break.
        let mut delay = 0;
        w.set(2, 64, 0, s);
        m.on_player_damage_block(&mut w, &mut delay, (2, 64, 0), Facing::West, &hold(0));
        assert_eq!(delay, 0);
    }

    #[test]
    fn instant_blocks_break_on_the_click_and_stage_follows_progress() {
        // A flower has hardness 0.
        let (mut w, mut m) = rig(state(id::RED_FLOWER, 0));
        m.click_block(&mut w, (2, 64, 0), Facing::Up, &hold(0));
        assert_eq!(w.state(2, 64, 0), 0);
        assert!(!m.is_hitting_block);
        assert!(m.out.iter().all(|o| !matches!(o, Out::Progress(..))));
        assert_eq!([0.0, 0.09, 0.1, 0.55, 0.99].map(progress_of), [-1, -1, 0, 4, 8]);
    }

    #[test]
    fn letting_go_aborts_and_a_new_block_or_tool_restarts() {
        let s = state(block::STONE, 0);
        let (mut w, mut m) = rig(s);
        w.set(3, 64, 0, s);
        let h = hold(0);
        let mut delay = 0;
        m.click_block(&mut w, (2, 64, 0), Facing::West, &h);
        m.on_player_damage_block(&mut w, &mut delay, (2, 64, 0), Facing::West, &h);
        assert!(m.cur_block_damage_mp > 0.0);
        // Another block: abort the first, start the second, progress back to nothing.
        m.out.clear();
        m.on_player_damage_block(&mut w, &mut delay, (3, 64, 0), Facing::West, &h);
        let acts: Vec<u8> = m.out.iter().filter_map(|o| if let Out::Dig(d) = o { Some(d.action) } else { None }).collect();
        assert_eq!(acts, [proto::DIG_ABORT, proto::DIG_START]);
        assert_eq!(m.cur_block_damage_mp, 0.0);
        // A different tool on the same block is a new dig.
        m.out.clear();
        let pick = hold(item::WOODEN_PICKAXE);
        m.on_player_damage_block(&mut w, &mut delay, (3, 64, 0), Facing::West, &pick);
        assert_eq!(m.out.len(), 3, "abort, start, progress: {:?}", m.out);
        // Releasing the button.
        m.out.clear();
        m.reset_block_removing();
        assert_eq!(m.out, [dig(proto::DIG_ABORT, (3, 64, 0), Facing::Down, 0), Out::Progress((3, 64, 0), -1)]);
        assert!(!m.is_hitting_block);
        m.out.clear();
        m.reset_block_removing();
        assert!(m.out.is_empty());
    }
}
