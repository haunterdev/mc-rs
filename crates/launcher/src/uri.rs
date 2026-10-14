//! `mcrs://` links, the ones the website's Play and Connect buttons open:
//!
//! - `mcrs://play` opens the launcher window.
//! - `mcrs://join/<host>[:<port>]` opens it on Join a server with the address filled in
//!   (join.rs picks the port when it is left out: a tunnel's 443, or a local server's 25580).
//!
//! Windows starts `mc-rs-launcher.exe --uri "<link>"` for a link (`register` writes that command under
//! `HKCU\Software\Classes\mcrs`, the per-user place, no elevation). A link is untrusted text from
//! any web page, so `parse` accepts only those two shapes, a host of letters, digits, dots and
//! hyphens, and a numeric port; anything else is refused. Nothing from a link reaches a command
//! line, and main.rs reads only the one argument after `--uri`.

pub const SCHEME: &str = "mcrs";
const MAX_LEN: usize = 300;

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    Play,
    Join(Server),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub host: String,
    pub port: Option<u16>,
}

impl std::fmt::Display for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.port {
            Some(p) => write!(f, "{}:{p}", self.host),
            None => write!(f, "{}", self.host),
        }
    }
}

pub fn parse(uri: &str) -> Result<Link, String> {
    if uri.len() > MAX_LEN {
        return Err("the link is too long".into());
    }
    let rest = uri
        .split_once("://")
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case(SCHEME))
        .map(|(_, rest)| rest)
        .ok_or("the link does not start with mcrs://")?;
    // Browsers may add a trailing slash.
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.eq_ignore_ascii_case("play") {
        return Ok(Link::Play);
    }
    let addr = match rest.split_once('/') {
        Some((verb, addr)) if verb.eq_ignore_ascii_case("join") => addr,
        _ => return Err("the link is neither mcrs://play nor mcrs://join/<address>".into()),
    };
    let (host, port) = match addr.rsplit_once(':') {
        Some((h, p)) => (h, Some(parse_port(p)?)),
        None => (addr, None),
    };
    Ok(Link::Join(Server { host: parse_host(host)?, port }))
}

pub fn parse_port(p: &str) -> Result<u16, String> {
    let bad = || format!("\"{p}\" is not a port number");
    if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    match p.parse::<u16>() {
        Ok(0) | Err(_) => Err(bad()),
        Ok(n) => Ok(n),
    }
}

/// A DNS name or dotted IPv4 address, lowercased.
pub fn parse_host(h: &str) -> Result<String, String> {
    let bad = || "the server address may hold only letters, digits, dots and hyphens".to_string();
    if h.is_empty() || h.len() > 253 {
        return Err("the server address is empty or too long".into());
    }
    for label in h.split('.') {
        let ok = (1..=63).contains(&label.len())
            && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-');
        if !ok {
            return Err(bad());
        }
    }
    Ok(h.to_ascii_lowercase())
}

/// Points `mcrs://` links at this exe, for the current user only. Skipped when the key already
/// names this exe, so a normal start writes nothing.
pub fn register() -> std::io::Result<()> {
    register_as(SCHEME)
}

