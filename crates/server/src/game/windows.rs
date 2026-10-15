//! Server windows (T7a): the player's inventory window (id 0, always there), the crafting table
//! window, clicks, closes, creative slot changes, the selected slot, swapping hands and the
//! equipment other players see. The containers are `sim::container`; this file is the network
//! half of `NetHandlerPlayServer` (`processClickWindow`, `processCloseWindow`,
//! `processCreativeInventoryAction`, `processHeldItemChange`, `processPlayerDigging`
//! SWAP_HELD_ITEMS, `processTryUseItemOnBlock` for the crafting table) and of `EntityPlayerMP`
//! (`getNextWindowId`, `displayGui`, `closeContainer`, `onUpdate`'s `detectAndSendChanges` and
//! `canInteractWith` check).
//!
//! The inventory itself is `Survival::inv` (persisted by playerdata.rs). A window holds only the
//! crafting grid and the result; the cursor stack is `InventoryPlayer::cursor`.
//!
//! Sync model: the viewer predicts every click with the same `sim::container` code, so an accepted
//! click is answered by `S_CONFIRM` alone. When the server's window had changed behind the
//! viewer's back (a pickup, a swap), the whole window is sent after the click. Changes made
//! outside a click (pickups, drops, hand swaps) go out as an `S_SLOTS` diff from [`tick`].
//!
//! T8: chests and furnaces open the same way (`open_chest`, `open_furnace`). The container holds a
//! copy of the world's tile ([`sim::container::Tile`]): it is refreshed from `tiles.rs` before a
//! click and each tick, and stored back after a click, so every viewer of one chest sees the
//! others' changes as `S_SLOTS`; the furnace fields go out as `S_WINDOW_PROP`.
//!
//! DROPPED: `isChangingQuantityOnly` and `pendingTransactions` (the viewer sends no clicked item,
//! so there is no transaction to refuse), the `BlockEntityTag` check of creative stacks, the
//! recipe book, `markPlayerActive`, `resetActiveHand` and the off hand attribute modifiers.

use std::collections::HashMap;
use std::sync::Arc;

use ::items::stack::are_item_stacks_equal;
use ::items::{ItemStack, Recipes, Tag};
use proto::{WireStack, SNEAKING};
use serde_json::Value;
use sim::container::{ClickType, Container, ContainerKind, PlayerRef};

use super::{items as item_entities, net, terrain, Game};

/// The block id of the crafting table (`Blocks.CRAFTING_TABLE`).
const CRAFTING_TABLE: u16 = ::items::ids::block::CRAFTING_TABLE;
/// `NetHandlerPlayServer.itemDropThreshold` ceiling and step.
const DROP_LIMIT: i32 = 200;
const DROP_STEP: i32 = 20;

/// One player's windows.
pub struct PlayerWin {
    /// `inventoryContainer`
    pub inv: Container,
    /// `openContainer` when it is not the inventory.
    pub open: Option<Container>,
    /// `EntityPlayerMP.currentWindowId`
    pub window_counter: u8,
    /// The cursor stack the viewer last had.
    sent_cursor: ItemStack,
    /// `itemDropThreshold`
    pub drop_threshold: i32,
    /// The equipment the others last saw: main hand, off hand, feet, legs, chest, head.
    equip: [ItemStack; 6],
}

impl PlayerWin {
    fn new(recipes: Arc<Recipes>) -> PlayerWin {
        PlayerWin { inv: Container::player(recipes), open: None, window_counter: 0, sent_cursor: ItemStack::EMPTY, drop_threshold: 0, equip: [ItemStack::EMPTY; 6] }
    }

    /// `openContainer`
    pub fn active(&mut self) -> &mut Container {
        match &mut self.open {
            Some(c) => c,
            None => &mut self.inv,
        }
    }

    /// The window id the viewer has on screen.
    pub fn active_id(&self) -> u8 {
        self.open.as_ref().map_or(0, |c| c.window_id as u8)
    }
}

