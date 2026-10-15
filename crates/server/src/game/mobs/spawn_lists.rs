//! What can spawn where: `EnumCreatureType`, `Biome.SpawnListEntry` and the four per-biome lists
//! (`Biome.getSpawnableList`) as each 1.12.2 overworld biome class leaves them after its
//! constructor, and `EntitySpawnPlacementRegistry`.
//!
//! An entry names its kind by registry name. Kinds `MOBS` does not have yet (slime, enderman,
//! witch, squid, bat, rabbit, wolf, horse and the rest) are listed anyway: they weigh into
//! `WeightedRandom.getRandomItem` and decide which pack a spot rolls, and the spawner skips the
//! spawn itself until the kind exists.

use worldgen::biome::{Biome, Kind};

use super::MobDef;

/// `EnumCreatureType`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreatureType {
    Monster,
    Creature,
    Ambient,
    WaterCreature,
}

/// `EnumCreatureType.values()`, in declaration order.
pub const TYPES: [CreatureType; 4] = [CreatureType::Monster, CreatureType::Creature, CreatureType::Ambient, CreatureType::WaterCreature];

impl CreatureType {
    /// `getMaxNumberOfCreature`
    pub fn max_number(self) -> i32 {
        match self {
            CreatureType::Monster => 70,
            CreatureType::Creature => 10,
            CreatureType::Ambient => 15,
            CreatureType::WaterCreature => 5,
        }
    }

    /// `getPeacefulCreature`
    pub fn peaceful(self) -> bool {
        self != CreatureType::Monster
    }

    /// `getAnimal`
    pub fn animal(self) -> bool {
        self == CreatureType::Creature
    }
}

/// `EntityLiving.SpawnPlacementType`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    OnGround,
    InWater,
}

/// `EntitySpawnPlacementRegistry.getPlacementForEntity`: only the squid is in water; a class the
/// registry lacks (null) takes the ground branch of `canCreatureTypeSpawnAtLocation`.
pub fn placement(name: &str) -> Placement {
    if name == "squid" {
        Placement::InWater
    } else {
        Placement::OnGround
    }
}

/// One `Biome.SpawnListEntry`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The entity's registry name without `minecraft:`.
    pub name: &'static str,
    pub weight: i32,
    pub min: i32,
    pub max: i32,
}

const fn e(name: &'static str, weight: i32, min: i32, max: i32) -> Entry {
    Entry { name, weight, min, max }
}

/// Which `EnumCreatureType` counts a mob (`IMob`, `EntityAnimal`): the kinds `MOBS` has.
pub fn creature_type_of(def: &MobDef) -> CreatureType {
    match def.kind.name {
        "zombie" | "skeleton" | "creeper" | "spider" => CreatureType::Monster,
        _ => CreatureType::Creature,
    }
}

/// The kinds that extend `EntityMob`.
pub fn is_hostile(def: &MobDef) -> bool {
    creature_type_of(def) == CreatureType::Monster
}

/// `BiomeForest.Type` as the registry passes it per biome id.
fn forest_flower(b: &Biome) -> bool {
    b.id == 132
}

fn forest_normal(b: &Biome) -> bool {
    !matches!(b.kind, Kind::ForestMutated) && !matches!(b.id, 132 | 27 | 28 | 29 | 157)
}

/// `BiomeJungle.isEdge`
fn jungle_edge(b: &Biome) -> bool {
    matches!(b.id, 23 | 151)
}

