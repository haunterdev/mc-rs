//! The passive animals' kind-specific parts (T9i), ported 1:1 from 1.12.2 `EntityPig`,
//! `EntitySheep` and `EntityChicken` (and `EntityCow`'s goal list, which T9g began with):
//! `initEntityAI`, the breeding items, the sheep's fleece (`DYE_COLOR`: colour in bits 0 to 3,
//! sheared in bit 4), the pig's saddle, the chicken's egg timer and slow fall.
//!
//! The sheep colour, sheared flag and saddle are server state on [`Mob`] and in `mobs.json`; they
//! are not in `MobMeta` yet.
//!
//! DROPPED: the sounds of all three; `EntityLiving.onInitialSpawn`'s follow range bonus and
//! left-handedness draws; the sheep's `sheepTimer` and head animation (client side, driven by
//! status 10); the chicken's wing fields (`wingRotation`, `destPos`, `oFlap`, `oFlapSpeed`,
//! `wingRotDelta`, client animation only); the chicken jockey (`chickenJockey` is always false,
//! so `canDespawn` stays the animals' never, and `getExperiencePoints` is the default); the pig's
//! riding: `boost`, `canBeSteered`, `travel` while ridden and `getControllingPassenger`, and
//! `onStruckByLightning` (no lightning); name tags.

use items::ids::item;
use items::ItemStack;
use worldgen::java_random::JavaRandom;

use super::ai::{self, animal_goals, eat_grass, goals};
use super::Mob;

/// `EnumDyeColor.getDyeDamage` of a colour's metadata, and back: they sum to 15.
pub fn dye_damage(meta: u8) -> i32 {
    15 - (meta & 15) as i32
}

/// `EnumDyeColor.byDyeDamage(damage).getMetadata()`: out of range is damage 0.
pub fn meta_of_dye_damage(damage: i32) -> u8 {
    let d = if (0..16).contains(&damage) { damage } else { 0 };
    (15 - d) as u8
}

/// `EntitySheep.getRandomSheepColor`: the metadata of the colour.
pub fn get_random_sheep_color(rand: &mut JavaRandom) -> u8 {
    const WHITE: u8 = 0;
    const PINK: u8 = 6;
    const GRAY: u8 = 7;
    const SILVER: u8 = 8;
    const BROWN: u8 = 12;
    const BLACK: u8 = 15;
    let i = rand.next_int_bounded(100);
    if i < 5 {
        BLACK
    } else if i < 10 {
        GRAY
    } else if i < 15 {
        SILVER
    } else if i < 18 {
        BROWN
    } else if rand.next_int_bounded(500) == 0 {
        PINK
    } else {
        WHITE
    }
}

/// The loot table file names of `EntitySheep.getLootTable`, by colour metadata.
const SHEEP_TABLES: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "silver", "cyan", "purple", "blue", "brown",
    "green", "red", "black",
];

/// `EntityPig.TEMPTATION_ITEMS`
pub const PIG_FOOD: [u16; 3] = [item::CARROT, item::POTATO, item::BEETROOT];
/// `EntityChicken.TEMPTATION_ITEMS`
pub const CHICKEN_FOOD: [u16; 4] = [item::WHEAT_SEEDS, item::MELON_SEEDS, item::PUMPKIN_SEEDS, item::BEETROOT_SEEDS];

impl Mob {
    /// `initEntityAI` of the kind (`EntityCow`, `EntityPig`, `EntitySheep`, `EntityChicken`).
    /// The zombie's lists are `zombie.rs`; the other hostile kinds' goals are DROPPED until they
    /// are ported.
    pub(super) fn init_entity_ai(&mut self) {
        match self.def.kind.name {
            "cow" => {
                let swimming = goals::Swimming::new(self);
                self.tasks.add_task(0, swimming);
                self.tasks.add_task(1, goals::Panic::new(2.0));
                self.tasks.add_task(2, animal_goals::Mate::new(1.0));
                self.tasks.add_task(3, animal_goals::Tempt::new(1.25, item::WHEAT, false));
                self.tasks.add_task(4, animal_goals::FollowParent::new(1.25));
                self.tasks.add_task(5, goals::Wander::avoid_water(1.0));
                self.tasks.add_task(6, goals::WatchClosest::new(6.0));
                self.tasks.add_task(7, goals::LookIdle::new());
            }
            "pig" => {
                let swimming = goals::Swimming::new(self);
                self.tasks.add_task(0, swimming);
                self.tasks.add_task(1, goals::Panic::new(1.25));
                self.tasks.add_task(3, animal_goals::Mate::new(1.0));
                self.tasks.add_task(4, animal_goals::Tempt::new(1.2, item::CARROT_ON_A_STICK, false));
                self.tasks.add_task(4, animal_goals::Tempt::with_items(1.2, &PIG_FOOD, false));
                self.tasks.add_task(5, animal_goals::FollowParent::new(1.1));
                self.tasks.add_task(6, goals::Wander::avoid_water(1.0));
                self.tasks.add_task(7, goals::WatchClosest::new(6.0));
                self.tasks.add_task(8, goals::LookIdle::new());
            }
            "sheep" => {
                let swimming = goals::Swimming::new(self);
                self.tasks.add_task(0, swimming);
                self.tasks.add_task(1, goals::Panic::new(1.25));
                self.tasks.add_task(2, animal_goals::Mate::new(1.0));
                self.tasks.add_task(3, animal_goals::Tempt::new(1.1, item::WHEAT, false));
                self.tasks.add_task(4, animal_goals::FollowParent::new(1.1));
                self.tasks.add_task(5, eat_grass::EatGrass::new());
                self.tasks.add_task(6, goals::Wander::avoid_water(1.0));
                self.tasks.add_task(7, goals::WatchClosest::new(6.0));
                self.tasks.add_task(8, goals::LookIdle::new());
            }
            "chicken" => {
                let swimming = goals::Swimming::new(self);
                self.tasks.add_task(0, swimming);
                self.tasks.add_task(1, goals::Panic::new(1.4));
                self.tasks.add_task(2, animal_goals::Mate::new(1.0));
                self.tasks.add_task(3, animal_goals::Tempt::with_items(1.0, &CHICKEN_FOOD, false));
                self.tasks.add_task(4, animal_goals::FollowParent::new(1.1));
                self.tasks.add_task(5, goals::Wander::avoid_water(1.0));
                self.tasks.add_task(6, goals::WatchClosest::new(6.0));
                self.tasks.add_task(7, goals::LookIdle::new());
            }
            "zombie" => self.init_zombie_ai(),
            "skeleton" => self.init_skeleton_ai(),
            _ => {}
        }
    }

