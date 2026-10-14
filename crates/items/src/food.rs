//! Food as data: `ItemFood` (heal amount, saturation modifier, wolf meat, always edible, the
//! `setPotionEffect` chance), `ItemSoup`, `ItemAppleGold`, `ItemChorusFruit`, `ItemSeedFood` and
//! `ItemFishFood.FishType`, with what `onItemUseFinish`/`onFoodEaten` do to the stack and which
//! potion effects they roll. `FoodStats.addStats(heal, saturation)` is the eater's side:
//! `foodLevel = min(heal + foodLevel, 20)`, `saturation = min(saturation + heal * modifier *
//! 2, foodLevel)`.

use worldgen::java_random::JavaRandom;

use crate::ids::item;
use crate::stack::ItemStack;

/// A `Potion.REGISTRY` entry that food can give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    Nausea,
    Regeneration,
    Resistance,
    FireResistance,
    Hunger,
    Poison,
    Absorption,
}

impl Effect {
    /// `Potion.getIdFromPotion`
    pub const fn id(self) -> i32 {
        match self {
            Effect::Nausea => 9,
            Effect::Regeneration => 10,
            Effect::Resistance => 11,
            Effect::FireResistance => 12,
            Effect::Hunger => 17,
            Effect::Poison => 19,
            Effect::Absorption => 22,
        }
    }
    /// The registry name (`minecraft:<name>`).
    pub const fn name(self) -> &'static str {
        match self {
            Effect::Nausea => "nausea",
            Effect::Regeneration => "regeneration",
            Effect::Resistance => "resistance",
            Effect::FireResistance => "fire_resistance",
            Effect::Hunger => "hunger",
            Effect::Poison => "poison",
            Effect::Absorption => "absorption",
        }
    }
}

/// `new PotionEffect(potion, duration, amplifier)`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PotionEffect {
    pub effect: Effect,
    /// ticks
    pub duration: i32,
    pub amplifier: i32,
}

const fn pe(effect: Effect, duration: i32, amplifier: i32) -> PotionEffect {
    PotionEffect { effect, duration, amplifier }
}

/// One `ItemFood` as constructed in `Item.registerItems`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Food {
    /// `healAmount`
    pub heal: i32,
    /// `saturationModifier`
    pub saturation: f32,
    /// `isWolfsFavoriteMeat`
    pub wolf_meat: bool,
    /// `alwaysEdible` (`setAlwaysEdible`): edible on a full food bar
    pub always_edible: bool,
    /// `setPotionEffect(effect, probability)`
    pub effect: Option<(PotionEffect, f32)>,
}

const fn f(heal: i32, saturation: f32, wolf_meat: bool) -> Food {
    Food { heal, saturation, wolf_meat, always_edible: false, effect: None }
}

/// `ItemFood(amount, isWolfFood)`: saturation 0.6 (`ItemSoup` uses this).
const fn soup(heal: i32) -> Food {
    f(heal, 0.6, false)
}

/// `ItemFood` values by item id; `None` for anything that is not food. Fish give the cod
/// values here; [`heal_amount`] and [`saturation_modifier`] read the fish type.
pub fn food(item_id: u16) -> Option<Food> {
    Some(match item_id {
        item::APPLE => f(4, 0.3, false),
        item::MUSHROOM_STEW => soup(6),
        item::BREAD => f(5, 0.6, false),
        item::PORKCHOP => f(3, 0.3, true),
        item::COOKED_PORKCHOP => f(8, 0.8, true),
        item::GOLDEN_APPLE => Food { always_edible: true, ..f(4, 1.2, false) },
        // ItemFishFood: super(0, 0.0F, false); the values come from FishType
        item::FISH | item::COOKED_FISH => f(0, 0.0, false),
        item::COOKIE => f(2, 0.1, false),
        item::MELON => f(2, 0.3, false),
        item::BEEF => f(3, 0.3, true),
        item::COOKED_BEEF => f(8, 0.8, true),
        item::CHICKEN => Food { effect: Some((pe(Effect::Hunger, 600, 0), 0.3)), ..f(2, 0.3, true) },
        item::COOKED_CHICKEN => f(6, 0.6, true),
        item::ROTTEN_FLESH => Food { effect: Some((pe(Effect::Hunger, 600, 0), 0.8)), ..f(4, 0.1, true) },
        item::SPIDER_EYE => Food { effect: Some((pe(Effect::Poison, 100, 0), 1.0)), ..f(2, 0.8, false) },
        item::CARROT => f(3, 0.6, false),
        item::POTATO => f(1, 0.3, false),
        item::BAKED_POTATO => f(5, 0.6, false),
        item::POISONOUS_POTATO => Food { effect: Some((pe(Effect::Poison, 100, 0), 0.6)), ..f(2, 0.3, false) },
        item::GOLDEN_CARROT => f(6, 1.2, false),
        item::PUMPKIN_PIE => f(8, 0.3, false),
        item::RABBIT => f(3, 0.3, true),
        item::COOKED_RABBIT => f(5, 0.6, true),
        item::RABBIT_STEW => soup(10),
        item::MUTTON => f(2, 0.3, true),
        item::COOKED_MUTTON => f(6, 0.8, true),
        item::CHORUS_FRUIT => Food { always_edible: true, ..f(4, 0.3, false) },
        item::BEETROOT => f(1, 0.6, false),
        item::BEETROOT_SOUP => soup(6),
        _ => return None,
    })
}

