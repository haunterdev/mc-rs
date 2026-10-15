package mcrs.harness;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;

import net.minecraft.block.Block;
import net.minecraft.block.BlockColored;
import net.minecraft.block.BlockDirt;
import net.minecraft.block.BlockSand;
import net.minecraft.block.state.IBlockState;
import net.minecraft.init.Blocks;
import net.minecraft.init.Bootstrap;
import net.minecraft.item.EnumDyeColor;
import net.minecraft.util.math.MathHelper;
import net.minecraft.world.biome.Biome;
import net.minecraft.world.biome.BiomeProvider;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.chunk.ChunkPrimer;
import net.minecraft.world.gen.ChunkGeneratorOverworld;
import net.minecraft.world.gen.MapGenBase;
import net.minecraft.world.gen.NoiseGeneratorOctaves;
import net.minecraft.world.gen.NoiseGeneratorPerlin;
import net.minecraft.world.gen.NoiseGeneratorSimplex;
import net.minecraft.world.gen.layer.GenLayer;
import net.minecraft.world.gen.layer.GenLayerHills;
import net.minecraft.world.gen.layer.GenLayerRiverMix;
import net.minecraft.world.gen.layer.IntCache;

/**
 * Reference harness for mc-rs. Drives the vanilla 1.12.2 overworld generator and writes
 * fixtures the Rust port is diffed against.
 *
 *   misc  <outdir>           Random, MathHelper, StrictMath, noise samples, biome table
 *   seed  <seed> <outdir>    GenLayer per-layer dumps and staged chunk dumps for one seed
 *   pop   <seed> <outdir>    populated chunk squares, one per biome class (see Pop)
 *   props <outdir>           per-state block and material properties
 *   bench <seed> <side>      single-thread chunks/sec over a side x side square at the origin
 *
 * Run each seed in its own JVM: BiomeMesa keeps per-instance seed state (see DECOMP_NOTES).
 */
public final class Harness {
    static PrintStream out;

    public static void main(String[] args) throws Exception {
        out = System.out;
        PrintStream err = System.err;
        Bootstrap.register();
        // Bootstrap.register redirects stdout/stderr into the logger.
        System.setOut(out);
        System.setErr(err);
        String mode = args[0];
        if (mode.equals("misc")) {
            Misc.run(new File(args[1]));
        } else if (mode.equals("seed")) {
            SeedDump.run(Long.parseLong(args[1]), new File(args[2]));
        } else if (mode.equals("pop")) {
            Pop.run(Long.parseLong(args[1]), new File(args[2]));
        } else if (mode.equals("props")) {
            Pop.props(new File(args[1]));
        } else if (mode.equals("trace")) {
            int[] box = new int[6];
            for (int i = 0; i < 6; i++) box[i] = Integer.parseInt(args[4 + i]);
            Pop.trace(Long.parseLong(args[1]), Integer.parseInt(args[2]), Integer.parseInt(args[3]), box);
        } else if (mode.equals("creative")) {
            Creative.run(new File(args[1]));
        } else if (mode.equals("states")) {
            States.run(new File(args[1]));
        } else if (mode.equals("bench")) {
            Bench.run(Long.parseLong(args[1]), Integer.parseInt(args[2]));
        } else {
            throw new IllegalArgumentException("unknown mode " + mode);
        }
    }

    static Field field(Class<?> c, String name) {
        try {
            Field f = c.getDeclaredField(name);
            f.setAccessible(true);
            return f;
        } catch (NoSuchFieldException e) {
            throw new IllegalStateException(c.getName() + "." + name, e);
        }
    }

    static Object get(Field f, Object o) {
        try {
            return f.get(o);
        } catch (IllegalAccessException e) {
            throw new IllegalStateException(e);
        }
    }

    static int stateId(IBlockState s) {
        return Block.BLOCK_STATE_IDS.get(s);
    }

    static void writeText(File f, String s) throws IOException {
        Writer w = new OutputStreamWriter(new FileOutputStream(f), StandardCharsets.UTF_8);
        try {
            w.write(s);
        } finally {
            w.close();
        }
    }

    static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }
}

/** One seed's generator, plus reflective access to the generator's private stage state. */
final class Gen {
    static final Field RAND = Harness.field(ChunkGeneratorOverworld.class, "rand");
    static final Field HEIGHT_MAP = Harness.field(ChunkGeneratorOverworld.class, "heightMap");
    static final Field DEPTH_BUFFER = Harness.field(ChunkGeneratorOverworld.class, "depthBuffer");
    static final Field BIOMES_FOR_GEN = Harness.field(ChunkGeneratorOverworld.class, "biomesForGeneration");
    static final Field MAIN_NOISE = Harness.field(ChunkGeneratorOverworld.class, "mainNoiseRegion");
    static final Field MIN_LIMIT = Harness.field(ChunkGeneratorOverworld.class, "minLimitRegion");
    static final Field MAX_LIMIT = Harness.field(ChunkGeneratorOverworld.class, "maxLimitRegion");
    static final Field DEPTH_REGION = Harness.field(ChunkGeneratorOverworld.class, "depthRegion");
    static final Field CAVES = Harness.field(ChunkGeneratorOverworld.class, "caveGenerator");
    static final Field RAVINES = Harness.field(ChunkGeneratorOverworld.class, "ravineGenerator");
    static final Field PRIMER_DATA = Harness.field(ChunkPrimer.class, "data");

