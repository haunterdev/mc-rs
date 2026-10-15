//! `GuiEnchantment` on `textures/gui/container/enchanting_table.png`: the window, the three offer
//! buttons with their galactic-alphabet clue text and level cost, and the hover tooltip with the
//! enchantment hint. The offers come from the viewer's copy of `ContainerEnchantment`, fed by
//! `S_SLOTS` and `S_WINDOW_PROP`. A click sends `V_ENCHANT`.
//!
//! DROPPED: the book model and its page-flip animation (`tickBook`), the client-side prediction
//! of `enchantItem` (the server resends the slots), and the italic style of the clue line.

use bevy::prelude::*;
use items::JavaRandom;
use sim::container::enchant::Enchanted;
use sim::container::Container;
use sim::lang::{Arg, Lang};

use super::draw::{self, string_width, Frame};
use super::slots::Gui;
use crate::players::Font;

pub fn plugin(_app: &mut App) {}

/// `GuiContainer.xSize` and `ySize`
pub const SIZE: (i32, i32) = (176, 166);

/// The `standardGalacticFontRenderer`'s texture, `textures/font/ascii_sga.png`.
#[derive(Resource)]
pub struct Galactic(pub Font);

/// What the screen knows about the player and what it has to send.
#[derive(Default)]
pub struct Enchant {
    /// `EntityPlayer.experienceLevel`, set before each frame.
    pub xp_level: i32,
    /// `capabilities.isCreativeMode`
    pub creative: bool,
    /// The buttons clicked and not yet sent as `V_ENCHANT`.
    pub pending: Vec<u8>,
}

/// `EnchantmentNameParts.namePartsArray`
const NAME_PARTS: &str = "the elder scrolls klaatu berata niktu xyzzy bless curse light darkness fire air earth water hot dry cold wet ignite snuff embiggen twist shorten stretch fiddle destroy imbue galvanize enchant free limited range of towards inside sphere cube self other ball mental physical grow shrink demon elemental spirit animal creature beast humanoid undead fresh stale phnglui mglwnafh cthulhu rlyeh wgahnagl fhtagnbaguette";

/// `FontRenderer.sizeStringToWidth`: how many bytes fit in `width`, breaking at the last space.
fn size_to_width(font: &Font, s: &str, width: i32) -> usize {
    let b = s.as_bytes();
    let (mut i, mut j, mut k) = (0, None, 0);
    while k < b.len() {
        if b[k] == b' ' {
            j = Some(k);
        }
        i += font.widths[b[k] as usize] as i32;
        if i > width {
            break;
        }
        k += 1;
    }
    match j {
        Some(j) if k != b.len() && j < k => j,
        _ => k,
    }
}

/// `FontRenderer.listFormattedStringToWidth` for plain text.
pub fn wrap(font: &Font, s: &str, width: i32) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.to_string();
    loop {
        let i = size_to_width(font, &rest, width);
        if rest.len() <= i {
            out.push(rest);
            return out;
        }
        let skip = usize::from(rest.as_bytes()[i] == b' ');
        out.push(rest[..i].to_string());
        rest = rest[i + skip..].to_string();
    }
}

/// `EnchantmentNameParts.generateNewRandomName(font, length)`: three or four words, the first two
/// wrapped lines of them joined by a space.
pub fn generate_name(font: &Font, rand: &mut JavaRandom, length: i32) -> String {
    let parts: Vec<&str> = NAME_PARTS.split(' ').collect();
    let n = rand.next_int_bounded(2) + 3;
    let mut s = String::new();
    for j in 0..n {
        if j > 0 {
            s.push(' ');
        }
        s += parts[rand.next_int_bounded(parts.len() as i32) as usize];
    }
    let mut list = wrap(font, &s, length);
    list.truncate(2);
    list.join(" ")
}

/// `Enchantment.getTranslatedName(level)`: the name, red for a curse, then the level as a roman
/// numeral unless it is a level-1 enchantment that has only one level. Returns the text and its
/// color.
pub fn translated_name(lang: &Lang, id: u8, level: i32) -> Option<(String, u32)> {
    let def = items::creative::enchantment(id)?;
    let mut s = lang.translate(def.key).to_string();
    if level != 1 || items::enchant::max_level(id) != 1 {
        s += " ";
        s += lang.translate(&format!("enchantment.level.{level}"));
    }
    Some((s, if items::enchant::is_curse(id) { 0xFF5555 } else { 0xAAAAAA }))
}

