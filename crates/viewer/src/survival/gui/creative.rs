//! The creative inventory, `GuiContainerCreative`: E opens it in creative mode. The tabs list
//! every item (`ContainerCreative` from sim, scrolled with the wheel or the scrollbar), the
//! search tab filters them by name, and the inventory tab shows the player's own slots
//! (`inventoryContainer`, sim's `Container::player`) with the destroy slot.
//!
//! The screen edits the viewer's [`Inventory`] directly and tells the server each slot that
//! changed (`V_CREATIVE_SLOT`, the `CreativeCrafting` listener's `sendSlotPacket` after
//! `detectAndSendChanges`), plus `sendPacketDropItem` drops as slot -1.
//!
//! DROPPED: the hotbar snapshots (`CreativeSettings`, the C and X keys; the hotbar tab shows
//! only its locked papers), the player model on the inventory tab
//! (`GuiInventory.drawEntityOnScreen`), the stats button, item rarity colors and every
//! tooltip line after the name, the italic of a custom name, the search field's cursor
//! movement, selection and clipboard (typing appends, Backspace deletes the last character),
//! characters outside ASCII (the unicode font), and the search tree's tooltip-line matches
//! (names and registry ids are searched).

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use items::creative::{display_all_relevant_items, enchantment, tab_of, Tab};
use items::ids::item;
use items::names::display_name;
use items::stack::are_item_stack_tags_equal;
use items::{ItemStack, Tag};
use sim::container::{get_drag_event, ClickType, Container, Inv, PlayerRef};
use sim::inventory::InventoryPlayer;
use sim::lang::Arg;
use std::sync::Arc;

use super::draw::{self, trim_to_width, Frame, Icon};
use super::slots::{
    in_region, inventory_slot_pos, scroll_from_mouse, scroll_track, tab_at, tab_hovered, tab_sprite, thumb_y, wheel_scroll, Gui, CREATIVE_H,
    CREATIVE_W,
};
use super::{check_hotbar_keys, draw_cursor, draw_slots, mouse_click_move, mouse_clicked, mouse_released, Base, GuiLang, Screen, Sent};
use crate::hotbar::Inventory;
use crate::link::{Link, StreamCursor};
use crate::player::Player;
use crate::players::Font;
use crate::settings::{self, Menu};
use crate::survival::net::{send_creative_slot, wire_stack};

pub fn plugin(app: &mut App) {
    app.init_resource::<Creative>()
        .add_systems(
        Update,
        (keys.after(settings::toggle_menu).before(crate::input::focus), draw_screen.after(keys)).run_if(resource_exists::<super::ContainerImages>),
    );
}

/// The open screen, and what outlives it: `selectedTabIndex` is static in Java.
#[derive(Resource)]
pub struct Creative {
    pub screen: Option<CreativeScreen>,
    pub tab: Tab,
    /// `--screen creative[:TAB]`: open it on the first frame, for a `--shot`.
    pub open_at_start: Option<Tab>,
}

impl Default for Creative {
    fn default() -> Creative {
        Creative { screen: None, tab: Tab::BuildingBlocks, open_at_start: None }
    }
}

/// A slot of the screen: one of `ContainerCreative`'s, an inventory-tab `CreativeSlot` around a
/// `ContainerPlayer` slot, or the destroy slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SlotRef {
    Creative(usize),
    Wrapped(usize),
    Destroy,
}

#[derive(Clone, Copy, Debug)]
struct ScreenSlot {
    r: SlotRef,
    x: i32,
    y: i32,
}

/// The search field's text (`GuiTextField` with the cursor always at the end).
struct SearchField {
    text: String,
    /// `lineScrollOffset`
    offset: usize,
}

/// `GuiTextField.maxStringLength` as `initGui` sets it, and the field's width.
const SEARCH_MAX: usize = 50;
const SEARCH_W: i32 = 80;

impl SearchField {
    /// `setSelectionPos(text.length())`
    fn set_selection_pos(&mut self, font: &Font) {
        let i = self.text.len();
        let position = i;
        if self.offset > i {
            self.offset = i;
        }
        let s = trim_to_width(font, &self.text[self.offset..], SEARCH_W, false);
        let k = s.len() + self.offset;
        if position == self.offset {
            self.offset = self.offset.saturating_sub(trim_to_width(font, &self.text, SEARCH_W, true).len());
        }
        if position > k {
            self.offset += position - k;
        } else if position <= self.offset {
            self.offset -= self.offset - position;
        }
        self.offset = self.offset.min(i);
    }

    fn set_text(&mut self, font: &Font, text: &str) {
        self.text = text.chars().take(SEARCH_MAX).collect();
        self.set_selection_pos(font);
    }

    /// `textboxKeyTyped` while focused: true when the field took the key.
    fn key_typed(&mut self, font: &Font, key: KeyCode, c: Option<char>) -> bool {
        match key {
            KeyCode::Backspace => {
                self.text.pop();
                self.set_selection_pos(font);
                true
            }
            KeyCode::Home | KeyCode::End | KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::Delete => true,
            _ => match c {
                // ChatAllowedCharacters.isAllowedCharacter
                Some(c) if c.is_ascii() && c >= ' ' && c != '\x7f' => {
                    if self.text.len() < SEARCH_MAX {
                        self.text.push(c);
                    }
                    self.set_selection_pos(font);
                    true
                }
                _ => false,
            },
        }
    }
}