    final long seed;
    final HarnessWorld world;
    final ChunkGeneratorOverworld cg;
    final Random rand;
    final MapGenBase caves;
    final MapGenBase ravines;

    Gen(long seed) {
        this.seed = seed;
        this.world = new HarnessWorld(seed);
        this.cg = (ChunkGeneratorOverworld) world.provider.createChunkGenerator();
        this.rand = (Random) Harness.get(RAND, cg);
        this.caves = (MapGenBase) Harness.get(CAVES, cg);
        this.ravines = (MapGenBase) Harness.get(RAVINES, cg);
    }

    static char[] data(ChunkPrimer p) {
        return ((char[]) Harness.get(PRIMER_DATA, p)).clone();
    }

    static int[] ids(Biome[] b, int n) {
        int[] r = new int[n];
        for (int i = 0; i < n; i++) r[i] = Biome.getIdForBiome(b[i]);
        return r;
    }

    double[] doubles(Field f) {
        return ((double[]) Harness.get(f, cg)).clone();
    }

    /** ChunkGeneratorOverworld.generateChunk through the ravine pass, without building a Chunk. */
    ChunkPrimer pipeline(int x, int z) {
        rand.setSeed((long) x * 341873128712L + (long) z * 132897987541L);
        ChunkPrimer p = new ChunkPrimer();
        cg.setBlocksInChunk(x, z, p);
        Biome[] biomes = world.getBiomeProvider().getBiomes(null, x * 16, z * 16, 16, 16);
        cg.replaceBiomeBlocks(x, z, p, biomes);
        caves.generate(world, x, z, p);
        ravines.generate(world, x, z, p);
        return p;
    }

    /** Same order as pipeline(), snapshotting after each stage. */
    Stages staged(int x, int z) {
        Stages s = new Stages();
        rand.setSeed((long) x * 341873128712L + (long) z * 132897987541L);
        ChunkPrimer p = new ChunkPrimer();
        cg.setBlocksInChunk(x, z, p);
        s.genBiomes = ids((Biome[]) Harness.get(BIOMES_FOR_GEN, cg), 100);
        s.depthRegion = doubles(DEPTH_REGION);
        s.mainNoise = doubles(MAIN_NOISE);
        s.minLimit = doubles(MIN_LIMIT);
        s.maxLimit = doubles(MAX_LIMIT);
        s.heightMap = doubles(HEIGHT_MAP);
        s.noise = data(p);
        Biome[] biomes = world.getBiomeProvider().getBiomes(null, x * 16, z * 16, 16, 16);
        s.biomes = ids(biomes, 256);
        cg.replaceBiomeBlocks(x, z, p, biomes);
        s.depthBuffer = doubles(DEPTH_BUFFER);
        s.surface = data(p);
        caves.generate(world, x, z, p);
        s.caves = data(p);
        ravines.generate(world, x, z, p);
        s.ravines = data(p);
        return s;
    }

    /** True when the real generateChunk produces the same blocks and biome bytes as staged(). */
    boolean matchesGenerateChunk(int x, int z, Stages s) {
        Chunk c = cg.generateChunk(x, z);
        for (int bx = 0; bx < 16; bx++) {
            for (int bz = 0; bz < 16; bz++) {
                for (int y = 0; y < 256; y++) {
                    int id = Harness.stateId(c.getBlockState(bx, y, bz));
                    if (id != s.ravines[bx << 12 | bz << 8 | y]) return false;
                }
            }
        }
        byte[] ba = c.getBiomeArray();
        for (int i = 0; i < 256; i++) {
            if ((ba[i] & 255) != s.biomes[i]) return false;
        }
        return true;
    }
}

final class Stages {
    int[] genBiomes;
    int[] biomes;
    double[] depthRegion;
    double[] mainNoise;
    double[] minLimit;
    double[] maxLimit;
    double[] heightMap;
    double[] depthBuffer;
    char[] noise;
    char[] surface;
    char[] caves;
    char[] ravines;

    static int diff(char[] a, char[] b) {
        int n = 0;
        for (int i = 0; i < a.length; i++) if (a[i] != b[i]) n++;
        return n;
    }

    static int count(char[] a, int id) {
        int n = 0;
        for (char c : a) if (c == id) n++;
        return n;
    }
}

final class SeedDump {
    /**
     * Regions each layer is sampled over, in that layer's own coordinates: x, z, w, h. Each keeps
     * ((x - 2) & 3) + w % 4 <= 4 on both axes: past that GenLayerVoronoiZoom reads beyond its scratch
     * rows and returns stale IntCache contents (see docs/DECOMP_NOTES.md).
     */
    static final int[][] LAYER_REGIONS = {
        {-5, -5, 10, 10},
        {37, -90, 13, 7},
        {-120003, 119997, 8, 8},
        {7499990, -7500010, 6, 6},
    };