/// `ItemFishFood.FishType`, by metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FishType {
    Cod,
    Salmon,
    Clownfish,
    Pufferfish,
}

impl FishType {
    /// `byMetadata(meta)`: unknown metadata is COD.
    pub fn by_metadata(meta: i32) -> FishType {
        match meta {
            1 => FishType::Salmon,
            2 => FishType::Clownfish,
            3 => FishType::Pufferfish,
            _ => FishType::Cod,
        }
    }
    /// `getTranslationKey()`
    pub fn translation_key(self) -> &'static str {
        match self {
            FishType::Cod => "cod",
            FishType::Salmon => "salmon",
            FishType::Clownfish => "clownfish",
            FishType::Pufferfish => "pufferfish",
        }
    }
    /// `(uncookedHeal, uncookedSaturation, cookedHeal, cookedSaturation)`
    fn values(self) -> (i32, f32, i32, f32) {
        match self {
            FishType::Cod => (2, 0.1, 5, 0.6),
            FishType::Salmon => (2, 0.1, 6, 0.8),
            FishType::Clownfish => (1, 0.1, 0, 0.0),
            FishType::Pufferfish => (1, 0.1, 0, 0.0),
        }
    }
    /// `canCook()`
    pub fn can_cook(self) -> bool {
        matches!(self, FishType::Cod | FishType::Salmon)
    }
    /// `byItemStack(stack)`: COD for anything that is not a fish item.
    pub fn by_item_stack(stack: &ItemStack) -> FishType {
        match stack.get_item() {
            item::FISH | item::COOKED_FISH => FishType::by_metadata(stack.get_metadata()),
            _ => FishType::Cod,
        }
    }
}

/// `ItemFood.getHealAmount(stack)` (with `ItemFishFood`'s override); `None` for non-food.
pub fn heal_amount(stack: &ItemStack) -> Option<i32> {
    let it = stack.get_item();
    if it == item::FISH || it == item::COOKED_FISH {
        let t = FishType::by_item_stack(stack);
        let (uh, _, ch, _) = t.values();
        return Some(if it == item::COOKED_FISH && t.can_cook() { ch } else { uh });
    }
    food(it).map(|f| f.heal)
}

/// `ItemFood.getSaturationModifier(stack)` (with `ItemFishFood`'s override).
pub fn saturation_modifier(stack: &ItemStack) -> Option<f32> {
    let it = stack.get_item();
    if it == item::FISH || it == item::COOKED_FISH {
        let t = FishType::by_item_stack(stack);
        let (_, us, _, cs) = t.values();
        return Some(if it == item::COOKED_FISH && t.can_cook() { cs } else { us });
    }
    food(it).map(|f| f.saturation)
}

/// `ItemFood.getMaxItemUseDuration(stack)`: 32 ticks of eating; 0 for non-food.
pub fn max_item_use_duration(stack: &ItemStack) -> i32 {
    if food(stack.get_item()).is_some() {
        32
    } else {
        0
    }
}

/// `onFoodEaten(stack, world, player)` with every override: the effects to add, drawing
/// `World.rand.nextFloat()` only where Java does (an `ItemFood` with `setPotionEffect`).
/// `ItemFood.onItemUseFinish` draws one float for the burp pitch before this; that draw is the
/// caller's.
pub fn on_food_eaten(stack: &ItemStack, rand: &mut JavaRandom) -> Vec<PotionEffect> {
    let it = stack.get_item();
    let mut out = Vec::new();
    match it {
        // ItemAppleGold.onFoodEaten (does not call super)
        item::GOLDEN_APPLE => {
            if stack.get_metadata() > 0 {
                out.push(pe(Effect::Regeneration, 400, 1));
                out.push(pe(Effect::Resistance, 6000, 0));
                out.push(pe(Effect::FireResistance, 6000, 0));
                out.push(pe(Effect::Absorption, 2400, 3));
            } else {
                out.push(pe(Effect::Regeneration, 100, 1));
                out.push(pe(Effect::Absorption, 2400, 0));
            }
        }
        _ => {
            // ItemFishFood.onFoodEaten
            if (it == item::FISH || it == item::COOKED_FISH) && FishType::by_item_stack(stack) == FishType::Pufferfish {
                out.push(pe(Effect::Poison, 1200, 3));
                out.push(pe(Effect::Hunger, 300, 2));
                out.push(pe(Effect::Nausea, 300, 1));
            }
            // ItemFood.onFoodEaten
            if let Some(Food { effect: Some((e, p)), .. }) = food(it) {
                if rand.next_float() < p {
                    out.push(e);
                }
            }
        }
    }
    // DROPPED: ItemChorusFruit's random teleport (world and entity code) after eating.
    out
}

/// What `onItemUseFinish` leaves in the hand: the stack less one, or a bowl for `ItemSoup`.
pub fn eat(stack: &ItemStack) -> ItemStack {
    let mut s = *stack;
    s.shrink(1);
    match stack.get_item() {
        // ItemSoup.onItemUseFinish: super, then new ItemStack(Items.BOWL)
        item::MUSHROOM_STEW | item::RABBIT_STEW | item::BEETROOT_SOUP => ItemStack::of(item::BOWL),
        _ => {
            if s.is_empty() {
                ItemStack::EMPTY
            } else {
                s
            }
        }
    }
}