    /// `EntityAnimal.isBreedingItem` (wheat) and the overrides of `EntityPig` and `EntityChicken`.
    pub fn is_breeding_item(&self, it: u16) -> bool {
        match self.def.kind.name {
            "pig" => PIG_FOOD.contains(&it),
            "chicken" => CHICKEN_FOOD.contains(&it),
            "cow" | "sheep" => it == item::WHEAT,
            _ => false,
        }
    }

    /// `EntitySheep.getFleeceColor`: the colour metadata.
    pub fn get_fleece_color(&self) -> u8 {
        self.dye_color & 15
    }

    /// `EntitySheep.setFleeceColor`
    pub fn set_fleece_color(&mut self, color: u8) {
        self.dye_color = self.dye_color & 240 | color & 15;
    }

    /// `EntitySheep.getSheared`
    pub fn get_sheared(&self) -> bool {
        self.dye_color & 16 != 0
    }

    /// `EntitySheep.setSheared`
    pub fn set_sheared(&mut self, sheared: bool) {
        if sheared {
            self.dye_color |= 16;
        } else {
            self.dye_color &= !16;
        }
    }

    /// `EntitySheep.eatGrassBonus`
    pub fn eat_grass_bonus(&mut self) {
        self.set_sheared(false);
        if self.is_child() {
            self.age_up(60, false);
        }
    }

    /// `EntityPig.getSaddled`
    pub fn get_saddled(&self) -> bool {
        self.saddled
    }

    /// `getLootTable`: the jar path of the table `onDeath` rolls. A sheep's is the plain mutton
    /// table once sheared, else the one of its fleece colour.
    pub fn loot_table_path(&self) -> String {
        if self.def.kind.name == "sheep" {
            if self.get_sheared() {
                return "loot_tables/entities/sheep.json".into();
            }
            return format!("loot_tables/entities/sheep/{}.json", SHEEP_TABLES[self.get_fleece_color() as usize]);
        }
        super::loot::table_path(self.def.kind.name)
    }

    /// The `Entity.rand` draws of the constructors: `EntityChicken` sets `timeUntilNextEgg`.
    pub(super) fn on_construct(&mut self, rand: &mut JavaRandom) {
        if self.def.kind.name == "chicken" {
            self.egg_timer = rand.next_int_bounded(6000) + 6000;
        }
    }

    /// `onInitialSpawn` for the spawner and `/summon`: `EntitySheep` picks a random fleece colour.
    pub(super) fn on_initial_spawn(&mut self, rand: &mut JavaRandom) {
        if self.def.kind.name == "skeleton" {
            self.skeleton_initial_spawn();
        }
        if self.def.kind.name == "sheep" {
            let c = get_random_sheep_color(rand);
            self.set_fleece_color(c);
        }
    }

    /// The tail of `EntityChicken.onLivingUpdate`: the slow fall and the egg.
    pub(super) fn chicken_living_update(&mut self, env: &mut ai::Env) {
        if self.def.kind.name != "chicken" {
            return;
        }
        if !self.body.on_ground && self.body.motion.y < 0.0 {
            self.body.motion.y *= 0.6;
        }
        // `chickenJockey` is always false here.
        if !self.is_child() {
            self.egg_timer -= 1;
            if self.egg_timer <= 0 {
                // playSound(ENTITY_CHICKEN_EGG, 1.0F, (rand - rand) * 0.2F + 1.0F): the pitch draws.
                let _pitch = env.rand.next_float() - env.rand.next_float();
                let p = self.body.pos;
                env.fx.push(ai::Effect::Drop { pos: [p.x, p.y, p.z], stack: ItemStack::of(item::EGG) });
                self.egg_timer = env.rand.next_int_bounded(6000) + 6000;
            }
        }
    }
}