    /** Fixed chunk coordinates: origin, negative, mixed, far (+-30000) and the world border. */
    static final int[][] FIXED = {
        {0, 0}, {-1, 0}, {0, -1}, {-1, -1}, {1, 0}, {0, 1}, {1, 1}, {-1, 1}, {1, -1},
        {-17, -23}, {-5, -100}, {7, -3}, {-9, 11},
        {30000, 30000}, {-30000, -30000}, {30000, -30000}, {-30000, 30000},
        {1874999, 1874999}, {-1875000, -1875000},
    };

    static final Object[][] BIOME_CLASSES = {
        {"mesa", new int[] {37}},
        {"mesa_plateau", new int[] {38, 39}},
        {"mesa_bryce", new int[] {165}},
        {"mesa_plateau_m", new int[] {166, 167}},
        {"swamp", new int[] {6, 134}},
        {"mega_taiga", new int[] {32, 33, 160, 161}},
        {"extreme_hills", new int[] {3, 20}},
        {"extreme_hills_plus", new int[] {34}},
        {"extreme_hills_m", new int[] {131, 162}},
        {"savanna_m", new int[] {163, 164}},
        {"mushroom", new int[] {14, 15}},
        {"ice_spikes", new int[] {140}},
        {"frozen", new int[] {10, 11, 12, 13, 30, 31, 158}},
        {"desert", new int[] {2, 17, 130}},
        {"beach", new int[] {16}},
        {"stone_beach", new int[] {25}},
        {"cold_beach", new int[] {26}},
        {"deep_ocean", new int[] {24}},
        {"jungle", new int[] {21, 22, 23, 149, 151}},
        {"river", new int[] {7}},
        {"roofed_forest", new int[] {29, 157}},
    };

    static void run(long seed, File dir) throws Exception {
        dir.mkdirs();
        long t0 = System.nanoTime();
        Gen g = new Gen(seed);
        File bin = new File(dir, "seed_" + seed + ".bin.gz");
        FixtureWriter fx = new FixtureWriter(bin);
        StringBuilder js = new StringBuilder();
        js.append("{\n  \"seed\": ").append(seed).append(",\n  \"file\": ").append(Harness.q(bin.getName())).append(",\n");

        // GenLayer graph and per-layer samples.
        BiomeProvider bp = g.world.getBiomeProvider();
        GenLayer genBiomes = (GenLayer) Harness.get(Harness.field(BiomeProvider.class, "genBiomes"), bp);
        GenLayer indexLayer = (GenLayer) Harness.get(Harness.field(BiomeProvider.class, "biomeIndexLayer"), bp);
        LayerGraph graph = new LayerGraph();
        graph.walk(indexLayer);
        js.append("  \"layer_regions\": [");
        for (int r = 0; r < LAYER_REGIONS.length; r++) {
            int[] a = LAYER_REGIONS[r];
            js.append(r == 0 ? "" : ", ").append("[").append(a[0]).append(", ").append(a[1]).append(", ").append(a[2]).append(", ").append(a[3]).append("]");
        }
        js.append("],\n  \"layers\": [\n");
        for (int i = 0; i < graph.layers.size(); i++) {
            GenLayer l = graph.layers.get(i);
            js.append("    {\"i\": ").append(i)
              .append(", \"class\": ").append(Harness.q(l.getClass().getSimpleName()))
              .append(", \"baseSeed\": ").append((Long) Harness.get(LayerGraph.BASE_SEED, l))
              .append(", \"worldGenSeed\": ").append((Long) Harness.get(LayerGraph.WORLD_GEN_SEED, l))
              .append(", \"children\": ").append(graph.childrenJson(l)).append("}")
              .append(i + 1 < graph.layers.size() ? ",\n" : "\n");
            for (int r = 0; r < LAYER_REGIONS.length; r++) {
                int[] a = LAYER_REGIONS[r];
                IntCache.resetIntCache();
                int[] v = l.getInts(a[0], a[1], a[2], a[3]);
                int[] copy = new int[a[2] * a[3]];
                System.arraycopy(v, 0, copy, 0, copy.length);
                fx.i32("layer/" + i + "/" + r, copy);
            }
        }
        js.append("  ],\n");
        js.append("  \"gen_biomes_layer\": ").append(graph.ids.get(genBiomes)).append(",\n");
        js.append("  \"index_layer\": ").append(graph.ids.get(indexLayer)).append(",\n");

        // Chunk selection.
        Map<Long, List<String>> chunks = new LinkedHashMap<Long, List<String>>();
        for (int[] c : FIXED) tag(chunks, c[0], c[1], "fixed");
        Map<String, String> missing = new LinkedHashMap<String, String>();
        coverage(g, genBiomes, chunks, missing);
        ravineChunks(g, chunks);
        js.append("  \"coverage_missing\": [");
        int mi = 0;
        for (String k : missing.keySet()) js.append(mi++ == 0 ? "" : ", ").append(Harness.q(k));
        js.append("],\n");

        // Staged dumps.
        js.append("  \"chunks\": [\n");
        int ci = 0;
        int mismatches = 0;
        for (Map.Entry<Long, List<String>> e : chunks.entrySet()) {
            int x = (int) (e.getKey() >> 32);
            int z = (int) (long) e.getKey();
            Stages s = g.staged(x, z);
            boolean match = g.matchesGenerateChunk(x, z, s);
            if (!match) mismatches++;
            String p = "c/" + x + "/" + z + "/";
            fx.i32(p + "gen_biomes", s.genBiomes);
            fx.i32(p + "biomes", s.biomes);
            fx.f64(p + "depth_region", s.depthRegion);
            fx.f64(p + "main_noise", s.mainNoise);
            fx.f64(p + "min_limit", s.minLimit);
            fx.f64(p + "max_limit", s.maxLimit);
            fx.f64(p + "height_map", s.heightMap);
            fx.f64(p + "depth_buffer", s.depthBuffer);
            fx.u16(p + "noise", s.noise);
            fx.u16(p + "surface", s.surface);
            fx.u16(p + "caves", s.caves);
            fx.u16(p + "ravines", s.ravines);
            js.append(ci++ == 0 ? "" : ",\n").append("    {\"x\": ").append(x).append(", \"z\": ").append(z)
              .append(", \"tags\": [");
            for (int t = 0; t < e.getValue().size(); t++) js.append(t == 0 ? "" : ", ").append(Harness.q(e.getValue().get(t)));
            js.append("], \"changed\": {\"surface\": ").append(Stages.diff(s.noise, s.surface))
              .append(", \"caves\": ").append(Stages.diff(s.surface, s.caves))
              .append(", \"ravines\": ").append(Stages.diff(s.caves, s.ravines))
              .append("}, \"lava\": ").append(Stages.count(s.caves, 11 << 4))
              .append(", \"flowing_lava\": ").append(Stages.count(s.ravines, 10 << 4))
              .append(", \"generate_chunk_match\": ").append(match).append("}");
        }
        js.append("\n  ],\n");
        fx.close();
        double secs = (System.nanoTime() - t0) / 1e9;
        js.append("  \"records\": ").append(fx.records()).append(",\n");
        js.append("  \"generate_chunk_mismatches\": ").append(mismatches).append(",\n");
        js.append("  \"seconds\": ").append(String.format("%.2f", secs)).append("\n}\n");
        Harness.writeText(new File(dir, "seed_" + seed + ".json"), js.toString());
        Harness.out.println("seed " + seed + ": " + chunks.size() + " chunks, " + fx.records() + " records, "
                + mismatches + " generateChunk mismatches, missing " + missing.keySet() + ", " + String.format("%.1f", secs) + "s");
    }

