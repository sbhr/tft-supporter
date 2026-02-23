use serde::{Deserialize, Serialize};
use scraper::{Html, Selector};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

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

#[derive(Debug, Deserialize, Serialize, Clone)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MissingComponent {
    item_id: String,
    missing_count: u8,
}

#[derive(Debug, Serialize)]
struct MetaTftDeckTierResponse {
    source_url: String,
    entries: Vec<MetaTftDeckTierEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MetaTftDeckTierEntry {
    deck_summary: String,
    tier: String,
    avg_placement: String,
    top4_rate: String,
    games: String,
    unit_names: Vec<String>,
    item_names: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MetaTftDeckAnalysisResponse {
    source_url: String,
    entries: Vec<MetaTftDeckAnalysisEntry>,
    recommendations: Vec<MetaTftDeckRecommendation>,
    cache_file_path: String,
    loaded_from_cache: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MetaTftDeckAnalysisEntry {
    comp_name: String,
    tier: String,
    avg_placement: String,
    top4_rate: String,
    games: String,
    ace_champion: String,
    champions: Vec<String>,
    mandatory_items: Vec<String>,
    priority_items: Vec<String>,
    style: String,
    craftable_target_items: usize,
    missing_components: Vec<MissingComponent>,
    fit_score: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MetaTftDeckRecommendation {
    comp_name: String,
    tier: String,
    style: String,
    ace_champion: String,
    mandatory_items: Vec<String>,
    priority_items: Vec<String>,
    craftable_target_items: usize,
    missing_components: Vec<MissingComponent>,
    fit_score: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedMetaTftDeckAnalysis {
    saved_at_unix: u64,
    source_url: String,
    entries: Vec<MetaTftDeckAnalysisEntry>,
    recommendations: Vec<MetaTftDeckRecommendation>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GeneratedCompFile {
    generated_from: String,
    updated_at_unix: u64,
    comps: Vec<Comp>,
}

#[derive(Debug, Serialize)]
struct ExportLocalCompsResponse {
    file_path: String,
    exported_count: usize,
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

#[derive(Clone)]
struct MatchPhrase {
    words: Vec<String>,
    display: String,
}

#[derive(Clone)]
struct ParsedDeck {
    comp_name: String,
    champions: Vec<String>,
    opening_items: Vec<String>,
    all_items: Vec<String>,
}

#[derive(Clone)]
struct DeckItemPlan {
    craftable_count: usize,
    missing_components: HashMap<String, u8>,
}

const META_CACHE_FILE_NAME: &str = "meta-deck-analysis.json";
const GENERATED_COMPS_FILE_NAME: &str = "generated-recommended-comps.json";
const META_DECK_SOURCE_URL: &str = "https://meta-tft.com/en/decks/?period=7&tiers=EMERALD&tiers=DIAMOND&tiers=MASTER&tiers=GRANDMASTER&tiers=CHALLENGER";

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

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_camel_case(token: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    for ch in token.chars() {
        if ch.is_ascii_uppercase() && !current.is_empty() {
            words.push(current.clone());
            current.clear();
        }
        current.push(ch);
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

fn derive_name_from_href(href: &str) -> Option<String> {
    let path = href.split('?').next().unwrap_or_default().trim_end_matches('/');
    let segment = path.rsplit('/').next().unwrap_or_default();
    if segment.is_empty() {
        return None;
    }

    let mut parts: Vec<String> = segment
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect();

    if let Some(first) = parts.first() {
        let first_upper = first.to_ascii_uppercase();
        if first_upper.starts_with("TFT") {
            parts.remove(0);
        }
    }

    if let Some(first) = parts.first() {
        if first.eq_ignore_ascii_case("item") {
            parts.remove(0);
        }
    }

    if parts.is_empty() {
        return None;
    }

    let words = parts
        .into_iter()
        .flat_map(|part| split_camel_case(&part))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();

    if words.is_empty() {
        None
    } else {
        Some(normalize_text(&words.join(" ")))
    }
}

fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let key = canonical_key(&value);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        deduped.push(value);
    }
    deduped
}

fn cache_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app_data_dir: {error}"))?;
    Ok(base_dir.join(META_CACHE_FILE_NAME))
}

fn generated_comps_file_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app_data_dir: {error}"))?;
    Ok(base_dir.join(GENERATED_COMPS_FILE_NAME))
}

fn resolve_generated_comps_file_path(
    app: &tauri::AppHandle,
    target_path: Option<String>,
) -> Result<PathBuf, String> {
    if let Some(custom_path) = target_path {
        let trimmed = custom_path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    generated_comps_file_path(app)
}

fn save_analysis_cache(app: &tauri::AppHandle, response: &MetaTftDeckAnalysisResponse) -> Result<String, String> {
    let cache_path = cache_file_path(app)?;
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create cache directory: {error}"))?;
    }

    let saved_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to get current time: {error}"))?
        .as_secs();

    let payload = SavedMetaTftDeckAnalysis {
        saved_at_unix,
        source_url: response.source_url.clone(),
        entries: response.entries.clone(),
        recommendations: response.recommendations.clone(),
    };

    let serialized = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("failed to serialize cache payload: {error}"))?;

    fs::write(&cache_path, serialized).map_err(|error| format!("failed to write cache file: {error}"))?;
    Ok(cache_path.to_string_lossy().to_string())
}

fn load_analysis_cache(app: &tauri::AppHandle) -> Result<MetaTftDeckAnalysisResponse, String> {
    let cache_path = cache_file_path(app)?;
    if !cache_path.exists() {
        return Err("saved meta deck cache file was not found".to_string());
    }

    let raw = fs::read_to_string(&cache_path)
        .map_err(|error| format!("failed to read cache file: {error}"))?;
    let saved: SavedMetaTftDeckAnalysis = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse cache file: {error}"))?;

    Ok(MetaTftDeckAnalysisResponse {
        source_url: saved.source_url,
        entries: saved.entries,
        recommendations: saved.recommendations,
        cache_file_path: cache_path.to_string_lossy().to_string(),
        loaded_from_cache: true,
    })
}

fn load_generated_comps(app: &tauri::AppHandle) -> Result<Vec<Comp>, String> {
    let file_path = generated_comps_file_path(app)?;
    if !file_path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&file_path)
        .map_err(|error| format!("failed to read generated comps file: {error}"))?;
    let parsed: GeneratedCompFile = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse generated comps file: {error}"))?;
    Ok(parsed.comps)
}

fn completed_item_name_to_id_map(data: &TftData) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in &data.completed_items {
        let display_name = title_from_item_id(&item.id);
        let id = item.id.clone();
        map.insert(canonical_key(&display_name), id.clone());
        map.insert(compact_key(&display_name), id.clone());

        let id_words = item.id.replace('_', " ");
        map.insert(canonical_key(&id_words), id.clone());
        map.insert(compact_key(&id_words), id.clone());

        let display_without_of = display_name.replace(" Of ", " ");
        map.insert(canonical_key(&display_without_of), id.clone());
        map.insert(compact_key(&display_without_of), id);
    }

    for (component_id, component_name) in component_name_map() {
        map.insert(canonical_key(&component_name), component_id.clone());
        map.insert(compact_key(&component_name), component_id);
    }
    map
}

fn resolve_item_id(item_name_to_id: &HashMap<String, String>, item_name: &str) -> Option<String> {
    item_name_to_id
        .get(&canonical_key(item_name))
        .or_else(|| item_name_to_id.get(&compact_key(item_name)))
        .cloned()
}

fn comp_slug(input: &str) -> String {
    canonical_key(input).replace(' ', "_")
}

fn generate_comps_from_analysis(
    entries: &[MetaTftDeckAnalysisEntry],
    item_name_to_id: &HashMap<String, String>,
) -> Vec<Comp> {
    let mut generated = Vec::new();

    for (index, entry) in entries.iter().take(20).enumerate() {
        let mut core_items = Vec::new();
        for item_name in entry
            .mandatory_items
            .iter()
            .chain(entry.priority_items.iter().take(6))
        {
            if let Some(item_id) = resolve_item_id(item_name_to_id, item_name) {
                core_items.push(item_id);
            }
        }

        core_items = dedupe_preserve_order(core_items);
        if core_items.is_empty() {
            continue;
        }

        generated.push(Comp {
            id: format!("meta_generated_{}_{}", index + 1, comp_slug(&entry.comp_name)),
            name: format!("[Meta] {}", entry.comp_name),
            description: format!(
                "Tier {} / {} / Ace {} / Fit {}",
                entry.tier, entry.style, entry.ace_champion, entry.fit_score
            ),
            core_items,
        });
    }

    generated
}

fn merge_base_and_generated_comps(base: Vec<Comp>, generated: Vec<Comp>) -> Vec<Comp> {
    let mut merged = base;
    merged.extend(generated);
    merged
}

fn canonical_key(value: &str) -> String {
    let mut normalized = String::new();
    let mut prev_space = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            normalized.push(lower);
            prev_space = false;
        } else if !prev_space {
            normalized.push(' ');
            prev_space = true;
        }
    }
    normalized
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn title_from_item_id(item_id: &str) -> String {
    item_id
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn component_name_map() -> HashMap<String, String> {
    HashMap::from([
        ("bf_sword".to_string(), "B.F. Sword".to_string()),
        ("recurve_bow".to_string(), "Recurve Bow".to_string()),
        (
            "needlessly_large_rod".to_string(),
            "Needlessly Large Rod".to_string(),
        ),
        (
            "tear_of_the_goddess".to_string(),
            "Tear of the Goddess".to_string(),
        ),
        ("chain_vest".to_string(), "Chain Vest".to_string()),
        ("negatron_cloak".to_string(), "Negatron Cloak".to_string()),
        ("giants_belt".to_string(), "Giant's Belt".to_string()),
        ("sparring_gloves".to_string(), "Sparring Gloves".to_string()),
        ("spatula".to_string(), "Spatula".to_string()),
        ("frying_pan".to_string(), "Frying Pan".to_string()),
    ])
}

fn build_item_recipes(data: &TftData) -> HashMap<String, [String; 2]> {
    let component_map = component_name_map();
    let mut recipes: HashMap<String, [String; 2]> = HashMap::new();

    for item in &data.completed_items {
        if item.recipe.len() != 2 {
            continue;
        }
        let display_name = title_from_item_id(&item.id);
        recipes.insert(
            canonical_key(&display_name),
            [item.recipe[0].clone(), item.recipe[1].clone()],
        );
    }

    for name in component_map.values() {
        recipes.entry(canonical_key(name)).or_insert(["".to_string(), "".to_string()]);
    }

    recipes
}

fn build_item_phrases(data: &TftData) -> Vec<MatchPhrase> {
    let mut seen = HashSet::new();
    let mut phrases = Vec::new();

    for item in &data.completed_items {
        let display = title_from_item_id(&item.id);
        let key = canonical_key(&display);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        let words = display
            .split_whitespace()
            .map(|word| canonical_key(word))
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>();
        if !words.is_empty() {
            phrases.push(MatchPhrase { words, display });
        }
    }

    for name in component_name_map().into_values() {
        let key = canonical_key(&name);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        let words = name
            .split_whitespace()
            .map(|word| canonical_key(word))
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>();
        if !words.is_empty() {
            phrases.push(MatchPhrase {
                words,
                display: name,
            });
        }
    }

    phrases.sort_by(|a, b| b.words.len().cmp(&a.words.len()));
    phrases
}

fn split_words(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(canonical_key)
        .filter(|word| !word.is_empty())
        .collect()
}

fn parse_deck_name(summary: &str) -> String {
    let words: Vec<&str> = summary.split_whitespace().collect();
    if words.is_empty() {
        return "不明構成".to_string();
    }
    words.iter().take(2).copied().collect::<Vec<_>>().join(" ")
}

fn starts_with_words(tokens: &[String], index: usize, phrase_words: &[String]) -> bool {
    if index + phrase_words.len() > tokens.len() {
        return false;
    }
    tokens[index..index + phrase_words.len()] == *phrase_words
}

fn detect_champion_ranges(tokens: &[String], item_keys: &HashSet<String>) -> Vec<(usize, usize, String)> {
    let mut champions = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let mut found: Option<(usize, usize, String)> = None;
        for phrase_len in (1..=4).rev() {
            if index + phrase_len * 2 > tokens.len() {
                continue;
            }
            let first = &tokens[index..index + phrase_len];
            let second = &tokens[index + phrase_len..index + phrase_len * 2];
            if first != second {
                continue;
            }
            let joined = first.join(" ");
            if item_keys.contains(&joined) {
                continue;
            }
            found = Some((index, index + phrase_len * 2, joined));
            break;
        }

        if let Some((start, end, name_key)) = found {
            let champion_name = name_key
                .split_whitespace()
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => {
                            format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            champions.push((start, end, champion_name));
            index = end;
        } else {
            index += 1;
        }
    }

    champions
}

fn parse_items_in_segment(
    tokens: &[String],
    item_phrases: &[MatchPhrase],
    start: usize,
    end: usize,
) -> Vec<String> {
    let mut items = Vec::new();
    let mut index = start;

    while index < end {
        let mut matched = false;
        for phrase in item_phrases {
            if starts_with_words(tokens, index, &phrase.words) && index + phrase.words.len() <= end {
                items.push(phrase.display.clone());
                index += phrase.words.len();
                matched = true;
                break;
            }
        }
        if !matched {
            index += 1;
        }
    }

    items
}

fn parse_deck_summary(
    summary: &str,
    item_phrases: &[MatchPhrase],
    item_keys: &HashSet<String>,
    linked_units: &[String],
    linked_items: &[String],
) -> ParsedDeck {
    let comp_name = parse_deck_name(summary);
    let words: Vec<&str> = summary.split_whitespace().collect();
    let tail = if words.len() > 2 {
        words[2..].join(" ")
    } else {
        String::new()
    };
    let tokens = split_words(&tail);
    let champion_ranges = detect_champion_ranges(&tokens, item_keys);

    let detected_champions = champion_ranges
        .iter()
        .map(|(_, _, name)| name.clone())
        .collect::<Vec<_>>();

    let champions = if linked_units.is_empty() {
        detected_champions
    } else {
        dedupe_preserve_order(linked_units.to_vec())
    };

    let mut all_items = Vec::new();
    let detected_opening_items = if let Some((first_start, _, _)) = champion_ranges.first() {
        parse_items_in_segment(&tokens, item_phrases, 0, *first_start)
    } else {
        parse_items_in_segment(&tokens, item_phrases, 0, tokens.len())
    };
    all_items.extend(detected_opening_items.clone());

    for range_index in 0..champion_ranges.len() {
        let (_, range_end, _) = champion_ranges[range_index];
        let next_start = champion_ranges
            .get(range_index + 1)
            .map(|(start, _, _)| *start)
            .unwrap_or(tokens.len());
        let segment_items = parse_items_in_segment(&tokens, item_phrases, range_end, next_start);
        all_items.extend(segment_items);
    }

    if !linked_items.is_empty() {
        let linked = dedupe_preserve_order(linked_items.to_vec());
        let opening_items = linked.iter().take(3).cloned().collect::<Vec<_>>();
        return ParsedDeck {
            comp_name,
            champions,
            opening_items,
            all_items: linked,
        };
    }

    ParsedDeck {
        comp_name,
        champions,
        opening_items: detected_opening_items,
        all_items,
    }
}

fn infer_ace_champion(champions: &[String]) -> String {
    if champions.is_empty() {
        return "不明".to_string();
    }
    let mut freq: HashMap<String, usize> = HashMap::new();
    for champion in champions {
        *freq.entry(champion.clone()).or_insert(0) += 1;
    }
    freq.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)))
        .map(|(name, _)| name)
        .unwrap_or_else(|| champions[0].clone())
}

