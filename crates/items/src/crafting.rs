//! Crafting: `InventoryCrafting`, `Ingredient`, `ShapedRecipes` and `ShapelessRecipes` matching,
//! the code recipes that survive without NBT (`RecipeRepairItem`,
//! `ShulkerBoxRecipes.ShulkerBoxColoring`), `CraftingManager`'s first-match lookup and the grid
//! side of `SlotCrafting.onTake`. The JSON recipes are read from the client jar at runtime by
//! [`crate::recipe_json`]; nothing from them is embedded.
//!
//! DROPPED, with `CraftingManager.init`'s other code recipes: `RecipesArmorDyes` (the color
//! lives in the stack's NBT), `RecipeBookCloning` (copies the book's NBT pages),
//! `RecipesMapCloning` and `RecipesMapExtending` (need the world's `MapData`), `RecipeFireworks`
//! (builds NBT explosions), `RecipeTippedArrow` (reads the potion from NBT),
//! `RecipesBanners.RecipeDuplicatePattern`/`RecipeAddPattern` and `ShieldRecipes.Decoration`
//! (banner patterns are NBT). None of them can match a grid that a JSON recipe matches, so the
//! first-match order of the rest is unchanged.

use crate::ids::{block, item};
use crate::registry::{self, block_from_item};
use crate::stack::{are_item_stack_tags_equal, are_items_equal, ItemStack};

/// `ItemStack` metadata 32767 (`OreDictionary.WILDCARD_VALUE` in Forge): any metadata.
pub const WILDCARD: i32 = 32767;

/// `InventoryCrafting`: a `width` x `height` grid, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct CraftingGrid {
    pub width: usize,
    pub height: usize,
    pub slots: Vec<ItemStack>,
}

impl CraftingGrid {
    /// An empty grid: 2x2 in the player inventory, 3x3 at a crafting table.
    pub fn new(width: usize, height: usize) -> CraftingGrid {
        CraftingGrid { width, height, slots: vec![ItemStack::EMPTY; width * height] }
    }

    /// A grid from rows of stacks (shorter rows are padded with empty slots).
    pub fn from_rows(width: usize, rows: &[&[ItemStack]]) -> CraftingGrid {
        let mut g = CraftingGrid::new(width, rows.len());
        for (r, row) in rows.iter().enumerate() {
            for (c, s) in row.iter().enumerate() {
                g.slots[r * width + c] = *s;
            }
        }
        g
    }

    /// `getSizeInventory()`
    pub fn get_size_inventory(&self) -> usize {
        self.slots.len()
    }

    /// `getStackInSlot(index)`: empty past the end.
    pub fn get_stack_in_slot(&self, index: usize) -> ItemStack {
        self.slots.get(index).copied().unwrap_or(ItemStack::EMPTY)
    }

    /// `getStackInRowAndColumn(row, column)`, where `row` is the x index and `column` the y index.
    /// Java tests `column <= inventoryHeight`, one past the last row; that slot index is past
    /// the end and reads empty through `getStackInSlot`.
    pub fn get_stack_in_row_and_column(&self, row: i32, column: i32) -> ItemStack {
        if row >= 0 && row < self.width as i32 && column >= 0 && column <= self.height as i32 {
            self.get_stack_in_slot((row + column * self.width as i32) as usize)
        } else {
            ItemStack::EMPTY
        }
    }

    /// `setInventorySlotContents(index, stack)`
    pub fn set_inventory_slot_contents(&mut self, index: usize, stack: ItemStack) {
        if let Some(s) = self.slots.get_mut(index) {
            *s = stack;
        }
    }

    /// `decrStackSize(index, count)` (`ItemStackHelper.getAndSplit`)
    pub fn decr_stack_size(&mut self, index: usize, count: i32) -> ItemStack {
        match self.slots.get_mut(index) {
            Some(s) if !s.is_empty() && count > 0 => {
                let out = s.split_stack(count);
                if s.is_empty() {
                    *s = ItemStack::EMPTY;
                }
                out
            }
            _ => ItemStack::EMPTY,
        }
    }
}

/// `net.minecraft.item.crafting.Ingredient`
#[derive(Clone, Debug, PartialEq)]
pub enum Ingredient {
    /// `Ingredient.EMPTY`: matches only an empty slot.
    Empty,
    /// `matchingStacks`: any of these items, at their metadata or any with 32767.
    Stacks(Vec<ItemStack>),
}