    static long key(int x, int z) {
        return ((long) x << 32) | (z & 0xffffffffL);
    }

    static void tag(Map<Long, List<String>> chunks, int x, int z, String tag) {
        Long k = key(x, z);
        List<String> l = chunks.get(k);
        if (l == null) {
            l = new ArrayList<String>();
            chunks.put(k, l);
        }
        if (!l.contains(tag)) l.add(tag);
    }

    static boolean in(int[] set, int v) {
        for (int s : set) if (s == v) return true;
        return false;
    }

    /**
     * For each biome class, take the nearest dozen 1:4 cells from genBiomes (scanned in rings
     * around the origin), then keep the chunk whose real 16x16 index biomes contain the class most.
     */
    static void coverage(Gen g, GenLayer genBiomes, Map<Long, List<String>> chunks, Map<String, String> missing) {
        final int r = 1024;
        final int side = 2 * r;
        int[] grid = new int[side * side];
        for (int tz = -r; tz < r; tz += 256) {
            for (int tx = -r; tx < r; tx += 256) {
                IntCache.resetIntCache();
                int[] a = genBiomes.getInts(tx, tz, 256, 256);
                for (int z = 0; z < 256; z++) {
                    System.arraycopy(a, z * 256, grid, (tz + r + z) * side + (tx + r), 256);
                }
            }
        }
        int n = BIOME_CLASSES.length;
        List<List<long[]>> cands = new ArrayList<List<long[]>>();
        for (int i = 0; i < n; i++) cands.add(new ArrayList<long[]>());
        for (int d = 0; d < r; d++) {
            for (int dz = -d; dz <= d; dz++) {
                boolean edge = dz == -d || dz == d;
                for (int dx = -d; dx <= d; dx += edge ? 1 : 2 * d) {
                    int v = grid[(dz + r) * side + (dx + r)];
                    for (int i = 0; i < n; i++) {
                        List<long[]> l = cands.get(i);
                        if (l.size() < 12 && in((int[]) BIOME_CLASSES[i][1], v)) {
                            long[] c = {dx >> 2, dz >> 2};
                            boolean dup = false;
                            for (long[] o : l) if (o[0] == c[0] && o[1] == c[1]) dup = true;
                            if (!dup) l.add(c);
                        }
                    }
                    if (d == 0) break;
                }
            }
        }
        for (int i = 0; i < n; i++) {
            String name = (String) BIOME_CLASSES[i][0];
            int[] set = (int[]) BIOME_CLASSES[i][1];
            long[] best = null;
            int bestCount = 0;
            for (long[] c : cands.get(i)) {
                Biome[] b = g.world.getBiomeProvider().getBiomes(null, (int) c[0] * 16, (int) c[1] * 16, 16, 16, false);
                int count = 0;
                for (Biome bi : b) if (in(set, Biome.getIdForBiome(bi))) count++;
                if (count > bestCount) {
                    bestCount = count;
                    best = c;
                }
            }
            if (best == null) {
                missing.put(name, name);
            } else {
                tag(chunks, (int) best[0], (int) best[1], "biome:" + name);
            }
        }
    }

