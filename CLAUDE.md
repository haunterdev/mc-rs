
## Rust workspace

Cargo workspace, edition 2021. `Cargo.toml` at the root lists members as
`crates/*`; one directory per crate, one file per system inside a crate, and a file past
about 600 lines gets split.

Check a change with `cargo test --workspace`. Release builds keep line tables for
profiling. A crate that opens a window is checked by hand; test its logic headless.

## mc-rs

Bit-exact port of 1.12.2 overworld terrain (default world type). Hard rules: never commit
Mojang code, the decompile (`../mc-1.12.2-src/`), `fixtures/` or `harness/build/`; port 1:1
with no algorithm changes in `crates/worldgen`; every ported function names its source class
and method (MCP names); every pitfall goes in `docs/DECOMP_NOTES.md`.

| File | Owns |
|---|---|
| `crates/worldgen/src/java_random.rs` | `java.util.Random` |
| `crates/worldgen/src/math_helper.rs`, `fdlibm.rs` | `MathHelper`, `Math.round`, `StrictMath.log` |
| `crates/worldgen/src/noise/` | octaves, improved, Perlin and simplex noise |
| `crates/worldgen/src/genlayer.rs`, `biome_provider.rs` | the GenLayer stack and `BiomeProvider` |
| `crates/worldgen/src/biome.rs`, `blocks.rs`, `primer.rs` | biome table, block state ids, `ChunkPrimer` |
| `crates/worldgen/src/terrain.rs` | `generateHeightmap` and `setBlocksInChunk` |
| `crates/worldgen/src/surface.rs` | `replaceBiomeBlocks` and every `genTerrainBlocks` override |
| `crates/worldgen/src/caves.rs`, `ravines.rs` | `MapGenBase`, `MapGenCaves`, `MapGenRavine` |
| `crates/worldgen/src/generator.rs` | `generateChunk` order, `ChunkGenerator`, staged output for tests |
| `crates/worldgen/src/populate/mod.rs` | `ChunkGeneratorOverworld.populate` order (lakes, dungeons, decorate, spawn draws, ice and snow) |
| `crates/worldgen/src/populate/world.rs` | `PopWorld`: the `World`/`Chunk` paths population touches (set with flags, heightmap, neighbour updates, immediate ticks) |
| `crates/worldgen/src/populate/state.rs`, `props.rs` | block state facts (material, opacity, faces) generated from Java by `tools/gen_props.py` |
| `crates/worldgen/src/populate/behavior.rs`, `liquid.rs` | per-block `neighborChanged` (signs drop when their support goes), `onBlockAdded`, `breakBlock`, drops, `canPlaceBlockAt` (a sign next to cactus refuses); `BlockLiquid`, `BlockDynamicLiquid` |
| `crates/worldgen/src/populate/decorator.rs`, `biomes.rs` | `BiomeDecorator` and every `Biome.decorate`/`getRandomTreeFeature` override |
| `crates/worldgen/src/populate/features.rs`, `features2.rs` | `WorldGen*` plants, ores, disks; huge mushrooms, springs, lakes, dungeons, ice spikes, wells, fossils |
| `crates/worldgen/src/populate/trees.rs`, `trees2.rs`, `trees3.rs` | every tree generator (oak, jungle, birch, taiga, swamp, shrub; big, canopy, savanna; mega jungle and pine) |
| `crates/worldgen/src/populate/spawn.rs` | `performWorldGenSpawning`: the `World.rand` draws only, no entities |
| `crates/worldgen/src/populate/packed.rs` | a populated chunk packed away out of range (the save/load stand-in), `PopChunk::to_chunk` |
| `crates/worldgen/tests/` | fixture comparisons: `primitives`, `genlayer`, `stages`, `populate` |
| `crates/worldgen/examples/bench.rs` | chunks/sec, mirrors the Java `Bench` |
| `tools/bench.py` | runs both benches, writes `docs/BENCHMARK.md` |
| `crates/viewer/src/stream.rs` | worker pool, chunk slots, mesh upload and unload |
| `crates/viewer/src/populator.rs` | the population thread: wraps a `sim::terrain::Terrain`, keeps the channels, the requested and wanted sets, answers each wanted chunk once final; re-exports `Edit` |
| `crates/jar/` | reads the local 1.12.2 client jar; nothing it reads touches disk |
| `crates/viewer/src/jar.rs` | shim: `pub use jar::*;` |
| `crates/sim/` | the simulation the viewer and the server share (every module declared in `lib.rs`): `collision.rs` (`AxisAlignedBB`, block collision boxes, `World.getCollisionBoxes`, liquid checks), `region.rs` (the 3x3 chunk neighborhood a mesh reads, block properties, `BlockLiquid.getLiquidHeightPercent`, `Block.getOffset`, `MathHelper.getCoordinateRandom`), `raytrace.rs` (`BlockPos`, `RayTraceResult`, block bounding and selection boxes, `AxisAlignedBB.calculateIntercept`, `World.rayTraceBlocks`, `REACH`), `entity.rs` (`Body`: `Entity.setPosition`, `Entity.move` with step and sneak edge, `moveRelative`, `handleWaterMovement`, `isInLava`, `isOffsetPositionInLiquid`, `pushOutOfBlocks`, `resetPositionToBB`, step and splash sounds; `travel_living` is `EntityLivingBase.travel`, calling back through the `Living` trait for the entity's own `move`), `terrain.rs` (`Terrain`: owns the `PopWorld`, the packed store, the populated set and the edit overlay; `Chunk.populate` scheduling gated by the caller's wanted predicate (`needed`, `ensure`, `insert_generated`), `is_final`, `pack_beyond`, `edit` and `remote` (the viewer's edits and other worlds' blocks), `set` (a server-made change with neighbour updates), `final_chunk`; its `BlockAccess` reads air unless the chunk is final and held), `kinds.rs` (the entity kinds survival knows: `EntityList` id and name, `setSize`, eye height, texture, `EntityTracker.track` range, interval and velocity; `by_id`, `by_name`), `lang.rs` (`Lang`: `Locale.loadLocaleData` with the `%d`/`%f` to `%s` rewrite, `translate`, `format` as `I18n.format` over the `String.format` subset lang values use), `inventory.rs` (`InventoryPlayer` over `items::PlayerInventory` by `Deref`: the pickup bob counter per slot (`decrementAnimations`, set by `add`), `getSlotFor`, `setPickedItemStack`, `pickItem`, `deleteStack`, `dropAllItems` returning the stacks, `to_save`/`from_save` with `writeToNBT` slot numbers 0..35, 100..103, -106), `container/` (`mod.rs`: `Container.slotClick` for every `ClickType`, `Slot`, `mergeItemStack`, the drag helpers, `detectAndSendChanges`, `onContainerClosed`, with drops returned through `PlayerRef::dropped`; `player.rs`: `ContainerPlayer`, `InventoryCrafting`/`InventoryCraftResult`/`SlotCrafting` crafting through `items::Recipes`, `clearContainer`, `getSlotForItemStack`; `workbench.rs`: `ContainerWorkbench`; tests in `tests.rs` and `tests_craft.rs`, the seeded click fuzz included), `testing.rs` (fixtures behind the `testing` feature) |
| `crates/viewer/src/dialog.rs` | the startup-error message box when the viewer runs alone |
| `crates/paths/` | where files live on a player's PC (`%APPDATA%\mc-rs`, `%LOCALAPPDATA%\Programs\mc-rs`), client jar candidates, Mojang's jar URL and SHA-1 |
| `crates/brand/` | the icon, drawn in code; `brand::embed` links icon, version info and manifest into both exes from their `build.rs`; `examples/icon.rs` writes it as a `.ico` for the website |
| `tools/dist.py` | the shareable `dist/mc-rs-launcher.exe` and `dist/manifest.json`: profile `dist`, feature `share`, static CRT, 20 MB limit |
| `tools/server_console.py` | types a command into the running server's console window, reads its text, or stops it with Ctrl+C (saves first); the restart-for-an-update steps |
| `crates/launcher/src/ui.rs` | the launcher window (egui, glow): the Single player and Join a server tabs, seed, Play, install and jar steps, progress, errors with the log tail, update banner, the refused-link banner from `mcrs://` |
| `crates/launcher/src/join.rs` | Join a server: the address (website link, `host:port` or `mcrs://join`) to `https`/`wss` URLs, the sign-in at `/api/login` or As guest at `/api/guest`, the viewer in `--join` mode with the token in `MCRS_JOIN_TOKEN`, the reason it left from its log, the last address and name in `%APPDATA%\mc-rs\launcher.json` (never the password) |
| `crates/launcher/src/payload.rs` | the viewer carried zstd-packed, installed to `%LOCALAPPDATA%\Programs\mc-rs\mc-rs-viewer.exe` with an id stamp |
| `crates/launcher/src/uri.rs` | `mcrs://play` and `mcrs://join/<host>[:<port>]` links from the website (a join link opens Join a server filled in): the strict parser and the per-user `HKCU\Software\Classes\mcrs` registration (only builds that carry the viewer register) |
| `crates/launcher/src/net.rs`, `update.rs` | in-process HTTPS (ureq, rustls), hash-checked downloads; the update manifest and the self-swap |
| `crates/launcher/src/game.rs` | starts the viewer (a seed or `--join`) with `MCRS_LAUNCHED`, keeps `%APPDATA%\mc-rs\logs\latest.log` |
| `crates/viewer/src/textures.rs` | PNG decode, `TextureUtil` mipmaps, the block sprite array, flat images |
| `crates/viewer/src/models.rs` | blockstates, block models, `FaceBakery` quads, element rotation, variants |
| `crates/viewer/src/blockstates.rs` | the state mappers: block state to blockstate file and variant key |
| `crates/viewer/src/region.rs`, `colors.rs` | shim: `pub use sim::region::*;`; biome tints |
| `crates/viewer/src/mesher.rs`, `fluid.rs` | `BlockModelRenderer` (smooth and flat light), `BlockFluidRenderer` |
| `crates/viewer/src/vertex.rs`, `shaders/chunk_vertex.wgsl` | the packed 20-byte chunk vertex, the chunk mesh and its bounds, the vertex shader for every chunk pass |
| `crates/viewer/src/lod.rs`, `lod_mesh.rs`, `shaders/lod.wgsl`, `lod_vertex.wgsl` | distant terrain: chunk reduction to cells, section meshes by distance (tops merged per chunk, a 20-byte vertex carrying color and chunk), the on-screen chunk mask (1024 chunks a side, 512 chunks out) the vertex shader drops quads by, scheduling on the worker pool |
| `crates/viewer/src/lightmap.rs`, `celestial.rs` | `updateLightmap`, celestial angle, sky, fog and cloud colors |
| `crates/viewer/src/env.rs` | the per-frame texture every shader reads (lightmap, fog, time, hand light) |
| `crates/viewer/src/material.rs`, `shaders/` | chunk, sky, cloud, hand, item and distant-terrain materials and their WGSL |
| `crates/viewer/src/sky.rs`, `clouds.rs` | tick clock (following the server's in a shared world), view state and the camera (`orientCamera`, third person), `renderSky`, `renderCloudsFancy` (the cloud field widens with the render distance) |
| `crates/viewer/src/hand.rs` | first-person arm, swing and equip state (the shown `ItemStack`), crosshair and attack indicator |
| `crates/viewer/src/settings.rs` | the Esc options menu (`GuiButton` faces from `widgets.png`, `drawDefaultBackground`, a whole-number scale that fits the window, `menu_scale`) and `%APPDATA%\mc-rs\options.txt` |
| `crates/viewer/src/collision.rs` | shim: `pub use sim::collision::*;` (`impl BlockAccess for Streamer` is in `stream.rs`) |
| `crates/viewer/src/player.rs`, `player/auto_jump.rs` | the `EntityPlayerSP` over a `sim::entity::Body` (`Deref` to it): keys, `MovementInput`, sprint and fly toggles, `EntityPlayer.travel`, jump, sneaking size, camera angles and bobbing, `run_ticks`; `updateAutoJump`; the guard `player::tests::golden_traces` in `player/tests.rs` |
| `crates/viewer/src/teleport.rs` | what server commands do to the player: `handlePlayerPosLook` teleports, respawns on `getTopSolidOrLiquidBlock`, the game mode's `allowFlying` |
| `crates/viewer/src/input.rs` | mouse capture (`setIngameFocus`), mouse turn, queued attack/use/pick presses, wheel notches, F5 (`thirdPersonView`) |
| `crates/viewer/src/target.rs` | `pub use sim::raytrace::*;` plus what stayed: `getMouseOver` for blocks, the block outline mesh and its systems |
| `crates/viewer/src/interact.rs` | creative break, place and pick block: `clickMouse`, `rightClickMouse`, `middleClickMouse`, `PlayerControllerMP`, `ItemBlock`/`ItemSlab`/`ItemSnow`/`ItemLilyPad`, `canPlaceBlockAt` per block, `BlockTorch.getStateForPlacement`, `ItemSign.onItemUse` (sets `Controller::sign` for the edit screen); tests in `interact/tests.rs` |
| `crates/viewer/src/sign.rs` | signs: the lines per block (`S_SIGN` in, `V_SIGN` out when the screen closes), `TileEntitySignRenderer` (`ModelSign` board and stick from `entity/sign.png`, the lines in the jar's font, 64-block range, block light), `GuiEditSign` (typing, Up/Down/Enter lines, the blinking `> <` marker, Done or Esc to close; a streamed session's keys typed on a US layout) |
| `crates/viewer/src/hotbar.rs` | `Inventory`: `sim::inventory::InventoryPlayer` (`items::ItemStack` slots, `setPickedItemStack`) plus queued wheel notches and number keys; `block_stack`/`placed_block`/`held_state` between block keys and stacks; the hotbar widgets |
| `crates/viewer/src/item.rs` | every item's model: `ItemModelMesher` lookup (`model_key`, `item_model_name`), loading and baking (block models and flat `item/generated` and `item/handheld` ones), display transforms, `ItemColors` with no NBT. `item/blocks.rs` and `item/item_table.rs` are the `RenderItem.registerItems` tables (block half; item half and the `ItemMeshDefinition`s), `item/generator.rs` is `ItemModelGenerator` |
| `crates/viewer/src/item_render.rs` | the hotbar icons (`renderItemIntoGUI`, its own camera over the UI) and the held item (`renderItemInFirstPerson`) for any stack; under the options menu the icons stay, dimmed to its backdrop; under the creative screen they move behind its backdrop |
| `crates/viewer/src/media.rs` | media in the world, like Immersive Paintings: a picture or video dropped onto the window (or hung from the page, `S_HANG`) becomes a painting on the wall grid, centered on the face under the crosshair and sized in whole blocks to the picture's shape (2 blocks on the long side); pointing at one, `=`/`-` change its width, `]`/`[` its height (1 to 16 blocks), Delete takes it down, a new drop swaps its picture; GIFs and videos play, lit by the block in front of it, a broken anchor block takes it down. In a session with a server the file goes up once (`V_MEDIA`, kept by content hash), each painting is shared (`V_PAINTING`/`S_PAINTING`) and every viewer fetches the file it lacks (`S_MEDIA`, cached in `media-cache/`); local single player keeps them for the session |
| `crates/viewer/src/media_load.rs` | decoding for media.rs on a thread of its own: PNG and GIF with the viewer's decoders, JPEG, WebP and BMP as one frame through Bevy's, video (MP4, WebM, MKV, MOV and the rest) through a looping `ffmpeg` child (`MCRS_FFMPEG` overrides the path) whose newest frame waits in `Video::latest` |
| `crates/viewer/src/capture.rs` | `--shot`: saves the smoke run's frame as PNG |
| `crates/viewer/src/link.rs` | a streamed session (`--stream`): the server connection, browser input as Bevy input messages, the menu's pointer, state and edits out, the cameras drawing into the read-back image, the server's clock, teleports and game mode in; `receive` and `send` also serve a native session over `Link::over`; `pace` holds the stream to its frame rate on a fixed grid and starts a frame up to half a frame early when browser input is waiting, `change_picture` restarts the encoder at a new size, rate and bitrate when the page asks (`S_PICTURE`) |
| `crates/viewer/src/join.rs`, `chat.rs` | a native session (`--join URL`, the launcher's Join): the WebSocket to the server's `/native` (tungstenite, rustls with ring and the webpki roots), the welcome's seed, the server's reason on disconnect (exit code 3), joining again with the same token after a dropped line (up to 5 tries); chat (T or `/` opens the line, Enter sends, lines fade as `GuiNewChat`) |
| `crates/viewer/src/survival/` | survival in a server session (`SurvivalPlugin`): `mod.rs` holds `SurvivalState` (health, food, saturation, air, burning, xp, dead, kept by `apply_state`) and one Bevy message per S_ survival frame; `net.rs` decodes the frames link.rs's `receive` leaves in `survival::Inbox` (tags 160 to 219, both session kinds) and has a send helper per V_ message; `V_CLIENT_INFO` goes once after the welcome (`Link::connect`, join.rs `open`). The game mode stays `Player::game_mode`. `gui/` is the container screens: `mod.rs` holds the `Screen` trait and `GuiContainer`'s click, drag, release, number-key, slot and cursor drawing as generic functions, plus `GuiLang` (`lang/en_us.lang`); `slots.rs` the GUI-pixel geometry (`ScaledResolution`, mouse mapping, tab and scrollbar layout, tested); `draw.rs` the `Frame` of z-ordered quad bands and item icons that `present` turns into meshes on the item camera, `ContainerImages`, the font and tooltip drawing; `creative.rs` `GuiContainerCreative` (E in creative mode, `--screen creative[:TAB]` for a shot; changed slots go out as `V_CREATIVE_SLOT`); `inventory.rs`, `crafting.rs`, `chest.rs` and `furnace.rs` are stubs. The other files are empty `plugin` stubs for the survival systems still to come: hud, death, hurt, items, mining, crack, chest_render, mobs, attack and falling |
| `crates/viewer/src/encode.rs` | the session's video: the ffmpeg child (H.264 constrained baseline, libx264 zerolatency by default, a two-frame rate buffer, one frame in flight) and the access unit splitter; each unit keeps its frame's input sequence and render-to-encoded time |
| `crates/viewer/src/grab.rs` | the frame grab in the render world right after the submit (copy, wait, hand to the encoder in the same frame), and the `FrameStamp` (input sequence, update start) it tags frames with; the read-back buffer follows a picture change and a frame of the old size is skipped |
| `crates/viewer/src/players.rs`, `shaders/entity.wgsl`, `shaders/nameplate.wgsl` | player models: `EntityOtherPlayerMP` interpolation, limb swing and body turn, `ModelPlayer` posed by `ModelBiped.setRotationAngles`, vanilla entity lighting; `drawNameplate` name tags in the jar's font (`FontRenderer` widths); the player's own model in third person |
| `crates/viewer/src/audio.rs` | sound events from the local 1.12 assets, lewton decoding, the mixer (distance, pan, pitch), the cpal output or the streamed PCM, `MusicTicker` |
| `crates/viewer/src/sfx.rs` | what plays: break and place sounds and debris, steps, swim, splash, sprint dust, music, the button click; `SoundType` names |
| `crates/viewer/src/particles.rs` | `ParticleDigging`: tick, block collision, camera-facing quads in the packed chunk vertex |
| `crates/proto/` | the server to viewer frames (`V_*`, `S_*`), `PlayerState`, block changes, the browser input encoding, Annex B splitting; a native client's messages (`split_frames`, `V_TEXT`, `S_TEXT`); media and paintings (`V_MEDIA`, `V_PAINTING`, `S_MEDIA`, `S_PAINTING`, `S_HANG`, `Painting`, `media_id`, `MAX_MEDIA`); a picture change mid-game (`S_PICTURE`); survival messages (`survival.rs`, V_ 60..99 and S_ 160..219, `SURVIVAL_PROTOCOL` 2; a sign's lines are `SignText`, `V_SIGN` and `S_SIGN`) |
| `crates/server/` | `mc-rs-server`: `config.rs` (`server.json`; the survival fields `difficulty`, `pvp`, `sim_distance`, `max_entities`, `jar`, `default_game_mode` are carried; `sim_distance` drives the game's terrain and tracker, the rest are not yet acted on), `game/` (the 20 Hz game thread: `mod.rs` holds `Game`, `GameMsg`, the absolute-deadline tick loop with catch-up and the 5 s tick line; `net.rs` `send`/`broadcast`/`send_near`/`new_entity_id`; one hook module per later task, `players`, `damage`, `playerdata`, `items`, `mining`, `windows`, `tiles`, `mobs/`, `persist`, `terrain`, `tracker`, each with `on_join`/`on_leave`/`on_frame`/`tick`/`save`/`load` and its own `State`, listed in `MODULES` and `TICK_ORDER`, so a module's behaviour goes in its own file; `terrain.rs` is the game's `sim::terrain::Terrain` for the world seed: two `terrain-gen` workers generate, the game thread (256 MB stack) populates in `plan` order within `sim_distance` of every player and packs past `sim_distance + 3`, the overlay starts from `World::all_edits`, `on_blocks` takes `V_BLOCKS` raw, `set_block` runs neighbour updates and writes edits.bin and `S_BLOCKS`, `state`, and `light` from `sim::region` cached per chunk; `tiles.rs` is the signs: a sign block placed makes an empty `TileEntitySign` that only its placer may fill (`processUpdateSign`, formatting codes stripped), every viewer gets `S_SIGN` on join and on each edit, saved in `tiles.json`; `persist.rs` loads each module's `<module>.json` at start and writes them every 900 ticks and at shutdown; `tracker.rs` is `EntityTracker`/`EntityTrackerEntry` over the `Tracked` trait items and mobs implement: spawn, batched `S_ENTITY_MOVE`, `S_VELOCITY`, `S_DESTROY` by the kinds' range and interval; `testing.rs` is the test `Harness`: a seeded game with terrain in a temp folder, fake sessions, `join`, `push_frame`, `tick`, `settle`, `frames_for`), `accounts.rs` (Argon2, tokens, guests: a day-long token and no account), `world.rs` (edits, players and their game modes, the clock, spawn, game rules, weather, paintings in `paintings.bin` and their files in `media/` by content hash), `hub.rs` (sessions and relays; joins, leaves, states, edits and the survival frames 60..99 to the game), `commands.rs`, `commands_exec.rs` (slash commands in chat and the server console, permissions, feedback, `/kick`, `/ban <name> [ip true|false]`, `/ban-ip`, `/pardon`, `/pardon-ip`, `/banlist`; guests may not chat, `/me`, `/say` or `/tell`; the clock to every viewer), `commands_backup.rs` (`/backup [list]`, `/rollback [list|cancel|NAME]`, ops only), `backup.rs` (world backups in `backups/<YYYYMMDD-HHMMSS>`: the backup thread takes `/backup` requests and runs every `backup_minutes` of play, `GameMsg::Save` then `World::save` then the copy of `edits.bin`, `paintings.bin`, every `.json` and `media/` (hard links), pruned to `backup_keep`; `rollback_at_start` runs `--rollback NAME` or the `/rollback` request in `backups/rollback.txt` after the port is bound and before the world loads, backing up the current world as `-before-rollback` first and keeping accounts, tokens, bans and `server.json`; tests in `backup_tests.rs`), `bans.rs` (`bans.json`, refused at the login, the guest sign-in and both sockets; the address behind the tunnel from `CF-Connecting-IP`), `web.rs` (routes and the socket; `/api/guest`, `/api/ping` and `/api/probe` for the page's tuner, `/api/media` for a file the player hangs from the page); `hub.rs` also takes the page's `{"t":"picture"}` and changes that player's stream in place, `job.rs` (a Windows job object that closes cloudflared and the viewers with the server, however it ends), `main.rs` (binds the port before anything starts, so a second copy says another server holds it), `native.rs` (the launcher's `/native` socket: a hub session with no viewer here, edits checked, survival frames 60..99 under 64 bytes passed to the game, a ping every 15 s and a close after 45 s of silence), `tunnel.rs` (cloudflared, its output in `tunnel.log`) |
| `crates/server/web/` | the browser client: `client.js` (sign-in and Play as guest, WebCodecs video, the PCM worklet and volume, pointer-lock input with input sequence numbers, chat and player list), `lag.js` (the delay readout and 5-second stats reports), the canvas double-buffered on phones (`?desync=` overrides), `touch.js` (phone controls: stick, look, tap and hold, buttons, hotbar taps, the menu pointer), `prefs.js` (what the page keeps in localStorage), `tune.js` (the settings tuner: screen size and refresh rate, ping and line speed from `/api/ping` and `/api/probe`, H.264 decode time on a clip it encodes itself; picks size, frame rate and bitrate, kept a day in `mcrs.tune`; the in-game watcher: a step down after two 5-second reports behind, a step up after a minute with room, kept in `mcrs.tune.cap`), `picture.js` (the Auto line, the Advanced section with the dropdowns on the card and in game, waiting for the tuner before a join, changing the running picture on Auto or Apply without a rejoin), and the "Hang a picture" button and drag-drop onto the game (`/api/media`) |
| `crates/items/src/registry.rs`, `items_table.rs`, `blocks_table.rs`, `ids.rs` | `Item.registerItems` and `Block.registerBlocks` as data: ids, names, translation keys, stack size, durability, subtypes, container items, `getItemFromBlock`/`getBlockFromItem` |
| `crates/items/src/stack.rs`, `inventory.rs` | `ItemStack` (item, count, damage; NBT dropped) and its comparisons; `InventoryPlayer` stacking (`storeItemStack`, `canMergeStacks`, `addResource`, `add`) |
| `crates/items/src/tools.rs`, `dig.rs` | `ToolMaterial`, `ArmorMaterial`, `ItemTool`/`ItemSword`/`ItemShears` destroy speed, harvest and wear; `Material.isToolNotRequired`, `getDigSpeed`, `getPlayerRelativeBlockHardness`, break ticks |
| `crates/items/src/drop_rules.rs`, `drops.rs`, `harvest.rs` | `getItemDropped`/`quantityDropped`/`damageDropped` per block, `dropBlockAsItemWithChance` on a `JavaRandom` (offsets, xp), `harvestBlock` and the survival half of `tryHarvestBlock` |
| `crates/items/src/crafting.rs`, `recipe_json.rs` | `CraftingManager`: the jar's JSON recipes parsed at runtime (`Recipes::from_jar`), shaped (mirrored) and shapeless matching, `RecipeRepairItem`, shulker box coloring, remaining items, `SlotCrafting.onTake` grid side |
| `crates/items/src/smelting.rs`, `food.rs`, `names.rs` | `FurnaceRecipes`, `getItemBurnTime`, the furnace tick; `ItemFood` values and effects; per-stack translation keys and `.lang` lookup |
| `crates/items/tests/` | registry, dig speed, seeded drops, crafting, smelting, food and names; the jar cases read `MCRS_JAR_DATA` (an extracted `assets/minecraft`) and skip without it |
| `crates/viewer/src/hud.rs`, `main.rs` | stats overlay, CLI, jar loading, cameras, AA, smoke mode |
| `harness/src/`, `tools/fixtures.py`, `tools/setup_src.py`, `tools/remap/` | Java fixture harness, decompile setup |

Fixtures: `python tools/setup_src.py` once, then `python tools/fixtures.py gen`. Tests:
`cargo test -p worldgen`. Viewer: `cargo run --release -p viewer -- --seed <s>`; headless check
`--smoke 20`; `--hide-gui` starts with the GUI hidden (F1), for website screenshots; `--menu`
opens the options menu on the first frame, so `--smoke 8 --shot FILE.png --menu` checks its layout. Java bench: `python tools/fixtures.py bench --side 32`; Rust bench:
`cargo run --release -p worldgen --example bench -- --side 32`; both plus `docs/BENCHMARK.md`:
`python tools/bench.py`. Shareable exe for a friend's PC: `python tools/dist.py`, which writes
`dist/mc-rs-launcher.exe` and `dist/manifest.json` (gitignored); the viewer builds in
profile `dist-viewer` (thin LTO): about 4 minutes after a viewer change, 10 from cold; `--launcher-only`
skips that step when only the launcher changed, `--no-guard` builds without the memory guard. The shared viewer logs a failing system once instead
of panicking (`log_error_once` in main.rs, `share` feature only). `mc-rs-launcher.exe` with viewer arguments (`--smoke 20`)
skips its window and runs the viewer with them.

Browser server: `Start Server.bat` (builds what changed when cargo is on PATH, and offers to stop a server that is already running), or `cargo build --release -p viewer -p server` then
`target\release\mc-rs-server.exe`; `docs/guides/hosting.md` covers settings, files and accounts.
The server is tested with a stand-in viewer (a script speaking the `proto` frames) and Python
web clients (`tools/server_e2e.py`), and with a native client next to a streamed player
(`tools/native_e2e.py`); the real session needs ffmpeg and a GPU, so it runs on a desktop PC, where
`tools/stream_probe.py` plays it like a browser and measures input to frame (mouse turns on their
own `--hz` clock; the summary splits the delay into host pipeline and rest; `--start DIR` runs that
build's server and viewer on a spare port with fresh data, so two builds compare A/B), and
`tools/picture_probe.py` (release build, headless viewer) uploads a picture through `/api/media`,
changes the picture size and rate mid-game, and checks a second player's viewer fetches the painting.
`tools/join_check.py` starts a local server on a spare port, signs a player in and runs the shipping
viewer with `--join` on that token (`--smoke` keeps the window hidden and takes its settings from the
command line), then checks the welcome line, a live process at the end of the run and no panic. That
is the app-assembly half of the join path; the `#[ignore]`d `joins_a_running_server` test covers the
transport half.

Delay diagnosis: every session writes `latency.log` (JSON lines) in the server's data folder:
`source: browser` every 5 s (input to frame and to shown, ping, decode, host pipeline, frame
gaps, skipped frames, the server's backlog drops for that player), `source: viewer` every 5 s
(frame time, grab time, encoder busy drops, render to encoded unit), `source: server` on join
and leave, and `source: server`, `event: tick` every 5 s (game tick p50, p99 and max, dropped
ticks, entity counts). Read it after a play session before guessing where delay comes from.

Shipping an update: bump `[workspace.package] version` in `Cargo.toml`, run `tools/dist.py
--notes "..."`, then publish `mc-rs-launcher.exe` and `manifest.json` as release `v<version>` on the
channel `tools/dist.py` names (`REPO`, haunterdev/mc-rs). Every launcher built since 0.2.0 checks that channel at
start and offers the newer exe, which replaces itself in place.

The viewer reads models and textures from the local client jar at startup (`MCRS_CLIENT_JAR`,
Prism's library cache, the vanilla launcher's versions folder, or `%APPDATA%\mc-rs`) and keeps
them in memory. With none found, the launcher asks in its window and downloads the jar from
Mojang into `%APPDATA%\mc-rs`; the viewer alone reports it missing. Never write anything it reads to the repo; viewer tests
build their geometry in code and skip the jar-dependent cases when no jar is found.

Bevy features are listed one by one in `crates/viewer/Cargo.toml` (the "3d" and "ui" sets minus
what the viewer does not use). A Bevy type that will not resolve after an upgrade usually means a
feature to add back there.

Adding a rendered block: the state goes through `blockstates::blockstate_of` to its blockstate
file and variant, and into `blockstates::generated_states` so the jar test bakes it. Material,
opacity and light derive from `worldgen::populate::state` in `sim::region::derive`; the render layer
is `sim::region::layer_of`, the tint `colors::multiplier`, a neighbour-dependent model
`mesher::actual_state`, a non-full collision box `sim::collision::collision_box`.

Memory: the release test build of the viewer peaks near 2 GB, where the dev test build passes
4.3 GB, so test the viewer with `--release`. `tools/bench.py`, `tools/fixtures.py`,
`tools/dist.py` and `tools/join_check.py` run their heavy processes behind the memory guard
`MCRS_RAMGUARD` names, when set.