pub struct CreativeScreen {
    base: Base,
    /// `ContainerCreative`
    container: Container,
    /// `mc.player.inventoryContainer`
    player: Container,
    slots: Vec<ScreenSlot>,
    tab: Tab,
    /// `currentScroll`
    scroll: f32,
    is_scrolling: bool,
    was_clicking: bool,
    search: SearchField,
    clear_search: bool,
    /// The inventory as the server was last told it, by `getStackInSlot` index.
    sent: [ItemStack; 41],
}

/// `ContainerPlayer`'s slot number for an `InventoryPlayer` index.
fn container_slot(index: usize) -> i16 {
    match index {
        0..=8 => 36 + index as i16,
        9..=35 => index as i16,
        36..=39 => 5 + (3 - (index as i16 - 36)),
        _ => 45,
    }
}

impl CreativeScreen {
    /// `initGui`: `selectedTabIndex` reset, then `setCurrentCreativeTab` with the saved tab.
    fn new(tab: Tab, inv: &InventoryPlayer, font: &Font) -> CreativeScreen {
        let mut s = CreativeScreen {
            base: Base::default(),
            container: Container::creative(),
            player: Container::player(Arc::new(items::Recipes::default())),
            slots: Vec::new(),
            tab,
            scroll: 0.0,
            is_scrolling: false,
            was_clicking: false,
            search: SearchField { text: String::new(), offset: 0 },
            clear_search: false,
            sent: std::array::from_fn(|i| inv.get_stack_in_slot(i)),
        };
        s.set_tab(tab, font);
        s
    }

    fn list(&mut self) -> &mut Vec<ItemStack> {
        &mut self.container.creative.as_mut().expect("the creative container").item_list
    }

    /// `setCurrentCreativeTab`
    fn set_tab(&mut self, tab: Tab, font: &Font) {
        self.tab = tab;
        self.base.drag_slots.clear();
        let list = self.list();
        list.clear();
        if tab == Tab::Hotbar {
            // DROPPED: the saved snapshots; each row is empty but for its locked paper.
            for j in 0..9 {
                for k in 0..9 {
                    list.push(if k == j { ItemStack::of(item::PAPER) } else { ItemStack::EMPTY });
                }
            }
        } else if tab != Tab::Search {
            *list = display_all_relevant_items(tab);
        }
        self.slots = if tab == Tab::Inventory {
            let mut slots: Vec<ScreenSlot> = (0..self.player.slots.len())
                .map(|l| {
                    let (x, y) = inventory_slot_pos(l);
                    ScreenSlot { r: SlotRef::Wrapped(l), x, y }
                })
                .collect();
            slots.push(ScreenSlot { r: SlotRef::Destroy, x: 173, y: 112 });
            slots
        } else {
            self.container.slots.iter().enumerate().map(|(i, s)| ScreenSlot { r: SlotRef::Creative(i), x: s.x, y: s.y }).collect()
        };
        if tab == Tab::Search {
            // The callers then run update_search, which needs the language file.
            self.search.set_text(font, "");
        }
        self.scroll = 0.0;
        self.container.scroll_to(0.0);
    }

    /// `updateCreativeSearch`: every item when the field is empty, else the search tree's
    /// matches (`SearchTree.search`): a lowercase name containing the text, or with a ':' in
    /// the text, a registry id containing it.
    fn update_search(&mut self, lang: Option<&GuiLang>) {
        let all = display_all_relevant_items(Tab::Search);
        let q = self.search.text.to_lowercase();
        *self.list() = if q.is_empty() {
            all
        } else {
            let names = lang.map(|l| l.0.map());
            all.into_iter()
                .filter(|s| {
                    let by_name = names.is_some_and(|m| display_name(s, m).to_lowercase().contains(&q));
                    by_name || q.contains(':') && format!("minecraft:{}", s.def().name).contains(&q)
                })
                .collect()
        };
        self.scroll = 0.0;
        self.container.scroll_to(0.0);
    }

    fn needs_scroll_bars(&self) -> bool {
        self.tab != Tab::Inventory && self.tab.has_scrollbar() && self.container.can_scroll()
    }

    fn inventory_of(&self, i: usize) -> Inv {
        match self.slots[i].r {
            SlotRef::Creative(c) => self.container.slots[c].inventory,
            SlotRef::Wrapped(l) => self.player.slots[l].inventory,
            SlotRef::Destroy => Inv::Basic,
        }
    }

    /// `basicInventory`
    fn basic(&self) -> &[ItemStack] {
        &self.container.creative.as_ref().expect("the creative container").basic
    }

    /// A hotbar-tab paper, which `CustomCreativeLock` keeps in its `LockedSlot`.
    fn locked(&self, stack: &ItemStack) -> bool {
        self.tab == Tab::Hotbar && stack.get_item() == item::PAPER
    }

