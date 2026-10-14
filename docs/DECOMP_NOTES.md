# DECOMP_NOTES

Notes from porting 1.12.2 overworld terrain to `crates/worldgen`. Names are MCP (stable_39)
names from the decompile in `../mc-1.12.2-src/`. Every entry under "Pitfalls" is reproduced in
the port and covered by a fixture test; the Rust file that holds it is named in brackets.

## Call path

`ChunkGeneratorOverworld.generateChunk(x, z)`:

1. `rand.setSeed(x * 341873128712L + z * 132897987541L)`, wrapping long arithmetic.
2. `setBlocksInChunk(x, z, primer)` [terrain.rs]
   - `BiomeProvider.getBiomesForGeneration(x * 4 - 2, z * 4 - 2, 10, 10)`: `genBiomes.getInts`,
     1:4 scale.
   - `generateHeightmap(x * 4, 0, z * 4)`: 5x33x5 densities from `depthNoise` (2D octaves),
     `mainPerlinNoise`, `minLimitPerlinNoise` and `maxLimitPerlinNoise` (3D octaves), weighted by
     the 5x5 `biomeWeights` kernel over the 10x10 biomes.
   - Trilinear fill of the density field: stone where density > 0, water below y 63, air above.
3. `BiomeProvider.getBiomes(x * 16, z * 16, 16, 16)`: `biomeIndexLayer` (the Voronoi zoom),
   1:1. In a server this goes through `BiomeCache`, which calls the same method uncached.
4. `replaceBiomeBlocks(x, z, primer, biomes)` [surface.rs]
   - `surfaceNoise.getRegion` (Perlin, 4 octaves) into `depthBuffer`.
   - `Biome.genTerrainBlocks` per column, overridden by `BiomeHills`, `BiomeMesa`,
     `BiomeSavannaMutated`, `BiomeSwamp` and `BiomeTaiga`; the base is
     `Biome.generateBiomeTerrain` (bedrock, top block, filler, sandstone under sand).
5. `MapGenCaves.generate(world, x, z, primer)` [caves.rs]: `MapGenBase.generate` walks the 17x17
   source chunks around `(x, z)`, seeds `Random(seed)`, then `recursiveGenerate` per source
   chunk, which calls `addRoom` and `addTunnel`.
6. `MapGenRavine.generate(world, x, z, primer)` [ravines.rs]: the same walk, `addTunnel` with the
   ravine shape.
7. Structures (mineshaft, village, stronghold, temple, monument, mansion), `new Chunk(...)`,
   the biome byte array and `generateSkylightMap`. Only the biome array is ported
   [generator.rs]; the rest is out of scope and marked `// DROPPED:`.

## Classes touched

- `net.minecraft.world.gen`: `ChunkGeneratorOverworld`, `ChunkGeneratorSettings` (and its
  `Factory` defaults), `MapGenBase`, `MapGenCaves`, `MapGenRavine`, `NoiseGenerator`,
  `NoiseGeneratorOctaves`, `NoiseGeneratorImproved`, `NoiseGeneratorPerlin`,
  `NoiseGeneratorSimplex`.
- `net.minecraft.world.gen.layer`: `GenLayer`, `GenLayerIsland`, `GenLayerFuzzyZoom`,
  `GenLayerZoom`, `GenLayerAddIsland`, `GenLayerRemoveTooMuchOcean`, `GenLayerAddSnow`,
  `GenLayerEdge`, `GenLayerAddMushroomIsland`, `GenLayerDeepOcean`, `GenLayerRiverInit`,
  `GenLayerBiome`, `GenLayerBiomeEdge`, `GenLayerHills`, `GenLayerRiver`, `GenLayerSmooth`,
  `GenLayerRareBiome`, `GenLayerShore`, `GenLayerRiverMix`, `GenLayerVoronoiZoom`, `IntCache`.
- `net.minecraft.world.biome`: `BiomeProvider`, `BiomeCache`, `Biome` and the registered
  subclasses (`BiomeOcean`, `BiomePlains`, `BiomeDesert`, `BiomeHills`, `BiomeForest`,
  `BiomeTaiga`, `BiomeSwamp`, `BiomeRiver`, `BiomeSnow`, `BiomeMushroomIsland`, `BiomeBeach`,
  `BiomeJungle`, `BiomeStoneBeach`, `BiomeSavanna`, `BiomeSavannaMutated`, `BiomeMesa`,
  `BiomeForestMutated`, `BiomeVoid`, `BiomeEnd`, `BiomeHell`), `Biome.TempCategory`.
