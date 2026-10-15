//! `net.minecraft.world.Explosion` (1.12.2) and `World.newExplosion`, ported 1:1: `doExplosionA`
//! (the 16 by 16 by 16 ray grid, the affected block list, damage and knockback on the entities in
//! range), `doExplosionB` (the blocks go to air through `terrain::set_block`, the drops come at
//! chance 1/size) and the `SPacketExplosion` to every player within 64 blocks.
//!
//! DROPPED: the sound and the particles; `causesFire` (nothing here asks for a fire explosion; the
//! Epic Siege napalm flag is not ported); `Block.onExplosionDestroy` (primed TNT chains, so a TNT
//! block is just removed); `canDropFromExplosion` (true for every block ported); the blast
//! protection enchantment (`getBlastDamageReduction`, no armor items on a mob); damage to item
//! entities, orbs and paintings (only mobs and players are hurt); the sneaking box of a player (the
//! standing box is used for exposure); `Forge ExplosionEvent`s; the affected block list inside
//! `SPacketExplosion` (the blocks reach each viewer as `S_BLOCKS`).

use glam::DVec3;
use sim::collision::{floor, Aabb};
use sim::mining::block_hardness;
use sim::region::{props, Mat};
use worldgen::java_random::JavaRandom;

use super::damage::{self, Attacker, DamageSource};
use super::mobs::combat;
use super::terrain::{self, Blocks};
use super::{items as item_entities, net, players, Game};

/// `Block.getExplosionResistance(null)` of a block state: the block's `blockResistance` over 5.
/// `setHardness(h)` raises the resistance to `5h` and `setResistance(r)` sets it to `3r`, so a
/// block without its own `setResistance` resists as much as it is hard, and the table lists only
/// the blocks that do. Unbreakable blocks (hardness below 0) resist 3600000 (`setResistance
/// (6000000)`), except the nether portal, which sets none.
pub fn block_resistance(state: u16) -> f32 {
    let id = state >> 4;
    let h = block_hardness(state);
    if h < 0.0 {
        return if id == 90 { 0.0 } else { 3_600_000.0 };
    }
    match id {
        // stone, cobblestone, gold/iron/diamond/emerald/coal blocks, slabs, bricks, mossy cobble,
        // jukebox, stone brick, iron bars, walls, redstone block, prismarine and the purpur and
        // red nether brick family, with their stairs
        1 | 4 | 41 | 42 | 43 | 44 | 45 | 48 | 57 | 67 | 84 | 98 | 101 | 108 | 109 | 112 | 113 | 114 | 133 | 139 | 152 | 168 | 173 | 181 | 182 | 201..=205 | 215 => 6.0,
        // planks (and their stairs, fences, gates, slabs), ores, lapis, cocoa
        5 | 14 | 15 | 16 | 21 | 22 | 53 | 73 | 74 | 85 | 107 | 125 | 126 | 127 | 129 | 134..=136 | 153 | 163 | 164 | 183..=192 => 3.0,
        49 | 116 | 145 => 1200.0,
        130 => 600.0,
        121 | 122 => 9.0,
        154 => 4.8,
        159 | 172 => 4.2,
        _ => h,
    }
}

/// `World.getBlockDensity(vec, bb)`: the share of a grid of points on the box that `vec` sees.
pub fn get_block_density(game: &Game, vec: DVec3, bb: &Aabb) -> f32 {
    let d0 = 1.0 / ((bb.max_x - bb.min_x) * 2.0 + 1.0);
    let d1 = 1.0 / ((bb.max_y - bb.min_y) * 2.0 + 1.0);
    let d2 = 1.0 / ((bb.max_z - bb.min_z) * 2.0 + 1.0);
    let d3 = (1.0 - (1.0 / d0).floor() * d0) / 2.0;
    let d4 = (1.0 - (1.0 / d2).floor() * d2) / 2.0;
    if d0 < 0.0 || d1 < 0.0 || d2 < 0.0 {
        return 0.0;
    }
    let w = Blocks(game);
    let (mut j2, mut k2) = (0i32, 0i32);
    let mut f = 0.0f32;
    while f <= 1.0 {
        let mut f1 = 0.0f32;
        while f1 <= 1.0 {
            let mut f2 = 0.0f32;
            while f2 <= 1.0 {
                let d5 = bb.min_x + (bb.max_x - bb.min_x) * f as f64;
                let d6 = bb.min_y + (bb.max_y - bb.min_y) * f1 as f64;
                let d7 = bb.min_z + (bb.max_z - bb.min_z) * f2 as f64;
                if sim::raytrace::ray_trace_blocks(&w, DVec3::new(d5 + d3, d6, d7 + d4), vec, false, false).is_none() {
                    j2 += 1;
                }
                k2 += 1;
                f2 = (f2 as f64 + d2) as f32;
            }
            f1 = (f1 as f64 + d1) as f32;
        }
        f = (f as f64 + d0) as f32;
    }
    j2 as f32 / k2 as f32
}

