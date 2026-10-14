//! The survival frames on the wire: [`dispatch`] turns each S_ frame link.rs left in the
//! [`Inbox`] into its typed message, and the `send_*` helpers put the V_ messages on the
//! [`Link`]. `V_CLIENT_INFO` goes once per connection, straight after the welcome (link.rs
//! `Link::connect` for a streamed session, join.rs `open` for a native one), so the server knows
//! this viewer reads survival frames.

// The send helpers are called by the tasks that add the actions (T4 respawn, T5 drop and swap,
// T6 dig, T7 clicks, T10 attack and interact).
#![allow(dead_code)]

use bevy::prelude::*;

use super::*;
use super::EntityEvent;
use crate::link::Link;

/// `V_CLIENT_INFO` with this viewer's survival protocol, as a whole frame.
pub fn client_info_frame() -> Vec<u8> {
    proto::frame(proto::V_CLIENT_INFO, &proto::ClientInfo { version: proto::SURVIVAL_PROTOCOL }.encode())
}

/// Queues `V_CLIENT_INFO` on a link. Called once, by `Link::connect`.
pub fn send_client_info(link: &Link) {
    link.send(proto::V_CLIENT_INFO, &proto::ClientInfo { version: proto::SURVIVAL_PROTOCOL }.encode());
}

pub fn send_dig(link: &Link, m: &proto::Dig) {
    link.send(proto::V_DIG, &m.encode());
}

pub fn send_drop(link: &Link, m: &proto::DropItem) {
    link.send(proto::V_DROP, &m.encode());
}

pub fn send_held(link: &Link, m: &proto::HeldSlot) {
    link.send(proto::V_HELD, &m.encode());
}

pub fn send_use_item(link: &Link, m: &proto::UseItem) {
    link.send(proto::V_USE_ITEM, &m.encode());
}

pub fn send_use_on(link: &Link, m: &proto::UseOn) {
    link.send(proto::V_USE_ON, &m.encode());
}

pub fn send_attack(link: &Link, m: &proto::Attack) {
    link.send(proto::V_ATTACK, &m.encode());
}

pub fn send_interact(link: &Link, m: &proto::Interact) {
    link.send(proto::V_INTERACT, &m.encode());
}

pub fn send_click(link: &Link, m: &proto::Click) {
    link.send(proto::V_CLICK, &m.encode());
}

pub fn send_close(link: &Link, m: &proto::CloseWindow) {
    link.send(proto::V_CLOSE, &m.encode());
}

pub fn send_creative_slot(link: &Link, m: &proto::CreativeSlot) {
    link.send(proto::V_CREATIVE_SLOT, &m.encode());
}

/// A stack as `V_CREATIVE_SLOT` carries it, the tag as [`proto::WireStack`]'s three bytes.
pub fn wire_stack(s: &::items::ItemStack) -> proto::WireStack {
    if s.is_empty() {
        return proto::WireStack::EMPTY;
    }
    let tag = match s.tag {
        ::items::Tag::None => [0, 0, 0],
        ::items::Tag::Potion(p) => [1, p, 0],
        ::items::Tag::StoredEnchant { id, level } => [2, id, level],
        ::items::Tag::Entity(e) => [3, e, 0],
    };
    proto::WireStack { tag, ..proto::WireStack::new(s.item as i16, s.count.clamp(0, 255) as u8, s.damage.clamp(0, u16::MAX as i32) as u16) }
}

pub fn send_respawn(link: &Link) {
    link.send(proto::V_RESPAWN, &proto::encode_empty());
}

pub fn send_swap_hands(link: &Link) {
    link.send(proto::V_SWAP_HANDS, &proto::encode_empty());
}

/// Decodes the frames in the [`Inbox`] into their messages. A frame that does not read, or a
/// survival tag this viewer does not know (a newer server), is logged and skipped.
pub fn dispatch(world: &mut World) {
    let frames = std::mem::take(&mut world.resource_mut::<Inbox>().0);
    for (tag, body) in frames {
        if decode_into(world, tag, &body).is_none() {
            warn!("survival frame {tag} ({} bytes) did not read", body.len());
        }
    }
}

