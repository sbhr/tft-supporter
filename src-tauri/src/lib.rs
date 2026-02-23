use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct TftData {
    #[serde(rename = "completedItems")]
    completed_items: Vec<CompletedItem>,
    comps: Vec<Comp>,
}

#[derive(Debug, Deserialize)]
struct CompletedItem {
    id: String,
    recipe: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Comp {
    id: String,
    name: String,
    description: String,
    #[serde(rename = "coreItems")]
    core_items: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RecommendRequest {
    #[serde(rename = "ownedItems")]
    owned_items: HashMap<String, u8>,
}

#[derive(Debug, Serialize)]
struct RecommendResponse {
    recommendations: Vec<Recommendation>,
}

#[derive(Debug, Serialize)]
struct Recommendation {
    comp_id: String,
    comp_name: String,
    description: String,
    crafted_count: usize,
    total_core_items: usize,
    missing_components: Vec<MissingComponent>,
    craftable_items: Vec<String>,
    score: i32,
}

#[derive(Debug, Serialize)]
struct MissingComponent {
    item_id: String,
    missing_count: u8,
}

#[derive(Clone)]
struct PlanResult {
    crafted_count: usize,
    missing: HashMap<String, u8>,
    crafted_item_ids: Vec<String>,
}

#[derive(Clone)]
struct CoreRecipe {
    item_id: String,
    recipe: [String; 2],
}

fn load_tft_data() -> Result<TftData, String> {
    let raw = include_str!("../../public/data/tft-data.json");
    serde_json::from_str(raw).map_err(|error| format!("failed to parse tft data: {error}"))
}

fn can_craft(recipe: &[String; 2], inventory: &HashMap<String, u8>) -> bool {
    let first = recipe[0].as_str();
    let second = recipe[1].as_str();

    if first == second {
        return inventory.get(first).copied().unwrap_or(0) >= 2;
    }

    inventory.get(first).copied().unwrap_or(0) >= 1 && inventory.get(second).copied().unwrap_or(0) >= 1
}

fn consume(recipe: &[String; 2], inventory: &mut HashMap<String, u8>) {
    for component in recipe {
        if let Some(count) = inventory.get_mut(component) {
            *count = count.saturating_sub(1);
        }
    }
}

fn missing_for_recipe(recipe: &[String; 2], inventory: &HashMap<String, u8>) -> HashMap<String, u8> {
    let mut required: HashMap<String, u8> = HashMap::new();
    for component in recipe {
        *required.entry(component.clone()).or_insert(0) += 1;
    }

    let mut missing = HashMap::new();
    for (component, needed) in required {
        let owned = inventory.get(&component).copied().unwrap_or(0);
        if owned < needed {
            missing.insert(component, needed - owned);
        }
    }

    missing
}

fn merge_missing(base: &HashMap<String, u8>, add: &HashMap<String, u8>) -> HashMap<String, u8> {
    let mut merged = base.clone();
    for (component, count) in add {
        *merged.entry(component.clone()).or_insert(0) += *count;
    }
    merged
}

fn total_missing(missing: &HashMap<String, u8>) -> u16 {
    missing.values().map(|count| *count as u16).sum()
}

fn pick_better(a: PlanResult, b: PlanResult) -> PlanResult {
    if a.crafted_count != b.crafted_count {
        return if a.crafted_count > b.crafted_count { a } else { b };
    }

    let missing_a = total_missing(&a.missing);
    let missing_b = total_missing(&b.missing);
    if missing_a != missing_b {
        return if missing_a < missing_b { a } else { b };
    }

    if a.crafted_item_ids.len() >= b.crafted_item_ids.len() {
        a
    } else {
        b
    }
}

fn search_best_plan(index: usize, recipes: &[CoreRecipe], inventory: HashMap<String, u8>) -> PlanResult {
    if index >= recipes.len() {
        return PlanResult {
            crafted_count: 0,
            missing: HashMap::new(),
            crafted_item_ids: Vec::new(),
        };
    }

    let current = &recipes[index];
    let next_skip = search_best_plan(index + 1, recipes, inventory.clone());
    let skip_missing = merge_missing(&next_skip.missing, &missing_for_recipe(&current.recipe, &inventory));
    let skip_plan = PlanResult {
        crafted_count: next_skip.crafted_count,
        missing: skip_missing,
        crafted_item_ids: next_skip.crafted_item_ids,
    };

    if !can_craft(&current.recipe, &inventory) {
        return skip_plan;
    }

    let mut consumed_inventory = inventory;
    consume(&current.recipe, &mut consumed_inventory);
    let mut crafted_plan = search_best_plan(index + 1, recipes, consumed_inventory);
    crafted_plan.crafted_count += 1;
    crafted_plan.crafted_item_ids.push(current.item_id.clone());

    pick_better(crafted_plan, skip_plan)
}

#[tauri::command]
fn recommend_comps(payload: RecommendRequest) -> Result<RecommendResponse, String> {
    let data = load_tft_data()?;
    let completed_lookup: HashMap<String, [String; 2]> = data
        .completed_items
        .iter()
        .filter_map(|item| {
            if item.recipe.len() != 2 {
                return None;
            }
            Some((item.id.clone(), [item.recipe[0].clone(), item.recipe[1].clone()]))
        })
        .collect();

    let mut recommendations = Vec::new();
    for comp in data.comps {
        let core_recipes: Vec<CoreRecipe> = comp
            .core_items
            .iter()
            .filter_map(|item_id| {
                completed_lookup.get(item_id).map(|recipe| CoreRecipe {
                    item_id: item_id.clone(),
                    recipe: recipe.clone(),
                })
            })
            .collect();

        let best_plan = search_best_plan(0, &core_recipes, payload.owned_items.clone());
        let mut missing_components: Vec<MissingComponent> = best_plan
            .missing
            .into_iter()
            .map(|(item_id, missing_count)| MissingComponent {
                item_id,
                missing_count,
            })
            .collect();
        missing_components.sort_by(|a, b| a.item_id.cmp(&b.item_id));

        let total_missing_count: i32 = missing_components
            .iter()
            .map(|component| component.missing_count as i32)
            .sum();

        recommendations.push(Recommendation {
            comp_id: comp.id,
            comp_name: comp.name,
            description: comp.description,
            crafted_count: best_plan.crafted_count,
            total_core_items: core_recipes.len(),
            missing_components,
            craftable_items: best_plan.crafted_item_ids,
            score: (best_plan.crafted_count as i32 * 100) - (total_missing_count * 10),
        });
    }

    recommendations.sort_by(|a, b| {
        b.crafted_count
            .cmp(&a.crafted_count)
            .then(a.missing_components.len().cmp(&b.missing_components.len()))
            .then(b.score.cmp(&a.score))
            .then(a.comp_name.cmp(&b.comp_name))
    });

    Ok(RecommendResponse {
        recommendations: recommendations.into_iter().take(5).collect(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![recommend_comps])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
