# Building mc-rs

Two ways: let an AI coding agent do it, or follow the steps by hand. Either way you need to own
Minecraft: Java Edition, because mc-rs reads its textures, models and sounds from your installed
copy of the 1.12.2 client.

## Before you start: the 1.12.2 client

1. Open the official Minecraft Launcher and sign in.
2. Installations, New installation, version **release 1.12.2**, Create.
3. Play that installation once, until the title screen, then quit.

mc-rs finds the client in `%APPDATA%\.minecraft\versions\1.12.2\1.12.2.jar`. Prism Launcher's
1.12.2 instances work too. For a copy anywhere else, set the environment variable
`MCRS_CLIENT_JAR` to the jar's full path.

## Option 1: with an AI coding agent

Any coding agent that can run terminal commands works: Google Antigravity, OpenAI Codex, Claude
Code, Cursor, Windsurf, GitHub Copilot agent mode and others.

1. Install [Git](https://git-scm.com/downloads), then clone the repository:
   `git clone https://github.com/haunterdev/mc-rs`
2. Open the `mc-rs` folder in your agent (in Antigravity or Cursor: File, Open Folder; in Codex
   or Claude Code: start it from inside the folder).
3. Paste this prompt:

> Build and run this project on my Windows PC. Read README.md, CLAUDE.md and
> docs/guides/building.md first. Install anything missing: Rust through rustup (with the Visual
> Studio C++ Build Tools it asks for), and Python 3.14 only if packaging is needed. Then run
> `cargo build --release -p viewer` and start `target\release\viewer.exe`. If the build fails,
> read the error, fix the cause (missing tools, PATH, memory), and try again. Tell me each step
> you take. Do not change the source code unless I ask.

To host a world for friends as well, add: "Also build the server with `cargo build --release
-p viewer -p server`, install ffmpeg and cloudflared with winget, and follow
docs/guides/hosting.md."

## Option 2: by hand

### 1. Install the tools

| Tool | Get it | Needed for |
|---|---|---|
| Git | [git-scm.com](https://git-scm.com/downloads) | downloading the source |
| Rust | [rustup.rs](https://rustup.rs): run `rustup-init.exe` and accept the defaults. When it asks for the Visual Studio C++ Build Tools, let it install them. | everything |
| Python 3.14+ | [python.org](https://www.python.org/downloads/) | `tools/dist.py`, the single shareable exe |
| ffmpeg | `winget install Gyan.FFmpeg` | hosting the browser server |
| cloudflared | `winget install Cloudflare.cloudflared` | the server's public link |

Open a new terminal after installing so the tools are on PATH. Check with `cargo --version`.

### 2. Get the source

```
git clone https://github.com/haunterdev/mc-rs
cd mc-rs
```

### 3. Build and play

```
cargo run --release -p viewer
```

The first build downloads the dependencies and compiles for 10 to 30 minutes; later builds take
seconds to a few minutes. The game opens in a window on a random seed. Pick one with
`cargo run --release -p viewer -- --seed 12345` (any text works as a seed, as in the game).
The built game is `target\release\viewer.exe`.

### 4. Host a world for friends (optional)

```
cargo build --release -p viewer -p server
```

Then double-click `Start Server.bat`. The window prints a link to send to friends.
[hosting.md](hosting.md) covers accounts, settings and performance.

### 5. Build the launcher exe (optional)

```
python tools/dist.py
```

This writes `dist\mc-rs-launcher.exe`, one file that installs and starts the game on another PC. It
builds with whole-program optimization, which takes 10 to 40 minutes.

## Build speed

Release builds are incremental (`Cargo.toml`) and link with `rust-lld.exe`, which every rustup
toolchain carries (`.cargo/config.toml`). Measured on a one-line viewer change with
`cargo build --release -p viewer`:

| Setup | Rebuild |
|---|---|
| link.exe, not incremental (before) | 83 to 90 s |
| link.exe, incremental | 17 to 41 s |
| rust-lld, incremental (now) | 13 to 16 s |

The first build after a change to either setting rebuilds every dependency once (11 minutes
here). The `dist` profile stays non-incremental, since it optimizes the whole program at once.

## Run the tests

```
cargo test --release -p viewer
cargo test -p server -p proto -p launcher
```

`cargo test -p worldgen` compares generated chunks with fixtures made from the original game. It
needs those fixtures, which `tools/setup_src.py` and `tools/fixtures.py` create from a local
decompile with JDK 8 (found under the usual install folders, or set `MCRS_JDK8` to its folder);
CLAUDE.md has the details.

## Troubleshooting

| Problem | Fix |
|---|---|
| "no Minecraft 1.12.2 client jar found" | Play release 1.12.2 once in the Minecraft Launcher, or set `MCRS_CLIENT_JAR` to the jar's path. |
| `link.exe not found` or MSVC errors | Install the Visual Studio C++ Build Tools ("Desktop development with C++"), then open a new terminal. |
| The build runs out of memory | Build with fewer jobs: `cargo build --release -p viewer -j 2`. |
| The window opens black or crashes at start | Update the graphics driver. |
| No sound | Check that Windows has an output device selected; the game plays through the default one. |
