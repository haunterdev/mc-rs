//! Beds: `ItemBed.onItemUse` (the item in the `USES` table of use_on.rs), `BlockBed.
//! onBlockActivated`, `EntityPlayer.trySleep` and `wakeUpPlayer`, the sleep timer, and
//! `WorldServer`'s skip to morning when every player is fully asleep (`areAllPlayersAsleep`,
//! `wakeAllPlayers`). The spawn point a bed sets is kept in `players.json` (`world::Saved`) and read
//! by `damage::respawn` through [`respawn_adjust`]. Breaking a half removes the other
//! (mining.rs calls [`other_half`]).
//!
//! The viewer is told only by the same frames a walk would send: `S_TELEPORT` puts the player in
//! the bed and again at the exit, `S_TIME` carries the skipped night, `S_TEXT` the refusals.
//! Sneaking or walking away leaves the bed (the viewer's `CPacketEntityAction` STOP_SLEEPING).
//!
//! DROPPED: the `TileEntityBed` color (a placed bed is red whatever the item's damage; the item
//! drop in drops.rs is red too), the sleeping pose, the fade screen and `SPacketUseBed`, the
//! placement sound, `canPlayerEdit`, the neighbour notifications, `isRiding` and shoulder
//! entities, `canRespawnHere` and the Nether explosion (there is one dimension), `onFallenUpon`
//! and `onLanded` (the bounce), and the status message as an action bar line (sent as chat).

use std::collections::HashMap;

use ::items::ids::{block, item};
use ::items::ItemStack;
use proto::{UseOn, GM_SPECTATOR, SNEAKING};
use serde_json::{json, Value};
use sim::collision::Aabb;
use sim::placement::is_replaceable;
use worldgen::populate::state as wst;
use worldgen::populate::Facing;

use super::terrain::{self, Blocks};
use super::use_on::Used;
use super::{mobs, net, players, Game};

/// `EnumFacing.getHorizontalIndex()` order: south, west, north, east, as (x, z) offsets.
const HORIZONTAL: [(i32, i32); 4] = [(0, 1), (-1, 0), (0, -1), (1, 0)];
/// `OCCUPIED` in a head's metadata.
const OCCUPIED: u16 = 4;
/// `PART == HEAD`
const HEAD: u16 = 8;
/// `EntityPlayer.sleepTimer` when `isPlayerFullyAsleep`.
const FULLY_ASLEEP: i32 = 100;
/// `BlockBed.REACH`: `NetHandlerPlayServer`'s block reach, squared.
const REACH_SQ: f64 = 36.0;

type Pos = (i32, i32, i32);

/// A sleeping player.
#[derive(Clone, Copy, Debug)]
pub struct Sleeper {
    /// `EntityPlayer.bedLocation`: the head of the bed.
    pub bed: Pos,
    /// `EntityPlayer.sleepTimer`
    pub timer: i32,
}

#[derive(Default)]
pub struct State {
    pub sleeping: HashMap<u32, Sleeper>,
}

pub fn on_join(_game: &mut Game, _id: u32) {}

/// The player is gone: the bed is free again.
pub fn on_leave(game: &mut Game, id: u32) {
    if let Some(s) = game.beds.sleeping.remove(&id) {
        set_occupied(game, s.bed, false);
    }
}

pub fn on_frame(_game: &mut Game, _id: u32, _tag: u8, _body: &[u8]) -> bool {
    false
}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

// ---- the item ----

pub fn is_bed_item(s: &ItemStack) -> bool {
    !s.is_empty() && s.item == item::BED
}

