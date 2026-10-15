//! Tests for commands.rs: parsing, selectors, permissions and the feedback each command gives.

use super::exec::{block_pos, coordinate, java_double, Coord};
use super::*;

struct T {
    dir: std::path::PathBuf,
    world: World,
    players: Vec<Who>,
    ops: Vec<String>,
    bans: crate::bans::Bans,
    ips: std::collections::HashMap<String, String>,
    /// "commands": "everyone"; off here, so the tests see what ops alone may do.
    open: bool,
}

impl T {
    fn new() -> T {
        let dir = std::env::temp_dir().join(format!("mcrs-cmd-{}", crate::config::random_u64()));
        let world = World::load(&dir).unwrap();
        let at = |pos: [f64; 3], yaw: f32| Some(PlayerState { pos, yaw, pitch: 10.0, ..Default::default() });
        let players = vec![
            Who { id: 1, name: "Xy".into(), state: at([10.25, 64.0, -3.75], 90.0) },
            Who { id: 2, name: "Bob".into(), state: at([100.5, 70.0, 100.5], -45.0) },
            Who { id: 3, name: "Cat".into(), state: at([20.0, 64.0, 0.0], 0.0) },
        ];
        T { dir, world, players, ops: vec!["xy".into()], bans: Default::default(), ips: [("bob".to_string(), "203.0.113.9".to_string())].into(), open: false }
    }

    /// Runs a line as player `id` (0 for the console) and returns what it did.
    fn run(&mut self, id: u32, line: &str) -> Vec<Effect> {
        let sender = match self.players.iter().find(|p| p.id == id) {
            Some(p) => Sender { id: Some(id), name: p.name.clone(), op: self.ops.iter().any(|o| o.eq_ignore_ascii_case(&p.name)) },
            None => Sender { id: None, name: "Server".into(), op: true },
        };
        let mut ctx =
            Ctx { world: &mut self.world, players: &self.players, ops: &mut self.ops, bans: &mut self.bans, ips: &self.ips, sender: &sender, open: self.open, seed: -42, max_players: 4, effects: Vec::new() };
        ctx.execute(line);
        ctx.effects
    }

    /// The text of every line the sender got back, red ones marked with a leading `!`.
    fn said(&mut self, id: u32, line: &str) -> Vec<String> {
        self.run(id, line)
            .into_iter()
            .filter_map(|e| match e {
                Effect::Feedback(l) => Some(if l.color == Some("red") { format!("!{}", l.text) } else { l.text }),
                _ => None,
            })
            .collect()
    }

