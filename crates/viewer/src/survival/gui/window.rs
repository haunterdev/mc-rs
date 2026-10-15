//! The survival container screens, `GuiInventory` (E) and `GuiCrafting` (a crafting table), on the
//! shared `GuiContainer` of mod.rs. A screen holds the viewer's copy of the server's window, a
//! `sim::container::Container`; each click is predicted on it with the same `slot_click` the
//! server runs, then goes out as `V_CLICK` (`PlayerControllerMP.windowClick`).
//!
//! The cursor stack is `InventoryPlayer::cursor`. `S_CONFIRM` alone means the prediction was
//! right, so nothing reads it; a refused click and every server-side change arrive as `S_SLOTS`,
//! which this file takes as the truth (slot -1 is the cursor). `V_CLOSE` goes out when the player
//! closes, after which the server resends window 0; `S_CLOSE_WINDOW` means the server closed it.
//! A crafting table is opened by `V_USE_ON` (interact.rs sets `Controller::table`) and the
//! server's `S_OPEN_WINDOW`; without a server the screen opens at once.
//!
//! The tooltip is the item's name in its rarity colour, then `Item.addInformation` (potion
//! effects, a record's title) and the enchantments. DROPPED: the advanced (F3+H) lines.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use items::names::display_name;
use items::{ItemStack, Recipes};
use proto::WireStack;
use sim::container::{ClickType, Container, Inv, PlayerRef};
use sim::inventory::InventoryPlayer;
use std::sync::Arc;

use super::creative::pointer;
use super::draw::{self, Frame};
use super::slots::Gui;
use super::{check_hotbar_keys, chest, crafting, draw_cursor, draw_slots, inventory, mouse_click_move, mouse_clicked, mouse_released, Base, GuiLang, Screen, Sent};
use super::{enchanting, furnace};
use crate::hotbar::Inventory;
use crate::interact::Controller;
use crate::link::{Link, StreamCursor};
use crate::player::Player;
use crate::players::Font;
use crate::settings::{self, Menu};
use crate::survival::items::stack_of;
use crate::survival::net::{send_click, send_close, send_enchant, send_use_on};
use crate::stream::Streamer;
use crate::survival::{CloseWindow, OpenWindow, Slots, SurvivalState, WindowProp};

pub fn plugin(app: &mut App) {
    app.init_resource::<Windows>().add_systems(
        Update,
        (net_windows, apply_slots, apply_props, keys, draw_screen).chain().after(settings::toggle_menu).before(crate::input::focus).run_if(resource_exists::<super::ContainerImages>),
    );
}

/// Which screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Inventory,
    Crafting,
    /// `GuiChest` with this many rows: 3, or 6 for a large chest.
    Chest(u8),
    Furnace,
    Enchanting,
}

impl Kind {
    /// Its index in [`super::ContainerImages::backgrounds`].
    fn background(self) -> usize {
        match self {
            Kind::Inventory => 3,
            Kind::Crafting => 4,
            Kind::Chest(_) => 5,
            Kind::Furnace => 6,
            Kind::Enchanting => 7,
        }
    }

    pub fn size(self) -> (i32, i32) {
        match self {
            Kind::Inventory => inventory::SIZE,
            Kind::Crafting => crafting::SIZE,
            Kind::Chest(rows) => chest::size(rows),
            Kind::Furnace => furnace::SIZE,
            Kind::Enchanting => enchanting::SIZE,
        }
    }
}

/// The open screen.
#[derive(Resource, Default)]
pub struct Windows {
    pub screen: Option<WindowScreen>,
    /// `--screen inventory|crafting`: open it on the first frame, for a `--shot`.
    pub open_at_start: Option<Kind>,
    pub recipes: Option<Arc<Recipes>>,
}

impl Windows {
    /// `CraftingManager`: the jar's recipes, read on first use; the code recipes alone without a jar.
    fn recipes(&mut self) -> Arc<Recipes> {
        self.recipes
            .get_or_insert_with(|| {
                #[cfg(test)]
                let r = Recipes::code_recipes();
                #[cfg(not(test))]
                let r = match crate::jar::Jar::find().map_err(|e| e.to_string()).and_then(|j| Recipes::from_jar(j.names(), |n| j.read(n).ok()).map_err(|e| e.to_string())) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("no recipes from the jar ({e}); only the code recipes craft");
                        Recipes::code_recipes()
                    }
                };
                Arc::new(r)
            })
            .clone()
    }
}

/// A screen over one window.
pub struct WindowScreen {
    base: Base,
    pub kind: Kind,
    /// The viewer's copy of the server's window.
    pub container: Container,
    /// `V_CLICK.action` of the next click.
    action: u16,
    /// The clicks predicted and not yet sent.
    pub pending: Vec<proto::Click>,
    /// The window title as the server sent it, a lang key (`container.chest`).
    title: String,
    /// The player level and mode, and the offers clicked (the enchanting table).
    pub enchant: enchanting::Enchant,
}