- `net.minecraft.world.chunk.ChunkPrimer`, `net.minecraft.util.math.MathHelper`,
  `java.util.Random`, `java.lang.Math.round`, `java.lang.StrictMath.log` (fdlibm).
- Block states: stone, grass, dirt (and its variants), bedrock, water, flowing water, lava,
  flowing lava, sand, red sand, gravel, sandstone, red sandstone, ice, snow layer, snow,
  mycelium, waterlily, hardened clay, stained hardened clay [blocks.rs].

## Pitfalls

### Arithmetic

- **MathHelper.floor wraps.** `floor(double)` is `(int)value`, then `i - 1` when the value is
  below `i`. Java's cast saturates, so for a value below `Integer.MIN_VALUE` the `- 1` wraps to
  `Integer.MAX_VALUE`. A plain Rust `i - 1` panics in a debug build. [math_helper.rs]
- **Math.round has two JDK 8 implementations.** The fixture JVM (8u492) uses the
  JDK-8010430 bit algorithm (round half up on the exact significand), not
  `(long)Math.floor(a + 0.5)`. They differ only for odd integers at or above 2^52, which
  `BiomeMesa.getBand` never reaches; the port uses the bit algorithm so the round-trip test
  passes on every input. [math_helper.rs]
- **Signed zero in Math.min.** `Math.min(0.0, -0.0)` is `-0.0`, and a NaN first argument wins.
  `f64::min` does neither. `BiomeMesa` takes the min of an `abs` and a noise value, so both
  cases can occur. [surface.rs `java_min`]
- **`NoiseGeneratorSimplex.fastFloor`** is `value > 0 ? (int)value : (int)value - 1`, so a whole
  non-positive number floors one too low (0 gives -1). Ported as is. [noise/simplex.rs]
- **Float versus double.** `ChunkGeneratorSettings` stores its scales as `float`, and the
  biome weights, cave and ravine angles and widths are `float`. Each value is widened to
  double exactly where Java widens it; changing the point of widening changes the output.
  [terrain.rs, caves.rs, ravines.rs]
- **`MathHelper.sin`/`cos` read the 65536-entry table** with `(int)(value * 10430.378F) & 65535`,
  so the float multiply and the table, not `f32::sin`, are what caves and ravines use.
  [math_helper.rs]

### Noise

- **The 2D octave overload ignores its last argument.**
  `generateNoiseOctaves(double[], int, int, int, int, double, double, double)` samples at y
  offset 10 with y scale 1.0 whatever the caller passes. [noise/octaves.rs]
- **NoiseGeneratorImproved 2D path.** With `ySize == 1` it ignores `yOffset` and mixes
  `grad2` with `grad(.., 0.0, ..)`. [noise/improved.rs]
- **The 3D gradient cache key lives outside the column loops.** `k` is declared once per
  call; the `j5 == 0` check forces a refresh at the start of each column, so it is harmless,
  but it has to stay that way to match. [noise/improved.rs]
- **Simplex `getValue` takes no offset**, unlike `add`. [noise/simplex.rs]
- **`scaleNoise` is never sampled but must still be built**: it advances the shared
  `Random(seed)` that `depthNoise` is built from next. `forestNoise` is built last, so it can
  be dropped. [terrain.rs]

### GenLayer

- **Per-layer chunk seed.** Vanilla mutates `chunkSeed` in each layer object, which makes a
  stack single-threaded. Every layer calls `initChunkSeed` before its first `nextInt` for a
  cell, so the port keeps the seed in a per-cell local and the stack is shareable across
  threads. [genlayer.rs]
- **`worldGenSeed` is 0 on two layers.** `initializeAllBiomeGenerators` calls
  `initWorldGenSeed` on the river mix and the Voronoi zoom only. The recursion follows
  `parent`, and `GenLayerRiverMix` adds its second chain, but `GenLayerHills` does not recurse
  into its river input, so the two `GenLayerZoom` nodes from
  `magnify(1000L, genlayerriverinit, 2)` that feed it keep `worldGenSeed == 0`. [genlayer.rs]
