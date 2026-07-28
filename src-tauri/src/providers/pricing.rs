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
    #[serde(default)]
    opencode: Option<ProviderConfig>,
    #[serde(default)]
    kimi: Option<ProviderConfig>,
    #[serde(default)]
    glm: Option<ProviderConfig>,
    #[serde(default)]
    grok: Option<ProviderConfig>,
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
    cache_write_1h: f64,
    #[serde(default)]
    cached_input: f64,
    /// Date-scheduled price overrides. When a model's price changes on a known
    /// future date (e.g. Sonnet 5's introductory pricing ending 2026-08-31),
    /// list the new prices with a `from` date. On lookup, the latest schedule
    /// whose `from` is on/before today (UTC) overrides the base fields.
    #[serde(default)]
    scheduled: Vec<ScheduledPrice>,
    /// Higher rates for long-context requests. xAI bills *every* token in a
    /// request at the higher rate once the prompt reaches the threshold (200k),
    /// so the tier has to be picked per request — this only carries the rates,
    /// and the caller (provider) does the picking.
    #[serde(default)]
    high_context: Option<HighContextTier>,
}

/// Rates that apply when a prompt reaches `threshold_tokens`.
#[derive(Deserialize, Clone, Copy)]
struct HighContextTier {
    threshold_tokens: u64,
    #[serde(default)]
    input: f64,
    #[serde(default)]
    output: f64,
    #[serde(default)]
    cached_input: f64,
}

/// A price override that takes effect on `from` (inclusive, UTC). Any price
/// field left unset (0.0) falls back to the entry's base value.
#[derive(Deserialize)]
struct ScheduledPrice {
    from: String,
    #[serde(default)]
    input: f64,
    #[serde(default)]
    output: f64,
    #[serde(default)]
    cache_read: f64,
    #[serde(default)]
    cache_write: f64,
    #[serde(default)]
    cache_write_1h: f64,
    #[serde(default)]
    cached_input: f64,
    /// Scheduled high-context rates. Base and high tiers live in the same entry,
    /// so a price change has to be able to move both — otherwise the base shifts
    /// on the `from` date while the high tier silently keeps the old rate.
    #[serde(default)]
    high_context: Option<HighContextTier>,
}

/// Prices for one model, already resolved for the current date. This is what
/// callers read from — it hides the base-vs-scheduled distinction.
struct ResolvedPrice {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
    cache_write_1h: f64,
    cached_input: f64,
    /// The entry's high-context rates, replaced wholesale by a schedule that
    /// carries its own.
    high_context: Option<HighContextTier>,
}

impl PricingEntry {
    /// Resolve this entry's prices for `today` (a UTC `YYYY-MM-DD` string),
    /// applying the latest matching scheduled override. Schedules are compared
    /// as ISO date strings, which sort chronologically.
    fn resolve_for(&self, today: &str) -> ResolvedPrice {
        let mut price = ResolvedPrice {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
            cache_write_1h: self.cache_write_1h,
            cached_input: self.cached_input,
            high_context: self.high_context,
        };
        // Pick the latest schedule effective on/before today.
        if let Some(sched) = self
            .scheduled
            .iter()
            .filter(|s| s.from.as_str() <= today)
            .max_by(|a, b| a.from.cmp(&b.from))
        {
            if sched.input > 0.0 { price.input = sched.input; }
            if sched.output > 0.0 { price.output = sched.output; }
            if sched.cache_read > 0.0 { price.cache_read = sched.cache_read; }
            if sched.cache_write > 0.0 { price.cache_write = sched.cache_write; }
            if sched.cache_write_1h > 0.0 { price.cache_write_1h = sched.cache_write_1h; }
            if sched.cached_input > 0.0 { price.cached_input = sched.cached_input; }
            if sched.high_context.is_some() { price.high_context = sched.high_context; }
        }
        price
    }
}

/// Today's date as a UTC `YYYY-MM-DD` string, used to resolve scheduled prices.
fn today_utc() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

// --- Public pricing types (used by providers) ---

pub struct ClaudePricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
}

pub struct CodexPricing {
    pub input: f64,
    pub output: f64,
    pub cached_input: f64,
}

pub struct OpenCodePricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

pub struct KimiPricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

#[allow(dead_code)]
pub struct GlmPricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