impl WindowScreen {
    /// `new GuiInventory(player)`: window 0.
    pub fn inventory(recipes: Arc<Recipes>) -> WindowScreen {
        WindowScreen { base: Base::default(), kind: Kind::Inventory, container: Container::player(recipes), action: 0, pending: Vec::new(), enchant: enchanting::Enchant::default(), title: String::new() }
    }

    /// `new GuiCrafting(inventory, world, pos)` for the window the server opened.
    pub fn crafting(window: u8, pos: [i32; 3], recipes: Arc<Recipes>) -> WindowScreen {
        WindowScreen { base: Base::default(), kind: Kind::Crafting, container: Container::workbench(window as i32, pos, recipes), action: 0, pending: Vec::new(), enchant: enchanting::Enchant::default(), title: String::new() }
    }

    /// `new GuiChest(playerInventory, chestInventory)`: `rows` of 3 or 6, the contents empty
    /// until `S_SLOTS`.
    pub fn chest(window: u8, pos: [i32; 3], rows: u8, title: &str) -> WindowScreen {
        let contents = vec![ItemStack::EMPTY; rows as usize * 9];
        WindowScreen {
            base: Base::default(),
            kind: Kind::Chest(rows),
            container: Container::chest(window as i32, pos, None, contents),
            action: 0,
            pending: Vec::new(), enchant: enchanting::Enchant::default(),
            title: title.to_string(),
        }
    }

    /// `new GuiFurnace(playerInventory, furnaceInventory)`: every field 0 until `S_WINDOW_PROP`.
    pub fn furnace(window: u8, pos: [i32; 3], title: &str) -> WindowScreen {
        WindowScreen {
            base: Base::default(),
            kind: Kind::Furnace,
            container: Container::furnace(window as i32, pos, items::Furnace::default()),
            action: 0,
            pending: Vec::new(), enchant: enchanting::Enchant::default(),
            title: title.to_string(),
        }
    }

    /// `new GuiEnchantment(playerInv, world)`: no offers until `S_WINDOW_PROP`.
    pub fn enchanting(window: u8, pos: [i32; 3], title: &str) -> WindowScreen {
        WindowScreen {
            base: Base::default(),
            kind: Kind::Enchanting,
            container: Container::enchanting(window as i32, pos, 0, None),
            action: 0,
            pending: Vec::new(),
            enchant: enchanting::Enchant::default(),
            title: title.to_string(),
        }
    }

    /// `S_WINDOW_PROP` (`SPacketWindowProperty` into `TileEntityFurnace.setField`): 0 burn time, 1
    /// current item burn time, 2 cook time, 3 total cook time.
    pub fn set_property(&mut self, property: u16, value: i16) {
        if self.kind == Kind::Enchanting {
            self.container.update_enchant_property(property, value as i32);
        }
        if let Some(sim::container::Tile::Furnace(f)) = &mut self.container.tile {
            let value = value as i32;
            match property {
                0 => f.burn_time = value,
                1 => f.current_item_burn_time = value,
                2 => f.cook_time = value,
                3 => f.total_cook_time = value,
                _ => {}
            }
        }
    }

    /// `TileEntityFurnace.getField` 0 to 3.
    pub fn furnace_fields(&self) -> [i32; 4] {
        match &self.container.tile {
            Some(sim::container::Tile::Furnace(f)) => [f.burn_time, f.current_item_burn_time, f.cook_time, f.total_cook_time],
            _ => [0; 4],
        }
    }

    /// `windowId`
    pub fn window(&self) -> u8 {
        self.container.window_id as u8
    }

    /// `S_SLOTS` for this window: every slot is put as the server has it (the result last, as
    /// changing the grid recomputes it), and slot -1 is the cursor.
    pub fn apply(&mut self, inv: &mut InventoryPlayer, slots: &[(i16, WireStack)]) {
        let mut cursor = None;
        let mut result = None;
        let mut p = PlayerRef::new(inv, false);
        for (slot, w) in slots {
            let stack = stack_of(w);
            if *slot == -1 {
                cursor = Some(stack);
            } else if let Some(i) = self.container.slot_index(*slot as i32) {
                if self.container.slots[i].inventory == Inv::Result {
                    result = Some((i, stack));
                } else {
                    self.container.put_stack(&mut p, i, stack);
                }
            }
        }
        if let Some((i, stack)) = result {
            self.container.put_stack(&mut p, i, stack);
        }
        if let Some(c) = cursor {
            inv.cursor = c;
        }
    }

