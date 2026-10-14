//! `CraftingManager.parseJsonRecipes` and the JSON half of `ShapedRecipes`/`ShapelessRecipes`
//! (`deserialize`, `deserializeKey`, `shrink`, `patternFromJson`, `deserializeIngredient`,
//! `deserializeItem`), reading the recipe files the caller takes from the client jar
//! (`assets/minecraft/recipes/*.json`). Error messages are Java's.

use serde_json::{Map, Value};

use crate::crafting::{Ingredient, Recipe, Recipes};
use crate::registry;
use crate::stack::ItemStack;

/// A recipe file that did not parse: `JsonParseException`/`JsonSyntaxException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeError {
    /// The recipe name (`ResourceLocation` path).
    pub name: String,
    pub message: String,
}

impl std::fmt::Display for RecipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parsing error loading recipe minecraft:{}: {}", self.name, self.message)
    }
}

impl std::error::Error for RecipeError {}

type R<T> = Result<T, String>;

/// The jar entry prefix of the recipe files.
pub const RECIPE_DIR: &str = "assets/minecraft/recipes/";

/// The recipe name of a jar entry (`assets/minecraft/recipes/oak_planks.json` is
/// `oak_planks`), `None` for anything else.
pub fn recipe_name(entry: &str) -> Option<&str> {
    let rel = entry.trim_start_matches('/').strip_prefix(RECIPE_DIR)?;
    let name = rel.strip_suffix(".json")?;
    (!name.is_empty()).then_some(name)
}

/// `JsonUtils.getString(json, key)`: any primitive, as `getAsString()` reads it.
fn get_string(json: &Map<String, Value>, key: &str) -> R<String> {
    match json.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        Some(Value::Bool(b)) => Ok(b.to_string()),
        Some(v) => Err(format!("Expected {key} to be a string, was {}", type_name(v))),
        None => Err(format!("Missing {key}, expected to find a string")),
    }
}

/// `JsonUtils.getInt(json, key, fallback)`
fn get_int(json: &Map<String, Value>, key: &str, fallback: i32) -> R<i32> {
    match json.get(key) {
        None => Ok(fallback),
        Some(Value::Number(n)) => {
            // JsonPrimitive.getAsInt(): Number.intValue(), truncating a decimal.
            if let Some(i) = n.as_i64() {
                Ok(i as i32)
            } else {
                Ok(n.as_f64().unwrap_or(0.0) as i32)
            }
        }
        Some(v) => Err(format!("Expected {key} to be a Int, was {}", type_name(v))),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// `JsonUtils.getJsonObject(element, name)`
fn as_object<'a>(v: &'a Value, name: &str) -> R<&'a Map<String, Value>> {
    v.as_object().ok_or_else(|| format!("Expected {name} to be a JsonObject, was {}", type_name(v)))
}

/// `ShapedRecipes.deserializeItem(json, useCount)`
fn deserialize_item(json: &Map<String, Value>, use_count: bool) -> R<ItemStack> {
    let s = get_string(json, "item")?;
    let d = registry::by_name(&s).ok_or_else(|| format!("Unknown item '{s}'"))?;
    if d.has_subtypes && !json.contains_key("data") {
        return Err(format!("Missing data for item '{s}'"));
    }
    let i = get_int(json, "data", 0)?;
    let j = if use_count { get_int(json, "count", 1)? } else { 1 };
    Ok(ItemStack::new(d.id, j, i))
}

/// `ShapedRecipes.deserializeIngredient(element)`
fn deserialize_ingredient(v: Option<&Value>) -> R<Ingredient> {
    match v {
        None | Some(Value::Null) => Err("Item cannot be null".into()),
        Some(Value::Object(o)) => Ok(Ingredient::from_stacks(vec![deserialize_item(o, false)?])),
        Some(Value::Array(a)) => {
            if a.is_empty() {
                return Err("Item array cannot be empty, at least one item must be defined".into());
            }
            let mut stacks = Vec::with_capacity(a.len());
            for e in a {
                stacks.push(deserialize_item(as_object(e, "item")?, false)?);
            }
            Ok(Ingredient::from_stacks(stacks))
        }
        Some(_) => Err("Expected item to be object or array of objects".into()),
    }
}

