//! `--mobs`: the four kinds in front of the `--at` camera for a `--shot`, fed through the same
//! frames the server sends: an adult and a baby cow, pig (the adult saddled), sheep (the adult
//! pink and eating grass every 80 ticks, the baby sheared) and chicken, and one cow walking a
//! circle behind them (a move every 3 ticks, as the tracker sends them). Nothing here is a second
//! code path; it only plays the server.
//!
//! `--mobs:hostile` stands the hostile set instead: a burning zombie that swings, a skeleton aiming a
//! bow, a creeper whose fuse is lit and put out, a spider, and in front a baby zombie, an idle
//! skeleton with a bow, a powered creeper and a creeper at rest.

use bevy::prelude::*;
use proto::{EntityMove, MobMeta, SpawnMob};

use crate::sky::View;
use crate::stream::Streamer;
use crate::survival::{Animation as AnimationMsg, EntityEvent as EventMsg, EntityMeta as MetaMsg, EntityMoves, Equipment as EquipMsg, SpawnMob as SpawnMsg};

const IDLE: u32 = 9100;
const WALKER: u32 = 9101;
const BABY: u32 = 9102;
const PIG: u32 = 9103;
const PIG_BABY: u32 = 9104;
const SHEEP: u32 = 9105;
const SHEEP_BABY: u32 = 9106;
const HEN: u32 = 9107;
const HEN_BABY: u32 = 9108;
const ZOMBIE: u32 = 9110;
const SKELETON: u32 = 9111;
const CREEPER: u32 = 9112;
const SPIDER: u32 = 9113;
const ZOMBIE_BABY: u32 = 9114;
const SKELETON_IDLE: u32 = 9115;
const CREEPER_POWERED: u32 = 9116;
const CREEPER_IDLE: u32 = 9117;
/// `EnumDyeColor.PINK`'s metadata, the adult sheep's fleece.
const PINK: u8 = 6;
/// Blocks a tick, the ground speed of a cow at its 0.2 movement attribute.
const SPEED: f64 = 0.1;
const RADIUS: f64 = 1.5;

/// Where the walker is after `n` ticks on its circle, and the yaw it faces (`atan2(dz, dx) - 90`).
pub fn walker_at(centre: [f64; 3], n: u32) -> ([f64; 3], f32) {
    let a = n as f64 * SPEED / RADIUS;
    let pos = [centre[0] + RADIUS * a.cos(), centre[1], centre[2] + RADIUS * a.sin()];
    let heading = (a.cos()).atan2(-a.sin());
    (pos, (heading.to_degrees() - 90.0) as f32)
}

pub(super) struct Demo {
    centre: [f64; 3],
    idle: [f64; 3],
    ticks: u32,
}

