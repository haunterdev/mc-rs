//! Smelting, burn times, the furnace tick, food values and translation keys (checked against
//! the jar's `en_us.lang` when `MCRS_JAR_DATA` is set).

mod common;

use items::food::{eat, food, heal_amount, on_food_eaten, saturation_modifier, Effect};
use items::ids::{block, item};
use items::names::{display_name, name_key, parse_lang, translation_key};
use items::registry;
use items::smelting::{furnace_output_xp, get_item_burn_time, get_smelting_experience, get_smelting_result, xp_split};
use items::{Furnace, ItemStack, JavaRandom};

fn s(item: u16, meta: i32) -> ItemStack {
    ItemStack::new(item, 1, meta)
}

#[test]
fn smelting_results() {
    assert_eq!(get_smelting_result(&s(block::IRON_ORE, 0)), ItemStack::of(item::IRON_INGOT));
    assert_eq!(get_smelting_result(&s(block::LOG2, 1)), s(item::COAL, 1), "any log makes charcoal");
    assert_eq!(get_smelting_result(&s(block::STONEBRICK, 0)), s(block::STONEBRICK, 2));
    assert!(get_smelting_result(&s(block::STONEBRICK, 1)).is_empty(), "only plain stone bricks crack");
    assert_eq!(get_smelting_result(&s(block::SPONGE, 1)), s(block::SPONGE, 0));
    assert!(get_smelting_result(&s(block::SPONGE, 0)).is_empty());
    assert_eq!(get_smelting_result(&s(item::FISH, 1)), s(item::COOKED_FISH, 1));
    assert!(get_smelting_result(&s(item::FISH, 3)).is_empty(), "pufferfish does not cook");
    assert_eq!(get_smelting_result(&s(block::CACTUS, 0)), s(item::DYE, 2));
    assert_eq!(get_smelting_result(&s(block::STAINED_HARDENED_CLAY, 11)), ItemStack::of(block::BLUE_GLAZED_TERRACOTTA));
    assert_eq!(get_smelting_result(&ItemStack::new(item::IRON_SWORD, 1, 100)), ItemStack::of(item::IRON_NUGGET));
    assert!(get_smelting_result(&s(item::STICK, 0)).is_empty());
    assert_eq!(get_smelting_experience(&ItemStack::of(item::GOLD_INGOT)), 1.0);
    assert_eq!(get_smelting_experience(&s(item::COAL, 1)), 0.15);
    assert_eq!(get_smelting_experience(&s(item::COAL, 0)), 0.1);
    assert_eq!(get_smelting_experience(&ItemStack::of(block::GLASS)), 0.1);
    assert_eq!(get_smelting_experience(&ItemStack::of(item::STICK)), 0.0);
    // SlotFurnaceOutput: 10 glass at 0.1 is exactly 1; 3 iron at 0.7 is 2.1, 3 with a lucky roll
    assert_eq!(furnace_output_xp(&ItemStack::of(block::GLASS), 10, || 0.99), 1);
    assert_eq!(furnace_output_xp(&ItemStack::of(item::IRON_INGOT), 3, || 0.05), 3);
    assert_eq!(furnace_output_xp(&ItemStack::of(item::IRON_INGOT), 3, || 0.5), 2);
    assert_eq!(furnace_output_xp(&ItemStack::of(item::DIAMOND), 5, || 0.0), 5, "1.0 or more pays the count");
    assert_eq!((xp_split(40), xp_split(6), xp_split(2)), (37, 3, 1));
}

#[test]
fn burn_times() {
    let bt = |it: u16, meta: i32| get_item_burn_time(&s(it, meta));
    assert_eq!(bt(item::COAL, 0), 1600);
    assert_eq!(bt(item::COAL, 1), 1600);
    assert_eq!(bt(block::COAL_BLOCK, 0), 16000);
    assert_eq!(bt(item::LAVA_BUCKET, 0), 20000);
    assert_eq!(bt(item::BLAZE_ROD, 0), 2400);
    assert_eq!(bt(block::PLANKS, 3), 300);
    assert_eq!(bt(block::LOG, 0), 300);
    assert_eq!(bt(block::CRAFTING_TABLE, 0), 300);
    assert_eq!(bt(block::CHEST, 0), 300);
    assert_eq!(bt(item::BANNER, 0), 300, "an ItemBlock of a WOOD block");
    assert_eq!(bt(block::WOODEN_SLAB, 0), 150);
    assert_eq!(bt(block::WOOL, 4), 100);
    assert_eq!(bt(block::CARPET, 0), 67);
    assert_eq!(bt(block::LADDER, 0), 300);
    assert_eq!(bt(block::WOODEN_BUTTON, 0), 100);
    assert_eq!(bt(block::SAPLING, 5), 100);
    assert_eq!(bt(item::STICK, 0), 100);
    assert_eq!(bt(item::BOWL, 0), 100);
    assert_eq!(bt(item::WOODEN_PICKAXE, 30), 200);
    assert_eq!(bt(item::WOODEN_SWORD, 0), 200);
    assert_eq!(bt(item::WOODEN_HOE, 0), 200);
    assert_eq!(bt(item::BOW, 0), 300);
    assert_eq!(bt(item::FISHING_ROD, 0), 300);
    assert_eq!(bt(item::SIGN, 0), 200);
    assert_eq!(bt(item::OAK_DOOR, 0), 200);
    assert_eq!(bt(item::IRON_DOOR, 0), 0);
    assert_eq!(bt(item::BIRCH_BOAT, 0), 400);
    assert_eq!(bt(item::STONE_PICKAXE, 0), 0);
    assert_eq!(bt(block::COBBLESTONE, 0), 0);
    assert_eq!(bt(item::BED, 0), 0, "ItemBed is not an ItemBlock");
    assert_eq!(get_item_burn_time(&ItemStack::EMPTY), 0);
}