/// `ItemStack.getTooltip`'s enchantment lines: each `Enchantment.getTranslatedName`, gray, red for
/// a curse. (An enchanted book lists its stored enchantments the same way.)
pub fn tooltip_lines(stack: &items::ItemStack, lang: &Lang) -> Vec<(String, u32, bool)> {
    // Item.addInformation first (potion effects, a record's title), then the enchantments.
    let tr = |k: &str| lang.translate(k).to_string();
    let fmt = |k: &str, a: &[String]| lang.format(k, &a.iter().map(|s| Arg::from(s.as_str())).collect::<Vec<_>>());
    let mut out: Vec<(String, u32, bool)> = items::tooltip::item_information(stack, &tr, &fmt).into_iter().map(|(s, c)| (s, c, false)).collect();
    out.extend(stack.ench.iter().filter_map(|(id, level)| translated_name(lang, id, level as i32)).map(|(s, c)| (s, c, false)));
    out
}

/// `ItemStack.getTooltip`'s first line: the display name in the stack's rarity colour.
pub fn name_line(stack: &items::ItemStack, name: String) -> (String, u32, bool) {
    (name, items::tooltip::rarity(stack).color(), false)
}

/// Whether the player could take offer `k` now: `enchantItem` accepting on the viewer's copy.
pub fn can_enchant(c: &mut Container, e: &Enchant, k: usize) -> bool {
    c.enchant_item(e.creative, e.xp_level, k, || 0) == Enchanted::Accepted
}

/// `GuiEnchantment.mouseClicked`: the button under `(mx, my)` that can be taken.
pub fn clicked(c: &mut Container, e: &Enchant, gui: &Gui, mx: i32, my: i32) -> Option<u8> {
    (0..3).find(|&k| {
        let (l, i1) = (mx - (gui.left + 60), my - (gui.top + 14 + 19 * k as i32));
        l >= 0 && i1 >= 0 && l < 108 && i1 < 19 && can_enchant(c, e, k)
    })
    .map(|k| k as u8)
}

/// `drawGuiContainerBackgroundLayer` minus the book: the window, then each button with its
/// scrambled name and level.
#[allow(clippy::too_many_arguments)]
pub fn background(frame: &mut Frame, font: &Font, galactic: Option<&Font>, gui: &Gui, c: &Container, e: &Enchant, mx: i32, my: i32) {
    frame.bands[draw::BACKGROUND].sprite(gui.left, gui.top, 0, 0, SIZE.0, SIZE.1);
    let t = c.enchant_ref();
    let mut rand = JavaRandom::new(t.xp_seed as i64);
    let lapis = t.lapis_amount();
    for l in 0..3i32 {
        let (i1, y) = (gui.left + 60, gui.top + 14 + 19 * l);
        let j1 = i1 + 20;
        let k1 = t.levels[l as usize];
        if k1 == 0 {
            frame.bands[draw::BACKGROUND].sprite(i1, y, 0, 185, 108, 19);
            continue;
        }
        let s = k1.to_string();
        let l1 = 86 - string_width(font, &s, false);
        let name = generate_name(font, &mut rand, l1);
        let mut i2 = 6_839_882u32;
        let off = (lapis < l + 1 || e.xp_level < k1) && !e.creative;
        if off {
            frame.bands[draw::BACKGROUND].sprite(i1, y, 0, 185, 108, 19);
            frame.bands[draw::BACKGROUND].sprite(i1 + 1, y + 1, 16 * l, 239, 16, 16);
            split(frame, galactic, &name, j1, y + 2, l1, (i2 & 16_711_422) >> 1);
            i2 = 4_226_832;
        } else {
            let (j2, k2) = (mx - (gui.left + 60), my - y);
            if j2 >= 0 && k2 >= 0 && j2 < 108 && k2 < 19 {
                frame.bands[draw::BACKGROUND].sprite(i1, y, 0, 204, 108, 19);
                i2 = 16_777_088;
            } else {
                frame.bands[draw::BACKGROUND].sprite(i1, y, 0, 166, 108, 19);
            }
            frame.bands[draw::BACKGROUND].sprite(i1 + 1, y + 1, 16 * l, 223, 16, 16);
            split(frame, galactic, &name, j1, y + 2, l1, i2);
            i2 = 8_453_920;
        }
        frame.bands[draw::TEXT].text(font, &s, j1 + 86 - string_width(font, &s, false), y + 2 + 5, i2, true, false);
    }
}

/// `FontRenderer.drawSplitString` in the galactic font: the text wrapped to `width`, 9 pixels a
/// line.
fn split(frame: &mut Frame, galactic: Option<&Font>, s: &str, x: i32, y: i32, width: i32, color: u32) {
    let Some(g) = galactic else { return };
    for (n, line) in wrap(g, s, width).iter().enumerate() {
        frame.bands[draw::GALACTIC].text(g, line, x, y + 9 * n as i32, color, false, false);
    }
}