/// One explosion: `Explosion` with the exploder kept as its damage-source record.
pub struct Explosion {
    pub pos: [f64; 3],
    pub size: f32,
    /// `Explosion.exploder` as `getExplosivePlacedBy` and `causeExplosionDamage` see it.
    pub exploder: Option<Attacker>,
    /// `Explosion.damagesTerrain`
    pub damages_terrain: bool,
    /// `affectedBlockPositions`
    pub affected: Vec<(i32, i32, i32)>,
    /// `playerKnockbackMap`: the push on each player, by session id.
    pub knockback: Vec<(u32, [f64; 3])>,
}

impl Explosion {
    pub fn new(pos: [f64; 3], size: f32, exploder: Option<Attacker>, damages_terrain: bool) -> Explosion {
        Explosion { pos, size, exploder, damages_terrain, affected: Vec::new(), knockback: Vec::new() }
    }

    /// `DamageSource.causeExplosionDamage(explosion)`: "explosion.player" with the exploder, else
    /// "explosion"; difficulty scaled.
    fn damage_source(&self) -> DamageSource {
        match &self.exploder {
            Some(a) => DamageSource { kind: "explosion.player", ..DamageSource::mob(a.clone()) },
            None => DamageSource { kind: "explosion", unblockable: false, difficulty_scaled: true, ..DamageSource::generic() },
        }
    }

