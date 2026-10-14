//! Each command's `execute`, ported from 1.12.2's `Command*` classes with their en_us.lang
//! strings, and the `CommandBase` parsers they share. `/spawn` is not vanilla: it is a respawn
//! at the world spawn on demand.

use proto::Teleport;

use super::backup_cmds::{BACKUP, ROLLBACK};
use super::{usage, world_spawn, Command, Ctx, Line, Perm, Effect, PERMISSION, R, UNKNOWN};
use crate::bans::Ban;
use crate::world::DEFAULT_RULES;

/// The words after a ban's target, or `BanEntry`'s default.
fn reason_of(rest: &[&str]) -> String {
    let r = rest.join(" ");
    if r.trim().is_empty() {
        crate::bans::DEFAULT_REASON.to_string()
    } else {
        r
    }
}

fn none(_: &[&str]) -> Option<usize> {
    None
}

fn first(_: &[&str]) -> Option<usize> {
    Some(0)
}

const KICK: &str = "/kick <player> [reason ...]";
/// Vanilla's `/ban <name> [reason ...]` with `ip true|false` after the name.
const BAN: &str = "/ban <name> [ip true|false] [reason ...]";
const BAN_IP: &str = "/ban-ip <address|name> [reason ...]";
const NOT_ONLINE: &str ="You have entered an invalid IP address or a player that is not online";

/// Sorted by name, as `/help` lists them.
pub(super) const COMMANDS: &[Command] = &[
    Command { name: "backup", aliases: &[], usage: BACKUP, perm: Perm::OpsOnly, user: none, run: |c, a| c.backup(a) },
    Command { name: "ban", aliases: &[], usage: BAN, perm: Perm::OpsOnly, user: none, run: |c, a| c.ban(a) },
    Command { name: "ban-ip", aliases: &[], usage: BAN_IP, perm: Perm::OpsOnly, user: none, run: |c, a| c.ban_ip(a) },
    Command { name: "banlist", aliases: &[], usage: "/banlist [ips|players]", perm: Perm::OpsOnly, user: none, run: |c, a| c.banlist(a) },
    Command { name: "deop", aliases: &[], usage: "/deop <player>", perm: Perm::OpsOnly, user: none, run: |c, a| c.deop(a) },
    Command { name: "gamemode", aliases: &[], usage: "/gamemode <mode> [player]", perm: Perm::Ops, user: |_| Some(1), run: |c, a| c.gamemode(a) },
    Command { name: "gamerule", aliases: &[], usage: "/gamerule <rule name> [value]", perm: Perm::Ops, user: none, run: |c, a| c.gamerule(a) },
    Command { name: "help", aliases: &["?"], usage: "/help [page|command name]", perm: Perm::All, user: none, run: |c, a| c.help(a) },
    Command { name: "kick", aliases: &[], usage: KICK, perm: Perm::OpsOnly, user: first, run: |c, a| c.kick(a) },
    Command { name: "kill", aliases: &[], usage: "/kill [player|entity]", perm: Perm::Ops, user: first, run: |c, a| c.kill(a) },
    Command { name: "list", aliases: &[], usage: "/list", perm: Perm::All, user: none, run: |c, a| c.list(a) },
    Command { name: "me", aliases: &[], usage: "/me <action ...>", perm: Perm::All, user: none, run: |c, a| c.emote(a) },
    Command { name: "op", aliases: &[], usage: "/op <player>", perm: Perm::OpsOnly, user: none, run: |c, a| c.op(a) },
    Command { name: "pardon", aliases: &[], usage: "/pardon <name>", perm: Perm::OpsOnly, user: none, run: |c, a| c.pardon(a) },
    Command { name: "pardon-ip", aliases: &[], usage: "/pardon-ip <address>", perm: Perm::OpsOnly, user: none, run: |c, a| c.pardon_ip(a) },
    Command { name: "rollback", aliases: &[], usage: ROLLBACK, perm: Perm::OpsOnly, user: none, run: |c, a| c.rollback(a) },
    Command { name: "say", aliases: &[], usage: "/say <message ...>", perm: Perm::Ops, user: none, run: |c, a| c.say(a) },
    Command { name: "seed", aliases: &[], usage: "/seed", perm: Perm::Ops, user: none, run: |c, a| c.seed(a) },
    Command { name: "setworldspawn", aliases: &[], usage: "/setworldspawn [<x> <y> <z>]", perm: Perm::Ops, user: none, run: |c, a| c.setworldspawn(a) },
    Command { name: "spawn", aliases: &[], usage: "/spawn [player]", perm: Perm::All, user: first, run: |c, a| c.spawn(a) },
    Command { name: "spawnpoint", aliases: &[], usage: "/spawnpoint [player] [<x> <y> <z>]", perm: Perm::Ops, user: first, run: |c, a| c.spawnpoint(a) },
    Command { name: "teleport", aliases: &[], usage: "/teleport <entity> <x> <y> <z> [<y-rot> <x-rot>]", perm: Perm::Ops, user: first, run: |c, a| c.teleport(a) },
    Command { name: "tell", aliases: &["msg", "w"], usage: "/tell <player> <private message ...>", perm: Perm::All, user: first, run: |c, a| c.tell(a) },
    Command { name: "time", aliases: &[], usage: "/time <set|add|query> <value>", perm: Perm::Ops, user: none, run: |c, a| c.time(a) },
    Command {
        name: "tp",
        aliases: &[],
        usage: "/tp [target player] <destination player> OR /tp [target player] <x> <y> <z> [<yaw> <pitch>]",
        perm: Perm::Ops,
        user: |a| matches!(a.len(), 2 | 4 | 6).then_some(0),
        run: |c, a| c.tp(a),
    },
    Command { name: "weather", aliases: &[], usage: "/weather <clear|rain|thunder> [duration in seconds]", perm: Perm::Ops, user: none, run: |c, a| c.weather(a) },
];