/// `drawGuiContainerForegroundLayer`: the table's name at `(12, 5)` and the inventory's at
/// `(8, ySize - 96 + 2)`.
pub fn foreground(frame: &mut Frame, font: &Font, title: &str, inventory: &str, gui: &Gui) {
    frame.bands[draw::TEXT].text(font, title, gui.left + 12, gui.top + 5, 0x404040, false, false);
    frame.bands[draw::TEXT].text(font, inventory, gui.left + 8, gui.top + SIZE.1 - 96 + 2, 0x404040, false, false);
}

/// `drawScreen`'s hover text over an offer: the clue line, then the lapis and level costs.
pub fn tooltip(lang: &Lang, c: &Container, e: &Enchant, gui: &Gui, mx: i32, my: i32) -> Option<Vec<(String, u32, bool)>> {
    let t = c.enchant_ref();
    let lapis = t.lapis_amount();
    for j in 0..3usize {
        let k = t.levels[j];
        let l = t.world_clue[j];
        let i1 = j as i32 + 1;
        let (rx, ry) = (mx - gui.left - 60, my - gui.top - 14 - 19 * j as i32);
        if !(rx >= 0 && ry >= 0 && rx < 108 && ry < 17 && k > 0 && l >= 0 && t.clue[j] >= 0) {
            continue;
        }
        let Some((name, _)) = translated_name(lang, t.clue[j] as u8, l) else { continue };
        let mut list = vec![(lang.format("container.enchant.clue", &[Arg::Str(name)]), 0xFFFFFF, false)];
        if !e.creative {
            list.push((String::new(), 0xFFFFFF, false));
            if e.xp_level < k {
                list.push((lang.format("container.enchant.level.requirement", &[Arg::Int(k as i64)]), 0xFF5555, false));
            } else {
                let s = if i1 == 1 { lang.format("container.enchant.lapis.one", &[]) } else { lang.format("container.enchant.lapis.many", &[Arg::Int(i1 as i64)]) };
                list.push((s, if lapis >= i1 { 0xAAAAAA } else { 0xFF5555 }, false));
                let s = if i1 == 1 { lang.format("container.enchant.level.one", &[]) } else { lang.format("container.enchant.level.many", &[Arg::Int(i1 as i64)]) };
                list.push((s, 0xAAAAAA, false));
            }
        }
        return Some(list);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use items::ids::item;
    use items::ItemStack;
    use sim::container::PlayerRef;
    use sim::inventory::InventoryPlayer;

    fn font() -> Font {
        Font { image: Handle::default(), widths: [6; 256] }
    }

    /// A viewer table with the sword and lapis in, the offers as the server's properties give them.
    fn table(levels: [i32; 3]) -> Container {
        let mut c = Container::enchanting(1, [0, 0, 0], 0, None);
        let mut inv = InventoryPlayer::new();
        let mut p = PlayerRef::new(&mut inv, false);
        c.put_stack(&mut p, 0, ItemStack::of(item::DIAMOND_SWORD));
        c.put_stack(&mut p, 1, ItemStack::new(item::DYE, 3, 4));
        for (n, v) in levels.iter().enumerate() {
            c.update_enchant_property(n as u16, *v);
        }
        for (n, v) in [(4, 16), (5, 17), (6, 20), (7, 1), (8, 2), (9, 3)] {
            c.update_enchant_property(n, v);
        }
        c
    }

    #[test]
    fn the_gui_lays_out_three_offers_that_can_be_clicked() {
        let mut c = table([8, 19, 30]);
        let gui = Gui::new(640, 480, SIZE.0, SIZE.1);
        let e = Enchant { xp_level: 20, creative: false, pending: Vec::new() };
        // Offers 1 and 2 are affordable at level 20 with 3 lapis, offer 3 needs level 30.
        let at = |k: i32| (gui.left + 60 + 50, gui.top + 14 + 19 * k + 5);
        let (x, y) = at(0);
        assert_eq!(clicked(&mut c, &e, &gui, x, y), Some(0));
        let (x, y) = at(1);
        assert_eq!(clicked(&mut c, &e, &gui, x, y), Some(1));
        let (x, y) = at(2);
        assert_eq!(clicked(&mut c, &e, &gui, x, y), None);
        // Creative takes all three; outside the buttons nothing is hit.
        let creative = Enchant { creative: true, ..Enchant::default() };
        assert_eq!(clicked(&mut c, &creative, &gui, x, y), Some(2));
        assert_eq!(clicked(&mut c, &e, &gui, gui.left + 10, gui.top + 20), None);
        // Each offer draws its button, icon and level; the window itself is drawn first.
        let mut frame = Frame::default();
        let g = font();
        background(&mut frame, &g, Some(&g), &gui, &c, &e, -1, -1);
        let quads = frame.bands[draw::BACKGROUND].vertices() / 4;
        assert_eq!(quads, 1 + 3 * 2, "the window and a button and icon for each offer");
        assert!(!frame.bands[draw::GALACTIC].is_empty() && !frame.bands[draw::TEXT].is_empty());
    }

    #[test]
    fn an_empty_table_draws_only_the_disabled_buttons() {
        let c = Container::enchanting(1, [0, 0, 0], 0, None);
        let gui = Gui::new(640, 480, SIZE.0, SIZE.1);
        let mut frame = Frame::default();
        let g = font();
        background(&mut frame, &g, Some(&g), &gui, &c, &Enchant::default(), 0, 0);
        assert_eq!(frame.bands[draw::BACKGROUND].vertices() / 4, 4);
        assert!(frame.bands[draw::GALACTIC].is_empty());
    }

    #[test]
    fn the_clue_names_come_from_the_seed_and_wrap_to_two_lines() {
        let f = font();
        let name = |seed: i64| generate_name(&f, &mut JavaRandom::new(seed), 40);
        assert_eq!(name(5), name(5));
        assert!((0..30).any(|s| name(s) != name(5)));
        // 6 pixels a character: 40 pixels hold six, so the name is at most two wrapped lines.
        for s in 0..50 {
            assert!(wrap(&f, &name(s), 40).len() <= 2 || name(s).len() > 12, "{}", name(s));
        }
        assert_eq!(wrap(&f, "ab cd ef", 14), vec!["ab", "cd", "ef"]);
    }

    #[test]
    fn an_enchanted_stack_decodes_with_its_glint_and_lists_its_enchantments() {
        let mut sword = ItemStack::of(item::DIAMOND_SWORD);
        sword.add_enchantment(items::enchant::id::SHARPNESS, 5);
        sword.add_enchantment(items::enchant::id::SILK_TOUCH, 1);
        let back = crate::survival::items::stack_of(&crate::survival::net::wire_stack(&sword));
        assert!(back.has_effect() && back.is_item_enchanted());
        assert!(!crate::survival::items::stack_of(&crate::survival::net::wire_stack(&ItemStack::of(item::DIAMOND_SWORD))).has_effect());
        let key = items::creative::enchantment(items::enchant::id::SILK_TOUCH).unwrap().key;
        let lang = Lang::parse(&format!("enchantment.damage.all=Sharpness
enchantment.level.5=V
{key}=Silk Touch"));
        let lines = tooltip_lines(&back, &lang);
        assert_eq!(lines, vec![("Sharpness V".to_string(), 0xAAAAAA, false), ("Silk Touch".to_string(), 0xAAAAAA, false)], "a level-1 single-level enchantment has no numeral");
        // Books, golden apples and the glint: no enchantment lines without a list.
        assert!(tooltip_lines(&ItemStack::of(item::ENCHANTED_BOOK), &lang).is_empty());
        assert!(ItemStack::of(item::ENCHANTED_BOOK).has_effect() && ItemStack::new(item::GOLDEN_APPLE, 1, 1).has_effect());
    }

    #[test]
    fn the_hover_text_names_the_enchantment_and_the_costs() {
        let lang = Lang::parse("container.enchant.clue=%s . . . ?\nenchantment.level.1=I\nenchantment.level.2=II\nenchantment.damage.undead=Smite\ncontainer.enchant.lapis.many=%d Lapis Lazuli\ncontainer.enchant.lapis.one=1 Lapis Lazuli\ncontainer.enchant.level.many=%d Enchantment Levels\ncontainer.enchant.level.one=1 Enchantment Level\ncontainer.enchant.level.requirement=Level Requirement: %d");
        let c = table([8, 19, 30]);
        let gui = Gui::new(640, 480, SIZE.0, SIZE.1);
        let e = Enchant { xp_level: 20, creative: false, pending: Vec::new() };
        let over = |k: i32| (gui.left + 100, gui.top + 14 + 19 * k + 3);
        let (x, y) = over(1);
        let tip = tooltip(&lang, &c, &e, &gui, x, y).unwrap();
        assert!(tip[0].0.contains("Smite II"), "{:?}", tip);
        assert_eq!(tip.len(), 4);
        assert_eq!(tip[2].0, "2 Lapis Lazuli");
        let (x, y) = over(2);
        let tip = tooltip(&lang, &c, &e, &gui, x, y).unwrap();
        assert_eq!((tip.len(), tip[2].1), (3, 0xFF5555), "too few levels: the requirement in red");
        assert!(tooltip(&lang, &c, &e, &gui, gui.left + 5, gui.top + 5).is_none());
    }
}
