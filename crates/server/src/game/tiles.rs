//! Tile entities: signs (`TileEntitySign` and `NetHandlerPlayServer.processUpdateSign`), saved in
//! `tiles.json`.
//!
//! A sign's tile entity comes with its block: a block change that puts a standing or wall sign
//! where there was none makes an empty one, and a change that takes the sign away removes it
//! (`Chunk.setBlockState` replacing the tile entity when the block changes). The session whose
//! change placed it is the one `V_SIGN` may fill (`EntityPlayerMP.openEditSign` sets the player).
//! Every viewer gets each sign's lines as `S_SIGN` on join and after every edit.
//! DROPPED: chests and furnaces, and the sign's click commands (`executeCommand`).

use std::collections::HashMap;

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
    for &(pos, s) in changes {
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

pub fn tick(_game: &mut Game) {}

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
    serde_json::json!({ "signs": signs })
}

pub fn load(game: &mut Game, v: Value) {
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