/// Writes the message for one frame; `None` when it does not decode.
fn decode_into(world: &mut World, tag: u8, b: &[u8]) -> Option<()> {
    match tag {
        proto::S_HEALTH => put(world, proto::Health::decode(b).map(Health)),
        proto::S_SIGN => put(world, proto::SignText::decode(b).map(crate::sign::SignUpdate)),
        proto::S_XP => put(world, proto::Xp::decode(b).map(Xp)),
        proto::S_DEATH => put(world, proto::Death::decode(b).map(Death)),
        proto::S_HELD => put(world, proto::HeldSlot::decode(b).map(Held)),
        proto::S_SLOTS => put(world, proto::WindowSlots::decode(b).map(Slots)),
        proto::S_CONFIRM => put(world, proto::Confirm::decode(b).map(Confirm)),
        proto::S_OPEN_WINDOW => put(world, proto::OpenWindow::decode(b).map(OpenWindow)),
        proto::S_CLOSE_WINDOW => put(world, proto::CloseWindow::decode(b).map(CloseWindow)),
        proto::S_WINDOW_PROP => put(world, proto::WindowProp::decode(b).map(WindowProp)),
        proto::S_SPAWN_OBJECT => put(world, proto::SpawnObject::decode(b).map(SpawnObject)),
        proto::S_SPAWN_MOB => put(world, proto::SpawnMob::decode(b).map(SpawnMob)),
        proto::S_ENTITY_MOVE => put(world, proto::decode_entity_moves(b).map(EntityMoves)),
        proto::S_VELOCITY => put(world, proto::Velocity::decode(b).map(Velocity)),
        proto::S_DESTROY => put(world, proto::decode_destroy(b).map(Destroy)),
        proto::S_ENTITY_EVENT => put(world, proto::EntityEvent::decode(b).map(EntityEvent)),
        proto::S_ENTITY_META => put(world, proto::EntityMeta::decode(b).map(EntityMeta)),
        proto::S_ANIMATION => put(world, proto::Animation::decode(b).map(Animation)),
        proto::S_EQUIPMENT => put(world, proto::Equipment::decode(b).map(Equipment)),
        proto::S_COLLECT => put(world, proto::Collect::decode(b).map(Collect)),
        proto::S_BREAK_ANIM => put(world, proto::BreakAnim::decode(b).map(BreakAnim)),
        proto::S_BLOCK_ACTION => put(world, proto::BlockAction::decode(b).map(BlockAction)),
        proto::S_SOUND => put(world, proto::Sound::decode(b).map(Sound)),
        proto::S_EXPLOSION => put(world, proto::Explosion::decode(b).map(Explosion)),
        _ => None,
    }
}