- **Several layers seed the cell RNG and never draw** (`GenLayerEdge` cool/warm,
  `GenLayerBiomeEdge`, `GenLayerShore`). Harmless with per-cell seeding; kept for fidelity.
- **Voronoi over-read.** `GenLayerVoronoiZoom` fills a scratch grid
  `((width >> 2) + 1) * 4` wide and copies from offset `(areaX - 2) & 3`. When
  `((areaX - 2) & 3) + width % 4 > 4` (same for z) it reads rows it never wrote and returns
  whatever `IntCache` held. Vanilla only calls it with chunk-aligned 16x16 areas; the port
  panics on such a request instead of returning garbage. [genlayer.rs]
- **`BiomeOcean` overrides `getTempCategory`** to OCEAN. The base method compares the float
  temperature widened to double against 0.2 and 1.0. [biome.rs]
- **Unknown biome ids fall back to ocean** (`Biome.getBiome(id, Biomes.DEFAULT)`). [biome.rs]

### Surface (replaceBiomeBlocks)

- **Transposed columns.** `replaceBiomeBlocks` loops `i` (x) then `j` (z), reads
  `biomes[j + i * 16]` and `depthBuffer[j + i * 16]`, and passes world coordinates
  `(x * 16 + i, z * 16 + j)`. `generateBiomeTerrain` then writes the primer at
  `(z & 15, y, x & 15)`. The block column and the biome agree, but the world x and z the
  biome code sees (Mesa bands and pillars, the Swamp grass-color noise) belong to the
  transposed column. [surface.rs]
- **The topBlock leak.** Biomes are singletons, and `BiomeHills`, `BiomeSavannaMutated` and
  the mega `BiomeTaiga` variants assign their own `topBlock`/`fillerBlock` fields on every
  call. Caves and ravines later read `world.getBiome(pos).topBlock`, so they see the value the
  biome's last column in this chunk left. The port returns that value per column
  (`SurfaceResult::column_top`) instead of mutating shared state. [surface.rs]
- **Caves and ravines read that top block differently.** Caves write
  `topBlock.getBlock().getDefaultState()`, which turns coarse dirt and podzol into plain dirt;
  ravines write `topBlock` with its variant. [caves.rs, ravines.rs]
- **Mesa pillars come from `Random(0)`.** `BiomeMesa.genTerrainBlocks` rebuilds its pillar
  noises when the seed changes, but seeds them with the field `worldSeed` before updating it.
  On the first world in a JVM that field is 0, so pillars are the same for every seed. A
  second world generated in the same JVM gets pillars from the previous world's seed; the
  harness runs one JVM per seed, and the port models the first-world case. [surface.rs]
- **`getBand` samples x twice.** `clayBandsOffsetNoise.getValue(x / 512.0, x / 512.0)`; z is
  never read. [surface.rs]
- **Dead Mesa state.** `BiomeMesa.genTerrainBlocks` assigns an `iblockstate` (stained clay,
  air or water) that is never written to the primer. Dropped. [surface.rs]
- **`Biome.getTemperature` above y 64** is never reached from the water-or-ice check (the
  column index there is below 63), so the base temperature is used. [surface.rs]

### Caves and ravines

- **Seeding.** `MapGenBase.generate` draws `j = nextLong()` and `k = nextLong()` from
  `Random(worldSeed)`, then seeds with `l * j ^ i1 * k ^ worldSeed` per source chunk, wrapping.
  [caves.rs `for_each_source_chunk`]
- **Draw order around rooms.** `addRoom(rand.nextLong(), ...)` draws the long first, then
  `addRoom` draws the room width with `nextFloat`, then the caller draws
  `k += nextInt(4)`. [caves.rs]
- **Water check jumps over the interior.** In the water scan, any column that is not on the
  box edge sets `i2 = l2` after the first test, so it checks only the top and the bottom
  layer of interior columns. [caves.rs `hits_water`]
- **Off by one in the carve loop.** The ellipsoid test uses `(j2 - 1) + 0.5`, but the block
  read and replaced is at `j2`; the grass-to-top-block fix-up is at `j2 - 1`. The lava check
  is also `j2 - 1 < 10`. [caves.rs, ravines.rs]
