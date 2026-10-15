//! `PathNodeType`, `NodeProcessor`, and [`NavEntity`]: the `EntityLiving` fields the path code
//! reads and writes, copied out of a [`super::super::Mob`] for one navigation step so the
//! pathfinder needs no reference back into the mob list.

use std::collections::HashMap;

use glam::DVec3;
use sim::collision::{floor, Aabb, BlockAccess};

use super::math::sqrt_f;
use super::point::PathPoint;

/// `PathNodeType` (declaration order is the `EnumSet` iteration order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathNodeType {
    Blocked,
    Open,
    Walkable,
    Trapdoor,
    Fence,
    Lava,
    Water,
    Rail,
    DangerFire,
    DamageFire,
    DangerCactus,
    DamageCactus,
    DangerOther,
    DamageOther,
    DoorOpen,
    DoorWoodClosed,
    DoorIronClosed,
}

pub const NODE_TYPE_COUNT: usize = 17;

impl PathNodeType {
    pub const ALL: [PathNodeType; NODE_TYPE_COUNT] = [
        PathNodeType::Blocked,
        PathNodeType::Open,
        PathNodeType::Walkable,
        PathNodeType::Trapdoor,
        PathNodeType::Fence,
        PathNodeType::Lava,
        PathNodeType::Water,
        PathNodeType::Rail,
        PathNodeType::DangerFire,
        PathNodeType::DamageFire,
        PathNodeType::DangerCactus,
        PathNodeType::DamageCactus,
        PathNodeType::DangerOther,
        PathNodeType::DamageOther,
        PathNodeType::DoorOpen,
        PathNodeType::DoorWoodClosed,
        PathNodeType::DoorIronClosed,
    ];

    /// `PathNodeType.getPriority`
    pub fn priority(self) -> f32 {
        use PathNodeType::*;
        match self {
            Blocked | Fence | Lava | DamageCactus | DamageOther | DoorWoodClosed | DoorIronClosed => -1.0,
            Open | Walkable | Trapdoor | Rail | DoorOpen => 0.0,
            Water | DangerFire | DangerCactus | DangerOther => 8.0,
            DamageFire => 16.0,
        }
    }
}

/// `EntityLiving.mapPathPriority`: an override per node type, else the type's own priority.
#[derive(Clone, Copy, Debug, Default)]
pub struct Priorities([Option<f32>; NODE_TYPE_COUNT]);

impl Priorities {
    /// `EntityLiving.getPathPriority`
    pub fn get(&self, t: PathNodeType) -> f32 {
        self.0[t as usize].unwrap_or_else(|| t.priority())
    }

    /// `EntityLiving.setPathPriority`
    pub fn set(&mut self, t: PathNodeType, p: f32) {
        self.0[t as usize] = Some(p);
    }
}

/// The `EntityLiving` state one navigation step reads and writes.
#[derive(Clone, Debug)]
pub struct NavEntity {
    pub pos: DVec3,
    pub bb: Aabb,
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    pub step_height: f32,
    pub on_ground: bool,
    /// `Entity.inWater`
    pub in_water: bool,
    /// `Entity.isInLava()`, which needs the world, so the caller evaluates it.
    pub in_lava: bool,
    /// `EntityLiving.getMaxFallHeight`
    pub max_fall_height: i32,
    /// `SharedMonsterAttributes.FOLLOW_RANGE`
    pub follow_range: f32,
    /// `SharedMonsterAttributes.MOVEMENT_SPEED`
    pub movement_speed: f32,
    pub priorities: Priorities,
    pub yaw: f32,
    pub pitch: f32,
    pub head_yaw: f32,
    pub render_yaw_offset: f32,
    /// `EntityLivingBase.landMovementFactor` (`getAIMoveSpeed`)
    pub ai_move_speed: f32,
    pub move_forward: f32,
    pub move_strafing: f32,
    /// `EntityLivingBase.isJumping`
    pub is_jumping: bool,
}

