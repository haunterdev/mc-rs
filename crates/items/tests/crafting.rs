//! Crafting: JSON parsing (from `MCRS_JAR_DATA`, skipped without it) and matching as
//! `ShapedRecipes.matches` (with mirroring), `ShapelessRecipes.matches`, `RecipeRepairItem`
//! and `getRemainingItems` do it.

mod common;

use items::crafting::WILDCARD;
use items::ids::{block, item};
use items::recipe_json::{parse_recipe, recipe_name, shrink};
use items::{CraftingGrid, Ingredient, ItemStack, Recipe, Recipes};

fn recipes() -> Option<Recipes> {
    let Some(entries) = common::recipe_entries() else {
        eprintln!("skipped: MCRS_JAR_DATA not set");
        return None;
    };
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    let r = Recipes::from_jar(names, |n| entries.iter().find(|(e, _)| e == n).map(|(_, b)| b.clone())).expect("recipes parse");
    Some(r)
}

const E: ItemStack = ItemStack::EMPTY;

fn s(item: u16, meta: i32) -> ItemStack {
    ItemStack::new(item, 1, meta)
}

#[test]
fn all_jar_recipes_parse() {
    let Some(entries) = common::recipe_entries() else {
        eprintln!("skipped: MCRS_JAR_DATA not set");
        return;
    };
    let mut shaped = 0;
    let mut shapeless = 0;
    for (name, bytes) in &entries {
        match parse_recipe(bytes) {
            Ok(Recipe::Shaped { .. }) => shaped += 1,
            Ok(Recipe::Shapeless { .. }) => shapeless += 1,
            Ok(_) => unreachable!(),
            Err(e) => panic!("{name}: {e}"),
        }
    }
    eprintln!("{} recipe files: {shaped} shaped, {shapeless} shapeless", entries.len());
    assert_eq!(entries.len(), 432);
    assert_eq!((shaped, shapeless), (342, 90));
    let r = recipes().unwrap();
    // 432 JSON recipes plus repairitem and shulkerboxcoloring
    assert_eq!(r.len(), 434);
    assert_eq!(r.get("minecraft:oak_planks").unwrap().id, 11 + r.iter().filter(|n| n.id >= 11 && n.name.as_str() < "oak_planks").count());
}

#[test]
fn every_recipe_matches_its_own_grid_first() {
    // Fill a 3x3 grid with the first stack of each ingredient; the first match in registry
    // order must be the recipe itself (so the name-order registration is safe), and it must
    // still match mirrored (shaped) or shuffled (shapeless).
    let Some(r) = recipes() else { return };
    let mut checked = 0;
    for nr in r.iter() {
        let mut g = CraftingGrid::new(3, 3);
        match &nr.recipe {
            Recipe::Shaped { width, height, items, .. } => {
                for y in 0..*height {
                    for x in 0..*width {
                        if let Some(st) = items[x + y * width].matching_stacks().first() {
                            let meta = if st.damage == WILDCARD { 0 } else { st.damage };
                            g.slots[x + y * 3] = ItemStack::new(st.item, 1, meta);
                        }
                    }
                }
                let mut m = CraftingGrid::new(3, 3);
                for y in 0..3 {
                    for x in 0..3 {
                        m.slots[(2 - x) + y * 3] = g.slots[x + y * 3];
                    }
                }
                assert!(nr.recipe.matches(&m), "{} mirrored", nr.name);
            }
            Recipe::Shapeless { items, .. } => {
                for (i, ing) in items.iter().enumerate() {
                    g.slots[8 - i] = ing.matching_stacks()[0];
                }
            }
            _ => continue,
        }
        let first = r.find_matching_recipe(&g).unwrap_or_else(|| panic!("{} matches nothing", nr.name));
        assert_eq!(first.name, nr.name);
        assert_eq!(r.find_matching_result(&g), nr.recipe.get_recipe_output());
        checked += 1;
    }
    eprintln!("{checked} recipes match their own grid first");
    assert_eq!(checked, 432);
}