    /** Chunks where MapGenRavine starts a ravine (nextInt(50) == 0) and the ravine pass changes blocks. */
    static void ravineChunks(Gen g, Map<Long, List<String>> chunks) {
        Random r = new Random(g.seed);
        long j = r.nextLong();
        long k = r.nextLong();
        int found = 0;
        for (int d = 0; d < 64 && found < 3; d++) {
            for (int dz = -d; dz <= d && found < 3; dz++) {
                boolean edge = dz == -d || dz == d;
                for (int dx = -d; dx <= d && found < 3; dx += edge ? 1 : 2 * d) {
                    r.setSeed((long) dx * j ^ (long) dz * k ^ g.seed);
                    if (r.nextInt(50) == 0) {
                        Stages s = g.staged(dx, dz);
                        if (Stages.diff(s.caves, s.ravines) > 0) {
                            tag(chunks, dx, dz, "ravine");
                            found++;
                        }
                    }
                    if (d == 0) break;
                }
            }
        }
    }
}

/** Depth-first walk of the GenLayer graph: the node, then parent, then any second parent. */
final class LayerGraph {
    static final Field PARENT = Harness.field(GenLayer.class, "parent");
    static final Field BASE_SEED = Harness.field(GenLayer.class, "baseSeed");
    static final Field WORLD_GEN_SEED = Harness.field(GenLayer.class, "worldGenSeed");
    static final Field RM_BIOME = Harness.field(GenLayerRiverMix.class, "biomePatternGeneratorChain");
    static final Field RM_RIVER = Harness.field(GenLayerRiverMix.class, "riverPatternGeneratorChain");
    static final Field HILLS_RIVER = Harness.field(GenLayerHills.class, "riverLayer");

    final List<GenLayer> layers = new ArrayList<GenLayer>();
    final IdentityHashMap<GenLayer, Integer> ids = new IdentityHashMap<GenLayer, Integer>();

    List<GenLayer> children(GenLayer l) {
        List<GenLayer> c = new ArrayList<GenLayer>();
        GenLayer p = (GenLayer) Harness.get(PARENT, l);
        if (p != null) c.add(p);
        if (l instanceof GenLayerRiverMix) {
            c.add((GenLayer) Harness.get(RM_BIOME, l));
            c.add((GenLayer) Harness.get(RM_RIVER, l));
        }
        if (l instanceof GenLayerHills) c.add((GenLayer) Harness.get(HILLS_RIVER, l));
        return c;
    }

    void walk(GenLayer l) {
        if (ids.containsKey(l)) return;
        ids.put(l, layers.size());
        layers.add(l);
        for (GenLayer c : children(l)) walk(c);
    }

    String childrenJson(GenLayer l) {
        StringBuilder b = new StringBuilder("[");
        List<GenLayer> c = children(l);
        for (int i = 0; i < c.size(); i++) b.append(i == 0 ? "" : ", ").append(ids.get(c.get(i)));
        return b.append("]").toString();
    }
}

final class Misc {
    static final long[] RANDOM_SEEDS = {0L, 1L, -1L, 42L, 123456789L, Long.MIN_VALUE, Long.MAX_VALUE, -4172144997902289642L};
    static final int[] BOUNDS = {1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 16, 25, 40, 50, 64, 100, 256, 299999, 1000000,
        1 << 30, (1 << 30) + 1, 1500000000, Integer.MAX_VALUE};
    static final long[] NOISE_SEEDS = {0L, -4172144997902289642L, 8091867987493326313L};
    /** Noise sample origins in 1:4 generator coordinates (chunk * 4), y = 0. */
    static final int[][] NOISE_POS = {{0, 0}, {-68, 92}, {120000, -120000}, {7499996, 7499996}, {-7500000, -7499996}};

    static void run(File dir) throws Exception {
        dir.mkdirs();
        File bin = new File(dir, "misc.bin.gz");
        FixtureWriter fx = new FixtureWriter(bin);
        StringBuilder js = new StringBuilder();
        js.append("{\n  \"file\": ").append(Harness.q(bin.getName())).append(",\n");
        js.append("  \"java_version\": ").append(Harness.q(System.getProperty("java.version"))).append(",\n");
        js.append("  \"java_vm\": ").append(Harness.q(System.getProperty("java.vm.name") + " " + System.getProperty("java.vm.version"))).append(",\n");

        // Must come first: generation mutates Biome.topBlock / fillerBlock.
        biomeTable(js);
        stateIds(js);
        random(fx, js);
        math(fx);
        noise(fx, js);

        fx.close();
        js.append("  \"records\": ").append(fx.records()).append("\n}\n");
        Harness.writeText(new File(dir, "misc.json"), js.toString());
        Harness.out.println("misc: " + fx.records() + " records");
    }

