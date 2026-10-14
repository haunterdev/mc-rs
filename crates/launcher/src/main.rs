//! `mc-rs-launcher.exe`, the exe players download (tools/dist.py). It opens a launcher window
//! (ui.rs): seed field and Play, which installs the viewer it carries (payload.rs), fetches the
//! 1.12.2 client jar from Mojang when the PC has none (net.rs), starts the viewer with its log
//! kept (game.rs), and offers a newer `mc-rs-launcher.exe` from the update channel (update.rs). Join a
//! server (join.rs) plays on a friend's mc-rs server with the game rendered here.
//!
//! Viewer arguments (`mc-rs-launcher.exe --smoke 20`, `--seed 42`) skip the window: the viewer is
//! installed, run with them, and its exit code comes back. `--uri <link>` is an `mcrs://` link
//! from a web page (uri.rs); it opens the window, and nothing after the link is read.

#![windows_subsystem = "windows"]

mod game;
mod join;
mod net;
mod payload;
mod ui;
mod update;
mod uri;

use std::ffi::OsString;
use std::io;

fn main() {
    update::clean_up_after_update();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let updated_from = match args.as_slice() {
        [flag, v] if flag == "--updated-from" => Some(v.to_string_lossy().into_owned()),
        _ => None,
    };
    let link = match args.as_slice() {
        [flag, rest @ ..] if flag == "--uri" => {
            Some(rest.first().map_or(Err("the link was empty".into()), |u| uri::parse(&u.to_string_lossy())))
        }
        _ => None,
    };
    if args.is_empty() || updated_from.is_some() || link.is_some() {
        // A build without the viewer (plain cargo) leaves the links to the real launcher.
        if payload::carried() {
            let _ = uri::register();
        }
        if let Err(e) = ui::run(updated_from, link) {
            error(&format!("mc-rs could not open its window: {e}"));
            std::process::exit(1);
        }
        return;
    }
    let code = passthrough(&args).unwrap_or_else(|e| {
        error(&format!("mc-rs could not start: {e}"));
        1
    });
    std::process::exit(code);
}

fn passthrough(args: &[OsString]) -> io::Result<i32> {
    let exe = payload::install(&|_, _| {})?;
    let status = std::process::Command::new(exe).args(args).status()?;
    Ok(status.code().unwrap_or(1))
}

fn error(text: &str) {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
        }
        const MB_ICONERROR: u32 = 0x10;
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (text, caption) = (wide(text), wide("mc-rs launcher"));
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_ICONERROR) };
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
}