#[derive(Default)]
pub struct State {
    pub players: HashMap<u32, PlayerWin>,
    /// The recipes every window crafts with; the jar's, read when the first player joins.
    pub recipes: Option<Arc<Recipes>>,
}

/// `CraftingManager`: the client jar's recipes, once. Tests inject their own through
/// `State::recipes`; without a jar only the code recipes exist.
pub fn recipes(game: &mut Game) -> Arc<Recipes> {
    if game.windows.recipes.is_none() {
        #[cfg(not(test))]
        let r = {
            let jar = if game.cfg.jar.is_empty() { jar::Jar::find() } else { jar::Jar::open(std::path::Path::new(&game.cfg.jar)) };
            match jar.map_err(|e| e.to_string()).and_then(|j| Recipes::from_jar(j.names(), |n| j.read(n).ok()).map_err(|e| e.to_string())) {
                Ok(r) => r,
                Err(e) => {
                    println!("windows: no recipes from the jar ({e}); only the code recipes craft");
                    Recipes::code_recipes()
                }
            }
        };
        #[cfg(test)]
        let r = Recipes::code_recipes();
        game.windows.recipes = Some(Arc::new(r));
    }
    game.windows.recipes.clone().unwrap()
}

fn ensure(game: &mut Game, id: u32) {
    if !game.windows.players.contains_key(&id) {
        let r = recipes(game);
        game.windows.players.insert(id, PlayerWin::new(r));
    }
}

fn survival_client(game: &Game, id: u32) -> bool {
    game.sessions.get(&id).is_some_and(|s| s.survival_client)
}

/// A wire stack as an `ItemStack`; ids the registry does not know are empty.
pub fn stack_of(w: &WireStack) -> ItemStack {
    if w.id < 0 || w.count == 0 || ::items::registry::item(w.id as u16).is_none() {
        return ItemStack::EMPTY;
    }
    let tag = match w.tag {
        [1, p, _] => Tag::Potion(p),
        [3, e, _] => Tag::Entity(e),
        _ => Tag::None,
    };
    ItemStack::new(w.id as u16, w.count as i32, w.meta as i32).with_tag(tag).with_ench(::items::EnchList::from_bytes(&w.ench))
}

pub(super) fn wire(s: &ItemStack) -> WireStack {
    if s.is_empty() {
        WireStack::EMPTY
    } else {
        item_entities::wire_stack(s)
    }
}

/// The window and survival state of player `id`, as disjoint borrows.
fn parts(game: &mut Game, id: u32) -> Option<(&mut PlayerWin, &mut super::players::Survival)> {
    let w = game.windows.players.get_mut(&id)?;
    let p = game.players.players.get_mut(&id)?;
    Some((w, p))
}

/// Hands the stacks a window threw (`EntityPlayer.dropItem(stack, false)`) to the world.
fn throw(game: &mut Game, id: u32, dropped: Vec<ItemStack>) {
    for s in dropped {
        super::players::drop_item(game, id, s, false, false);
    }
}

// ---- sending ----

/// The whole of the window on screen, and the cursor, as `SPacketWindowItems`. The window's idea
/// of what the viewer has becomes this. Survival viewers only.
pub fn send_full(game: &mut Game, id: u32) {
    if !survival_client(game, id) {
        return;
    }
    ensure(game, id);
    let creative = super::players::mode(game, id) == proto::GM_CREATIVE;
    let Some((w, p)) = parts(game, id) else { return };
    let pr = PlayerRef::new(&mut p.inv, creative);
    let window = w.active_id();
    let c = w.active();
    c.detect_and_send_changes(&pr);
    c.listener.clear();
    let mut slots: Vec<(i16, WireStack)> = (0..c.slots.len()).map(|i| (i as i16, wire(&c.get_stack(&pr, i)))).collect();
    let cursor = pr.inventory.cursor;
    slots.push((-1, wire(&cursor)));
    w.sent_cursor = cursor;
    net::send(game, id, proto::S_SLOTS, &proto::WindowSlots { window, slots }.encode());
    send_props(game, id);
}

