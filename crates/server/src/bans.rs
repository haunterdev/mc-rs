//! Bans, as 1.12.2's `UserListBans` and `UserListIPBans`: names and IP addresses the
//! server turns away at the login, the guest sign-in and both game sockets, kept in `bans.json`
//! in the data folder. A player's address is the socket's peer, or behind the tunnel (cloudflared
//! on this PC, so a loopback peer) the `CF-Connecting-IP` Cloudflare adds.

use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// `BanEntry`'s default reason.
pub const DEFAULT_REASON: &str = "Banned by an operator.";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ban {
    /// A player name, or an IP address.
    pub target: String,
    pub reason: String,
    /// Who an IP ban was taken from, when it named a player.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bans {
    #[serde(default)]
    pub players: Vec<Ban>,
    #[serde(default)]
    pub ips: Vec<Ban>,
}

impl Bans {
    pub fn load(dir: &Path) -> Bans {
        std::fs::read_to_string(dir.join("bans.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let path = dir.join("bans.json");
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap())?;
        std::fs::rename(&tmp, &path)
    }

    pub fn player(&self, name: &str) -> Option<&Ban> {
        self.players.iter().find(|b| b.target.eq_ignore_ascii_case(name))
    }

    pub fn ip(&self, ip: &str) -> Option<&Ban> {
        self.ips.iter().find(|b| b.target == ip)
    }

    /// The line a banned player reads instead of joining
    /// (`multiplayer.disconnect.banned.reason`, `multiplayer.disconnect.banned_ip.reason`).
    pub fn refusal(&self, name: &str, ip: Option<&str>) -> Option<String> {
        if let Some(b) = self.player(name) {
            return Some(format!("You are banned from this server.\nReason: {}", b.reason));
        }
        let b = self.ip(ip?)?;
        Some(format!("Your IP address is banned from this server.\nReason: {}", b.reason))
    }
}

/// `InetAddresses.isInetAddress`, written the way [`client_ip`] writes it.
pub fn parse_ip(s: &str) -> Option<String> {
    s.trim().parse::<IpAddr>().ok().map(|ip| ip.to_canonical().to_string())
}

/// Where a request came from: the peer, unless the peer is this PC (the tunnel), which forwards
/// the player's own address in `CF-Connecting-IP`. A header from anyone else is not trusted.
pub fn client_ip(peer: SocketAddr, headers: &axum::http::HeaderMap) -> String {
    let peer = peer.ip().to_canonical();
    if peer.is_loopback() {
        if let Some(ip) = headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()).and_then(parse_ip) {
            return ip;
        }
    }
    peer.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_and_the_tunnel_address() {
        let mut b = Bans::default();
        b.players.push(Ban { target: "Bob".into(), reason: DEFAULT_REASON.into(), name: String::new() });
        b.ips.push(Ban { target: "203.0.113.9".into(), reason: "griefing".into(), name: "Cat".into() });
        assert_eq!(b.refusal("bob", None).unwrap(), "You are banned from this server.\nReason: Banned by an operator.");
        assert_eq!(b.refusal("Dan", Some("203.0.113.9")).unwrap(), "Your IP address is banned from this server.\nReason: griefing");
        assert_eq!(b.refusal("Dan", Some("203.0.113.10")), None);

        let mut h = axum::http::HeaderMap::new();
        h.insert("cf-connecting-ip", "203.0.113.9".parse().unwrap());
        assert_eq!(client_ip("127.0.0.1:5000".parse().unwrap(), &h), "203.0.113.9");
        assert_eq!(client_ip("[::ffff:127.0.0.1]:5000".parse().unwrap(), &h), "203.0.113.9");
        assert_eq!(client_ip("192.168.1.20:5000".parse().unwrap(), &h), "192.168.1.20");
        assert_eq!(parse_ip("::ffff:10.0.0.1").as_deref(), Some("10.0.0.1"));
        assert_eq!(parse_ip("bob"), None);

        let dir = std::env::temp_dir().join(format!("mcrs-bans-{}", crate::config::random_u64()));
        std::fs::create_dir_all(&dir).unwrap();
        b.save(&dir).unwrap();
        assert_eq!(Bans::load(&dir), b);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
