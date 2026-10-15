//! Tile entities: signs (`TileEntitySign` and `NetHandlerPlayServer.processUpdateSign`), saved in
//! `tiles.json`.
//!
//! A sign's tile entity comes with its block: a block change that puts a standing or wall sign
//! where there was none makes an empty one, and a change that takes the sign away removes it
//! (`Chunk.setBlockState` replacing the tile entity when the block changes). The session whose
//! change placed it is the one `V_SIGN` may fill (`EntityPlayerMP.openEditSign` sets the player).
//! Every viewer gets each sign's lines as `S_SIGN` on join and after every edit.
//! T8 adds `TileEntityChest` and `TileEntityFurnace`: made with their block (or on the first open
//! for a block worldgen placed), emptied onto the ground when the block goes
//! (`BlockChest.breakBlock`, `BlockFurnace.breakBlock` through `InventoryHelper.
//! dropInventoryItems`), saved in `tiles.json`. The furnace ticks only where its chunk is loaded
//! and swaps `FURNACE` and `LIT_FURNACE` with `set_block` (`BlockFurnace.setState`). A chest
//! counts its viewers and tells everyone within 64 blocks with `S_BLOCK_ACTION` (the lid).
//! The windows over them are `windows.rs`, which calls [`open_chest`], [`open_furnace`],
//! [`load_into`], [`store_from`], [`on_opened`], [`on_closed`] and [`usable`].
//!
//! `TileEntityJukebox` (the disc) is `jukebox.rs`, kept in `State::jukeboxes` and saved here.
//!
//! DROPPED: the sign's click commands (`executeCommand`); dungeon and village chest loot
//! (`TileEntityLockableLoot` tables: the generated chests are empty); the chest open and close
//! sounds; trapped chests, ender chests and hoppers; the ocelot check of `BlockChest.isBlocked`.

use std::collections::HashMap;

use ::items::ids::block as bid;
use ::items::ItemStack;
use proto::{BlockChange, SignText};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use worldgen::blocks::id;

use super::{net, Game};

/// One `TileEntitySign`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sign {
    /// The block the sign belongs to: standing or wall.
    pub block: u16,
    /// `signText`, without formatting codes.
    pub lines: [String; 4],
    /// `getPlayer()` while `getIsEditable()`: the session that placed the sign. A sign read from
    /// the save is not editable (`readFromNBT` clears `isEditable`).
    pub editor: Option<u32>,
}

#[derive(Default)]
pub struct State {
    pub signs: HashMap<(i32, i32, i32), Sign>,
    /// `TileEntityChest.chestContents`, 27 stacks.
    pub chests: HashMap<(i32, i32, i32), Vec<ItemStack>>,
    pub furnaces: HashMap<(i32, i32, i32), ::items::Furnace>,
    /// `TileEntityJukebox.record`, empty when no disc plays.
    pub jukeboxes: HashMap<(i32, i32, i32), ItemStack>,
    /// `TileEntityChest.numPlayersUsing`
    pub users: HashMap<(i32, i32, i32), u8>,
}

type At = (i32, i32, i32);

fn is_chest(s: u16) -> bool {
    s >> 4 == bid::CHEST
}

fn is_furnace(s: u16) -> bool {
    matches!(s >> 4, bid::FURNACE | bid::LIT_FURNACE)
}

/// The block is a standing or wall sign.
pub fn is_sign(s: u16) -> bool {
    matches!(s >> 4, id::STANDING_SIGN | id::WALL_SIGN)
}

fn text_of(pos: (i32, i32, i32), sign: &Sign) -> SignText {
    SignText { pos: [pos.0, pos.1, pos.2], lines: sign.lines.clone() }
}

pub fn on_join(game: &mut Game, id: u32) {
    for (&pos, sign) in &game.tiles.signs {
        net::send(game, id, proto::S_SIGN, &text_of(pos, sign).encode());
    }
}

/// The player is gone, so no later session can pass as the one that placed a sign.
pub fn on_leave(game: &mut Game, id: u32) {
    for sign in game.tiles.signs.values_mut() {
        if sign.editor == Some(id) {
            sign.editor = None;
        }
    }
}

pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    if tag != proto::V_SIGN {
        return false;
    }
    if let Some(m) = SignText::decode(body) {
        process_update_sign(game, id, m);
    }
    true
}

