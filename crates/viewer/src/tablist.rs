//! The player list in a native session (join.rs): `GuiPlayerTabOverlay.renderPlayerlist` over
//! the names the server sends every page (`{"t":"players","list":[...]}`), shown while Tab is
//! held, as `GuiIngame.renderGameOverlay` does on a remote server. The page's own list has an
//! "N online" line on top; here that line is the overlay's header. Bevy's UI font writes the
//! names, as chat.rs does; the columns are sized by the jar font's widths (`players::Font`).
//! DROPPED: the faces from skins and the ping bars (the server sends neither), the scoreboard
//! column, spectator italics, teams in `PlayerComparator` (names sort as `String.compareTo`), the
//! footer.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::hand::{gui_scale, HideGui};
use crate::players::Font;
use crate::settings::Menu;

/// `renderPlayerlist`: at most 80 entries.
const MAX_ENTRIES: usize = 80;
/// `drawRect` colors: `Integer.MIN_VALUE` behind the list, 553648127 behind each name.
const BACK: Color = Color::srgba(0.0, 0.0, 0.0, 128.0 / 255.0);
const CELL: Color = Color::srgba(1.0, 1.0, 1.0, 32.0 / 255.0);

/// The names the server last sent, sorted.
#[derive(Resource, Default)]
pub struct TabList(pub Vec<String>);

impl TabList {
    pub fn set(&mut self, mut names: Vec<String>) {
        names.sort();
        self.0 = names;
    }
}