#[test]
fn shaped_matching_and_mirroring() {
    let Some(r) = recipes() else { return };
    let plank = s(block::PLANKS, 2);
    let stick = s(item::STICK, 0);
    // wooden_axe: XX / X# / _#  in the top left of a 3x3 grid
    let axe = CraftingGrid::from_rows(3, &[&[plank, plank, E], &[plank, stick, E], &[E, stick, E]]);
    assert_eq!(r.find_matching_result(&axe), ItemStack::of(item::WOODEN_AXE));
    // mirrored: XX / #X / #_
    let mirrored = CraftingGrid::from_rows(3, &[&[plank, plank, E], &[stick, plank, E], &[stick, E, E]]);
    assert_eq!(r.find_matching_result(&mirrored), ItemStack::of(item::WOODEN_AXE));
    // shifted right by one
    let shifted = CraftingGrid::from_rows(3, &[&[E, plank, plank], &[E, plank, stick], &[E, E, stick]]);
    assert_eq!(r.find_matching_result(&shifted), ItemStack::of(item::WOODEN_AXE));
    // upside down is not a mirror
    let flipped = CraftingGrid::from_rows(3, &[&[E, stick, E], &[plank, stick, E], &[plank, plank, E]]);
    assert!(r.find_matching_result(&flipped).is_empty());
    // an extra item breaks the match
    let extra = CraftingGrid::from_rows(3, &[&[plank, plank, stick], &[plank, stick, E], &[E, stick, E]]);
    assert!(r.find_matching_result(&extra).is_empty());
    // oak planks: one log of data 0 anywhere, in a 2x2 grid too; birch log makes birch planks
    let mut g2 = CraftingGrid::new(2, 2);
    g2.slots[3] = s(block::LOG, 0);
    assert_eq!(r.find_matching_result(&g2), ItemStack::new(block::PLANKS, 4, 0));
    g2.slots[3] = s(block::LOG, 2);
    assert_eq!(r.find_matching_result(&g2), ItemStack::new(block::PLANKS, 4, 2));
    // crafting table in the 2x2 grid; mixed planks are fine
    let table = CraftingGrid::from_rows(2, &[&[s(block::PLANKS, 0), s(block::PLANKS, 5)], &[plank, plank]]);
    assert_eq!(r.find_matching_result(&table), ItemStack::of(block::CRAFTING_TABLE));
    // sticks: two planks stacked; torch needs coal or charcoal over a stick
    let torch = CraftingGrid::from_rows(2, &[&[E, s(item::COAL, 1)], &[E, stick]]);
    assert_eq!(r.find_matching_result(&torch), ItemStack::new(block::TORCH, 4, 0));
    // a 3-wide recipe cannot be made in the 2x2 grid
    let wheat = s(item::WHEAT, 0);
    let bread3 = CraftingGrid::from_rows(3, &[&[wheat, wheat, wheat]]);
    assert_eq!(r.find_matching_result(&bread3), ItemStack::of(item::BREAD));
    let bread2 = CraftingGrid::from_rows(2, &[&[wheat, wheat], &[E, E]]);
    assert!(r.find_matching_result(&bread2).is_empty());
    assert!(r.get("bread").unwrap().recipe.can_fit(3, 3) && !r.get("bread").unwrap().recipe.can_fit(2, 2));
}

#[test]
fn shapeless_and_remaining_items() {
    let Some(r) = recipes() else { return };
    // flint and steel in any slots
    let mut g = CraftingGrid::new(3, 3);
    g.slots[8] = s(item::IRON_INGOT, 0);
    g.slots[0] = s(item::FLINT, 0);
    assert_eq!(r.find_matching_result(&g), ItemStack::of(item::FLINT_AND_STEEL));
    g.slots[4] = s(item::FLINT, 0);
    assert!(r.find_matching_result(&g).is_empty(), "an extra flint");
    // bone meal: a bone (shapeless, dye result with data 15, count 3)
    let mut b = CraftingGrid::new(2, 2);
    b.slots[1] = s(item::BONE, 0);
    assert_eq!(r.find_matching_result(&b), ItemStack::new(item::DYE, 3, 15));
    // cake: the milk buckets leave empty buckets behind
    let milk = s(item::MILK_BUCKET, 0);
    let sugar = s(item::SUGAR, 0);
    let wheat = s(item::WHEAT, 0);
    let mut cake = CraftingGrid::from_rows(3, &[&[milk, milk, milk], &[sugar, s(item::EGG, 0), sugar], &[wheat, wheat, wheat]]);
    assert_eq!(r.find_matching_result(&cake), ItemStack::of(item::CAKE));
    let rem = r.get_remaining_items(&cake);
    assert_eq!(&rem[..3], &[ItemStack::of(item::BUCKET); 3]);
    assert!(rem[3..].iter().all(|x| x.is_empty()));
    // SlotCrafting.onTake: inputs used up, buckets left in the grid
    cake.slots[6].count = 5;
    let overflow = r.consume(&mut cake);
    assert!(overflow.is_empty());
    assert_eq!(&cake.slots[..3], &[ItemStack::of(item::BUCKET); 3]);
    assert_eq!(cake.slots[6].count, 4);
    assert!(cake.slots[4].is_empty());
    // no match: the grid comes back unchanged
    let junk = CraftingGrid::from_rows(2, &[&[s(item::DIAMOND, 0), s(item::BONE, 0)]]);
    assert_eq!(r.get_remaining_items(&junk), junk.slots);
}