#[test]
fn furnace_ticks() {
    let mut f = Furnace::default();
    f.set_inventory_slot_contents(0, ItemStack::new(block::IRON_ORE, 2, 0));
    f.set_inventory_slot_contents(1, ItemStack::new(item::COAL, 1, 0));
    assert_eq!(f.total_cook_time, 200);
    let t = f.update();
    assert!(t.lit_changed && f.is_burning());
    assert!(f.items[1].is_empty(), "the coal is used up");
    assert_eq!((f.burn_time, f.current_item_burn_time, f.cook_time), (1600, 1600, 1));
    for _ in 0..199 {
        f.update();
    }
    assert_eq!(f.items[2], ItemStack::of(item::IRON_INGOT));
    assert_eq!(f.items[0].count, 1);
    for _ in 0..200 {
        f.update();
    }
    assert_eq!(f.items[2].count, 2);
    assert!(f.items[0].is_empty());
    // a lava bucket leaves its bucket behind; wet sponge fills an empty bucket in the fuel slot
    let mut g = Furnace::default();
    g.set_inventory_slot_contents(0, s(block::SPONGE, 1));
    g.set_inventory_slot_contents(1, s(item::LAVA_BUCKET, 0));
    g.update();
    assert_eq!(g.items[1], ItemStack::of(item::BUCKET));
    for _ in 0..199 {
        g.update();
    }
    assert_eq!(g.items[2], s(block::SPONGE, 0));
    assert_eq!(g.items[1], ItemStack::of(item::WATER_BUCKET));
    assert!(g.is_item_valid_for_slot(1, &ItemStack::of(item::BUCKET)));
    g.items[1] = ItemStack::of(item::BUCKET);
    assert!(!g.is_item_valid_for_slot(1, &ItemStack::of(item::BUCKET)), "one empty bucket at a time");
    assert!(!g.is_item_valid_for_slot(2, &ItemStack::of(item::COAL)));
}

#[test]
fn food_values() {
    let f = food(item::COOKED_BEEF).unwrap();
    assert_eq!((f.heal, f.saturation, f.wolf_meat), (8, 0.8, true));
    assert!(food(item::GOLDEN_APPLE).unwrap().always_edible);
    assert!(food(item::CHORUS_FRUIT).unwrap().always_edible);
    assert_eq!(food(item::MUSHROOM_STEW).unwrap().saturation, 0.6);
    assert_eq!(food(item::RABBIT_STEW).unwrap().heal, 10);
    assert!(food(item::STICK).is_none());
    assert_eq!(heal_amount(&s(item::COOKED_FISH, 1)), Some(6));
    assert_eq!(saturation_modifier(&s(item::COOKED_FISH, 1)), Some(0.8));
    assert_eq!(heal_amount(&s(item::FISH, 3)), Some(1));
    assert_eq!(heal_amount(&s(item::COOKED_FISH, 9)), Some(5), "unknown fish metadata is cod");
    let (e, p) = food(item::ROTTEN_FLESH).unwrap().effect.unwrap();
    assert_eq!((e.effect, e.duration, e.amplifier, p), (Effect::Hunger, 600, 0, 0.8));
    // onFoodEaten: a raw chicken rolls nextFloat() < 0.3
    let mut r = JavaRandom::new(42);
    let mut probe = JavaRandom::new(42);
    let hit = probe.next_float() < 0.3;
    assert_eq!(!on_food_eaten(&ItemStack::of(item::CHICKEN), &mut r).is_empty(), hit);
    // a pufferfish always poisons, draws nothing
    let mut r = JavaRandom::new(1);
    let fx = on_food_eaten(&s(item::FISH, 3), &mut r);
    assert_eq!(fx.len(), 3);
    assert_eq!(r.next_int(), JavaRandom::new(1).next_int());
    // enchanted golden apple
    assert_eq!(on_food_eaten(&s(item::GOLDEN_APPLE, 1), &mut r).len(), 4);
    // soup leaves a bowl, bread one less
    assert_eq!(eat(&ItemStack::of(item::BEETROOT_SOUP)), ItemStack::of(item::BOWL));
    assert_eq!(eat(&ItemStack::new(item::BREAD, 3, 0)), ItemStack::new(item::BREAD, 2, 0));
    assert!(eat(&ItemStack::of(item::BREAD)).is_empty());
}