fn put<M: Message>(world: &mut World, m: Option<M>) -> Option<()> {
    world.write_message(m?);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::Messages;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(SurvivalPlugin);
        app
    }

    fn got<M: Message + Clone>(app: &App) -> Vec<M> {
        app.world().resource::<Messages<M>>().iter_current_update_messages().cloned().collect()
    }

    #[test]
    fn survival_frames_become_messages() {
        let mut app = app();
        let health = proto::Health { health: 7.5, food: 12, saturation: 1.5, air: 120, flags: proto::HEALTH_BURNING };
        let xp = proto::Xp { bar: 0.25, level: 3, total: 40 };
        let moves = vec![
            proto::EntityMove { id: 4, pos: [1.0, 64.0, -2.5], yaw: 90.0, pitch: 0.0, head_yaw: 45.0, on_ground: true },
            proto::EntityMove { id: 5, pos: [0.0, 70.0, 0.0], ..Default::default() },
        ];
        let vel = proto::Velocity { id: 4, motion: [0.0, 0.42, 0.0] };
        app.world_mut().resource_mut::<Inbox>().0.extend([
            (proto::S_HEALTH, health.encode()),
            (proto::S_XP, xp.encode()),
            (proto::S_HELD, proto::HeldSlot { slot: 6 }.encode()),
            (proto::S_ENTITY_MOVE, proto::encode_entity_moves(&moves)),
            (proto::S_VELOCITY, vel.encode()),
            (proto::S_DESTROY, proto::encode_destroy(&[4, 9])),
            // A broken frame and an unknown survival tag are skipped.
            (proto::S_HEALTH, vec![1, 2]),
            (219, vec![]),
        ]);
        app.update();
        assert_eq!(got::<Health>(&app), [Health(health)]);
        assert_eq!(got::<Xp>(&app), [Xp(xp)]);
        assert_eq!(got::<Held>(&app), [Held(proto::HeldSlot { slot: 6 })]);
        assert_eq!(got::<EntityMoves>(&app), [EntityMoves(moves)]);
        assert_eq!(got::<Velocity>(&app), [Velocity(vel)]);
        assert_eq!(got::<Destroy>(&app), [Destroy(vec![4, 9])]);
        assert!(got::<Death>(&app).is_empty());
        assert!(app.world().resource::<Inbox>().0.is_empty());
        let s = app.world().resource::<SurvivalState>();
        assert_eq!((s.health, s.food, s.saturation, s.air, s.burning, s.xp, s.dead), (7.5, 12, 1.5, 120, true, xp, false));
    }

    #[test]
    fn death_holds_until_health_returns() {
        let mut app = app();
        assert_eq!(*app.world().resource::<SurvivalState>(), SurvivalState::default());
        let dead = proto::Health { health: 0.0, food: 20, saturation: 5.0, air: 300, flags: 0 };
        let death = proto::Death { message: "xy fell from a high place".into() };
        app.world_mut().resource_mut::<Inbox>().0.extend([(proto::S_HEALTH, dead.encode()), (proto::S_DEATH, death.encode())]);
        app.update();
        assert_eq!(got::<Death>(&app), [Death(death)]);
        assert!(app.world().resource::<SurvivalState>().dead);
        // Another zero-health update keeps the death screen.
        app.world_mut().resource_mut::<Inbox>().0.push((proto::S_HEALTH, dead.encode()));
        app.update();
        assert!(app.world().resource::<SurvivalState>().dead);
        let alive = proto::Health { health: 20.0, ..dead };
        app.world_mut().resource_mut::<Inbox>().0.push((proto::S_HEALTH, alive.encode()));
        app.update();
        let s = app.world().resource::<SurvivalState>();
        assert!(!s.dead);
        assert_eq!(s.health, 20.0);
    }

    /// A stand-in server: the client says hello, gets its welcome, then the first frame it sends
    /// is `V_CLIENT_INFO`, and only once.
    #[test]
    fn a_streamed_session_says_its_survival_version_once() {
        use std::io::{BufReader, BufWriter, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
            let mut r = BufReader::new(tcp.try_clone().unwrap());
            let mut w = BufWriter::new(tcp);
            assert_eq!(proto::read_frame(&mut r).unwrap(), (proto::V_HELLO, b"tok".to_vec()));
            proto::write_frame(&mut w, proto::S_WELCOME, br#"{"id":2,"name":"xy","time":0}"#).unwrap();
            w.flush().unwrap();
            let first = proto::read_frame(&mut r).unwrap();
            let second = proto::read_frame(&mut r).unwrap();
            (first, second)
        });
        let video = crate::encode::VideoConfig { width: 64, height: 64, fps: 20, encoder: "libx264".into(), bitrate_kbps: 100 };
        let link = Link::connect(&crate::link::StreamArgs { addr, session: "tok".into(), video }).unwrap();
        link.send(proto::V_STATE, &proto::PlayerState::default().encode());
        let ((t1, b1), (t2, _)) = server.join().unwrap();
        assert_eq!(t1, proto::V_CLIENT_INFO);
        assert_eq!(proto::ClientInfo::decode(&b1), Some(proto::ClientInfo { version: proto::SURVIVAL_PROTOCOL }));
        assert_eq!(t2, proto::V_STATE);
    }

    #[test]
    fn the_client_info_frame_reads_back() {
        let f = client_info_frame();
        let frames = proto::split_frames(&f).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].0, proto::V_CLIENT_INFO);
        assert_eq!(proto::ClientInfo::decode(frames[0].1), Some(proto::ClientInfo { version: proto::SURVIVAL_PROTOCOL }));
    }
}
