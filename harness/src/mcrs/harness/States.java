package mcrs.harness;

import java.io.File;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

import net.minecraft.block.Block;
import net.minecraft.block.BlockBeacon;
import net.minecraft.block.BlockBed;
import net.minecraft.block.BlockBrewingStand;
import net.minecraft.block.BlockBush;
import net.minecraft.block.BlockCactus;
import net.minecraft.block.BlockCake;
import net.minecraft.block.BlockChorusFlower;
import net.minecraft.block.BlockChorusPlant;
import net.minecraft.block.BlockCocoa;
import net.minecraft.block.BlockDoor;
import net.minecraft.block.BlockEndRod;
import net.minecraft.block.BlockFire;
import net.minecraft.block.BlockFlowerPot;
import net.minecraft.block.BlockGlass;
import net.minecraft.block.BlockGrass;
import net.minecraft.block.BlockHopper;
import net.minecraft.block.BlockIce;
import net.minecraft.block.BlockLadder;
import net.minecraft.block.BlockLeaves;
import net.minecraft.block.BlockLiquid;
import net.minecraft.block.BlockMobSpawner;
import net.minecraft.block.BlockPane;
import net.minecraft.block.BlockPortal;
import net.minecraft.block.BlockRailBase;
import net.minecraft.block.BlockRedstoneDiode;
import net.minecraft.block.BlockRedstoneWire;
import net.minecraft.block.BlockReed;
import net.minecraft.block.BlockSlime;
import net.minecraft.block.BlockStainedGlass;
import net.minecraft.block.BlockStainedGlassPane;
import net.minecraft.block.BlockStairs;
import net.minecraft.block.BlockTorch;
import net.minecraft.block.BlockTrapDoor;
import net.minecraft.block.BlockTripWire;
import net.minecraft.block.BlockTripWireHook;
import net.minecraft.block.BlockVine;
import net.minecraft.block.BlockWeb;
import net.minecraft.block.material.Material;
import net.minecraft.block.properties.IProperty;
import net.minecraft.block.state.IBlockState;

/**
 * What the client's model lookup needs per state id, from the server jar: each block's
 * registry name, the render layer, and per state `getRenderType` and the property map as
 * `StateMapperBase.getPropertyString` writes it (names sorted, as `BlockStateContainer` keeps
 * them). `Block.getRenderLayer` is client-only, so its overrides are mirrored here by class.
 *
 *   states <outdir>   writes states.json
 */
final class States {
    private States() {}

    static final Field STAIRS_MODEL = Harness.field(BlockStairs.class, "modelBlock");

    /** `Block.getRenderLayer` and its overrides: 0 SOLID, 1 CUTOUT_MIPPED, 2 CUTOUT, 3 TRANSLUCENT. */
    static int layer(Block b) {
        if (b instanceof BlockStairs) return layer((Block) Harness.get(STAIRS_MODEL, b));
        // BlockLeaves: CUTOUT_MIPPED with fancy graphics, which the viewer draws.
        // BlockStainedGlassPane extends BlockPane and overrides it.
        if (b instanceof BlockStainedGlassPane) return 3;
        if (b instanceof BlockLeaves || b instanceof BlockGrass || b instanceof BlockHopper
            || b instanceof BlockTripWireHook) return 1;
        if (b instanceof BlockPane) return 1;
        if (b instanceof BlockIce || b instanceof BlockSlime || b instanceof BlockPortal || b instanceof BlockStainedGlass
            || b instanceof BlockTripWire) return 3;
        if (b instanceof BlockLiquid) return b.getDefaultState().getMaterial() == Material.WATER ? 3 : 0;
        if (b instanceof BlockBush || b instanceof BlockCake || b instanceof BlockCocoa || b instanceof BlockBrewingStand
            || b instanceof BlockBeacon || b instanceof BlockChorusFlower || b instanceof BlockChorusPlant
            || b instanceof BlockCactus || b instanceof BlockBed || b instanceof BlockDoor || b instanceof BlockEndRod
            || b instanceof BlockFlowerPot || b instanceof BlockFire || b instanceof BlockGlass || b instanceof BlockLadder
            || b instanceof BlockMobSpawner || b instanceof BlockRedstoneDiode || b instanceof BlockRailBase
            || b instanceof BlockRedstoneWire || b instanceof BlockReed || b instanceof BlockTorch
            || b instanceof BlockTrapDoor || b instanceof BlockVine || b instanceof BlockWeb) return 2;
        return 0;
    }

    @SuppressWarnings({"rawtypes", "unchecked"})
    static String propertyString(IBlockState s) {
        List<String> parts = new ArrayList<String>();
        for (Map.Entry<IProperty<?>, Comparable<?>> e : s.getProperties().entrySet()) {
            IProperty p = e.getKey();
            parts.add(p.getName() + "=" + p.getName(e.getValue()));
        }
        Collections.sort(parts);
        StringBuilder sb = new StringBuilder();
        for (String part : parts) {
            if (sb.length() > 0) sb.append(',');
            sb.append(part);
        }
        return sb.toString();
    }

    static void run(File dir) throws Exception {
        dir.mkdirs();
        StringBuilder js = new StringBuilder("{\n  \"blocks\": [");
        boolean first = true;
        for (int id = 0; id < 256; id++) {
            Block b = Block.getBlockById(id);
            if (Block.getIdFromBlock(b) != id) continue;
            js.append(first ? "\n" : ",\n").append("    [").append(id).append(", ")
              .append(Harness.q(Block.REGISTRY.getNameForObject(b).getPath())).append(", ").append(layer(b)).append("]");
            first = false;
        }
        js.append("\n  ],\n  \"states\": [");
        first = true;
        for (int id = 0; id < 256; id++) {
            Block b = Block.getBlockById(id);
            if (Block.getIdFromBlock(b) != id) continue;
            for (int meta = 0; meta < 16; meta++) {
                IBlockState s;
                try {
                    s = b.getStateFromMeta(meta);
                } catch (RuntimeException e) {
                    continue;
                }
                js.append(first ? "\n" : ",\n").append("    [").append(id << 4 | meta).append(", ")
                  .append(Harness.q(s.getRenderType().name())).append(", ").append(Harness.q(propertyString(s))).append("]");
                first = false;
            }
        }
        js.append("\n  ]\n}\n");
        Harness.writeText(new File(dir, "states.json"), js.toString());
        Harness.out.println("wrote " + new File(dir, "states.json"));
    }
}