    /// The `itemList` row `scrollTo` put at the top.
    fn first_row(&self) -> i32 {
        let n = self.container.creative.as_ref().map_or(0, |c| c.item_list.len()) as i32;
        let i = (n + 9 - 1) / 9 - 5;
        (((self.scroll * i as f32) as f64 + 0.5) as i32).max(0)
    }

    /// `handleMouseInput`'s wheel.
    fn wheel(&mut self, notch: i32) {
        if notch != 0 && self.needs_scroll_bars() {
            let n = self.container.creative.as_ref().map_or(0, |c| c.item_list.len());
            self.scroll = wheel_scroll(self.scroll, notch, n);
            self.container.scroll_to(self.scroll);
        }
    }

    /// `keyTyped`. False when the key closes the screen.
    #[allow(clippy::too_many_arguments)]
    fn key_typed(&mut self, inv: &mut InventoryPlayer, out: &mut Vec<Sent>, font: &Font, lang: Option<&GuiLang>, key: KeyCode, c: Option<char>, chat_held: bool, ctrl: bool) -> bool {
        if self.tab != Tab::Search {
            if key == KeyCode::KeyT || chat_held {
                self.set_tab(Tab::Search, font);
                self.update_search(lang);
                return true;
            }
            return self.container_key_typed(inv, out, key, ctrl);
        }
        if self.clear_search {
            self.clear_search = false;
            self.search.set_text(font, "");
        }
        if check_hotbar_keys(self, inv, out, digit(key)) {
            return true;
        }
        if self.search.key_typed(font, key, c) {
            self.update_search(lang);
            true
        } else {
            self.container_key_typed(inv, out, key, ctrl)
        }
    }

    /// `GuiContainer.keyTyped`: Escape or E closes, a number key swaps, Q throws the hovered
    /// stack (all of it with Ctrl).
    fn container_key_typed(&mut self, inv: &mut InventoryPlayer, out: &mut Vec<Sent>, key: KeyCode, ctrl: bool) -> bool {
        let close = matches!(key, KeyCode::Escape | KeyCode::KeyE);
        check_hotbar_keys(self, inv, out, digit(key));
        if let Some(h) = self.base.hovered.filter(|&h| !self.stack(inv, h).is_empty()) {
            if key == KeyCode::KeyQ {
                let n = self.slot_number(h);
                self.handle_mouse_click(inv, out, Some(h), n, ctrl as i32, ClickType::Throw);
            }
        }
        !close
    }

    /// `drawScreen`, with the background and foreground layers.
    #[allow(clippy::too_many_arguments)]
    fn draw(&mut self, frame: &mut Frame, font: &Font, lang: &GuiLang, inv: &mut InventoryPlayer, gui: &Gui, mx: i32, my: i32, down: bool) {
        frame.bands[draw::BACKDROP].gradient(0, 0, gui.w, gui.h, 0xC010_1010, 0xD010_1010);
        let (k, l, i1, j1) = scroll_track(gui);
        if !self.was_clicking && down && mx >= k && my >= l && mx < i1 && my < j1 {
            self.is_scrolling = self.needs_scroll_bars();
        }
        if !down {
            self.is_scrolling = false;
        }
        self.was_clicking = down;
        if self.is_scrolling {
            self.scroll = scroll_from_mouse(gui, my);
            self.container.scroll_to(self.scroll);
        }

        self.draw_background(frame, font, gui);
        draw_slots(self, frame, font, inv, gui, mx, my);
        if self.tab.draws_title() {
            let title = lang.0.translate(&self.tab.translation_key()).to_string();
            frame.bands[draw::TEXT].text(font, &title, gui.left + 8, gui.top + 6, 0x404040, false, false);
        }
        draw_cursor(self, frame, font, inv, mx, my);

        if let Some(tab) = Tab::ALL.into_iter().find(|&t| tab_hovered(gui, t, mx, my)) {
            let name = lang.0.translate(&tab.translation_key()).to_string();
            frame.hovering_text(font, &[(name, 0xFFFFFF, false)], mx, my, gui.w, gui.h);
        }
        if self.tab == Tab::Inventory && in_region(gui, 173, 112, 16, 16, mx, my) {
            let bin = lang.0.translate("inventory.binSlot").to_string();
            frame.hovering_text(font, &[(bin, 0xFFFFFF, false)], mx, my, gui.w, gui.h);
        }
        // renderHoveredToolTip
        if let Some(h) = self.base.hovered.filter(|_| inv.cursor.is_empty()) {
            let stack = self.stack(inv, h);
            if !stack.is_empty() {
                self.render_tooltip(frame, font, lang, h, &stack, mx, my, gui);
            }
        }
    }