impl Ingredient {
    /// `Ingredient.fromStacks(stacks)`: `EMPTY` unless some stack is non-empty.
    pub fn from_stacks(stacks: Vec<ItemStack>) -> Ingredient {
        if stacks.iter().any(|s| !s.is_empty()) {
            Ingredient::Stacks(stacks)
        } else {
            Ingredient::Empty
        }
    }

    /// `Ingredient.fromItem(item)`: any metadata.
    pub fn from_item(item: u16) -> Ingredient {
        Ingredient::from_stacks(vec![ItemStack::new(item, 1, WILDCARD)])
    }

    /// `apply(stack)`
    pub fn apply(&self, stack: &ItemStack) -> bool {
        match self {
            Ingredient::Empty => stack.is_empty(),
            Ingredient::Stacks(v) => v.iter().any(|s| {
                s.get_item() == stack.get_item() && (s.get_metadata() == WILDCARD || s.get_metadata() == stack.get_metadata())
            }),
        }
    }

    /// `getMatchingStacks()`
    pub fn matching_stacks(&self) -> &[ItemStack] {
        match self {
            Ingredient::Empty => &[],
            Ingredient::Stacks(v) => v,
        }
    }
}

/// One `IRecipe`.
#[derive(Clone, Debug, PartialEq)]
pub enum Recipe {
    /// `ShapedRecipes`: `items` is `width * height` row-major.
    Shaped { group: String, width: usize, height: usize, items: Vec<Ingredient>, output: ItemStack },
    /// `ShapelessRecipes`
    Shapeless { group: String, items: Vec<Ingredient>, output: ItemStack },
    /// `RecipeRepairItem`: two of the same damageable item merge with a 5% bonus.
    RepairItem,
    /// `ShulkerBoxRecipes.ShulkerBoxColoring`: a shulker box and a dye.
    ShulkerBoxColoring,
}

/// `IRecipe.getRemainingItems(inv)`: the container item (an empty bucket for a milk bucket)
/// left in each slot. Every 1.12.2 recipe class shares this body.
fn container_items(inv: &CraftingGrid) -> Vec<ItemStack> {
    inv.slots
        .iter()
        .map(|s| {
            let d = s.def();
            if d.has_container_item() {
                ItemStack::of(d.container)
            } else {
                ItemStack::EMPTY
            }
        })
        .collect()
}

/// `ShapedRecipes.checkMatch(inv, x, y, mirrored)`
fn shaped_check_match(inv: &CraftingGrid, w: usize, h: usize, items: &[Ingredient], x: i32, y: i32, mirrored: bool) -> bool {
    for i in 0..3 {
        for j in 0..3 {
            let k = i - x;
            let l = j - y;
            let empty = Ingredient::Empty;
            let mut ingredient = &empty;
            if k >= 0 && l >= 0 && k < w as i32 && l < h as i32 {
                ingredient = if mirrored {
                    &items[(w as i32 - k - 1 + l * w as i32) as usize]
                } else {
                    &items[(k + l * w as i32) as usize]
                };
            }
            if !ingredient.apply(&inv.get_stack_in_row_and_column(i, j)) {
                return false;
            }
        }
    }
    true
}

/// The stacks of a repair or coloring grid, Java's `list`, stopping at the first mismatch.
fn repair_list(inv: &CraftingGrid) -> Option<Vec<ItemStack>> {
    let mut list: Vec<ItemStack> = Vec::new();
    for s in &inv.slots {
        if !s.is_empty() {
            list.push(*s);
            if list.len() > 1 {
                let first = list[0];
                if s.get_item() != first.get_item()
                    || first.get_count() != 1
                    || s.get_count() != 1
                    || !first.def().is_damageable()
                {
                    return None;
                }
            }
        }
    }
    Some(list)
}

fn is_shulker_box_item(it: u16) -> bool {
    (block::WHITE_SHULKER_BOX..=block::BLACK_SHULKER_BOX).contains(&block_from_item(it))
}