/// `ItemBed.onItemUse`: a foot where the click is (or above it), a head in front of it, on two
/// solid tops.
pub fn item_use(game: &mut Game, id: u32, m: &UseOn, _stack: ItemStack) -> Used {
    if Facing::VALUES.get(m.face as usize).copied() != Some(Facing::Up) {
        return Used::No;
    }
    let Some(state) = game.sessions.get(&id).and_then(|s| s.state) else { return Used::No };
    let mut pos = (m.pos[0], m.pos[1], m.pos[2]);
    let flag = is_replaceable(&Blocks(game), pos);
    if !flag {
        pos.1 += 1;
    }
    let i = ((state.yaw * 4.0 / 360.0) as f64 + 0.5).floor() as i32 & 3;
    let (dx, dz) = HORIZONTAL[i as usize];
    let head = (pos.0 + dx, pos.1, pos.2 + dz);
    if pos.1 < 1 || pos.1 > 255 {
        return Used::No;
    }
    // DROPPED: canPlayerEdit on both cells.
    let flag2 = flag || is_replaceable(&Blocks(game), pos);
    let flag3 = is_replaceable(&Blocks(game), head);
    let top_solid = |game: &Game, p: Pos| wst::is_top_solid(terrain::state(game, (p.0, p.1 - 1, p.2)));
    if flag2 && flag3 && top_solid(game, pos) && top_solid(game, head) {
        let foot_state = worldgen::blocks::state(block::BED, i as u16);
        terrain::set_block(game, pos, foot_state);
        terrain::set_block(game, head, foot_state | HEAD);
        // DROPPED: the placement sound, notifyNeighborsRespectDebug, the PLACED_BLOCK trigger.
        return Used::Success { consume: true };
    }
    Used::No
}

// ---- the block ----

fn chat(game: &Game, id: u32, text: &str) {
    let v = json!({"t": "chat", "from": "", "kind": "command", "text": text, "color": "white"});
    net::send(game, id, proto::S_TEXT, v.to_string().as_bytes());
}

fn set_occupied(game: &mut Game, head: Pos, occupied: bool) {
    let s = terrain::state(game, head);
    if s >> 4 != block::BED {
        return;
    }
    let meta = s & 15;
    let new = if occupied { meta | OCCUPIED } else { meta & !OCCUPIED };
    if new != meta {
        terrain::set_block(game, head, (s & !15) | new);
    }
}

/// `BlockBed.onBlockActivated`. True when the click was a bed's (a sneaking player with an item in
/// hand uses the item instead, as for the crafting table).
pub fn use_on(game: &mut Game, id: u32, m: &UseOn) -> bool {
    let Some(state) = game.sessions.get(&id).and_then(|s| s.state) else { return false };
    let mut pos = (m.pos[0], m.pos[1], m.pos[2]);
    let s = terrain::state(game, pos);
    if s >> 4 != block::BED {
        return false;
    }
    let held_any = game.players.players.get(&id).is_some_and(|p| !p.inv.get_current_item().is_empty() || !p.inv.off_hand[0].is_empty());
    if state.flags & SNEAKING != 0 && held_any {
        return false;
    }
    let (dx, dy, dz) = (state.pos[0] - (pos.0 as f64 + 0.5), state.pos[1] - (pos.1 as f64 + 0.5), state.pos[2] - (pos.2 as f64 + 0.5));
    if dx * dx + dy * dy + dz * dz >= REACH_SQ || players::mode(game, id) == GM_SPECTATOR {
        return true;
    }
    let mut st = s;
    if st & HEAD == 0 {
        let (ox, oz) = HORIZONTAL[(st & 3) as usize];
        pos = (pos.0 + ox, pos.1, pos.2 + oz);
        st = terrain::state(game, pos);
        if st >> 4 != block::BED {
            return true;
        }
    }
    if st & OCCUPIED != 0 {
        if game.beds.sleeping.values().any(|s| s.bed == pos) {
            chat(game, id, "This bed is occupied");
            return true;
        }
        set_occupied(game, pos, false);
    }
    match try_sleep(game, id, pos) {
        Ok(()) => set_occupied(game, pos, true),
        Err(Some(text)) => chat(game, id, text),
        Err(None) => {}
    }
    true
}

/// `World.isDaytime`: `skylightSubtracted < 4`.
fn is_daytime(game: &Game) -> bool {
    let Some(w) = terrain::world(game) else { return true };
    let w = w.lock().unwrap();
    super::mobs::spawner::skylight_subtracted(w.time(), w.weather.raining, w.weather.thundering) < 4
}

