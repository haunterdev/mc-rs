//! mc-rs-server: hosts one shared mc-rs world and lets friends play it in a web browser. Each
//! signed-in player gets a viewer session on this PC (`viewer --stream`), whose picture is H.264
//! encoded on the GPU and streamed to the page, while the page's keys and mouse go back to it.
//!
//! `mc-rs-server [--data DIR] [--port N] [--no-tunnel] [--rollback BACKUP]`
//! `mc-rs-server --set-password NAME PASSWORD` creates or resets an account and exits.
//! `mc-rs-server --op NAME` makes a name an operator (server.json "ops") and exits.
//! `--rollback BACKUP` puts the world back as that backup in `backups/` left it before starting
//! (backup.rs); `/backup` and `/rollback` do the same from chat or the console.
//!
//! Lines typed into the server window run as commands with every permission (commands.rs).
//!
//! Files live in `%APPDATA%\mc-rs\server` (`--data` moves them): `server.json` (settings, made
//! on the first start), `accounts.json`, `tokens.json`, `edits.bin`, `players.json`, `world.json`.
//! With `"tunnel": true` the server starts a Cloudflare tunnel (a quick one, or `tunnel_name`) and
//! prints the public link.
//! The launcher's Join plays the same world rendered on the friend's own PC (native.rs).

mod accounts;
mod backup;
mod bans;
mod commands;
mod config;
mod game;
mod hub;
mod job;
mod native;
mod tunnel;
mod web;
mod world;

use std::path::PathBuf;
use std::time::Duration;

use accounts::Accounts;
use config::Config;
use hub::Hub;
use world::World;

fn usage() -> ! {
    eprintln!("usage: mc-rs-server [--data DIR] [--port N] [--no-tunnel] [--rollback BACKUP] | --set-password NAME PASSWORD | --op NAME");
    std::process::exit(2);
}