impl Recipe {
    /// `IRecipe.matches(inv, world)`
    pub fn matches(&self, inv: &CraftingGrid) -> bool {
        match self {
            // ShapedRecipes.matches: every offset, mirrored first
            Recipe::Shaped { width, height, items, .. } => {
                for i in 0..=(3 - *width as i32) {
                    for j in 0..=(3 - *height as i32) {
                        if shaped_check_match(inv, *width, *height, items, i, j, true) {
                            return true;
                        }
                        if shaped_check_match(inv, *width, *height, items, i, j, false) {
                            return true;
                        }
                    }
                }
                false
            }
            // ShapelessRecipes.matches: each stack takes the first ingredient left that accepts it
            Recipe::Shapeless { items, .. } => {
                let mut list: Vec<&Ingredient> = items.iter().collect();
                for i in 0..inv.height as i32 {
                    for j in 0..inv.width as i32 {
                        let s = inv.get_stack_in_row_and_column(j, i);
                        if !s.is_empty() {
                            match list.iter().position(|ing| ing.apply(&s)) {
                                Some(p) => {
                                    list.remove(p);
                                }
                                None => return false,
                            }
                        }
                    }
                }
                list.is_empty()
            }
            // RecipeRepairItem.matches
            Recipe::RepairItem => repair_list(inv).is_some_and(|l| l.len() == 2),
            // ShulkerBoxColoring.matches
            Recipe::ShulkerBoxColoring => {
                let (mut i, mut j) = (0, 0);
                for s in &inv.slots {
                    if !s.is_empty() {
                        if is_shulker_box_item(s.get_item()) {
                            i += 1;
                        } else {
                            if s.get_item() != item::DYE {
                                return false;
                            }
                            j += 1;
                        }
                        if j > 1 || i > 1 {
                            return false;
                        }
                    }
                }
                i == 1 && j == 1
            }
        }
    }

    /// `IRecipe.getCraftingResult(inv)`
    pub fn get_crafting_result(&self, inv: &CraftingGrid) -> ItemStack {
        match self {
            Recipe::Shaped { output, .. } | Recipe::Shapeless { output, .. } => output.copy(),
            // RecipeRepairItem.getCraftingResult
            Recipe::RepairItem => {
                let Some(list) = repair_list(inv) else { return ItemStack::EMPTY };
                if list.len() == 2 {
                    let (a, b) = (list[0], list[1]);
                    if a.get_item() == b.get_item() && a.get_count() == 1 && b.get_count() == 1 && a.def().is_damageable() {
                        let max = a.def().max_damage;
                        let j = max - a.damage;
                        let k = max - b.damage;
                        let l = j + k + max * 5 / 100;
                        let i1 = (max - l).max(0);
                        // DROPPED: Java builds the result without the inputs' enchantments too.
                        return ItemStack::new(a.get_item(), 1, i1);
                    }
                }
                ItemStack::EMPTY
            }
            // ShulkerBoxColoring.getCraftingResult: BlockShulkerBox.getColoredItemStack(
            // EnumDyeColor.byDyeDamage(dye meta)). DROPPED: copying the box's NBT contents.
            Recipe::ShulkerBoxColoring => {
                let mut dye = ItemStack::EMPTY;
                for s in &inv.slots {
                    if !s.is_empty() && !is_shulker_box_item(s.get_item()) && s.get_item() == item::DYE {
                        dye = *s;
                    }
                }
                let dmg = dye.get_metadata();
                let dmg = if (0..16).contains(&dmg) { dmg } else { 0 };
                // byDyeDamage(d) is the color with metadata 15 - d.
                ItemStack::of(block::WHITE_SHULKER_BOX + (15 - dmg) as u16)
            }
        }
    }

    /// `IRecipe.getRecipeOutput()`: empty for the code recipes.
    pub fn get_recipe_output(&self) -> ItemStack {
        match self {
            Recipe::Shaped { output, .. } | Recipe::Shapeless { output, .. } => *output,
            _ => ItemStack::EMPTY,
        }
    }

    /// `IRecipe.getRemainingItems(inv)`
    pub fn get_remaining_items(&self, inv: &CraftingGrid) -> Vec<ItemStack> {
        container_items(inv)
    }

    /// `IRecipe.canFit(width, height)`
    pub fn can_fit(&self, width: usize, height: usize) -> bool {
        match self {
            Recipe::Shaped { width: w, height: h, .. } => width >= *w && height >= *h,
            Recipe::Shapeless { items, .. } => width * height >= items.len(),
            Recipe::RepairItem | Recipe::ShulkerBoxColoring => width * height >= 2,
        }
    }

    /// `IRecipe.isDynamic()`: hidden from the recipe book.
    pub fn is_dynamic(&self) -> bool {
        matches!(self, Recipe::RepairItem | Recipe::ShulkerBoxColoring)
    }

    /// `IRecipe.getGroup()`
    pub fn group(&self) -> &str {
        match self {
            Recipe::Shaped { group, .. } | Recipe::Shapeless { group, .. } => group,
            _ => "",
        }
    }

    /// `IRecipe.getIngredients()`
    pub fn ingredients(&self) -> &[Ingredient] {
        match self {
            Recipe::Shaped { items, .. } | Recipe::Shapeless { items, .. } => items,
            _ => &[],
        }
    }
}