    /// `drawGuiContainerBackgroundLayer`
    fn draw_background(&self, frame: &mut Frame, font: &Font, gui: &Gui) {
        for tab in Tab::ALL {
            if tab != self.tab {
                self.draw_tab(frame, gui, tab, draw::TABS);
            }
        }
        frame.background = match self.tab.background() {
            "items.png" => 0,
            "item_search.png" => 1,
            _ => 2,
        };
        frame.bands[draw::BACKGROUND].sprite(gui.left, gui.top, 0, 0, CREATIVE_W, CREATIVE_H);
        if self.tab == Tab::Search {
            self.draw_search(frame, font, gui);
        }
        if self.tab.has_scrollbar() {
            let u = 232 + if self.needs_scroll_bars() { 0 } else { 12 };
            frame.bands[draw::FRONT].sprite(gui.left + 175, thumb_y(gui, self.scroll), u, 0, 12, 15);
        }
        self.draw_tab(frame, gui, self.tab, draw::FRONT);
        // DROPPED: GuiInventory.drawEntityOnScreen, the player on the inventory tab.
    }

    /// `drawTab`
    fn draw_tab(&self, frame: &mut Frame, gui: &Gui, tab: Tab, band: usize) {
        let t = tab_sprite(gui, tab, tab == self.tab);
        frame.bands[band].sprite(t.x, t.y, t.u, t.v, 28, 32);
        frame.icons.push(Icon { stack: tab.icon(), x: t.icon.0, y: t.icon.1, z: draw::SLOT_Z });
    }

    /// `GuiTextField.drawTextBox` without a background, the cursor at the end of the text.
    /// The creative screen never calls `updateCursorCounter`, so the cursor does not blink.
    fn draw_search(&self, frame: &mut Frame, font: &Font, gui: &Gui) {
        let text = &self.search.text;
        let j = text.len() - self.search.offset;
        let s = trim_to_width(font, &text[self.search.offset..], SEARCH_W, false);
        let flag = j <= s.len();
        let (l, i1) = (gui.left + 82, gui.top + 6);
        let mut j1 = l;
        if !s.is_empty() {
            let s1 = if flag { &s[..j] } else { s };
            j1 = frame.bands[draw::TEXT].text(font, s1, l, i1, 0xFFFFFF, true, false);
        }
        let flag2 = text.len() >= SEARCH_MAX;
        let mut k1 = j1;
        if !flag {
            k1 = if j > 0 { l + SEARCH_W } else { l };
        } else if flag2 {
            k1 = j1 - 1;
        }
        if flag {
            if flag2 {
                frame.bands[draw::UNDER].rect(k1, i1 - 1, k1 + 1, i1 + 1 + 9, 0xFFD0_D0D0);
            } else {
                frame.bands[draw::TEXT].text(font, "_", k1, i1, 0xFFFFFF, true, false);
            }
        }
    }

    /// `renderToolTip`: the name, and on the search tab the item's tab in blue (`BOLD` then
    /// `BLUE`, and a color code ends bold, so it is not bold).
    #[allow(clippy::too_many_arguments)]
    fn render_tooltip(&self, frame: &mut Frame, font: &Font, lang: &GuiLang, slot: usize, stack: &ItemStack, mx: i32, my: i32, gui: &Gui) {
        let name = if self.locked(stack) {
            // The paper's row: its place in itemList over nine.
            let j = match self.slots[slot].r {
                SlotRef::Creative(c) => (self.container.slots[c].index as i32 + self.first_row() * 9) / 9,
                _ => 0,
            };
            lang.0.format("inventory.hotbarInfo", &[Arg::from("C"), Arg::from((j + 1).to_string())])
        } else {
            display_name(stack, lang.0.map())
        };
        let mut lines = vec![(name, 0xFFFFFF, false)];
        if self.tab == Tab::Search {
            let mut tab = tab_of(stack.get_item());
            if tab.is_none() && stack.get_item() == item::ENCHANTED_BOOK {
                if let Tag::StoredEnchant { id, .. } = stack.tag {
                    let ty = enchantment(id).and_then(|e| e.ty);
                    tab = Tab::ALL.into_iter().find(|t| t.has_relevant_enchantment_type(ty));
                }
            }
            if let Some(t) = tab {
                lines.insert(1, (lang.0.translate(&t.translation_key()).to_string(), 0x5555FF, false));
            }
        }
        frame.hovering_text(font, &lines, mx, my, gui.w, gui.h);
    }

    /// Clears the inventory as the shift-clicked destroy slot asks the server to.
    fn clear_inventory(inv: &mut InventoryPlayer) {
        for i in 0..41 {
            inv.set_inventory_slot_contents(i, ItemStack::EMPTY);
        }
    }

