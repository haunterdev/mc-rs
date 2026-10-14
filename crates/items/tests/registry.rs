//! The item registry, `ItemStack` comparisons and the `InventoryPlayer` stacking rules.

use items::ids::{block, item};
use items::registry::{self, block_from_item, by_name, item_from_block};
use items::stack::{are_item_stacks_equal, are_items_equal, are_items_equal_ignore_durability};
use items::tools::{attack_modifiers, ArmorMaterial, ArmorSlot, ToolMaterial};
use items::{ItemStack, Kind, PlayerInventory};

#[test]
fn registry_ids_names_and_limits() {
    // 1.12.2 registers 202 block items and 209 numbered items.
    assert_eq!(registry::all_items().len(), 411);
    let pick = registry::def(item::DIAMOND_PICKAXE);
    assert_eq!((pick.id, pick.name), (278, "diamond_pickaxe"));
    assert_eq!((pick.max_stack, pick.max_damage), (1, 1561));
    assert!(pick.is_damageable());
    assert_eq!(registry::def(item::GOLDEN_SWORD).max_damage, 32);
    assert_eq!(registry::def(item::WOODEN_HOE).max_damage, 59);
    assert_eq!(registry::def(item::SHEARS).max_damage, 238);
    assert_eq!(registry::def(item::BOW).max_damage, 384);
    // ItemArmor: MAX_DAMAGE_ARRAY[slot] * factor
    assert_eq!(registry::def(item::DIAMOND_CHESTPLATE).max_damage, 16 * 33);
    assert_eq!(registry::def(item::LEATHER_BOOTS).max_damage, 13 * 5);
    assert_eq!(ArmorMaterial::Iron.durability(ArmorSlot::Head), 165);
    assert_eq!(registry::def(item::BUCKET).max_stack, 16);
    assert_eq!(registry::def(item::WATER_BUCKET).max_stack, 1);
    assert_eq!(registry::def(item::WATER_BUCKET).container, item::BUCKET);
    assert_eq!(registry::def(item::DRAGON_BREATH).container, item::GLASS_BOTTLE);
    assert_eq!(registry::def(item::ENDER_PEARL).max_stack, 16);
    assert_eq!(registry::def(item::SIGN).max_stack, 16);
    assert_eq!(registry::def(item::WRITTEN_BOOK).max_stack, 16);
    assert_eq!(registry::def(item::CAKE).max_stack, 1);
    assert_eq!(registry::def(block::WHITE_SHULKER_BOX).max_stack, 1);
    assert_eq!(registry::def(item::RECORD_WAIT).id, 2267);
    assert!(registry::item(451).is_none(), "451 is unused in 1.12.2");
    assert!(registry::def(item::COAL).has_subtypes && registry::def(item::DYE).has_subtypes);
    assert!(registry::def(block::STONE).has_subtypes && !registry::def(block::GRASS).has_subtypes);
    assert!(registry::def(block::TALLGRASS).has_subtypes && !registry::def(block::VINE).has_subtypes);
    // names
    assert_eq!(by_name("minecraft:stick").unwrap().id, item::STICK);
    assert_eq!(by_name("STONE").unwrap().id, 1);
    assert_eq!(by_name("wooden_door").unwrap().id, item::OAK_DOOR);
    assert!(by_name("other:stick").is_none());
    assert_eq!(registry::by_name_or_id("280").unwrap().id, item::STICK);
    // getItemFromBlock / getBlockFromItem
    assert_eq!(item_from_block(block::STONE), 1);
    assert_eq!(item_from_block(block::OAK_DOOR), item::AIR, "doors have ItemDoor, not an ItemBlock");
    assert_eq!(item_from_block(block::LIT_FURNACE), item::AIR);
    assert_eq!(block_from_item(item::BANNER), block::STANDING_BANNER);
    assert_eq!(block_from_item(item::REEDS), block::AIR, "ItemBlockSpecial is not an ItemBlock");
    assert!(matches!(registry::def(item::STONE_PICKAXE).kind, Kind::Tool(_, ToolMaterial::Stone)));
    // Item.getMetadata(damage): the placed block metadata
    assert_eq!(registry::def(block::LEAVES).metadata(2), 6);
    assert_eq!(registry::def(block::PISTON).metadata(3), 7);
    assert_eq!(registry::def(block::ANVIL).metadata(2), 8);
    assert_eq!(registry::def(block::COBBLESTONE).metadata(5), 0);
}

#[test]
fn attack_values() {
    // ItemSword: 3 + material damage; ItemAxe arrays; ItemTool attackDamageIn + material
    assert_eq!(attack_modifiers(item::DIAMOND_SWORD), Some((6.0, -2.4000000953674316)));
    assert_eq!(attack_modifiers(item::STONE_AXE), Some((8.0, -3.200000047683716)));
    assert_eq!(attack_modifiers(item::IRON_PICKAXE), Some((3.0, -2.799999952316284)));
    assert_eq!(attack_modifiers(item::IRON_SHOVEL), Some((3.5, -3.0)));
    assert_eq!(attack_modifiers(item::STICK), None);
}

