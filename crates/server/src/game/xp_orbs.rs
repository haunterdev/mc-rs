//! `EntityXPOrb` on the server: spawn (`getXPSplit`), the per-tick physics of `sim::xp_orb`,
//! the pickup by a player (`onCollideWithPlayer`: `xpCooldown`, `S_COLLECT`, `addExperience`),
//! the age 6000 despawn, and the `Tracked` view the entity tracker reads (range 64, interval 20,
//! velocity on, from `sim::kinds`). The sources are the mob death (`mobs::tick`), breaking an ore
//! ([`drop_xp_on_block_break`]), taking a furnace output ([`furnace_orbs`]), breeding, and a
//! player's death (`xp.rs`).
//!
//! The pickup sound is the viewer's, on the `S_COLLECT` frame (`NetHandlerPlayClient.
//! handleCollectItem`).
//!
//! DROPPED: Mending in `onCollideWithPlayer` (no enchantments yet) and saving orbs with the world
//! (they are gone after a restart).

use items::ItemStack;
use proto::{ObjectData, SpawnObject};
use sim::collision::{floor, BlockAccess};
use sim::kinds::{Kind, XP_ORB};
use sim::xp_orb::{xp_split, Target, XpOrb};
use worldgen::java_random::JavaRandom;

use super::terrain::{self, Blocks};
use super::tracker::Tracked;
use super::{items as item_entities, net, Game};

pub struct State {
    /// Every orb, in spawn order.
    pub orbs: Vec<XpOrb>,
    /// Beside `orbs`: the entity id.
    pub ids: Vec<u32>,
    /// `Math.random()` for the furnace's fractional share, and the seeds of each orb's own `rand`.
    pub rand: JavaRandom,
}

impl Default for State {
    fn default() -> State {
        State { orbs: Vec::new(), ids: Vec::new(), rand: JavaRandom::new(crate::config::random_u64() as i64) }
    }
}

/// `World.spawnEntity(new EntityXPOrb(world, x, y, z, value))`. Returns the entity id.
pub fn spawn_orb(game: &mut Game, pos: [f64; 3], value: i32) -> u32 {
    let seed = game.xp_orbs.rand.next_long();
    let id = net::new_entity_id(game);
    game.xp_orbs.orbs.push(XpOrb::new(pos[0], pos[1], pos[2], value, JavaRandom::new(seed)));
    game.xp_orbs.ids.push(id);
    id
}

/// The `while (i > 0) { j = getXPSplit(i); i -= j; spawn }` loop every source runs.
pub fn spawn_orbs(game: &mut Game, pos: [f64; 3], mut xp: i32) {
    while xp > 0 {
        let j = xp_split(xp);
        xp -= j;
        spawn_orb(game, pos, j);
    }
}

/// `Block.dropXpOnBlockBreak`: orbs at the middle of the block, while `doTileDrops` is on.
pub fn drop_xp_on_block_break(game: &mut Game, at: (i32, i32, i32), amount: i32) {
    if amount > 0 && terrain::rule(game, "doTileDrops", true) {
        spawn_orbs(game, [at.0 as f64 + 0.5, at.1 as f64 + 0.5, at.2 as f64 + 0.5], amount);
    }
}

/// `SlotFurnaceOutput.onCrafting(stack)` for each take the player's window recorded: the share
/// of `FurnaceRecipes.getSmeltingExperience` for what was taken, as orbs at the player.
pub fn furnace_orbs(game: &mut Game, id: u32) {
    let Some(w) = game.windows.players.get_mut(&id) else { return };
    let events: Vec<(ItemStack, i32)> = w.active().furnace_xp.drain(..).collect();
    let Some(st) = game.sessions.get(&id).and_then(|s| s.state) else { return };
    for (stack, count) in events {
        let rand = &mut game.xp_orbs.rand;
        let xp = items::smelting::furnace_output_xp(&stack, count, || rand.next_double());
        spawn_orbs(game, [st.pos[0], st.pos[1] + 0.5, st.pos[2] + 0.5], xp);
    }
}

/// `EntityPlayer.getEyeHeight`
fn eye_height(flags: u8) -> f64 {
    if flags & proto::SNEAKING != 0 {
        1.54
    } else {
        1.62
    }
}

