package mcrs.harness;

import java.io.File;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;

import net.minecraft.block.Block;
import net.minecraft.block.SoundType;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.BlockFaceShape;
import net.minecraft.block.state.IBlockState;
import net.minecraft.util.EnumFacing;
import net.minecraft.util.math.BlockPos;
import net.minecraft.world.biome.Biome;
import net.minecraft.world.chunk.Chunk;

/**
 * Population fixtures. For each region, a fresh world pre-generates a margin of chunks, populates a
 * 4x4 square in a fixed order (z outer, x inner) and dumps the 3x3 chunks that received every
 * population that can reach them.
 */
final class Pop {
    static final int SIDE = 4;

    static final Object[][] CLASSES = {
        {"plains", new int[] {1, 129}},
        {"forest", new int[] {4, 18, 132}},
        {"birch", new int[] {27, 28, 155, 156}},
        {"roofed_forest", new int[] {29, 157}},
        {"taiga", new int[] {5, 19, 133}},
        {"mega_taiga", new int[] {32, 33, 160, 161}},
        {"cold_taiga", new int[] {30, 31, 158}},
        {"jungle", new int[] {21, 22, 149}},
        {"jungle_edge", new int[] {23, 151}},
        {"swamp", new int[] {6, 134}},
        {"desert", new int[] {2, 17, 130}},
        {"savanna", new int[] {35, 36, 163, 164}},
        {"mesa", new int[] {37, 38, 39, 165, 166, 167}},
        {"extreme_hills", new int[] {3, 20, 34, 131, 162}},
        {"ice_plains", new int[] {12, 13}},
        {"ice_spikes", new int[] {140}},
        {"mushroom", new int[] {14, 15}},
        {"ocean", new int[] {0, 24}},
        {"frozen", new int[] {10, 11}},
        {"river", new int[] {7}},
        {"beach", new int[] {16, 25, 26}},
    };

    /** One region of `run`, printing every setBlockState and scheduled tick inside `box`. */
    static void trace(long seed, int ox, int oz, int[] box) {
        Gen g = new Gen(seed);
        g.world.useGenerator(g.cg);
        for (int z = oz - 2; z < oz + SIDE + 2; z++) {
            for (int x = ox - 2; x < ox + SIDE + 2; x++) g.world.chunk(x, z);
        }
        g.world.freeze();
        HarnessWorld.traceBox = box;
        for (int z = oz; z < oz + SIDE; z++) {
            for (int x = ox; x < ox + SIDE; x++) {
                Harness.out.println("populate " + x + " " + z);
                g.cg.populate(x, z);
            }
        }
        HarnessWorld.traceBox = null;
    }