/// `Double.toString` for the values commands print.
pub fn java_double(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e7 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

/// `MathHelper.wrapDegrees`
fn wrap_degrees(v: f32) -> f32 {
    let mut v = v % 360.0;
    if v >= 180.0 {
        v -= 360.0;
    }
    if v < -180.0 {
        v += 360.0;
    }
    v
}

/// `CommandBase.parseInt`, with its bounds.
pub fn parse_int(s: &str, min: i32, max: i32) -> Result<i32, String> {
    let v: i32 = s.parse().map_err(|_| format!("'{s}' is not a valid number"))?;
    if v < min {
        return Err(format!("The number you have entered ({v}) is too small, it must be at least {min}"));
    }
    if v > max {
        return Err(format!("The number you have entered ({v}) is too big, it must be at most {max}"));
    }
    Ok(v)
}

/// `CommandBase.parseDouble(String)`: Java's `Double.parseDouble`, finite only.
fn parse_double(s: &str) -> Result<f64, String> {
    let t = s.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(s);
    t.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("'{s}' is not a valid number"))
}

/// `CommandBase.CoordinateArg`: the absolute value, what was typed after any `~`, and whether
/// it was relative.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coord {
    pub result: f64,
    pub amount: f64,
    pub relative: bool,
}

/// `CommandBase.parseCoordinate(base, arg, min, max, centerBlock)`: `~` is relative to `base`,
/// and a whole number gets 0.5 added when `center` (the middle of the block).
pub fn coordinate(base: f64, arg: &str, center: bool, min: i32, max: i32) -> Result<Coord, String> {
    let relative = arg.starts_with('~');
    let mut amount = 0.0;
    if !relative || arg.len() > 1 {
        let dotted = arg.contains('.');
        amount += parse_double(if relative { &arg[1..] } else { arg })?;
        if !dotted && !relative && center {
            amount += 0.5;
        }
    }
    let result = amount + if relative { base } else { 0.0 };
    if min != 0 || max != 0 {
        if result < min as f64 {
            return Err(format!("The number you have entered ({result:.2}) is too small, it must be at least {min}"));
        }
        if result > max as f64 {
            return Err(format!("The number you have entered ({result:.2}) is too big, it must be at most {max}"));
        }
    }
    Ok(Coord { result, amount, relative })
}

