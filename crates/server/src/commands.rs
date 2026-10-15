//! Chat commands as 1.12.2's `CommandHandler` runs them: a line typed in chat with a leading
//! slash (or into the server console without one) is split on spaces, the command looked up by
//! name or alias, its permission checked, a selector at a player argument (`@a`, `@p`, `@r`,
//! `@s`, `@e`) run once per match, and the command's own `execute` (commands_exec.rs) ported
//! with vanilla's feedback strings from en_us.lang. Errors come back red to the sender; feedback
//! goes only to the sender, as `notifyCommandListener` does when `sendCommandFeedback` is on.
//! `/say`, `/me`, the death message and whispers reach others.
//!
//! Permissions: with `server.json` "commands" at "everyone" (the default) every signed-in player
//! may run every command but `/op` and `/deop`; at "ops" only the names in "ops" may run the ones
//! that change the world or other players. The console may run everything. The ban and kick
//! commands are ops only, as `/op` is, and so are `/backup` and `/rollback` (backup.rs). A guest may not chat or run `/me`, `/say` or `/tell`.
//!
//! [`Ctx`] runs a command against the world and the players online and lists what has to
//! happen ([`Effect`]); [`run`] does that with the hub's sessions. Feedback and broadcasts go to
//! the page as a `chat` message from "" (the page's system line) marked `"kind": "command"`, with
//! the vanilla color beside it, and to a native client the same way (`S_TEXT`, chat.rs).

use proto::{PlayerState, Teleport};
use serde_json::json;

use crate::hub::{Hub, ToBrowser};
use crate::world::World;

#[path = "commands_exec.rs"]
mod exec;
#[path = "commands_backup.rs"]
mod backup_cmds;

use exec::COMMANDS;

/// A chat line with vanilla's formatting.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    /// A `TextFormatting` name: "red", "gray", "dark_green".
    pub color: Option<&'static str>,
    pub italic: bool,
}

impl Line {
    fn plain(text: impl Into<String>) -> Line {
        Line { text: text.into(), color: None, italic: false }
    }

    fn colored(text: impl Into<String>, color: &'static str) -> Line {
        Line { text: text.into(), color: Some(color), italic: false }
    }
}

/// What `/effect` does to a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PotionOp {
    Add(items::potion::PotionEffect),
    Remove(items::potion::Effect),
    Clear,
}

/// What a command leaves for the hub to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// To whoever ran the command.
    Feedback(Line),
    /// To one player's chat.
    Tell(u32, Line),
    /// A system line in everyone's chat.
    Broadcast(String),
    Teleport(u32, Teleport),
    GameMode(u32, u8),
    /// `/kill` on a player (id, name): the game thread kills and the death screen respawns them;
    /// with no game thread, the old fallback (message and teleport to the respawn point).
    Kill(u32, String),
    /// `/effect` on a player (the game thread adds or takes the effect).
    Potion(u32, PotionOp),
    /// The time or the daylight cycle changed: every viewer's clock jumps to it.
    Time,
    /// `/difficulty`: the id 0 to 3 is now the world's; the game thread applies it and every viewer is told.
    Difficulty(u8),
    /// `/summon`: a mob of this kind at this position (the game thread spawns it).
    Summon(&'static sim::kinds::Kind, [f64; 3]),
    /// The operator list changed: write it to `server.json`.
    Ops,
    /// Ends a player's session with this line (`/kick`, `/ban`).
    Kick(String, String),
    /// The bans changed: write `bans.json` (bans.rs).
    Bans,
    /// `/backup`: the backup thread saves and copies the world, then tells the sender (backup.rs).
    Backup,
    /// `/rollback NAME`: roll back to this backup at the next start; `None` cancels that.
    Rollback(Option<String>),
}

/// A player online, as the commands see them.
#[derive(Clone, Debug)]
pub struct Who {
    pub id: u32,
    pub name: String,
    pub state: Option<PlayerState>,
}

/// Who runs the command: a player (`id`) or the console.
pub struct Sender {
    pub id: Option<u32>,
    pub name: String,
    /// Named in "ops", or the console.
    pub op: bool,
}