    /// `GuiContainer.keyTyped`: Escape or E closes, a number key swaps, Q throws the hovered
    /// stack (all of it with Ctrl). False when the key closes the screen.
    fn key_typed(&mut self, inv: &mut InventoryPlayer, key: KeyCode, ctrl: bool) -> bool {
        let close = matches!(key, KeyCode::Escape | KeyCode::KeyE);
        let mut out = Vec::new();
        check_hotbar_keys(self, inv, &mut out, digit(key));
        if let Some(h) = self.base.hovered.filter(|&h| !self.stack(inv, h).is_empty()) {
            if key == KeyCode::KeyQ {
                let n = self.slot_number(h);
                self.handle_mouse_click(inv, &mut out, Some(h), n, ctrl as i32, ClickType::Throw);
            }
        }
        !close
    }

    /// `drawScreen`: the backdrop, the background texture, the slots, the foreground text, the
    /// cursor stack and the hovered slot's tooltip.
    #[allow(clippy::too_many_arguments)]
    fn draw(&mut self, frame: &mut Frame, font: &Font, galactic: Option<&Font>, lang: &GuiLang, inv: &mut InventoryPlayer, gui: &Gui, mx: i32, my: i32, effects: &[items::potion::PotionEffect]) {
        frame.bands[draw::BACKDROP].gradient(0, 0, gui.w, gui.h, 0xC010_1010, 0xD010_1010);
        frame.background = self.kind.background();
        let (w, h) = self.kind.size();
        match self.kind {
            Kind::Chest(rows) => chest::background(frame, gui, rows),
            Kind::Furnace => furnace::background(frame, gui, &self.furnace_fields()),
            Kind::Enchanting => enchanting::background(frame, font, galactic, gui, &self.container, &self.enchant, mx, my),
            _ => frame.bands[draw::BACKGROUND].sprite(gui.left, gui.top, 0, 0, w, h),
        }
        draw_slots(self, frame, font, inv, gui, mx, my);
        let crafting_name = lang.0.translate("container.crafting").to_string();
        let inventory_name = lang.0.translate("container.inventory");
        let title = lang.0.translate(&self.title);
        match self.kind {
            Kind::Inventory => {
                inventory::foreground(frame, font, &crafting_name, gui);
                inventory::player_model(frame, gui, mx, my);
                // InventoryEffectRenderer.drawActivePotionEffects
                super::super::effects::draw_list(frame, font, &lang.0, gui.left, gui.top, effects);
            }
            Kind::Crafting => crafting::foreground(frame, font, &crafting_name, inventory_name, gui),
            Kind::Chest(rows) => chest::foreground(frame, font, title, inventory_name, rows, gui),
            Kind::Furnace => furnace::foreground(frame, font, title, inventory_name, gui),
            Kind::Enchanting => enchanting::foreground(frame, font, title, inventory_name, gui),
        }
        draw_cursor(self, frame, font, inv, mx, my);
        if self.kind == Kind::Enchanting {
            if let Some(lines) = enchanting::tooltip(&lang.0, &self.container, &self.enchant, gui, mx, my) {
                frame.hovering_text(font, &lines, mx, my, gui.w, gui.h);
            }
        }
        // renderHoveredToolTip
        if let Some(h) = self.base.hovered.filter(|_| inv.cursor.is_empty()) {
            let stack = self.stack(inv, h);
            if !stack.is_empty() {
                let mut lines = vec![enchanting::name_line(&stack, display_name(&stack, lang.0.map()))];
                lines.extend(enchanting::tooltip_lines(&stack, &lang.0));
                frame.hovering_text(font, &lines, mx, my, gui.w, gui.h);
            }
        }
    }
}

/// `keyBindsHotbar`: 1 to 9.
fn digit(key: KeyCode) -> Option<usize> {
    [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ]
    .iter()
    .position(|&k| k == key)
}

impl Screen for WindowScreen {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn size(&self) -> (i32, i32) {
        self.kind.size()
    }

    fn slot_count(&self) -> usize {
        self.container.slots.len()
    }

    fn slot_pos(&self, i: usize) -> (i32, i32) {
        (self.container.slots[i].x, self.container.slots[i].y)
    }

    fn slot_number(&self, i: usize) -> i32 {
        self.container.slots[i].slot_number as i32
    }

    fn stack(&self, inv: &mut InventoryPlayer, i: usize) -> ItemStack {
        self.container.get_stack(&PlayerRef::new(inv, false), i)
    }

    fn same_inventory(&self, a: usize, b: usize) -> bool {
        self.container.slots[a].inventory == self.container.slots[b].inventory
    }

    fn can_take(&self, inv: &mut InventoryPlayer, i: usize) -> bool {
        self.container.can_take_stack(&PlayerRef::new(inv, false), i)
    }