    /// `Explosion.doExplosionA`
    pub fn do_explosion_a(&mut self, game: &mut Game) {
        let mut rand = std::mem::replace(&mut game.items.rand, JavaRandom::new(0));
        let mut set: std::collections::HashSet<(i32, i32, i32)> = std::collections::HashSet::new();
        for j in 0..16 {
            for k in 0..16 {
                for l in 0..16 {
                    if j == 0 || j == 15 || k == 0 || k == 15 || l == 0 || l == 15 {
                        let mut d0 = (j as f32 / 15.0 * 2.0 - 1.0) as f64;
                        let mut d1 = (k as f32 / 15.0 * 2.0 - 1.0) as f64;
                        let mut d2 = (l as f32 / 15.0 * 2.0 - 1.0) as f64;
                        let d3 = (d0 * d0 + d1 * d1 + d2 * d2).sqrt();
                        d0 /= d3;
                        d1 /= d3;
                        d2 /= d3;
                        let mut f = self.size * (0.7 + rand.next_float() * 0.6);
                        let (mut d4, mut d6, mut d8) = (self.pos[0], self.pos[1], self.pos[2]);
                        while f > 0.0 {
                            let at = (floor(d4), floor(d6), floor(d8));
                            let s = terrain::state(game, at);
                            if props(s).mat != Mat::Air {
                                // Entity.getExplosionResistance(...) is the block's.
                                f -= (block_resistance(s) + 0.3) * 0.3;
                            }
                            // canExplosionDestroyBlock is true for an EntityCreeper.
                            if f > 0.0 {
                                set.insert(at);
                            }
                            d4 += d0 * 0.300_000_011_920_928_96;
                            d6 += d1 * 0.300_000_011_920_928_96;
                            d8 += d2 * 0.300_000_011_920_928_96;
                            f -= 0.225_000_01;
                        }
                    }
                }
            }
        }
        game.items.rand = rand;
        let mut affected: Vec<_> = set.into_iter().collect();
        affected.sort_unstable();
        self.affected = affected;

        let f3 = self.size * 2.0;
        let (x, y, z) = (self.pos[0], self.pos[1], self.pos[2]);
        let (f3d, w) = (f3 as f64, Aabb::new(x - f3 as f64 - 1.0, y - f3 as f64 - 1.0, z - f3 as f64 - 1.0, x + f3 as f64 + 1.0, y + f3 as f64 + 1.0, z + f3 as f64 + 1.0));
        let vec = DVec3::new(x, y, z);
        // getEntitiesWithinAABBExcludingEntity: mobs first (spawn order), then players.
        struct Target {
            id: u32,
            is_player: bool,
            pos: [f64; 3],
            eye: f64,
            bb: Aabb,
            creative: bool,
        }
        let mut targets: Vec<Target> = Vec::new();
        for m in game.mobs.mobs.iter().filter(|m| !m.dead && m.body.bb.intersects(&w)) {
            let p = m.body.pos;
            targets.push(Target { id: m.id, is_player: false, pos: [p.x, p.y, p.z], eye: m.eye_height() as f64, bb: m.body.bb, creative: false });
        }
        let mut ids: Vec<u32> = game.sessions.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let s = &game.sessions[&id];
            let Some(st) = s.state else { continue };
            if s.mode == proto::GM_SPECTATOR || game.players.players.get(&id).is_none_or(|p| p.dead) {
                continue;
            }
            let bb = players::player_bb(st.pos);
            if bb.intersects(&w) {
                let sneak = st.flags & proto::SNEAKING != 0;
                targets.push(Target { id, is_player: true, pos: st.pos, eye: players::eye_height(sneak) as f64, bb, creative: s.mode == proto::GM_CREATIVE && st.flags & proto::FLYING != 0 });
            }
        }
        let src = self.damage_source();
        for t in targets {
            // Entity.getDistance(x, y, z)
            let d12 = ((t.pos[0] - x).powi(2) + (t.pos[1] - y).powi(2) + (t.pos[2] - z).powi(2)).sqrt() / f3d;
            if d12 > 1.0 {
                continue;
            }
            let (mut d5, mut d7, mut d9) = (t.pos[0] - x, t.pos[1] + t.eye - y, t.pos[2] - z);
            let d13 = ((d5 * d5 + d7 * d7 + d9 * d9) as f32).sqrt() as f64;
            if d13 == 0.0 {
                continue;
            }
            d5 /= d13;
            d7 /= d13;
            d9 /= d13;
            let d14 = get_block_density(game, vec, &t.bb) as f64;
            let d10 = (1.0 - d12) * d14;
            let amount = ((d10 * d10 + d10) / 2.0 * 7.0 * f3d + 1.0) as i32 as f32;
            if t.is_player {
                damage::attack_entity_from(game, t.id, &src, amount);
                // The player is not a living thing the blast-protection reduction touches here.
                if !t.creative {
                    self.knockback.push((t.id, [d5 * d10, d7 * d10, d9 * d10]));
                }
            } else {
                combat::hurt_mob(game, t.id, &src, amount);
                // entity.motionX += d5 * d11, with d11 = d10 (the blast reduction is DROPPED).
                if let Some(m) = game.mobs.mobs.iter_mut().find(|m| m.id == t.id) {
                    m.body.motion.x += d5 * d10;
                    m.body.motion.y += d7 * d10;
                    m.body.motion.z += d9 * d10;
                    m.velocity_changed = true;
                }
            }
        }
    }

    /// `Explosion.doExplosionB(false)`: the blocks go and some drop.
    pub fn do_explosion_b(&mut self, game: &mut Game) {
        if !self.damages_terrain {
            return;
        }
        for &at in &self.affected {
            let s = terrain::state(game, at);
            if props(s).mat != Mat::Air {
                // canDropFromExplosion is true: dropBlockAsItemWithChance(.., 1 / size, 0).
                let stacks = items::drops::drop_block_as_item_with_chance(s, 1.0 / self.size, 0, &mut game.items.rand).stacks();
                for stack in stacks {
                    item_entities::spawn_as_entity(game, at, stack);
                }
                terrain::set_block(game, at, 0);
                // block.onExplosionDestroy is DROPPED (primed TNT).
            }
        }
    }
}

/// `World.newExplosion(exploder, x, y, z, size, causesFire = false, damagesTerrain)`: both halves,
/// then `SPacketExplosion` to every player within 64 blocks, carrying that player's push.
pub fn create_explosion(game: &mut Game, exploder: Option<Attacker>, pos: [f64; 3], size: f32, damages_terrain: bool) -> Explosion {
    let mut e = Explosion::new(pos, size, exploder, damages_terrain);
    e.do_explosion_a(game);
    e.do_explosion_b(game);
    let mut ids: Vec<u32> = game.sessions.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let Some(st) = game.sessions[&id].state else { continue };
        let d = (st.pos[0] - pos[0]).powi(2) + (st.pos[1] - pos[1]).powi(2) + (st.pos[2] - pos[2]).powi(2);
        if d < 4096.0 {
            let motion = e.knockback.iter().find(|(i, _)| *i == id).map_or([0.0; 3], |(_, m)| m.map(|v| v as f32));
            let body = proto::Explosion { pos, strength: size, motion }.encode();
            net::send(game, id, proto::S_EXPLOSION, &body);
        }
    }
    e
}