#[cfg(windows)]
fn register_as(scheme: &str) -> std::io::Result<()> {
    use std::ffi::c_void;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegSetKeyValueW(key: *mut c_void, sub: *const u16, name: *const u16, kind: u32, data: *const c_void, len: u32) -> i32;
        fn RegGetValueW(key: *mut c_void, sub: *const u16, name: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, len: *mut u32) -> i32;
    }
    // HKEY_CURRENT_USER is ((HKEY)(LONG)0x80000001), sign-extended to the pointer width.
    let hkcu = 0x8000_0001u32 as i32 as isize as *mut c_void;
    const REG_SZ: u32 = 1;
    const RRF_RT_REG_SZ: u32 = 2;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();

    let exe = std::env::current_exe()?;
    let exe = exe.display();
    let root = format!(r"Software\Classes\{scheme}");
    let command_key = format!(r"{root}\shell\open\command");
    let command = format!("\"{exe}\" --uri \"%1\"");

    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let current = unsafe {
        RegGetValueW(hkcu, wide(&command_key).as_ptr(), std::ptr::null(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut len)
    };
    if current == 0 {
        let got = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
        if got == command {
            return Ok(());
        }
    }

    let set = |key: &str, name: Option<&str>, value: &str| -> std::io::Result<()> {
        let data = wide(value);
        let name = name.map(wide);
        let r = unsafe {
            RegSetKeyValueW(
                hkcu,
                wide(key).as_ptr(),
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if r == 0 { Ok(()) } else { Err(std::io::Error::from_raw_os_error(r)) }
    };
    set(&root, None, "URL:mc-rs")?;
    set(&root, Some("URL Protocol"), "")?;
    set(&format!(r"{root}\DefaultIcon"), None, &format!("\"{exe}\",0"))?;
    set(&command_key, None, &command)
}

#[cfg(not(windows))]
fn register_as(_scheme: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(host: &str, port: Option<u16>) -> Result<Link, String> {
        Ok(Link::Join(Server { host: host.into(), port }))
    }

    #[test]
    fn play_and_join_links_parse() {
        assert_eq!(parse("mcrs://play"), Ok(Link::Play));
        assert_eq!(parse("MCRS://Play/"), Ok(Link::Play));
        assert_eq!(parse("mcrs://join/example.gl.joinmc.link"), join("example.gl.joinmc.link", None));
        assert_eq!(parse("mcrs://join/Name-1.ply.gg:41234/"), join("name-1.ply.gg", Some(41234)));
        assert_eq!(parse("mcrs://join/147.185.221.5:5000"), join("147.185.221.5", Some(5000)));
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            "",
            "mcrs:play",
            "http://join/example.com",
            "mcrs://",
            "mcrs://open",
            "mcrs://join/",
            "mcrs://join/example.com:",
            "mcrs://join/example.com:0",
            "mcrs://join/example.com:65536",
            "mcrs://join/example.com:+80",
            "mcrs://join/example.com:80:80",
            "mcrs://join/-seed.example.com",
            "mcrs://join/a..b",
            "mcrs://join/a b",
            "mcrs://join/a\"b",
            "mcrs://join/example.com\" --seed \"1",
            "mcrs://join/example.com/extra",
            "mcrs://join/%65xample.com",
            "mcrs://join/[::1]:25565",
            "mcrs://join/user@example.com",
            "mcrs://join/exa\u{fffd}mple.com",
        ] {
            assert!(parse(bad).is_err(), "{bad:?} parsed");
        }
        let long = format!("mcrs://join/{}.com", "a".repeat(300));
        assert!(parse(&long).is_err());
        let label = format!("mcrs://join/{}.com", "a".repeat(64));
        assert!(parse(&label).is_err());
    }

    /// Writes a throwaway scheme under HKCU, reads it back with reg.exe and removes it:
    /// `cargo test --release -p launcher -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn registers_a_scheme_for_this_user() {
        let scheme = "mcrs-selftest";
        let key = format!(r"HKCU\Software\Classes\{scheme}");
        let query = |sub: &str| {
            let out = std::process::Command::new("reg").args(["query", &format!("{key}{sub}"), "/ve"]).output().unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        register_as(scheme).unwrap();
        register_as(scheme).unwrap();
        let exe = std::env::current_exe().unwrap();
        let command = query(r"\shell\open\command");
        let _ = std::process::Command::new("reg").args(["delete", &key, "/f"]).output();
        assert!(command.contains(&format!("\"{}\" --uri \"%1\"", exe.display())), "{command}");
        assert!(query("").is_empty(), "the test key was not removed");
    }

    #[test]
    fn server_shows_as_host_and_port() {
        assert_eq!(Server { host: "a.b".into(), port: Some(7) }.to_string(), "a.b:7");
        assert_eq!(Server { host: "a.b".into(), port: None }.to_string(), "a.b");
    }
}
