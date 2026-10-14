//! Tests for the entity tracker with a dummy entity.

use super::*;
use sim::kinds::{ARROW, COW, ITEM};

#[derive(Clone)]
struct Dummy {
    id: u32,
    kind: &'static Kind,
    pos: [f64; 3],
    rot: [f32; 3],
    motion: [f64; 3],
    on_ground: bool,
}

impl Tracked for Dummy {
    fn id(&self) -> u32 {
        self.id
    }
    fn kind(&self) -> &'static Kind {
        self.kind
    }
    fn pos(&self) -> [f64; 3] {
        self.pos
    }
    fn rotation(&self) -> [f32; 3] {
        self.rot
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    fn motion(&self) -> [f64; 3] {
        self.motion
    }
    fn spawn_body(&self) -> Vec<u8> {
        self.id.to_le_bytes().to_vec()
    }
}

fn dummy(id: u32, kind: &'static Kind, x: f64, z: f64) -> Dummy {
    Dummy { id, kind, pos: [x, 64.0, z], rot: [0.0; 3], motion: [0.0; 3], on_ground: true }
}

fn tick(t: &mut EntityTracker, ents: &[Dummy], players: &[(u32, [f64; 3])]) -> BTreeMap<u32, Vec<(u8, Vec<u8>)>> {
    let refs: Vec<&dyn Tracked> = ents.iter().map(|e| e as &dyn Tracked).collect();
    t.tick(&refs, players)
}

fn tags(out: &BTreeMap<u32, Vec<(u8, Vec<u8>)>>, p: u32) -> Vec<u8> {
    out.get(&p).map(|f| f.iter().map(|x| x.0).collect()).unwrap_or_default()
}

/// The entity ids in player `p`'s move batch.
fn moved(out: &BTreeMap<u32, Vec<(u8, Vec<u8>)>>, p: u32) -> Vec<EntityMove> {
    out.get(&p)
        .into_iter()
        .flatten()
        .filter(|f| f.0 == proto::S_ENTITY_MOVE)
        .flat_map(|f| proto::decode_entity_moves(&f.1).unwrap())
        .collect()
}

fn at(x: f64, z: f64) -> [f64; 3] {
    [x, 64.0, z]
}

#[test]
fn range_is_the_kinds_square_clamped_to_the_view_distance() {
    let mut t = EntityTracker::new(8);
    // The item's square is 64 blocks: (50, 50) is inside although 70 blocks away.
    let ents = [dummy(1_000_000, &ITEM, 50.0, 50.0), dummy(1_000_001, &COW, 150.0, 0.0)];
    let out = tick(&mut t, &ents, &[(1, at(0.0, 0.0)), (2, at(200.0, 0.0))]);
    assert_eq!(tags(&out, 1), vec![proto::S_SPAWN_OBJECT, proto::S_VELOCITY], "items send velocity after the spawn");
    assert_eq!(tags(&out, 2), vec![proto::S_SPAWN_MOB], "a mob's spawn carries its motion");
    assert_eq!(t.tracking(1_000_000), vec![1]);
    assert_eq!(t.tracking(1_000_001), vec![2]);
    // A view distance of 2 caps every range at 16 blocks.
    let mut t = EntityTracker::new(2);
    let out = tick(&mut t, &[dummy(1_000_000, &ITEM, 20.0, 0.0)], &[(1, at(0.0, 0.0))]);
    assert!(out.is_empty());
    let out = tick(&mut t, &[dummy(1_000_000, &ITEM, 20.0, 0.0)], &[(1, at(5.0, 0.0))]);
    assert_eq!(tags(&out, 1), vec![proto::S_SPAWN_OBJECT, proto::S_VELOCITY], "a player 5 blocks on checks again");
}