/// A recipe with its `CraftingManager.REGISTRY` name and id.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedRecipe {
    /// `ResourceLocation` path: `"repairitem"`, or the JSON file name (`"oak_planks"`).
    pub name: String,
    /// `CraftingManager.getIDForRecipe`
    pub id: usize,
    pub recipe: Recipe,
}

/// `CraftingManager.REGISTRY`: the code recipes in `init` order, then the JSON recipes.
#[derive(Clone, Debug, Default)]
pub struct Recipes {
    pub(crate) list: Vec<NamedRecipe>,
}

impl Recipes {
    /// The code recipes `CraftingManager.init` registers before the JSON ones, with their ids
    /// counting every registration (the dropped ones included), so JSON recipes keep vanilla ids.
    pub fn code_recipes() -> Recipes {
        let mut r = Recipes::default();
        // armordye 0, bookcloning 1, mapcloning 2, mapextending 3, fireworks 4: DROPPED
        r.list.push(NamedRecipe { name: "repairitem".into(), id: 5, recipe: Recipe::RepairItem });
        // tippedarrow 6, bannerduplicate 7, banneraddpattern 8, shielddecoration 9: DROPPED
        r.list.push(NamedRecipe { name: "shulkerboxcoloring".into(), id: 10, recipe: Recipe::ShulkerBoxColoring });
        r
    }

    /// The number of ids `CraftingManager.init` hands out before the JSON recipes.
    pub const CODE_RECIPE_IDS: usize = 11;

    /// `CraftingManager.register(name, recipe)`: fails on a duplicate name, as Java throws.
    pub fn register(&mut self, name: &str, recipe: Recipe) -> Result<(), String> {
        if self.list.iter().any(|r| r.name == name) {
            return Err(format!("Duplicate recipe ignored with ID {name}"));
        }
        let id = self.list.last().map_or(0, |r| r.id + 1).max(Self::CODE_RECIPE_IDS);
        self.list.push(NamedRecipe { name: name.to_string(), id, recipe });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &NamedRecipe> {
        self.list.iter()
    }

    /// `CraftingManager.getRecipe(name)`
    pub fn get(&self, name: &str) -> Option<&NamedRecipe> {
        let p = registry::resource_path(name)?;
        self.list.iter().find(|r| r.name == p)
    }

    /// `CraftingManager.getRecipeById(id)`
    pub fn by_id(&self, id: usize) -> Option<&NamedRecipe> {
        self.list.iter().find(|r| r.id == id)
    }

    /// `CraftingManager.findMatchingRecipe(grid, world)`: the first match in registry order.
    pub fn find_matching_recipe(&self, grid: &CraftingGrid) -> Option<&NamedRecipe> {
        self.list.iter().find(|r| r.recipe.matches(grid))
    }

    /// `CraftingManager.findMatchingResult(grid, world)`
    pub fn find_matching_result(&self, grid: &CraftingGrid) -> ItemStack {
        self.find_matching_recipe(grid).map_or(ItemStack::EMPTY, |r| r.recipe.get_crafting_result(grid))
    }

    /// `CraftingManager.getRemainingItems(grid, world)`: the match's container items, or a copy
    /// of the grid when nothing matches.
    pub fn get_remaining_items(&self, grid: &CraftingGrid) -> Vec<ItemStack> {
        match self.find_matching_recipe(grid) {
            Some(r) => r.recipe.get_remaining_items(grid),
            None => grid.slots.clone(),
        }
    }

    /// The grid half of `SlotCrafting.onTake`: takes one of every input, puts container items
    /// back (merging with what is left in the slot), and returns the container items that did
    /// not fit, which Java gives to `addItemStackToInventory` and drops if that fails. Call it
    /// after handing out [`Recipes::find_matching_result`].
    pub fn consume(&self, grid: &mut CraftingGrid) -> Vec<ItemStack> {
        let remaining = self.get_remaining_items(grid);
        let mut overflow = Vec::new();
        for (i, mut itemstack1) in remaining.into_iter().enumerate() {
            let mut itemstack = grid.get_stack_in_slot(i);
            if !itemstack.is_empty() {
                grid.decr_stack_size(i, 1);
                itemstack = grid.get_stack_in_slot(i);
            }
            if !itemstack1.is_empty() {
                if itemstack.is_empty() {
                    grid.set_inventory_slot_contents(i, itemstack1);
                } else if are_items_equal(&itemstack, &itemstack1) && are_item_stack_tags_equal(&itemstack, &itemstack1) {
                    itemstack1.grow(itemstack.get_count());
                    grid.set_inventory_slot_contents(i, itemstack1);
                } else {
                    overflow.push(itemstack1);
                }
            }
        }
        overflow
    }
}