fn infer_mandatory_and_priority_items(
    opening_items: &[String],
    all_items: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut freq: HashMap<String, usize> = HashMap::new();
    for item in all_items {
        *freq.entry(item.clone()).or_insert(0) += 1;
    }

    let mut mandatory_set: HashSet<String> = HashSet::new();
    for item in opening_items.iter().take(3) {
        mandatory_set.insert(item.clone());
    }
    for (item, count) in &freq {
        if *count >= 2 {
            mandatory_set.insert(item.clone());
        }
    }

    let mut mandatory = mandatory_set.into_iter().collect::<Vec<_>>();
    mandatory.sort_by(|a, b| {
        let left = freq.get(a).copied().unwrap_or(0);
        let right = freq.get(b).copied().unwrap_or(0);
        right.cmp(&left).then_with(|| a.cmp(b))
    });
    if mandatory.len() > 6 {
        mandatory.truncate(6);
    }

    let mandatory_lookup: HashSet<String> = mandatory.iter().cloned().collect();
    let mut priority = freq
        .into_iter()
        .filter(|(item, _)| !mandatory_lookup.contains(item))
        .collect::<Vec<_>>();
    priority.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    (
        mandatory,
        priority
            .into_iter()
            .map(|(item, _)| item)
            .take(8)
            .collect(),
    )
}

