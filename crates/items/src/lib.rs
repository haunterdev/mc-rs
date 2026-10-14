//! Minecraft 1.12.2 items as a pure data layer for survival: the item registry
//! (`Item.registerItems`), `ItemStack` and the inventory stacking rules, tools and harvesting
//! (`ItemTool` and friends, `EntityPlayer.getDigSpeed`), block drops
//! (`Block.dropBlockAsItemWithChance` and every override, on a `java.util.Random`), crafting
//! (`CraftingManager` with the jar's JSON recipes read at runtime), smelting and fuel
//! (`FurnaceRecipes`, `TileEntityFurnace`), food (`ItemFood`) and translation keys.
//!
//! Block states are `worldgen` state ids (`block id << 4 | meta`, see `worldgen::blocks`);
//! hardness and materials come from the generated `worldgen::populate::state` table. Every
//! ported function names its source class and method (MCP names). Nothing from the client jar is
//! embedded: recipes come in through [`Recipes::from_jar`] or [`Recipes::from_files`], names
//! through [`names::parse_lang`].

pub mod blocks_table;
pub mod crafting;
pub mod dig;
pub mod drop_rules;
pub mod drops;
pub mod food;
pub mod harvest;
pub mod ids;
pub mod inventory;
pub mod items_table;
pub mod names;
pub mod recipe_json;
pub mod registry;
pub mod smelting;
pub mod stack;
pub mod tools;

pub use crafting::{CraftingGrid, Ingredient, NamedRecipe, Recipe, Recipes};
pub use dig::DigContext;
pub use drops::{Drop, Drops};
pub use harvest::Harvest;
pub use inventory::PlayerInventory;
pub use recipe_json::RecipeError;
pub use registry::{ItemDef, Kind, Names};
pub use smelting::Furnace;
pub use stack::ItemStack;
pub use tools::{ArmorMaterial, ArmorSlot, ToolClass, ToolMaterial};
pub use worldgen::JavaRandom;