/// The furnace fields `detectAndSendChanges` found as `S_WINDOW_PROP`.
fn send_props(game: &mut Game, id: u32) {
    let Some(w) = game.windows.players.get_mut(&id) else { return };
    let Some(c) = w.open.as_mut() else { return };
    let window = c.window_id as u8;
    let props: Vec<(u16, i32)> = c.prop_listener.drain(..).collect();
    for (property, value) in props {
        net::send(game, id, proto::S_WINDOW_PROP, &proto::WindowProp { window, property, value: value as i16 }.encode());
    }
}

/// Whether the window on screen differs from what the viewer has: some slot or the cursor.
fn out_of_date(game: &mut Game, id: u32) -> bool {
    let creative = super::players::mode(game, id) == proto::GM_CREATIVE;
    let Some((w, p)) = parts(game, id) else { return false };
    let pr = PlayerRef::new(&mut p.inv, creative);
    let cursor_differs = !are_item_stacks_equal(&w.sent_cursor, &pr.inventory.cursor);
    let c = w.active();
    c.detect_and_send_changes(&pr);
    cursor_differs || !c.listener.is_empty()
}

/// `Container.detectAndSendChanges` for the window on screen (or the inventory window when
/// `inventory` is set): the slots that changed since the viewer was told, and the cursor.
fn send_changes(game: &mut Game, id: u32, inventory: bool) {
    if !survival_client(game, id) {
        return;
    }
    let creative = super::players::mode(game, id) == proto::GM_CREATIVE;
    let Some((w, p)) = parts(game, id) else { return };
    let pr = PlayerRef::new(&mut p.inv, creative);
    let on_screen = w.open.is_none() || inventory;
    let window = if inventory { 0 } else { w.active_id() };
    let c = if inventory { &mut w.inv } else { w.active() };
    c.detect_and_send_changes(&pr);
    let mut slots: Vec<(i16, WireStack)> = c.listener.drain(..).map(|(i, s)| (i as i16, wire(&s))).collect();
    let cursor = pr.inventory.cursor;
    if on_screen && !are_item_stacks_equal(&w.sent_cursor, &cursor) {
        w.sent_cursor = cursor;
        slots.push((-1, wire(&cursor)));
    }
    if !slots.is_empty() {
        net::send(game, id, proto::S_SLOTS, &proto::WindowSlots { window, slots }.encode());
    }
    send_props(game, id);
}

/// The open window's tile from the world (a no-op for windows without one).
fn refresh(game: &mut Game, id: u32) {
    let Some(mut c) = game.windows.players.get_mut(&id).and_then(|w| w.open.take()) else { return };
    super::tiles::load_into(game, &mut c);
    // The bookshelves around the table may have changed since the last tick.
    if let ContainerKind::Enchanting { pos } = c.kind {
        let power = bookshelf_power(game, pos);
        c.set_bookshelf_power(power);
    }
    game.windows.players.get_mut(&id).unwrap().open = Some(c);
}

/// `ContainerEnchantment.onCraftMatrixChanged`' bookshelf count around the table at `pos`.
fn bookshelf_power(game: &Game, pos: [i32; 3]) -> i32 {
    let at = |dx: i32, dy: i32, dz: i32| terrain::state(game, (pos[0] + dx, pos[1] + dy, pos[2] + dz)) >> 4;
    sim::container::enchant::count_bookshelves(|x, y, z| at(x, y, z) == 0, |x, y, z| at(x, y, z) == ::items::ids::block::BOOKSHELF)
}