fn item_style_weight(item_key: &str) -> (i32, i32) {
    match item_key {
        "infinity edge"
        | "last whisper"
        | "deathblade"
        | "bloodthirster"
        | "sterak s gage"
        | "red buff"
        | "kraken s fury"
        | "giant slayer"
        | "edge of night"
        | "runaan s hurricane"
        | "titan s resolve"
        | "guinsoo s rageblade" => (2, 0),
        "jeweled gauntlet"
        | "rabadon s deathcap"
        | "archangel s staff"
        | "nashor s tooth"
        | "hextech gunblade"
        | "morellonomicon"
        | "void staff"
        | "blue buff"
        | "adaptive helm"
        | "crownguard"
        | "ionic spark"
        | "zhonya s paradox" => (0, 2),
        "spear of shojin" | "hand of justice" => (1, 1),
        _ => (0, 0),
    }
}

fn infer_style(mandatory_items: &[String], priority_items: &[String]) -> String {
    let mut ad_score = 0;
    let mut ap_score = 0;

    for item in mandatory_items {
        let (ad, ap) = item_style_weight(&canonical_key(item));
        ad_score += ad * 2;
        ap_score += ap * 2;
    }

    for item in priority_items {
        let (ad, ap) = item_style_weight(&canonical_key(item));
        ad_score += ad;
        ap_score += ap;
    }

    if ad_score - ap_score >= 3 {
        "AD".to_string()
    } else if ap_score - ad_score >= 3 {
        "AP".to_string()
    } else {
        "Hybrid".to_string()
    }
}