    fn is_item_valid(&self, i: usize, stack: &ItemStack) -> bool {
        self.container.is_item_valid(i, stack)
    }

    fn item_stack_limit(&self, i: usize, stack: &ItemStack) -> i32 {
        self.container.get_item_stack_limit(i, stack)
    }

    fn can_drag_into(&self, i: usize) -> bool {
        self.container.can_drag_into_slot(i)
    }

    fn can_merge(&self, i: usize) -> bool {
        self.container.can_merge_slot(&ItemStack::EMPTY, i)
    }

    /// `ContainerPlayer`'s `getSlotTexture`: the armor slots head to feet and the off hand.
    fn slot_texture(&self, i: usize) -> Option<usize> {
        match (self.kind, self.container.slots[i].slot_number) {
            (Kind::Inventory, n @ 5..=8) => Some(n - 5),
            (Kind::Inventory, 45) => Some(4),
            _ => None,
        }
    }

    /// `GuiContainer.handleMouseClick`: `windowClick` predicts the click on the container, and
    /// the `CPacketClickWindow` is queued as a `V_CLICK`.
    fn handle_mouse_click(&mut self, inv: &mut InventoryPlayer, _out: &mut Vec<Sent>, slot: Option<usize>, slot_id: i32, button: i32, click: ClickType) {
        let slot_id = slot.map_or(slot_id, |i| self.slot_number(i));
        self.container.slot_click(slot_id, button, click, &mut PlayerRef::new(inv, false));
        self.pending.push(click_message(self.window(), slot_id, button, click, self.action));
        self.action = self.action.wrapping_add(1);
    }
}

/// `CPacketClickWindow` as `V_CLICK`.
pub fn click_message(window: u8, slot_id: i32, button: i32, click: ClickType, action: u16) -> proto::Click {
    proto::Click { window, slot: slot_id as i16, button: button as u8, mode: click.ordinal(), action }
}

/// `S_OPEN_WINDOW` and `S_CLOSE_WINDOW`, and the table the player used.
fn net_windows(
    mut win: ResMut<Windows>,
    mut menu: ResMut<Menu>,
    mut opens: MessageReader<OpenWindow>,
    mut closes: MessageReader<CloseWindow>,
    mut ctl: ResMut<Controller>,
    link: Option<Res<Link>>,
    streamer: Option<Res<Streamer>>,
) {
    if let Some(hit) = ctl.table.take() {
        let pos = [hit.pos.0, hit.pos.1, hit.pos.2];
        let frac = |v: f64, p: i32| (v - p as f64).clamp(0.0, 1.0) as f32;
        match link.as_deref() {
            Some(link) => send_use_on(
                link,
                &proto::UseOn { pos, face: hit.face as u8, hit: [frac(hit.vec.x, pos[0]), frac(hit.vec.y, pos[1]), frac(hit.vec.z, pos[2])], hand: 0, state: 0 },
            ),
            None if win.screen.is_none() && !menu.open && streamer.as_ref().and_then(|s| s.state_at(pos[0], pos[1], pos[2])).is_some_and(|s| s >> 4 == items::ids::block::CRAFTING_TABLE) => {
                let r = win.recipes();
                win.screen = Some(WindowScreen::crafting(1, pos, r));
                menu.open = true;
            }
            None => {}
        }
    }
    // The rest of the right clicks on a block (PlayerControllerMP.processRightClickBlock): the
    // server places the block or uses the item.
    for (hit, state, hand) in ctl.use_on.drain(..) {
        if let Some(link) = link.as_deref() {
            let pos = [hit.pos.0, hit.pos.1, hit.pos.2];
            let frac = |v: f64, p: i32| (v - p as f64).clamp(0.0, 1.0) as f32;
            send_use_on(link, &proto::UseOn { pos, face: hit.face as u8, hit: [frac(hit.vec.x, pos[0]), frac(hit.vec.y, pos[1]), frac(hit.vec.z, pos[2])], hand, state });
        }
    }
    for OpenWindow(o) in opens.read() {
        win.screen = Some(match o.kind {
            proto::WK_CRAFTING_TABLE => {
                let r = win.recipes();
                WindowScreen::crafting(o.window, o.pos, r)
            }
            proto::WK_CHEST => WindowScreen::chest(o.window, o.pos, if o.rows >= 6 { 6 } else { 3 }, &o.title),
            proto::WK_FURNACE => WindowScreen::furnace(o.window, o.pos, &o.title),
            proto::WK_ENCHANTING => WindowScreen::enchanting(o.window, o.pos, &o.title),
            _ => continue,
        });
        menu.open = true;
    }
    for CloseWindow(c) in closes.read() {
        if win.screen.as_ref().is_some_and(|s| s.window() == c.window) {
            win.screen = None;
            menu.open = false;
        }
    }
}

