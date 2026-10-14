package mcrs.harness;

import java.io.File;
import java.util.HashMap;
import java.util.Map;

import net.minecraft.block.Block;
import net.minecraft.block.material.Material;
import net.minecraft.block.state.IBlockState;
import net.minecraft.nbt.NBTTagCompound;
import net.minecraft.profiler.Profiler;
import net.minecraft.server.MinecraftServer;
import net.minecraft.util.ResourceLocation;
import net.minecraft.util.math.BlockPos;
import net.minecraft.world.EnumSkyBlock;
import net.minecraft.world.GameType;
import net.minecraft.world.World;
import net.minecraft.world.WorldProviderSurface;
import net.minecraft.world.WorldSettings;
import net.minecraft.world.WorldProvider;
import net.minecraft.world.WorldType;
import net.minecraft.world.chunk.Chunk;
import net.minecraft.world.chunk.IChunkProvider;
import net.minecraft.world.chunk.storage.IChunkLoader;
import net.minecraft.world.gen.IChunkGenerator;
import net.minecraft.world.gen.structure.template.Template;
import net.minecraft.world.gen.structure.template.TemplateManager;
import net.minecraft.world.storage.IPlayerFileData;
import net.minecraft.world.storage.ISaveHandler;
import net.minecraft.world.storage.WorldInfo;

/**
 * Bare overworld for the harness. Chunks come from a map that generates on first access; population
 * never cascades (the caller populates in a fixed order). A chunk counts as loaded once it exists, lighting
 * updates are off, and scheduled ticks run only in the WorldServer immediate branch. Each of these
 * is a deliberate divergence from a live server, listed in docs/DECOMP_NOTES.md.
 */
final class HarnessWorld extends World {
    private final Map<Long, Chunk> chunks = new HashMap<Long, Chunk>();
    private IChunkGenerator generator;
    /** Chunks generated after freeze(): population reached outside the pre-generated margin. */
    int lateGenerations;
    private boolean frozen;

    HarnessWorld(long seed) {
        super(null, new WorldInfo(new WorldSettings(seed, GameType.SURVIVAL, false, false, WorldType.DEFAULT), "mcrs"),
                new WorldProviderSurface(), new Profiler(), false);
        this.provider.setWorld(this);
        this.chunkProvider = new Provider();
        this.rand.setSeed(0L);
    }

    void useGenerator(IChunkGenerator g) {
        this.generator = g;
    }

    /** After this, any chunk generated on access is counted in lateGenerations. */
    void freeze() {
        this.frozen = true;
    }

    Chunk chunk(int x, int z) {
        return this.chunkProvider.provideChunk(x, z);
    }

    /** Only WorldGenFossils reaches the save handler. Its templates come back empty, so it draws its
     *  own Random as vanilla does and places nothing: the port drops fossils (Mojang structure assets). */
    @Override
    public ISaveHandler getSaveHandler() {
        return saves;
    }

    private final ISaveHandler saves = new ISaveHandler() {
        private final TemplateManager templates = new TemplateManager("", null) {
            @Override
            public Template get(MinecraftServer server, ResourceLocation id) {
                return null;
            }
        };

        @Override public WorldInfo loadWorldInfo() { return null; }
        @Override public void checkSessionLock() {}
        @Override public IChunkLoader getChunkLoader(WorldProvider p) { throw new UnsupportedOperationException(); }
        @Override public void saveWorldInfoWithPlayer(WorldInfo w, NBTTagCompound t) {}
        @Override public void saveWorldInfo(WorldInfo w) {}
        @Override public IPlayerFileData getPlayerNBTManager() { throw new UnsupportedOperationException(); }
        @Override public void flush() {}
        @Override public File getWorldDirectory() { throw new UnsupportedOperationException(); }
        @Override public File getMapFileFromName(String name) { throw new UnsupportedOperationException(); }
        @Override public TemplateManager getStructureTemplateManager() { return templates; }
    };

    @Override
    protected IChunkProvider createChunkProvider() {
        return null;
    }

    /** WorldServer: loaded means the chunk exists. A chunk mid-generation is not in the map yet, so
     *  MapGenCaves' getBiome falls through to the biome provider instead of recursing into it. */
    @Override
    protected boolean isChunkLoaded(int x, int z, boolean allowEmpty) {
        return chunks.containsKey(((long) x << 32) | (z & 0xffffffffL));
    }

    @Override
    public boolean checkLightFor(EnumSkyBlock lightType, BlockPos pos) {
        return false;
    }

    /** The `trace` mode's box (x0 y0 z0 x1 y1 z1, inclusive), or null. */
    static int[] traceBox;

    static boolean traced(BlockPos p) {
        int[] b = traceBox;
        return b != null && p.getX() >= b[0] && p.getY() >= b[1] && p.getZ() >= b[2] && p.getX() <= b[3] && p.getY() <= b[4] && p.getZ() <= b[5];
    }

    static String at(BlockPos p) {
        return p.getX() + " " + p.getY() + " " + p.getZ();
    }

    @Override
    public boolean setBlockState(BlockPos pos, IBlockState newState, int flags) {
        if (traced(pos)) {
            int s = Harness.stateId(newState);
            Harness.out.println("set " + at(pos) + " " + (s >> 4) + ":" + (s & 15) + " f" + flags + " imm " + this.scheduledUpdatesAreImmediate);
        }
        return super.setBlockState(pos, newState, flags);
    }

    /** WorldServer.scheduleUpdate: World's is empty, so without this nested liquid ticks never run. */
    @Override
    public void scheduleUpdate(BlockPos pos, Block blockIn, int delay) {
        this.updateBlockTick(pos, blockIn, delay, 0);
    }

    /** WorldServer.updateBlockTick, immediate branch only. */
    @Override
    public void updateBlockTick(BlockPos pos, Block blockIn, int delay, int priority) {
        if (traced(pos)) Harness.out.println("sched " + at(pos) + " " + Block.getIdFromBlock(blockIn) + " imm " + this.scheduledUpdatesAreImmediate);
        Material material = blockIn.getDefaultState().getMaterial();
        if (this.scheduledUpdatesAreImmediate && material != Material.AIR) {
            if (blockIn.requiresUpdates()) {
                if (this.isAreaLoaded(pos.add(-8, -8, -8), pos.add(8, 8, 8))) {
                    IBlockState iblockstate = this.getBlockState(pos);
                    if (iblockstate.getMaterial() != Material.AIR && iblockstate.getBlock() == blockIn) {
                        iblockstate.getBlock().updateTick(this, pos, iblockstate, this.rand);
                    }
                }
                return;
            }
        }
        // DROPPED: the pending tick list; nothing ticks after population in the harness.
    }

    private final class Provider implements IChunkProvider {
        @Override
        public Chunk getLoadedChunk(int x, int z) {
            return provideChunk(x, z);
        }

        @Override
        public Chunk provideChunk(int x, int z) {
            long k = ((long) x << 32) | (z & 0xffffffffL);
            Chunk c = chunks.get(k);
            if (c == null) {
                if (frozen) lateGenerations++;
                c = generator.generateChunk(x, z);
                chunks.put(k, c);
            }
            return c;
        }

        @Override
        public boolean tick() {
            return false;
        }

        @Override
        public String makeString() {
            return "mcrs";
        }

        @Override
        public boolean isChunkGeneratedAt(int x, int z) {
            return chunks.containsKey(((long) x << 32) | (z & 0xffffffffL));
        }
    }
}
