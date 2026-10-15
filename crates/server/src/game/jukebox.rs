//! `BlockJukebox` and `TileEntityJukebox`: a music disc right clicked on a jukebox goes in
//! (`ItemRecord.onItemUse`: `insertRecord`, `HAS_RECORD` true, world event 1010 with the disc's
//! item id), a click on a loaded jukebox ejects it (`onBlockActivated` -> `dropRecord`: event 1010
//! with 0, the disc thrown out above the block), and a jukebox that stops being one drops its disc
//! too (`breakBlock`). The disc is the tile's `RecordItem`, saved in `tiles.json`. The event goes
//! to every viewer within 64 blocks as `S_WORLD_EVENT`; the sound, the "Now Playing" text and the
//! stop are the viewer's (`survival/jukebox.rs`).
//!
//! DROPPED: the stat (`RECORD_PLAYED`), the comparator output, and the replay for a viewer that
//! joins while a disc plays (vanilla keeps no such state either: the sound is client side).

use ::items::ids::block as bid;
use ::items::tooltip::record_name;
use ::items::ItemStack;
use proto::{UseOn, SNEAKING};

use super::use_on::Used;
use super::{items, net, players, terrain, Game};

type At = (i32, i32, i32);

/// `World.playEvent`'s record event.
const EVENT_RECORD: i32 = 1010;

/// The block state is a jukebox of any `HAS_RECORD`.
pub fn is_jukebox(s: u16) -> bool {
    s >> 4 == bid::JUKEBOX
}

/// `HAS_RECORD`, `getMetaFromState`.
fn has_record(s: u16) -> bool {
    s & 15 > 0
}

/// `ItemRecord`
pub fn is_record_item(s: &ItemStack) -> bool {
    !s.is_empty() && record_name(s.item).is_some()
}

/// `World.playEvent(1010, pos, data)` to the players within 64 blocks (`sendToAllNearExcept`).
fn play_event(game: &Game, at: At, data: i32) {
    let m = proto::WorldEvent { event: EVENT_RECORD, pos: [at.0, at.1, at.2], data };
    net::send_near(game, None, [at.0 as f64 + 0.5, at.1 as f64 + 0.5, at.2 as f64 + 0.5], 64.0, proto::S_WORLD_EVENT, &m.encode());
}

/// The tile of a jukebox block that has none yet, and the disc of a jukebox that is gone.
pub fn on_changes(game: &mut Game, changes: &[proto::BlockChange]) {
    for &(pos, s) in changes {
        if is_jukebox(s) {
            game.tiles.jukeboxes.entry(pos).or_insert(ItemStack::EMPTY);
        } else if game.tiles.jukeboxes.contains_key(&pos) {
            // BlockJukebox.breakBlock: dropRecord, then the tile goes.
            drop_record(game, pos);
            game.tiles.jukeboxes.remove(&pos);
        }
    }
}

/// `BlockJukebox.dropRecord`: event 1010 with 0 and the disc as an item entity above the block
/// with the default pickup delay. Nothing happens for an empty jukebox.
pub fn drop_record(game: &mut Game, at: At) {
    let Some(rec) = game.tiles.jukeboxes.get_mut(&at) else { return };
    if rec.is_empty() {
        return;
    }
    let stack = std::mem::replace(rec, ItemStack::EMPTY);
    play_event(game, at, 0);
    let r = &mut game.items.rand;
    let d0 = r.next_float() as f64 * 0.7f32 as f64 + 0.150_000_005_960_464_48;
    let d1 = r.next_float() as f64 * 0.7f32 as f64 + 0.060_000_002_384_185_79 + 0.6;
    let d2 = r.next_float() as f64 * 0.7f32 as f64 + 0.150_000_005_960_464_48;
    items::spawn_stack_at(game, [at.0 as f64 + d0, at.1 as f64 + d1, at.2 as f64 + d2], stack, None, 10);
}

/// `BlockJukebox.onBlockActivated` through `processRightClickBlock`: a jukebox holding a disc
/// ejects it, whatever the player holds; a sneaking player with an item in hand uses the item
/// instead. False when the block is not a loaded jukebox, so the click goes on to the item.
pub fn use_on(game: &mut Game, id: u32, m: &UseOn) -> bool {
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let s = terrain::state(game, at);
    if !is_jukebox(s) || !has_record(s) {
        return false;
    }
    let held_any = game.players.players.get(&id).is_some_and(|pl| !pl.inv.get_current_item().is_empty() || !pl.inv.off_hand[0].is_empty());
    let sneaking = game.sessions.get(&id).and_then(|s| s.state).is_some_and(|s| s.flags & SNEAKING != 0);
    if sneaking && held_any {
        return false;
    }
    if players::mode(game, id) == proto::GM_SPECTATOR {
        return false;
    }
    drop_record(game, at);
    terrain::set_block(game, at, bid::JUKEBOX << 4);
    true
}

/// `ItemRecord.onItemUse`: onto an empty jukebox the disc goes in (`insertRecord`) and event 1010
/// carries its id. SUCCESS consumes one disc outside creative.
pub fn item_use(game: &mut Game, _id: u32, m: &UseOn, stack: ItemStack) -> Used {
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let s = terrain::state(game, at);
    if !is_jukebox(s) || has_record(s) {
        return Used::No;
    }
    game.tiles.jukeboxes.insert(at, stack);
    terrain::set_block(game, at, bid::JUKEBOX << 4 | 1);
    play_event(game, at, stack.item as i32);
    Used::Success { consume: true }
}