    static void run(long seed, File dir) throws Exception {
        dir.mkdirs();
        long t0 = System.nanoTime();
        File bin = new File(dir, "pop_" + seed + ".bin.gz");
        FixtureWriter fx = new FixtureWriter(bin);
        StringBuilder js = new StringBuilder();
        js.append("{\n  \"seed\": ").append(seed).append(",\n  \"file\": ").append(Harness.q(bin.getName()))
          .append(",\n  \"side\": ").append(SIDE).append(",\n  \"regions\": [\n");

        List<Object[]> regions = new ArrayList<Object[]>();
        regions.add(new Object[] {"origin", -1, -1, SIDE * SIDE});
        Gen probe = new Gen(seed);
        List<String> missing = new ArrayList<String>();
        for (Object[] c : CLASSES) {
            Object[] r = find(probe, (String) c[0], (int[]) c[1]);
            if (r == null) missing.add((String) c[0]); else regions.add(r);
        }

        int late = 0;
        for (int i = 0; i < regions.size(); i++) {
            Object[] r = regions.get(i);
            int ox = (Integer) r[1];
            int oz = (Integer) r[2];
            Gen g = new Gen(seed);
            g.world.useGenerator(g.cg);
            for (int z = oz - 2; z < oz + SIDE + 2; z++) {
                for (int x = ox - 2; x < ox + SIDE + 2; x++) g.world.chunk(x, z);
            }
            g.world.freeze();
            for (int z = oz; z < oz + SIDE; z++) {
                for (int x = ox; x < ox + SIDE; x++) g.cg.populate(x, z);
            }
            late += g.world.lateGenerations;
            for (int z = oz + 1; z < oz + SIDE; z++) {
                for (int x = ox + 1; x < ox + SIDE; x++) {
                    Chunk c = g.world.chunk(x, z);
                    char[] d = new char[65536];
                    int[] hm = new int[256];
                    for (int bx = 0; bx < 16; bx++) {
                        for (int bz = 0; bz < 16; bz++) {
                            for (int y = 0; y < 256; y++) d[bx << 12 | bz << 8 | y] = (char) Harness.stateId(c.getBlockState(bx, y, bz));
                            hm[bz << 4 | bx] = c.getHeightValue(bx, bz);
                        }
                    }
                    String p = "pop/" + i + "/" + x + "/" + z + "/";
                    fx.u16(p + "blocks", d);
                    fx.i32(p + "height", hm);
                }
            }
            js.append(i == 0 ? "" : ",\n").append("    {\"i\": ").append(i).append(", \"tag\": ").append(Harness.q((String) r[0]))
              .append(", \"x\": ").append(ox).append(", \"z\": ").append(oz).append(", \"in_class\": ").append(r[3])
              .append(", \"late_generations\": ").append(g.world.lateGenerations).append("}");
            Harness.out.println("  region " + i + " " + r[0] + " at " + ox + "," + oz + " late " + g.world.lateGenerations);
        }
        fx.close();
        js.append("\n  ],\n  \"missing\": [");
        for (int i = 0; i < missing.size(); i++) js.append(i == 0 ? "" : ", ").append(Harness.q(missing.get(i)));
        double secs = (System.nanoTime() - t0) / 1e9;
        js.append("],\n  \"late_generations\": ").append(late).append(",\n  \"records\": ").append(fx.records())
          .append(",\n  \"seconds\": ").append(String.format("%.2f", secs)).append("\n}\n");
        Harness.writeText(new File(dir, "pop_" + seed + ".json"), js.toString());
        Harness.out.println("pop " + seed + ": " + regions.size() + " regions, late " + late + ", missing " + missing + ", "
                + String.format("%.1f", secs) + "s");
    }

    static int popBiome(Gen g, int x, int z) {
        Biome[] b = g.world.getBiomeProvider().getBiomes(null, x * 16 + 16, z * 16 + 16, 1, 1, false);
        return Biome.getIdForBiome(b[0]);
    }

    /** First square (rings around the origin) whose 16 population biomes are mostly in the class. */
    static Object[] find(Gen g, String name, int[] set) {
        Object[] best = null;
        int bestCount = 0;
        for (int d = 0; d < 320; d += 2) {
            for (int dz = -d; dz <= d; dz += 2) {
                boolean edge = dz == -d || dz == d;
                for (int dx = -d; dx <= d; dx += edge ? 2 : 2 * d) {
                    if (SeedDump.in(set, popBiome(g, dx + 1, dz + 1))) {
                        int count = 0;
                        for (int z = 0; z < SIDE; z++) {
                            for (int x = 0; x < SIDE; x++) if (SeedDump.in(set, popBiome(g, dx + x, dz + z))) count++;
                        }
                        if (count > bestCount) {
                            bestCount = count;
                            best = new Object[] {name, dx, dz, count};
                        }
                        if (count >= 12) return best;
                    }
                    if (d == 0) break;
                }
            }
        }
        return best;
    }