/// `EntityPlayer.trySleep(bedLocation)`. `Err` carries the `SleepResult`'s message, if it has one.
pub fn try_sleep(game: &mut Game, id: u32, head: Pos) -> Result<(), Option<&'static str>> {
    let Some(p) = game.players.players.get(&id) else { return Err(None) };
    if game.beds.sleeping.contains_key(&id) || p.dead {
        return Err(None);
    }
    if is_daytime(game) {
        return Err(Some("You can only sleep at night"));
    }
    let Some(state) = game.sessions.get(&id).and_then(|s| s.state) else { return Err(None) };
    let s = terrain::state(game, head);
    let (ox, oz) = HORIZONTAL[(s & 3) as usize];
    let near = |b: Pos| (state.pos[0] - b.0 as f64).abs() <= 3.0 && (state.pos[1] - b.1 as f64).abs() <= 2.0 && (state.pos[2] - b.2 as f64).abs() <= 3.0;
    if !near(head) && !near((head.0 - ox, head.1, head.2 - oz)) {
        return Err(Some("The bed is too far away"));
    }
    // getEntitiesWithinAABB(EntityMob, 8 x 5 x 8 around the bed) with isPreventingPlayerRest.
    let zone = Aabb::new(head.0 as f64 - 8.0, head.1 as f64 - 5.0, head.2 as f64 - 8.0, head.0 as f64 + 8.0, head.1 as f64 + 5.0, head.2 as f64 + 8.0);
    if game.mobs.mobs.iter().any(|m| !m.dead && mobs::spawn_lists::is_hostile(m.def) && m.body.bb.intersects(&zone)) {
        return Err(Some("You may not rest now, there are monsters nearby"));
    }
    // setPosition onto the bed, then sleeping = true.
    let to = [head.0 as f64 + (0.5f32 + ox as f32 * 0.4) as f64, head.1 as f64 + 0.6875f32 as f64, head.2 as f64 + (0.5f32 + oz as f32 * 0.4) as f64];
    teleport(game, id, to);
    game.beds.sleeping.insert(id, Sleeper { bed: head, timer: 0 });
    Ok(())
}

fn teleport(game: &mut Game, id: u32, to: [f64; 3]) {
    let (yaw, pitch) = game.sessions.get(&id).and_then(|s| s.state).map_or((0.0, 0.0), |s| (s.yaw, s.pitch));
    net::send(game, id, proto::S_TELEPORT, &proto::Teleport { pos: to, yaw, pitch, flags: 0 }.encode());
    players::on_teleport(game, id);
}