    static void biomeTable(StringBuilder js) {
        Field baseName = Harness.field(Biome.class, "baseBiomeRegName");
        js.append("  \"biomes\": [\n");
        boolean first = true;
        for (int id = 0; id < 256; id++) {
            Biome b = Biome.getBiomeForId(id);
            if (b == null) continue;
            Biome mut = Biome.getMutationForBiome(b);
            String base = (String) Harness.get(baseName, b);
            js.append(first ? "" : ",\n").append("    {\"id\": ").append(id)
              .append(", \"name\": ").append(Harness.q(Biome.REGISTRY.getNameForObject(b).toString()))
              .append(", \"class\": ").append(Harness.q(b.getBiomeClass().getSimpleName()))
              .append(", \"base_height\": ").append(Float.floatToRawIntBits(b.getBaseHeight()))
              .append(", \"height_variation\": ").append(Float.floatToRawIntBits(b.getHeightVariation()))
              .append(", \"temperature\": ").append(Float.floatToRawIntBits(b.getDefaultTemperature()))
              .append(", \"rainfall\": ").append(Float.floatToRawIntBits(b.getRainfall()))
              .append(", \"snowy\": ").append(b.isSnowyBiome())
              .append(", \"temp_category\": ").append(Harness.q(b.getTempCategory().name()))
              .append(", \"top_block\": ").append(Harness.stateId(b.topBlock))
              .append(", \"filler_block\": ").append(Harness.stateId(b.fillerBlock))
              .append(", \"is_mutation\": ").append(b.isMutation())
              .append(", \"base_biome\": ").append(base == null ? "null" : Harness.q(base))
              .append(", \"mutation\": ").append(mut == null ? -1 : Biome.getIdForBiome(mut)).append("}");
            first = false;
        }
        js.append("\n  ],\n");
    }

    static void stateIds(StringBuilder js) {
        Map<String, IBlockState> m = new LinkedHashMap<String, IBlockState>();
        m.put("air", Blocks.AIR.getDefaultState());
        m.put("stone", Blocks.STONE.getDefaultState());
        m.put("grass", Blocks.GRASS.getDefaultState());
        m.put("dirt", Blocks.DIRT.getDefaultState());
        m.put("coarse_dirt", Blocks.DIRT.getDefaultState().withProperty(BlockDirt.VARIANT, BlockDirt.DirtType.COARSE_DIRT));
        m.put("podzol", Blocks.DIRT.getDefaultState().withProperty(BlockDirt.VARIANT, BlockDirt.DirtType.PODZOL));
        m.put("bedrock", Blocks.BEDROCK.getDefaultState());
        m.put("flowing_water", Blocks.FLOWING_WATER.getDefaultState());
        m.put("water", Blocks.WATER.getDefaultState());
        m.put("flowing_lava", Blocks.FLOWING_LAVA.getDefaultState());
        m.put("lava", Blocks.LAVA.getDefaultState());
        m.put("sand", Blocks.SAND.getDefaultState());
        m.put("red_sand", Blocks.SAND.getDefaultState().withProperty(BlockSand.VARIANT, BlockSand.EnumType.RED_SAND));
        m.put("gravel", Blocks.GRAVEL.getDefaultState());
        m.put("sandstone", Blocks.SANDSTONE.getDefaultState());
        m.put("red_sandstone", Blocks.RED_SANDSTONE.getDefaultState());
        m.put("ice", Blocks.ICE.getDefaultState());
        m.put("snow_layer", Blocks.SNOW_LAYER.getDefaultState());
        m.put("snow", Blocks.SNOW.getDefaultState());
        m.put("mycelium", Blocks.MYCELIUM.getDefaultState());
        m.put("waterlily", Blocks.WATERLILY.getDefaultState());
        m.put("hardened_clay", Blocks.HARDENED_CLAY.getDefaultState());
        for (EnumDyeColor c : EnumDyeColor.values()) {
            m.put("stained_hardened_clay:" + c.getName(), Blocks.STAINED_HARDENED_CLAY.getDefaultState().withProperty(BlockColored.COLOR, c));
        }
        js.append("  \"states\": {");
        boolean first = true;
        for (Map.Entry<String, IBlockState> e : m.entrySet()) {
            js.append(first ? "" : ", ").append(Harness.q(e.getKey())).append(": ").append(Harness.stateId(e.getValue()));
            first = false;
        }
        js.append("},\n");
    }