/// `NetHandlerPlayServer.processEnchantItem`: the button of the open table, then
/// `detectAndSendChanges`. `EntityPlayer.onEnchant` takes the levels and draws the next seed.
fn enchant_item(game: &mut Game, id: u32, m: &proto::EnchantItem) {
    ensure(game, id);
    let mode = super::players::mode(game, id);
    let active = game.windows.players[&id].active_id();
    if m.window != active || mode == proto::GM_SPECTATOR {
        return;
    }
    refresh(game, id);
    let Some((w, p)) = parts(game, id) else { return };
    let Some(c) = w.open.as_mut() else { return };
    let ContainerKind::Enchanting { pos } = c.kind else { return };
    let done = c.enchant_item(mode == proto::GM_CREATIVE, p.xp.level, m.button as usize, || (crate::config::random_u64() >> 16) as i32);
    if let sim::container::enchant::Enchanted::Done { cost, new_seed } = done {
        p.add_experience_level(-cost);
        p.xp_seed = new_seed;
        super::xp::send(game, id);
        let pitch = game.mobs.rand.next_float() * 0.1 + 0.9;
        super::mobs::sounds::play_sound(game, [pos[0] as f64 + 0.5, pos[1] as f64 + 0.5, pos[2] as f64 + 0.5], "block.enchantment_table.use", 4, 1.0, pitch);
    }
    send_changes(game, id, false);
}

/// The open window's tile into the world.
fn store(game: &mut Game, id: u32) {
    let Some(c) = game.windows.players.get_mut(&id).and_then(|w| w.open.take()) else { return };
    super::tiles::store_from(game, &c);
    game.windows.players.get_mut(&id).unwrap().open = Some(c);
}

// ---- open and close ----

/// `EntityPlayerMP.displayGui(InterfaceCraftingTable)`: the next window id (1 to 100), a
/// `ContainerWorkbench`, `SPacketOpenWindow` and the window's contents.
pub fn open_workbench(game: &mut Game, id: u32, pos: [i32; 3]) {
    ensure(game, id);
    // displayGui: a window already open is closed first.
    if game.windows.players[&id].open.is_some() {
        close_open(game, id, true);
    }
    let r = recipes(game);
    let w = game.windows.players.get_mut(&id).unwrap();
    w.window_counter = w.window_counter % 100 + 1;
    let window = w.window_counter;
    w.open = Some(Container::workbench(window as i32, pos, r));
    let open = proto::OpenWindow { window, kind: proto::WK_CRAFTING_TABLE, rows: 0, pos, title: "container.crafting".into() };
    net::send(game, id, proto::S_OPEN_WINDOW, &open.encode());
    send_full(game, id);
}

/// `EntityPlayerMP.displayGUIChest`/`displayGui(furnace)`: closes a window already open, then
/// shows `c` with its id, `SPacketOpenWindow` and the contents.
fn display(game: &mut Game, id: u32, make: impl FnOnce(i32) -> Container, kind: u8, rows: u8, pos: [i32; 3], title: &str) {
    ensure(game, id);
    if game.windows.players[&id].open.is_some() {
        close_open(game, id, true);
    }
    let w = game.windows.players.get_mut(&id).unwrap();
    w.window_counter = w.window_counter % 100 + 1;
    let window = w.window_counter;
    let c = make(window as i32);
    super::tiles::on_opened(game, &c.kind);
    game.windows.players.get_mut(&id).unwrap().open = Some(c);
    let open = proto::OpenWindow { window, kind, rows, pos, title: title.into() };
    net::send(game, id, proto::S_OPEN_WINDOW, &open.encode());
    send_full(game, id);
}

/// `EntityPlayerMP.closeContainer` on the open window, if there is one: `onContainerClosed`
/// (the cursor is thrown, the grid goes back to the inventory), then the inventory window is the
/// one on screen again. `notify` sends `SPacketCloseWindow` too (`closeScreen`).
fn close_open(game: &mut Game, id: u32, notify: bool) {
    let creative = super::players::mode(game, id) == proto::GM_CREATIVE;
    let alive = super::players::get(game, id).is_some_and(|p| !p.dead);
    let Some((w, p)) = parts(game, id) else { return };
    let Some(mut c) = w.open.take() else { return };
    let window = c.window_id as u8;
    let mut pr = PlayerRef::new(&mut p.inv, creative);
    pr.alive = alive;
    c.on_container_closed(&mut pr);
    let dropped = pr.dropped;
    super::tiles::on_closed(game, &c.kind);
    if notify {
        net::send(game, id, proto::S_CLOSE_WINDOW, &proto::CloseWindow { window }.encode());
    }
    throw(game, id, dropped);
    // The grid came back through the inventory and the cursor is empty: the viewer has window 0.
    send_full(game, id);
}