/// Who may run a command.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Perm {
    /// Anyone: vanilla's level 0 (`/help`, `/list`, `/me`, `/tell`) and `/spawn` for oneself.
    All,
    /// Anyone while "commands" is "everyone"; only ops under "ops".
    Ops,
    /// Only ops, whatever "commands" says.
    OpsOnly,
}

type R = Result<(), String>;

struct Command {
    name: &'static str,
    aliases: &'static [&'static str],
    /// `commands.<name>.usage`
    usage: &'static str,
    perm: Perm,
    /// `isUsernameIndex`: which argument names a player, for these arguments.
    user: fn(&[&str]) -> Option<usize>,
    run: fn(&mut Ctx, &[&str]) -> R,
}

const UNKNOWN: &str = "Unknown command. Try /help for a list of commands";
const PERMISSION: &str = "You do not have permission to use this command";

fn find_command(name: &str) -> Option<&'static Command> {
    let name = name.to_ascii_lowercase();
    COMMANDS.iter().find(|c| c.name == name || c.aliases.contains(&name.as_str()))
}

fn usage(c: &str) -> String {
    format!("Usage: {c}")
}

/// One command run against the world and the players online.
pub struct Ctx<'a> {
    pub world: &'a mut World,
    pub players: &'a [Who],
    pub ops: &'a mut Vec<String>,
    pub bans: &'a mut crate::bans::Bans,
    /// Each name's address (lowercase name), as last seen at a game socket since the server started.
    pub ips: &'a std::collections::HashMap<String, String>,
    pub sender: &'a Sender,
    /// `server.json` "commands" is "everyone".
    pub open: bool,
    pub seed: i64,
    pub max_players: usize,
    pub effects: Vec<Effect>,
}

impl Ctx<'_> {
    /// Runs one line (without the slash).
    pub fn execute(&mut self, line: &str) {
        let args: Vec<&str> = line.trim().split(' ').collect();
        let Some(cmd) = find_command(args[0]) else {
            return self.error(UNKNOWN.into());
        };
        if !self.may(cmd.perm) {
            return self.error(PERMISSION.into());
        }
        let args = &args[1..];
        // CommandHandler.tryExecute: a selector matching several players runs once for each.
        if let Some(i) = (cmd.user)(args).filter(|&i| i < args.len() && args[i].starts_with('@')) {
            let names: Vec<String> = self.select(args[i]).into_iter().map(|k| self.players[k].name.clone()).collect();
            if names.len() > 1 {
                for n in names {
                    let mut a = args.to_vec();
                    a[i] = &n;
                    if let Err(e) = (cmd.run)(self, &a) {
                        self.error(e);
                    }
                }
                return;
            }
        }
        if let Err(e) = (cmd.run)(self, args) {
            self.error(e);
        }
    }

    fn may(&self, perm: Perm) -> bool {
        match perm {
            Perm::All => true,
            Perm::Ops => self.open || self.sender.op,
            Perm::OpsOnly => self.sender.op,
        }
    }

    fn error(&mut self, text: String) {
        self.effects.push(Effect::Feedback(Line::colored(text, "red")));
    }

    /// `sender.sendMessage`: always shown.
    fn message(&mut self, line: Line) {
        self.effects.push(Effect::Feedback(line));
    }

    /// `notifyCommandListener`: shown while `sendCommandFeedback` is on (always on the console).
    fn notify(&mut self, text: String) {
        if self.sender.id.is_none() || self.feedback_on() {
            self.message(Line::plain(text));
        }
    }

    fn feedback_on(&self) -> bool {
        self.world.rule("sendCommandFeedback") != Some("false")
    }

    /// The players a name or selector stands for.
    fn select(&self, token: &str) -> Vec<usize> {
        let all = 0..self.players.len();
        match token {
            "@a" | "@e" => all.collect(),
            "@s" => all.filter(|&k| Some(self.players[k].id) == self.sender.id).collect(),
            "@r" if !self.players.is_empty() => vec![(crate::config::random_u64() % self.players.len() as u64) as usize],
            "@p" => {
                let me = self.sender_pos();
                let d = |k: usize| {
                    let p = self.pos_of(k).0;
                    (0..3).map(|i| (p[i] - me[i]).powi(2)).sum::<f64>()
                };
                all.min_by(|&a, &b| d(a).total_cmp(&d(b))).into_iter().collect()
            }
            t if t.starts_with('@') => Vec::new(),
            name => all.filter(|&k| self.players[k].name.eq_ignore_ascii_case(name)).collect(),
        }
    }

    /// `CommandBase.getPlayer`
    fn player(&self, token: &str) -> Result<usize, String> {
        self.select(token).first().copied().ok_or_else(|| format!("Player '{token}' cannot be found"))
    }

    /// `getCommandSenderAsPlayer`
    fn sender_player(&self) -> Result<usize, String> {
        self.players
            .iter()
            .position(|p| Some(p.id) == self.sender.id)
            .ok_or_else(|| "You must specify which player you wish to perform this action on.".to_string())
    }

    /// Feet, yaw and pitch: the last state the player's session sent, else where they were saved,
    /// else the world spawn.
    fn pos_of(&self, k: usize) -> ([f64; 3], f32, f32) {
        let p = &self.players[k];
        if let Some(s) = p.state {
            return (s.pos, s.yaw, s.pitch);
        }
        match self.world.player(&p.name).and_then(|s| s.pos.map(|pos| (pos, s.yaw, s.pitch))) {
            Some(at) => at,
            None => (self.world.spawn.map(|v| v as f64 + 0.5), 0.0, 0.0),
        }
    }

    /// `getPositionVector`: the console stands at the origin.
    fn sender_pos(&self) -> [f64; 3] {
        self.sender_player().map_or([0.0; 3], |k| self.pos_of(k).0)
    }

    /// `Entity.getPosition`: the block at the feet, half a block up.
    fn block_of(p: [f64; 3]) -> [i32; 3] {
        [p[0].floor() as i32, (p[1] + 0.5).floor() as i32, p[2].floor() as i32]
    }
}