#[test]
fn moves_go_out_on_the_kinds_interval() {
    let mut t = EntityTracker::new(8);
    let mut item = dummy(1_000_000, &ITEM, 10.0, 0.0);
    item.motion = [0.05, 0.0, 0.0];
    let mut cow = dummy(1_000_001, &COW, -10.0, 0.0);
    let players = [(1, at(0.0, 0.0))];
    let (mut item_moves, mut cow_moves, mut velocities) = (Vec::new(), Vec::new(), Vec::new());
    for n in 0..=60 {
        if n == 25 {
            item.motion = [0.0, 0.5, 0.0];
        }
        let out = tick(&mut t, &[item.clone(), cow.clone()], &players);
        for m in moved(&out, 1) {
            if m.id == item.id {
                item_moves.push((n, m));
            } else {
                cow_moves.push(n);
            }
        }
        if n > 0 && tags(&out, 1).contains(&proto::S_VELOCITY) {
            velocities.push(n);
        }
        item.pos[0] += 0.05;
        cow.pos[2] += 0.2;
        cow.rot[2] = n as f32 * 3.0;
    }
    // Every 20 ticks, never on the first.
    assert_eq!(item_moves.iter().map(|m| m.0).collect::<Vec<_>>(), vec![20, 40, 60]);
    // The position the client ends at is the encoded one: 1/4096 of a block under the real one.
    let (_, m) = item_moves[0];
    assert!((m.pos[0] - (10.0 + 20.0 * 0.05)).abs() < 1.0 / 4096.0 + 1e-9, "{:?}", m.pos);
    // Mobs every 3 ticks.
    assert_eq!(cow_moves, (1..=20).map(|i| i * 3).collect::<Vec<_>>());
    // The motion change at tick 25 goes out at the next interval.
    assert_eq!(velocities, vec![40]);
}

#[test]
fn a_far_jump_teleports_and_an_arrow_moves_from_the_first_tick() {
    let mut t = EntityTracker::new(8);
    let mut item = dummy(1_000_000, &ITEM, 10.0, 0.0);
    let arrow = dummy(1_000_001, &ARROW, 0.0, 10.0);
    let players = [(1, at(0.0, 0.0))];
    let out = tick(&mut t, &[item.clone(), arrow.clone()], &players);
    assert_eq!(moved(&out, 1).iter().map(|m| m.id).collect::<Vec<_>>(), vec![arrow.id], "S17 for an arrow at counter 0");
    for _ in 1..20 {
        tick(&mut t, &[item.clone(), arrow.clone()], &players);
    }
    // 9.5 blocks is past the 32768 units a relative move holds.
    item.pos[0] = 19.5;
    let out = tick(&mut t, &[item.clone(), arrow.clone()], &players);
    let m = moved(&out, 1).into_iter().find(|m| m.id == item.id).unwrap();
    assert_eq!(m.pos, item.pos, "a teleport carries the exact position");
}

#[test]
fn destroys_on_leaving_range_and_on_removal() {
    let mut t = EntityTracker::new(8);
    let item = dummy(1_000_000, &ITEM, 10.0, 0.0);
    let cow = dummy(1_000_001, &COW, -10.0, 0.0);
    let both = [item.clone(), cow.clone()];
    tick(&mut t, &both, &[(1, at(0.0, 0.0)), (2, at(0.0, 5.0))]);
    // Player 2 walks 70 blocks away: past the item's 64, inside the cow's 80.
    let out = tick(&mut t, &both, &[(1, at(0.0, 0.0)), (2, at(0.0, 75.0))]);
    assert_eq!(out.get(&2).unwrap().last().unwrap(), &(proto::S_DESTROY, proto::encode_destroy(&[item.id])));
    assert!(!out.contains_key(&1));
    // The cow is removed: everyone tracking it hears.
    let out = tick(&mut t, &[item.clone()], &[(1, at(0.0, 0.0)), (2, at(0.0, 75.0))]);
    for p in [1, 2] {
        assert_eq!(out.get(&p).unwrap().last().unwrap(), &(proto::S_DESTROY, proto::encode_destroy(&[cow.id])), "player {p}");
    }
    // A player who leaves is dropped without a frame.
    let out = tick(&mut t, &[item.clone()], &[(2, at(0.0, 75.0))]);
    assert!(out.is_empty());
    assert!(t.tracking(item.id).is_empty());
}