#[test]
fn repair_and_shulker_coloring() {
    // RecipeRepairItem and ShulkerBoxColoring are code recipes: no jar needed
    let r = Recipes::code_recipes();
    let a = ItemStack::new(item::IRON_PICKAXE, 1, 200);
    let b = ItemStack::new(item::IRON_PICKAXE, 1, 100);
    let g = CraftingGrid::from_rows(2, &[&[a, E], &[E, b]]);
    // 250 - 200 + 250 - 100 + 250 * 5 / 100 = 212 durability left, damage 38
    assert_eq!(r.find_matching_result(&g), ItemStack::new(item::IRON_PICKAXE, 1, 38));
    assert_eq!(r.find_matching_recipe(&g).unwrap().name, "repairitem");
    let fresh = CraftingGrid::from_rows(2, &[&[ItemStack::of(item::WOODEN_SWORD), ItemStack::of(item::WOODEN_SWORD)]]);
    assert_eq!(r.find_matching_result(&fresh), ItemStack::new(item::WOODEN_SWORD, 1, 0), "clamped at 0");
    let mixed = CraftingGrid::from_rows(2, &[&[a, ItemStack::of(item::IRON_AXE)]]);
    assert!(r.find_matching_result(&mixed).is_empty());
    let three = CraftingGrid::from_rows(3, &[&[a, a, a]]);
    assert!(r.find_matching_result(&three).is_empty());
    let sticks = CraftingGrid::from_rows(2, &[&[s(item::STICK, 0), s(item::STICK, 0)]]);
    assert!(r.find_matching_result(&sticks).is_empty(), "not damageable");
    // purple shulker box + lime dye (dye damage 10) -> lime shulker box
    let g = CraftingGrid::from_rows(2, &[&[ItemStack::of(block::PURPLE_SHULKER_BOX), s(item::DYE, 10)]]);
    assert_eq!(r.find_matching_result(&g), ItemStack::of(block::LIME_SHULKER_BOX));
}

#[test]
fn json_forms_and_errors() {
    // shrink trims blank rows and columns
    let rows: Vec<Vec<char>> = ["   ", " # ", " # "].iter().map(|x| x.chars().collect()).collect();
    assert_eq!(shrink(&rows), vec![vec!['#'], vec!['#']]);
    assert_eq!(recipe_name("assets/minecraft/recipes/stick.json"), Some("stick"));
    assert_eq!(recipe_name("assets/minecraft/lang/en_us.lang"), None);
    let ok = br##"{"type":"crafting_shapeless","ingredients":[{"item":"minecraft:dye","data":4},[{"item":"stick"},{"item":"minecraft:planks","data":32767}]],"result":{"item":"minecraft:stone","data":3,"count":2}}"##;
    match parse_recipe(ok).unwrap() {
        Recipe::Shapeless { items, output, .. } => {
            assert_eq!(output, ItemStack::new(block::STONE, 2, 3));
            assert!(items[1].apply(&ItemStack::new(block::PLANKS, 1, 5)), "32767 is any metadata");
            assert!(items[1].apply(&ItemStack::of(item::STICK)));
            assert!(!items[0].apply(&ItemStack::new(item::DYE, 1, 3)));
            assert!(Ingredient::Empty.apply(&ItemStack::EMPTY));
        }
        other => panic!("{other:?}"),
    }
    let err = |j: &[u8]| parse_recipe(j).unwrap_err();
    assert_eq!(err(br##"{"type":"smelting"}"##), "Invalid or unsupported recipe type 'smelting'");
    assert_eq!(
        err(br##"{"type":"crafting_shapeless","ingredients":[{"item":"minecraft:dye"}],"result":{"item":"stone","data":0}}"##),
        "Missing data for item 'minecraft:dye'"
    );
    assert_eq!(
        err(br##"{"type":"crafting_shaped","pattern":["#"],"key":{"#":{"item":"minecraft:nope"}},"result":{"item":"stone","data":0}}"##),
        "Unknown item 'minecraft:nope'"
    );
    assert_eq!(
        err(br##"{"type":"crafting_shaped","pattern":["#X"],"key":{"#":{"item":"stick"}},"result":{"item":"stick"}}"##),
        "Pattern references symbol 'X' but it's not defined in the key"
    );
    assert_eq!(
        err(br##"{"type":"crafting_shaped","pattern":["#"],"key":{"#":{"item":"stick"},"Y":{"item":"stick"}},"result":{"item":"stick"}}"##),
        "Key defines symbols that aren't used in pattern: [Y]"
    );
    assert_eq!(
        err(br##"{"type":"crafting_shaped","pattern":["XXXX"],"key":{"X":{"item":"stick"}},"result":{"item":"stick"}}"##),
        "Invalid pattern: too many columns, 3 is maximum"
    );
}