/// `processCloseWindow` for window 0 or the open window: the grid goes back, the cursor is thrown.
fn close_window(game: &mut Game, id: u32, window: u8) {
    ensure(game, id);
    let active = game.windows.players[&id].active_id();
    // DROPPED: vanilla closes whatever is open whatever the id; an id that is not on screen is a
    // late close from a screen the server already replaced, so it is ignored.
    if window != active {
        return;
    }
    if active != 0 {
        close_open(game, id, false);
        return;
    }
    let creative = super::players::mode(game, id) == proto::GM_CREATIVE;
    let Some((w, p)) = parts(game, id) else { return };
    let mut pr = PlayerRef::new(&mut p.inv, creative);
    w.inv.on_container_closed(&mut pr);
    let dropped = pr.dropped;
    throw(game, id, dropped);
    send_full(game, id);
}

// ---- frames ----

/// The windows' frames. Returns false for the ones it leaves to other modules.
pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    if !matches!(tag, proto::V_CLICK | proto::V_ENCHANT | proto::V_CLOSE | proto::V_CREATIVE_SLOT | proto::V_HELD | proto::V_SWAP_HANDS) {
        return false;
    }
    if !survival_client(game, id) || !game.players.players.contains_key(&id) {
        return false;
    }
    let dead = game.players.players[&id].dead;
    match tag {
        _ if dead => true,
        proto::V_CLICK => {
            if let Some(m) = proto::Click::decode(body) {
                click(game, id, &m);
            }
            true
        }
        proto::V_ENCHANT => {
            if let Some(m) = proto::EnchantItem::decode(body) {
                enchant_item(game, id, &m);
            }
            true
        }
        proto::V_CLOSE => {
            if let Some(m) = proto::CloseWindow::decode(body) {
                close_window(game, id, m.window);
            }
            true
        }
        proto::V_CREATIVE_SLOT => {
            if let Some(m) = proto::CreativeSlot::decode(body) {
                creative_slot(game, id, &m);
            }
            true
        }
        proto::V_HELD => {
            // processHeldItemChange: the slot is already 0 to 8 (the decode refuses the rest).
            if let Some(m) = proto::HeldSlot::decode(body) {
                if let Some(p) = game.players.players.get_mut(&id) {
                    p.inv.current_item = m.slot as usize;
                }
            }
            true
        }
        _ => {
            // processPlayerDigging SWAP_HELD_ITEMS: not for a spectator.
            if super::players::mode(game, id) != proto::GM_SPECTATOR {
                if let Some(p) = game.players.players.get_mut(&id) {
                    let cur = p.inv.current_item;
                    let main = p.inv.main[cur];
                    p.inv.main[cur] = p.inv.off_hand[0];
                    p.inv.off_hand[0] = main;
                }
            }
            true
        }
    }
}

