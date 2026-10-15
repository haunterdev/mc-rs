# Hosting the browser server

`mc-rs-server` runs one shared creative world on this PC. Friends open a link in their browser,
sign in with a name and password, and play: each player gets a game session on this PC whose
picture is encoded on the GPU and streamed to the page, and their keys and mouse go back to it.
Their browser needs no download and no Minecraft files. A friend on a slow line can instead join
from the launcher, which draws the game on their own PC ([below](#joining-from-the-launcher)).

## Requirements on the host

- A release build: `cargo build --release -p viewer -p server` (both exes land in
  `target\release`, where the server finds the viewer).
- The Minecraft 1.12.2 client jar (Prism Launcher or the vanilla launcher), for models and
  textures, and the 1.12 assets (`assets\indexes\1.12.json`), for sound. The viewer finds both
  in Prism's and the vanilla launcher's folders.
- `ffmpeg` on PATH. The default encoder is `libx264` on the CPU (about half a core per player
  at 1280x720, 60 fps): it hands each frame back in 2 to 3 ms. The GPU encoders (`h264_amf`,
  `h264_nvenc`, `h264_qsv`) cost no CPU, but ffmpeg holds each frame in them until the next
  one comes in, a whole frame of extra delay (18 ms at 60 fps for `h264_amf` here). Set it as
  `encoder` in `server.json`; `python tools/encoder_latency.py` times an encoder on its own.
- `cloudflared` for the public link (installed under Program Files here).

## Start

Double-click `Start Server.bat` in the repo, or run `target\release\mc-rs-server.exe`.
`Start Server.bat` builds whatever changed since the last start (when cargo is on PATH), and when
a server is already running it asks whether to stop it and start the current build. A second
server on the same port stops at once and says so. The window prints the public link (`https://...trycloudflare.com`) and writes it to `link.txt` in
the data folder. The link changes every time the server starts, unless the server runs a named
tunnel ([below](#a-fixed-address)). On this PC the page is also at `http://127.0.0.1:25580`.

### A fixed address

With a domain of your own on Cloudflare, the server can answer at the same address every time
through a named tunnel. Set it up once:

1. Add the domain to a Cloudflare account and set its nameservers at the registrar to the two
   Cloudflare gives.
2. `cloudflared tunnel login` (pick the domain in the browser), `cloudflared tunnel create
   mc-rs`, `cloudflared tunnel route dns mc-rs play.example.com`.
3. Name both in `server.json`:

```json
"tunnel_name": "mc-rs",
"hostname": "play.example.com"
```

The server writes `cloudflared.yml` in the data folder on every start (the hostname to the web
port, anything else 404) and runs `cloudflared tunnel --config cloudflared.yml run mc-rs`, which
reads the tunnel's credentials from `%USERPROFILE%\.cloudflared`. It prints the link once the
first tunnel connection registers. Friends type `play.example.com` in the launcher's Join tab,
or open it in the browser.

Another local web server can share the hostname under a path. `"routes": {"/rooster": 28015}`
in `server.json` puts a rule ahead of the web port's that sends `https://HOST/rooster` and
everything under it to `http://127.0.0.1:28015`, path unchanged, so that server has to answer
under `/rooster` itself (Rustlite does). A route reaches `cloudflared.yml` on the next start.

On Cloudflare's free plan the domain's traffic is served from whichever colo has room, often
far from home (measured 2026-10-14: about 100 ms ping against 48 ms for a quick tunnel from the
same PC). `python tools/fast_link.py` starts a quick tunnel next to the running server and points
the domain's front page at it with a 302, so browser players get the near colo while the
address stays fixed. The launcher's Join on the domain keeps the named tunnel. The redirect needs
an API token with Zone, Single Redirect, Edit, saved as `cloudflare-token.txt` in the data
folder; run the script again after a reboot, since the quick tunnel's link changes.

Close the window or press Ctrl+C to stop; the world saves every 10 seconds and on exit.
cloudflared and the viewers close with the server, the window's close button included.

## Players

The first sign-in with a new name creates the account (names are 3 to 16 letters, digits or
underscores; passwords at least 4 characters). The browser stays signed in for 30 days.

- `"open_registration": false` in `server.json` stops new names; `mc-rs-server.exe
  --set-password NAME PASSWORD` then creates accounts by hand, and resets a forgotten password.
- `"join_password": "..."` makes new names give a server password once (guests too).
- Play as guest (the page) joins with no account, under the name typed or `GuestNNNN` when the
  field is empty; the launcher's As guest joins with no name, so it is always a `GuestNNNN`.
  A guest cannot take a name that has an account or that another guest holds, is never an
  operator, and the token lasts a day. `"guests": false` turns it off; so does
  `open_registration: false`.
- `"max_players"` caps the sessions (default 4). Each session renders on this PC's GPU and
  generates terrain with `"workers"` threads (default 3).

In the page: click into the picture to play; Esc frees the mouse and opens the options; T or
Enter opens chat; Tab shows who is online; F3 shows the stats, F5 changes the view, F1 hides the
GUI; Full screen also keeps Ctrl+W and Esc for the game.

The picture is automatic. While the player signs in, the page measures the screen (size and
refresh rate), the line to this PC (ping and speed) and the browser's H.264 decoder (a short
clip it encodes and decodes itself), then picks the picture size, frame rate and bitrate, and
the card says what it picked and the delay to expect. Play waits for it, a few seconds on the
first visit; the result is kept for a day. In game, two 5-second stretches in a row where the
device falls behind (frames short or skipped, slow decoding) bring up an offer of lighter
settings, which Auto also keeps for the next visit. Advanced (on the card, and under Settings in
game, where Apply rejoins) has the measurements, Measure again, and the three dropdowns; choosing
one by hand turns Auto off, and the line then says what Auto would pick. Settings also has the
volume. The corner shows the delay from input to picture, the ping, frames and bitrate.

On a phone or tablet the page shows touch controls instead (turn the phone sideways): drag on
the left half to walk (push to the edge to sprint), drag on the right half to look, tap there to
place and press and hold to break; Jump (tap twice to fly), Sneak (stays on until tapped again),
View (F5), Chat and Menu (Esc) are buttons, and tapping a hotbar slot selects it. In the menu a
tap is the pointer. Play asks for full screen where the browser allows it. Settings in game has
the touch controls (Auto, On, Off), the look speed and the button size.

The browser remembers the sign-in (never the password), the last name and every choice above
for the next visit. The saved sign-in goes only when the server answers and refuses the token;
a server that is down or restarting, the usual state just after a relaunch, leaves it saved.
The page keeps them per link, so a quick tunnel's new link each start begins empty: players
sign in and choose again. `http://127.0.0.1:25580` and any fixed address keep them. The game's
own Esc options are not kept between sessions yet.

## Joining from the launcher

A friend whose line is too slow for the video can join the same world from the launcher
(`mc-rs-launcher.exe`, the download on the update channel) instead: the game is drawn on their own PC,
and only the world's changes and the players cross the network, a few kilobytes a second instead
of the picture. Their PC needs to run the launcher's single player (it fetches the Minecraft
1.12.2 client jar from Mojang when the PC has none).

1. Start `mc-rs-launcher.exe` and pick Join a server.
2. Server address: the public link (`https://...trycloudflare.com`, as in `link.txt`) as it is.
   On the local network, this PC's address (`192.168.1.20`, port 25580 is assumed; the server
   needs `"bind": "0.0.0.0"`), or `host:port` for any other port. An `mcrs://join/<host>` link
   on a web page opens the launcher with the address filled in.
3. Name and Password: the account from the website (a new name signs up, as on the page; with
   `join_password` set, new names sign up on the page once first).
4. Join. A wrong password, a full server or a lost connection shows in the launcher's window.

In game, T opens chat (`/` opens it with the slash for a command) and Enter sends it; the rest
plays as single player. Native players count toward `max_players`, see the browser players and
are seen by them, and their edits land in the same world. The server checks a native player's
edits: inside the world, within 48 blocks of where it last stood, and not in adventure or
spectator mode. The launcher remembers the last address, name and sign-in token, never the password. A saved
token is checked with `/api/me` on start, so a returning player sees Signed in as NAME and joins
with one click; Sign out ends the sign-in and clears the token, and a token the server refuses
brings the name and password fields back. As guest joins with no name. A guest's token is signed
out when the game closes; an account's token stays, as the browser's does.

What crosses the network: to the player the seed, where it was, the saved edits, the other
players' names and moves, their edits, chat and command answers, the clock, teleports and the
game mode; from the player its own moves, its edits and its chat lines. The server's `/native`
socket is on the web port, so the tunnel carries it with the page.

## Commands

Players type commands into chat with a leading slash; the server window takes the same lines
(with or without the slash) and runs them with every permission. They follow Minecraft 1.12.2,
with its messages; the answer shows only in the chat of whoever typed it (an error in red),
except where noted.

From another shell on the host, `python tools/server_console.py say "Restarting in 30 seconds"`
types a line into the server window, `read [lines]` prints the end of it, and `stop` sends Ctrl+C,
which saves the world and exits. A restart for an update is `say`, a wait, `stop`, and starting the
new build.

| Command | Does |
|---|---|
| `/spawn [player]` | back to the world spawn, on the top block there (not in vanilla) |
| `/setworldspawn [x y z]` | the world spawn: new players and `/kill` land there |
| `/spawnpoint [player] [x y z]` | where `/kill` puts that player instead |
| `/tp [player] x y z [yaw pitch]`, `/tp [player] other` | teleport; `~` is relative (`/tp ~ ~10 ~`) |
| `/teleport player x y z [yaw pitch]` | teleport, `~` relative to whoever typed it |
| `/time set day\|night\|noon\|midnight\|N`, `/time add N`, `/time query daytime\|gametime\|day` | the shared time of day |
| `/gamerule doDaylightCycle true\|false` | stops or restarts the day; `/gamerule` alone lists the rules |
| `/gamemode survival\|creative\|adventure\|spectator\|0-3 [player]` | kept per player; only creative and spectator may fly, survival play itself is not built yet |
| `/weather clear\|rain\|thunder [seconds]` | kept, not drawn yet |
| `/kill [player]` | a respawn at the spawn point or the world spawn, announced to everyone |
| `/say message`, `/me action` | to everyone |
| `/tell`, `/msg`, `/w player message` | a whisper |
| `/list`, `/seed`, `/help [page\|command]` | who is online, the seed, the commands you may use |
| `/op player`, `/deop player` | the operator list (`"ops"` in `server.json`) |
| `/backup`, `/backup list` | operators: saves the world and copies it into `backups/` now; names the newest backups |
| `/rollback NAME`, `/rollback cancel`, `/rollback list` | operators: the world goes back to that backup at the next server start |

Players can use `@p` (nearest), `@a` (everyone), `@r` (random) and `@s` (yourself) for a player.

Who may run them: with `"commands": "everyone"` (the default) every signed-in player may run
every command but `/op` and `/deop`. With `"commands": "ops"` the others may only `/spawn`
themselves, `/help`, `/list`, `/me` and `/tell`. Operators, listed in `"ops"`, may run everything.
The first account to play on a server with no operators (the host's) becomes one; `/op NAME` in
the server window or `mc-rs-server.exe --op NAME` adds another.

The server keeps the time of day: it runs 20 ticks a second (stopped by `doDaylightCycle
false`), every player's sky follows it, and it is saved with the world.

## Finding where delay comes from

Each session appends to `latency.log` in the data folder, one JSON line every 5 seconds from
the page (input to frame, input to shown, ping, decode time, the host's render-to-encoded time,
frame gaps, skipped frames, frames the server dropped for a slow connection) and from the game
session (frame time, grab time, frames the encoder was too busy for, render to encoded unit).
The server adds its own line every 5 seconds (`"event": "tick"`): the game tick's time at p50,
p99 and the most, ticks dropped for falling behind, and the players and entities it ticks.
Input to shown minus the host pipeline minus decode is the network both ways. On this PC the
host pipeline is about 14 ms at 1280x720, 60 fps and 10.5 ms at 960x540 (render about 9,
grab 2, encode 3 to 4); input to frame on the PC itself is 18 to 19 ms.

## Files

All in `%APPDATA%\mc-rs\server` (`--data DIR` moves them):

| File | Holds |
|---|---|
| `server.json` | settings, written with the defaults on the first start (the seed is random) |
| `accounts.json`, `tokens.json` | names with Argon2 password hashes; sign-in tokens |
| `edits.bin` | every block a player changed (the terrain itself comes from the seed) |
| `players.json` | where each player was, whether they were flying, their game mode and `/spawnpoint` |
| `world.json` | the time of day and the world's age, the world spawn, the game rules, the weather |
| `link.txt` | the current public link |
| `tunnel.log` | cloudflared's own output, for a link that stops working or a join that drops |
| `latency.log` | the timing reports above |
| `backups/` | world backups, one folder each, named by the time they were made (see below) |

## Backups and rollback

Every `backup_minutes` while someone has played since the last one, and whenever an operator
types `/backup` (or `backup` in the server window), the server saves the world and copies it to
`backups/<YYYYMMDD-HHMMSS>`: the block changes, players, world file, signs and the other game
files, paintings and their pictures, the accounts, bans and `server.json`. Only the newest
`backup_keep` stay.

To roll back, type `/rollback NAME` with a name from `/rollback list` and restart the server, or
start it with `mc-rs-server.exe --rollback NAME`. Before the world loads, the server backs up the
world as it is (`<time>-before-rollback`, so a rollback can itself be undone) and puts the
backup's world files back. Accounts, sign-in tokens, bans and `server.json` stay as they are now;
the backup folder still holds its copies of them for a manual restore.

## Settings (`server.json`)

| Key | Default | Meaning |
|---|---|---|
| `bind`, `port` | `127.0.0.1`, `25580` | where the page listens; `0.0.0.0` also serves the local network |
| `seed` | random | the world seed |
| `width`, `height`, `fps` | 1920, 1080, 120 | the largest picture and frame rate a player can pick, and Auto's ceiling |
| `bitrate_kbps` | 6000 | the bitrate "Auto" gives 1280x720 at 60 fps, scaled by pixels and frame rate |
| `max_bitrate_kbps` | 30000 | the most a player can pick |
| `encoder` | `libx264` | the ffmpeg encoder |
| `render_distance` | 8 | chunks around each player |
| `tunnel`, `cloudflared` | true, found | the Cloudflare tunnel and where cloudflared is |
| `tunnel_name`, `hostname` | empty | a named tunnel and the hostname it serves, instead of a quick tunnel (see [A fixed address](#a-fixed-address)) |
| `routes` | empty | path prefixes on the named tunnel's hostname sent to other local ports, `{"/rooster": 28015}` |
| `viewer` | found | the viewer exe, when it is not next to the server |
| `guests` | true | Play as guest without an account |
| `commands` | `everyone` | who may run the commands that change the world or other players: `everyone` or `ops` |
| `ops` | the first account | operators, who may run every command (see Commands) |
| `backup_minutes` | 30 | minutes of play between two timed backups; 0 turns them off |
| `backup_keep` | 24 | the newest backups kept; 0 keeps every one |
| `difficulty` | `normal` | `peaceful`, `easy`, `normal` or `hard`: the starvation floor, mob damage to players; peaceful stops hostile spawning |
| `pvp` | true | players may hurt each other |
| `sim_distance` | 8 | chunks around each player the server keeps loaded and ticking |
| `max_entities` | 300 | the most items and mobs at once, on top of the mob caps |
| `jar` | found | the 1.12.2 client jar the server reads recipes and loot tables from |
| `default_game_mode` | `survival` | the game mode of a player new to the server |

The last six belong to survival play and take effect as its parts land;
until then every player keeps the creative default.