impl NavEntity {
    /// `new BlockPos(entity)`
    pub fn block_pos(&self) -> (i32, i32, i32) {
        (floor(self.pos.x), floor(self.pos.y), floor(self.pos.z))
    }
}

/// What `NodeProcessor` keeps: the node arena (`pointMap` owns the nodes in Java), the entity's
/// cell footprint and the three door and swim flags.
#[derive(Clone, Debug, Default)]
pub struct NodeBase {
    pub arena: Vec<PathPoint>,
    /// `IntHashMap<PathPoint> pointMap`, keyed by `PathPoint.makeHash` (two cells that share a
    /// hash share a node, as in Java).
    point_map: HashMap<i32, usize>,
    pub entity_size_x: i32,
    pub entity_size_y: i32,
    pub entity_size_z: i32,
    pub can_enter_doors: bool,
    pub can_open_doors: bool,
    pub can_swim: bool,
}

impl NodeBase {
    /// `NodeProcessor.init`
    pub fn init(&mut self, e: &NavEntity) {
        self.arena.clear();
        self.point_map.clear();
        self.entity_size_x = floor((e.width + 1.0) as f64);
        self.entity_size_y = floor((e.height + 1.0) as f64);
        self.entity_size_z = floor((e.width + 1.0) as f64);
    }

    /// `NodeProcessor.openPoint`
    pub fn open_point(&mut self, x: i32, y: i32, z: i32) -> usize {
        let hash = PathPoint::make_hash(x, y, z);
        if let Some(&i) = self.point_map.get(&hash) {
            return i;
        }
        self.arena.push(PathPoint::new(x, y, z));
        let i = self.arena.len() - 1;
        self.point_map.insert(hash, i);
        i
    }
}

/// `NodeProcessor`. The world and the entity come in as arguments where Java stores them in
/// `blockaccess` and `entity` between `init` and `postProcess` (a `ChunkCache` there; the
/// world itself here, which reads the same blocks).
pub trait NodeProcessor {
    fn base(&self) -> &NodeBase;
    fn base_mut(&mut self) -> &mut NodeBase;

    fn init(&mut self, _w: &impl BlockAccess, e: &mut NavEntity) {
        self.base_mut().init(e);
    }
    fn post_process(&mut self, _e: &mut NavEntity) {}

    fn get_start(&mut self, w: &impl BlockAccess, e: &NavEntity) -> usize;
    fn get_path_point_to_coords(&mut self, e: &NavEntity, x: f64, y: f64, z: f64) -> usize;
    fn find_path_options(&mut self, w: &impl BlockAccess, e: &NavEntity, options: &mut Vec<usize>, current: usize, target: usize, max_distance: f32) -> usize;
    #[allow(clippy::too_many_arguments)]
    fn get_path_node_type_sized(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32, e: &NavEntity, size: (i32, i32, i32), can_break_doors: bool, can_enter_doors: bool) -> PathNodeType;
    fn get_path_node_type(&self, w: &impl BlockAccess, x: i32, y: i32, z: i32) -> PathNodeType;

    fn set_can_enter_doors(&mut self, v: bool) {
        self.base_mut().can_enter_doors = v;
    }
    fn set_can_open_doors(&mut self, v: bool) {
        self.base_mut().can_open_doors = v;
    }
    fn set_can_swim(&mut self, v: bool) {
        self.base_mut().can_swim = v;
    }
    fn get_can_enter_doors(&self) -> bool {
        self.base().can_enter_doors
    }
    fn get_can_open_doors(&self) -> bool {
        self.base().can_open_doors
    }
    fn get_can_swim(&self) -> bool {
        self.base().can_swim
    }
}

/// `distanceTo(target) < maxDistance` and not yet visited: the filter every processor applies
/// before it offers a neighbor.
pub fn offered(arena: &[PathPoint], p: usize, target: usize, max_distance: f32) -> bool {
    !arena[p].visited && arena[p].distance_to(&arena[target]) < max_distance
}

/// `MathHelper.sqrt(float)` re-export for the processors.
pub fn sqrt(v: f32) -> f32 {
    sqrt_f(v)
}