#[cfg(test)]
mod tests {
    use ::items::ids::item;

    use super::*;
    use crate::game::testing::{run, Harness};
    use crate::game::GameMsg;

    const JUKEBOX: u16 = bid::JUKEBOX << 4;
    const AT: At = (8, 201, 8);

    fn setup() -> Harness {
        let mut h = Harness::new(7, 2);
        h.join(1, "Xy", [5.5, 230.0, 5.5]);
        h.settle();
        for x in 4..=13 {
            for z in 4..=13 {
                terrain::set_block(&mut h.game, (x, 200, z), ::items::ids::block::STONE << 4);
            }
        }
        h.game.sessions.get_mut(&1).unwrap().state = Some(proto::PlayerState { pos: [5.5, 201.0, 5.5], flags: proto::ON_GROUND, ..Default::default() });
        terrain::set_block(&mut h.game, AT, JUKEBOX);
        h.tick(1);
        h.frames_for(1);
        h
    }

    fn give(h: &mut Harness, s: ItemStack) {
        let p = h.game.players.players.get_mut(&1).unwrap();
        let c = p.inv.current_item;
        p.inv.main[c] = s;
    }

    fn held(h: &Harness) -> ItemStack {
        let p = &h.game.players.players[&1];
        p.inv.main[p.inv.current_item]
    }

    fn click(h: &mut Harness) {
        h.push_frame(1, proto::V_USE_ON, &proto::UseOn { pos: [AT.0, AT.1, AT.2], face: 1, hit: [0.5; 3], hand: 0, state: 0 }.encode());
        h.tick(1);
    }

    fn events(h: &mut Harness) -> Vec<proto::WorldEvent> {
        h.frames_for(1).iter().filter(|f| f.0 == proto::S_WORLD_EVENT).filter_map(|f| proto::WorldEvent::decode(&f.1)).collect()
    }

    #[test]
    fn a_disc_goes_in_and_comes_out() {
        run(|| {
            let mut h = setup();
            assert!(h.game.tiles.jukeboxes.contains_key(&AT), "the block comes with its tile");
            give(&mut h, ItemStack::new(item::RECORD_CAT, 1, 0));
            click(&mut h);
            assert_eq!(terrain::state(&h.game, AT), JUKEBOX | 1);
            assert_eq!(h.game.tiles.jukeboxes[&AT].item, item::RECORD_CAT);
            assert!(held(&h).is_empty(), "the disc is used up");
            let ev = events(&mut h);
            assert_eq!(ev, vec![proto::WorldEvent { event: 1010, pos: [8, 201, 8], data: item::RECORD_CAT as i32 }]);
            // A second click ejects it: event 1010 with 0, the disc as an item above the block.
            click(&mut h);
            assert_eq!(terrain::state(&h.game, AT), JUKEBOX);
            assert!(h.game.tiles.jukeboxes[&AT].is_empty());
            assert_eq!(events(&mut h), vec![proto::WorldEvent { event: 1010, pos: [8, 201, 8], data: 0 }]);
            let drops: Vec<_> = h.game.items.entities.iter().filter(|e| e.stack.item == item::RECORD_CAT).collect();
            assert_eq!(drops.len(), 1);
            let p = drops[0].body.pos;
            assert!((8.0..9.0).contains(&p.x) && (201.6..202.6).contains(&p.y) && (8.0..9.0).contains(&p.z), "{p:?}");
        });
    }

    #[test]
    fn a_full_jukebox_takes_no_second_disc_and_a_creative_player_keeps_theirs() {
        run(|| {
            let mut h = setup();
            h.send(GameMsg::GameMode { id: 1, mode: proto::GM_CREATIVE });
            give(&mut h, ItemStack::new(item::RECORD_13, 1, 0));
            click(&mut h);
            assert_eq!(held(&h).item, item::RECORD_13, "creative keeps the stack");
            assert_eq!(h.game.tiles.jukeboxes[&AT].item, item::RECORD_13);
            // The disc in hand is not inserted into a full jukebox: the click ejects.
            give(&mut h, ItemStack::new(item::RECORD_WAIT, 1, 0));
            click(&mut h);
            assert!(h.game.tiles.jukeboxes[&AT].is_empty());
            assert_eq!(terrain::state(&h.game, AT), JUKEBOX);
        });
    }

    #[test]
    fn breaking_the_jukebox_drops_the_disc_and_stops_it() {
        run(|| {
            let mut h = setup();
            give(&mut h, ItemStack::new(item::RECORD_STAL, 1, 0));
            click(&mut h);
            events(&mut h);
            terrain::set_block(&mut h.game, AT, 0);
            assert!(!h.game.tiles.jukeboxes.contains_key(&AT));
            assert_eq!(h.game.items.entities.iter().filter(|e| e.stack.item == item::RECORD_STAL).count(), 1);
            assert_eq!(events(&mut h), vec![proto::WorldEvent { event: 1010, pos: [8, 201, 8], data: 0 }]);
        });
    }

    #[test]
    fn the_disc_survives_a_save() {
        run(|| {
            let mut h = setup();
            give(&mut h, ItemStack::new(item::RECORD_MALL, 1, 0));
            click(&mut h);
            let saved = crate::game::tiles::save(&h.game);
            let mut h2 = setup();
            h2.game.tiles.jukeboxes.clear();
            crate::game::tiles::load(&mut h2.game, saved);
            assert_eq!(h2.game.tiles.jukeboxes[&AT].item, item::RECORD_MALL);
        });
    }
}