    /// The slots that changed since the last call, as `V_CREATIVE_SLOT` slot numbers.
    fn changed_slots(&mut self, inv: &InventoryPlayer) -> Vec<(i16, ItemStack)> {
        let mut out = Vec::new();
        for i in 0..41 {
            let s = inv.get_stack_in_slot(i);
            if s != self.sent[i] {
                self.sent[i] = s;
                out.push((container_slot(i), s));
            }
        }
        out
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

impl Screen for CreativeScreen {
    fn base(&self) -> &Base {
        &self.base
    }

    fn base_mut(&mut self) -> &mut Base {
        &mut self.base
    }

    fn size(&self) -> (i32, i32) {
        (CREATIVE_W, CREATIVE_H)
    }

    fn slot_count(&self) -> usize {
        self.slots.len()
    }

    fn slot_pos(&self, i: usize) -> (i32, i32) {
        (self.slots[i].x, self.slots[i].y)
    }

    /// A `CreativeSlot` and the destroy slot are added to the list without
    /// `addSlotToContainer`, so their `slotNumber` stays 0.
    fn slot_number(&self, i: usize) -> i32 {
        match self.slots[i].r {
            SlotRef::Creative(c) => c as i32,
            _ => 0,
        }
    }

    fn stack(&self, inv: &mut InventoryPlayer, i: usize) -> ItemStack {
        match self.slots[i].r {
            SlotRef::Creative(c) => self.container.get_stack(&PlayerRef::new(inv, true), c),
            SlotRef::Wrapped(l) => self.player.get_stack(&PlayerRef::new(inv, true), l),
            SlotRef::Destroy => self.basic()[0],
        }
    }

    fn same_inventory(&self, a: usize, b: usize) -> bool {
        self.inventory_of(a) == self.inventory_of(b)
    }

    fn can_take(&self, inv: &mut InventoryPlayer, i: usize) -> bool {
        match self.slots[i].r {
            // LockedSlot
            SlotRef::Creative(c) if self.container.slots[c].inventory == Inv::Basic => {
                let s = self.stack(inv, i);
                s.is_empty() || !self.locked(&s)
            }
            SlotRef::Creative(c) => self.container.can_take_stack(&PlayerRef::new(inv, true), c),
            SlotRef::Wrapped(l) => self.player.can_take_stack(&PlayerRef::new(inv, true), l),
            SlotRef::Destroy => true,
        }
    }

    fn is_item_valid(&self, i: usize, stack: &ItemStack) -> bool {
        match self.slots[i].r {
            SlotRef::Creative(c) => self.container.is_item_valid(c, stack),
            SlotRef::Wrapped(l) => self.player.is_item_valid(l, stack),
            SlotRef::Destroy => true,
        }
    }

    fn item_stack_limit(&self, i: usize, stack: &ItemStack) -> i32 {
        match self.slots[i].r {
            SlotRef::Creative(c) => self.container.get_item_stack_limit(c, stack),
            SlotRef::Wrapped(l) => self.player.get_item_stack_limit(l, stack),
            // InventoryBasic.getInventoryStackLimit
            SlotRef::Destroy => 64,
        }
    }

    /// `ContainerCreative.canDragIntoSlot` on the screen's slot.
    fn can_drag_into(&self, i: usize) -> bool {
        let s = self.slots[i];
        self.inventory_of(i) == Inv::Player || s.y > 90 && s.x <= 162
    }

    fn can_merge(&self, i: usize) -> bool {
        self.slots[i].y > 90
    }

    fn slot_texture(&self, i: usize) -> Option<usize> {
        match self.slots[i].r {
            SlotRef::Wrapped(l @ 5..=8) => Some(l - 5),
            SlotRef::Wrapped(45) => Some(4),
            _ => None,
        }
    }

    /// `GuiContainerCreative.handleMouseClick`
    fn handle_mouse_click(&mut self, inv: &mut InventoryPlayer, out: &mut Vec<Sent>, slot: Option<usize>, slot_id: i32, button: i32, click: ClickType) {
        self.clear_search = true;
        let flag = click == ClickType::QuickMove;
        let click = if slot_id == -999 && click == ClickType::Pickup { ClickType::Throw } else { click };
        if slot.is_none() && self.tab != Tab::Inventory && click != ClickType::QuickCraft {
            if !inv.cursor.is_empty() {
                if button == 0 {
                    out.push(Sent::Drop(inv.cursor));
                    inv.cursor = ItemStack::EMPTY;
                }
                if button == 1 {
                    let itemstack6 = inv.cursor.split_stack(1);
                    out.push(Sent::Drop(itemstack6));
                }
            }
            return;
        }
        if let Some(i) = slot {
            if !self.can_take(inv, i) {
                return;
            }
        }
        let r = slot.map(|i| self.slots[i].r);
        if r == Some(SlotRef::Destroy) && flag {
            // Java sends an empty stack for every inventoryContainer slot and waits for the
            // server; the viewer clears its own copy, and the diff sends the same packets.
            Self::clear_inventory(inv);
        } else if self.tab == Tab::Inventory {
            if r == Some(SlotRef::Destroy) {
                inv.cursor = ItemStack::EMPTY;
            } else if let Some(l) = match r {
                Some(SlotRef::Wrapped(l)) if click == ClickType::Throw && !self.player.get_stack(&PlayerRef::new(inv, true), l).is_empty() => Some(l),
                _ => None,
            } {
                let n = if button == 0 { 1 } else { self.player.get_stack(&PlayerRef::new(inv, true), l).get_max_stack_size() };
                let itemstack = self.player.decr_stack_size(&mut PlayerRef::new(inv, true), l, n);
                out.push(Sent::Drop(itemstack));
            } else if click == ClickType::Throw && !inv.cursor.is_empty() {
                out.push(Sent::Drop(inv.cursor));
                inv.cursor = ItemStack::EMPTY;
            } else {
                let id = match r {
                    Some(SlotRef::Wrapped(l)) => l as i32,
                    _ => slot_id,
                };
                self.player.slot_click(id, button, click, &mut PlayerRef::new(inv, true));
            }
        } else if let Some(SlotRef::Creative(c)) = r.filter(|_| click != ClickType::QuickCraft).filter(|r| matches!(r, SlotRef::Creative(c) if self.container.slots[*c].inventory == Inv::Basic)) {
            let itemstack5 = inv.cursor;
            let itemstack7 = self.container.get_stack(&PlayerRef::new(inv, true), c);
            match click {
                ClickType::Swap => {
                    if !itemstack7.is_empty() && (0..9).contains(&button) {
                        let mut itemstack10 = itemstack7.copy();
                        itemstack10.set_count(itemstack10.get_max_stack_size());
                        inv.set_inventory_slot_contents(button as usize, itemstack10);
                    }
                    return;
                }
                ClickType::Clone => {
                    if inv.cursor.is_empty() && !itemstack7.is_empty() {
                        let mut itemstack9 = itemstack7.copy();
                        itemstack9.set_count(itemstack9.get_max_stack_size());
                        inv.cursor = itemstack9;
                    }
                    return;
                }
                ClickType::Throw => {
                    if !itemstack7.is_empty() {
                        let mut itemstack8 = itemstack7.copy();
                        itemstack8.set_count(if button == 0 { 1 } else { itemstack8.get_max_stack_size() });
                        out.push(Sent::Drop(itemstack8));
                    }
                    return;
                }
                _ => {}
            }
            if !itemstack5.is_empty() && !itemstack7.is_empty() && itemstack5.is_item_equal(&itemstack7) && are_item_stack_tags_equal(&itemstack5, &itemstack7) {
                if button == 0 {
                    if flag {
                        let max = inv.cursor.get_max_stack_size();
                        inv.cursor.set_count(max);
                    } else if inv.cursor.get_count() < inv.cursor.get_max_stack_size() {
                        inv.cursor.grow(1);
                    }
                } else {
                    inv.cursor.shrink(1);
                }
            } else if !itemstack7.is_empty() && itemstack5.is_empty() {
                inv.cursor = itemstack7.copy();
                if flag {
                    let max = inv.cursor.get_max_stack_size();
                    inv.cursor.set_count(max);
                }
            } else if button == 0 {
                inv.cursor = ItemStack::EMPTY;
            } else {
                inv.cursor.shrink(1);
            }
        } else {
            let c = match r {
                Some(SlotRef::Creative(c)) => Some(c),
                _ => None,
            };
            let itemstack3 = c.map_or(ItemStack::EMPTY, |c| self.container.get_stack(&PlayerRef::new(inv, true), c));
            self.container.slot_click(c.map_or(slot_id, |c| c as i32), button, click, &mut PlayerRef::new(inv, true));
            // The hotbar packets after a drag (get_drag_event 2) and the clicked slot's are the
            // diff's; the extra drop of a thrown stack is sent here.
            if get_drag_event(button) != 2 && c.is_some() && click == ClickType::Throw && !itemstack3.is_empty() {
                let mut itemstack2 = itemstack3.copy();
                itemstack2.set_count(if button == 0 { 1 } else { itemstack2.get_max_stack_size() });
                out.push(Sent::Drop(itemstack2));
            }
        }
    }
}

/// The pointer in physical pixels from the window's top left: the stream's when streamed.
fn pointer(window: &Window, stream: Option<&StreamCursor>) -> Option<Vec2> {
    stream.and_then(|c| c.0).or_else(|| window.physical_cursor_position())
}

/// Opens the screen on E in creative mode, closes it on E or Escape, and feeds it the keys,
/// buttons and wheel; then sends the slots that changed.
#[allow(clippy::too_many_arguments)]
fn keys(
    mut creative: ResMut<Creative>,
    mut menu: ResMut<Menu>,
    mut inventory: ResMut<Inventory>,
    player: Res<Player>,
    mut typed: MessageReader<KeyboardInput>,
    mut wheel: MessageReader<MouseWheel>,
    mut pressed: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    stream_cursor: Option<Res<StreamCursor>>,
    time: Res<Time>,
    font: Option<Res<Font>>,
    lang: Option<Res<GuiLang>>,
    link: Option<Res<Link>>,
) {
    let Some(font) = font.as_deref() else { return };
    let creative = &mut *creative;
    let inv = &mut inventory.player;
    let creative_mode = player.game_mode == proto::GM_CREATIVE;
    let Some(screen) = creative.screen.as_mut() else {
        let start = creative.open_at_start.take();
        if creative_mode && !menu.open && (pressed.just_pressed(KeyCode::KeyE) || start.is_some()) {
            if let Some(tab) = start {
                creative.tab = tab;
            }
            let mut screen = CreativeScreen::new(creative.tab, inv, font);
            if screen.tab == Tab::Search {
                screen.update_search(lang.as_deref());
            }
            creative.screen = Some(screen);
            menu.open = true;
            typed.clear();
            wheel.clear();
            pressed.clear();
        }
        return;
    };
    let mut out = Vec::new();
    let mut open = menu.open && creative_mode;
    if open {
        let shift = link.as_deref().is_some_and(|l| l.shift_held()) || pressed.pressed(KeyCode::ShiftLeft) || pressed.pressed(KeyCode::ShiftRight);
        let ctrl = pressed.pressed(KeyCode::ControlLeft) || pressed.pressed(KeyCode::ControlRight);
        let chat_held = pressed.pressed(KeyCode::KeyT);
        for ev in typed.read() {
            if ev.state != ButtonState::Pressed || !open {
                continue;
            }
            let c = match &ev.text {
                Some(t) => t.chars().next(),
                None => crate::sign::typed_char(ev.key_code, shift),
            };
            open = screen.key_typed(inv, &mut out, font, lang.as_deref(), ev.key_code, c, chat_held && ev.key_code != KeyCode::KeyT, ctrl);
        }
        let gui = Gui::new(window.physical_width(), window.physical_height(), CREATIVE_W, CREATIVE_H);
        let at = pointer(&window, stream_cursor.as_deref()).map(|p| gui.mouse(p.x, p.y));
        let now = time.elapsed_secs_f64() * 1000.0;
        if let (Some((mx, my)), true) = (at, open) {
            for (b, n) in [(MouseButton::Left, 0), (MouseButton::Right, 1), (MouseButton::Middle, super::PICK)] {
                // GuiContainerCreative.mouseClicked: a left press on a tab does nothing.
                if mouse.just_pressed(b) && !(n == 0 && tab_at(mx - gui.left, my - gui.top).is_some()) {
                    mouse_clicked(screen, inv, &mut out, &gui, mx, my, n, shift, now);
                }
            }
            if [MouseButton::Left, MouseButton::Right, MouseButton::Middle].iter().any(|&b| mouse.pressed(b)) {
                mouse_click_move(screen, inv, &gui, mx, my);
            }
            for (b, n) in [(MouseButton::Left, 0), (MouseButton::Right, 1), (MouseButton::Middle, super::PICK)] {
                if mouse.just_released(b) {
                    // GuiContainerCreative.mouseReleased: a left release on a tab selects it.
                    match tab_at(mx - gui.left, my - gui.top).filter(|_| n == 0) {
                        Some(tab) => {
                            screen.set_tab(tab, font);
                            if tab == Tab::Search {
                                screen.update_search(lang.as_deref());
                            }
                        }
                        None => mouse_released(screen, inv, &mut out, &gui, mx, my, n, shift),
                    }
                }
            }
            for ev in wheel.read() {
                if ev.y != 0.0 {
                    screen.wheel(ev.y.signum() as i32);
                }
            }
        }
        // Typed letters are not key bindings.
        pressed.clear();
    }
    let changed = screen.changed_slots(inv);
    if !open {
        // onGuiClosed: ContainerCreative drops the cursor stack, on the client only.
        screen.container.on_container_closed(&mut PlayerRef::new(inv, true));
        creative.tab = screen.tab;
        creative.screen = None;
        menu.open = false;
    }
    if let Some(link) = link.as_deref() {
        for (slot, stack) in changed {
            send_creative_slot(link, &proto::CreativeSlot { slot, stack: wire_stack(&stack) });
        }
        for Sent::Drop(stack) in out {
            send_creative_slot(link, &proto::CreativeSlot { slot: -1, stack: wire_stack(&stack) });
        }
    }
}

/// Fills the [`Frame`] from the open screen.
#[allow(clippy::too_many_arguments)]
fn draw_screen(
    mut creative: ResMut<Creative>,
    mut frame: ResMut<Frame>,
    mut inventory: ResMut<Inventory>,
    window: Single<&Window, With<PrimaryWindow>>,
    stream_cursor: Option<Res<StreamCursor>>,
    mouse: Res<ButtonInput<MouseButton>>,
    font: Option<Res<Font>>,
    lang: Option<Res<GuiLang>>,
) {
    let frame = &mut *frame;
    let was_open = frame.open;
    let Some(screen) = creative.screen.as_mut() else {
        if was_open {
            frame.clear();
        }
        return;
    };
    let (Some(font), Some(lang)) = (font, lang) else { return };
    frame.clear();
    frame.open = true;
    let gui = Gui::new(window.physical_width(), window.physical_height(), CREATIVE_W, CREATIVE_H);
    let (mx, my) = pointer(&window, stream_cursor.as_deref()).map_or((-10_000, -10_000), |p| gui.mouse(p.x, p.y));
    screen.draw(frame, &font, &lang, &mut inventory.player, &gui, mx, my, mouse.pressed(MouseButton::Left));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Font {
        Font { image: Handle::default(), widths: [6; 256] }
    }

    #[test]
    fn inventory_indices_map_to_container_player_slots() {
        assert_eq!(container_slot(0), 36);
        assert_eq!(container_slot(8), 44);
        assert_eq!(container_slot(9), 9);
        assert_eq!(container_slot(35), 35);
        // armorInventory 3 is the helmet, slot 5; 0 is the boots, slot 8.
        assert_eq!(container_slot(39), 5);
        assert_eq!(container_slot(36), 8);
        assert_eq!(container_slot(40), 45);
    }

    #[test]
    fn picking_from_a_tab_and_placing_in_the_hotbar_sends_that_slot() {
        let mut inv = InventoryPlayer::new();
        let f = font();
        let mut s = CreativeScreen::new(Tab::BuildingBlocks, &inv, &f);
        let mut out = Vec::new();
        let first = s.stack(&mut inv, 0);
        assert!(!first.is_empty());
        s.handle_mouse_click(&mut inv, &mut out, Some(0), 0, 0, ClickType::Pickup);
        assert!(inv.cursor.is_item_equal(&first));
        assert_eq!(inv.cursor.get_count(), 1);
        // Shift on a list slot fills the cursor stack.
        inv.cursor = ItemStack::EMPTY;
        s.handle_mouse_click(&mut inv, &mut out, Some(0), 0, 0, ClickType::QuickMove);
        assert_eq!(inv.cursor.get_count(), inv.cursor.get_max_stack_size());
        let hotbar0 = (0..s.slot_count()).find(|&i| s.inventory_of(i) == Inv::Player && s.slot_pos(i).0 == 9).unwrap();
        s.handle_mouse_click(&mut inv, &mut out, Some(hotbar0), s.slot_number(hotbar0), 0, ClickType::Pickup);
        assert!(inv.cursor.is_empty());
        assert!(inv.get_stack_in_slot(0).is_item_equal(&first));
        let changed = s.changed_slots(&inv);
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, 36);
        assert!(s.changed_slots(&inv).is_empty());
        assert!(out.is_empty());
    }

    #[test]
    fn throwing_from_a_tab_drops_a_copy() {
        let mut inv = InventoryPlayer::new();
        let f = font();
        let mut s = CreativeScreen::new(Tab::BuildingBlocks, &inv, &f);
        let mut out = Vec::new();
        s.handle_mouse_click(&mut inv, &mut out, Some(0), 0, 0, ClickType::Throw);
        s.handle_mouse_click(&mut inv, &mut out, Some(0), 0, 1, ClickType::Throw);
        let counts: Vec<i32> = out.iter().map(|Sent::Drop(st)| st.get_count()).collect();
        assert_eq!(counts, vec![1, 64]);
        assert!(inv.cursor.is_empty());
    }

    #[test]
    fn the_inventory_tab_shows_the_player_and_the_destroy_slot_clears_the_cursor() {
        let mut inv = InventoryPlayer::new();
        inv.set_inventory_slot_contents(0, ItemStack::new(item::PAPER, 5, 0));
        let f = font();
        let mut s = CreativeScreen::new(Tab::Inventory, &inv, &f);
        assert_eq!(s.slot_count(), 47);
        // ContainerPlayer slot 36 is hotbar 0, drawn on the bottom row.
        let h = (0..s.slot_count()).find(|&i| s.slots[i].r == SlotRef::Wrapped(36)).unwrap();
        assert_eq!(s.slot_pos(h), (9, 112));
        assert_eq!(s.stack(&mut inv, h).get_count(), 5);
        let mut out = Vec::new();
        s.handle_mouse_click(&mut inv, &mut out, Some(h), 0, 0, ClickType::Pickup);
        assert_eq!(inv.cursor.get_count(), 5);
        let d = s.slot_count() - 1;
        s.handle_mouse_click(&mut inv, &mut out, Some(d), 0, 0, ClickType::Pickup);
        assert!(inv.cursor.is_empty());
        assert_eq!(s.changed_slots(&inv), vec![(36, ItemStack::EMPTY)]);
    }

    #[test]
    fn the_search_field_takes_ascii_up_to_fifty() {
        let f = font();
        let mut field = SearchField { text: String::new(), offset: 0 };
        for _ in 0..60 {
            assert!(field.key_typed(&f, KeyCode::KeyA, Some('a')));
        }
        assert_eq!(field.text.len(), SEARCH_MAX);
        assert!(!field.key_typed(&f, KeyCode::KeyA, Some('\u{e9}')));
        assert!(field.key_typed(&f, KeyCode::Backspace, None));
        assert_eq!(field.text.len(), SEARCH_MAX - 1);
        // 49 characters of width 6 do not fit 80 pixels; the start scrolls off.
        assert!(field.offset > 0);
    }

    #[test]
    fn wire_stacks_carry_the_tag() {
        let book = ItemStack::new(item::ENCHANTED_BOOK, 1, 0).with_tag(Tag::StoredEnchant { id: 16, level: 3 });
        let w = wire_stack(&book);
        assert_eq!((w.id, w.count, w.meta, w.tag), (item::ENCHANTED_BOOK as i16, 1, 0, [2, 16, 3]));
        assert_eq!(wire_stack(&ItemStack::EMPTY), proto::WireStack::EMPTY);
    }
}
