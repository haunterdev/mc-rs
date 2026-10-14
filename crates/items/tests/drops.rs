//! Block drops on a seeded `java.util.Random`. The expected sequences were worked out from the
//! Java methods with an independent `java.util.Random` (`new Random(42).nextInt()` is
//! -1170105035) stepping the same draws: quantity, the chance float, the item roll, then three
//! offset floats per spawned item.

use items::drops::{damage_dropped, drop_block_as_item, get_item_dropped, Drops};
use items::harvest::{double_plant_upper_harvested, harvest_block, player_break_block};
use items::ids::{block, item};
use items::{ItemStack, JavaRandom};

const fn st(id: u16, meta: u16) -> u16 {
    id << 4 | meta
}

fn one(s: ItemStack) -> (u16, i32, i32) {
    (s.item, s.count, s.damage)
}

fn stacks(d: &Drops) -> Vec<(u16, i32, i32)> {
    d.items.iter().map(|x| one(x.stack)).collect()
}

#[test]
fn simple_drops() {
    let mut r = JavaRandom::new(1);
    // BlockStone: stone drops cobblestone; granite drops granite (damageDropped = variant)
    let d = drop_block_as_item(st(block::STONE, 0), &mut r);
    assert_eq!(stacks(&d), vec![(block::COBBLESTONE, 1, 0)]);
    assert_eq!(d.items[0].offset, [0.3002365827560425, 0.4550403952598572, 0.4537198841571808]);
    assert_eq!(stacks(&drop_block_as_item(st(block::STONE, 1), &mut r)), vec![(block::STONE, 1, 1)]);
    // grass, mycelium, farmland and podzol drop dirt 0; coarse dirt keeps its metadata
    for s in [st(block::GRASS, 0), st(block::MYCELIUM, 0), st(block::FARMLAND, 7), st(block::DIRT, 2)] {
        assert_eq!(stacks(&drop_block_as_item(s, &mut r)), vec![(block::DIRT, 1, 0)], "state {s}");
    }
    assert_eq!(stacks(&drop_block_as_item(st(block::DIRT, 1), &mut r)), vec![(block::DIRT, 1, 1)]);
    // logs keep the wood type, log2 shifted down by 4; axis bits do not matter
    assert_eq!(stacks(&drop_block_as_item(st(block::LOG, 2 | 4), &mut r)), vec![(block::LOG, 1, 2)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::LOG2, 1 | 8), &mut r)), vec![(block::LOG2, 1, 1)]);
    // BlockSlab: a double slab drops two half slabs of its variant
    assert_eq!(stacks(&drop_block_as_item(st(block::DOUBLE_STONE_SLAB, 3), &mut r)), vec![(block::STONE_SLAB, 1, 3); 2]);
    assert_eq!(stacks(&drop_block_as_item(st(block::DOUBLE_WOODEN_SLAB, 5), &mut r)), vec![(block::WOODEN_SLAB, 1, 5); 2]);
    assert_eq!(stacks(&drop_block_as_item(st(block::STONE_SLAB, 8 | 1), &mut r)), vec![(block::STONE_SLAB, 1, 1)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::PURPUR_DOUBLE_SLAB, 0), &mut r)), vec![(block::PURPUR_SLAB, 1, 0); 2]);
    // things that drop something else
    assert_eq!(stacks(&drop_block_as_item(st(block::CLAY, 0), &mut r)), vec![(item::CLAY_BALL, 1, 0); 4]);
    assert_eq!(stacks(&drop_block_as_item(st(block::SNOW, 0), &mut r)), vec![(item::SNOWBALL, 1, 0); 4]);
    assert_eq!(stacks(&drop_block_as_item(st(block::BOOKSHELF, 0), &mut r)), vec![(item::BOOK, 1, 0); 3]);
    assert_eq!(stacks(&drop_block_as_item(st(block::ENDER_CHEST, 2), &mut r)), vec![(block::OBSIDIAN, 1, 0); 8]);
    assert_eq!(stacks(&drop_block_as_item(st(block::DIAMOND_ORE, 0), &mut r))[0], (item::DIAMOND, 1, 0));
    assert_eq!(stacks(&drop_block_as_item(st(block::IRON_ORE, 0), &mut r)), vec![(block::IRON_ORE, 1, 0)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::LIT_FURNACE, 3), &mut r)), vec![(block::FURNACE, 1, 0)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::QUARTZ_BLOCK, 4), &mut r)), vec![(block::QUARTZ_BLOCK, 1, 2)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::WOOL, 14), &mut r)), vec![(block::WOOL, 1, 14)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::RED_FLOWER, 8), &mut r)), vec![(block::RED_FLOWER, 1, 8)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::SAPLING, 8 | 5), &mut r)), vec![(block::SAPLING, 1, 5)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::OAK_DOOR, 0), &mut r)), vec![(item::OAK_DOOR, 1, 0)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::DARK_OAK_DOOR, 2), &mut r)), vec![(item::DARK_OAK_DOOR, 1, 0)]);
    assert_eq!(stacks(&drop_block_as_item(st(block::UNPOWERED_REPEATER, 0), &mut r)), vec![(item::REPEATER, 1, 0)]);
    // double plants: the lower half drops the plant, the upper half nothing
    assert_eq!(stacks(&drop_block_as_item(st(block::DOUBLE_PLANT, 4), &mut r)), vec![(block::DOUBLE_PLANT, 1, 4)]);
    assert!(drop_block_as_item(st(block::DOUBLE_PLANT, 8), &mut r).items.is_empty());
    assert!(drop_block_as_item(st(block::DOUBLE_PLANT, 3), &mut r).items.is_empty(), "large fern");
    // nothing at all
    for s in [
        st(block::GLASS, 0),
        st(block::STAINED_GLASS, 3),
        st(block::GLASS_PANE, 0),
        st(block::ICE, 0),
        st(block::PACKED_ICE, 0),
        st(block::VINE, 1),
        st(block::SNOW_LAYER, 0),
        st(block::BEDROCK, 0),
        st(block::WATER, 0),
        st(block::FIRE, 0),
        st(block::CAKE, 0),
        st(block::MOB_SPAWNER, 0),
        st(block::OAK_DOOR, 8),
        st(block::BED, 0),
    ] {
        assert!(drop_block_as_item(s, &mut r).items.is_empty(), "state {s}");
    }
    // the bed head without its tile entity drops a red bed
    assert_eq!(stacks(&drop_block_as_item(st(block::BED, 8), &mut r)), vec![(item::BED, 1, 14)]);
    // silverfish stone spawns a silverfish instead
    let d = drop_block_as_item(st(block::MONSTER_EGG, 2), &mut r);
    assert!(d.items.is_empty() && d.silverfish);
    // damageDropped
    assert_eq!(damage_dropped(st(block::ANVIL, 2 << 2 | 1)), 2);
    assert_eq!(damage_dropped(st(block::LEAVES2, 1 | 8)), 5);
    assert_eq!(damage_dropped(st(block::LAPIS_ORE, 0)), 4);
}