/// A respawn at the world spawn: on the column's top block, out of any blocks, flying ended.
/// DROPPED: `spawnFuzz`, the random spot within `spawnRadius` vanilla picks, so `/spawn` lands
/// on the spawn itself.
fn world_spawn(world: &World) -> Teleport {
    let [x, y, z] = world.spawn;
    Teleport { pos: [x as f64 + 0.5, y as f64, z as f64 + 0.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN | proto::TP_SURFACE }
}

/// Where a dead player comes back: at their `/spawnpoint` exactly (`EntityPlayer.getBedSpawnLocation`
/// with a forced spawn point), else on the top block at the world spawn
/// (`PlayerList.recreatePlayerEntity`).
pub(crate) fn respawn_point(world: &World, name: &str) -> Teleport {
    match world.player(name).and_then(|s| s.spawn) {
        Some([x, y, z]) => Teleport { pos: [x as f64 + 0.5, y as f64 + 0.1, z as f64 + 0.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN },
        None => world_spawn(world),
    }
}

/// The page's `chat` message from "" (a system line) marked `"kind": "command"`, with its
/// vanilla color (white unless the line names one: red errors, gray whispers) and italics, so a
/// client tells it from the yellow join and leave lines.
fn chat_json(line: &Line) -> serde_json::Value {
    let mut v = json!({"t": "chat", "from": "", "kind": "command", "text": line.text, "color": line.color.unwrap_or("white")});
    if line.italic {
        v["italic"] = json!(true);
    }
    v
}

/// What the browser (or a native client) typed into chat: a command, or a chat line.
pub fn chat(hub: &Hub, id: u32, name: &str, text: &str) {
    match text.strip_prefix('/') {
        Some(line) if find_command(line.split(' ').next().unwrap_or("")).is_some_and(|c| TALK.contains(&c.name)) && guest(hub, name) => tell_guest(hub, id),
        Some(line) => {
            println!("{name} issued server command: /{line}");
            let op = hub.ops.lock().unwrap().iter().any(|o| o.eq_ignore_ascii_case(name));
            run(hub, &Sender { id: Some(id), name: name.to_string(), op }, line);
        }
        None if guest(hub, name) => tell_guest(hub, id),
        None => hub.chat(name, text),
    }
}

/// The commands that talk to other players, which a guest may not run.
const TALK: &[&str] = &["me", "say", "tell"];

fn guest(hub: &Hub, name: &str) -> bool {
    !hub.accounts.lock().unwrap().is_account(name)
}

fn tell_guest(hub: &Hub, id: u32) {
    let line = Line::colored("Guests cannot chat. Sign in with an account to talk.", "red");
    if let Some(s) = hub.sessions().iter().find(|s| s.id == id) {
        let _ = s.to_browser.send(ToBrowser::Text(chat_json(&line).to_string()));
    }
}

/// A line typed into the server window: the console runs commands as the host, with every
/// permission, slash or not.
pub fn console(hub: &Hub, line: &str) {
    let line = line.trim();
    if !line.is_empty() {
        run(hub, &Sender { id: None, name: "Server".into(), op: true }, line.strip_prefix('/').unwrap_or(line));
    }
}

/// Runs a command with the hub's world and sessions and delivers what it did.
pub fn run(hub: &Hub, sender: &Sender, line: &str) {
    let line: String = line.chars().filter(|c| !c.is_control()).take(256).collect();
    let sessions = hub.sessions();
    let players: Vec<Who> = sessions.iter().map(|s| Who { id: s.id, name: s.name.clone(), state: s.state }).collect();
    let effects = {
        let mut world = hub.world.lock().unwrap();
        let mut ops = hub.ops.lock().unwrap();
        let mut bans = hub.bans.lock().unwrap();
        let ips = hub.ips.lock().unwrap();
        let mut ctx = Ctx {
            world: &mut world,
            players: &players,
            ops: &mut ops,
            bans: &mut bans,
            ips: &ips,
            sender,
            open: hub.cfg.commands_open(),
            seed: hub.cfg.seed,
            max_players: hub.cfg.max_players,
            effects: Vec::new(),
        };
        ctx.execute(&line);
        ctx.effects
    };
    let tell = |id: Option<u32>, line: &Line| match id.and_then(|id| sessions.iter().find(|s| s.id == id)) {
        Some(s) => {
            let _ = s.to_browser.send(ToBrowser::Text(chat_json(line).to_string()));
        }
        None if id.is_none() => println!("{}", line.text),
        None => {}
    };
    for e in effects {
        match e {
            Effect::Feedback(l) => tell(sender.id, &l),
            Effect::Tell(id, l) => tell(Some(id), &l),
            Effect::Broadcast(text) => {
                println!("{text}");
                hub.tell_all(&chat_json(&Line::plain(text)));
            }
            Effect::Teleport(id, tp) => {
                send_to(&sessions, id, proto::frame(proto::S_TELEPORT, &tp.encode()));
                hub.to_game(crate::game::GameMsg::Teleport { id });
            }
            Effect::GameMode(id, m) => {
                send_to(&sessions, id, proto::frame(proto::S_GAMEMODE, &[m]));
                hub.to_game(crate::game::GameMsg::GameMode { id, mode: m });
            }
            Effect::Potion(id, op) => hub.to_game(crate::game::GameMsg::Potion { id, op }),
            Effect::Kill(id, name) => {
                if hub.has_game() {
                    hub.to_game(crate::game::GameMsg::Kill { id });
                } else {
                    if hub.world.lock().unwrap().rule("showDeathMessages") != Some("false") {
                        let text = format!("{name} fell out of the world");
                        println!("{text}");
                        hub.tell_all(&chat_json(&Line::plain(text)));
                    }
                    let tp = respawn_point(&hub.world.lock().unwrap(), &name);
                    send_to(&sessions, id, proto::frame(proto::S_TELEPORT, &tp.encode()));
                }
            }
            Effect::Summon(kind, pos) => hub.to_game(crate::game::GameMsg::Summon { kind, pos }),
            Effect::Time => sync_time(hub, true),
            Effect::Difficulty(d) => {
                hub.to_game(crate::game::GameMsg::Difficulty { id: d });
                let frame = proto::frame(proto::S_DIFFICULTY, &[d]);
                for s in hub.sessions() {
                    let _ = s.to_viewer.send(frame.clone());
                }
            }
            Effect::Ops => {
                let ops = hub.ops.lock().unwrap().clone();
                if let Err(e) = crate::config::Config::save_ops(&hub.dir, &ops) {
                    eprintln!("could not write the ops to server.json: {e}");
                }
            }
            Effect::Kick(name, reason) => hub.kick(&name, &reason),
            Effect::Bans => {
                if let Err(e) = hub.bans.lock().unwrap().save(&hub.dir) {
                    eprintln!("could not write bans.json: {e}");
                }
            }
            Effect::Backup => {
                if !hub.request_backup(sender.id) {
                    tell(sender.id, &Line::colored("Backups are not running on this server", "red"));
                }
            }
            Effect::Rollback(name) => {
                if let Err(e) = crate::backup::set_pending(&hub.dir, name.as_deref()) {
                    tell(sender.id, &Line::colored(format!("Could not write the rollback request: {e}"), "red"));
                }
            }
        }
    }
}

/// A line to one player's chat, or (`None`) the server window: white, or red for an error.
pub fn notice(hub: &Hub, id: Option<u32>, text: Result<String, String>) {
    let line = match text {
        Ok(t) => Line::plain(t),
        Err(t) => Line::colored(t, "red"),
    };
    match id {
        Some(id) => {
            if let Some(s) = hub.sessions().iter().find(|s| s.id == id) {
                let _ = s.to_browser.send(ToBrowser::Text(chat_json(&line).to_string()));
            }
        }
        None => println!("{}", line.text),
    }
}

fn send_to(sessions: &[crate::hub::Session], id: u32, frame: Vec<u8>) {
    if let Some(s) = sessions.iter().find(|s| s.id == id) {
        let _ = s.to_viewer.send(frame);
    }
}

/// The shared clock to every viewer: every second, and at once (`snap`) after `/time`.
pub fn sync_time(hub: &Hub, snap: bool) {
    let frame = proto::frame(proto::S_TIME, &hub.world.lock().unwrap().time_sync(snap).encode());
    for s in hub.sessions() {
        let _ = s.to_viewer.send(frame.clone());
    }
}

/// What a joining viewer gets after the welcome: the clock, its game mode and, for a player the
/// world has never seen, the way to the world spawn. On a server with no ops yet, the first
/// account (the host's, normally) becomes one; a guest never does.
pub fn join_frames(hub: &Hub, name: &str, seen_before: bool) -> Vec<u8> {
    {
        let mut ops = hub.ops.lock().unwrap();
        let accounts = hub.accounts.lock().unwrap();
        if ops.is_empty() && accounts.count() == 1 && accounts.is_account(name) {
            ops.push(name.to_string());
            println!("{name} is the first account, so an operator (server.json \"ops\")");
            if let Err(e) = crate::config::Config::save_ops(&hub.dir, &ops) {
                eprintln!("could not write the ops to server.json: {e}");
            }
        }
    }
    let mut w = hub.world.lock().unwrap();
    // Kept from the first visit on, so a later change of the default leaves this player's alone.
    w.update_player(name, |_| {});
    let mut out = proto::frame(proto::S_TIME, &w.time_sync(true).encode());
    out.extend(proto::frame(proto::S_GAMEMODE, &[w.game_mode(name)]));
    out.extend(proto::frame(proto::S_DIFFICULTY, &[w.difficulty.unwrap_or(2)]));
    if !seen_before {
        out.extend(proto::frame(proto::S_TELEPORT, &world_spawn(&w).encode()));
    }
    out
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