fn plan_for_deck_items(
    owned_items: &HashMap<String, u8>,
    mandatory_items: &[String],
    priority_items: &[String],
    recipe_lookup: &HashMap<String, [String; 2]>,
) -> DeckItemPlan {
    let mut inventory = owned_items.clone();
    let mut missing_components = HashMap::new();
    let mut craftable_count = 0;

    let mut target_items = Vec::new();
    target_items.extend(mandatory_items.iter().cloned());
    target_items.extend(priority_items.iter().take(4).cloned());

    for item in target_items {
        let item_key = canonical_key(&item);
        let Some(recipe) = recipe_lookup.get(&item_key) else {
            continue;
        };
        if recipe[0].is_empty() || recipe[1].is_empty() {
            continue;
        }

        if can_craft(recipe, &inventory) {
            consume(recipe, &mut inventory);
            craftable_count += 1;
        } else {
            let missing = missing_for_recipe(recipe, &inventory);
            for (component, count) in missing {
                *missing_components.entry(component).or_insert(0) += count;
            }
        }
    }

    DeckItemPlan {
        craftable_count,
        missing_components,
    }
}

fn parse_number(value: &str) -> i32 {
    value
        .replace(',', "")
        .trim()
        .parse::<i32>()
        .unwrap_or(0)
}