/// `processTryUseItemOnBlock` onto a crafting table: within 8 blocks of the player, and a
/// sneaking player with something in a hand uses the item instead (`ItemStack.onItemUse` path of
/// `PlayerInteractionManager.processRightClickBlock`). False leaves the frame to other modules.
pub(super) fn use_on(game: &mut Game, id: u32, m: &proto::UseOn) -> bool {
    let Some(state) = game.sessions.get(&id).and_then(|s| s.state) else { return false };
    if super::players::mode(game, id) == proto::GM_SPECTATOR || game.players.players[&id].dead {
        return false;
    }
    let at = (m.pos[0], m.pos[1], m.pos[2]);
    let blk = terrain::state(game, at) >> 4;
    if !matches!(blk, CRAFTING_TABLE | ::items::ids::block::CHEST | ::items::ids::block::FURNACE | ::items::ids::block::LIT_FURNACE | ::items::ids::block::ENCHANTING_TABLE) {
        return false;
    }
    let (dx, dy, dz) = (state.pos[0] - (at.0 as f64 + 0.5), state.pos[1] - (at.1 as f64 + 0.5), state.pos[2] - (at.2 as f64 + 0.5));
    if dx * dx + dy * dy + dz * dz >= 64.0 {
        return false;
    }
    let p = &game.players.players[&id];
    let sneaking = state.flags & SNEAKING != 0;
    if sneaking && (!p.inv.get_current_item().is_empty() || !p.inv.off_hand[0].is_empty()) {
        return false;
    }
    // Already looking at this table: the second hand's packet opens nothing more.
    ensure(game, id);
    match blk {
        CRAFTING_TABLE => {
            if game.windows.players[&id].open.as_ref().is_some_and(|c| c.kind == ContainerKind::Workbench { pos: m.pos }) {
                return true;
            }
            open_workbench(game, id, m.pos);
        }
        ::items::ids::block::CHEST => {
            // BlockChest.onBlockActivated: a blocked chest does nothing.
            let Some((a, other, contents)) = super::tiles::open_chest(game, at) else { return true };
            if game.windows.players[&id].open.as_ref().is_some_and(|c| matches!(c.kind, ContainerKind::Chest { pos, .. } if pos == a)) {
                return true;
            }
            let rows = (contents.len() / 9) as u8;
            let title = if other.is_some() { "container.chestDouble" } else { "container.chest" };
            display(game, id, |w| Container::chest(w, a, other, contents), proto::WK_CHEST, rows, a, title);
        }
        ::items::ids::block::ENCHANTING_TABLE => {
            // BlockEnchantmentTable.onBlockActivated: displayGUIChest(the table).
            if game.windows.players[&id].open.as_ref().is_some_and(|c| c.kind == ContainerKind::Enchanting { pos: m.pos }) {
                return true;
            }
            let (seed, power) = (game.players.players[&id].xp_seed, bookshelf_power(game, m.pos));
            display(game, id, |w| Container::enchanting(w, m.pos, seed, Some(power)), proto::WK_ENCHANTING, 0, m.pos, "container.enchant");
        }
        _ => {
            let Some(f) = super::tiles::open_furnace(game, at) else { return true };
            if game.windows.players[&id].open.as_ref().is_some_and(|c| c.kind == ContainerKind::Furnace { pos: m.pos }) {
                return true;
            }
            display(game, id, |w| Container::furnace(w, m.pos, f), proto::WK_FURNACE, 0, m.pos, "container.furnace");
        }
    }
    true
}

/// `NetHandlerPlayServer.processClickWindow`.
fn click(game: &mut Game, id: u32, m: &proto::Click) {
    ensure(game, id);
    let mode = super::players::mode(game, id);
    let creative = mode == proto::GM_CREATIVE;
    let active = game.windows.players[&id].active_id();
    let Some(ct) = ClickType::from_ordinal(m.mode) else { return };
    if m.window != active || mode == proto::GM_SPECTATOR {
        // A click for a window that is not on screen, or a spectator's: nothing happens, and the
        // viewer gets the real contents back (`sendAllContents`).
        net::send(game, id, proto::S_CONFIRM, &proto::Confirm { window: m.window, action: m.action, accepted: false }.encode());
        send_full(game, id);
        return;
    }
    refresh(game, id);
    let stale = out_of_date(game, id);
    let Some((w, p)) = parts(game, id) else { return };
    let mut pr = PlayerRef::new(&mut p.inv, creative);
    w.active().slot_click(m.slot as i32, m.button as i32, ct, &mut pr);
    let dropped = pr.dropped;
    store(game, id);
    super::xp_orbs::furnace_orbs(game, id);
    throw(game, id, dropped);
    net::send(game, id, proto::S_CONFIRM, &proto::Confirm { window: m.window, action: m.action, accepted: true }.encode());
    if stale {
        // The viewer predicted from an older window than ours: send all of it.
        send_full(game, id);
    } else {
        // The viewer's own copy reached the same state; only the bookkeeping moves on.
        let creative = mode == proto::GM_CREATIVE;
        if let Some((w, p)) = parts(game, id) {
            let pr = PlayerRef::new(&mut p.inv, creative);
            let c = w.active();
            c.detect_and_send_changes(&pr);
            c.listener.clear();
            w.sent_cursor = pr.inventory.cursor;
        }
        send_props(game, id);
    }
}

