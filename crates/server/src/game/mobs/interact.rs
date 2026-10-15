//! A player's right-click on a mob: `NetHandlerPlayServer.processUseEntity` INTERACT into
//! `EntityPlayer.interactOn`, then `EntityCow.processInteract` (milking), `EntitySheep` (shears),
//! `EntityPig`, `EntityAnimal.processInteract` (breeding item: love mode, or feeding a calf) and
//! `EntityAgeable.processInteract`, then the held item's `itemInteractionForEntity` (dye, saddle).
//!
//! DROPPED: the spawn-egg branch of `EntityAgeable.processInteract` (spawn eggs are not an item
//! here), `INTERACT_AT`, sounds (the cow milk sound), the `Entity.processInitialInteract`
//! lead and name tag handling, and `EntityLiving.processInteract` leads.

use glam::DVec3;
use items::ids::item;
use items::ItemStack;
use proto::{EV_LOVE, GM_CREATIVE, GM_SPECTATOR, HAND_MAIN, SNEAKING};
use sim::raytrace::ray_trace_blocks;

use crate::game::terrain::Blocks;
use crate::game::{players, tracker, Game};

const REACH_SQ: f64 = 36.0;
const REACH_SQ_BLIND: f64 = 9.0;

/// `processUseEntity` INTERACT: the reach check, then the mob's `processInteract`.
pub fn process_interact(game: &mut Game, pid: u32, target: u32, hand: u8) {
    let Some(i) = game.mobs.mobs.iter().position(|m| m.id == target && !m.dead) else { return };
    let Some(s) = game.sessions.get(&pid) else { return };
    let (Some(st), mode) = (s.state, s.mode) else { return };
    if mode == GM_SPECTATOR || game.players.players.get(&pid).is_none_or(|p| p.dead) {
        return;
    }
    let m = &game.mobs.mobs[i];
    let eye = DVec3::new(st.pos[0], st.pos[1] + players::eye_height(st.flags & SNEAKING != 0) as f64, st.pos[2]);
    let to = DVec3::new(m.body.pos.x, m.body.pos.y + m.eye_height() as f64, m.body.pos.z);
    let seen = ray_trace_blocks(&Blocks(game), eye, to, false, false).is_none();
    let reach = if seen { REACH_SQ } else { REACH_SQ_BLIND };
    let d = (st.pos[0] - m.body.pos.x).powi(2) + (st.pos[1] - m.body.pos.y).powi(2) + (st.pos[2] - m.body.pos.z).powi(2);
    if d < reach {
        process_mob_interact(game, pid, target, hand);
    }
}

/// The held stack of `hand`.
fn held(game: &Game, pid: u32, hand: u8) -> ItemStack {
    let Some(p) = game.players.players.get(&pid) else { return ItemStack::EMPTY };
    if hand == HAND_MAIN {
        p.inv.get_current_item()
    } else {
        p.inv.off_hand[0]
    }
}

fn set_held(game: &mut Game, pid: u32, hand: u8, stack: ItemStack) {
    let Some(p) = game.players.players.get_mut(&pid) else { return };
    let stack = if stack.is_empty() { ItemStack::EMPTY } else { stack };
    if hand == HAND_MAIN {
        let slot = p.inv.current_item;
        p.inv.main[slot] = stack;
    } else {
        p.inv.off_hand[0] = stack;
    }
}

/// `EntityPlayer.interactOn` for a mob: `processInitialInteract` (the kind's `processInteract`),
/// then, when that is false, the held item's `itemInteractionForEntity` (dye on a sheep, a saddle
/// on a pig). True when the click was used.
pub fn process_mob_interact(game: &mut Game, pid: u32, target: u32, hand: u8) -> bool {
    let Some(i) = game.mobs.mobs.iter().position(|m| m.id == target && !m.dead) else { return false };
    let creative = game.sessions.get(&pid).is_some_and(|s| s.mode == GM_CREATIVE);
    let kind = game.mobs.mobs[i].def.kind.name;
    let before = held(game, pid, hand);
    let used = match kind {
        "cow" => cow_interact(game, pid, hand, i, creative),
        "creeper" => creeper_interact(game, pid, hand, i, creative),
        "sheep" => {
            sheep_shear(game, pid, hand, i, creative);
            animal_interact(game, pid, hand, i, creative)
        }
        // EntityPig.processInteract: super first. DROPPED: the name tag, and startRiding for a
        // saddled pig (riding), which would answer true; the click falls through to the item.
        _ => animal_interact(game, pid, hand, i, creative),
    };
    if used {
        // interactOn: a creative player's stack keeps its count.
        if creative && !before.is_empty() && held(game, pid, hand).get_count() < before.get_count() {
            set_held(game, pid, hand, before);
        }
        return true;
    }
    item_interaction(game, pid, hand, i, creative)
}

/// `EntityCreeper.processInteract`: flint and steel plays its sound, ignites the creeper and wears
/// the item by 1 (`damageItem`). Anything else is `EntityMob`'s, which does nothing here.
fn creeper_interact(game: &mut Game, pid: u32, hand: u8, i: usize, creative: bool) -> bool {
    let mut stack = held(game, pid, hand);
    if stack.get_item() != item::FLINT_AND_STEEL {
        return false;
    }
    let id = game.mobs.mobs[i].id;
    super::Mob::flint_and_steel_sound(game, id);
    game.mobs.mobs[i].ignite();
    stack.damage_item(1, creative, &mut players::wear_rand());
    set_held(game, pid, hand, stack);
    players::send_slots(game, pid);
    true
}

