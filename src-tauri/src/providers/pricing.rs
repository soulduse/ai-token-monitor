use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Embedded pricing JSON (compile-time fallback)
const EMBEDDED_PRICING: &str = include_str!("../../pricing.json");

static PRICING: OnceLock<PricingConfig> = OnceLock::new();

// --- JSON schema types ---

#[derive(Deserialize)]
struct PricingConfig {
    claude: ProviderConfig,
    codex: ProviderConfig,
}

#[derive(Deserialize)]
struct ProviderConfig {
    default: String,
    models: Vec<PricingEntry>,
}

#[derive(Deserialize)]
struct PricingEntry {
    #[serde(rename = "match")]
    match_pattern: String,
    #[serde(default)]
    label: String,
    input: f64,
    output: f64,
    #[serde(default)]
    cache_read: f64,
    #[serde(default)]
    cache_write: f64,
    #[serde(default)]
    cached_input: f64,
}

// --- Public pricing types (used by providers) ---

pub struct ClaudePricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

pub struct CodexPricing {
    pub input: f64,
    pub output: f64,
    pub cached_input: f64,
}

// --- Loading ---

fn config() -> &'static PricingConfig {
    PRICING.get_or_init(|| {
        // Try loading from user's ~/.claude/pricing.json first
        if let Some(home) = dirs::home_dir() {
            let user_path = home.join(".claude").join("pricing.json");
            if let Ok(contents) = std::fs::read_to_string(&user_path) {
                if let Ok(cfg) = serde_json::from_str(&contents) {
                    eprintln!("[PRICING] Loaded from {}", user_path.display());
                    return cfg;
                }
            }
        }

        // Fallback to embedded
        eprintln!("[PRICING] Using embedded pricing data");
        serde_json::from_str(EMBEDDED_PRICING).expect("embedded pricing.json must be valid")
    })
}

fn find_pricing<'a>(provider: &'a ProviderConfig, model: &str) -> &'a PricingEntry {
    // First match wins (order in JSON matters)
    provider
        .models
        .iter()
        .find(|e| model.contains(&e.match_pattern))
        .unwrap_or_else(|| {
            // Fallback to default model
            provider
                .models
                .iter()
                .find(|e| e.match_pattern == provider.default)
                .unwrap_or(&provider.models[0])
        })
}

// --- Public API ---

pub fn get_claude_pricing(model: &str) -> ClaudePricing {
    let entry = find_pricing(&config().claude, model);
    ClaudePricing {
        input: entry.input,
        output: entry.output,
        cache_read: entry.cache_read,
        cache_write: entry.cache_write,
    }
}

pub fn get_codex_pricing(model: &str) -> CodexPricing {
    let entry = find_pricing(&config().codex, model);
    CodexPricing {
        input: entry.input,
        output: entry.output,
        cached_input: entry.cached_input,
    }
}

// --- Frontend API (pricing table for tooltip display) ---

#[derive(Serialize, Clone)]
pub struct PricingRow {
    pub model: String,
    pub input: String,
    pub output: String,
    pub cache_read: String,
    pub cache_write: String,
}

#[derive(Serialize, Clone)]
pub struct PricingTable {
    pub version: String,
    pub last_updated: String,
    pub claude: Vec<PricingRow>,
    pub codex: Vec<PricingRow>,
}

fn format_price(val: f64) -> String {
    if val == 0.0 {
        "—".to_string()
    } else if val < 0.01 {
        format!("${:.3}", val)
    } else if val == val.floor() {
        format!("${:.0}", val)
    } else {
        format!("${:.2}", val)
    }
}

fn deduplicated_rows(provider: &ProviderConfig, use_cached_input: bool) -> Vec<PricingRow> {
    let mut rows = Vec::new();
    let mut seen_labels = std::collections::HashSet::new();
    for entry in &provider.models {
        let label = if entry.label.is_empty() { &entry.match_pattern } else { &entry.label };
        if seen_labels.insert(label.to_string()) {
            rows.push(PricingRow {
                model: label.to_string(),
                input: format_price(entry.input),
                output: format_price(entry.output),
                cache_read: format_price(if use_cached_input { entry.cached_input } else { entry.cache_read }),
                cache_write: if use_cached_input { "—".to_string() } else { format_price(entry.cache_write) },
            });
        }
    }
    rows
}

pub fn get_pricing_table() -> PricingTable {
    let cfg = config();
    // Read version/last_updated from the raw JSON
    let raw: serde_json::Value = serde_json::from_str(EMBEDDED_PRICING).unwrap_or_default();
    PricingTable {
        version: raw.get("version").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
        last_updated: raw.get("last_updated").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
        claude: deduplicated_rows(&cfg.claude, false),
        codex: deduplicated_rows(&cfg.codex, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_json_parses() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        assert!(!cfg.claude.models.is_empty());
        assert!(!cfg.codex.models.is_empty());
    }

    #[test]
    fn claude_opus_pricing() {
        let p = get_claude_pricing("claude-opus-4-6-20260320");
        assert!((p.input - 5.0).abs() < 0.001);
        assert!((p.output - 25.0).abs() < 0.001);
    }

    #[test]
    fn claude_sonnet_pricing() {
        let p = get_claude_pricing("claude-sonnet-4-6-20260320");
        assert!((p.input - 3.0).abs() < 0.001);
        assert!((p.output - 15.0).abs() < 0.001);
    }

    #[test]
    fn claude_haiku_pricing() {
        let p = get_claude_pricing("claude-haiku-4-5-20251001");
        assert!((p.input - 1.0).abs() < 0.001);
        assert!((p.output - 5.0).abs() < 0.001);
    }

    #[test]
    fn claude_unknown_defaults_to_sonnet() {
        let p = get_claude_pricing("claude-unknown-model");
        assert!((p.input - 3.0).abs() < 0.001);
    }

    #[test]
    fn codex_o4_mini_pricing() {
        let p = get_codex_pricing("o4-mini-2025-04-16");
        assert!((p.input - 1.10).abs() < 0.001);
    }

    #[test]
    fn codex_gpt52_pricing() {
        let p = get_codex_pricing("gpt-5.2-codex");
        assert!((p.input - 1.25).abs() < 0.001);
    }

    #[test]
    fn codex_unknown_defaults_to_o4_mini() {
        let p = get_codex_pricing("some-future-model");
        assert!((p.input - 1.10).abs() < 0.001);
    }
}