#[test]
fn stack_comparisons() {
    let a = ItemStack::new(block::PLANKS, 3, 1);
    let b = ItemStack::new(block::PLANKS, 5, 1);
    let c = ItemStack::new(block::PLANKS, 3, 2);
    assert!(are_items_equal(&a, &b));
    assert!(!are_items_equal(&a, &c));
    assert!(!are_item_stacks_equal(&a, &b), "counts differ");
    assert!(are_item_stacks_equal(&a, &a.copy()));
    assert!(are_items_equal(&ItemStack::EMPTY, &ItemStack::new(item::AIR, 5, 0)));
    // a damaged pickaxe is the same item ignoring durability, not otherwise
    let p1 = ItemStack::new(item::IRON_PICKAXE, 1, 10);
    let p2 = ItemStack::new(item::IRON_PICKAXE, 1, 200);
    assert!(!are_items_equal(&p1, &p2));
    assert!(are_items_equal_ignore_durability(&p1, &p2));
    // emptiness
    assert!(ItemStack::new(item::STICK, 0, 0).is_empty());
    assert!(ItemStack::new(451, 1, 0).is_empty());
    assert_eq!(ItemStack::new(item::STICK, 1, -5).damage, 0);
    // stackable: max stack > 1 and not a damaged damageable
    assert!(!p1.is_stackable());
    assert!(ItemStack::of(item::STICK).is_stackable());
    assert!(!ItemStack::of(item::WATER_BUCKET).is_stackable());
    let mut s = ItemStack::new(item::STICK, 10, 0);
    let half = s.split_stack(4);
    assert_eq!((half.count, s.count), (4, 6));
}

#[test]
fn damage_and_breaking() {
    let mut p = ItemStack::new(item::WOODEN_PICKAXE, 1, 58);
    assert!(!p.damage_item(1, false), "59 uses: damage 59 is the last use");
    assert_eq!(p.damage, 59);
    assert!(p.damage_item(1, false), "damage past max breaks it");
    assert!(p.is_empty());
    let mut q = ItemStack::new(item::WOODEN_PICKAXE, 1, 0);
    assert!(!q.damage_item(5, true) && q.damage == 0, "creative takes no damage");
    let mut stick = ItemStack::of(item::STICK);
    assert!(!stick.damage_item(1, false) && stick.damage == 0);
}

#[test]
fn inventory_stacking_rules() {
    let mut inv = PlayerInventory::new();
    // 100 cobblestone: 64 in slot 0, 36 in slot 1
    let mut s = ItemStack::new(block::COBBLESTONE, 100, 0);
    assert!(inv.add_item_stack_to_inventory(&mut s, false));
    assert!(s.is_empty());
    assert_eq!(inv.main[0].count, 64);
    assert_eq!(inv.main[1].count, 36);
    // storeItemStack prefers the selected slot, then the off hand, then the first match
    inv.current_item = 1;
    assert_eq!(inv.store_item_stack(&ItemStack::of(block::COBBLESTONE)), 1);
    inv.current_item = 0;
    assert_eq!(inv.store_item_stack(&ItemStack::of(block::COBBLESTONE)), 1, "slot 0 is full");
    inv.off_hand[0] = ItemStack::new(block::COBBLESTONE, 10, 0);
    assert_eq!(inv.store_item_stack(&ItemStack::of(block::COBBLESTONE)), 40);
    // subtypes must match metadata; non-subtype items ignore it
    let mut granite = ItemStack::new(block::STONE, 5, 1);
    inv.add_item_stack_to_inventory(&mut granite, false);
    assert_eq!(inv.main[2], ItemStack::new(block::STONE, 5, 1));
    let mut stone = ItemStack::new(block::STONE, 5, 0);
    inv.add_item_stack_to_inventory(&mut stone, false);
    assert_eq!(inv.main[3], ItemStack::new(block::STONE, 5, 0));
    // canMergeStacks: same item, room left
    assert!(inv.can_merge_stacks(&ItemStack::new(item::STICK, 3, 0), &ItemStack::of(item::STICK)));
    assert!(!inv.can_merge_stacks(&ItemStack::new(item::STICK, 64, 0), &ItemStack::of(item::STICK)));
    assert!(!inv.can_merge_stacks(&ItemStack::new(item::ENDER_PEARL, 16, 0), &ItemStack::of(item::ENDER_PEARL)));
    assert!(!inv.can_merge_stacks(&ItemStack::new(block::STONE, 3, 0), &ItemStack::new(block::STONE, 1, 1)));
    // damaged tools go whole into the first empty slot, never merged
    let mut t = ItemStack::new(item::IRON_SWORD, 1, 7);
    assert!(inv.add_item_stack_to_inventory(&mut t, false));
    assert_eq!(inv.main[4], ItemStack::new(item::IRON_SWORD, 1, 7));
    // a full inventory takes nothing, except in creative where the stack is used up
    let mut full = PlayerInventory::new();
    for i in 0..36 {
        full.main[i] = ItemStack::new(item::STICK, 64, 0);
    }
    let mut d = ItemStack::new(item::DIAMOND, 3, 0);
    assert!(!full.add_item_stack_to_inventory(&mut d, false));
    assert_eq!(d.count, 3);
    assert!(full.add_item_stack_to_inventory(&mut d, true));
    assert!(d.is_empty());
    // clearMatchingItems
    let mut inv2 = PlayerInventory::new();
    inv2.main[5] = ItemStack::new(item::STICK, 10, 0);
    inv2.main[9] = ItemStack::new(item::STICK, 10, 0);
    assert_eq!(inv2.clear_matching_items(Some(item::STICK), -1, 15), 15);
    assert!(inv2.main[5].is_empty());
    assert_eq!(inv2.main[9].count, 5);
    // the mouse wheel
    inv2.current_item = 0;
    inv2.change_current_item(1);
    assert_eq!(inv2.current_item, 8);
    inv2.change_current_item(-3);
    assert_eq!(inv2.current_item, 0);
}
