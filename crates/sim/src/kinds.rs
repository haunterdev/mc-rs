//! Entity kinds survival knows: the `EntityList` id and registry name, the size each entity's
//! constructor gives `Entity.setSize`, the adult eye height (`getEyeHeight`), the texture its
//! renderer binds, and how `EntityTracker.track` sends it to players (range in blocks, update
//! interval in ticks, whether velocity goes along).
//!
//! Sizes and eye heights are the adult, standing values. A child's size is
//! `EntityAgeable.setScale` (half) or `EntityZombie.setChildSize`; the eye heights that depend on
//! state (`EntityPlayer.getEyeHeight` sneaking, `EntityCow.getEyeHeight` for a calf) are left to
//! the code that has that state.

/// One entity kind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kind {
    /// `EntityList.register` id; `None` for the player, which `EntityList` names but never
    /// registers.
    pub id: Option<u16>,
    /// The registry name without the `minecraft:` domain.
    pub name: &'static str,
    /// `Entity.setSize` in the entity's constructor.
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    /// The texture the renderer's `getEntityTexture` returns, under `assets/minecraft/`.
    pub texture: &'static str,
    /// `EntityTracker.track(entity, trackingRange, updateFrequency, sendVelocityUpdates)`.
    pub tracker_range: i32,
    pub tracker_interval: i32,
    pub tracker_velocity: bool,
}

/// `Entity.getEyeHeight`: `height * 0.85F`, for kinds that do not override it.
const fn default_eye(height: f32) -> f32 {
    height * 0.85
}

/// `TextureMap.LOCATION_BLOCKS_TEXTURE`: items and falling blocks draw from the block atlas
/// (`RenderEntityItem.getEntityTexture`, `RenderFallingBlock.getEntityTexture`).
pub const BLOCK_ATLAS: &str = "textures/atlas/blocks.png";

pub const PLAYER: Kind = Kind {
    id: None,
    // EntityList.PLAYER.
    name: "player",
    // EntityPlayer.<init> and updateSize standing: setSize(0.6F, 1.8F).
    width: 0.6,
    height: 1.8,
    // EntityPlayer.getEyeHeight: 1.62F standing.
    eye_height: 1.62,
    // DefaultPlayerSkin.TEXTURE_STEVE.
    texture: "textures/entity/steve.png",
    // EntityTracker.track: EntityPlayerMP is track(entity, 512, 2), the three-argument form,
    // which passes sendVelocityUpdates false.
    tracker_range: 512,
    tracker_interval: 2,
    tracker_velocity: false,
};

pub const ITEM: Kind = Kind {
    // EntityList.init: register(1, "item", EntityItem.class, "Item").
    id: Some(1),
    name: "item",
    // EntityItem.<init>: setSize(0.25F, 0.25F).
    width: 0.25,
    height: 0.25,
    eye_height: default_eye(0.25),
    texture: BLOCK_ATLAS,
    // EntityTracker.track: EntityItem is track(entity, 64, 20, true).
    tracker_range: 64,
    tracker_interval: 20,
    tracker_velocity: true,
};

pub const XP_ORB: Kind = Kind {
    // EntityList.init: register(2, "xp_orb", EntityXPOrb.class, "XPOrb").
    id: Some(2),
    name: "xp_orb",
    // EntityXPOrb.<init>(world, x, y, z, value): setSize(0.5F, 0.5F).
    width: 0.5,
    height: 0.5,
    eye_height: default_eye(0.5),
    // RenderXPOrb.EXPERIENCE_ORB_TEXTURES.
    texture: "textures/entity/experience_orb.png",
    // EntityTracker.track: EntityXPOrb is track(entity, 64, 20, true).
    tracker_range: 64,
    tracker_interval: 20,
    tracker_velocity: true,
};