/// `EntityCow.processInteract`: milking, else `EntityAnimal.processInteract`.
fn cow_interact(game: &mut Game, pid: u32, hand: u8, i: usize, creative: bool) -> bool {
    let mut stack = held(game, pid, hand);
    if !stack.is_empty() && stack.get_item() == item::BUCKET && !creative && !game.mobs.mobs[i].is_child() {
        stack.shrink(1);
        let milk = ItemStack::of(item::MILK_BUCKET);
        if stack.is_empty() {
            set_held(game, pid, hand, milk);
        } else {
            set_held(game, pid, hand, stack);
            let mut m = milk;
            let added = game.players.players.get_mut(&pid).is_some_and(|p| p.inv.add_item_stack_to_inventory(&mut m, false));
            if !added {
                players::drop_item(game, pid, milk, false, false);
            }
        }
        players::send_slots(game, pid);
        return true;
    }
    animal_interact(game, pid, hand, i, creative)
}

/// The shearing branch of `EntitySheep.processInteract`: it does not answer true itself (the
/// `super` call's result is what returns). DROPPED: the shear sound.
fn sheep_shear(game: &mut Game, pid: u32, hand: u8, i: usize, creative: bool) {
    let mut stack = held(game, pid, hand);
    let m = &game.mobs.mobs[i];
    if stack.is_empty() || stack.get_item() != item::SHEARS || m.get_sheared() || m.is_child() {
        return;
    }
    let (pos, color) = (m.body.pos, m.get_fleece_color());
    game.mobs.mobs[i].set_sheared(true);
    let n = 1 + game.mobs.rand.next_int_bounded(3);
    for _ in 0..n {
        // entityDropItem(new ItemStack(WOOL, 1, colour), 1.0F)
        let wool = ItemStack::new(items::ids::block::WOOL, 1, color as i32);
        super::super::items::spawn_stack_at(game, [pos.x, pos.y + 1.0, pos.z], wool, None, sim::item_entity::DEFAULT_PICKUP_DELAY);
        let r = &mut game.mobs.rand;
        let dy = (r.next_float() * 0.05f32) as f64;
        let dx = ((r.next_float() - r.next_float()) * 0.1f32) as f64;
        let dz = ((r.next_float() - r.next_float()) * 0.1f32) as f64;
        if let Some(e) = game.items.entities.last_mut() {
            e.body.motion.y += dy;
            e.body.motion.x += dx;
            e.body.motion.z += dz;
        }
    }
    // itemstack.damageItem(1, player)
    stack.damage_item(1, creative, &mut players::wear_rand());
    set_held(game, pid, hand, stack);
    players::send_slots(game, pid);
}

/// `EntityAnimal.processInteract`. True when the click was used.
fn animal_interact(game: &mut Game, pid: u32, hand: u8, i: usize, creative: bool) -> bool {
    let mut stack = held(game, pid, hand);
    if !game.mobs.mobs[i].is_animal() || stack.is_empty() || !game.mobs.mobs[i].is_breeding_item(stack.get_item()) {
        return false;
    }
    let (age, in_love) = (game.mobs.mobs[i].growing_age, game.mobs.mobs[i].in_love);
    if age == 0 && in_love <= 0 {
        if !creative {
            stack.shrink(1);
        }
        set_held(game, pid, hand, stack);
        // setInLove(player)
        let m = &mut game.mobs.mobs[i];
        m.in_love = 600;
        m.love_cause = Some(pid);
        let id = m.id;
        tracker::send_to_tracking(game, id, proto::S_ENTITY_EVENT, &proto::EntityEvent { id, status: EV_LOVE }.encode());
        players::send_slots(game, pid);
        return true;
    }
    if age < 0 {
        if !creative {
            stack.shrink(1);
        }
        set_held(game, pid, hand, stack);
        game.mobs.mobs[i].age_up((((-age / 20) as f32) * 0.1f32) as i32, true);
        players::send_slots(game, pid);
        return true;
    }
    false
}

/// `ItemStack.interactWithEntity` after a false `processInitialInteract`: `ItemDye` on a sheep
/// (the fleece takes the dye's colour unless sheared or already that colour; the click is used
/// either way) and `ItemSaddle` on a pig (saddles an unsaddled adult). A creative player's stack
/// is a copy, so it is not used up.
fn item_interaction(game: &mut Game, pid: u32, hand: u8, i: usize, creative: bool) -> bool {
    let mut stack = held(game, pid, hand);
    if stack.is_empty() {
        return false;
    }
    let kind = game.mobs.mobs[i].def.kind.name;
    let used = if stack.get_item() == item::DYE && kind == "sheep" {
        // byDyeDamage(stack.getMetadata())
        let color = super::animals::meta_of_dye_damage(stack.get_metadata());
        let m = &mut game.mobs.mobs[i];
        if !m.get_sheared() && m.get_fleece_color() != color {
            m.set_fleece_color(color);
            stack.shrink(1);
        }
        true
    } else if stack.get_item() == item::SADDLE && kind == "pig" {
        let m = &mut game.mobs.mobs[i];
        if !m.saddled && !m.is_child() {
            m.saddled = true;
            stack.shrink(1);
        }
        true
    } else {
        false
    };
    if used && !creative {
        set_held(game, pid, hand, stack);
        players::send_slots(game, pid);
    }
    used
}