/// Grok rates. xAI splits pricing into two tiers by prompt length, and once a
/// request crosses the threshold every token in it bills at the higher rate.
/// Call [`GrokPricing::tier_for`] per request to get the rates that apply.
pub struct GrokPricing {
    pub input: f64,
    pub output: f64,
    pub cached_input: f64,
    /// 0 means a single flat tier (no threshold).
    pub high_threshold_tokens: u64,
    pub high_input: f64,
    pub high_output: f64,
    pub high_cached_input: f64,
}

/// The rates that actually apply to one request.
pub struct GrokTier {
    pub input: f64,
    pub output: f64,
    pub cached_input: f64,
}

impl GrokPricing {
    /// Pick the tier from this request's prompt size — at or above the
    /// threshold, the higher rates apply.
    pub fn tier_for(&self, prompt_tokens: u64) -> GrokTier {
        if self.high_threshold_tokens > 0 && prompt_tokens >= self.high_threshold_tokens {
            GrokTier {
                input: self.high_input,
                output: self.high_output,
                cached_input: self.high_cached_input,
            }
        } else {
            GrokTier {
                input: self.input,
                output: self.output,
                cached_input: self.cached_input,
            }
        }
    }
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

/// Look up and date-resolve a model's prices in one step.
fn resolved_pricing(provider: &ProviderConfig, model: &str) -> ResolvedPrice {
    find_pricing(provider, model).resolve_for(&today_utc())
}

pub fn get_claude_pricing(model: &str) -> ClaudePricing {
    let p = resolved_pricing(&config().claude, model);
    ClaudePricing {
        input: p.input,
        output: p.output,
        cache_read: p.cache_read,
        cache_write_5m: p.cache_write,
        cache_write_1h: if p.cache_write_1h > 0.0 { p.cache_write_1h } else { p.cache_write },
    }
}

pub fn get_codex_pricing(model: &str) -> CodexPricing {
    let p = resolved_pricing(&config().codex, model);
    CodexPricing {
        input: p.input,
        output: p.output,
        cached_input: p.cached_input,
    }
}

pub fn get_kimi_pricing(model: &str) -> KimiPricing {
    let cfg = config();
    if let Some(ref kimi) = cfg.kimi {
        let p = resolved_pricing(kimi, model);
        return KimiPricing {
            input: p.input,
            output: p.output,
            cache_read: p.cache_read,
        };
    }
    // Fallback defaults
    KimiPricing { input: 0.60, output: 2.00, cache_read: 0.0 }
}

#[allow(dead_code)]
pub fn get_glm_pricing(model: &str) -> GlmPricing {
    let cfg = config();
    if let Some(ref glm) = cfg.glm {
        let p = resolved_pricing(glm, model);
        return GlmPricing {
            input: p.input,
            output: p.output,
            cache_read: p.cache_read,
        };
    }
    // Fallback defaults
    GlmPricing { input: 0.50, output: 1.00, cache_read: 0.0 }
}

pub fn get_grok_pricing(model: &str) -> GrokPricing {
    let cfg = config();
    if let Some(ref grok) = cfg.grok {
        let p = resolved_pricing(grok, model);
        let high = p.high_context;
        return GrokPricing {
            input: p.input,
            output: p.output,
            cached_input: p.cached_input,
            high_threshold_tokens: high.map_or(0, |h| h.threshold_tokens),
            high_input: high.map_or(p.input, |h| h.input),
            high_output: high.map_or(p.output, |h| h.output),
            high_cached_input: high.map_or(p.cached_input, |h| h.cached_input),
        };
    }
    // Fallback defaults (Grok 4.5 rates) when pricing.json has no grok section.
    GrokPricing {
        input: 2.00,
        output: 6.00,
        cached_input: 0.30,
        high_threshold_tokens: 200_000,
        high_input: 4.00,
        high_output: 12.00,
        high_cached_input: 0.60,
    }
}

pub fn get_opencode_pricing(model: &str) -> OpenCodePricing {
    let cfg = config();
    // Use dedicated opencode pricing if available, otherwise try to match
    // against claude or codex pricing tables based on model name.
    if let Some(ref oc) = cfg.opencode {
        let p = resolved_pricing(oc, model);
        return OpenCodePricing {
            input: p.input,
            output: p.output,
            cache_read: p.cache_read,
            cache_write: p.cache_write,
        };
    }

    // Fallback: try claude pricing first (for claude-* models), then codex
    if model.contains("claude") || model.contains("fable") || model.contains("mythos") || model.contains("sonnet") || model.contains("opus") || model.contains("haiku") {
        let p = resolved_pricing(&cfg.claude, model);
        OpenCodePricing {
            input: p.input,
            output: p.output,
            cache_read: p.cache_read,
            cache_write: p.cache_write,
        }
    } else {
        let p = resolved_pricing(&cfg.codex, model);
        OpenCodePricing {
            input: p.input,
            output: p.output,
            cache_read: p.cached_input,
            cache_write: 0.0,
        }
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub opencode: Vec<PricingRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kimi: Vec<PricingRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub glm: Vec<PricingRow>,
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
    let today = today_utc();
    let mut rows = Vec::new();
    let mut seen_labels = std::collections::HashSet::new();
    for entry in &provider.models {
        let label = if entry.label.is_empty() { &entry.match_pattern } else { &entry.label };
        if seen_labels.insert(label.to_string()) {
            // Show the price effective today so scheduled changes (e.g. Sonnet 5
            // introductory → standard) surface in the tooltip automatically.
            let p = entry.resolve_for(&today);
            rows.push(PricingRow {
                model: label.to_string(),
                input: format_price(p.input),
                output: format_price(p.output),
                cache_read: format_price(if use_cached_input { p.cached_input } else { p.cache_read }),
                cache_write: if use_cached_input { "—".to_string() } else { format_price(p.cache_write) },
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
        opencode: cfg.opencode.as_ref().map(|oc| deduplicated_rows(oc, false)).unwrap_or_default(),
        kimi: cfg.kimi.as_ref().map(|k| deduplicated_rows(k, false)).unwrap_or_default(),
        glm: cfg.glm.as_ref().map(|g| deduplicated_rows(g, false)).unwrap_or_default(),
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
        assert!(cfg.opencode.is_some());
        assert!(!cfg.opencode.unwrap().models.is_empty());
        assert!(cfg.kimi.is_some());
        assert!(!cfg.kimi.unwrap().models.is_empty());
        assert!(cfg.glm.is_some());
        assert!(!cfg.glm.unwrap().models.is_empty());
        assert!(cfg.grok.is_some());
        assert!(!cfg.grok.unwrap().models.is_empty());
    }

    #[test]
    fn grok_45_pricing() {
        let p = get_grok_pricing("grok-4.5");
        assert_eq!(p.input, 2.00);
        assert_eq!(p.output, 6.00);
        assert_eq!(p.cached_input, 0.30);
        assert_eq!(p.high_threshold_tokens, 200_000);
    }

    #[test]
    fn grok_build_variant_bills_as_grok_45() {
        // The CLI reports the agent variant as its model id on some turns.
        let variant = get_grok_pricing("grok-4.5-build");
        let base = get_grok_pricing("grok-4.5");
        assert_eq!(variant.input, base.input);
        assert_eq!(variant.output, base.output);
    }

    #[test]
    fn grok_code_fast_is_cheaper_than_45() {
        let fast = get_grok_pricing("grok-code-fast-1");
        assert_eq!(fast.input, 1.00);
        assert_eq!(fast.output, 2.00);
        // grok-build-0.1 is the same model under its versioned id.
        assert_eq!(get_grok_pricing("grok-build-0.1").input, fast.input);
    }

    #[test]
    fn grok_43_not_billed_as_45() {
        let p43 = get_grok_pricing("grok-4.3");
        assert_eq!(p43.input, 1.25);
        assert_ne!(p43.output, get_grok_pricing("grok-4.5").output);
    }

    #[test]
    fn grok_unknown_model_defaults_to_45() {
        let unknown = get_grok_pricing("grok-9.9-experimental");
        assert_eq!(unknown.input, get_grok_pricing("grok-4.5").input);
    }

    #[test]
    fn scheduled_override_moves_the_high_context_tier_too() {
        // Base and high rates live in one entry, so a scheduled price change has
        // to move both — otherwise the base shifts on the `from` date while long
        // prompts keep billing at the old high rate, with nothing to signal it.
        let entry: PricingEntry = serde_json::from_str(
            r#"{ "match": "grok-test", "input": 2.0, "output": 6.0, "cached_input": 0.3,
                 "high_context": { "threshold_tokens": 200000, "input": 4.0, "output": 12.0, "cached_input": 0.6 },
                 "scheduled": [ { "from": "2026-01-01", "input": 3.0, "output": 9.0,
                   "high_context": { "threshold_tokens": 200000, "input": 6.0, "output": 18.0, "cached_input": 0.9 } } ] }"#,
        )
        .unwrap();

        let resolved = entry.resolve_for("2026-07-29");
        assert_eq!(resolved.input, 3.0);
        let high = resolved.high_context.expect("scheduled high tier");
        assert_eq!(high.input, 6.0);
        assert_eq!(high.output, 18.0);
    }

    #[test]
    fn schedule_without_high_context_keeps_the_base_tier() {
        let entry: PricingEntry = serde_json::from_str(
            r#"{ "match": "grok-test", "input": 2.0, "output": 6.0,
                 "high_context": { "threshold_tokens": 200000, "input": 4.0, "output": 12.0, "cached_input": 0.6 },
                 "scheduled": [ { "from": "2026-01-01", "input": 3.0 } ] }"#,
        )
        .unwrap();

        let resolved = entry.resolve_for("2026-07-29");
        assert_eq!(resolved.input, 3.0);
        assert_eq!(resolved.high_context.expect("base high tier").input, 4.0);
    }

    #[test]
    fn grok_high_context_tier_doubles_rates() {
        let p = get_grok_pricing("grok-4.5");
        let below = p.tier_for(199_999);
        let above = p.tier_for(200_000);
        assert_eq!(below.input, 2.00);
        assert_eq!(above.input, 4.00);
        assert_eq!(above.output, 12.00);
        assert_eq!(above.cached_input, 0.60);
    }

    #[test]
    fn claude_opus_pricing() {
        let p = get_claude_pricing("claude-opus-4-6-20260320");
        assert!((p.input - 5.0).abs() < 0.001);
        assert!((p.output - 25.0).abs() < 0.001);
        assert!((p.cache_write_5m - 6.25).abs() < 0.001);
        assert!((p.cache_write_1h - 10.0).abs() < 0.001);
    }

    // Regression guard: "opus-4-7" must match its own entry, not fall through
    // to the "opus-4" substring and get billed at Opus 4.1 rates ($15/$75).
    #[test]
    fn claude_opus_47_not_billed_as_41() {
        let p = get_claude_pricing("claude-opus-4-7-20260416");
        assert!((p.input - 5.0).abs() < 0.001, "Opus 4.7 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 25.0).abs() < 0.001, "Opus 4.7 output must be $25/MTok, got ${}", p.output);
        assert!((p.cache_read - 0.50).abs() < 0.001);
        assert!((p.cache_write_5m - 6.25).abs() < 0.001);
        assert!((p.cache_write_1h - 10.0).abs() < 0.001);
    }

    #[test]
    fn opencode_opus_47_not_billed_as_41() {
        let p = get_opencode_pricing("anthropic/claude-opus-4-7-20260416");
        assert!((p.input - 5.0).abs() < 0.001, "Opencode Opus 4.7 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 25.0).abs() < 0.001);
    }

    // Regression guard: "opus-4-8" must match its own entry, not fall through
    // to the "opus-4" substring and get billed at Opus 4.1 rates ($15/$75).
    // Opus 4.8 shares 4.5/4.6/4.7 pricing ($5/$25) but needs its own entry
    // so the substring match does not land on "opus-4" (Opus 4.1/4).
    #[test]
    fn claude_opus_48_not_billed_as_41() {
        let p = get_claude_pricing("claude-opus-4-8-20260528");
        assert!((p.input - 5.0).abs() < 0.001, "Opus 4.8 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 25.0).abs() < 0.001, "Opus 4.8 output must be $25/MTok, got ${}", p.output);
        assert!((p.cache_read - 0.50).abs() < 0.001);
        assert!((p.cache_write_5m - 6.25).abs() < 0.001);
        assert!((p.cache_write_1h - 10.0).abs() < 0.001);
    }

    #[test]
    fn opencode_opus_48_not_billed_as_41() {
        let p = get_opencode_pricing("anthropic/claude-opus-4-8-20260528");
        assert!((p.input - 5.0).abs() < 0.001, "Opencode Opus 4.8 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 25.0).abs() < 0.001);
    }

    // Regression guard: "claude-fable-5" must resolve to its own Fable entry
    // ($10/$50), not fall through to the "sonnet" default and get under-billed
    // at Sonnet rates ($3/$15). Fable 5 is the top tier above Opus.
    #[test]
    fn claude_fable_5_not_billed_as_sonnet() {
        let p = get_claude_pricing("claude-fable-5");
        assert!((p.input - 10.0).abs() < 0.001, "Fable 5 input must be $10/MTok, got ${}", p.input);
        assert!((p.output - 50.0).abs() < 0.001, "Fable 5 output must be $50/MTok, got ${}", p.output);
        assert!((p.cache_read - 1.00).abs() < 0.001);
        assert!((p.cache_write_5m - 12.5).abs() < 0.001);
        assert!((p.cache_write_1h - 20.0).abs() < 0.001);
    }

    #[test]
    fn opencode_fable_5_not_billed_as_sonnet() {
        let p = get_opencode_pricing("anthropic/claude-fable-5");
        assert!((p.input - 10.0).abs() < 0.001, "Opencode Fable 5 input must be $10/MTok, got ${}", p.input);
        assert!((p.output - 50.0).abs() < 0.001);
    }

    // Regression guard: "claude-mythos-5" (Project Glasswing, limited availability)
    // shares Fable 5 pricing ($10/$50) and must not fall through to the "sonnet"
    // default ($3/$15).
    #[test]
    fn claude_mythos_5_not_billed_as_sonnet() {
        let p = get_claude_pricing("claude-mythos-5");
        assert!((p.input - 10.0).abs() < 0.001, "Mythos 5 input must be $10/MTok, got ${}", p.input);
        assert!((p.output - 50.0).abs() < 0.001, "Mythos 5 output must be $50/MTok, got ${}", p.output);
        assert!((p.cache_read - 1.00).abs() < 0.001);
        assert!((p.cache_write_5m - 12.5).abs() < 0.001);
        assert!((p.cache_write_1h - 20.0).abs() < 0.001);
    }

    // Regression guard: "claude-opus-5" must resolve to its own "Opus 5" entry.
    // Its price ($5/$25) coincides with the generic "opus" fallback, so the
    // label is the signal that the match landed on the right entry — without a
    // dedicated entry the tooltip/breakdown would mislabel it "Opus 4.8/4.7/4.6/4.5".
    #[test]
    fn claude_opus_5_matches_own_entry() {
        let p = get_claude_pricing("claude-opus-5");
        assert!((p.input - 5.0).abs() < 0.001, "Opus 5 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 25.0).abs() < 0.001, "Opus 5 output must be $25/MTok, got ${}", p.output);
        assert!((p.cache_read - 0.50).abs() < 0.001);
        assert!((p.cache_write_5m - 6.25).abs() < 0.001);
        assert!((p.cache_write_1h - 10.0).abs() < 0.001);

        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let entry = find_pricing(&cfg.claude, "claude-opus-5");
        assert_eq!(entry.label, "Opus 5", "claude-opus-5 must match its own entry, not the generic opus fallback");
    }

    // "opus-5" must not shadow the opus-4-x entries ("claude-opus-4-5..." does
    // not contain "opus-5"), and 1M-context variants like "claude-opus-5[1m]"
    // must still land on the Opus 5 entry.
    #[test]
    fn claude_opus_5_ordering_is_safe() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let opus45 = find_pricing(&cfg.claude, "claude-opus-4-5-20251101");
        assert_eq!(opus45.label, "Opus 4.8/4.7/4.6/4.5");
        let opus5_1m = find_pricing(&cfg.claude, "claude-opus-5[1m]");
        assert_eq!(opus5_1m.label, "Opus 5");
    }

    #[test]
    fn opencode_opus_5_pricing() {
        let p = get_opencode_pricing("anthropic/claude-opus-5");
        assert!((p.input - 5.0).abs() < 0.001, "Opencode Opus 5 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 25.0).abs() < 0.001);

        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let oc = cfg.opencode.as_ref().expect("opencode config present");
        let entry = find_pricing(oc, "anthropic/claude-opus-5");
        assert_eq!(entry.label, "Claude Opus 5");
    }

    #[test]
    fn claude_sonnet_pricing() {
        let p = get_claude_pricing("claude-sonnet-4-6-20260320");
        assert!((p.input - 3.0).abs() < 0.001);
        assert!((p.output - 15.0).abs() < 0.001);
        assert!((p.cache_write_5m - 3.75).abs() < 0.001);
        assert!((p.cache_write_1h - 6.0).abs() < 0.001);
    }

    // Regression guard: "claude-sonnet-5" must match its own entry, not fall
    // through to the "sonnet" default (Sonnet 4.x, $3/$15). Its entry sits above
    // the "sonnet" fallback so the substring match lands correctly.
    #[test]
    fn claude_sonnet_5_not_billed_as_sonnet_4x() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let entry = find_pricing(&cfg.claude, "claude-sonnet-5");
        // Introductory window: $2/$10.
        let intro = entry.resolve_for("2026-07-01");
        assert!((intro.input - 2.0).abs() < 0.001, "Sonnet 5 intro input must be $2/MTok, got ${}", intro.input);
        assert!((intro.output - 10.0).abs() < 0.001, "Sonnet 5 intro output must be $10/MTok, got ${}", intro.output);
        assert!((intro.cache_read - 0.20).abs() < 0.001);
        assert!((intro.cache_write - 2.50).abs() < 0.001);
        assert!((intro.cache_write_1h - 4.0).abs() < 0.001);
    }

    // Scheduled price transition: on/after 2026-09-01, Sonnet 5 moves to standard
    // pricing ($3/$15) automatically, with no manual pricing.json edit.
    #[test]
    fn claude_sonnet_5_switches_to_standard_on_sept_1() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let entry = find_pricing(&cfg.claude, "claude-sonnet-5");

        // Day before the switch: still introductory.
        let before = entry.resolve_for("2026-08-31");
        assert!((before.input - 2.0).abs() < 0.001, "on 8/31 input must still be $2, got ${}", before.input);
        assert!((before.output - 10.0).abs() < 0.001);

        // Switch day (inclusive): standard.
        let on = entry.resolve_for("2026-09-01");
        assert!((on.input - 3.0).abs() < 0.001, "on 9/1 input must be $3, got ${}", on.input);
        assert!((on.output - 15.0).abs() < 0.001, "on 9/1 output must be $15, got ${}", on.output);
        assert!((on.cache_read - 0.30).abs() < 0.001);
        assert!((on.cache_write - 3.75).abs() < 0.001);
        assert!((on.cache_write_1h - 6.0).abs() < 0.001);

        // Well after the switch: still standard.
        let after = entry.resolve_for("2027-01-01");
        assert!((after.input - 3.0).abs() < 0.001);
    }

    // Entries without a `scheduled` array resolve to their base values unchanged.
    #[test]
    fn unscheduled_entry_resolves_to_base() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let entry = find_pricing(&cfg.claude, "claude-opus-4-8-20260528");
        let p = entry.resolve_for("2026-09-01");
        assert!((p.input - 5.0).abs() < 0.001);
        assert!((p.output - 25.0).abs() < 0.001);
    }

    #[test]
    fn opencode_sonnet_5_not_billed_as_sonnet_4x() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let oc = cfg.opencode.as_ref().expect("opencode config present");
        let entry = find_pricing(oc, "anthropic/claude-sonnet-5");
        let intro = entry.resolve_for("2026-07-01");
        assert!((intro.input - 2.0).abs() < 0.001, "Opencode Sonnet 5 intro input must be $2/MTok, got ${}", intro.input);
        assert!((intro.output - 10.0).abs() < 0.001);
        let standard = entry.resolve_for("2026-09-01");
        assert!((standard.input - 3.0).abs() < 0.001, "Opencode Sonnet 5 standard input must be $3/MTok, got ${}", standard.input);
        assert!((standard.output - 15.0).abs() < 0.001);
    }

    #[test]
    fn claude_haiku_pricing() {
        let p = get_claude_pricing("claude-haiku-4-5-20251001");
        assert!((p.input - 1.0).abs() < 0.001);
        assert!((p.output - 5.0).abs() < 0.001);
        assert!((p.cache_write_5m - 1.25).abs() < 0.001);
        assert!((p.cache_write_1h - 2.0).abs() < 0.001);
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
        // base gpt-5.2 is $1.25; the gpt-5.2-codex variant is $1.75 (see
        // codex_gpt52_codex_uses_codex_rate).
        let p = get_codex_pricing("gpt-5.2");
        assert!((p.input - 1.25).abs() < 0.001);
    }

    #[test]
    fn codex_unknown_defaults_to_gpt54() {
        let p = get_codex_pricing("some-future-model");
        assert!((p.input - 2.50).abs() < 0.001);
    }

    // Regression guard: the GPT-5.6 tiers (Sol/Terra/Luna, GA 2026-07-09) must
    // each match their own entry instead of falling through the substring chain
    // to bare "gpt-5" ($1.25/$10) — Sol would be under-billed 4x, Luna
    // over-billed on input.
    #[test]
    fn codex_gpt56_sol_pricing() {
        let p = get_codex_pricing("gpt-5.6-sol");
        assert!((p.input - 5.00).abs() < 0.001, "gpt-5.6-sol input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 30.00).abs() < 0.001, "gpt-5.6-sol output must be $30/MTok, got ${}", p.output);
        assert!((p.cached_input - 0.50).abs() < 0.001);
    }

    #[test]
    fn codex_gpt56_terra_pricing() {
        let p = get_codex_pricing("gpt-5.6-terra");
        assert!((p.input - 2.50).abs() < 0.001, "gpt-5.6-terra input must be $2.5/MTok, got ${}", p.input);
        assert!((p.output - 15.00).abs() < 0.001);
        assert!((p.cached_input - 0.25).abs() < 0.001);
    }

    #[test]
    fn codex_gpt56_luna_pricing() {
        let p = get_codex_pricing("gpt-5.6-luna");
        assert!((p.input - 1.00).abs() < 0.001, "gpt-5.6-luna input must be $1/MTok, got ${}", p.input);
        assert!((p.output - 6.00).abs() < 0.001);
        assert!((p.cached_input - 0.10).abs() < 0.001);
    }

    // Bare "gpt-5.6" (no tier suffix) is priced at the flagship Sol rate and
    // must not fall through to the bare "gpt-5" entry.
    #[test]
    fn codex_gpt56_bare_not_billed_as_gpt5() {
        let p = get_codex_pricing("gpt-5.6");
        assert!((p.input - 5.00).abs() < 0.001, "gpt-5.6 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 30.00).abs() < 0.001);
    }

    #[test]
    fn opencode_gpt56_terra_pricing() {
        let p = get_opencode_pricing("openai/gpt-5.6-terra");
        assert!((p.input - 2.50).abs() < 0.001, "opencode gpt-5.6-terra input must be $2.5/MTok, got ${}", p.input);
        assert!((p.output - 15.00).abs() < 0.001);
    }

    // Regression guard: "gpt-5-codex" (the default Codex CLI model) must match
    // its own entry, not fall through to the gpt-5.4 default and get billed at
    // $2.50/$15 instead of the correct $1.25/$10 (~1.9x overcharge). Reporter saw
    // Codex daily cost inflated vs. their reference tool.
    #[test]
    fn codex_gpt5_codex_not_billed_as_gpt54() {
        let p = get_codex_pricing("gpt-5-codex");
        assert!((p.input - 1.25).abs() < 0.001, "gpt-5-codex input must be $1.25/MTok, got ${}", p.input);
        assert!((p.output - 10.00).abs() < 0.001, "gpt-5-codex output must be $10/MTok, got ${}", p.output);
        assert!((p.cached_input - 0.125).abs() < 0.001, "gpt-5-codex cached must be $0.125/MTok, got ${}", p.cached_input);
    }

    // contains() ordering guard: "gpt-5.3-codex" must still match its own entry,
    // not the broader gpt-5-codex one.
    #[test]
    fn codex_gpt53_codex_unaffected_by_gpt5_codex_entry() {
        let p = get_codex_pricing("gpt-5.3-codex");
        assert!((p.input - 1.75).abs() < 0.001, "gpt-5.3-codex input must be $1.75/MTok, got ${}", p.input);
        assert!((p.output - 14.00).abs() < 0.001);
    }

    // Regression guard: "gpt-5.2-codex" was previously mis-registered at the
    // gpt-5.2 base rate ($1.25/$10); the codex variant is actually $1.75/$14.
    #[test]
    fn codex_gpt52_codex_uses_codex_rate() {
        let p = get_codex_pricing("gpt-5.2-codex");
        assert!((p.input - 1.75).abs() < 0.001, "gpt-5.2-codex input must be $1.75/MTok, got ${}", p.input);
        assert!((p.output - 14.00).abs() < 0.001, "gpt-5.2-codex output must be $14/MTok, got ${}", p.output);
        assert!((p.cached_input - 0.175).abs() < 0.001);
    }

    // "gpt-5.1-codex-max" / "gpt-5.1-codex-mini" must match their own specific
    // entries before the broader "gpt-5.1-codex" / "gpt-5.1" patterns.
    #[test]
    fn codex_gpt51_codex_variants_resolve_specifically() {
        let max = get_codex_pricing("gpt-5.1-codex-max");
        assert!((max.input - 1.25).abs() < 0.001, "gpt-5.1-codex-max input must be $1.25, got ${}", max.input);
        assert!((max.output - 10.00).abs() < 0.001);
        let mini = get_codex_pricing("gpt-5.1-codex-mini");
        assert!((mini.input - 0.25).abs() < 0.001, "gpt-5.1-codex-mini input must be $0.25, got ${}", mini.input);
        assert!((mini.output - 2.00).abs() < 0.001);
        let base = get_codex_pricing("gpt-5.1");
        assert!((base.input - 0.625).abs() < 0.001, "gpt-5.1 input must be $0.625, got ${}", base.input);
        assert!((base.output - 5.00).abs() < 0.001);
    }

    // "gpt-5" base must resolve to its own entry, not be shadowed by a gpt-5.x
    // pattern (it is placed last, after all gpt-5.x entries).
    #[test]
    fn codex_gpt5_base_resolves_after_dotted_variants() {
        let p = get_codex_pricing("gpt-5");
        assert!((p.input - 1.25).abs() < 0.001, "gpt-5 input must be $1.25, got ${}", p.input);
        assert!((p.output - 10.00).abs() < 0.001);
        // gpt-5.4 must NOT be captured by the gpt-5 base entry
        let dotted = get_codex_pricing("gpt-5.4");
        assert!((dotted.input - 2.50).abs() < 0.001, "gpt-5.4 must stay $2.50, got ${}", dotted.input);
    }

    // Dated snapshot of gpt-5-codex (e.g. gpt-5-codex-2025-09-15) must still
    // resolve to the gpt-5-codex entry.
    #[test]
    fn codex_gpt5_codex_dated_snapshot_resolves() {
        let p = get_codex_pricing("gpt-5-codex-2025-09-15");
        assert!((p.input - 1.25).abs() < 0.001, "dated gpt-5-codex must be $1.25, got ${}", p.input);
        assert!((p.output - 10.00).abs() < 0.001);
    }

    // Opencode "openai/gpt-5-codex" must match the opencode codex entry, not
    // fall through to the opencode default ("sonnet" = $3/$15).
    #[test]
    fn opencode_gpt5_codex_not_billed_as_sonnet() {
        let p = get_opencode_pricing("openai/gpt-5-codex");
        assert!((p.input - 1.25).abs() < 0.001, "opencode gpt-5-codex input must be $1.25, got ${}", p.input);
        assert!((p.output - 10.00).abs() < 0.001, "opencode gpt-5-codex output must be $10, got ${}", p.output);
    }

    // Regression guard: "gpt-5.5" must match its own entry, not fall through
    // to the default ("gpt-5.4") and get billed at GPT-5.4 rates ($2.50/$15).
    #[test]
    fn codex_gpt55_not_billed_as_gpt54() {
        let p = get_codex_pricing("gpt-5.5");
        assert!((p.input - 5.00).abs() < 0.001, "GPT-5.5 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 30.00).abs() < 0.001, "GPT-5.5 output must be $30/MTok, got ${}", p.output);
        assert!((p.cached_input - 0.50).abs() < 0.001);
    }

    #[test]
    fn codex_gpt55_pro_not_billed_as_gpt54() {
        let p = get_codex_pricing("gpt-5.5-pro");
        assert!((p.input - 30.00).abs() < 0.001, "GPT-5.5 Pro input must be $30/MTok, got ${}", p.input);
        assert!((p.output - 180.00).abs() < 0.001, "GPT-5.5 Pro output must be $180/MTok, got ${}", p.output);
    }

    #[test]
    fn opencode_gpt55_not_billed_as_gpt54() {
        let p = get_opencode_pricing("openai/gpt-5.5");
        assert!((p.input - 5.00).abs() < 0.001, "Opencode GPT-5.5 input must be $5/MTok, got ${}", p.input);
        assert!((p.output - 30.00).abs() < 0.001);
    }

    // Regression guard: dated snapshot IDs (e.g. gpt-5.5-2026-04-23) must
    // resolve to the gpt-5.5 entry, not the gpt-5.4 default fallback.
    #[test]
    fn codex_gpt55_dated_snapshot_resolves_correctly() {
        let p = get_codex_pricing("gpt-5.5-2026-04-23");
        assert!((p.input - 5.00).abs() < 0.001, "GPT-5.5 dated snapshot must match gpt-5.5, got input ${}", p.input);
        assert!((p.output - 30.00).abs() < 0.001);
    }
}