- **Lava differs.** Caves place `LAVA`, ravines place `FLOWING_LAVA`. [caves.rs, ravines.rs]
- **Ravine width profile.** `rs[]` is 1024 floats, but `addTunnel` fills and reads only the
  first 256 on each call, rerolling on `j == 0 || nextInt(3) == 0`. [ravines.rs]

### Population

`ChunkGeneratorOverworld.populate(x, z)` [populate/mod.rs]: lakes, dungeons,
`Biome.decorate` and `BiomeDecorator` (ores, disks, trees, plants, springs),
`WorldEntitySpawner.performWorldGenSpawning`, then ice and snow. It runs against `PopWorld`
[populate/world.rs], a port of the `World`/`Chunk` paths population touches (`setBlockState`
with its flags, the heightmap, `neighborChanged`, the immediate tick branch). Checked by
`tests/populate.rs` against the harness `pop` dumps: blocks, heightmaps and late generations
over the 3x3 fully populated chunks of a 4x4 square, one region per biome class, six seeds.

- **The harness world needs `WorldServer.scheduleUpdate`.** `World.scheduleUpdate` is empty;
  only `WorldServer` forwards it to `updateBlockTick`. Without the override a liquid placed
  during population (a lake's flowing edge, a spring) never runs its nested immediate ticks, and
  Java dumps air where the port, correctly, has flowing water. [harness HarnessWorld.java]
- **Immediate ticks only.** With `scheduledUpdatesAreImmediate` set (springs, lakes,
  `fallInstantly`), `updateBlockTick` calls `updateTick` at once when the 17x17x17 area is
  loaded. The pending tick list is dropped: nothing ticks after population in the fixtures, and
  the viewer does not tick yet. [populate/world.rs]
- **Fossils place nothing.** `WorldGenFossils` draws its own `Random` exactly as vanilla does,
  but its two structure templates are Mojang assets loaded through the save handler; the
  harness returns empty templates and the port keeps only the draws. [populate/features2.rs]
- **Tile entity data is dropped.** The dungeon chest's loot table and the spawner's mob id live
  on tile entities; the blocks are placed, their data is not. [populate/features2.rs,
  populate/world.rs]
- **Entities are dropped, their draws are kept.** `performWorldGenSpawning` draws every
  `World.rand` value vanilla draws (pack size, positions, `onInitialSpawn` rolls), so the
  population RNG stays aligned, but no entity is created. [populate/spawn.rs]
- **`WorldGenBigTree` uses `Math.sin`/`Math.cos`**, not the `MathHelper` table, for its limb
  directions. Rust's `f64::sin`/`cos` agree with the JDK 8 intrinsics on every fixture value.
  [populate/trees2.rs]
- **`WorldGenBigTree.trunkSize` is always 1.** No caller changes it, so the `trunkSize == 2`
  branches are dropped. [populate/trees2.rs]
- **Structures are not generated:** `generateStructure` for every structure is dropped and the
  village result `flag` is the constant false. [populate/mod.rs]
- **Lighting is dropped.** `checkLight`, skylight propagation and `generateSkylightMap` do not
  affect any block population places (the harness `checkLightFor` does nothing). The viewer
  computes light itself. [populate/world.rs]

### Outside the 0.1 scope

- **`ChunkPrimer.findGroundBlockIdx(x, z)` reads the wrong column.** It sets
  `i = (x << 12 | z << 8) + 256 - 1` and then reads `data[i + j]` for `j` from 255 down, so
  for `j >= 1` it reads the next column in storage order, (x, z + 1) or (x + 1, 0) when z is
  15, at y `j - 1`, and only `j == 0` lands in its own
  column (at y 255). The result is the top non-air y of the next column, plus one. At
  (15, 15) it runs past the array. Its only callers, `MapGenEndCity` and `WoodlandMansion`,
  pass columns 2 to 12, so vanilla never overruns. A structures port has to keep this. The
  bench checksum uses its own top-of-column scan on both sides. [examples/bench.rs]

## Viewer pitfalls

The viewer (`crates/viewer`) renders the generated chunks the way the 1.12.2 client does. It
is a port too, so its pitfalls go here. Assets come from the local client jar at runtime.

### Textures and meshing

- **Mipmap blend check reads the wrong span.** `TextureUtil.generateMipmapData` decides between
  the alpha-aware and the plain blend by scanning `data[0][0 .. data.length]`, the first
  `levels + 1` pixels of level 0, not the whole image. The result depends on the mipmap
  setting. [textures.rs `generate_mipmap_data`]
- **Unmipped cutout.** `BlockRenderLayer.CUTOUT` draws with the mipmaps off, so its sprites
  sample level 0 only; `CUTOUT_MIPPED` (leaves) uses the mips. [chunk_common.wgsl `sample_block`]
- **Two alpha tests.** Solid and cutout layers test at 0.5 (`alphaFunc(GREATER, 0.5)`), the
  translucent layer at 0.1 with blending on. [material.rs, chunk.wgsl]
- **Raw-space filtering.** The atlas is an RGBA8 texture with no sRGB flag, so mip and linear
  filtering run on the stored gamma values and the lightmap multiplies them before display.
  The array texture is `Rgba8Unorm` and the shader converts to linear only at the end.
  [textures.rs `image`]
- **`getAoBrightness` zero fill.** A zero neighbor brightness is replaced by the center's before
  averaging, and the sum is masked with `16711935` after the shift, so sky and block light
  average separately. [mesher.rs `ao_brightness`]
- **AO translucency.** The AO corner test reads `isTranslucent`, which is
  `!material.blocksLight()`: water, lava and ice block light and so count as opaque corners.
  [region.rs `translucent`]
- **`useNeighborBrightness`** is set for every non-air block that is translucent or has opacity
  0, and makes `getLight` take the brightest of up, east, west, south and north.
  [region.rs, sky.rs `world_light`]
- **Fluid flow angle uses `MathHelper.atan2`**, a table lookup with a cubic correction, not
  `Math.atan2`. [fluid.rs `atan2`]
- **Fluid corner heights.** `getFluidHeight` averages the corner shared with the -x, -z and -x-z
  neighbors, and water under water reads full height. [fluid.rs]

### Populated blocks

- **Element rotation rescale.** A rotated model element (the cross planes of grass and flowers,
  45 degrees on y) stretches by `1 / cos(angle) - 1` on the two other axes when `rescale` is set,
  with the 22.5 and 45 degree factors computed from float radians. Facing remaps (`apply_facing`)
  skip rotated elements. [models.rs `rotate_part`]
- **Non-cubic AO weights.** A quad that does not cover its face (snow layers, cactus sides)
  weights the four corner values by `quadBounds` products per vertex
  (`EnumNeighborInfo.vert0Weights`..`vert3Weights`), for both the color and the brightness,
  which `getVertexBrightness` mixes per channel and masks with 255. [mesher.rs `WEIGHTS`]
- **Plant offset.** Flowers and double plants shift on x and z, tall grass also down, by
  `getCoordinateRandom(x, 0, z)`; the shift is added in double before the float vertex add.
  [mesher.rs `offset`]
- **Actual state, not stored state.** A double plant's upper half stores only `half=upper`; its
  model and tint come from the variant below (sunflower when the block below is not a lower half).
  Vines read `UP` from the block above's bottom face. [mesher.rs `actual_state`]
- **Spruce and birch leaves are not biome tinted.** `BlockColors` gives them fixed colors
  (6396257, 8431445); the other leaves and vines use the foliage colormap, whose lookup has no
  bounds check. Swamp and mesa override the foliage color. [colors.rs]
- **Snow layer sides.** `BlockSnow.shouldSideBeRendered` always draws the top and hides a side
  against a snow layer at least as high. [mesher.rs `should_side_be_rendered`]
- **Population order is the loader's, not the server's.** The populator populates chunk `q` as
  soon as `q`, `q + x`, `q + z` and `q + x + z` are loaded, as `Chunk.populate(provider,
  generator)` does, but chunks arrive in worker order, so the order (and so `World.rand` and the
  overlap between neighbouring passes) differs from a vanilla server's. A pass that writes past its
  2x2 into a chunk already sent to the mesher is not sent again (vanilla sends a block update).
  [populator.rs]
- **Dropped in the viewer:** chests (tile entity renderer, so `invisible`), connected-chest
  collision boxes, and fast-graphics leaves (the fancy rule is the plain neighbour test).

### Sky, fog and clouds

- **The sky's origin is the feet.** `orientCamera` translates by `-eyeHeight` before
  `renderSky`, so the planes at y 16 and -16 and the dark plane's `d3 = eyeY - 63` sit in the
  frame of the feet. [sky.rs `sky_mesh`]
- **Face culling hides the low planes.** Culling stays on during `renderSky`; the up-facing
  planes below the horizon show only while they are under the eye. [sky.rs `below`]
- **The black box under the horizon** sits inside the black plane for every direction it covers
  and fogs to the same color. Dropped. [sky.rs]
- **Two fogs.** The sky uses `setupFog(-1)`, which starts at the eye; terrain and clouds use
  `setupFog(0)`, which starts at three quarters of the far plane. Fog distance is radial
  (`GL_NV_fog_distance`). [sky.rs `write_env`]
- **Cloud order depends on the eye.** Below y 128 the clouds draw before the terrain with alpha
  test 0.5 and write depth; from 128 up they draw after everything with the default 0.1.
  [clouds.rs `update_clouds`]
- **Fancy clouds in two passes.** Pass 0 writes depth with `colorMask(false)`, pass 1 colors only
  the nearest face. The faces are 8-cell strips, alpha tested per texel; the port splits them
  per cell and keeps the cells the test keeps. [clouds.rs]
- **Cloud drift** is `(cloudTickCounter + partialTicks) * 0.03` blocks east, computed in float
  and widened, then wrapped at 2048 cells. [clouds.rs `placement`]
- **Light at the eye for fog.** `getLightFromNeighborsFor` clamps y to 0 and answers sky 15 for
  y at or above 256 and for unloaded chunks; `World.getLight` answers 0 for an unloaded chunk.
  [sky.rs `neighbor_light`, `light_subtracted`]
- **FOV in flight.** `getFovModifier` starts at 1.1 for a flying player and averages the speed
  attribute (raised 30 % by sprinting) against the walk speed, so a creative flyer sees a wider
  FOV than the setting. Water multiplies the result by 60 / 70. [sky.rs `update_view`, `View::fov`]

### Hand and GUI

- **Item lights live in eye space.** `rotateArroundXAndY` sets the two standard item lights
  after rotating by the player's pitch and yaw and then pops the matrix, so the lights stay
  fixed in the world while the arm moves with the view. [hand.rs `item_lights`]
- **Arm sway.** `rotateArm` uses the gap between the view angles and `renderArmPitch/Yaw`, which
  step halfway toward the view each tick, in double. [hand.rs `Hand::tick`, `arm_matrix`]
- **Swing restarts.** `swingArm` restarts only when no swing is running or the current one is
  past half (`swingProgressInt >= 3`). Each attack press also resets the cooldown, which dips
  the equip progress and shows the attack indicator. [hand.rs]
- **Hand FOV.** The hand uses `getFOVModifier(pt, false)`: 70 regardless of the FOV setting,
  60 in water. [main.rs `sync_cameras`]

### Break, place and pick

- **A miss still has a block.** `getMouseOver` asks `rayTraceBlocks` for the last block it passed
  without colliding, as a MISS whose face is the one the ray entered through (moving +z enters
  NORTH). A click on it resets the cooldown and swings, nothing breaks. [target.rs]
- **The ray tests the bounding box**, not the collision box: flowers, grass, snow and vines are
  selectable, and a vine on one wall is its thin side box. Cactus draws its inset selection box.
  [target.rs `bounding_box`, `selected_box`]
- **Two kinds of replaceable.** `ItemBlock.onItemUse` moves to the neighbour when the clicked block
  is not `Block.isReplaceable` (one snow layer, tall grass, dead bush, vine, double grass and fern
  are), but `World.mayPlace` then checks the target's `Material.isReplaceable`. [interact.rs]
- **The client checks before it sends.** `processRightClickBlock` fails at once when
  `ItemBlock.canPlaceBlockOnSide` says no, so no swing. Reeds are an `ItemBlockSpecial` and skip
  the check. A chest is "activated" (true on the client) unless sneaking with an item.
- **The slab merge succeeds even when blocked.** `ItemSlab.onItemUse` returns SUCCESS for a
  matching half whether or not the double slab clears the player, so the arm swings with no
  block. [interact.rs `on_item_use`]
- **Lily pads take two paths.** The block click fails (no bush soil under the water), then
  `rightClickMouse` goes on to `onItemRightClick`, whose own ray stops at liquids. [interact.rs]
- **Held attack in creative** breaks the pointed block every sixth tick: `clickBlock` sets
  `blockHitDelay` to 5 and `onPlayerDamageBlock` counts it down before breaking again.
- **Pick block returns the item, not the block:** huge mushrooms give the small one, farmland
  gives dirt, the double slab its half, lit redstone ore the plain one; a double plant's upper
  half gives the variant below. [interact.rs `pick`]
- **Placing runs twice.** The client sets the block at once and the populator thread runs
  `canPlaceBlockAt` again with light (mushrooms) and answers with what the world holds, which
  replaces the guess. [stream.rs `edit`, populator.rs]
- **Item model names follow `RenderItem.registerItems`, not the blockstate names.** The polished
  stones are `granite_smooth`, `diorite_smooth`, `andesite_smooth`; the jar has no
  `smooth_granite`. Wood variants take `<variant>_<kind>` with LOG2 and LEAVES2 offset by 4.
  [item.rs `item_model_name`]
- **`ItemModelGenerator` edge faces divide by the size minus one.** An UP or DOWN span scales its
  y by `16 / (height - 1)`, a WEST or EAST span its x by `16 / (width - 1)`, so the edge strips of
  a flat item sit a hair off the pixel grid. Kept. [item.rs `generated_parts`]
- **Items ignore the baked face shade.** `RenderItem.renderQuads` writes the stack color with
  `putColor4`, replacing the shade the bake put in; GUI 3D items and held items get the fixed
  item lights instead. [item_render.rs `item_mesh`, shaders/item.wgsl]
- **Items draw from mip level 0.** `renderItemModel` calls `setBlurMipmap(false, false)`, so the
  block sheet is sampled GL_NEAREST with no mipmaps whatever the mipmap option says.
  [chunk_common.wgsl `sample_block_level0`]

### Sound, particles and other players

- **`Particle.move` reads `y != y` in the decompile.** The decompiler reused the name for the
  asked motion and the collided one; the original tests `origY != y && origY < 0`, which is what
  makes debris land. [particles.rs `Particles::tick`]
- **`swingProgressInt` starts at -1.** A swing runs seven ticks from `swingArm` to the end
  (-1 up to `getArmSwingAnimationEnd`), not six. [players.rs `Other::tick`]
- **Other players' steps come from the server.** `EntityOtherPlayerMP` moves by `setPosition`,
  so no step sound plays client-side; the server's `Entity.move` plays it for everyone but the
  walker. A shared world here computes the same steps from the relayed positions.
  [players.rs `step_due`]
- **Sound volume and range.** `SoundManager.playSound` clamps `volume * category level` to 0..1
  for the gain, but the attenuation distance is `16 * max(volume, 1)` from the unclamped volume;
  pitch clamps to 0.5..2. [audio.rs `Audio::play`]

### Third person, entity lighting and name tags

- **`orientCamera` reads back to the modelview's translate.** Behind the player it is
  `translate(0, 0, -d)` before `Rx(pitch) Ry(yaw + 180)`; in front `rotate(180, Y)` then
  `translate(0, 0, d)` (the decompiled chain of four rotations cancels to these). `d` starts at 4
  (`thirdPersonDistancePrev` never changes in 1.12.2) and shrinks to the nearest of eight ray
  traces from a 0.2 box around the eye. [sky.rs `third_person_distance`, `update_view`]
- **Entity lighting is world space.** `RenderHelper.enableStandardItemLighting` runs after the
  camera transform, so the two lights stay fixed in the world, not the view.
  [shaders/entity.wgsl]
- **`drawNameplate` scales by 0.025** in 1.12.2 (the 0.016666668 * 1.6 of older versions is
  gone); the quad is `-width/2 - 1` to `width/2 + 1` by -1 to 8 in font pixels, and
  `RenderManager.playerViewY` turns 180 degrees in the front view with the pitch negated.
  [players.rs `plate_meshes`, `pose_others`]
- **Glyph widths come from the font texture.** `FontRenderer.readFontTexture` takes each cell's
  last column with an opaque pixel, plus one, scaled to 8 pixels a cell, plus one; a glyph quad
  is `charWidth - 1.01` wide. [players.rs `Font::widths`]

## Items pitfalls (`crates/items`)

- Recipe load order: vanilla registers the JSON recipes in the jar's zip-walk order; `Recipes`
  loads them in name order. No two 1.12.2 JSON recipes match the same grid (checked by
  `every_recipe_matches_its_own_grid`), so the first match is the same either way.
- A snow layer drops layers + 1 snowballs, and only to a shovel (`BlockSnow.harvestBlock` and
  `canHarvestBlock`), not one snowball per layer.
- Break time follows `PlayerControllerMP.onPlayerDamageBlock`'s float accumulation of
  `getPlayerRelativeBlockHardness` per tick: bare-hand stone takes 151 ticks, not the 150 the
  division suggests.
- Furnace experience (`SlotFurnaceOutput.onCrafting`): when the per-item value is 1.0 or more the
  player gets exactly `count` experience; below 1.0 the fraction is rolled with `Math.random`.
- No block overrides `getBlockHardness` in 1.12.2, so the per-state hardness in the generated
  property table is the whole answer.
- `InventoryPlayer.addResource` into an empty slot builds `new ItemStack(item, 0, meta)`, which is
  empty, so its `getMaxStackSize()` is air's 64: an empty slot takes up to 64 ender pearls or
  buckets in one stack (1.12.2's `/give` overstacking). A slot already holding the item stops at
  the item's own limit.
- `InventoryPlayer.add` resets `i` to the stack's count on every pass of its loop, so when a pass
  takes some items and the next takes none it returns false, although the stack shrank.
- `writeToNBT` writes the off hand as `(byte)150`, which reads back as -106 in the signed byte;
  `readFromNBT` masks `Slot & 255`, so both 150 and -106 load into the off hand.

### Containers (`crates/sim/src/container`, T7a)

- `slotClick` mutates the slot's own `ItemStack` object in place (`grow`, `shrink`, the
  `mergeItemStack` top-ups). `items::ItemStack` is a value, so those spots write the new value
  back with `Container::set_raw`, which fires no `onCraftMatrixChanged` and keeps the pickup bob;
  `putStack` and `setInventorySlotContents` fire both.
- `InventoryCraftResult.decrStackSize` is `getAndRemove`: it hands out the whole result whatever
  the count, so a right click on the result takes all of it, and in PICKUP the "slot is now
  empty" test reads the taken stack (the same Java object), not a split remainder.
- Shift-clicking the result when the inventory cannot take the whole craft: the remainder that
  stayed in the result slot is dropped after `onTake` (`playerIn.dropItem(itemstack2, false)`).
  Vanilla behaviour, kept.
- `dragSlots` is a `HashSet<Slot>` iterated in identity-hash order; stage 2 gives every slot the
  same share and caps per slot, so the result is order-independent and a `Vec` in insertion
  order stands in.
- A non-QUICK_CRAFT click while `dragEvent != 0` only cancels the drag; the click itself is lost.
- Java throws `IndexOutOfBoundsException` for a slot id outside the window (the server's packet
  task swallows it); the port returns at that point with whatever state already changed.
- `slotChangedCraftingGrid` sets `recipeUsed` only when a recipe matched, and `SlotCrafting.onTake`
  takes the grid one slot at a time, each change recomputing the result, so `recipeUsed` can end
  on a recipe an intermediate grid matched. Only the recipe book reads it (dropped).
- `mergeItemStack`'s top-up loop ignores the target slot's limit; the armor slots stay at one
  only because `ContainerPlayer.transferStackInSlot` targets an armor slot when it is empty.

## How the port is checked

`harness/` drives the real `ChunkGeneratorOverworld` in a JVM through a minimal `World`
(`HarnessWorld`), and writes gzip binary dumps plus a JSON manifest per seed to `fixtures/`
(gitignored). `crates/worldgen/tests/` compares, per seed and chunk: the GenLayer stack
(classes, seeds, every layer's `getInts`), biomes, the density field, the noise primer, the
surface primer, caves, ravines and the final chunk with its biome array. A mismatch prints
the first differing coordinate and both values. Overflow checks stay on in the test build
(`opt-level = 3` on the dev profile), so a missed wrapping operation panics.