#[test]
fn gravel_flint_seeded() {
    let mut r = JavaRandom::new(12345);
    let got: Vec<u16> = (0..20).map(|_| drop_block_as_item(st(block::GRAVEL, 0), &mut r).items[0].stack.item).collect();
    let mut want = vec![block::GRAVEL; 20];
    for i in [0, 13, 16] {
        want[i] = item::FLINT;
    }
    assert_eq!(got, want);
}

#[test]
fn leaves_saplings_and_apples_seeded() {
    // 400 oak leaves: nextInt(20) for a sapling, then nextInt(200) for an apple
    let mut r = JavaRandom::new(777);
    let (mut saplings, mut apples) = (0, 0);
    for _ in 0..400 {
        for d in drop_block_as_item(st(block::LEAVES, 4), &mut r).items {
            match d.stack.item {
                block::SAPLING => {
                    assert_eq!(d.stack.damage, 0);
                    saplings += 1
                }
                item::APPLE => apples += 1,
                other => panic!("unexpected {other}"),
            }
        }
    }
    assert_eq!((saplings, apples), (29, 2));
    // jungle leaves use nextInt(40) and drop no apples
    let mut r = JavaRandom::new(777);
    let n: usize = (0..400).map(|_| drop_block_as_item(st(block::LEAVES, 3), &mut r).items.len()).sum();
    assert_eq!(n, 9);
    // dark oak (leaves2 variant 1) drops dark oak saplings (damage 5) and apples
    let mut r = JavaRandom::new(777);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..400 {
        for d in drop_block_as_item(st(block::LEAVES2, 1), &mut r).items {
            seen.insert((d.stack.item, d.stack.damage));
        }
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec![(block::SAPLING, 5), (item::APPLE, 0)]);
}