fn tier_weight(tier: &str) -> i32 {
    match tier.trim() {
        "S" => 60,
        "A" => 45,
        "B" => 30,
        "C" => 15,
        "D" => 5,
        _ => 0,
    }
}

fn build_analysis_entry(
    raw: MetaTftDeckTierEntry,
    payload: &RecommendRequest,
    item_phrases: &[MatchPhrase],
    item_keys: &HashSet<String>,
    recipe_lookup: &HashMap<String, [String; 2]>,
) -> MetaTftDeckAnalysisEntry {
    let parsed = parse_deck_summary(
        &raw.deck_summary,
        item_phrases,
        item_keys,
        &raw.unit_names,
        &raw.item_names,
    );
    let ace_champion = infer_ace_champion(&parsed.champions);
    let (mandatory_items, priority_items) =
        infer_mandatory_and_priority_items(&parsed.opening_items, &parsed.all_items);
    let style = infer_style(&mandatory_items, &priority_items);
    let plan = plan_for_deck_items(
        &payload.owned_items,
        &mandatory_items,
        &priority_items,
        recipe_lookup,
    );

    let mut missing_components: Vec<MissingComponent> = plan
        .missing_components
        .into_iter()
        .map(|(item_id, missing_count)| MissingComponent {
            item_id,
            missing_count,
        })
        .collect();
    missing_components.sort_by(|a, b| a.item_id.cmp(&b.item_id));

    let total_missing: i32 = missing_components
        .iter()
        .map(|component| component.missing_count as i32)
        .sum();
    let fit_score = tier_weight(&raw.tier)
        + (plan.craftable_count as i32 * 18)
        + (parse_number(&raw.games) / 150)
        - (total_missing * 8);

    MetaTftDeckAnalysisEntry {
        comp_name: parsed.comp_name,
        tier: raw.tier,
        avg_placement: raw.avg_placement,
        top4_rate: raw.top4_rate,
        games: raw.games,
        ace_champion,
        champions: parsed.champions,
        mandatory_items,
        priority_items,
        style,
        craftable_target_items: plan.craftable_count,
        missing_components,
        fit_score,
    }
}