/// `TextFormatting.getTextWithoutFormattingCodes`: drops `(?i)§[0-9A-FK-OR]`.
pub fn strip_formatting(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{a7}' {
            if let Some(&n) = chars.peek() {
                if matches!(n.to_ascii_lowercase(), '0'..='9' | 'a'..='f' | 'k'..='o' | 'r') {
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// `NetHandlerPlayServer.processUpdateSign`.
/// DROPPED: `markPlayerActive` (no idle kick) and the loaded-chunk check; a sign only exists
/// where its block was set, so its chunk is in the world.
fn process_update_sign(game: &mut Game, id: u32, m: SignText) {
    let pos = (m.pos[0], m.pos[1], m.pos[2]);
    let Some(sign) = game.tiles.signs.get_mut(&pos) else { return };
    if sign.editor != Some(id) {
        let name = game.sessions.get(&id).map_or("?", |s| s.name.as_str());
        println!("Player {name} just tried to change non-editable sign");
        return;
    }
    sign.lines = m.lines.map(|l| strip_formatting(&l));
    // markDirty and notifyBlockUpdate: everyone gets the new lines.
    let body = text_of(pos, sign).encode();
    net::broadcast(game, None, proto::S_SIGN, &body);
}

/// Block changes as they reach the world, from a session (`id`) or from the server (`None`): a
/// new sign block gets an empty sign, a sign block gone takes its sign with it.
pub fn on_changes(game: &mut Game, id: Option<u32>, changes: &[BlockChange]) {
    super::jukebox::on_changes(game, changes);
    for &(pos, s) in changes {
        if is_chest(s) {
            game.tiles.chests.entry(pos).or_insert_with(|| vec![ItemStack::EMPTY; 27]);
        } else if game.tiles.chests.contains_key(&pos) {
            let items = game.tiles.chests.remove(&pos).unwrap();
            game.tiles.users.remove(&pos);
            drop_inventory(game, pos, items);
        }
        if is_furnace(s) {
            game.tiles.furnaces.entry(pos).or_default();
        } else if game.tiles.furnaces.contains_key(&pos) {
            let f = game.tiles.furnaces.remove(&pos).unwrap();
            drop_inventory(game, pos, f.items.to_vec());
        }
        if is_sign(s) {
            let block = s >> 4;
            if game.tiles.signs.get(&pos).is_some_and(|t| t.block == block) {
                continue;
            }
            game.tiles.signs.insert(pos, Sign { block, lines: Default::default(), editor: id });
        } else {
            game.tiles.signs.remove(&pos);
        }
    }
}

/// `InventoryHelper.dropInventoryItems`: each stack at a random spot of the block in pieces of 10
/// to 30, thrown with a small gaussian motion.
fn drop_inventory(game: &mut Game, at: At, items: Vec<ItemStack>) {
    for mut stack in items {
        if stack.is_empty() {
            continue;
        }
        let r = &mut game.items.rand;
        let (f, f1, f2) = (r.next_float() * 0.8 + 0.1, r.next_float() * 0.8 + 0.1, r.next_float() * 0.8 + 0.1);
        while !stack.is_empty() {
            let n = game.items.rand.next_int_bounded(21) + 10;
            let piece = stack.split_stack(n);
            let r = &mut game.items.rand;
            let m = [r.next_gaussian() * 0.05f32 as f64, r.next_gaussian() * 0.05f32 as f64 + 0.200_000_002_980_232_24, r.next_gaussian() * 0.05f32 as f64];
            let pos = [at.0 as f64 + f as f64, at.1 as f64 + f1 as f64, at.2 as f64 + f2 as f64];
            super::items::spawn_stack_at(game, pos, piece, Some(m), 10);
        }
    }
}

/// `BlockChest.isBlocked`/`getContainer`: the chest at `pos` and the chest beside it, if the
/// block above is not an opaque cube. Returns the first half, the second half and the contents
/// of both (the west or north chest first), making the tiles of chests worldgen placed.
pub fn open_chest(game: &mut Game, pos: At) -> Option<([i32; 3], Option<[i32; 3]>, Vec<ItemStack>)> {
    use super::terrain::state;
    if !is_chest(state(game, pos)) {
        return None;
    }
    let blocked = |g: &Game, p: At| sim::region::props(state(g, (p.0, p.1 + 1, p.2))).opaque_cube;
    if blocked(game, pos) {
        return None;
    }
    let mut halves = vec![pos];
    // EnumFacing.Plane.HORIZONTAL order: north, east, south, west; the first same chest wins.
    for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
        let q = (pos.0 + dx, pos.1, pos.2 + dz);
        if state(game, q) >> 4 == state(game, pos) >> 4 {
            if blocked(game, q) {
                return None;
            }
            // A neighbour to the west or north is the upper half.
            if dx < 0 || dz < 0 {
                halves.insert(0, q);
            } else {
                halves.push(q);
            }
            break;
        }
    }
    let mut contents = Vec::new();
    for &h in &halves {
        contents.extend(game.tiles.chests.entry(h).or_insert_with(|| vec![ItemStack::EMPTY; 27]).iter().copied());
    }
    let a = halves[0];
    let other = halves.get(1).map(|b| [b.0, b.1, b.2]);
    Some(([a.0, a.1, a.2], other, contents))
}

/// The furnace tile at `pos`, if the block is one.
pub fn open_furnace(game: &mut Game, pos: At) -> Option<::items::Furnace> {
    if !is_furnace(super::terrain::state(game, pos)) {
        return None;
    }
    Some(game.tiles.furnaces.entry(pos).or_default().clone())
}

fn half_positions(kind: &sim::container::ContainerKind) -> Vec<At> {
    use sim::container::ContainerKind::Chest;
    match kind {
        Chest { pos, other, .. } => std::iter::once(*pos).chain(*other).map(|p| (p[0], p[1], p[2])).collect(),
        _ => Vec::new(),
    }
}

/// The world's tiles into a window's copy (before a click and each tick), so players sharing a
/// chest see each other's changes.
pub fn load_into(game: &Game, c: &mut sim::container::Container) {
    use sim::container::{ContainerKind, Tile};
    match c.kind {
        ContainerKind::Chest { .. } => {
            let mut all = Vec::new();
            for h in half_positions(&c.kind) {
                match game.tiles.chests.get(&h) {
                    Some(v) => all.extend(v.iter().copied()),
                    None => return,
                }
            }
            c.tile = Some(Tile::Chest(all));
        }
        ContainerKind::Furnace { pos } => {
            if let Some(f) = game.tiles.furnaces.get(&(pos[0], pos[1], pos[2])) {
                c.tile = Some(Tile::Furnace(f.clone()));
            }
        }
        _ => {}
    }
}

/// A window's copy back into the world's tiles (after a click).
pub fn store_from(game: &mut Game, c: &sim::container::Container) {
    use sim::container::{ContainerKind, Tile};
    match (&c.kind, &c.tile) {
        (ContainerKind::Chest { .. }, Some(Tile::Chest(all))) => {
            for (i, h) in half_positions(&c.kind).into_iter().enumerate() {
                if let Some(v) = game.tiles.chests.get_mut(&h) {
                    v.copy_from_slice(&all[i * 27..i * 27 + 27]);
                }
            }
        }
        (ContainerKind::Furnace { pos }, Some(Tile::Furnace(f))) => {
            if let Some(t) = game.tiles.furnaces.get_mut(&(pos[0], pos[1], pos[2])) {
                *t = f.clone();
            }
        }
        _ => {}
    }
}

/// `canInteractWith`: the tile and its block are still there and the player is within 8 blocks
/// of it (`TileEntityChest.isUsableByPlayer`, `TileEntityFurnace.isUsableByPlayer`).
pub fn usable(game: &Game, kind: &sim::container::ContainerKind, player: [f64; 3]) -> bool {
    use sim::container::ContainerKind;
    let near = |p: [i32; 3]| {
        let d: f64 = (0..3).map(|i| (player[i] - (p[i] as f64 + 0.5)).powi(2)).sum();
        d <= 64.0
    };
    match *kind {
        ContainerKind::Chest { pos, .. } => {
            half_positions(kind).iter().all(|h| is_chest(super::terrain::state(game, *h)) && game.tiles.chests.contains_key(h)) && near(pos)
        }
        ContainerKind::Furnace { pos } => {
            let at = (pos[0], pos[1], pos[2]);
            is_furnace(super::terrain::state(game, at)) && game.tiles.furnaces.contains_key(&at) && near(pos)
        }
        _ => true,
    }
}

/// `TileEntityChest.openInventory`/`closeInventory`: counts the viewer and tells everyone near
/// (`SPacketBlockAction`: action 1, parameter the number of viewers).
fn lid(game: &mut Game, at: At, delta: i8) {
    let n = game.tiles.users.entry(at).or_insert(0);
    *n = (*n as i16 + delta as i16).max(0) as u8;
    let n = *n;
    if n == 0 {
        game.tiles.users.remove(&at);
    }
    let block = super::terrain::state(game, at) >> 4;
    let m = proto::BlockAction { pos: [at.0, at.1, at.2], block, a: 1, b: n };
    net::send_near(game, None, [at.0 as f64 + 0.5, at.1 as f64 + 0.5, at.2 as f64 + 0.5], 64.0, proto::S_BLOCK_ACTION, &m.encode());
}

/// A chest window opened (both halves of a large chest count the viewer).
pub fn on_opened(game: &mut Game, kind: &sim::container::ContainerKind) {
    for h in half_positions(kind) {
        lid(game, h, 1);
    }
}

/// A window closed: a chest loses a viewer.
pub fn on_closed(game: &mut Game, kind: &sim::container::ContainerKind) {
    for h in half_positions(kind) {
        lid(game, h, -1);
    }
}

/// `TileEntityFurnace.update` for every furnace whose chunk is loaded (its block is there), and
/// `BlockFurnace.setState` when it lights or goes out.
pub fn tick(game: &mut Game) {
    let mut at: Vec<At> = game.tiles.furnaces.keys().copied().collect();
    at.sort();
    for pos in at {
        let s = super::terrain::state(game, pos);
        if !is_furnace(s) {
            continue;
        }
        let t = game.tiles.furnaces.get_mut(&pos).unwrap().update();
        if t.lit_changed {
            let lit = game.tiles.furnaces[&pos].is_burning();
            let block = if lit { bid::LIT_FURNACE } else { bid::FURNACE };
            super::terrain::set_block(game, pos, block << 4 | (s & 15));
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    pos: [i32; 3],
    block: u16,
    lines: [String; 4],
}

/// Every sign as `{pos, block, lines}`, sorted by position so the file is stable.
pub fn save(game: &Game) -> Value {
    let mut signs: Vec<Saved> =
        game.tiles.signs.iter().map(|(&p, s)| Saved { pos: [p.0, p.1, p.2], block: s.block, lines: s.lines.clone() }).collect();
    signs.sort_by_key(|s| s.pos);
    let stacks = |items: &[ItemStack]| -> Vec<Value> {
        items.iter().enumerate().filter_map(|(i, s)| super::playerdata::stack_json(i as i32, s)).collect()
    };
    let mut chests: Vec<_> = game.tiles.chests.iter().collect();
    chests.sort_by_key(|(p, _)| **p);
    let chests: Vec<Value> = chests.into_iter().map(|(p, v)| serde_json::json!({ "pos": [p.0, p.1, p.2], "items": stacks(v) })).collect();
    let mut furnaces: Vec<_> = game.tiles.furnaces.iter().collect();
    furnaces.sort_by_key(|(p, _)| **p);
    let furnaces: Vec<Value> = furnaces
        .into_iter()
        .map(|(p, f)| {
            serde_json::json!({ "pos": [p.0, p.1, p.2], "items": stacks(&f.items), "burn": f.burn_time, "current": f.current_item_burn_time, "cook": f.cook_time, "total": f.total_cook_time })
        })
        .collect();
    let mut jukeboxes: Vec<_> = game.tiles.jukeboxes.iter().filter(|(_, s)| !s.is_empty()).collect();
    jukeboxes.sort_by_key(|(p, _)| **p);
    let jukeboxes: Vec<Value> = jukeboxes.into_iter().map(|(p, s)| serde_json::json!({ "pos": [p.0, p.1, p.2], "items": stacks(std::slice::from_ref(s)) })).collect();
    serde_json::json!({ "signs": signs, "chests": chests, "furnaces": furnaces, "jukeboxes": jukeboxes })
}

fn pos_of(v: &Value) -> Option<At> {
    let a = v.get("pos")?.as_array()?;
    Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32, a.get(2)?.as_i64()? as i32))
}

fn stacks_of(v: &Value, out: &mut [ItemStack]) {
    for s in v.get("items").and_then(Value::as_array).into_iter().flatten() {
        if let Some((slot, st)) = super::playerdata::stack_from(s) {
            if let Some(o) = out.get_mut(slot as usize) {
                *o = st;
            }
        }
    }
}

pub fn load(game: &mut Game, v: Value) {
    for c in v.get("chests").and_then(Value::as_array).into_iter().flatten() {
        if let Some(p) = pos_of(c) {
            let mut items = vec![ItemStack::EMPTY; 27];
            stacks_of(c, &mut items);
            game.tiles.chests.insert(p, items);
        }
    }
    for c in v.get("furnaces").and_then(Value::as_array).into_iter().flatten() {
        if let Some(p) = pos_of(c) {
            let n = |k: &str| c.get(k).and_then(Value::as_i64).unwrap_or(0) as i32;
            let mut f = ::items::Furnace { burn_time: n("burn"), current_item_burn_time: n("current"), cook_time: n("cook"), total_cook_time: n("total"), ..Default::default() };
            stacks_of(c, &mut f.items);
            game.tiles.furnaces.insert(p, f);
        }
    }
    for c in v.get("jukeboxes").and_then(Value::as_array).into_iter().flatten() {
        if let Some(p) = pos_of(c) {
            let mut rec = [ItemStack::EMPTY];
            stacks_of(c, &mut rec);
            game.tiles.jukeboxes.insert(p, rec[0]);
        }
    }
    let Some(list) = v.get("signs").cloned() else { return };
    let Ok(signs) = serde_json::from_value::<Vec<Saved>>(list) else {
        println!("tiles.json: the signs did not read");
        return;
    };
    for s in signs {
        let lines = s.lines.map(|l| l.chars().take(proto::SIGN_LINE_MAX).collect());
        game.tiles.signs.insert((s.pos[0], s.pos[1], s.pos[2]), Sign { block: s.block, lines, editor: None });
    }
}