/// `ShapedRecipes.deserializeKey(json)`: one-character symbols; `" "` is `Ingredient.EMPTY`.
fn deserialize_key(json: &Map<String, Value>) -> R<Vec<(char, Ingredient)>> {
    let mut map = Vec::new();
    for (k, v) in json {
        let mut chars = k.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return Err(format!("Invalid key entry: '{k}' is an invalid symbol (must be 1 character only)."));
        };
        if c == ' ' {
            return Err("Invalid key entry: ' ' is a reserved symbol.".into());
        }
        map.push((c, deserialize_ingredient(Some(v))?));
    }
    map.push((' ', Ingredient::Empty));
    Ok(map)
}

/// `ShapedRecipes.patternFromJson(array)`
fn pattern_from_json(v: &Value) -> R<Vec<Vec<char>>> {
    let arr = v.as_array().ok_or_else(|| format!("Expected pattern to be a JsonArray, was {}", type_name(v)))?;
    if arr.len() > 3 {
        return Err("Invalid pattern: too many rows, 3 is maximum".into());
    }
    if arr.is_empty() {
        return Err("Invalid pattern: empty pattern not allowed".into());
    }
    let mut rows: Vec<Vec<char>> = Vec::new();
    for (i, e) in arr.iter().enumerate() {
        let s = e.as_str().ok_or_else(|| format!("Expected pattern[{i}] to be a string, was {}", type_name(e)))?;
        let s: Vec<char> = s.chars().collect();
        if s.len() > 3 {
            return Err("Invalid pattern: too many columns, 3 is maximum".into());
        }
        if i > 0 && rows[0].len() != s.len() {
            return Err("Invalid pattern: each row must be the same width".into());
        }
        rows.push(s);
    }
    Ok(rows)
}

/// `ShapedRecipes.shrink(rows)`: drops blank rows above and below and blank columns at either
/// side.
pub fn shrink(rows: &[Vec<char>]) -> Vec<Vec<char>> {
    let first_non_space = |s: &[char]| s.iter().position(|&c| c != ' ').unwrap_or(s.len()) as i32;
    let last_non_space = |s: &[char]| s.iter().rposition(|&c| c != ' ').map_or(-1, |p| p as i32);
    let mut i = i32::MAX;
    let mut j = 0;
    let mut k = 0usize;
    let mut l = 0usize;
    for (i1, s) in rows.iter().enumerate() {
        i = i.min(first_non_space(s));
        let j1 = last_non_space(s);
        j = j.max(j1);
        if j1 < 0 {
            if k == i1 {
                k += 1;
            }
            l += 1;
        } else {
            l = 0;
        }
    }
    if rows.len() == l {
        return Vec::new();
    }
    (0..rows.len() - l - k).map(|k1| rows[k1 + k][i as usize..=j as usize].to_vec()).collect()
}

/// `ShapedRecipes.deserialize(json)`
fn deserialize_shaped(json: &Map<String, Value>) -> R<Recipe> {
    let group = match json.get("group") {
        None => String::new(),
        Some(_) => get_string(json, "group")?,
    };
    let key = as_object(json.get("key").unwrap_or(&Value::Null), "key")?;
    let keys = deserialize_key(key)?;
    let pattern = shrink(&pattern_from_json(json.get("pattern").unwrap_or(&Value::Null))?);
    // astring[0].length(): an all-blank pattern shrinks to nothing and Java throws
    // ArrayIndexOutOfBoundsException here.
    let first = pattern.first().ok_or_else(|| "Invalid pattern: empty pattern not allowed".to_string())?;
    let width = first.len();
    let height = pattern.len();
    // ShapedRecipes.deserializeIngredients(pattern, keys, width, height)
    let mut items = vec![Ingredient::Empty; width * height];
    let mut unused: Vec<char> = keys.iter().map(|(c, _)| *c).filter(|&c| c != ' ').collect();
    for (i, row) in pattern.iter().enumerate() {
        for (j, c) in row.iter().enumerate() {
            let ing = keys
                .iter()
                .find(|(k, _)| k == c)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("Pattern references symbol '{c}' but it's not defined in the key"))?;
            unused.retain(|u| u != c);
            items[j + width * i] = ing;
        }
    }
    if !unused.is_empty() {
        let list: Vec<String> = unused.iter().map(|c| c.to_string()).collect();
        return Err(format!("Key defines symbols that aren't used in pattern: [{}]", list.join(", ")));
    }
    let output = deserialize_item(as_object(json.get("result").unwrap_or(&Value::Null), "result")?, true)?;
    Ok(Recipe::Shaped { group, width, height, items, output })
}