    fn teleports(&mut self, id: u32, line: &str) -> Vec<(u32, Teleport)> {
        self.run(id, line)
            .into_iter()
            .filter_map(|e| match e {
                Effect::Teleport(id, t) => Some((id, t)),
                _ => None,
            })
            .collect()
    }
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn coordinates_center_whole_numbers_and_add_to_the_base() {
    let c = |base, arg, center| coordinate(base, arg, center, -30_000_000, 30_000_000);
    assert_eq!(c(7.0, "10", true), Ok(Coord { result: 10.5, amount: 10.5, relative: false }));
    assert_eq!(c(7.0, "10.0", true).unwrap().result, 10.0, "a dot means exactly there");
    assert_eq!(c(7.0, "10", false).unwrap().result, 10.0);
    assert_eq!(c(7.25, "~", true), Ok(Coord { result: 7.25, amount: 0.0, relative: true }));
    assert_eq!(c(7.25, "~-2", true), Ok(Coord { result: 5.25, amount: -2.0, relative: true }));
    assert_eq!(c(7.0, "~1.5", true).unwrap().result, 8.5);
    assert_eq!(c(0.0, "abc", true), Err("'abc' is not a valid number".into()));
    assert_eq!(c(0.0, "~x", true), Err("'x' is not a valid number".into()));
    assert_eq!(c(0.0, "NaN", true), Err("'NaN' is not a valid number".into()));
    assert_eq!(coordinate(0.0, "5000", false, -4096, 4096), Err("The number you have entered (5000.00) is too big, it must be at most 4096".into()));
    assert_eq!(block_pos([1, 64, 1], &["~", "~1", "-3"]), Ok([1, 65, -3]));
    assert_eq!(block_pos([1, 64, 1], &["0", "300", "0"]), Err("The number you have entered (300.00) is too big, it must be at most 256".into()));
    assert_eq!(java_double(64.0), "64.0");
    assert_eq!(java_double(-3.25), "-3.25");
}

#[test]
fn unknown_commands_and_permissions_answer_in_red() {
    let mut t = T::new();
    assert_eq!(t.said(1, "frobnicate"), vec!["!Unknown command. Try /help for a list of commands"]);
    assert_eq!(t.said(2, "time set day"), vec!["!You do not have permission to use this command"]);
    assert_eq!(t.said(2, "TIME set day"), vec!["!You do not have permission to use this command"]);
    // Anyone may list, whisper and emote.
    assert_eq!(t.said(2, "list"), vec!["There are 3/4 players online:", "Xy, Bob, Cat"]);
    assert_eq!(t.said(1, "time"), vec!["!Usage: /time <set|add|query> <value>"]);
    assert_eq!(t.said(1, "time set -5"), vec!["!The number you have entered (-5) is too small, it must be at least 0"]);
    assert_eq!(t.said(1, "time set dusk"), vec!["!'dusk' is not a valid number"]);
}

#[test]
fn time_sets_adds_and_queries() {
    let mut t = T::new();
    t.world.set_rule("doDaylightCycle", "false");
    let e = t.run(1, "time set night");
    assert_eq!(e, vec![Effect::Time, Effect::Feedback(Line::plain("Set the time to 13000"))]);
    assert_eq!(t.world.time(), 13000);
    assert_eq!(t.said(1, "time add 24500"), vec!["Added 24500 to the time"]);
    assert_eq!(t.said(1, "time query daytime"), vec!["Time is 13500"]);
    assert_eq!(t.said(1, "time query day"), vec!["Time is 1"]);
    assert_eq!(t.said(0, "time set noon"), vec!["Set the time to 6000"]);
    assert_eq!(t.said(1, "time query gametime").len(), 1);
    assert_eq!(t.said(1, "time query moon"), vec!["!Usage: /time <set|add|query> <value>"]);
}

#[test]
fn gamerules_list_query_set_and_check_booleans() {
    let mut t = T::new();
    assert!(t.said(1, "gamerule")[0].starts_with("announceAdvancements, commandBlockOutput, "));
    assert!(t.said(1, "gamerule")[0].ends_with(" and spectatorsGenerateChunks"));
    assert_eq!(t.said(1, "gamerule doDaylightCycle"), vec!["doDaylightCycle = true"]);
    assert_eq!(t.said(1, "gamerule nope"), vec!["!No game rule called 'nope' is available"]);
    assert_eq!(t.said(1, "gamerule doDaylightCycle maybe"), vec!["!'maybe' is not true or false"]);
    let e = t.run(1, "gamerule doDaylightCycle false");
    assert_eq!(e, vec![Effect::Time, Effect::Feedback(Line::plain("Game rule doDaylightCycle has been updated to false"))]);
    assert!(!t.world.clock.cycle());
    // Feedback off: success is silent, errors still show.
    t.run(1, "gamerule sendCommandFeedback false");
    assert_eq!(t.said(1, "time set 0"), Vec::<String>::new());
    assert_eq!(t.said(1, "time set x"), vec!["!'x' is not a valid number"]);
    assert_eq!(t.said(0, "time set 0"), vec!["Set the time to 0"], "the console always hears back");
}

#[test]
fn tp_keeps_relative_axes_relative_for_the_session() {
    let mut t = T::new();
    let tp = t.teleports(1, "tp ~ ~10 5");
    let flags = proto::TP_REL_X | proto::TP_REL_Y | proto::TP_REL_YAW | proto::TP_REL_PITCH;
    assert_eq!(tp, vec![(1, Teleport { pos: [0.0, 10.0, 5.5], yaw: 0.0, pitch: 0.0, flags })]);
    assert_eq!(t.said(1, "tp ~ ~10 5"), vec!["Teleported Xy to 10.25, 74.0, 5.5"]);
    // Another player, with an absolute rotation wrapped to -180..180.
    let tp = t.teleports(1, "tp Bob 1.5 70 2 270 ~");
    assert_eq!(tp, vec![(2, Teleport { pos: [1.5, 70.0, 2.5], yaw: -90.0, pitch: 0.0, flags: proto::TP_REL_PITCH })]);
    // To a player: their position and rotation, outright.
    let tp = t.teleports(1, "tp bob");
    assert_eq!(tp, vec![(1, Teleport { pos: [100.5, 70.0, 100.5], yaw: -45.0, pitch: 10.0, flags: 0 })]);
    assert_eq!(t.said(1, "tp Cat Bob"), vec!["Teleported Cat to Bob"]);
    assert_eq!(t.said(1, "tp Nobody"), vec!["!Player 'Nobody' cannot be found"]);
    assert_eq!(t.said(1, "tp 1 2"), vec!["!Player '1' cannot be found"]);
    assert_eq!(t.said(1, "tp 0 9999 0"), vec!["!The number you have entered (9999.00) is too big, it must be at most 4096"]);
    assert_eq!(t.said(0, "tp 0 64 0"), vec!["!You must specify which player you wish to perform this action on."]);
    assert!(t.said(1, "tp")[0].starts_with("!Usage: /tp [target player]"));
}

#[test]
fn teleport_is_relative_to_the_sender() {
    let mut t = T::new();
    let tp = t.teleports(1, "teleport Bob ~1 ~ ~");
    assert_eq!(tp, vec![(2, Teleport { pos: [11.25, 64.0, -3.75], yaw: 0.0, pitch: 0.0, flags: proto::TP_REL_YAW | proto::TP_REL_PITCH })]);
    assert_eq!(t.said(1, "teleport Bob"), vec!["!Usage: /teleport <entity> <x> <y> <z> [<y-rot> <x-rot>]"]);
}

#[test]
fn selectors_pick_players() {
    let mut t = T::new();
    // @a runs the command once per player.
    let tp = t.teleports(1, "tp @a 0 64 0");
    assert_eq!(tp.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![1, 2, 3]);
    // @p is the nearest to the sender, the sender included.
    assert_eq!(t.said(2, "tell @p hi"), vec!["!You can't send a private message to yourself!"]);
    assert_eq!(t.said(1, "spawnpoint @s"), vec!["Set Xy's spawn point to (10, 64, -4)"]);
    assert_eq!(t.said(0, "kill @s"), vec!["!Player '@s' cannot be found"]);
    assert_eq!(t.said(1, "gamemode 3 @x"), vec!["!Player '@x' cannot be found"]);
    assert_eq!(t.run(1, "tp @r 0 64 0").len(), 2, "one teleport and its feedback");
}

#[test]
fn gamemode_stores_sends_and_tells_the_player() {
    let mut t = T::new();
    let e = t.run(1, "gamemode spectator Bob");
    assert_eq!(
        e,
        vec![
            Effect::GameMode(2, proto::GM_SPECTATOR),
            Effect::Tell(2, Line::plain("Your game mode has been updated to Spectator Mode")),
            Effect::Feedback(Line::plain("Set Bob's game mode to Spectator Mode")),
        ]
    );
    assert_eq!(t.world.game_mode("bob"), proto::GM_SPECTATOR);
    assert_eq!(t.said(1, "gamemode 0"), vec!["Set own game mode to Survival Mode"]);
    assert_eq!(t.world.game_mode("xy"), proto::GM_SURVIVAL);
    assert_eq!(t.said(1, "gamemode c"), vec!["Set own game mode to Creative Mode"]);
    assert_eq!(t.said(1, "gamemode 4"), vec!["!The number you have entered (4) is too big, it must be at most 3"]);
    assert_eq!(t.said(1, "gamemode hard"), vec!["!'hard' is not a valid number"]);
}

#[test]
fn spawns_kill_and_respawn() {
    let mut t = T::new();
    assert_eq!(t.said(1, "setworldspawn"), vec!["Set the world spawn point to (10, 64, -4)"]);
    assert_eq!(t.said(1, "setworldspawn ~5 70 ~"), vec!["Set the world spawn point to (15, 70, -4)"]);
    assert_eq!(t.world.spawn, [15, 70, -4]);
    assert_eq!(t.said(1, "setworldspawn 1 2"), vec!["!Usage: /setworldspawn [<x> <y> <z>]"]);
    // No spawn point: the top block at the world spawn.
    let e = t.run(1, "kill Bob");
    assert_eq!(e[0], Effect::Kill(2, "Bob".into()));
    assert_eq!(e[1], Effect::Feedback(Line::plain("Killed Bob")));
    let surface = Teleport { pos: [15.5, 70.0, -3.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN | proto::TP_SURFACE };
    assert_eq!(respawn_point(&t.world, "Bob"), surface);
    // With one: exactly there.
    assert_eq!(t.said(1, "spawnpoint Bob 1 80 1"), vec!["Set Bob's spawn point to (1, 80, 1)"]);
    assert_eq!(respawn_point(&t.world, "Bob"), Teleport { pos: [1.5, 80.1, 1.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN });
    assert_eq!(t.said(1, "spawnpoint Bob 1"), vec!["!Usage: /spawnpoint [player] [<x> <y> <z>]"]);
}

#[test]
fn chat_commands_reach_the_right_people() {
    let mut t = T::new();
    assert_eq!(t.run(1, "say hello all"), vec![Effect::Broadcast("[Xy] hello all".into())]);
    assert_eq!(t.run(0, "say hi"), vec![Effect::Broadcast("[Server] hi".into())]);
    assert_eq!(t.run(2, "me waves"), vec![Effect::Broadcast("* Bob waves".into())]);
    let gray = |s: &str| Line { text: s.into(), color: Some("gray"), italic: true };
    assert_eq!(
        t.run(2, "w xy psst there"),
        vec![Effect::Tell(1, gray("Bob whispers to you: psst there")), Effect::Feedback(gray("You whisper to Xy: psst there"))]
    );
    assert_eq!(t.said(2, "msg xy"), vec!["!Usage: /tell <player> <private message ...>"]);
    assert_eq!(t.said(1, "seed"), vec!["Seed: -42"]);
}

#[test]
fn help_pages_list_what_the_sender_may_use() {
    let mut t = T::new();
    let page = t.said(2, "help");
    assert_eq!(
        page,
        vec!["--- Showing help page 1 of 1 (/help <page>) ---", "/help [page|command name]", "/list", "/me <action ...>", "/spawn [player]", "/tell <player> <private message ...>"]
    );
    let op = t.said(1, "help 2");
    assert_eq!(op[0], "--- Showing help page 2 of 5 (/help <page>) ---");
    assert_eq!(op.len(), 8);
    assert_eq!(t.said(1, "help 9"), vec!["!The number you have entered (9) is too big, it must be at most 5"]);
    assert_eq!(t.said(1, "help time"), vec!["!Usage: /time <set|add|query> <value>"]);
    assert_eq!(t.said(1, "help xyzzy"), vec!["!Unknown command. Try /help for a list of commands"]);
}

#[test]
fn ops_are_added_and_removed() {
    let mut t = T::new();
    assert_eq!(t.said(0, "op Bob"), vec!["Opped Bob"]);
    assert_eq!(t.said(2, "time set 0"), vec!["Set the time to 0"]);
    assert_eq!(t.run(2, "deop bob"), vec![Effect::Ops, Effect::Feedback(Line::plain("De-opped bob"))]);
    assert_eq!(t.said(1, "deop bob"), vec!["!Could not de-op bob"]);
    assert_eq!(t.said(1, "op a b"), vec!["!Usage: /op <player>"]);
    assert_eq!(t.said(1, "op x!"), vec!["!Could not op x!"]);
    assert_eq!(t.ops, vec!["xy".to_string()]);
}

#[test]
fn weather_is_kept() {
    let mut t = T::new();
    assert_eq!(t.said(1, "weather thunder 30"), vec!["Changing to rain and thunder", "Rain is not drawn yet; the weather is kept for when it is."]);
    assert_eq!(t.world.weather, crate::world::Weather { raining: true, thundering: true, duration: 600 });
    assert_eq!(t.said(1, "weather RAIN 5")[0], "Changing to rainy weather");
    assert_eq!(t.said(1, "weather clear"), vec!["Changing to clear weather"]);
    assert_eq!(t.said(1, "weather snow"), vec!["!Usage: /weather <clear|rain|thunder> [duration in seconds]"]);
    assert_eq!(t.said(1, "weather rain 0"), vec!["!The number you have entered (0) is too small, it must be at least 1"]);
}

#[test]
fn difficulty_takes_names_and_ids_and_is_kept() {
    let mut t = T::new();
    assert_eq!(t.run(1, "difficulty hard"), vec![Effect::Difficulty(3), Effect::Feedback(Line::plain("Set game difficulty to Hard"))]);
    assert_eq!(t.world.difficulty, Some(3));
    assert!(t.run(1, "difficulty 0").contains(&Effect::Difficulty(0)));
    assert!(t.run(1, "difficulty PEACEFUL").contains(&Effect::Difficulty(0)));
    assert_eq!(t.said(1, "difficulty 7"), vec!["!Usage: /difficulty <peaceful|easy|normal|hard>"]);
    assert_eq!(t.said(1, "difficulty"), vec!["!Usage: /difficulty <peaceful|easy|normal|hard>"]);
    assert!(t.said(2, "difficulty easy")[0].starts_with('!'), "a non-operator is refused");
    t.world.save();
    assert_eq!(World::load(&t.dir).unwrap().difficulty, Some(0));
}

#[test]
fn everyone_may_run_everything_but_op_by_default() {
    let mut t = T::new();
    t.open = true;
    assert_eq!(t.said(2, "time set day"), vec!["Set the time to 1000"]);
    assert_eq!(t.said(2, "gamemode creative Cat"), vec!["Set Cat's game mode to Creative Mode"]);
    assert_eq!(t.said(2, "op Bob"), vec!["!You do not have permission to use this command"]);
    assert_eq!(t.said(2, "deop xy"), vec!["!You do not have permission to use this command"]);
    assert_eq!(t.said(2, "help")[0], "--- Showing help page 1 of 3 (/help <page>) ---");
    assert_eq!(t.said(2, "help 3"), vec!["--- Showing help page 3 of 3 (/help <page>) ---", "/summon <entityName> [x] [y] [z] [dataTag]", "/tell <player> <private message ...>", "/time <set|add|query> <value>", "/tp [target player] <destination player> OR /tp [target player] <x> <y> <z> [<yaw> <pitch>]", "/weather <clear|rain|thunder> [duration in seconds]"]);
    // Under "ops" the same player is refused, and an op is not.
    t.open = false;
    assert_eq!(t.said(2, "time set day"), vec!["!You do not have permission to use this command"]);
    assert_eq!(t.said(1, "time set day"), vec!["Set the time to 1000"]);
}

#[test]
fn spawn_sends_a_player_to_the_top_of_the_world_spawn() {
    let mut t = T::new();
    t.world.spawn = [-20, 70, 33];
    let surface = Teleport { pos: [-19.5, 70.0, 33.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN | proto::TP_SURFACE };
    let e = t.run(2, "spawn");
    assert_eq!(e, vec![Effect::Teleport(2, surface), Effect::Feedback(Line::plain("Teleported Bob to the world spawn (-20, 70, 33)"))]);
    // Anyone may go; only those who may /tp send someone else.
    assert_eq!(t.said(2, "spawn xy"), vec!["!You do not have permission to use this command"]);
    assert_eq!(t.teleports(1, "spawn bob"), vec![(2, surface)]);
    assert_eq!(t.teleports(1, "spawn @a").len(), 3);
    assert_eq!(t.said(0, "spawn"), vec!["!You must specify which player you wish to perform this action on."]);
    assert_eq!(t.said(1, "spawn a b"), vec!["!Usage: /spawn [player]"]);
}

#[test]
fn gamerule_values_take_the_rest_of_the_line() {
    let mut t = T::new();
    assert_eq!(t.said(1, "gamerule myRule two words"), vec!["Game rule myRule has been updated to two words"]);
    assert_eq!(t.said(1, "gamerule myRule"), vec!["myRule = two words"]);
    assert_eq!(t.said(1, "  time set 5  "), vec!["Set the time to 5"], "the line is trimmed");
    assert_eq!(t.said(0, "setworldspawn"), vec!["!You must specify which player you wish to perform this action on."]);
}

#[test]
fn feedback_is_a_chat_line_marked_as_a_command_answer() {
    let v = chat_json(&Line::plain("Set the time to 0"));
    assert_eq!(v, serde_json::json!({"t": "chat", "from": "", "kind": "command", "text": "Set the time to 0", "color": "white"}));
    let v = chat_json(&Line { text: "psst".into(), color: Some("gray"), italic: true });
    assert_eq!((v["color"].as_str(), v["italic"].as_bool()), (Some("gray"), Some(true)));
}

#[test]
fn kick_ban_and_pardon() {
    let mut t = T::new();
    assert_eq!(t.said(2, "kick Cat"), vec!["!You do not have permission to use this command"]);
    assert_eq!(t.run(1, "kick cat"), vec![Effect::Kick("Cat".into(), "Kicked by an operator.".into()), Effect::Feedback(Line::plain("Kicked Cat from the game"))]);
    assert_eq!(t.said(1, "kick Cat go to bed"), vec!["Kicked Cat from the game: 'go to bed'"]);
    assert_eq!(t.said(1, "kick Nobody"), vec!["!Could not find that player"]);

    let e = t.run(1, "ban Bob ip true griefing the spawn");
    assert!(e.contains(&Effect::Bans));
    assert!(e.contains(&Effect::Kick("Bob".into(), "You are banned from this server.
Reason: griefing the spawn".into())));
    assert_eq!(t.bans.player("bob").unwrap().reason, "griefing the spawn");
    assert_eq!(t.bans.ip("203.0.113.9").unwrap().name, "Bob");
    assert_eq!(t.said(1, "ban Cat false"), vec!["Banned player Cat"]);
    assert_eq!(t.bans.player("cat").unwrap().reason, crate::bans::DEFAULT_REASON);
    assert_eq!(t.said(1, "ban Dan true"), vec!["Banned player Dan", "!You have entered an invalid IP address or a player that is not online"]);
    assert_eq!(t.said(1, "ban Dan ip maybe"), vec!["!'maybe' is not true or false"]);
    assert_eq!(t.said(1, "banlist"), vec!["There are 3 total banned players:", "Bob, Cat, Dan"]);
    assert_eq!(t.said(1, "banlist ips"), vec!["There are 1 total banned IP addresses:", "203.0.113.9"]);

    assert_eq!(t.said(1, "pardon bob"), vec!["Unbanned player bob"]);
    assert_eq!(t.said(1, "pardon bob"), vec!["!Could not unban player bob"]);
    assert_eq!(t.said(1, "pardon-ip 203.0.113.9"), vec!["Unbanned IP address 203.0.113.9"]);
    assert_eq!(t.said(1, "pardon-ip nope"), vec!["!You have entered an invalid IP address"]);
    assert_eq!(t.said(1, "ban-ip 198.51.100.4 spam"), vec!["Banned IP address 198.51.100.4"]);
    assert_eq!(t.said(1, "ban-ip Bob"), vec!["Banned IP address 203.0.113.9 belonging to Bob"]);
    assert_eq!(t.bans.ips.len(), 2);
}

#[test]
fn backup_and_rollback_are_for_ops() {
    let mut t = T::new();
    t.open = true;
    for line in ["backup", "backup list", "rollback", "rollback x"] {
        assert_eq!(t.said(2, line), vec![format!("!{PERMISSION}")], "{line}");
    }
    let e = t.run(1, "backup");
    assert_eq!(e, vec![Effect::Feedback(Line::plain("Saving a backup of the world...")), Effect::Backup]);
    assert_eq!(t.run(0, "backup").last(), Some(&Effect::Backup));
    assert_eq!(t.said(1, "backup now"), vec!["!Usage: /backup [list]"]);
    assert_eq!(t.said(1, "backup list"), vec!["There are 0 backups:"]);

    let name = crate::backup::create(&t.dir, "").unwrap();
    assert_eq!(t.said(1, "rollback list"), vec!["There are 1 backups:".to_string(), name.clone()]);
    assert_eq!(t.said(1, "rollback 19990101-000000"), vec!["!There is no backup named 19990101-000000; /rollback list names them"]);
    assert_eq!(t.said(1, "rollback ../edits.bin"), vec!["!There is no backup named ../edits.bin; /rollback list names them"]);
    let e = t.run(1, &format!("rollback {name}"));
    assert_eq!(e.first(), Some(&Effect::Rollback(Some(name.clone()))));
    assert_eq!(e.len(), 2);
    assert_eq!(t.said(1, "rollback cancel"), vec!["!No rollback is waiting"]);
    // The hub writes the request (commands.rs `run`); the list then names it.
    crate::backup::set_pending(&t.dir, Some(&name)).unwrap();
    assert_eq!(t.said(1, "backup list").last().unwrap(), &format!("The world rolls back to {name} when the server next starts"));
    assert_eq!(t.run(1, "rollback cancel"), vec![Effect::Rollback(None), Effect::Feedback(Line::plain("The rollback is cancelled"))]);
    assert_eq!(t.said(1, "rollback a b"), vec!["!Usage: /rollback [list|cancel|<backup name>]"]);
}

#[test]
fn summon_places_a_mob_at_the_sender_or_the_coordinates() {
    let mut t = T::new();
    let summons = |t: &mut T, id: u32, line: &str| -> Vec<(&'static str, [f64; 3])> {
        t.run(id, line).into_iter().filter_map(|e| if let Effect::Summon(k, p) = e { Some((k.name, p)) } else { None }).collect()
    };
    assert_eq!(summons(&mut t, 1, "summon cow"), vec![("cow", [10.25, 64.0, -3.75])]);
    assert_eq!(summons(&mut t, 1, "summon minecraft:cow 5 70 ~2"), vec![("cow", [5.5, 70.0, -1.75])]);
    assert_eq!(t.said(1, "summon cow 1 2 3"), vec!["Object successfully spawned"]);
    assert_eq!(t.said(1, "summon"), vec!["!Usage: /summon <entityName> [x] [y] [z] [dataTag]"]);
    assert_eq!(t.said(1, "summon notamob"), vec!["!Unable to summon object"]);
    assert_eq!(t.said(1, "summon item"), vec!["!Unable to summon object"]);
    assert_eq!(t.said(2, "summon cow"), vec!["!You do not have permission to use this command"]);
}

#[test]
fn effect_gives_and_clears() {
    use items::potion::{Effect as Fx, PotionEffect};
    let mut t = T::new();
    let e = t.run(1, "effect Bob speed 10 2");
    assert_eq!(e[0], Effect::Potion(2, PotionOp::Add(PotionEffect::new(Fx::Speed, 200, 2))));
    assert_eq!(e[1], Effect::Feedback(Line::plain("Given Speed (ID 1) * 2 to Bob for 10 seconds")));
    // The default is 30 seconds; an instant effect counts ticks as seconds.
    assert_eq!(t.run(1, "effect Bob minecraft:regeneration")[0], Effect::Potion(2, PotionOp::Add(PotionEffect::new(Fx::Regeneration, 600, 0))));
    assert_eq!(t.run(1, "effect Bob instant_health")[0], Effect::Potion(2, PotionOp::Add(PotionEffect::new(Fx::InstantHealth, 1, 0))));
    assert_eq!(t.run(1, "effect Bob 1 0")[0], Effect::Potion(2, PotionOp::Remove(Fx::Speed)));
    let e = t.run(1, "effect Bob clear");
    assert_eq!(e[0], Effect::Potion(2, PotionOp::Clear));
    assert_eq!(e[1], Effect::Feedback(Line::plain("Took all effects from Bob")));
    assert_eq!(t.said(1, "effect Bob nothing"), vec!["!There is no such mob effect with ID nothing"]);
}