    static void random(FixtureWriter fx, StringBuilder js) throws IOException {
        js.append("  \"random_seeds\": [");
        for (int i = 0; i < RANDOM_SEEDS.length; i++) js.append(i == 0 ? "" : ", ").append(RANDOM_SEEDS[i]);
        js.append("],\n  \"random_bounds\": [");
        for (int i = 0; i < BOUNDS.length; i++) js.append(i == 0 ? "" : ", ").append(BOUNDS[i]);
        js.append("],\n");
        for (int si = 0; si < RANDOM_SEEDS.length; si++) {
            long s = RANDOM_SEEDS[si];
            String p = "random/" + si + "/";
            Random r = new Random(s);
            int[] ia = new int[64];
            for (int i = 0; i < 64; i++) ia[i] = r.nextInt();
            fx.i32(p + "nextInt", ia);
            for (int bound : BOUNDS) {
                r = new Random(s);
                int[] b = new int[48];
                for (int i = 0; i < 48; i++) b[i] = r.nextInt(bound);
                fx.i32(p + "nextInt/" + bound, b);
            }
            r = new Random(s);
            long[] la = new long[64];
            for (int i = 0; i < 64; i++) la[i] = r.nextLong();
            fx.i64(p + "nextLong", la);
            r = new Random(s);
            double[] da = new double[64];
            for (int i = 0; i < 64; i++) da[i] = r.nextDouble();
            fx.f64(p + "nextDouble", da);
            r = new Random(s);
            float[] fa = new float[64];
            for (int i = 0; i < 64; i++) fa[i] = r.nextFloat();
            fx.f32(p + "nextFloat", fa);
            r = new Random(s);
            double[] ga = new double[64];
            for (int i = 0; i < 64; i++) ga[i] = r.nextGaussian();
            fx.f64(p + "nextGaussian", ga);
            r = new Random(s);
            int[] ba = new int[64];
            for (int i = 0; i < 64; i++) ba[i] = r.nextBoolean() ? 1 : 0;
            fx.i32(p + "nextBoolean", ba);
            // Interleaved calls: the cached second gaussian survives other calls and is cleared by setSeed.
            r = new Random(s);
            long[] mix = new long[16 * 5];
            for (int i = 0; i < 16; i++) {
                mix[i * 5] = Double.doubleToRawLongBits(r.nextGaussian());
                mix[i * 5 + 1] = r.nextInt(10);
                mix[i * 5 + 2] = Double.doubleToRawLongBits(r.nextGaussian());
                mix[i * 5 + 3] = Double.doubleToRawLongBits(r.nextGaussian());
                if (i == 7) r.setSeed(s);
                mix[i * 5 + 4] = Double.doubleToRawLongBits(r.nextDouble());
            }
            fx.i64(p + "mixed", mix);
        }
    }

    static void math(FixtureWriter fx) throws IOException {
        fx.f32("math/sin_table", (float[]) Harness.get(Harness.field(MathHelper.class, "SIN_TABLE"), null));

        float[] fin = new float[4096 + 12];
        for (int i = 0; i < 4096; i++) fin[i] = (i - 2048) * 0.0137F;
        float[] fs = {1.0e6F, -1.0e6F, 3.4e38F, -3.4e38F, Float.NaN, Float.POSITIVE_INFINITY, Float.NEGATIVE_INFINITY,
            0.0F, -0.0F, (float) Math.PI, 12345.678F, -98765.43F};
        System.arraycopy(fs, 0, fin, 4096, fs.length);
        float[] sin = new float[fin.length];
        float[] cos = new float[fin.length];
        float[] sqrt = new float[fin.length];
        for (int i = 0; i < fin.length; i++) {
            sin[i] = MathHelper.sin(fin[i]);
            cos[i] = MathHelper.cos(fin[i]);
            sqrt[i] = MathHelper.sqrt(fin[i]);
        }
        fx.f32("math/f_in", fin);
        fx.f32("math/sin", sin);
        fx.f32("math/cos", cos);
        fx.f32("math/sqrt_f", sqrt);

        Random r = new Random(99L);
        double[] din = new double[2048 + 16];
        for (int i = 0; i < 2048; i++) {
            din[i] = (r.nextDouble() - 0.25) * Math.pow(10.0, r.nextInt(600) - 300);
        }
        double[] ds = {0.0, -0.0, 1.0, 2.0, 0.5, 1.0e-310, Double.MIN_VALUE, Double.MAX_VALUE, Double.POSITIVE_INFINITY,
            Double.NaN, -1.0, 0.49999999999999994, -0.5, 2.5, 4503599627370497.0, -4503599627370497.0};
        System.arraycopy(ds, 0, din, 2048, ds.length);
        double[] log = new double[din.length];
        long[] round = new long[din.length];
        int[] floor = new int[din.length];
        long[] lfloor = new long[din.length];
        for (int i = 0; i < din.length; i++) {
            log[i] = StrictMath.log(din[i]);
            round[i] = Math.round(din[i]);
            floor[i] = MathHelper.floor(din[i]);
            lfloor[i] = MathHelper.lfloor(din[i]);
        }
        fx.f64("math/d_in", din);
        fx.f64("math/strict_log", log);
        fx.i64("math/round", round);
        fx.i32("math/floor", floor);
        fx.i64("math/lfloor", lfloor);
    }