/// `S_SLOTS` for the open window.
fn apply_slots(mut win: ResMut<Windows>, mut inventory: ResMut<Inventory>, mut slots: MessageReader<Slots>) {
    let Some(screen) = win.screen.as_mut() else {
        slots.clear();
        return;
    };
    for Slots(s) in slots.read() {
        if s.window == screen.window() {
            screen.apply(&mut inventory.player, &s.slots);
        }
    }
}

/// `S_WINDOW_PROP` for the open window.
fn apply_props(mut win: ResMut<Windows>, mut props: MessageReader<WindowProp>) {
    let Some(screen) = win.screen.as_mut() else {
        props.clear();
        return;
    };
    for WindowProp(p) in props.read() {
        if p.window == screen.window() {
            screen.set_property(p.property, p.value);
        }
    }
}

/// Opens the inventory on E in survival, closes the screen on E or Escape, feeds it the keys and
/// buttons, and sends the clicks it predicted.
#[allow(clippy::too_many_arguments)]
fn keys(
    mut win: ResMut<Windows>,
    mut menu: ResMut<Menu>,
    mut inventory: ResMut<Inventory>,
    player: Res<Player>,
    state: Option<Res<SurvivalState>>,
    mut typed: MessageReader<KeyboardInput>,
    mut pressed: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    stream_cursor: Option<Res<StreamCursor>>,
    time: Res<Time>,
    font: Option<Res<Font>>,
    link: Option<Res<Link>>,
) {
    if font.is_none() {
        return;
    }
    let dead = state.as_ref().is_some_and(|s| s.dead);
    let inv = &mut inventory.player;
    let Some(screen) = win.screen.as_mut() else {
        let start = win.open_at_start.take();
        let survival = matches!(player.game_mode, proto::GM_SURVIVAL | proto::GM_ADVENTURE);
        if !dead && !menu.open && (start.is_some() || survival && pressed.just_pressed(KeyCode::KeyE)) {
            let r = win.recipes();
            win.screen = Some(match start {
                Some(Kind::Crafting) => WindowScreen::crafting(1, [0, 0, 0], r),
                Some(Kind::Chest(rows)) => {
                    let title = if rows == 6 { "container.chestDouble" } else { "container.chest" };
                    demo(WindowScreen::chest(1, [0, 0, 0], rows, title), inv)
                }
                Some(Kind::Enchanting) => WindowScreen::enchanting(1, [0, 0, 0], "container.enchant"),
                Some(Kind::Furnace) => demo(WindowScreen::furnace(1, [0, 0, 0], "container.furnace"), inv),
                _ => WindowScreen::inventory(r),
            });
            menu.open = true;
            typed.clear();
            pressed.clear();
        }
        return;
    };
    // Escape closes through the menu toggle (settings.rs flips `Menu::open` before this runs).
    let mut open = menu.open && !dead;
    if open {
        let shift = link.as_deref().is_some_and(|l| l.shift_held()) || pressed.pressed(KeyCode::ShiftLeft) || pressed.pressed(KeyCode::ShiftRight);
        let ctrl = pressed.pressed(KeyCode::ControlLeft) || pressed.pressed(KeyCode::ControlRight);
        for ev in typed.read() {
            if ev.state == ButtonState::Pressed && open {
                open = screen.key_typed(inv, ev.key_code, ctrl);
            }
        }
        let (w, h) = screen.kind.size();
        let mut gui = Gui::new(window.physical_width(), window.physical_height(), w, h);
        if screen.kind == Kind::Inventory && !player.effects.is_empty() {
            gui = gui.with_effects(w);
        }
        screen.enchant.xp_level = state.as_ref().map_or(0, |s| s.xp.level);
        screen.enchant.creative = player.game_mode == proto::GM_CREATIVE;
        let at = pointer(&window, stream_cursor.as_deref()).map(|p| gui.mouse(p.x, p.y));
        let now = time.elapsed_secs_f64() * 1000.0;
        if let (Some((mx, my)), true) = (at, open) {
            let mut out = Vec::new();
            if screen.kind == Kind::Enchanting && mouse.just_pressed(MouseButton::Left) {
                // GuiEnchantment.mouseClicked: the offer button, when it can be taken.
                if let Some(k) = enchanting::clicked(&mut screen.container, &screen.enchant, &gui, mx, my) {
                    screen.enchant.pending.push(k);
                }
            }
            let buttons = [(MouseButton::Left, 0), (MouseButton::Right, 1), (MouseButton::Middle, super::PICK)];
            for (b, n) in buttons {
                if mouse.just_pressed(b) {
                    mouse_clicked(screen, inv, &mut out, &gui, mx, my, n, shift, now);
                }
            }
            if buttons.iter().any(|&(b, _)| mouse.pressed(b)) {
                mouse_click_move(screen, inv, &gui, mx, my);
            }
            for (b, n) in buttons {
                if mouse.just_released(b) {
                    mouse_released(screen, inv, &mut out, &gui, mx, my, n, shift);
                }
            }
        }
        // Typed letters are not key bindings.
        pressed.clear();
    }
    let id = screen.window();
    let clicks = std::mem::take(&mut screen.pending);
    let offers = std::mem::take(&mut screen.enchant.pending);
    if let Some(link) = link.as_deref() {
        for &button in &offers {
            send_enchant(link, &proto::EnchantItem { window: id, button });
        }
        for c in &clicks {
            send_click(link, c);
        }
    }
    if !open {
        // onGuiClosed: the grid goes back to the inventory and the cursor stack is thrown, as the
        // server does on `V_CLOSE`.
        screen.container.on_container_closed(&mut PlayerRef::new(inv, false));
        if let Some(link) = link.as_deref() {
            send_close(link, &proto::CloseWindow { window: id });
        }
        win.screen = None;
        menu.open = false;
    }
}