pub const ARROW: Kind = Kind {
    // EntityList.init: register(10, "arrow", EntityTippedArrow.class, "Arrow").
    id: Some(10),
    name: "arrow",
    // EntityArrow.<init>: setSize(0.5F, 0.5F).
    width: 0.5,
    height: 0.5,
    // EntityArrow.getEyeHeight: 0.0F.
    eye_height: 0.0,
    // RenderTippedArrow.RES_ARROW (an arrow with no potion color).
    texture: "textures/entity/projectiles/arrow.png",
    // EntityTracker.track: EntityArrow is track(entity, 64, 20, false).
    tracker_range: 64,
    tracker_interval: 20,
    tracker_velocity: false,
};

pub const FALLING_BLOCK: Kind = Kind {
    // EntityList.init: register(21, "falling_block", EntityFallingBlock.class, "FallingSand").
    id: Some(21),
    name: "falling_block",
    // EntityFallingBlock.<init>: setSize(0.98F, 0.98F).
    width: 0.98,
    height: 0.98,
    eye_height: default_eye(0.98),
    texture: BLOCK_ATLAS,
    // EntityTracker.track: EntityFallingBlock is track(entity, 160, 20, true).
    tracker_range: 160,
    tracker_interval: 20,
    tracker_velocity: true,
};

/// `EntityTracker.track`: every mob here is an `IAnimals` (`EntityAnimal` directly, `EntityMob`
/// through `IMob`), track(entity, 80, 3, true).
const MOB_RANGE: i32 = 80;
const MOB_INTERVAL: i32 = 3;