#[test]
fn translation_keys() {
    let k = |it: u16, meta: i32| translation_key(&s(it, meta));
    assert_eq!(k(block::STONE, 0), "tile.stone.stone");
    assert_eq!(k(block::STONE, 2), "tile.stone.graniteSmooth");
    assert_eq!(k(block::STONE, 99), "tile.stone.stone");
    assert_eq!(k(block::COBBLESTONE, 0), "tile.stonebrick");
    assert_eq!(k(block::PLANKS, 5), "tile.wood.big_oak");
    assert_eq!(k(block::LOG2, 1), "tile.log.big_oak");
    assert_eq!(k(block::LOG2, 2), "tile.log.oak", "byMetadata(6) falls back to oak");
    assert_eq!(k(block::LEAVES2, 0), "tile.leaves.acacia");
    assert_eq!(k(block::SPONGE, 1), "tile.sponge.wet");
    assert_eq!(k(block::WOOL, 3), "tile.cloth.lightBlue");
    assert_eq!(k(block::TALLGRASS, 2), "tile.tallgrass.fern");
    assert_eq!(k(block::TALLGRASS, 5), "tile.tallgrass");
    assert_eq!(k(block::STONE_SLAB, 3), "tile.stoneSlab.cobble");
    assert_eq!(k(block::WOODEN_SLAB, 2), "tile.woodSlab.birch");
    assert_eq!(k(block::PURPUR_SLAB, 0), "tile.purpurSlab");
    assert_eq!(k(block::RED_FLOWER, 4), "tile.flower2.tulipRed");
    assert_eq!(k(block::DOUBLE_PLANT, 0), "tile.doublePlant.sunflower");
    assert_eq!(k(block::GRAY_GLAZED_TERRACOTTA, 0), "tile.glazedTerracottaGray");
    assert_eq!(k(item::COAL, 1), "item.charcoal");
    assert_eq!(k(item::DYE, 0), "item.dyePowder.black");
    assert_eq!(k(item::DYE, 4), "item.dyePowder.blue");
    assert_eq!(k(item::FISH, 2), "item.fish.clownfish.raw");
    assert_eq!(k(item::COOKED_FISH, 1), "item.fish.salmon.cooked");
    assert_eq!(k(item::SKULL, 4), "item.skull.creeper");
    assert_eq!(k(item::BED, 14), "item.bed.red");
    assert_eq!(k(item::IRON_SHOVEL, 0), "item.shovelIron");
    assert_eq!(k(item::BOAT, 0), "item.boat.oak");
    assert_eq!(k(item::BANNER, 0), "tile.banner");
    assert_eq!(name_key(&s(item::BANNER, 0)), "item.banner.black.name");
    assert_eq!(name_key(&s(item::POTIONITEM, 0)), "potion.effect.empty");
}

#[test]
fn every_item_names_in_the_jar_lang() {
    let Some(bytes) = common::lang() else {
        eprintln!("skipped: MCRS_JAR_DATA not set");
        return;
    };
    let lang = parse_lang(&bytes);
    assert_eq!(display_name(&s(block::STONE, 1), &lang), "Granite");
    assert_eq!(display_name(&s(item::DIAMOND_PICKAXE, 0), &lang), "Diamond Pickaxe");
    assert_eq!(display_name(&s(item::DYE, 15), &lang), "Bone Meal");
    // Every registered item's base subtypes resolve to a lang entry. Metadata 0 to 15 for
    // subtype items (only the subtypes Java lists in getSubItems need names; the rest fall
    // back to metadata 0's name through byMetadata).
    let mut missing = Vec::new();
    let mut n = 0;
    for d in registry::all_items() {
        if d.id == item::AIR {
            continue;
        }
        let metas: &[i32] = if d.has_subtypes { &[0, 1, 2, 3, 4, 5] } else { &[0] };
        for &m in metas {
            let key = name_key(&s(d.id, m));
            n += 1;
            if !lang.contains_key(&key) {
                missing.push(format!("{} {}:{m} {key}", d.name, d.id));
            }
        }
    }
    eprintln!("{n} item names looked up, {} missing", missing.len());
    assert!(missing.is_empty(), "{missing:#?}");
}