/// `--screen chest|chest_double|furnace`: some stacks in the window and a furnace mid-smelt, so a
/// `--shot` shows what the screen draws.
fn demo(mut screen: WindowScreen, inv: &mut InventoryPlayer) -> WindowScreen {
    use items::ids::item;
    let mut p = PlayerRef::new(inv, false);
    let stacks: &[(i32, u16, i32)] = match screen.kind {
        Kind::Furnace => &[(0, item::IRON_INGOT, 8), (1, item::COAL, 5), (2, item::GOLD_INGOT, 3)],
        _ => &[(0, item::IRON_INGOT, 64), (1, item::COAL, 12), (4, item::GOLD_INGOT, 3), (11, item::PAPER, 1), (30, item::IRON_INGOT, 7)],
    };
    for &(slot, id, n) in stacks {
        if let Some(i) = screen.container.slot_index(slot) {
            screen.container.put_stack(&mut p, i, ItemStack::new(id, n, 0));
        }
    }
    if screen.kind == Kind::Furnace {
        for (prop, v) in [(0, 900), (1, 1600), (2, 120), (3, 200)] {
            screen.set_property(prop, v);
        }
    }
    screen
}

/// Fills the [`Frame`] from the open screen.
fn draw_screen(
    mut win: ResMut<Windows>,
    creative: Res<super::creative::Creative>,
    mut frame: ResMut<Frame>,
    mut inventory: ResMut<Inventory>,
    window: Single<&Window, With<PrimaryWindow>>,
    stream_cursor: Option<Res<StreamCursor>>,
    font: Option<Res<Font>>,
    lang: Option<Res<GuiLang>>,
    galactic: Option<Res<enchanting::Galactic>>,
    state: Option<Res<SurvivalState>>,
    player: Res<Player>,
) {
    let frame = &mut *frame;
    let Some(screen) = win.screen.as_mut() else { return };
    if creative.screen.is_some() {
        return;
    }
    let (Some(font), Some(lang)) = (font, lang) else { return };
    frame.clear();
    frame.open = true;
    let (w, h) = screen.kind.size();
    let mut gui = Gui::new(window.physical_width(), window.physical_height(), w, h);
    let effects = if screen.kind == Kind::Inventory { player.effects.sorted() } else { Vec::new() };
    if !effects.is_empty() {
        gui = gui.with_effects(w);
    }
    let (mx, my) = pointer(&window, stream_cursor.as_deref()).map_or((-10_000, -10_000), |p| gui.mouse(p.x, p.y));
    screen.enchant.xp_level = state.as_ref().map_or(0, |s| s.xp.level);
    screen.enchant.creative = player.game_mode == proto::GM_CREATIVE;
    screen.draw(frame, &font, galactic.as_ref().map(|g| &g.0), &lang, &mut inventory.player, &gui, mx, my, &effects);
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::ids::item;

    fn recipes() -> Arc<Recipes> {
        Arc::new(Recipes::code_recipes())
    }

    fn slot_of(s: &WindowScreen, number: usize) -> usize {
        (0..s.slot_count()).find(|&i| s.slot_number(i) == number as i32).unwrap()
    }

    #[test]
    fn every_slot_is_hit_at_gui_scales_1_to_4() {
        for kind in [Kind::Inventory, Kind::Crafting, Kind::Chest(3), Kind::Chest(6), Kind::Furnace, Kind::Enchanting] {
            let s = match kind {
                Kind::Inventory => WindowScreen::inventory(recipes()),
                Kind::Crafting => WindowScreen::crafting(1, [0, 0, 0], recipes()),
                Kind::Chest(rows) => WindowScreen::chest(1, [0, 0, 0], rows, "container.chest"),
                Kind::Furnace => WindowScreen::furnace(1, [0, 0, 0], "container.furnace"),
                Kind::Enchanting => WindowScreen::enchanting(1, [0, 0, 0], "container.enchant"),
            };
            let (w, h) = kind.size();
            for k in 1..=4u32 {
                // A window of 320 k x 240 k is exactly scale k.
                let gui = Gui::new(320 * k, 240 * k, w, h);
                assert_eq!(gui.k, k);
                assert_eq!((gui.w, gui.h), (320, 240));
                for i in 0..s.slot_count() {
                    let (sx, sy) = s.slot_pos(i);
                    // The top left pixel of the slot's square, the middle and the last pixel.
                    for (dx, dy) in [(0, 0), (15, 15), (8, 8)] {
                        let (px, py) = ((gui.left + sx + dx) as u32 * k, (gui.top + sy + dy) as u32 * k);
                        let (mx, my) = gui.mouse(px as f32, py as f32);
                        assert_eq!(super::super::slot_at(&s, &gui, mx, my), Some(i), "{kind:?} scale {k} slot {i} at +{dx},{dy}");
                    }
                    // Three pixels left of the slot is a neighbor or nothing, never this slot.
                    let (px, py) = ((gui.left + sx - 3) as u32 * k, (gui.top + sy + 8) as u32 * k);
                    let (mx, my) = gui.mouse(px as f32, py as f32);
                    assert_ne!(super::super::slot_at(&s, &gui, mx, my), Some(i));
                }
            }
        }
    }

    #[test]
    fn the_windows_have_vanillas_slots() {
        let s = WindowScreen::inventory(recipes());
        assert_eq!(s.slot_count(), 46);
        assert_eq!(s.slot_pos(slot_of(&s, 0)), (154, 28));
        assert_eq!(s.slot_pos(slot_of(&s, 36)), (8, 142));
        assert_eq!(s.slot_texture(slot_of(&s, 5)), Some(0));
        assert_eq!(s.slot_texture(slot_of(&s, 8)), Some(3));
        assert_eq!(s.slot_texture(slot_of(&s, 45)), Some(4));
        let t = WindowScreen::crafting(3, [1, 2, 3], recipes());
        assert_eq!((t.slot_count(), t.window()), (46, 3));
        assert_eq!(t.slot_pos(slot_of(&t, 0)), (124, 35));
        assert_eq!(t.slot_texture(slot_of(&t, 1)), None);
    }

    #[test]
    fn chest_and_furnace_windows_have_vanillas_slots_and_follow_the_server() {
        let c = WindowScreen::chest(4, [1, 2, 3], 6, "container.chestDouble");
        assert_eq!((c.slot_count(), c.window(), c.kind.size()), (54 + 36, 4, (176, 222)));
        assert_eq!(c.slot_pos(slot_of(&c, 0)), (8, 18));
        assert_eq!(c.slot_pos(slot_of(&c, 53)), (8 + 8 * 18, 18 + 5 * 18));
        assert_eq!(c.slot_pos(slot_of(&c, 54)), (8, 103 + 36));
        assert_eq!(c.slot_pos(slot_of(&c, 89)), (8 + 8 * 18, 161 + 36));
        let small = WindowScreen::chest(1, [0, 0, 0], 3, "container.chest");
        assert_eq!((small.slot_count(), small.kind.size()), (27 + 36, (176, 168)));
        let mut inv = InventoryPlayer::new();
        let mut f = WindowScreen::furnace(2, [0, 0, 0], "container.furnace");
        assert_eq!((f.slot_count(), f.slot_pos(slot_of(&f, 0)), f.slot_pos(slot_of(&f, 1)), f.slot_pos(slot_of(&f, 2))), (39, (56, 17), (56, 53), (116, 35)));
        // S_WINDOW_PROP fills the four fields; S_SLOTS puts the stacks.
        for (p, v) in [(0, 150), (1, 1600), (2, 33), (3, 200)] {
            f.set_property(p, v);
        }
        f.set_property(9, 5);
        assert_eq!(f.furnace_fields(), [150, 1600, 33, 200]);
        f.apply(&mut inv, &[(0, WireStack::new(item::PAPER as i16, 4, 0)), (-1, WireStack::new(item::PAPER as i16, 2, 0))]);
        assert_eq!(f.stack(&mut inv, slot_of(&f, 0)).get_count(), 4);
        assert_eq!(inv.cursor.get_count(), 2);
        // A shift-click on a chest slot is a QuickMove at its slot number.
        let mut c = WindowScreen::chest(4, [0, 0, 0], 3, "container.chest");
        c.apply(&mut inv, &[(5, WireStack::new(item::PAPER as i16, 9, 0))]);
        let mut out = Vec::new();
        c.handle_mouse_click(&mut inv, &mut out, Some(slot_of(&c, 5)), 5, 0, ClickType::QuickMove);
        assert_eq!((c.pending[0].window, c.pending[0].slot, c.pending[0].mode), (4, 5, 1));
        assert!(c.stack(&mut inv, slot_of(&c, 5)).is_empty());
    }

    #[test]
    fn clicks_become_predicted_v_click_messages() {
        let mut inv = InventoryPlayer::new();
        inv.set_inventory_slot_contents(0, ItemStack::new(item::PAPER, 5, 0));
        let mut s = WindowScreen::crafting(7, [0, 0, 0], recipes());
        let mut out = Vec::new();
        let hot0 = slot_of(&s, 37);
        // A pickup of the hotbar stack (hotbar 0 is slot 37 of the table).
        s.handle_mouse_click(&mut inv, &mut out, Some(hot0), 0, 0, ClickType::Pickup);
        assert_eq!(inv.cursor.get_count(), 5);
        assert!(inv.get_stack_in_slot(0).is_empty());
        // A right click into grid slot 1 places one.
        let grid = slot_of(&s, 1);
        s.handle_mouse_click(&mut inv, &mut out, Some(grid), 0, 1, ClickType::Pickup);
        assert_eq!(inv.cursor.get_count(), 4);
        // A click outside the window throws the rest.
        s.handle_mouse_click(&mut inv, &mut out, None, -999, 0, ClickType::Pickup);
        assert!(inv.cursor.is_empty());
        let sent: Vec<_> = s.pending.iter().map(|c| (c.window, c.slot, c.button, c.mode, c.action)).collect();
        assert_eq!(sent, vec![(7, 37, 0, 0, 0), (7, 1, 1, 0, 1), (7, -999, 0, 0, 2)]);
        // Shift is mode 1, a number key swap 2, the pick key 3, Q 4, a drag 5, a double click 6.
        assert_eq!(click_message(0, 9, 0, ClickType::QuickMove, 3), proto::Click { window: 0, slot: 9, button: 0, mode: 1, action: 3 });
        assert_eq!(click_message(0, 9, 4, ClickType::Swap, 0).mode, 2);
        assert_eq!(click_message(0, 9, 2, ClickType::Clone, 0).mode, 3);
        assert_eq!(click_message(0, 9, 1, ClickType::Throw, 0).mode, 4);
        assert_eq!(click_message(0, 9, 5, ClickType::QuickCraft, 0).mode, 5);
        assert_eq!(click_message(0, 9, 0, ClickType::PickupAll, 0).mode, 6);
    }

    #[test]
    fn mouse_clicks_go_through_guicontainer_to_messages() {
        let mut inv = InventoryPlayer::new();
        inv.set_inventory_slot_contents(0, ItemStack::new(item::PAPER, 5, 0));
        let mut s = WindowScreen::inventory(recipes());
        let gui = Gui::new(640, 480, 176, 166);
        let (sx, sy) = s.slot_pos(slot_of(&s, 36));
        let (mx, my) = (gui.left + sx + 4, gui.top + sy + 4);
        let mut out = Vec::new();
        mouse_clicked(&mut s, &mut inv, &mut out, &gui, mx, my, 0, true, 0.0);
        // Shift on a hotbar slot moves it up (QuickMove).
        assert_eq!(s.pending.len(), 1);
        assert_eq!((s.pending[0].slot, s.pending[0].mode), (36, 1));
        assert!(inv.get_stack_in_slot(0).is_empty());
        assert_eq!(inv.get_stack_in_slot(9).get_count(), 5);
        // A click outside the window with an empty cursor is a Throw at -999.
        mouse_clicked(&mut s, &mut inv, &mut out, &gui, 2, 2, 0, false, 500.0);
        assert_eq!((s.pending[1].slot, s.pending[1].mode), (-999, 4));
    }

    #[test]
    fn s_slots_are_taken_as_the_truth() {
        let mut inv = InventoryPlayer::new();
        let mut s = WindowScreen::crafting(2, [0, 0, 0], recipes());
        let wire = |id: u16, n: u8| WireStack::new(id as i16, n, 0);
        s.apply(&mut inv, &[(10, wire(item::PAPER, 3)), (1, wire(item::PAPER, 2)), (-1, wire(item::PAPER, 9)), (200, wire(item::PAPER, 1))]);
        assert_eq!(inv.get_stack_in_slot(9).get_count(), 3);
        assert_eq!(inv.cursor.get_count(), 9);
        let grid = slot_of(&s, 1);
        assert_eq!(s.stack(&mut inv, grid).get_count(), 2);
        s.apply(&mut inv, &[(-1, WireStack::EMPTY)]);
        assert!(inv.cursor.is_empty());
    }
}