    static void noise(FixtureWriter fx, StringBuilder js) throws IOException {
        float f = 684.412F;
        double[][] scales = {
            {(double) f, (double) f, (double) f},
            {(double) (f / 80.0F), (double) (f / 160.0F), (double) (f / 80.0F)},
        };
        int[] octaves = {16, 8};
        js.append("  \"noise_seeds\": [");
        for (int i = 0; i < NOISE_SEEDS.length; i++) js.append(i == 0 ? "" : ", ").append(NOISE_SEEDS[i]);
        js.append("],\n  \"noise_pos\": [");
        for (int i = 0; i < NOISE_POS.length; i++) js.append(i == 0 ? "" : ", ").append("[").append(NOISE_POS[i][0]).append(", ").append(NOISE_POS[i][1]).append("]");
        js.append("],\n  \"noise_scales_bits\": [");
        for (int i = 0; i < scales.length; i++) {
            js.append(i == 0 ? "" : ", ").append("[");
            for (int j = 0; j < 3; j++) js.append(j == 0 ? "" : ", ").append(Double.doubleToRawLongBits(scales[i][j]));
            js.append("]");
        }
        js.append("],\n  \"noise_octaves\": [16, 8],\n");

        for (int si = 0; si < NOISE_SEEDS.length; si++) {
            long s = NOISE_SEEDS[si];
            for (int c = 0; c < octaves.length; c++) {
                NoiseGeneratorOctaves n = new NoiseGeneratorOctaves(new Random(s), octaves[c]);
                for (int pi = 0; pi < NOISE_POS.length; pi++) {
                    int[] p = NOISE_POS[pi];
                    double[] v = n.generateNoiseOctaves(null, p[0], 0, p[1], 5, 33, 5, scales[c][0], scales[c][1], scales[c][2]);
                    fx.f64("noise/oct3d/" + si + "/" + c + "/" + pi, v);
                }
            }
            NoiseGeneratorOctaves n2 = new NoiseGeneratorOctaves(new Random(s), 16);
            for (int pi = 0; pi < NOISE_POS.length; pi++) {
                int[] p = NOISE_POS[pi];
                double[] v = n2.generateNoiseOctaves(null, p[0], p[1], 5, 5, (double) 200.0F, (double) 200.0F, (double) 0.5F);
                fx.f64("noise/oct2d/" + si + "/" + pi, v);
            }
            NoiseGeneratorPerlin perlin4 = new NoiseGeneratorPerlin(new Random(s), 4);
            for (int pi = 0; pi < NOISE_POS.length; pi++) {
                int[] p = NOISE_POS[pi];
                double[] v = perlin4.getRegion(null, (double) (p[0] * 4), (double) (p[1] * 4), 16, 16, 0.0625D, 0.0625D, 1.0D);
                fx.f64("noise/perlin_region/" + si + "/" + pi, v);
            }
            NoiseGeneratorPerlin perlin1 = new NoiseGeneratorPerlin(new Random(s), 1);
            NoiseGeneratorSimplex simplex = new NoiseGeneratorSimplex(new Random(s));
            double[] xs = new double[256];
            double[] zs = new double[256];
            double[] v1 = new double[256];
            double[] v4 = new double[256];
            double[] vs = new double[256];
            for (int i = 0; i < 256; i++) {
                xs[i] = (i - 128) * 37.123 + (si * 1000003.5);
                zs[i] = (128 - i) * 11.77 - (si * 70001.25);
                v1[i] = perlin1.getValue(xs[i], zs[i]);
                v4[i] = perlin4.getValue(xs[i] * 0.01, zs[i] * 0.01);
                vs[i] = simplex.getValue(xs[i] * 0.1, zs[i] * 0.1);
            }
            fx.f64("noise/xz/" + si + "/x", xs);
            fx.f64("noise/xz/" + si + "/z", zs);
            fx.f64("noise/perlin1_value/" + si, v1);
            fx.f64("noise/perlin4_value/" + si, v4);
            fx.f64("noise/simplex_value/" + si, vs);
        }
    }
}

final class Bench {
    /** Highest non-air y in the column, or 0. Not ChunkPrimer.findGroundBlockIdx, which reads the next column (DECOMP_NOTES). */
    static int top(ChunkPrimer p, IBlockState air, int x, int z) {
        for (int y = 255; y >= 0; --y) {
            if (p.getBlockState(x, y, z) != air) {
                return y;
            }
        }
        return 0;
    }

    static int square(Gen g, int x0, int z0, int side) {
        IBlockState air = Blocks.AIR.getDefaultState();
        int acc = 0;
        for (int z = 0; z < side; z++) {
            for (int x = 0; x < side; x++) {
                ChunkPrimer p = g.pipeline(x0 + x, z0 + z);
                acc += top(p, air, x & 15, z & 15);
            }
        }
        return acc;
    }

    static void run(long seed, int side) {
        Gen g = new Gen(seed);
        square(g, 100000, 100000, side);
        square(g, -100000, 100000, side);
        long t0 = System.nanoTime();
        int acc = square(g, -side / 2, -side / 2, side);
        long t1 = System.nanoTime();
        double secs = (t1 - t0) / 1e9;
        int n = side * side;
        Harness.out.println("{\"seed\": " + seed + ", \"side\": " + side + ", \"chunks\": " + n
                + ", \"seconds\": " + secs + ", \"chunks_per_sec\": " + (n / secs)
                + ", \"java\": " + Harness.q(System.getProperty("java.version"))
                + ", \"vm\": " + Harness.q(System.getProperty("java.vm.name")) + ", \"checksum\": " + acc + "}");
    }
}
