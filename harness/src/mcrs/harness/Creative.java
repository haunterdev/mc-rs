package mcrs.harness;

import java.io.File;
import java.util.Map;

import net.minecraft.block.Block;
import net.minecraft.creativetab.CreativeTabs;
import net.minecraft.enchantment.Enchantment;
import net.minecraft.enchantment.EnumEnchantmentType;
import net.minecraft.entity.EntityList;
import net.minecraft.item.Item;
import net.minecraft.item.ItemBanner;
import net.minecraft.item.ItemBlock;
import net.minecraft.item.ItemFishFood;
import net.minecraft.potion.Potion;
import net.minecraft.potion.PotionEffect;
import net.minecraft.potion.PotionType;
import net.minecraft.potion.PotionUtils;
import net.minecraft.util.ResourceLocation;

/**
 * The creative-menu data the server jar still carries: each item's creative tab (the
 * `getCreativeTab` overrides, read through the fields because the methods are client-only),
 * the potion types, the enchantments, the spawn eggs and the fish types. The sub-item rules
 * themselves (getSubItems, getSubBlocks) are stripped from the server jar and are ported by
 * hand in crates/items/src/creative.rs.
 *
 *   creative <outdir>   writes creative.json
 */
final class Creative {
    private Creative() {}

    static int tab(CreativeTabs t) {
        return t == null ? -1 : (Integer) Harness.get(Harness.field(CreativeTabs.class, "index"), t);
    }

    static void run(File dir) throws Exception {
        dir.mkdirs();
        StringBuilder js = new StringBuilder("{\n  \"items\": [");
        boolean first = true;
        for (Item item : Item.REGISTRY) {
            CreativeTabs t;
            if (item instanceof ItemBanner) {
                t = CreativeTabs.DECORATIONS;
            } else if (item instanceof ItemBlock) {
                Block b = ((ItemBlock) item).getBlock();
                t = (CreativeTabs) Harness.get(Harness.field(Block.class, "displayOnCreativeTab"), b);
            } else {
                t = (CreativeTabs) Harness.get(Harness.field(Item.class, "tabToDisplayOn"), item);
            }
            js.append(first ? "\n" : ",\n").append("    [").append(Item.getIdFromItem(item)).append(", ")
              .append(Harness.q(Item.REGISTRY.getNameForObject(item).toString())).append(", ").append(tab(t)).append("]");
            first = false;
        }
        js.append("\n  ],\n  \"tabs\": [");
        first = true;
        for (CreativeTabs t : CreativeTabs.CREATIVE_TAB_ARRAY) {
            if (t == null) continue;
            EnumEnchantmentType[] types = (EnumEnchantmentType[]) Harness.get(Harness.field(CreativeTabs.class, "enchantmentTypes"), t);
            js.append(first ? "\n" : ",\n").append("    [").append(tab(t)).append(", ")
              .append(Harness.q((String) Harness.get(Harness.field(CreativeTabs.class, "tabLabel"), t))).append(", [");
            for (int i = 0; i < types.length; i++) js.append(i == 0 ? "" : ", ").append(Harness.q(types[i].name()));
            js.append("]]");
            first = false;
        }
        js.append("\n  ],\n  \"potions\": [");
        first = true;
        for (PotionType p : PotionType.REGISTRY) {
            js.append(first ? "\n" : ",\n").append("    [").append(PotionType.REGISTRY.getIDForObject(p)).append(", ")
              .append(Harness.q(PotionType.REGISTRY.getNameForObject(p).toString())).append(", ")
              .append(PotionUtils.getPotionColorFromEffectList(p.getEffects())).append(", ")
              .append(Harness.q(p.getNamePrefixed(""))).append(", [");
            for (int i = 0; i < p.getEffects().size(); i++) {
                PotionEffect e = p.getEffects().get(i);
                js.append(i == 0 ? "[" : ", [").append(Harness.q(e.getPotion().getName())).append(", ")
                  .append(e.getAmplifier()).append(", ").append(e.getDuration()).append(", ")
                  .append((Boolean) Harness.get(Harness.field(Potion.class, "isBadEffect"), e.getPotion()) ? 1 : 0).append(", ").append(e.getPotion().isInstant() ? 1 : 0).append("]");
            }
            js.append("]]");
            first = false;
        }
        js.append("\n  ],\n  \"enchantments\": [");
        first = true;
        for (Enchantment e : Enchantment.REGISTRY) {
            js.append(first ? "\n" : ",\n").append("    [").append(Enchantment.getEnchantmentID(e)).append(", ")
              .append(Harness.q(Enchantment.REGISTRY.getNameForObject(e).toString())).append(", ")
              .append(Harness.q(e.getName())).append(", ")
              .append(e.type == null ? "null" : Harness.q(e.type.name())).append(", ")
              .append(e.getMinLevel()).append(", ").append(e.getMaxLevel()).append("]");
            first = false;
        }
        js.append("\n  ],\n  \"eggs\": [");
        first = true;
        for (Map.Entry<ResourceLocation, EntityList.EntityEggInfo> en : EntityList.ENTITY_EGGS.entrySet()) {
            EntityList.EntityEggInfo info = en.getValue();
            js.append(first ? "\n" : ",\n").append("    [").append(Harness.q(en.getKey().toString())).append(", ")
              .append(Harness.q(EntityList.getTranslationName(en.getKey()))).append(", ")
              .append(info.primaryColor).append(", ").append(info.secondaryColor).append("]");
            first = false;
        }
        js.append("\n  ],\n  \"fish\": [");
        first = true;
        for (ItemFishFood.FishType f : ItemFishFood.FishType.values()) {
            js.append(first ? "\n" : ",\n").append("    [").append(f.getMetadata()).append(", ")
              .append(Harness.q(f.name())).append(", ").append(f.canCook() ? 1 : 0).append("]");
            first = false;
        }
        js.append("\n  ]\n}\n");
        Harness.writeText(new File(dir, "creative.json"), js.toString());
        Harness.out.println("creative: written");
    }
}