/// The page's `players` message: the names; `None` for any other line.
pub fn players_of(json: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("t")?.as_str()? != "players" {
        return None;
    }
    Some(v.get("list")?.as_array()?.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
}

/// Where `renderPlayerlist` puts things, in GUI pixels.
#[derive(Debug, PartialEq)]
pub struct Layout {
    /// `i4`: names per column.
    pub rows: i32,
    /// `j4`: columns.
    pub cols: i32,
    /// `i1`: one entry's width.
    pub cell_w: i32,
    /// `j1`: the first column's left edge.
    pub left: i32,
    /// `l1`: the background's width, the header's included.
    pub back_w: i32,
}

/// `renderPlayerlist`'s sizes for names `widths` wide and a header `header_w` wide on a screen
/// `width` GUI pixels across. No faces are drawn, so `flag` is false: an entry is the widest name
/// plus 13 for the ping slot.
pub fn layout(widths: &[i32], header_w: i32, width: i32) -> Layout {
    let i = widths.iter().copied().max().unwrap_or(0);
    let l3 = widths.len().min(MAX_ENTRIES) as i32;
    let mut i4 = l3;
    let mut j4 = 1;
    while i4 > 20 {
        j4 += 1;
        i4 = (l3 + j4 - 1) / j4;
    }
    let i1 = (j4 * (i + 13)).min(width - 50) / j4;
    let j1 = width / 2 - (i1 * j4 + (j4 - 1) * 5) / 2;
    let l1 = (i1 * j4 + (j4 - 1) * 5).max(header_w);
    Layout { rows: i4, cols: j4, cell_w: i1, left: j1, back_w: l1 }
}

/// The header line, as the page writes it.
fn header(n: usize) -> String {
    format!("{n} online")
}

/// The overlay's root; its children are rebuilt when the names or the window change.
#[derive(Component)]
pub struct TabRoot;

pub fn spawn_tab(mut commands: Commands) {
    commands.spawn((
        TabRoot,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        GlobalZIndex(6),
        Visibility::Hidden,
    ));
}

/// Shows the list while Tab is held and no screen or chat line is open (F1 hides it with the rest
/// of the GUI), and lays it out at the GUI scale.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    list: Res<TabList>,
    menu: Res<Menu>,
    hide: Res<HideGui>,
    font: Option<Res<Font>>,
    window: Single<&Window, With<PrimaryWindow>>,
    root: Single<(Entity, &mut Visibility), With<TabRoot>>,
    mut built: Local<Option<(Vec<String>, u32, u32)>>,
) {
    let (root, mut vis) = root.into_inner();
    let show = keys.pressed(KeyCode::Tab) && !menu.open && !hide.0;
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    if !show {
        return;
    }
    let (pw, ph) = (window.physical_width(), window.physical_height());
    if built.as_ref().is_some_and(|(names, w, h)| *names == list.0 && *w == pw && *h == ph) {
        return;
    }
    *built = Some((list.0.clone(), pw, ph));
    commands.entity(root).despawn_related::<Children>();

    let k = gui_scale(pw, ph).max(1);
    let unit = k as f32 / window.scale_factor();
    // `ScaledResolution`: the width in GUI pixels, rounded up.
    let width = pw.div_ceil(k) as i32;
    let string_width = |s: &str| font.as_ref().map_or(s.chars().count() as i32 * 6, |f| f.string_width(s));
    let names: Vec<&String> = list.0.iter().take(MAX_ENTRIES).collect();
    let head = header(list.0.len());
    let widths: Vec<i32> = names.iter().map(|n| string_width(n)).collect();
    let l = layout(&widths, string_width(&head), width);
    let gp = |n: i32| px(n as f32 * unit);
    let text = |s: String| {
        (
            Text::new(s),
            TextFont { font_size: FontSize::Px(8.0 * unit), ..default() },
            TextColor(Color::WHITE),
            TextShadow { offset: Vec2::splat(unit), color: Color::srgb_u8(0x3f, 0x3f, 0x3f) },
            TextLayout::linebreak(LineBreak::NoWrap),
        )
    };
    let back_left = width / 2 - l.back_w / 2 - 1;
    let back_w = l.back_w / 2 * 2 + 2;
    let mut children = Vec::new();
    // The header: one line at y 10, its own background, then a pixel's gap.
    let mut k1 = 10;
    let head_text = commands.spawn(text(head)).id();
    let head_back = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: gp(back_left),
                top: gp(k1 - 1),
                width: gp(back_w),
                height: gp(9 + 1),
                justify_content: JustifyContent::Center,
                padding: UiRect::top(gp(1)),
                ..default()
            },
            BackgroundColor(BACK),
        ))
        .add_child(head_text)
        .id();
    children.push(head_back);
    k1 += 9 + 1;
    children.push(
        commands
            .spawn((
                Node { position_type: PositionType::Absolute, left: gp(back_left), top: gp(k1 - 1), width: gp(back_w), height: gp(l.rows * 9 + 1), ..default() },
                BackgroundColor(BACK),
            ))
            .id(),
    );
    for (n, name) in names.iter().enumerate() {
        let (col, row) = (n as i32 / l.rows, n as i32 % l.rows);
        let label = commands.spawn(text((*name).clone())).id();
        let cell = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: gp(l.left + col * l.cell_w + col * 5),
                    top: gp(k1 + row * 9),
                    width: gp(l.cell_w),
                    height: gp(8),
                    overflow: Overflow::visible(),
                    ..default()
                },
                BackgroundColor(CELL),
            ))
            .add_child(label)
            .id();
        children.push(cell);
    }
    commands.entity(root).add_children(&children);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_json_becomes_names() {
        assert_eq!(players_of(r#"{"t":"players","list":["xy","bob"]}"#), Some(vec!["xy".into(), "bob".into()]));
        assert_eq!(players_of(r#"{"t":"chat","from":"xy","text":"hi"}"#), None);
        assert_eq!(players_of("garbage"), None);
        let mut l = TabList::default();
        l.set(vec!["xy".into(), "Bob".into(), "alice".into()]);
        assert_eq!(l.0, ["Bob", "alice", "xy"]);
    }

    #[test]
    fn columns_follow_render_playerlist() {
        // Up to 20 names: one column, centered.
        let l = layout(&[30; 3], 40, 400);
        assert_eq!(l, Layout { rows: 3, cols: 1, cell_w: 43, left: 400 / 2 - 43 / 2, back_w: 43 });
        // 21 names: two columns of 11.
        let l = layout(&[30; 21], 0, 400);
        assert_eq!((l.rows, l.cols, l.cell_w), (11, 2, 43));
        assert_eq!(l.back_w, 43 * 2 + 5);
        // 80 and past: four columns of 20, the rest cut.
        let l = layout(&[30; 100], 0, 1000);
        assert_eq!((l.rows, l.cols), (20, 4));
        // A narrow screen caps the width at width - 50.
        let l = layout(&[300; 2], 0, 200);
        assert_eq!(l.cell_w, 150);
        // No names: the header alone sets the width.
        let l = layout(&[], 48, 400);
        assert_eq!((l.rows, l.cols, l.back_w), (0, 1, 48));
    }
}