fn compare_analysis_entry(left: &MetaTftDeckAnalysisEntry, right: &MetaTftDeckAnalysisEntry) -> Ordering {
    right
        .fit_score
        .cmp(&left.fit_score)
        .then_with(|| right.craftable_target_items.cmp(&left.craftable_target_items))
        .then_with(|| left.missing_components.len().cmp(&right.missing_components.len()))
        .then_with(|| left.comp_name.cmp(&right.comp_name))
}

fn parse_meta_tft_entries(html: &str) -> Result<Vec<MetaTftDeckTierEntry>, String> {
    let document = Html::parse_document(html);
    let row_selector = Selector::parse("table tbody tr")
        .map_err(|error| format!("failed to parse row selector: {error}"))?;
    let cell_selector = Selector::parse("td")
        .map_err(|error| format!("failed to parse cell selector: {error}"))?;
    let link_selector = Selector::parse("a")
        .map_err(|error| format!("failed to parse link selector: {error}"))?;

    let mut entries = Vec::new();
    for row in document.select(&row_selector) {
        let cells: Vec<_> = row.select(&cell_selector).collect();

        if cells.len() < 5 {
            continue;
        }

        let deck_summary = normalize_text(&cells[0].text().collect::<Vec<_>>().join(" "));
        if deck_summary.is_empty() {
            continue;
        }

        let mut unit_names = Vec::new();
        let mut item_names = Vec::new();
        for link in cells[0].select(&link_selector) {
            let href = link.value().attr("href").unwrap_or_default();
            let text = normalize_text(&link.text().collect::<Vec<_>>().join(" "));
            let parsed_name = if text.is_empty() {
                derive_name_from_href(href).unwrap_or_default()
            } else {
                text
            };
            if parsed_name.is_empty() {
                continue;
            }
            if href.contains("/units/") {
                unit_names.push(parsed_name);
            } else if href.contains("/items/") {
                item_names.push(parsed_name);
            }
        }

        let tier = normalize_text(&cells[1].text().collect::<Vec<_>>().join(" "));
        let avg_placement = normalize_text(&cells[2].text().collect::<Vec<_>>().join(" "));
        let top4_rate = normalize_text(&cells[3].text().collect::<Vec<_>>().join(" "));
        let games = normalize_text(&cells[4].text().collect::<Vec<_>>().join(" "));

        entries.push(MetaTftDeckTierEntry {
            deck_summary,
            tier,
            avg_placement,
            top4_rate,
            games,
            unit_names: dedupe_preserve_order(unit_names),
            item_names: dedupe_preserve_order(item_names),
        });
    }

    if entries.is_empty() {
        return Err("meta-tft tier list entries were not found".to_string());
    }

    Ok(entries)
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
fn recommend_comps(app: tauri::AppHandle, payload: RecommendRequest) -> Result<RecommendResponse, String> {
    let mut data = load_tft_data()?;
    let generated = load_generated_comps(&app)?;
    if !generated.is_empty() {
        data.comps = merge_base_and_generated_comps(data.comps, generated);
    }

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

#[tauri::command]
async fn fetch_meta_tft_deck_tier_list() -> Result<MetaTftDeckTierResponse, String> {
    let source_url = META_DECK_SOURCE_URL;
    let client = reqwest::Client::builder()
        .user_agent("tft-supporter/0.1")
        .build()
        .map_err(|error| format!("failed to build http client: {error}"))?;

    let html = client
        .get(source_url)
        .send()
        .await
        .map_err(|error| format!("failed to fetch meta-tft decks page: {error}"))?
        .text()
        .await
        .map_err(|error| format!("failed to read meta-tft response body: {error}"))?;

    let entries = parse_meta_tft_entries(&html)?;
    Ok(MetaTftDeckTierResponse {
        source_url: source_url.to_string(),
        entries: entries.into_iter().take(30).collect(),
    })
}

#[tauri::command]
async fn analyze_meta_tft_decks(
    app: tauri::AppHandle,
    payload: RecommendRequest,
) -> Result<MetaTftDeckAnalysisResponse, String> {
    let source_url = META_DECK_SOURCE_URL;
    let client = reqwest::Client::builder()
        .user_agent("tft-supporter/0.1")
        .build()
        .map_err(|error| format!("failed to build http client: {error}"))?;

    let html = client
        .get(source_url)
        .send()
        .await
        .map_err(|error| format!("failed to fetch meta-tft decks page: {error}"))?
        .text()
        .await
        .map_err(|error| format!("failed to read meta-tft response body: {error}"))?;

    let raw_entries = parse_meta_tft_entries(&html)?;
    let data = load_tft_data()?;
    let item_phrases = build_item_phrases(&data);
    let item_keys: HashSet<String> = item_phrases
        .iter()
        .map(|phrase| phrase.words.join(" "))
        .collect();
    let recipe_lookup = build_item_recipes(&data);

    let mut entries = raw_entries
        .into_iter()
        .take(30)
        .map(|entry| build_analysis_entry(entry, &payload, &item_phrases, &item_keys, &recipe_lookup))
        .collect::<Vec<_>>();

    entries.sort_by(compare_analysis_entry);

    let recommendations = entries
        .iter()
        .take(5)
        .map(|entry| MetaTftDeckRecommendation {
            comp_name: entry.comp_name.clone(),
            tier: entry.tier.clone(),
            style: entry.style.clone(),
            ace_champion: entry.ace_champion.clone(),
            mandatory_items: entry.mandatory_items.clone(),
            priority_items: entry.priority_items.clone(),
            craftable_target_items: entry.craftable_target_items,
            missing_components: entry.missing_components.clone(),
            fit_score: entry.fit_score,
        })
        .collect();

    let mut response = MetaTftDeckAnalysisResponse {
        source_url: source_url.to_string(),
        entries,
        recommendations,
        cache_file_path: String::new(),
        loaded_from_cache: false,
    };

    let saved_path = save_analysis_cache(&app, &response)?;
    response.cache_file_path = saved_path;

    Ok(response)
}

#[tauri::command]
fn load_saved_meta_tft_decks(app: tauri::AppHandle) -> Result<MetaTftDeckAnalysisResponse, String> {
    load_analysis_cache(&app)
}

#[tauri::command]
fn default_generated_comps_path(app: tauri::AppHandle) -> Result<String, String> {
    Ok(generated_comps_file_path(&app)?.to_string_lossy().to_string())
}

#[tauri::command]
async fn export_saved_meta_to_local_recommendations(
    app: tauri::AppHandle,
    target_path: Option<String>,
) -> Result<ExportLocalCompsResponse, String> {
    let mut cached = load_analysis_cache(&app)?;
    let data = load_tft_data()?;
    let item_name_to_id = completed_item_name_to_id_map(&data);
    let mut generated_comps = generate_comps_from_analysis(&cached.entries, &item_name_to_id);

    if generated_comps.is_empty() {
        let source_url = if cached.source_url.trim().is_empty() {
            META_DECK_SOURCE_URL.to_string()
        } else {
            cached.source_url.clone()
        };

        let client = reqwest::Client::builder()
            .user_agent("tft-supporter/0.1")
            .build()
            .map_err(|error| format!("failed to build http client during export refresh: {error}"))?;

        let html = client
            .get(&source_url)
            .send()
            .await
            .map_err(|error| format!("failed to fetch meta decks during export refresh: {error}"))?
            .text()
            .await
            .map_err(|error| format!("failed to read meta response during export refresh: {error}"))?;

        let raw_entries = parse_meta_tft_entries(&html)?;
        let item_phrases = build_item_phrases(&data);
        let item_keys: HashSet<String> = item_phrases
            .iter()
            .map(|phrase| phrase.words.join(" "))
            .collect();
        let recipe_lookup = build_item_recipes(&data);
        let empty_payload = RecommendRequest {
            owned_items: HashMap::new(),
        };

        let mut refreshed_entries = raw_entries
            .into_iter()
            .take(30)
            .map(|entry| {
                build_analysis_entry(
                    entry,
                    &empty_payload,
                    &item_phrases,
                    &item_keys,
                    &recipe_lookup,
                )
            })
            .collect::<Vec<_>>();
        refreshed_entries.sort_by(compare_analysis_entry);

        let refreshed_recommendations = refreshed_entries
            .iter()
            .take(5)
            .map(|entry| MetaTftDeckRecommendation {
                comp_name: entry.comp_name.clone(),
                tier: entry.tier.clone(),
                style: entry.style.clone(),
                ace_champion: entry.ace_champion.clone(),
                mandatory_items: entry.mandatory_items.clone(),
                priority_items: entry.priority_items.clone(),
                craftable_target_items: entry.craftable_target_items,
                missing_components: entry.missing_components.clone(),
                fit_score: entry.fit_score,
            })
            .collect();

        let refreshed_response = MetaTftDeckAnalysisResponse {
            source_url: source_url.clone(),
            entries: refreshed_entries,
            recommendations: refreshed_recommendations,
            cache_file_path: String::new(),
            loaded_from_cache: false,
        };

        let _ = save_analysis_cache(&app, &refreshed_response)?;
        cached = load_analysis_cache(&app)?;
        generated_comps = generate_comps_from_analysis(&cached.entries, &item_name_to_id);
    }

    if generated_comps.is_empty() {
        let analyzed_entries = cached.entries.len();
        return Err(format!(
            "export failed after refresh: analyzed_entries={analyzed_entries}, mapped_items=0. please run 'MetaTFTを分析して保存' once and retry."
        ));
    }

    let generated_path = resolve_generated_comps_file_path(&app, target_path)?;
    if let Some(parent) = generated_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create generated comps directory: {error}"))?;
    }

    let updated_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to get current time: {error}"))?
        .as_secs();

    let payload = GeneratedCompFile {
        generated_from: cached.source_url,
        updated_at_unix,
        comps: generated_comps.clone(),
    };

    let serialized = serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("failed to serialize generated comps: {error}"))?;
    fs::write(&generated_path, serialized)
        .map_err(|error| format!("failed to write generated comps file: {error}"))?;

    Ok(ExportLocalCompsResponse {
        file_path: generated_path.to_string_lossy().to_string(),
        exported_count: generated_comps.len(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            recommend_comps,
            fetch_meta_tft_deck_tier_list,
            analyze_meta_tft_decks,
            load_saved_meta_tft_decks,
            default_generated_comps_path,
            export_saved_meta_to_local_recommendations
        ])
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