    /** Per state id (block id << 4 | meta): the block and material properties the population port reads. */
    static void props(File dir) throws Exception {
        dir.mkdirs();
        IdentityHashMap<Material, Integer> matIds = new IdentityHashMap<Material, Integer>();
        StringBuilder js = new StringBuilder("{\n  \"materials\": [");
        for (Field f : Material.class.getFields()) {
            if (!Modifier.isStatic(f.getModifiers()) || f.getType() != Material.class) continue;
            Material m = (Material) f.get(null);
            if (matIds.containsKey(m)) continue;
            js.append(matIds.isEmpty() ? "" : ", ").append(Harness.q(f.getName()));
            matIds.put(m, matIds.size());
        }
        js.append("],\n  \"sound_types\": [");
        IdentityHashMap<SoundType, Integer> soundIds = new IdentityHashMap<SoundType, Integer>();
        for (Field f : SoundType.class.getFields()) {
            if (!Modifier.isStatic(f.getModifiers()) || f.getType() != SoundType.class) continue;
            SoundType t = (SoundType) f.get(null);
            if (soundIds.containsKey(t)) continue;
            js.append(soundIds.isEmpty() ? "" : ", ").append(Harness.q(f.getName()));
            soundIds.put(t, soundIds.size());
        }
        js.append("],\n");
        HarnessWorld w = new HarnessWorld(0L);
        int n = 4096;
        int[] canonical = new int[n];
        int[] material = new int[n];
        int[] flags = new int[n];
        int[] opacity = new int[n];
        int[] light = new int[n];
        int[] faces = new int[n * 6];
        float[] hardness = new float[n];
        int[] sound = new int[n];
        float[] particleGravity = new float[n];
        for (int id = 0; id < n; id++) {
            // Block.getStateById decodes id | meta << 12; the fixtures number states id << 4 | meta.
            IBlockState s = Block.BLOCK_STATE_IDS.getByValue(id);
            if (s == null) {
                // A meta no state maps to: the block's default state stands in. Generation never stores these.
                s = Block.getBlockById(id >> 4).getDefaultState();
            }
            Block b = s.getBlock();
            canonical[id] = Block.BLOCK_STATE_IDS.get(s) == id && Block.getIdFromBlock(b) == id >> 4 ? 1 : 0;
            Material m = s.getMaterial();
            material[id] = matIds.containsKey(m) ? matIds.get(m) : -1;
            int fl = 0;
            if (m.isSolid()) fl |= 1;
            if (m.isLiquid()) fl |= 2;
            if (m.blocksMovement()) fl |= 4;
            if (m.isReplaceable()) fl |= 8;
            if (m.isOpaque()) fl |= 16;
            if (m.blocksLight()) fl |= 32;
            if (m.isToolNotRequired()) fl |= 64;
            if (s.isFullBlock()) fl |= 128;
            if (s.isFullCube()) fl |= 256;
            if (s.isOpaqueCube()) fl |= 512;
            if (s.isTopSolid()) fl |= 1024;
            if (s.isNormalCube()) fl |= 2048;
            if ((Boolean) Harness.get(Harness.field(Block.class, "fullBlock"), b)) fl |= 4096;
            if (b.requiresUpdates()) fl |= 8192;
            flags[id] = fl;
            opacity[id] = s.getLightOpacity();
            light[id] = s.getLightValue();
            SoundType st = b.getSoundType();
            sound[id] = soundIds.containsKey(st) ? soundIds.get(st) : -1;
            particleGravity[id] = b.blockParticleGravity;
            for (EnumFacing f : EnumFacing.values()) {
                int v;
                try {
                    BlockFaceShape shape = s.getBlockFaceShape(w, BlockPos.ORIGIN, f);
                    v = shape.ordinal();
                } catch (RuntimeException e) {
                    v = -1;
                }
                faces[id * 6 + f.getIndex()] = v;
            }
            try {
                hardness[id] = s.getBlockHardness(w, BlockPos.ORIGIN);
            } catch (RuntimeException e) {
                hardness[id] = Float.NaN;
            }
        }
        File bin = new File(dir, "props.bin.gz");
        FixtureWriter fx = new FixtureWriter(bin);
        fx.i32("canonical", canonical);
        fx.i32("material", material);
        fx.i32("flags", flags);
        fx.i32("opacity", opacity);
        fx.i32("light", light);
        fx.i32("faces", faces);
        fx.f32("hardness", hardness);
        fx.i32("sound", sound);
        fx.f32("particle_gravity", particleGravity);
        fx.close();
        js.append("  \"file\": ").append(Harness.q(bin.getName())).append(",\n  \"records\": ").append(fx.records()).append("\n}\n");
        Harness.writeText(new File(dir, "props.json"), js.toString());
        Harness.out.println("props: " + matIds.size() + " materials");
    }
}