#[test]
fn counted_drops_seeded() {
    // tall grass: one roll of nextInt(8) per break; 2 seeds in 50 with seed 99
    let mut r = JavaRandom::new(99);
    let seeds: usize = (0..50).map(|_| drop_block_as_item(st(block::TALLGRASS, 1), &mut r).items.len()).sum();
    assert_eq!(seeds, 2);
    // glowstone: clamp(2 + nextInt(3) + nextInt(1), 1, 4)
    let mut r = JavaRandom::new(5);
    let g: Vec<usize> = (0..10).map(|_| drop_block_as_item(st(block::GLOWSTONE, 0), &mut r).items.len()).collect();
    assert_eq!(g, vec![4, 4, 3, 3, 3, 2, 2, 2, 3, 3]);
    // lapis ore: 4 + nextInt(5) blue dye, then 2 to 5 experience
    let mut r = JavaRandom::new(2024);
    let l: Vec<(usize, i32)> = (0..5)
        .map(|_| {
            let d = drop_block_as_item(st(block::LAPIS_ORE, 0), &mut r);
            assert!(d.items.iter().all(|x| one(x.stack) == (item::DYE, 1, 4)));
            (d.items.len(), d.xp)
        })
        .collect();
    assert_eq!(l, vec![(4, 2), (7, 2), (6, 2), (8, 3), (6, 4)]);
    // coal ore: one coal, 0 to 2 experience
    let mut r = JavaRandom::new(31);
    let xp: Vec<i32> = (0..8).map(|_| drop_block_as_item(st(block::COAL_ORE, 0), &mut r).xp).collect();
    assert_eq!(xp, vec![0, 0, 0, 1, 2, 2, 2, 0]);
    // redstone ore: 4 + nextInt(2) + nextInt(1) dust, 1 to 5 experience
    let mut r = JavaRandom::new(31);
    let rs: Vec<(usize, i32)> = (0..4)
        .map(|_| {
            let d = drop_block_as_item(st(block::LIT_REDSTONE_ORE, 0), &mut r);
            (d.items.len(), d.xp)
        })
        .collect();
    assert_eq!(rs, vec![(5, 5), (4, 4), (5, 1), (5, 5)]);
    // melon: min(9, 3 + nextInt(5) + nextInt(1)) slices
    let mut r = JavaRandom::new(8);
    let m: Vec<usize> = (0..6).map(|_| drop_block_as_item(st(block::MELON_BLOCK, 0), &mut r).items.len()).collect();
    assert_eq!(m, vec![7, 5, 7, 6, 5, 6]);
    // grown wheat: one wheat, then 3 rolls of nextInt(14) <= 7 for seeds
    let mut r = JavaRandom::new(4);
    for want_seeds in [2, 2, 2] {
        let d = drop_block_as_item(st(block::WHEAT, 7), &mut r);
        assert_eq!(one(d.items[0].stack), (item::WHEAT, 1, 0));
        assert_eq!(d.items[1..].iter().filter(|x| x.stack.item == item::WHEAT_SEEDS).count(), want_seeds);
    }
    // unripe wheat drops one seed
    assert_eq!(stacks(&drop_block_as_item(st(block::WHEAT, 3), &mut r)), vec![(item::WHEAT_SEEDS, 1, 0)]);
    // mob spawner: no item, 15 + nextInt(15) + nextInt(15) experience
    let mut r = JavaRandom::new(3);
    assert_eq!(drop_block_as_item(st(block::MOB_SPAWNER, 0), &mut r).xp, 34);
    // cocoa: 3 brown dye (dye damage 3) when grown
    let mut r = JavaRandom::new(3);
    assert_eq!(stacks(&drop_block_as_item(st(block::COCOA, 2 << 2), &mut r)), vec![(item::DYE, 1, 3); 3]);
    // the item roll draws: tall grass and gravel consume a nextInt
    let mut a = JavaRandom::new(10);
    let mut b = JavaRandom::new(10);
    get_item_dropped(st(block::GRAVEL, 0), &mut a, 0);
    b.next_int_bounded(10);
    assert_eq!(a.next_int(), b.next_int());
}