/// `NetHandlerPlayServer.processCreativeInventoryAction`, for creative players only.
fn creative_slot(game: &mut Game, id: u32, m: &proto::CreativeSlot) {
    if super::players::mode(game, id) != proto::GM_CREATIVE {
        return;
    }
    ensure(game, id);
    let stack = stack_of(&m.stack);
    let throws = m.slot < 0;
    let in_window = (1..=45).contains(&m.slot);
    let valid = stack.is_empty() || stack.count <= 64;
    if in_window && valid {
        let Some((w, p)) = parts(game, id) else { return };
        let mut pr = PlayerRef::new(&mut p.inv, true);
        w.inv.put_stack(&mut pr, m.slot as usize, stack);
        send_changes(game, id, true);
    } else if throws && valid && game.windows.players[&id].drop_threshold < DROP_LIMIT && !stack.is_empty() {
        game.windows.players.get_mut(&id).unwrap().drop_threshold += DROP_STEP;
        let before = game.items.entities.len();
        super::players::drop_item(game, id, stack, false, false);
        // EntityItem.setAgeToCreativeDespawnTime
        if game.items.entities.len() > before {
            if let Some(e) = game.items.entities.last_mut() {
                e.set_age_to_creative_despawn_time();
            }
        }
    }
}

// ---- hooks ----

/// A joining player gets the equipment of everyone already there (`EntityTrackerEntry.
/// updatePlayerEntity` sends it when tracking starts); theirs reaches the others from [`tick`].
pub fn on_join(game: &mut Game, id: u32) {
    ensure(game, id);
    let mut others: Vec<u32> = game.windows.players.keys().copied().filter(|&o| o != id).collect();
    others.sort();
    for o in others {
        let Some(p) = game.players.players.get(&o) else { continue };
        for (slot, s) in equipment(p).into_iter().enumerate() {
            if !s.is_empty() {
                net::send(game, id, proto::S_EQUIPMENT, &proto::Equipment { id: o, slot: slot as u8, stack: wire(&s) }.encode());
            }
        }
    }
}

/// The player is gone (`playerLoggedOut`): an open window closes, and what the grids and the
/// cursor held lies where the player stood (`hasDisconnected` makes `clearContainer` drop it).
pub fn on_leave(game: &mut Game, id: u32) {
    let creative = super::players::mode(game, id) == proto::GM_CREATIVE;
    let Some((w, p)) = parts(game, id) else {
        game.windows.players.remove(&id);
        return;
    };
    let mut pr = PlayerRef::new(&mut p.inv, creative);
    pr.alive = false;
    let mut closed = None;
    if let Some(mut c) = w.open.take() {
        c.on_container_closed(&mut pr);
        closed = Some(c.kind);
    }
    // DROPPED: vanilla never closes the inventory window on logout and loses its grid.
    w.inv.on_container_closed(&mut pr);
    let dropped = pr.dropped;
    if let Some(k) = closed {
        super::tiles::on_closed(game, &k);
    }
    throw(game, id, dropped);
    game.windows.players.remove(&id);
}