pub fn demo(
    view: Res<View>,
    mut streamer: ResMut<Streamer>,
    clock: Res<crate::sky::Clock>,
    mut spawn: MessageWriter<SpawnMsg>,
    mut moves: MessageWriter<EntityMoves>,
    mut events: MessageWriter<EventMsg>,
    mut equip: MessageWriter<EquipMsg>,
    mut anims: MessageWriter<AnimationMsg>,
    mut metas: MessageWriter<MetaMsg>,
    args: Res<crate::Args>,
    mut state: Local<Option<Demo>>,
) {
    if state.is_none() {
        let fwd = Vec3::new(-view.yaw.to_radians().sin(), 0.0, view.yaw.to_radians().cos()).normalize_or_zero();
        let right = Vec3::new(-fwd.z, 0.0, fwd.x);
        // A grass platform two blocks under the eye keeps trees and water out of the shot.
        let top = view.eye.y.floor() as i32 - 2;
        let middle = view.eye + fwd * 5.0;
        let (cx, cz) = (middle.x.floor() as i32, middle.z.floor() as i32);
        if streamer.state_at(cx, 0, cz).is_none() {
            return;
        }
        let mut edits = Vec::new();
        for dx in -9..=9 {
            for dz in -7..=7 {
                let (x, z) = (cx + (dx as f32 * right.x + dz as f32 * fwd.x).round() as i32, cz + (dx as f32 * right.z + dz as f32 * fwd.z).round() as i32);
                edits.push(sim::terrain::Edit { at: (x, top, z), s: (worldgen::blocks::id::GRASS << 4) as u16, flags: 3, place: true });
            }
        }
        streamer.edit(edits);
        let at = |p: Vec3| -> Option<[f64; 3]> { Some([p.x as f64, top as f64 + 1.0, p.z as f64]) };
        // Adults a row back, babies a row forward, one column a kind; the walker behind them all.
        let column = |k: f32, row: f32| at(middle + right * (3.2 * (k - 1.5)) + fwd * row);
        let (Some(idle), Some(baby), Some(walk)) = (column(0.0, 1.5), column(0.0, -1.3), at(middle + fwd * 5.0)) else { return };
        let mob = |id, kind, pos: [f64; 3], yaw: f32, flags, wool| SpawnMsg(SpawnMob { id, kind, pos, yaw, head_yaw: yaw, meta: MobMeta { flags, wool, ..Default::default() }, ..Default::default() });
        let (start, start_yaw) = walker_at(walk, 0);
        let hostile = args.mob_mode == 4;
        let mut placed = if hostile {
            let k = |n: f32, row: f32| column(n, row).unwrap_or_default();
            vec![
                (ZOMBIE, proto::MOB_ZOMBIE, k(0.0, 1.5), 200.0, proto::MM_AGGRESSIVE | proto::MM_BURNING, 0),
                (SKELETON, proto::MOB_SKELETON, k(1.0, 1.5), 200.0, proto::MM_AGGRESSIVE, 0),
                (CREEPER, proto::MOB_CREEPER, k(2.0, 1.5), 200.0, 0, 0),
                (SPIDER, proto::MOB_SPIDER, k(3.0, 1.5), 200.0, 0, 0),
                (ZOMBIE_BABY, proto::MOB_ZOMBIE, k(0.0, -1.3), 160.0, proto::MM_CHILD, 0),
                (SKELETON_IDLE, proto::MOB_SKELETON, k(1.0, -1.3), 160.0, 0, 0),
                (CREEPER_POWERED, proto::MOB_CREEPER, k(2.0, -1.3), 160.0, proto::MM_POWERED, 0),
                (CREEPER_IDLE, proto::MOB_CREEPER, k(3.0, -1.3), 160.0, 0, 0),
            ]
        } else {
            vec![
            (IDLE, proto::MOB_COW, idle, 200.0, 0, 0),
            (BABY, proto::MOB_COW, baby, 160.0, proto::MM_CHILD, 0),
            (PIG, proto::MOB_PIG, column(1.0, 1.5).unwrap_or_default(), 200.0, super::models::MM_SADDLED, 0),
            (PIG_BABY, proto::MOB_PIG, column(1.0, -1.3).unwrap_or_default(), 160.0, proto::MM_CHILD, 0),
            (SHEEP, proto::MOB_SHEEP, column(2.0, 1.5).unwrap_or_default(), 200.0, 0, PINK),
            (SHEEP_BABY, proto::MOB_SHEEP, column(2.0, -1.3).unwrap_or_default(), 160.0, proto::MM_CHILD | proto::MM_SHEARED, 0),
            (HEN, proto::MOB_CHICKEN, column(3.0, 1.5).unwrap_or_default(), 200.0, 0, 0),
            (HEN_BABY, proto::MOB_CHICKEN, column(3.0, -1.3).unwrap_or_default(), 160.0, proto::MM_CHILD, 0),
            ]
        };
        spawn.write(mob(WALKER, if hostile { proto::MOB_ZOMBIE } else { proto::MOB_COW }, start, start_yaw, 0, 0));
        let mut standing = Vec::new();
        for (id, kind, pos, yaw, flags, wool) in placed.drain(..) {
            spawn.write(mob(id, kind, pos, yaw, flags, wool));
            // A turn-only move carrying onGround, which a server sends for a mob standing on the floor
            // and which stops a chicken's wings (no spawn frame carries it).
            standing.push(EntityMove { id, pos, yaw, pitch: 0.0, head_yaw: yaw, on_ground: true, teleport: false });
        }
        moves.write(EntityMoves(standing));
        *state = Some(Demo { centre: walk, idle, ticks: 0 });
        return;
    }
    let Some(d) = state.as_mut() else { return };
    for _ in 0..clock.ticks_this_frame {
        d.ticks += 1;
        let status = |id, status| EventMsg(proto::EntityEvent { id, status });
        match args.mob_mode {
            // handleStatusUpdate 2 on the standing and the baby cow
            1 if d.ticks % 8 == 1 => {
                events.write(status(IDLE, proto::EV_HURT));
                events.write(status(BABY, proto::EV_HURT));
            }
            // The standing cow is spawned again once its 20 death ticks have removed it.
            2 if d.ticks % 24 == 1 => {
                let mob = SpawnMob { id: IDLE, kind: proto::MOB_COW, pos: d.idle, yaw: 200.0, head_yaw: 200.0, ..Default::default() };
                spawn.write(SpawnMsg(mob));
            }
            2 if d.ticks % 24 == 3 => {
                events.write(status(IDLE, proto::EV_DEATH));
            }
            // EntityAnimal.handleStatusUpdate 18 on the adults
            3 if d.ticks % 12 == 4 => {
                for id in [IDLE, PIG, SHEEP, HEN] {
                    events.write(status(id, proto::EV_LOVE));
                }
            }
            // The zombie swings its arm, the creeper's fuse is lit for 60 ticks of every 100.
            4 => {
                // after the spawn commands have applied, as the server's equipment frames come
                if d.ticks == 5 {
                    for id in [SKELETON, SKELETON_IDLE] {
                        equip.write(EquipMsg(proto::Equipment { id, slot: proto::EQ_MAIN_HAND, stack: proto::WireStack { id: items::ids::item::BOW as i16, count: 1, meta: 0, tag: [0; 3], ench: [0; proto::WireStack::ENCH_BYTES] } }));
                    }
                }
                if d.ticks % 12 == 3 {
                    anims.write(AnimationMsg(proto::Animation { id: ZOMBIE, animation: proto::ANIM_SWING_MAIN }));
                }
                let fuse = |flags| MetaMsg(proto::EntityMeta { id: CREEPER, payload: proto::MetaPayload::Mob(MobMeta { flags, ..Default::default() }) });
                match d.ticks % 100 {
                    10 => {
                        metas.write(fuse(proto::MM_IGNITED));
                    }
                    70 => {
                        metas.write(fuse(0));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        // The sheep lowers its head to eat grass (handleStatusUpdate 10).
        if d.ticks % 80 == 20 {
            events.write(status(SHEEP, proto::EV_EAT_GRASS));
        }
        if d.ticks % 3 == 0 {
            let (pos, yaw) = walker_at(d.centre, d.ticks);
            moves.write(EntityMoves(vec![EntityMove { id: WALKER, pos, yaw, pitch: 0.0, head_yaw: yaw, on_ground: true, teleport: false }]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walker_goes_a_tenth_of_a_block_a_tick_and_faces_its_heading() {
        let (a, _) = walker_at([0.0, 64.0, 0.0], 0);
        let (b, yaw) = walker_at([0.0, 64.0, 0.0], 1);
        let step = ((b[0] - a[0]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        assert!((step - 0.1).abs() < 1e-3, "{step}");
        // At the start of the circle it heads +z, which is yaw 0.
        assert!(yaw.abs() < 4.0, "{yaw}");
    }
}