/// `CommandBase.parseBlockPos(sender, args, 0, true)`: three block coordinates, `~` relative to
/// `base`, y within 0..256.
pub fn block_pos(base: [i32; 3], args: &[&str]) -> Result<[i32; 3], String> {
    let one = |b: i32, arg: &str, center: bool, min: i32, max: i32| coordinate(b as f64, arg, center, min, max).map(|c| c.result.floor() as i32);
    Ok([one(base[0], args[0], true, -30_000_000, 30_000_000)?, one(base[1], args[1], false, 0, 256)?, one(base[2], args[2], true, -30_000_000, 30_000_000)?])
}

/// `GameType` display names (`gameMode.*`).
fn mode_name(m: u8) -> &'static str {
    match m {
        proto::GM_SURVIVAL => "Survival Mode",
        proto::GM_ADVENTURE => "Adventure Mode",
        proto::GM_SPECTATOR => "Spectator Mode",
        _ => "Creative Mode",
    }
}

/// `CommandGameMode.getGameModeFromCommand`
fn parse_mode(s: &str) -> Result<u8, String> {
    Ok(match s {
        "survival" | "s" => proto::GM_SURVIVAL,
        "creative" | "c" => proto::GM_CREATIVE,
        "adventure" | "a" => proto::GM_ADVENTURE,
        "spectator" | "sp" => proto::GM_SPECTATOR,
        _ => parse_int(s, 0, 3)? as u8,
    })
}

/// `CommandBase.joinNiceString`: "a, b and c".
fn join_nice(items: &[&str]) -> String {
    let mut out = String::new();
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(if i == items.len() - 1 { " and " } else { ", " });
        }
        out.push_str(s);
    }
    out
}