/// `BlockBed.getSafeExitLocation(world, pos, tries)` with `tries` 0.
pub fn safe_exit(game: &Game, pos: Pos) -> Option<Pos> {
    let s = terrain::state(game, pos);
    let (ox, oz) = HORIZONTAL[(s & 3) as usize];
    for l in 0..=1 {
        let i1 = pos.0 - ox * l - 1;
        let j1 = pos.2 - oz * l - 1;
        for i2 in i1..=i1 + 2 {
            for j2 in j1..=j1 + 2 {
                let p = (i2, pos.1, j2);
                // hasRoomForPlayer
                if wst::is_top_solid(terrain::state(game, (p.0, p.1 - 1, p.2)))
                    && !wst::mat_solid(terrain::state(game, p))
                    && !wst::mat_solid(terrain::state(game, (p.0, p.1 + 1, p.2)))
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// `EntityPlayer.setSpawnPoint(bed, false)`.
fn set_spawn_point(game: &mut Game, id: u32, bed: Pos) {
    let Some(name) = game.sessions.get(&id).map(|s| s.name.clone()) else { return };
    if let Some(w) = terrain::world(game) {
        w.lock().unwrap().update_player(&name, |s| {
            s.spawn = Some([bed.0, bed.1, bed.2]);
            s.bed = true;
        });
    }
}

/// `EntityPlayer.wakeUpPlayer(immediately, updateWorldFlag, setSpawn)`.
pub fn wake_up(game: &mut Game, id: u32, set_spawn: bool) {
    let Some(s) = game.beds.sleeping.remove(&id) else { return };
    if terrain::state(game, s.bed) >> 4 == block::BED {
        set_occupied(game, s.bed, false);
        let exit = safe_exit(game, s.bed).unwrap_or((s.bed.0, s.bed.1 + 1, s.bed.2));
        teleport(game, id, [exit.0 as f64 + 0.5f32 as f64, exit.1 as f64 + 0.1f32 as f64, exit.2 as f64 + 0.5f32 as f64]);
    }
    if set_spawn {
        set_spawn_point(game, id, s.bed);
    }
}

/// `damage::respawn`: a bed's spawn point puts the player at the bed's safe exit
/// (`EntityPlayer.getBedSpawnLocation`), and a bed that is gone sends them to the world spawn.
pub fn respawn_adjust(game: &Game, name: &str, tp: &mut proto::Teleport) {
    let Some(w) = terrain::world(game) else { return };
    let (saved, world_spawn) = {
        let w = w.lock().unwrap();
        (w.player(name), w.spawn)
    };
    let Some(saved) = saved else { return };
    let Some([x, y, z]) = saved.spawn.filter(|_| saved.bed) else { return };
    if terrain::state(game, (x, y, z)) >> 4 == block::BED {
        if let Some(p) = safe_exit(game, (x, y, z)) {
            tp.pos = [p.0 as f64 + 0.5f32 as f64, p.1 as f64 + 0.1f32 as f64, p.2 as f64 + 0.5f32 as f64];
            tp.flags = proto::TP_RESPAWN;
        }
    } else {
        let [x, y, z] = world_spawn;
        *tp = proto::Teleport { pos: [x as f64 + 0.5, y as f64, z as f64 + 0.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN | proto::TP_SURFACE };
    }
}

/// The other half of the bed at `pos` (`BlockBed.neighborChanged`): a foot looks at the cell its
/// facing points to, a head at the cell behind it.
pub fn other_half(state: u16, pos: Pos) -> Pos {
    let (ox, oz) = HORIZONTAL[(state & 3) as usize];
    if state & HEAD == 0 {
        (pos.0 + ox, pos.1, pos.2 + oz)
    } else {
        (pos.0 - ox, pos.1, pos.2 - oz)
    }
}

// ---- the tick ----

/// `WorldServer.tick`'s sleep check (`areAllPlayersAsleep`, then `wakeAllPlayers`), then
/// `EntityPlayer.onUpdate`'s sleep timer for each sleeper.
pub fn tick(game: &mut Game) {
    if game.beds.sleeping.is_empty() {
        return;
    }
    let awake: Vec<u32> = game
        .players
        .players
        .keys()
        .copied()
        .filter(|&id| players::mode(game, id) != GM_SPECTATOR)
        .collect();
    let all_asleep = !awake.is_empty() && awake.iter().all(|id| game.beds.sleeping.get(id).is_some_and(|s| s.timer >= FULLY_ASLEEP));
    if all_asleep {
        skip_to_morning(game);
        let mut ids: Vec<u32> = game.beds.sleeping.keys().copied().collect();
        ids.sort();
        for id in ids {
            wake_up(game, id, true);
        }
        if terrain::rule(game, "doWeatherCycle", true) {
            // resetRainAndThunder
            if let Some(w) = terrain::world(game) {
                w.lock().unwrap().weather = crate::world::Weather::default();
            }
        }
        return;
    }
    let mut ids: Vec<u32> = game.beds.sleeping.keys().copied().collect();
    ids.sort();
    for id in ids {
        let Some(s) = game.beds.sleeping.get_mut(&id) else { continue };
        s.timer = (s.timer + 1).min(FULLY_ASLEEP);
        let (bed, timer) = (s.bed, s.timer);
        // The teleport cleared the player's state, so the chunks may be packed until the viewer
        // answers with its next one.
        if !sim::collision::BlockAccess::loaded(&Blocks(game), bed.0, bed.2) {
            continue;
        }
        if terrain::state(game, bed) >> 4 != block::BED {
            wake_up(game, id, false);
        } else if is_daytime(game) {
            wake_up(game, id, true);
        } else if let Some(st) = game.sessions.get(&id).and_then(|s| s.state) {
            // The viewer's STOP_SLEEPING: sneaking, or walking off the bed once the teleport has
            // been taken.
            let (dx, dz) = (st.pos[0] - (bed.0 as f64 + 0.5), st.pos[2] - (bed.2 as f64 + 0.5));
            if (timer >= 2 && st.flags & SNEAKING != 0) || (timer >= 20 && dx * dx + dz * dz > 4.0) {
                wake_up(game, id, true);
            }
        }
    }
}

/// `WorldServer.tick`: `setWorldTime(time + 24000 - time % 24000)` when `doDaylightCycle`.
fn skip_to_morning(game: &mut Game) {
    if !terrain::rule(game, "doDaylightCycle", true) {
        return;
    }
    let Some(w) = terrain::world(game) else { return };
    let sync = {
        let mut w = w.lock().unwrap();
        let i = w.time() + 24000;
        w.set_time(i - i % 24000);
        w.time_sync(true).encode()
    };
    net::broadcast(game, None, proto::S_TIME, &sync);
}