pub const PIG: Kind = Kind {
    // EntityList.init: register(90, "pig", EntityPig.class, "Pig").
    id: Some(90),
    name: "pig",
    // EntityPig.<init>: setSize(0.9F, 0.9F).
    width: 0.9,
    height: 0.9,
    eye_height: default_eye(0.9),
    // RenderPig.PIG_TEXTURES.
    texture: "textures/entity/pig/pig.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

pub const SHEEP: Kind = Kind {
    // EntityList.init: register(91, "sheep", EntitySheep.class, "Sheep").
    id: Some(91),
    name: "sheep",
    // EntitySheep.<init>: setSize(0.9F, 1.3F).
    width: 0.9,
    height: 1.3,
    // EntitySheep.getEyeHeight: 0.95F * this.height.
    eye_height: 0.95 * 1.3,
    // RenderSheep.SHEARED_SHEEP_TEXTURES (LayerSheepWool draws sheep_fur.png over it).
    texture: "textures/entity/sheep/sheep.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

pub const COW: Kind = Kind {
    // EntityList.init: register(92, "cow", EntityCow.class, "Cow").
    id: Some(92),
    name: "cow",
    // EntityCow.<init>: setSize(0.9F, 1.4F).
    width: 0.9,
    height: 1.4,
    // EntityCow.getEyeHeight: 1.3F for an adult, this.height for a calf.
    eye_height: 1.3,
    // RenderCow.COW_TEXTURES.
    texture: "textures/entity/cow/cow.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

pub const CHICKEN: Kind = Kind {
    // EntityList.init: register(93, "chicken", EntityChicken.class, "Chicken").
    id: Some(93),
    name: "chicken",
    // EntityChicken.<init>: setSize(0.4F, 0.7F).
    width: 0.4,
    height: 0.7,
    // EntityChicken.getEyeHeight: this.height.
    eye_height: 0.7,
    // RenderChicken.CHICKEN_TEXTURES.
    texture: "textures/entity/chicken.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

pub const ZOMBIE: Kind = Kind {
    // EntityList.init: register(54, "zombie", EntityZombie.class, "Zombie").
    id: Some(54),
    name: "zombie",
    // EntityZombie.<init>: setSize(0.6F, 1.95F).
    width: 0.6,
    height: 1.95,
    // EntityZombie.getEyeHeight: 1.74F, less 0.81 for a child.
    eye_height: 1.74,
    // RenderZombie.ZOMBIE_TEXTURES.
    texture: "textures/entity/zombie/zombie.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

pub const SKELETON: Kind = Kind {
    // EntityList.init: register(51, "skeleton", EntitySkeleton.class, "Skeleton").
    id: Some(51),
    name: "skeleton",
    // AbstractSkeleton.<init>: setSize(0.6F, 1.99F).
    width: 0.6,
    height: 1.99,
    // AbstractSkeleton.getEyeHeight: 1.74F.
    eye_height: 1.74,
    // RenderSkeleton.SKELETON_TEXTURES.
    texture: "textures/entity/skeleton/skeleton.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

pub const CREEPER: Kind = Kind {
    // EntityList.init: register(50, "creeper", EntityCreeper.class, "Creeper").
    id: Some(50),
    name: "creeper",
    // EntityCreeper.<init>: setSize(0.6F, 1.7F).
    width: 0.6,
    height: 1.7,
    eye_height: default_eye(1.7),
    // RenderCreeper.CREEPER_TEXTURES.
    texture: "textures/entity/creeper/creeper.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

pub const SPIDER: Kind = Kind {
    // EntityList.init: register(52, "spider", EntitySpider.class, "Spider").
    id: Some(52),
    name: "spider",
    // EntitySpider.<init>: setSize(1.4F, 0.9F).
    width: 1.4,
    height: 0.9,
    // EntitySpider.getEyeHeight: 0.65F.
    eye_height: 0.65,
    // RenderSpider.SPIDER_TEXTURES.
    texture: "textures/entity/spider/spider.png",
    tracker_range: MOB_RANGE,
    tracker_interval: MOB_INTERVAL,
    tracker_velocity: true,
};

/// Every kind, the player first.
pub static KINDS: [Kind; 13] = [PLAYER, ITEM, XP_ORB, ARROW, FALLING_BLOCK, PIG, SHEEP, COW, CHICKEN, ZOMBIE, SKELETON, CREEPER, SPIDER];

/// The kind with this `EntityList` id (`proto::SpawnMob::kind`).
pub fn by_id(id: u16) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == Some(id))
}

/// The kind with this registry name, with or without the `minecraft:` domain.
pub fn by_name(name: &str) -> Option<&'static Kind> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    KINDS.iter().find(|k| k.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_unique_and_sane() {
        for (i, k) in KINDS.iter().enumerate() {
            for o in &KINDS[i + 1..] {
                assert_ne!(k.name, o.name);
                assert!(k.id.is_none() || k.id != o.id, "{} and {} share an id", k.name, o.name);
            }
            assert!(k.width > 0.0 && k.height > 0.0, "{}", k.name);
            assert!((0.0..=k.height).contains(&k.eye_height), "{}", k.name);
            assert!(k.texture.starts_with("textures/") && k.texture.ends_with(".png"), "{}", k.name);
            assert!(k.tracker_range > 0 && k.tracker_interval > 0, "{}", k.name);
            assert_eq!(by_name(k.name), Some(k));
            assert_eq!(by_name(&format!("minecraft:{}", k.name)), Some(k));
            if let Some(id) = k.id {
                assert_eq!(by_id(id), Some(k));
            }
        }
    }

    #[test]
    fn values_match_vanilla() {
        // The ids proto's SpawnMob carries.
        assert_eq!(by_id(90).unwrap().name, "pig");
        assert_eq!(by_id(54).unwrap().name, "zombie");
        assert_eq!(by_id(52).unwrap().width, 1.4);
        assert_eq!(by_id(7), None);
        assert_eq!(PLAYER.eye_height, 1.62);
        // Entity.getEyeHeight in float arithmetic, as Java computes it.
        assert_eq!(CREEPER.eye_height, 1.7f32 * 0.85f32);
        assert_eq!(SHEEP.eye_height, 0.95f32 * 1.3f32);
        assert_eq!((ITEM.tracker_range, ITEM.tracker_interval, ITEM.tracker_velocity), (64, 20, true));
        assert_eq!((COW.tracker_range, COW.tracker_interval), (80, 3));
        assert_eq!(by_name("minecraft:enderman"), None);
    }
}