/// `EntityLivingBase.getItemStackFromSlot` for the six slots of `S_EQUIPMENT`.
fn equipment(p: &super::players::Survival) -> [ItemStack; 6] {
    let inv = &p.inv;
    [inv.get_current_item(), inv.off_hand[0], inv.armor[0], inv.armor[1], inv.armor[2], inv.armor[3]]
}

/// One tick of every player's windows, between damage and the item entities: a dead player's
/// windows close, a crafting table out of reach or gone closes (`EntityPlayerMP.onUpdate`), the
/// changes made outside a click go to the viewer, and changed equipment goes to the others.
pub fn tick(game: &mut Game) {
    let mut ids: Vec<u32> = game.windows.players.keys().copied().collect();
    ids.sort();
    for id in ids {
        let Some(p) = game.players.players.get(&id) else { continue };
        let dead = p.dead;
        let w = &game.windows.players[&id];
        if dead && (w.open.is_some() || !p.inv.cursor.is_empty() || w.inv.crafting.as_ref().is_some_and(|c| c.craft_matrix.slots.iter().any(|s| !s.is_empty()))) {
            close_dead(game, id);
        }
        if let Some(w) = game.windows.players.get_mut(&id) {
            if w.drop_threshold > 0 {
                w.drop_threshold -= 1;
            }
        }
        // canInteractWith
        let reach = {
            let w = &game.windows.players[&id];
            match (&w.open, game.sessions.get(&id).and_then(|s| s.state)) {
                (Some(c), Some(st)) => match c.kind {
                    ContainerKind::Workbench { pos } => Some((c.can_interact_with(terrain::state(game, (pos[0], pos[1], pos[2])) >> 4, st.pos), ())),
                    ContainerKind::Chest { .. } | ContainerKind::Furnace { .. } => Some((super::tiles::usable(game, &c.kind, st.pos), ())),
                    ContainerKind::Enchanting { pos } => Some((c.can_interact_with(terrain::state(game, (pos[0], pos[1], pos[2])) >> 4, st.pos), ())),
                    _ => None,
                },
                _ => None,
            }
        };
        if let Some((false, ())) = reach {
            close_open(game, id, true);
        }
        refresh(game, id);
        send_changes(game, id, false);
        send_equipment(game, id);
    }
}

/// A dead player's open window and grids close with `isEntityAlive()` false: everything lands on
/// the ground.
fn close_dead(game: &mut Game, id: u32) {
    let had_window = game.windows.players.get(&id).is_some_and(|w| w.open.is_some());
    let Some((w, p)) = parts(game, id) else { return };
    let mut pr = PlayerRef::new(&mut p.inv, false);
    pr.alive = false;
    let window = w.open.as_ref().map(|c| c.window_id as u8);
    let mut closed = None;
    if let Some(mut c) = w.open.take() {
        c.on_container_closed(&mut pr);
        closed = Some(c.kind);
    }
    w.inv.on_container_closed(&mut pr);
    let dropped = pr.dropped;
    if let Some(k) = closed {
        super::tiles::on_closed(game, &k);
    }
    throw(game, id, dropped);
    if let (true, Some(window)) = (had_window, window) {
        net::send(game, id, proto::S_CLOSE_WINDOW, &proto::CloseWindow { window }.encode());
    }
    send_full(game, id);
}

/// `EntityTrackerEntry`'s equipment diff: the six slots against what the others last saw.
fn send_equipment(game: &mut Game, id: u32) {
    let Some(p) = game.players.players.get(&id) else { return };
    let now = equipment(p);
    let Some(w) = game.windows.players.get_mut(&id) else { return };
    let mut changed = Vec::new();
    for (slot, s) in now.into_iter().enumerate() {
        if !are_item_stacks_equal(&w.equip[slot], &s) {
            w.equip[slot] = s;
            changed.push(proto::Equipment { id, slot: slot as u8, stack: wire(&s) });
        }
    }
    for msg in changed {
        net::broadcast(game, Some(id), proto::S_EQUIPMENT, &msg.encode());
    }
}

/// Window state is not saved: the inventory is in the player's playerdata file.
pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