/// `Biome.getSpawnableList(type)`. DROPPED: the structure lists `ChunkGeneratorOverworld.
/// getPossibleCreatures` swaps in (swamp huts, ocean monuments, mansions): those structures are
/// not generated.
pub fn spawnable_list(b: &Biome, t: CreatureType) -> Vec<Entry> {
    let mut monsters = vec![
        e("spider", 100, 4, 4),
        e("zombie", 95, 4, 4),
        e("zombie_villager", 5, 1, 1),
        e("skeleton", 100, 4, 4),
        e("creeper", 100, 4, 4),
        e("slime", 100, 4, 4),
        e("enderman", 10, 1, 4),
        e("witch", 5, 1, 1),
    ];
    let mut creatures = vec![e("sheep", 12, 4, 4), e("pig", 10, 4, 4), e("chicken", 10, 4, 4), e("cow", 8, 4, 4)];
    let mut water = vec![e("squid", 10, 4, 4)];
    let mut cave = vec![e("bat", 10, 8, 8)];
    match b.kind {
        Kind::Beach | Kind::StoneBeach | Kind::Ocean | Kind::River | Kind::Mesa { .. } => creatures.clear(),
        Kind::Desert => {
            creatures = vec![e("rabbit", 4, 2, 3)];
            monsters.retain(|m| m.name != "zombie" && m.name != "zombie_villager");
            monsters.push(e("zombie", 19, 4, 4));
            monsters.push(e("zombie_villager", 1, 1, 1));
            monsters.push(e("husk", 80, 4, 4));
        }
        Kind::Forest | Kind::ForestMutated => {
            if forest_flower(b) {
                creatures.push(e("rabbit", 4, 2, 3));
            }
            if forest_normal(b) {
                creatures.push(e("wolf", 5, 4, 4));
            }
        }
        Kind::Hills(_) => creatures.push(e("llama", 5, 4, 6)),
        Kind::Jungle => {
            if !jungle_edge(b) {
                monsters.push(e("ocelot", 2, 1, 1));
            }
            creatures.push(e("parrot", 40, 1, 2));
            creatures.push(e("chicken", 10, 4, 4));
        }
        Kind::MushroomIsland => {
            monsters.clear();
            creatures = vec![e("mooshroom", 8, 4, 8)];
            water.clear();
        }
        Kind::Plains => {
            creatures.push(e("horse", 5, 2, 6));
            creatures.push(e("donkey", 1, 1, 3));
        }
        Kind::Savanna | Kind::SavannaMutated => {
            creatures.push(e("horse", 1, 2, 6));
            creatures.push(e("donkey", 1, 1, 1));
            if b.base_height > 1.1 {
                creatures.push(e("llama", 8, 4, 4));
            }
        }
        Kind::Snow { .. } => {
            creatures = vec![e("rabbit", 10, 2, 3), e("polar_bear", 1, 1, 2)];
            monsters.retain(|m| m.name != "skeleton");
            monsters.push(e("skeleton", 20, 4, 4));
            monsters.push(e("stray", 80, 4, 4));
        }
        Kind::Swamp => monsters.push(e("slime", 1, 1, 1)),
        Kind::Taiga(_) => {
            creatures.push(e("wolf", 8, 4, 4));
            creatures.push(e("rabbit", 4, 2, 3));
        }
        Kind::Hell => {
            monsters = vec![e("ghast", 50, 4, 4), e("zombie_pigman", 100, 4, 4), e("magma_cube", 2, 4, 4), e("enderman", 1, 4, 4)];
            creatures.clear();
            water.clear();
            cave.clear();
        }
        Kind::End => {
            monsters = vec![e("enderman", 10, 4, 4)];
            creatures.clear();
            water.clear();
            cave.clear();
        }
        Kind::Void => {
            monsters.clear();
            creatures.clear();
            water.clear();
            cave.clear();
        }
    }
    match t {
        CreatureType::Monster => monsters,
        CreatureType::Creature => creatures,
        CreatureType::WaterCreature => water,
        CreatureType::Ambient => cave,
    }
}

/// `WeightedRandom.getRandomItem(random, list)`: the index picked.
pub fn pick(rand: &mut worldgen::java_random::JavaRandom, list: &[Entry]) -> usize {
    let total: i32 = list.iter().map(|x| x.weight).sum();
    let mut w = rand.next_int_bounded(total);
    for (i, x) in list.iter().enumerate() {
        w -= x.weight;
        if w < 0 {
            return i;
        }
    }
    unreachable!("weights sum to the bound")
}
