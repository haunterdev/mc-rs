//! A Cloudflare tunnel to the web port. A quick tunnel (`cloudflared tunnel --url`) names its
//! public `https://*.trycloudflare.com` link in its log, a new one every start. A named tunnel
//! (`tunnel_name` in server.json) runs from `cloudflared.yml`, written to the data folder
//! each start, and its link is `https://<hostname>`, told once the first connection registers.
//! The link is printed and written to `link.txt` in the data folder. cloudflared's whole log goes
//! to `tunnel.log` there, which shows when the tunnel itself dropped and reconnected.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::config::Config;

fn find(cfg: &Config) -> Option<PathBuf> {
    if !cfg.cloudflared.is_empty() {
        return Some(PathBuf::from(&cfg.cloudflared));
    }
    let exe = format!("cloudflared{}", std::env::consts::EXE_SUFFIX);
    let on_path = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(&exe)).find(|f| f.is_file()));
    on_path.or_else(|| {
        [r"C:\Program Files (x86)\cloudflared\cloudflared.exe", r"C:\Program Files\cloudflared\cloudflared.exe"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
    })
}

/// The first `https://...trycloudflare.com` in a log line.
pub fn link_in(line: &str) -> Option<&str> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '|').unwrap_or(rest.len());
    let url = &rest[..end];
    url.contains(".trycloudflare.com").then_some(url)
}

/// The named tunnel's `cloudflared.yml`: the hostname to the web port, anything else 404. With no
/// hostname every name the tunnel is routed to reaches the web port.
pub fn named_config(name: &str, hostname: &str, port: u16) -> String {
    let service = format!("http://127.0.0.1:{port}");
    let ingress = if hostname.is_empty() {
        format!("  - service: {service}\n")
    } else {
        format!("  - hostname: {hostname}\n    service: {service}\n  - service: http_status:404\n")
    };
    format!("# Written by mc-rs-server on every start from server.json (tunnel_name, hostname, port).\ntunnel: {name}\ningress:\n{ingress}")
}

/// Starts the tunnel; the child stops when the returned handle drops.
pub async fn start(cfg: &Config, dir: &Path) -> Option<Child> {
    let Some(exe) = find(cfg) else {
        println!("  no cloudflared found, so no public link (install it, or set \"cloudflared\" in server.json)");
        return None;
    };
    let mut cmd = Command::new(&exe);
    let named = !cfg.tunnel_name.is_empty();
    if named {
        let yml = dir.join("cloudflared.yml");
        if let Err(e) = std::fs::write(&yml, named_config(&cfg.tunnel_name, &cfg.hostname, cfg.port)) {
            println!("  could not write {}: {e}", yml.display());
            return None;
        }
        cmd.arg("tunnel").arg("--no-autoupdate").arg("--config").arg(&yml).arg("run").arg(&cfg.tunnel_name);
    } else {
        cmd.args(["tunnel", "--no-autoupdate", "--url", &format!("http://127.0.0.1:{}", cfg.port)]);
    }
    let fixed = (named && !cfg.hostname.is_empty()).then(|| format!("https://{}", cfg.hostname));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            println!("  could not start {}: {e}", exe.display());
            return None;
        }
    };
    let stderr = child.stderr.take()?;
    let link_file = dir.join("link.txt");
    let mut log = std::fs::File::create(dir.join("tunnel.log")).ok();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut told = false;
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(f) = log.as_mut() {
                let _ = writeln!(f, "{line}");
            }
            if told {
                continue;
            }
            let url = match &fixed {
                Some(url) => line.contains("Registered tunnel connection").then_some(url.as_str()),
                None => link_in(&line),
            };
            if let Some(url) = url {
                told = true;
                let _ = std::fs::write(&link_file, url);
                println!();
                println!("  Public link for your friends: {url}");
                println!("  (also in {})", link_file.display());
                println!();
            }
        }
    });
    Some(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_link_in_cloudflared_output() {
        let line = "2026-10-12T10:00:00Z INF |  https://clusters-subsidiaries-julian-fathers.trycloudflare.com                    |";
        assert_eq!(link_in(line), Some("https://clusters-subsidiaries-julian-fathers.trycloudflare.com"));
        assert_eq!(link_in("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
        assert_eq!(link_in("see https://developers.cloudflare.com/ for docs"), None);
    }

    #[test]
    fn named_tunnel_config_routes_the_hostname_to_the_port() {
        let yml = named_config("mc-rs", "play.example.com", 25580);
        assert!(yml.contains("tunnel: mc-rs\n"));
        assert!(yml.contains("  - hostname: play.example.com\n    service: http://127.0.0.1:25580\n  - service: http_status:404\n"));
        assert!(named_config("mc-rs", "", 8080).ends_with("ingress:\n  - service: http://127.0.0.1:8080\n"));
    }
}