impl Ctx<'_> {
    /// `CommandBanPlayer`. `true` after the name (or `ip true`) bans the player's address as
    /// well, as `/ban-ip <player>` does; a ban on a player online ends their session.
    fn ban(&mut self, args: &[&str]) -> R {
        let Some((&name, rest)) = args.split_first().filter(|(n, _)| !n.is_empty()) else { return Err(usage(BAN)) };
        let truth = |v: &str| match v.to_ascii_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(format!("'{v}' is not true or false")),
        };
        let (with_ip, rest) = match rest {
            ["ip", v, r @ ..] => (truth(v)?, r),
            [v, r @ ..] if truth(v).is_ok() => (truth(v)?, r),
            r => (false, r),
        };
        if !crate::accounts::valid_name(name) {
            return Err(format!("Could not ban player {name}"));
        }
        let reason = reason_of(rest);
        self.bans.players.retain(|b| !b.target.eq_ignore_ascii_case(name));
        self.bans.players.push(Ban { target: name.to_string(), reason: reason.clone(), name: String::new() });
        self.effects.push(Effect::Bans);
        if let Some(p) = self.players.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
            self.effects.push(Effect::Kick(p.name.clone(), self.bans.refusal(name, None).unwrap()));
        }
        self.notify(format!("Banned player {name}"));
        if with_ip {
            let ip = self.ips.get(&name.to_ascii_lowercase()).cloned().ok_or(NOT_ONLINE)?;
            self.ban_address(ip, reason, name);
        }
        Ok(())
    }

    /// `CommandBanIp`: an address, or the address a player was last seen at.
    fn ban_ip(&mut self, args: &[&str]) -> R {
        let Some((&target, rest)) = args.split_first().filter(|(t, _)| !t.is_empty()) else { return Err(usage(BAN_IP)) };
        let (ip, name) = match crate::bans::parse_ip(target) {
            Some(ip) => (ip, ""),
            None => (self.ips.get(&target.to_ascii_lowercase()).cloned().ok_or(NOT_ONLINE)?, target),
        };
        self.ban_address(ip, reason_of(rest), name);
        Ok(())
    }

    /// `CommandBanIp.banIp`: everyone online from the address leaves.
    fn ban_address(&mut self, ip: String, reason: String, name: &str) {
        self.bans.ips.retain(|b| b.target != ip);
        self.bans.ips.push(Ban { target: ip.clone(), reason, name: name.to_string() });
        self.effects.push(Effect::Bans);
        let refusal = self.bans.refusal("", Some(&ip)).unwrap();
        let mut gone = Vec::new();
        for p in self.players {
            if self.ips.get(&p.name.to_ascii_lowercase()) == Some(&ip) {
                self.effects.push(Effect::Kick(p.name.clone(), refusal.clone()));
                gone.push(p.name.clone());
            }
        }
        if gone.is_empty() {
            self.notify(format!("Banned IP address {ip}"));
        } else {
            self.notify(format!("Banned IP address {ip} belonging to {}", gone.join(", ")));
        }
    }

    /// `CommandListBans`
    fn banlist(&mut self, args: &[&str]) -> R {
        let (count, list) = match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
            Some("ips") => (format!("There are {} total banned IP addresses:", self.bans.ips.len()), &self.bans.ips),
            None | Some("players") => (format!("There are {} total banned players:", self.bans.players.len()), &self.bans.players),
            _ => return Err(usage("/banlist [ips|players]")),
        };
        let names = list.iter().map(|b| b.target.clone()).collect::<Vec<_>>().join(", ");
        self.message(Line::plain(count));
        self.message(Line::plain(names));
        Ok(())
    }

    /// `CommandPardonPlayer`
    fn pardon(&mut self, args: &[&str]) -> R {
        let [name] = args else { return Err(usage("/pardon <name>")) };
        let before = self.bans.players.len();
        self.bans.players.retain(|b| !b.target.eq_ignore_ascii_case(name));
        if self.bans.players.len() == before {
            return Err(format!("Could not unban player {name}"));
        }
        self.effects.push(Effect::Bans);
        self.notify(format!("Unbanned player {name}"));
        Ok(())
    }

    /// `CommandPardonIp`
    fn pardon_ip(&mut self, args: &[&str]) -> R {
        let [address] = args else { return Err(usage("/pardon-ip <address>")) };
        let ip = crate::bans::parse_ip(address).ok_or("You have entered an invalid IP address")?;
        self.bans.ips.retain(|b| b.target != ip);
        self.effects.push(Effect::Bans);
        self.notify(format!("Unbanned IP address {ip}"));
        Ok(())
    }

    /// `CommandServerKick`
    fn kick(&mut self, args: &[&str]) -> R {
        let Some((&target, rest)) = args.split_first().filter(|(t, _)| !t.is_empty()) else { return Err(usage(KICK)) };
        let k = self.select(target).first().copied().ok_or("Could not find that player")?;
        let name = self.players[k].name.clone();
        let reason = rest.join(" ");
        if reason.trim().is_empty() {
            self.effects.push(Effect::Kick(name.clone(), "Kicked by an operator.".into()));
            self.notify(format!("Kicked {name} from the game"));
        } else {
            self.effects.push(Effect::Kick(name.clone(), reason.clone()));
            self.notify(format!("Kicked {name} from the game: '{reason}'"));
        }
        Ok(())
    }

    fn deop(&mut self, args: &[&str]) -> R {
        let [name] = args else { return Err(usage("/deop <player>")) };
        let before = self.ops.len();
        self.ops.retain(|o| !o.eq_ignore_ascii_case(name));
        if self.ops.len() == before {
            return Err(format!("Could not de-op {name}"));
        }
        self.effects.push(Effect::Ops);
        self.notify(format!("De-opped {name}"));
        Ok(())
    }

    fn op(&mut self, args: &[&str]) -> R {
        let [name] = args else { return Err(usage("/op <player>")) };
        if !crate::accounts::valid_name(name) {
            return Err(format!("Could not op {name}"));
        }
        if !self.ops.iter().any(|o| o.eq_ignore_ascii_case(name)) {
            self.ops.push(name.to_string());
            self.effects.push(Effect::Ops);
        }
        self.notify(format!("Opped {name}"));
        Ok(())
    }

    /// `CommandGameMode`. The mode is kept per player and sent to their session, which allows
    /// flying in creative and spectator only; survival play itself is not built yet.
    fn gamemode(&mut self, args: &[&str]) -> R {
        if args.is_empty() {
            return Err(usage("/gamemode <mode> [player]"));
        }
        let mode = parse_mode(args[0])?;
        let k = if args.len() >= 2 { self.player(args[1])? } else { self.sender_player()? };
        let (id, name) = (self.players[k].id, self.players[k].name.clone());
        self.world.update_player(&name, |s| s.game_mode = mode);
        self.effects.push(Effect::GameMode(id, mode));
        if self.feedback_on() {
            self.effects.push(Effect::Tell(id, Line::plain(format!("Your game mode has been updated to {}", mode_name(mode)))));
            let note = match mode {
                proto::GM_SURVIVAL | proto::GM_ADVENTURE => Some("Survival play is not built yet: you keep creative building, but cannot fly."),
                proto::GM_SPECTATOR => Some("Spectator play is not built yet: you fly, but still collide and build."),
                _ => None,
            };
            if let Some(note) = note {
                self.effects.push(Effect::Tell(id, Line::colored(note, "gray")));
            }
        }
        if Some(id) == self.sender.id {
            self.notify(format!("Set own game mode to {}", mode_name(mode)));
        } else {
            self.notify(format!("Set {name}'s game mode to {}", mode_name(mode)));
        }
        Ok(())
    }

    /// `CommandGameRule`
    fn gamerule(&mut self, args: &[&str]) -> R {
        match args {
            [] => {
                let names: Vec<&str> = self.world.rules().collect();
                self.message(Line::plain(join_nice(&names)));
            }
            [rule] => match self.world.rule(rule) {
                Some(v) => self.message(Line::plain(format!("{rule} = {v}"))),
                None => return Err(format!("No game rule called '{rule}' is available")),
            },
            [rule, rest @ ..] => {
                let value = rest.join(" ");
                let boolean = DEFAULT_RULES.iter().any(|(k, v)| k == rule && (*v == "true" || *v == "false"));
                if boolean && value != "true" && value != "false" {
                    return Err(format!("'{value}' is not true or false"));
                }
                self.world.set_rule(rule, &value);
                if *rule == "doDaylightCycle" {
                    self.effects.push(Effect::Time);
                }
                self.notify(format!("Game rule {rule} has been updated to {value}"));
            }
        }
        Ok(())
    }

    /// `CommandHelp`: seven usages a page, only the commands the sender may use.
    /// DROPPED: the first page's footer about the Tab key; nothing completes commands here.
    fn help(&mut self, args: &[&str]) -> R {
        let list: Vec<&Command> = COMMANDS.iter().filter(|c| self.may(c.perm)).collect();
        let pages = (list.len() - 1) / 7;
        let page = match args.first() {
            None => 0,
            Some(a) => match parse_int(a, 1, pages as i32 + 1) {
                Ok(p) => p as usize - 1,
                Err(e) => {
                    if let Some(c) = super::find_command(a) {
                        return Err(usage(c.usage));
                    }
                    if a.parse::<i32>().is_err() {
                        return Err(UNKNOWN.into());
                    }
                    return Err(e);
                }
            },
        };
        let header = format!("--- Showing help page {} of {} (/help <page>) ---", page + 1, pages + 1);
        self.message(Line::colored(header, "dark_green"));
        for c in &list[page * 7..((page + 1) * 7).min(list.len())] {
            self.message(Line::plain(c.usage));
        }
        Ok(())
    }

    /// `CommandKill`, then the respawn: at the player's `/spawnpoint`, else on the top block at
    /// the world spawn (`PlayerList.recreatePlayerEntity`).
    fn kill(&mut self, args: &[&str]) -> R {
        let k = match args.first() {
            Some(a) => self.player(a)?,
            None => self.sender_player()?,
        };
        let (id, name) = (self.players[k].id, self.players[k].name.clone());
        if self.world.rule("showDeathMessages") != Some("false") {
            self.effects.push(Effect::Broadcast(format!("{name} fell out of the world")));
        }
        let tp = match self.world.player(&name).and_then(|s| s.spawn) {
            // EntityPlayer.getBedSpawnLocation with a forced spawn point: right there.
            Some([x, y, z]) => Teleport { pos: [x as f64 + 0.5, y as f64 + 0.1, z as f64 + 0.5], yaw: 0.0, pitch: 0.0, flags: proto::TP_RESPAWN },
            None => world_spawn(self.world),
        };
        self.effects.push(Effect::Teleport(id, tp));
        self.notify(format!("Killed {name}"));
        Ok(())
    }

    /// `CommandListPlayers`
    fn list(&mut self, _: &[&str]) -> R {
        self.message(Line::plain(format!("There are {}/{} players online:", self.players.len(), self.max_players)));
        let names: Vec<&str> = self.players.iter().map(|p| p.name.as_str()).collect();
        self.message(Line::plain(names.join(", ")));
        Ok(())
    }

    /// `CommandEmote`
    fn emote(&mut self, args: &[&str]) -> R {
        if args.is_empty() {
            return Err(usage("/me <action ...>"));
        }
        self.effects.push(Effect::Broadcast(format!("* {} {}", self.sender.name, args.join(" "))));
        Ok(())
    }

    /// `CommandBroadcast`
    fn say(&mut self, args: &[&str]) -> R {
        if args.is_empty() || args[0].is_empty() {
            return Err(usage("/say <message ...>"));
        }
        self.effects.push(Effect::Broadcast(format!("[{}] {}", self.sender.name, args.join(" "))));
        Ok(())
    }

    /// `CommandShowSeed`
    fn seed(&mut self, _: &[&str]) -> R {
        self.message(Line::plain(format!("Seed: {}", self.seed)));
        Ok(())
    }

    /// `CommandSetDefaultSpawnpoint`
    fn setworldspawn(&mut self, args: &[&str]) -> R {
        let at = match args.len() {
            0 => Self::block_of(self.pos_of(self.sender_player()?).0),
            3 => block_pos(Self::block_of(self.sender_pos()), args)?,
            _ => return Err(usage("/setworldspawn [<x> <y> <z>]")),
        };
        self.world.spawn = at;
        self.notify(format!("Set the world spawn point to ({}, {}, {})", at[0], at[1], at[2]));
        Ok(())
    }

    /// `/spawn [player]`: back to the world spawn, standing on the top block of its column as a
    /// respawn does. Anyone may send themselves; sending another player takes what `/tp` takes.
    fn spawn(&mut self, args: &[&str]) -> R {
        if args.len() > 1 {
            return Err(usage("/spawn [player]"));
        }
        let k = match args.first() {
            Some(a) => self.player(a)?,
            None => self.sender_player()?,
        };
        let (id, name) = (self.players[k].id, self.players[k].name.clone());
        if Some(id) != self.sender.id && !self.may(Perm::Ops) {
            return Err(PERMISSION.into());
        }
        self.effects.push(Effect::Teleport(id, world_spawn(self.world)));
        let [x, y, z] = self.world.spawn;
        self.notify(format!("Teleported {name} to the world spawn ({x}, {y}, {z})"));
        Ok(())
    }

    /// `CommandSetSpawnpoint`
    fn spawnpoint(&mut self, args: &[&str]) -> R {
        if args.len() > 1 && args.len() < 4 {
            return Err(usage("/spawnpoint [player] [<x> <y> <z>]"));
        }
        let k = if args.is_empty() { self.sender_player()? } else { self.player(args[0])? };
        let at = if args.len() > 3 { block_pos(Self::block_of(self.sender_pos()), &args[1..4])? } else { Self::block_of(self.pos_of(k).0) };
        let name = self.players[k].name.clone();
        self.world.update_player(&name, |s| s.spawn = Some(at));
        self.notify(format!("Set {name}'s spawn point to ({}, {}, {})", at[0], at[1], at[2]));
        Ok(())
    }

    /// `CommandMessage`
    fn tell(&mut self, args: &[&str]) -> R {
        if args.len() < 2 {
            return Err(usage("/tell <player> <private message ...>"));
        }
        let k = self.player(args[0])?;
        if Some(self.players[k].id) == self.sender.id {
            return Err("You can't send a private message to yourself!".into());
        }
        let msg = args[1..].join(" ");
        let whisper = |text: String| Line { text, color: Some("gray"), italic: true };
        self.effects.push(Effect::Tell(self.players[k].id, whisper(format!("{} whispers to you: {msg}", self.sender.name))));
        self.message(whisper(format!("You whisper to {}: {msg}", self.players[k].name)));
        Ok(())
    }

    /// `CommandTime`. `noon` and `midnight` are 1.13's names; 1.12 takes day, night and numbers.
    fn time(&mut self, args: &[&str]) -> R {
        let u = || usage("/time <set|add|query> <value>");
        if args.len() < 2 {
            return Err(u());
        }
        match args[0] {
            "set" => {
                let t = match args[1] {
                    "day" => 1000,
                    "night" => 13000,
                    "noon" => 6000,
                    "midnight" => 18000,
                    n => parse_int(n, 0, i32::MAX)?,
                };
                self.world.set_time(t as i64);
                self.effects.push(Effect::Time);
                self.notify(format!("Set the time to {t}"));
            }
            "add" => {
                let n = parse_int(args[1], 0, i32::MAX)?;
                let t = self.world.time() + n as i64;
                self.world.set_time(t);
                self.effects.push(Effect::Time);
                self.notify(format!("Added {n} to the time"));
            }
            "query" => {
                let v = match args[1] {
                    "daytime" => self.world.time() % 24000,
                    "day" => self.world.time() / 24000 % i32::MAX as i64,
                    "gametime" => self.world.total_time() % i32::MAX as i64,
                    _ => return Err(u()),
                };
                self.notify(format!("Time is {v}"));
            }
            _ => return Err(u()),
        }
        Ok(())
    }

    /// `CommandTP`: coordinates are relative to the player who moves, and stay relative on the
    /// way to their session, which adds them to where it is (`SPacketPlayerPosLook` flags).
    fn tp(&mut self, args: &[&str]) -> R {
        let u = || usage("/tp [target player] <destination player> OR /tp [target player] <x> <y> <z> [<yaw> <pitch>]");
        if args.is_empty() {
            return Err(u());
        }
        let (k, mut i) = if matches!(args.len(), 2 | 4 | 6) { (self.player(args[0])?, 1) } else { (self.sender_player()?, 0) };
        let (id, name) = (self.players[k].id, self.players[k].name.clone());
        if args.len() == 1 || args.len() == 2 {
            let d = self.player(args[args.len() - 1])?;
            let (pos, yaw, pitch) = self.pos_of(d);
            self.effects.push(Effect::Teleport(id, Teleport { pos, yaw, pitch, flags: 0 }));
            self.notify(format!("Teleported {name} to {}", self.players[d].name));
            return Ok(());
        }
        if args.len() < i + 3 {
            return Err(u());
        }
        let (pos, yaw, pitch) = self.pos_of(k);
        let x = coordinate(pos[0], args[i], true, -30_000_000, 30_000_000)?;
        let y = coordinate(pos[1], args[i + 1], false, -4096, 4096)?;
        let z = coordinate(pos[2], args[i + 2], true, -30_000_000, 30_000_000)?;
        i += 3;
        let ry = coordinate(yaw as f64, args.get(i).copied().unwrap_or("~"), false, -30_000_000, 30_000_000)?;
        let rp = coordinate(pitch as f64, args.get(i + 1).copied().unwrap_or("~"), false, -30_000_000, 30_000_000)?;
        let mut flags = 0;
        for (c, f) in [(x, proto::TP_REL_X), (y, proto::TP_REL_Y), (z, proto::TP_REL_Z), (ry, proto::TP_REL_YAW), (rp, proto::TP_REL_PITCH)] {
            if c.relative {
                flags |= f;
            }
        }
        let rot = |c: Coord| if c.relative { c.amount as f32 } else { wrap_degrees(c.amount as f32) };
        let tp = Teleport { pos: [x.amount, y.amount, z.amount], yaw: rot(ry), pitch: rot(rp), flags };
        self.effects.push(Effect::Teleport(id, tp));
        self.notify(format!("Teleported {name} to {}, {}, {}", java_double(x.result), java_double(y.result), java_double(z.result)));
        Ok(())
    }

    /// `CommandTeleport`: coordinates relative to whoever runs it, rotation to the one who moves.
    fn teleport(&mut self, args: &[&str]) -> R {
        if args.len() < 4 {
            return Err(usage("/teleport <entity> <x> <y> <z> [<y-rot> <x-rot>]"));
        }
        let k = self.player(args[0])?;
        let base = self.sender_pos();
        let x = coordinate(base[0], args[1], true, -30_000_000, 30_000_000)?;
        let y = coordinate(base[1], args[2], false, -4096, 4096)?;
        let z = coordinate(base[2], args[3], true, -30_000_000, 30_000_000)?;
        let (_, yaw, pitch) = self.pos_of(k);
        let ry = coordinate(yaw as f64, args.get(4).copied().unwrap_or("~"), false, -30_000_000, 30_000_000)?;
        let rp = coordinate(pitch as f64, args.get(5).copied().unwrap_or("~"), false, -30_000_000, 30_000_000)?;
        let mut flags = 0;
        let mut rot = |c: Coord, f: u8| {
            if c.relative {
                flags |= f;
                c.amount as f32
            } else {
                wrap_degrees(c.amount as f32)
            }
        };
        let (yaw, pitch) = (rot(ry, proto::TP_REL_YAW), rot(rp, proto::TP_REL_PITCH));
        let (id, name) = (self.players[k].id, self.players[k].name.clone());
        self.effects.push(Effect::Teleport(id, Teleport { pos: [x.result, y.result, z.result], yaw, pitch, flags }));
        self.notify(format!("Teleported {name} to {}, {}, {}", java_double(x.result), java_double(y.result), java_double(z.result)));
        Ok(())
    }

    /// `CommandWeather`. Nothing draws rain yet; the weather is kept for when something does.
    fn weather(&mut self, args: &[&str]) -> R {
        let u = || usage("/weather <clear|rain|thunder> [duration in seconds]");
        if args.is_empty() || args.len() > 2 {
            return Err(u());
        }
        let ticks = match args.get(1) {
            Some(s) => parse_int(s, 1, 1_000_000)? * 20,
            None => (300 + (crate::config::random_u64() % 600) as i32) * 20,
        };
        let (raining, thundering, text) = match args[0].to_ascii_lowercase().as_str() {
            "clear" => (false, false, "Changing to clear weather"),
            "rain" => (true, false, "Changing to rainy weather"),
            "thunder" => (true, true, "Changing to rain and thunder"),
            _ => return Err(u()),
        };
        self.world.weather = crate::world::Weather { raining, thundering, duration: ticks };
        self.notify(text.into());
        if raining {
            self.message(Line::colored("Rain is not drawn yet; the weather is kept for when it is.", "gray"));
        }
        Ok(())
    }
}