/// `ShapelessRecipes.deserialize(json)`
fn deserialize_shapeless(json: &Map<String, Value>) -> R<Recipe> {
    let group = match json.get("group") {
        None => String::new(),
        Some(_) => get_string(json, "group")?,
    };
    let arr = match json.get("ingredients") {
        Some(Value::Array(a)) => a,
        Some(v) => return Err(format!("Expected ingredients to be a JsonArray, was {}", type_name(v))),
        None => return Err("Missing ingredients, expected to find a JsonArray".into()),
    };
    let mut items = Vec::new();
    for e in arr {
        let ing = deserialize_ingredient(Some(e))?;
        if ing != Ingredient::Empty {
            items.push(ing);
        }
    }
    if items.is_empty() {
        return Err("No ingredients for shapeless recipe".into());
    }
    if items.len() > 9 {
        return Err("Too many ingredients for shapeless recipe".into());
    }
    let output = deserialize_item(as_object(json.get("result").unwrap_or(&Value::Null), "result")?, true)?;
    Ok(Recipe::Shapeless { group, items, output })
}

/// `CraftingManager.parseRecipeJson(json)` from a recipe file's bytes.
pub fn parse_recipe(bytes: &[u8]) -> R<Recipe> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let json = as_object(&v, "recipe")?;
    let s = get_string(json, "type")?;
    match s.as_str() {
        "crafting_shaped" => deserialize_shaped(json),
        "crafting_shapeless" => deserialize_shapeless(json),
        _ => Err(format!("Invalid or unsupported recipe type '{s}'")),
    }
}

impl Recipes {
    /// `CraftingManager.init`: the code recipes, then every `(name, bytes)` recipe file in name
    /// order. Java walks the jar's recipe folder in the zip file system's order, which nothing
    /// depends on: no two vanilla JSON recipes match the same grid. Stops at the first file that
    /// fails, as `parseJsonRecipes` does.
    pub fn from_files<N: AsRef<str>, B: AsRef<[u8]>>(files: impl IntoIterator<Item = (N, B)>) -> Result<Recipes, RecipeError> {
        let mut files: Vec<(N, B)> = files.into_iter().collect();
        files.sort_by(|a, b| a.0.as_ref().cmp(b.0.as_ref()));
        let mut r = Recipes::code_recipes();
        for (name, bytes) in files {
            let name = name.as_ref();
            let err = |message: String| RecipeError { name: name.to_string(), message };
            let path = registry::resource_path(name).ok_or_else(|| err("not a minecraft recipe".into()))?;
            let recipe = parse_recipe(bytes.as_ref()).map_err(err)?;
            r.register(&path, recipe).map_err(|m| RecipeError { name: path.clone(), message: m })?;
        }
        Ok(r)
    }

    /// Reads the recipes out of the client jar: `entries` lists the jar's entry names (any
    /// entries; the ones under `assets/minecraft/recipes/` ending in `.json` are used) and `read`
    /// returns an entry's bytes. A file `read` cannot return is skipped.
    pub fn from_jar<'a, F>(entries: impl IntoIterator<Item = &'a str>, mut read: F) -> Result<Recipes, RecipeError>
    where
        F: FnMut(&str) -> Option<Vec<u8>>,
    {
        let mut files = Vec::new();
        for e in entries {
            if let Some(name) = recipe_name(e) {
                if let Some(bytes) = read(e) {
                    files.push((name.to_string(), bytes));
                }
            }
        }
        Recipes::from_files(files)
    }
}