/// Every player picks up the orbs that touch it (`EntityPlayer.onLivingUpdate`,
/// `EntityXPOrb.onCollideWithPlayer`).
fn collide_with_players(game: &mut Game) {
    let mut ids: Vec<u32> = game.sessions.keys().copied().collect();
    ids.sort_unstable();
    for pid in ids {
        let Some(s) = game.sessions.get(&pid) else { continue };
        let Some(st) = s.state else { continue };
        if s.mode == proto::GM_SPECTATOR {
            continue;
        }
        let Some(p) = game.players.players.get_mut(&pid) else { continue };
        if p.dead || p.health <= 0.0 {
            continue;
        }
        let area = item_entities::pickup_box(st.pos, st.flags);
        let mut taken: Vec<(u32, i32)> = Vec::new();
        for (i, e) in game.xp_orbs.orbs.iter_mut().enumerate() {
            if e.dead || !e.body.bb.intersects(&area) {
                continue;
            }
            if e.delay_before_can_pickup == 0 && p.xp_cooldown == 0 {
                p.xp_cooldown = 2;
                taken.push((game.xp_orbs.ids[i], e.value));
                e.dead = true;
            }
        }
        for (orb, value) in taken {
            // EntityLivingBase.onItemPickup(orb, 1): SPacketCollectItem to the trackers and the collector.
            let msg = proto::Collect { item: orb, collector: pid, count: 1 }.encode();
            super::tracker::send_to_tracking(game, orb, proto::S_COLLECT, &msg);
            net::send(game, pid, proto::S_COLLECT, &msg);
            // DROPPED: Mending.
            if value > 0 {
                super::xp::add_experience(game, pid, value);
            }
        }
    }
}

/// One tick of every orb: `World.updateEntities` for `EntityXPOrb`, after the dead players' orbs
/// and the pickups.
pub fn tick(game: &mut Game) {
    super::xp::tick_dead(game);
    collide_with_players(game);
    let targets: Vec<Target> = game
        .sessions
        .iter()
        .filter_map(|(&id, s)| s.state.map(|st| Target { id, pos: st.pos, eye_height: eye_height(st.flags), spectator: s.mode == proto::GM_SPECTATOR }))
        .collect();
    let mut orbs = std::mem::take(&mut game.xp_orbs.orbs);
    let mut sounds: Vec<([f64; 3], f32)> = Vec::new();
    for (i, e) in orbs.iter_mut().enumerate() {
        if e.dead {
            continue;
        }
        let blocks = Blocks(game);
        // Entities in unloaded chunks are not updated.
        if !blocks.loaded(floor(e.body.pos.x), floor(e.body.pos.z)) {
            continue;
        }
        e.tick(game.xp_orbs.ids[i], &blocks, &targets, false);
        let at = [e.body.pos.x, e.body.pos.y, e.body.pos.z];
        sounds.extend(e.sounds.drain(..).map(|pitch| (at, pitch)));
        e.body.sounds.clear();
    }
    game.xp_orbs.orbs = orbs;
    for (pos, pitch) in sounds {
        let sound = proto::Sound { name: "entity.generic.burn".into(), category: 6, pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32], volume: 0.4, pitch };
        net::send_near(game, None, pos, 16.0, proto::S_SOUND, &sound.encode());
    }
    // World.updateEntities removes the dead.
    let mut keep = game.xp_orbs.orbs.iter().map(|e| !e.dead);
    game.xp_orbs.ids.retain(|_| keep.next().unwrap_or(true));
    game.xp_orbs.orbs.retain(|e| !e.dead);
}

/// What the tracker reads off one orb.
pub struct View<'a> {
    pub id: u32,
    pub e: &'a XpOrb,
}

pub fn views(state: &State) -> Vec<View<'_>> {
    state.orbs.iter().enumerate().map(|(i, e)| View { id: state.ids[i], e }).collect()
}

impl Tracked for View<'_> {
    fn id(&self) -> u32 {
        self.id
    }
    fn kind(&self) -> &'static Kind {
        &XP_ORB
    }
    fn pos(&self) -> [f64; 3] {
        let p = self.e.body.pos;
        [p.x, p.y, p.z]
    }
    fn rotation(&self) -> [f32; 3] {
        [self.e.body.yaw, self.e.body.pitch, 0.0]
    }
    fn on_ground(&self) -> bool {
        self.e.body.on_ground
    }
    fn motion(&self) -> [f64; 3] {
        let m = self.e.body.motion;
        [m.x, m.y, m.z]
    }
    fn spawn_body(&self) -> Vec<u8> {
        let p = self.e.body.pos;
        let m = self.e.body.motion;
        SpawnObject {
            id: self.id,
            pos: [p.x, p.y, p.z],
            yaw: self.e.body.yaw,
            pitch: self.e.body.pitch,
            motion: [m.x as f32, m.y as f32, m.z as f32],
            data: ObjectData::XpOrb { value: self.e.value.clamp(0, u16::MAX as i32) as u16 },
        }
        .encode()
    }
    fn air_borne(&self) -> bool {
        self.e.is_air_borne
    }
    fn velocity_changed(&self) -> bool {
        self.e.velocity_changed
    }
}

/// The tracker has run: `isAirBorne` and `velocityChanged` are cleared for every orb.
pub fn after_tracker(game: &mut Game) {
    for e in &mut game.xp_orbs.orbs {
        e.is_air_borne = false;
        e.velocity_changed = false;
    }
}