#[test]
fn player_harvest() {
    let mut r = JavaRandom::new(7);
    // a pickaxe takes 1 durability and stone drops cobblestone
    let mut pick = ItemStack::new(item::IRON_PICKAXE, 1, 0);
    let h = player_break_block(st(block::STONE, 0), &mut pick, &mut r);
    assert_eq!(stacks(&h.drops), vec![(block::COBBLESTONE, 1, 0)]);
    assert_eq!(pick.damage, 1);
    assert_eq!(h.exhaustion, 0.005);
    // a sword takes 2 and cannot harvest rock: no drop, no harvestBlock
    let mut sword = ItemStack::new(item::IRON_SWORD, 1, 0);
    let h = player_break_block(st(block::STONE, 0), &mut sword, &mut r);
    assert!(!h.harvested && h.drops.items.is_empty());
    assert_eq!(sword.damage, 2);
    // a wooden pickaxe on iron ore drops nothing
    let mut wp = ItemStack::of(item::WOODEN_PICKAXE);
    assert!(player_break_block(st(block::IRON_ORE, 0), &mut wp, &mut r).drops.items.is_empty());
    // the last use of a tool still harvests (Java passes the pre-damage copy)
    let mut worn = ItemStack::new(item::STONE_PICKAXE, 1, 131);
    let h = player_break_block(st(block::COAL_ORE, 0), &mut worn, &mut r);
    assert!(h.tool_broke && worn.is_empty());
    assert_eq!(h.drops.items[0].stack.item, item::COAL);
    // snow layer: only a shovel harvests it, layers + 1 snowballs, no exhaustion
    let mut hand = ItemStack::EMPTY;
    assert!(player_break_block(st(block::SNOW_LAYER, 0), &mut hand, &mut r).drops.items.is_empty());
    let mut shovel = ItemStack::of(item::WOODEN_SHOVEL);
    let h = player_break_block(st(block::SNOW_LAYER, 2), &mut shovel, &mut r);
    assert_eq!(stacks(&h.drops), vec![(item::SNOWBALL, 4, 0)]);
    assert_eq!(h.exhaustion, 0.0);
    // shears take the plant itself
    let shears = ItemStack::of(item::SHEARS);
    assert_eq!(stacks(&harvest_block(st(block::LEAVES, 2), &shears, &mut r).drops), vec![(block::LEAVES, 1, 2)]);
    assert_eq!(stacks(&harvest_block(st(block::LEAVES2, 1 | 4), &shears, &mut r).drops), vec![(block::LEAVES2, 1, 1)]);
    assert_eq!(stacks(&harvest_block(st(block::TALLGRASS, 2), &shears, &mut r).drops), vec![(block::TALLGRASS, 1, 2)]);
    assert_eq!(stacks(&harvest_block(st(block::VINE, 4), &shears, &mut r).drops), vec![(block::VINE, 1, 0)]);
    assert_eq!(stacks(&harvest_block(st(block::DEADBUSH, 0), &shears, &mut r).drops), vec![(block::DEADBUSH, 1, 0)]);
    assert_eq!(stacks(&harvest_block(st(block::WEB, 0), &shears, &mut r).drops), vec![(block::WEB, 1, 0)]);
    assert_eq!(stacks(&harvest_block(st(block::DOUBLE_PLANT, 2), &shears, &mut r).drops), vec![(block::TALLGRASS, 2, 1)]);
    // web by sword: string
    let swd = ItemStack::of(item::STONE_SWORD);
    assert_eq!(stacks(&harvest_block(st(block::WEB, 0), &swd, &mut r).drops), vec![(item::STRING, 1, 0)]);
    // ice: nothing, then melts
    let h = harvest_block(st(block::ICE, 0), &ItemStack::of(item::IRON_PICKAXE), &mut r);
    assert!(h.drops.items.is_empty() && h.ice_melts);
    // breaking the top of a sunflower destroys the bottom, which drops the sunflower
    let d = double_plant_upper_harvested(st(block::DOUBLE_PLANT, 0), &ItemStack::EMPTY, &mut r);
    assert_eq!(stacks(&d), vec![(block::DOUBLE_PLANT, 1, 0)]);
    let d = double_plant_upper_harvested(st(block::DOUBLE_PLANT, 3), &shears, &mut r);
    assert_eq!(stacks(&d), vec![(block::TALLGRASS, 2, 2)]);
}