#[tokio::main]
async fn main() {
    let mut dir = config::default_dir();
    let mut port = None;
    let mut no_tunnel = false;
    let mut set_password = None;
    let mut make_op = None;
    let mut rollback = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => dir = PathBuf::from(it.next().unwrap_or_else(|| usage())),
            "--port" => port = Some(it.next().and_then(|p| p.parse::<u16>().ok()).unwrap_or_else(|| usage())),
            "--no-tunnel" => no_tunnel = true,
            "--set-password" => {
                let (Some(n), Some(p)) = (it.next(), it.next()) else { usage() };
                set_password = Some((n, p));
            }
            "--op" => make_op = Some(it.next().unwrap_or_else(|| usage())),
            "--rollback" => rollback = Some(it.next().unwrap_or_else(|| usage())),
            _ => usage(),
        }
    }
    let mut cfg = Config::load(&dir).unwrap_or_else(|e| {
        eprintln!("mc-rs-server: {e}");
        std::process::exit(1);
    });
    if let Some((name, password)) = set_password {
        let mut a = Accounts::load(&dir);
        match a.set_password(&name, &password) {
            Ok(()) => println!("account {name} is set"),
            Err(e) => {
                eprintln!("{}", e.message());
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(name) = make_op {
        if !cfg.ops.iter().any(|o| o.eq_ignore_ascii_case(&name)) {
            cfg.ops.push(name.clone());
        }
        match Config::save_ops(&dir, &cfg.ops) {
            Ok(()) => println!("{name} is an operator"),
            Err(e) => {
                eprintln!("mc-rs-server: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(p) = port {
        cfg.port = p;
    }
    if no_tunnel {
        cfg.tunnel = false;
    }
    // Before anything else: a second server would only fail to listen, after loading the world.
    let addr = format!("{}:{}", cfg.bind, cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            eprintln!("mc-rs-server: port {} is in use: another mc-rs-server is running (http://127.0.0.1:{}).", cfg.port, cfg.port);
            eprintln!("  Close its window first, or start this one with --port N.");
        } else {
            eprintln!("mc-rs-server: could not listen on {addr}: {e}");
        }
        std::process::exit(1);
    });
    if !job::bind_children() {
        println!("  (Windows refused the job object: close the server with Ctrl+C so cloudflared stops with it)");
    }
    // After the port is ours (so no server is running on this folder), before the world loads.
    match backup::rollback_at_start(&dir, rollback) {
        Ok(Some((name, safety))) => println!("  rolled the world back to backup {name}; the world as it was is backup {safety}"),
        Ok(None) => {}
        Err(e) => {
            eprintln!("mc-rs-server: could not roll back: {e}");
            std::process::exit(1);
        }
    }
    let world = World::load(&dir).unwrap_or_else(|e| {
        eprintln!("mc-rs-server: could not open the world in {}: {e}", dir.display());
        std::process::exit(1);
    });
    let accounts = Accounts::load(&dir);
    println!("mc-rs-server {}: world in {}", env!("CARGO_PKG_VERSION"), dir.display());
    println!(
        "  seed {}, {} saved block edits, {} accounts, up to {} players, pictures up to {}x{} at {} fps ({}); timing reports in latency.log",
        cfg.seed,
        world.edit_count(),
        accounts.count(),
        cfg.max_players,
        cfg.width,
        cfg.height,
        cfg.fps,
        cfg.encoder
    );
    match cfg.viewer_path() {
        Some(v) => println!("  viewer: {}", v.display()),
        None => println!("  no viewer exe found: put viewer.exe next to this exe or set \"viewer\" in server.json"),
    }
    let (hub, sessions) = Hub::new(cfg.clone(), world, accounts, dir.clone()).await.unwrap_or_else(|e| {
        eprintln!("mc-rs-server: {e}");
        std::process::exit(1);
    });
    // The game thread (game/mod.rs), before any session can join.
    let log = hub.clone();
    let (game_tx, game_thread) = game::start(cfg.clone(), dir.clone(), hub.world.clone(), Box::new(move |v| log.log_latency("", "server", v))).unwrap_or_else(|e| {
        eprintln!("mc-rs-server: could not start the game thread: {e}");
        std::process::exit(1);
    });
    hub.set_game(game_tx.clone());
    tokio::spawn(hub.clone().accept_sessions(sessions));
    let saver = hub.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            saver.world.lock().unwrap().save();
        }
    });
    // World backups: on `/backup` and every `backup_minutes` of play (backup.rs).
    let (backup_tx, backup_rx) = std::sync::mpsc::channel();
    hub.set_backups(backup_tx);
    let backups = hub.clone();
    let _ = std::thread::Builder::new().name("backup".into()).spawn(move || backup::run(backups, backup_rx));
    // The shared clock to every viewer once a second (commands.rs).
    let clock = hub.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            commands::sync_time(&clock, false);
        }
    });
    // The server window's console: the host's commands.
    let console = hub.clone();
    let _ = std::thread::Builder::new().name("console".into()).spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            commands::console(&console, &line);
        }
    });
    println!("  open http://127.0.0.1:{} on this PC", cfg.port);
    let _tunnel = if cfg.tunnel { tunnel::start(&cfg, &dir).await } else { None };
    let app = web::router(hub.clone());
    // Every socket sends at once (no Nagle wait for the last ack): video, pongs and the page's
    // timing replies are small or end in a small piece, and each one is a frame of delay.
    let listener = axum::serve::ListenerExt::tap_io(listener, |tcp: &mut tokio::net::TcpStream| {
        let _ = tcp.set_nodelay(true);
    });
    let server = axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>());
    tokio::select! {
        r = server => if let Err(e) = r { eprintln!("mc-rs-server: {e}"); },
        _ = tokio::signal::ctrl_c() => println!("stopping"),
    }
    let _ = game_tx.send(game::GameMsg::Shutdown);
    let _ = game_thread.join();
    hub.world.lock().unwrap().save();
}
