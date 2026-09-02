use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

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
    /// future date (e.g. an introductory price ending), list the new prices
    /// with a `from` date. On lookup, the latest schedule whose `from` is
    /// on/before today (UTC) overrides the base fields.
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

/// Normalize a model id so cosmetic spelling differences don't defeat matching.
///
/// Proxies and gateways (omniroute, LiteLLM, Bedrock, …) rewrite the model id
/// on its way into the log, so the same model arrives under several spellings:
/// `claude-opus-4.8` (dots for hyphens), `CLAUDE5_SONNET` (upper snake), or
/// `kiro/claude-opus-5` (vendor prefix). The patterns in `pricing.json` use one
/// spelling — lowercase, hyphenated — so a dotted id like `claude-opus-4.8`
/// misses `opus-4-8` and falls through to the broader `opus-4` entry, billing
/// Opus 4.8 at Opus 4.1 rates (3x). Folding both sides to the same shape makes
/// the match spelling-independent.
///
/// This only folds spelling; it leaves vendor prefixes alone, since patterns
/// match as substrings and `kiro/claude-opus-5` already contains `opus-5`.
/// Stripping the prefix is [`normalize_model_id`]'s job.
fn canonical_model(model: &str) -> String {
    model
        .chars()
        .map(|c| match c {
            '.' | '_' => '-',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// Model ids already reported as unmatched, so the warning fires once per id
/// rather than once per message.
static WARNED_UNMATCHED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Warn once that `model` matched no pattern and is being billed at
/// `provider`'s default rates. A silent fallback is how a new model id ends up
/// mispriced with nothing to point at, so each unknown id gets one line.
fn warn_unmatched(provider_name: &str, model: &str, default: &str) {
    let mut guard = match WARNED_UNMATCHED.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let seen = guard.get_or_insert_with(HashSet::new);
    if seen.insert(format!("{}:{}", provider_name, model)) {
        eprintln!(
            "[PRICING] No pattern matched model {:?} in the {} table — billing at the default ({:?}) rates. \
             Add an entry to pricing.json if this model has its own price.",
            model, provider_name, default
        );
    }
}

/// Find the entry for `model`, or the provider's default when nothing matches.
/// `provider_name` only labels the unmatched-model warning.
fn find_pricing_in<'a>(
    provider: &'a ProviderConfig,
    provider_name: &str,
    model: &str,
) -> &'a PricingEntry {
    // Compare canonical forms so dotted/upper/underscored ids from proxies land
    // on the same entry as the hyphenated lowercase ids the patterns are written in.
    let canonical = canonical_model(model);
    // First match wins (order in JSON matters)
    provider
        .models
        .iter()
        .find(|e| canonical.contains(&canonical_model(&e.match_pattern)))
        .unwrap_or_else(|| {
            warn_unmatched(provider_name, model, &provider.default);
            // Fallback to default model
            provider
                .models
                .iter()
                .find(|e| e.match_pattern == provider.default)
                .unwrap_or(&provider.models[0])
        })
}

// --- Public API ---

/// Entry lookup without a provider label, for tests asserting which entry a
/// model id lands on.
#[cfg(test)]
fn find_pricing<'a>(provider: &'a ProviderConfig, model: &str) -> &'a PricingEntry {
    find_pricing_in(provider, "test", model)
}

/// Look up and date-resolve a model's prices in one step.
fn resolved_pricing(provider: &ProviderConfig, provider_name: &str, model: &str) -> ResolvedPrice {
    find_pricing_in(provider, provider_name, model).resolve_for(&today_utc())
}

/// Prefixes a gateway prepends to name the endpoint it routed through. They
/// describe the route, not the model, so they only ever split one model across
/// several rows in the breakdown.
const VENDOR_PREFIXES: &[&str] = &[
    "anthropic", "openai", "azure", "bedrock", "vertex", "google", "xai",
    "moonshot", "zhipu", "kiro", "litellm", "omniroute", "openrouter",
];

/// Stable identity for a model, for use as an aggregation key.
///
/// A proxy can emit the same model under several spellings, and each distinct
/// string becomes its own row in the model breakdown — `claude-opus-4-8` and
/// `claude-opus-4.8` show up as two models, as do `gpt-5.6-sol` and
/// `anthropic-gpt-5.6-sol`. Folding to one form collapses them back into one row.
///
/// Two prefixes are dropped:
/// - a leading `vendor/` path segment (`kiro/claude-opus-5` → `claude-opus-5`),
///   which is unambiguously routing metadata;
/// - a leading `vendor-` token *only when the remainder belongs to a different
///   vendor's family* (`anthropic-gpt-5.6-sol` → `gpt-5-6-sol`). That guard is
///   what keeps `claude-opus-5` intact: stripping `claude-` would leave `opus-5`,
///   an Anthropic family, so no strip happens.
///
/// The result still matches `pricing.json` patterns, which are compared in the
/// same canonical form — every table's patterns are bare model families with no
/// vendor path, so dropping the prefix cannot cost a match.
pub fn normalize_model_id(model: &str) -> String {
    let canonical = canonical_model(model);
    // Routing path segment: the model is whatever follows the last slash.
    let without_path = canonical.rsplit('/').next().unwrap_or(&canonical);
    // Strip a vendor token only when what follows still names a model family.
    // `moonshot` is both a vendor and part of real model ids (`moonshot-v1-8k`),
    // and there the remainder (`v1-8k`) names no family — so it is left alone.
    for vendor in VENDOR_PREFIXES {
        if let Some(rest) = without_path.strip_prefix(&format!("{}-", vendor)) {
            if names_a_family(rest) {
                return rest.to_string();
            }
        }
    }
    without_path.to_string()
}

/// Anthropic model families, as they appear inside a model id.
const ANTHROPIC_FAMILIES: &[&str] =
    &["claude", "opus", "sonnet", "haiku", "fable", "mythos"];

/// True when `id` names a recognizable model family (Anthropic or otherwise).
/// Used to tell a vendor prefix apart from a vendor name that is genuinely part
/// of the model id.
fn names_a_family(id: &str) -> bool {
    !id.is_empty()
        && (ANTHROPIC_FAMILIES.iter().any(|f| id.contains(f))
            || foreign_table(id).is_some()
            || OTHER_FAMILIES.iter().any(|f| id.contains(f)))
}

/// Which non-Anthropic pricing table a model id belongs to, if any.
///
/// Claude Code's log records whatever model id the endpoint reported, so a
/// gateway pointing it at a non-Anthropic model leaves a foreign id in a
/// Claude-shaped log (omniroute writes `anthropic-gpt-5.6-sol` — an OpenAI model
/// behind an Anthropic-compatible endpoint). Matching is on the model family in
/// the id, never on the vendor prefix, precisely because that prefix lies here.
///
/// Returns `None` for Anthropic models and for anything unrecognized, so the
/// caller falls through to the claude table and its unmatched-model warning.
fn foreign_table(canonical: &str) -> Option<&'static str> {
    // Anthropic families first: an Anthropic id must never be routed away, even
    // if some future name happens to contain a foreign substring.
    if ANTHROPIC_FAMILIES.iter().any(|f| canonical.contains(f)) {
        return None;
    }
    // OpenAI/Codex: gpt-*, o3/o4 reasoning models, and the codex variants.
    if canonical.contains("gpt")
        || canonical.contains("codex")
        || canonical.contains("o3-")
        || canonical.contains("o4-")
    {
        return Some("codex");
    }
    if canonical.contains("grok") {
        return Some("grok");
    }
    if canonical.contains("kimi") || canonical.contains("moonshot") {
        return Some("kimi");
    }
    if canonical.contains("glm") {
        return Some("glm");
    }
    None
}

/// Families that have no pricing table of their own but are still recognizable
/// model names. Only [`names_a_family`] consults these: a vendor prefix in front
/// of `gemini-3` is still routing metadata even though no table prices Gemini.
const OTHER_FAMILIES: &[&str] = &["gemini", "deepseek", "qwen", "llama", "mistral"];

/// Price a non-Anthropic model found in a Claude-shaped log, expressed as
/// [`ClaudePricing`] so the caller's cost math is unchanged.
///
/// These providers quote a single discounted rate for cached input rather than
/// Anthropic's separate read/write rates, so `cached_input` maps onto
/// `cache_read` and the cache-write rates are zero — matching how the codex and
/// grok providers already bill their own logs.
fn foreign_model_pricing(model: &str) -> Option<ClaudePricing> {
    let canonical = canonical_model(model);
    let table = foreign_table(&canonical)?;
    let cfg = config();
    let (provider, cached_as_read) = match table {
        "codex" => (&cfg.codex, true),
        "grok" => (cfg.grok.as_ref()?, true),
        "kimi" => (cfg.kimi.as_ref()?, false),
        "glm" => (cfg.glm.as_ref()?, false),
        _ => return None,
    };
    let p = resolved_pricing(provider, table, model);
    Some(ClaudePricing {
        input: p.input,
        output: p.output,
        cache_read: if cached_as_read { p.cached_input } else { p.cache_read },
        cache_write_5m: p.cache_write,
        cache_write_1h: if p.cache_write_1h > 0.0 { p.cache_write_1h } else { p.cache_write },
    })
}

pub fn get_claude_pricing(model: &str) -> ClaudePricing {
    // Claude Code's log can carry non-Anthropic models when a proxy/gateway
    // routes them (omniroute reports e.g. `anthropic-gpt-5.6-sol`). Those have
    // no entry in the claude table, so without this they'd silently bill at the
    // Sonnet default. Route by model family, not by which log file it came from.
    if let Some(p) = foreign_model_pricing(model) {
        return p;
    }
    let p = resolved_pricing(&config().claude, "claude", model);
    ClaudePricing {
        input: p.input,
        output: p.output,
        cache_read: p.cache_read,
        cache_write_5m: p.cache_write,
        cache_write_1h: if p.cache_write_1h > 0.0 { p.cache_write_1h } else { p.cache_write },
    }
}

pub fn get_codex_pricing(model: &str) -> CodexPricing {
    let p = resolved_pricing(&config().codex, "codex", model);
    CodexPricing {
        input: p.input,
        output: p.output,
        cached_input: p.cached_input,
    }
}

pub fn get_kimi_pricing(model: &str) -> KimiPricing {
    let cfg = config();
    if let Some(ref kimi) = cfg.kimi {
        let p = resolved_pricing(kimi, "kimi", model);
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
        let p = resolved_pricing(glm, "glm", model);
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
        let p = resolved_pricing(grok, "grok", model);
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
    // Fallback defaults (Grok 4.6 rates) when pricing.json has no grok section.
    GrokPricing {
        input: 2.00,
        output: 6.00,
        cached_input: 0.50,
        high_threshold_tokens: 200_000,
        high_input: 4.00,
        high_output: 12.00,
        high_cached_input: 1.00,
    }
}

/// Claude-log cost lookup that applies Grok's per-request high-context tier.
///
/// [`get_claude_pricing`] can only return one flat rate, so a Grok id found in
/// a Claude-shaped log would otherwise skip the 200k doubling. Call this at
/// cost time with the full prompt size (`input + cache_read`).
pub fn claude_pricing_for(model: &str, prompt_tokens: u64) -> ClaudePricing {
    let canonical = canonical_model(model);
    if foreign_table(&canonical) == Some("grok") {
        let tier = get_grok_pricing(model).tier_for(prompt_tokens);
        return ClaudePricing {
            input: tier.input,
            output: tier.output,
            cache_read: tier.cached_input,
            cache_write_5m: 0.0,
            cache_write_1h: 0.0,
        };
    }
    get_claude_pricing(model)
}

pub fn get_opencode_pricing(model: &str) -> OpenCodePricing {
    let cfg = config();
    // Use dedicated opencode pricing if available, otherwise try to match
    // against claude or codex pricing tables based on model name.
    if let Some(ref oc) = cfg.opencode {
        let p = resolved_pricing(oc, "opencode", model);
        return OpenCodePricing {
            input: p.input,
            output: p.output,
            cache_read: p.cache_read,
            cache_write: p.cache_write,
        };
    }

    // Fallback: try claude pricing first (for claude-* models), then codex
    if model.contains("claude") || model.contains("fable") || model.contains("mythos") || model.contains("sonnet") || model.contains("opus") || model.contains("haiku") {
        let p = resolved_pricing(&cfg.claude, "claude", model);
        OpenCodePricing {
            input: p.input,
            output: p.output,
            cache_read: p.cache_read,
            cache_write: p.cache_write,
        }
    } else {
        let p = resolved_pricing(&cfg.codex, "codex", model);
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grok: Vec<PricingRow>,
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
        // Grok quotes a discounted cached-input rate, like Codex — so the cache
        // column reads from cached_input rather than cache_read.
        grok: cfg.grok.as_ref().map(|g| deduplicated_rows(g, true)).unwrap_or_default(),
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
    fn grok_unknown_model_defaults_to_46() {
        let unknown = get_grok_pricing("grok-9.9-experimental");
        assert_eq!(unknown.input, get_grok_pricing("grok-4.6").input);
        assert_eq!(unknown.cached_input, get_grok_pricing("grok-4.6").cached_input);
    }

    #[test]
    fn grok_46_not_billed_as_45() {
        let p46 = get_grok_pricing("grok-4.6");
        let p45 = get_grok_pricing("grok-4.5");
        assert_eq!(p46.input, p45.input);
        assert_eq!(p46.output, p45.output);
        assert!(p46.cached_input > p45.cached_input);
        assert_eq!(p46.cached_input, 0.50);
        assert_eq!(p46.high_cached_input, 1.00);
    }

    #[test]
    fn grok_46_build_variant_bills_as_46() {
        let variant = get_grok_pricing("grok-4.6-build");
        let base = get_grok_pricing("grok-4.6");
        assert_eq!(variant.cached_input, base.cached_input);
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

    // Regression guard: "claude-fable-5-1" contains the substring "fable-5", so
    // without its own entry it would fall back to Fable 5 and bill cache reads
    // at $1.00 instead of $0.25 (Fable 5.1 cut cache reads to 0.025x base input;
    // input/output/cache writes are unchanged). The 5-1 entry must sit above 5.
    #[test]
    fn claude_fable_51_cache_read_not_billed_as_fable_5() {
        let p = get_claude_pricing("claude-fable-5-1");
        assert!((p.input - 10.0).abs() < 0.001, "Fable 5.1 input must be $10/MTok, got ${}", p.input);
        assert!((p.output - 50.0).abs() < 0.001, "Fable 5.1 output must be $50/MTok, got ${}", p.output);
        assert!((p.cache_read - 0.25).abs() < 0.001, "Fable 5.1 cache read must be $0.25/MTok, got ${}", p.cache_read);
        assert!((p.cache_write_5m - 12.5).abs() < 0.001);
        assert!((p.cache_write_1h - 20.0).abs() < 0.001);
    }

    #[test]
    fn opencode_fable_51_cache_read_not_billed_as_fable_5() {
        let p = get_opencode_pricing("anthropic/claude-fable-5-1");
        assert!((p.input - 10.0).abs() < 0.001, "Opencode Fable 5.1 input must be $10/MTok, got ${}", p.input);
        assert!((p.cache_read - 0.25).abs() < 0.001, "Opencode Fable 5.1 cache read must be $0.25/MTok, got ${}", p.cache_read);
    }

    #[test]
    fn claude_mythos_51_cache_read_not_billed_as_mythos_5() {
        let p = get_claude_pricing("claude-mythos-5-1");
        assert!((p.input - 10.0).abs() < 0.001, "Mythos 5.1 input must be $10/MTok, got ${}", p.input);
        assert!((p.cache_read - 0.25).abs() < 0.001, "Mythos 5.1 cache read must be $0.25/MTok, got ${}", p.cache_read);
    }

    // Plain "claude-fable-5" (and 1M-context "claude-fable-5-1[1m]") must keep
    // landing on their own entries after the 5-1 entry was added above them.
    #[test]
    fn claude_fable_51_ordering_is_safe() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let fable5 = find_pricing(&cfg.claude, "claude-fable-5");
        assert_eq!(fable5.label, "Fable 5");
        assert!((fable5.cache_read - 1.00).abs() < 0.001, "Fable 5 cache read must stay $1.00/MTok");
        let fable51_1m = find_pricing(&cfg.claude, "claude-fable-5-1[1m]");
        assert_eq!(fable51_1m.label, "Fable 5.1");
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

    // Anthropic cancelled the scheduled 2026-09-01 increase to $3/$15 — the
    // introductory $2/$10 is now the standard price. The `scheduled` override
    // was removed, so on/after 9/1 the price must stay $2/$10.
    #[test]
    fn claude_sonnet_5_stays_at_2_10_after_cancelled_increase() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let entry = find_pricing(&cfg.claude, "claude-sonnet-5");

        let on = entry.resolve_for("2026-09-01");
        assert!((on.input - 2.0).abs() < 0.001, "on 9/1 input must stay $2, got ${}", on.input);
        assert!((on.output - 10.0).abs() < 0.001, "on 9/1 output must stay $10, got ${}", on.output);
        assert!((on.cache_read - 0.20).abs() < 0.001);
        assert!((on.cache_write - 2.50).abs() < 0.001);
        assert!((on.cache_write_1h - 4.0).abs() < 0.001);

        let after = entry.resolve_for("2027-01-01");
        assert!((after.input - 2.0).abs() < 0.001);
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
        // The scheduled 9/1 increase was cancelled — $2/$10 stays after that date.
        let standard = entry.resolve_for("2026-09-01");
        assert!((standard.input - 2.0).abs() < 0.001, "Opencode Sonnet 5 input must stay $2/MTok, got ${}", standard.input);
        assert!((standard.output - 10.0).abs() < 0.001);
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

    // --- Proxy/gateway model id normalization ---

    #[test]
    fn canonical_model_folds_case_dots_and_underscores() {
        assert_eq!(canonical_model("claude-opus-4.8"), "claude-opus-4-8");
        assert_eq!(canonical_model("CLAUDE5_SONNET"), "claude5-sonnet");
        assert_eq!(canonical_model("claude-opus-4-8"), "claude-opus-4-8");
        // Vendor prefixes survive canonicalization: patterns match as
        // substrings, so `kiro/claude-opus-5` still contains `opus-5`.
        // Stripping the prefix is normalize_model_id's job, not this one's.
        assert_eq!(canonical_model("kiro/claude-opus-5"), "kiro/claude-opus-5");
    }

    // Regression guard: omniroute reports Opus 4.8 as `claude-opus-4.8` (dots).
    // Before canonical matching, `"claude-opus-4.8".contains("opus-4-8")` was
    // false, so it fell through to the broader `opus-4` entry and billed at Opus
    // 4.1 rates ($15/$75) — a 3x overcharge on every such message.
    #[test]
    fn claude_dotted_opus_48_not_billed_as_41() {
        let dotted = get_claude_pricing("claude-opus-4.8");
        assert!((dotted.input - 5.0).abs() < 0.001, "dotted Opus 4.8 input must be $5/MTok, got ${}", dotted.input);
        assert!((dotted.output - 25.0).abs() < 0.001, "dotted Opus 4.8 output must be $25/MTok, got ${}", dotted.output);

        // Both spellings must price identically.
        let hyphenated = get_claude_pricing("claude-opus-4-8");
        assert!((dotted.input - hyphenated.input).abs() < 0.001);
        assert!((dotted.output - hyphenated.output).abs() < 0.001);
        assert!((dotted.cache_read - hyphenated.cache_read).abs() < 0.001);
        assert!((dotted.cache_write_5m - hyphenated.cache_write_5m).abs() < 0.001);
        assert!((dotted.cache_write_1h - hyphenated.cache_write_1h).abs() < 0.001);
    }

    // Dotted spellings must not shadow neighbouring versions: `claude-opus-4.1`
    // still has to reach the Opus 4.1 entry, not the 4.8 one.
    #[test]
    fn claude_dotted_opus_41_still_resolves_to_41() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let entry = find_pricing(&cfg.claude, "claude-opus-4.1");
        assert_eq!(entry.label, "Opus 4.1/4");
        let p = get_claude_pricing("claude-opus-4.1");
        assert!((p.input - 15.0).abs() < 0.001, "Opus 4.1 input must stay $15/MTok, got ${}", p.input);
    }

    // Upper snake case ids (`CLAUDE5_SONNET`) matched nothing at all before
    // canonicalization and fell to the default. Now they reach the Sonnet entry.
    #[test]
    fn claude_upper_snake_case_id_matches_sonnet() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let entry = find_pricing(&cfg.claude, "CLAUDE5_SONNET");
        assert_eq!(entry.label, "Sonnet 4.x");
    }

    // Canonicalization must not disturb any existing resolution. Every pattern,
    // used as a model id, has to land on the entry it did before.
    #[test]
    fn canonical_matching_preserves_existing_resolutions() {
        let cfg: PricingConfig = serde_json::from_str(EMBEDDED_PRICING).unwrap();
        let tables: Vec<(&str, &ProviderConfig)> = vec![
            ("claude", &cfg.claude),
            ("codex", &cfg.codex),
            ("opencode", cfg.opencode.as_ref().unwrap()),
            ("kimi", cfg.kimi.as_ref().unwrap()),
            ("glm", cfg.glm.as_ref().unwrap()),
            ("grok", cfg.grok.as_ref().unwrap()),
        ];
        for (name, provider) in tables {
            // No two patterns may collapse to the same canonical form — that
            // would let one silently shadow the other.
            let mut seen = std::collections::HashSet::new();
            for e in &provider.models {
                assert!(
                    seen.insert(canonical_model(&e.match_pattern)),
                    "[{}] pattern {:?} collides with another after canonicalization",
                    name, e.match_pattern
                );
            }
            // Each pattern, treated as a model id, still resolves to itself
            // (first match wins, so the first pattern containing it may differ —
            // assert against raw matching, which is the pre-change behaviour).
            for e in &provider.models {
                let raw = provider
                    .models
                    .iter()
                    .find(|c| e.match_pattern.contains(&c.match_pattern))
                    .map(|c| c.match_pattern.as_str());
                let canon = find_pricing_in(provider, name, &e.match_pattern).match_pattern.as_str();
                assert_eq!(
                    raw, Some(canon),
                    "[{}] {:?} resolves differently under canonical matching",
                    name, e.match_pattern
                );
            }
        }
    }

    // --- Aggregation-key normalization ---

    // The point of normalization: spellings a gateway emits for one model must
    // collapse to one key, or the breakdown shows the same model twice.
    #[test]
    fn normalize_model_id_collapses_gateway_spellings() {
        // Dotted vs hyphenated.
        assert_eq!(normalize_model_id("claude-opus-4.8"), normalize_model_id("claude-opus-4-8"));
        // Vendor path segment.
        assert_eq!(normalize_model_id("kiro/claude-opus-5"), normalize_model_id("claude-opus-5"));
        assert_eq!(normalize_model_id("claude/claude-opus-5"), normalize_model_id("claude-opus-5"));
        // Contradicting vendor token on a foreign model.
        assert_eq!(normalize_model_id("anthropic-gpt-5.6-sol"), normalize_model_id("gpt-5.6-sol"));
        // Case and underscores.
        assert_eq!(normalize_model_id("CLAUDE-OPUS-5"), normalize_model_id("claude-opus-5"));
    }

    // Distinct models must stay distinct — normalization must not over-merge.
    #[test]
    fn normalize_model_id_keeps_distinct_models_apart() {
        let ids = [
            "claude-opus-5", "claude-opus-4-8", "claude-opus-4-1", "claude-sonnet-5",
            "claude-sonnet-4-6", "claude-haiku-4-5-20251001", "claude-fable-5",
            "claude-mythos-5", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5-codex", "grok-4.5", "grok-4.6",
        ];
        let mut seen = std::collections::HashMap::new();
        for id in ids {
            let key = normalize_model_id(id);
            if let Some(prev) = seen.insert(key.clone(), id) {
                panic!("{id:?} and {prev:?} both normalize to {key:?}");
            }
        }
    }

    // The Anthropic-family guard: `claude-` is in VENDOR_PREFIXES via `kiro`-style
    // handling, but stripping a family token would turn `claude-opus-5` into
    // `opus-5` and split it from ids that keep the prefix.
    #[test]
    fn normalize_model_id_preserves_anthropic_family_tokens() {
        assert_eq!(normalize_model_id("claude-opus-5"), "claude-opus-5");
        assert_eq!(normalize_model_id("claude-sonnet-5"), "claude-sonnet-5");
        // A vendor prefix in front of an Anthropic model is dropped, but the
        // Anthropic id underneath is left whole.
        assert_eq!(normalize_model_id("bedrock/claude-opus-5"), "claude-opus-5");
    }

    // `moonshot` is both a vendor name and a real model-id prefix
    // (`moonshot-v1-8k`). Stripping it there would destroy the id, so the strip
    // only fires when what remains still names a family.
    #[test]
    fn normalize_model_id_keeps_vendor_names_that_are_part_of_the_id() {
        assert_eq!(normalize_model_id("moonshot-v1-8k"), "moonshot-v1-8k");
        // With a family after it, the prefix is routing metadata and goes.
        assert_eq!(normalize_model_id("moonshot-kimi-k2"), "kimi-k2");
    }

    // Normalized ids must still price correctly — normalization feeds the same
    // lookup, so a normalized key must not lose its pricing entry.
    #[test]
    fn normalized_ids_still_resolve_to_the_same_price() {
        for id in [
            "claude-opus-4.8", "kiro/claude-opus-5", "anthropic-gpt-5.6-sol",
            "CLAUDE-OPUS-5", "claude-sonnet-5", "grok-4.5",
        ] {
            let raw = get_claude_pricing(id);
            let normalized = get_claude_pricing(&normalize_model_id(id));
            assert!(
                (raw.input - normalized.input).abs() < 0.001
                    && (raw.output - normalized.output).abs() < 0.001,
                "{id:?} prices differently after normalization: ${}/${} vs ${}/${}",
                raw.input, raw.output, normalized.input, normalized.output
            );
        }
    }

    // Normalization is idempotent — re-normalizing a stored key is a no-op, so a
    // cache written with normalized keys stays stable across versions.
    #[test]
    fn normalize_model_id_is_idempotent() {
        for id in [
            "claude-opus-4.8", "kiro/claude-opus-5", "anthropic-gpt-5.6-sol",
            "moonshot-v1-8k", "CLAUDE5_SONNET", "gpt-5.6-sol",
        ] {
            let once = normalize_model_id(id);
            assert_eq!(normalize_model_id(&once), once, "{id:?} is not idempotent");
        }
    }

    // --- Foreign models in Claude Code's log ---

    // Regression guard: omniroute routes Claude Code at OpenAI models and logs
    // them as `anthropic-gpt-5.6-sol` — an OpenAI model wearing an Anthropic
    // prefix. Billing it from the claude table meant no pattern matched and it
    // fell to the Sonnet default ($3/$15) instead of GPT-5.6 Sol's $5/$30.
    #[test]
    fn claude_log_gpt56_sol_billed_from_codex_table() {
        let codex = get_codex_pricing("gpt-5.6-sol");
        for id in ["gpt-5.6-sol", "anthropic-gpt-5.6-sol"] {
            let p = get_claude_pricing(id);
            assert!((p.input - codex.input).abs() < 0.001, "{id} input must be ${}, got ${}", codex.input, p.input);
            assert!((p.output - codex.output).abs() < 0.001, "{id} output must be ${}, got ${}", codex.output, p.output);
            // OpenAI quotes one discounted cached-input rate, mapped onto cache_read.
            assert!((p.cache_read - codex.cached_input).abs() < 0.001);
            assert!(p.cache_write_5m.abs() < 0.001, "{id} must have no cache-write rate");
        }
    }

    #[test]
    fn claude_log_foreign_families_route_to_their_tables() {
        let grok = get_claude_pricing("grok-4.6");
        let grok_ref = get_grok_pricing("grok-4.6");
        assert!((grok.input - grok_ref.input).abs() < 0.001);
        assert!((grok.output - grok_ref.output).abs() < 0.001);
        assert!((grok.cache_read - grok_ref.cached_input).abs() < 0.001);

        let grok_high = claude_pricing_for("grok-4.6", 200_000);
        assert!((grok_high.input - grok_ref.high_input).abs() < 0.001);
        assert!((grok_high.cache_read - grok_ref.high_cached_input).abs() < 0.001);
        let grok_low = claude_pricing_for("grok-4.6", 199_999);
        assert!((grok_low.cache_read - grok_ref.cached_input).abs() < 0.001);

        let kimi = get_claude_pricing("kimi-k2");
        let kimi_ref = get_kimi_pricing("kimi-k2");
        assert!((kimi.input - kimi_ref.input).abs() < 0.001);
        assert!((kimi.output - kimi_ref.output).abs() < 0.001);

        let codex = get_claude_pricing("gpt-5-codex");
        assert!((codex.input - 1.25).abs() < 0.001, "gpt-5-codex must bill at $1.25, got ${}", codex.input);
    }

    // Anthropic ids must never be routed away, and unknown ids must stay on the
    // claude table so the default fallback (and its warning) still applies.
    #[test]
    fn anthropic_and_unknown_ids_stay_on_claude_table() {
        for id in [
            "claude-opus-5", "claude-sonnet-5", "claude-fable-5", "claude-mythos-5",
            "claude-haiku-4-5-20251001", "opus", "sonnet", "haiku", "fable",
            "kiro/claude-opus-5", "claude/claude-opus-5",
        ] {
            assert!(
                foreign_table(&canonical_model(id)).is_none(),
                "{id} must stay on the claude table"
            );
        }
        assert_eq!(foreign_table(&canonical_model("some-unknown-model")), None);

        // Vendor-prefixed foreign ids still route by family, not by prefix.
        assert_eq!(foreign_table(&canonical_model("anthropic-gpt-5.6-sol")), Some("codex"));
        assert_eq!(foreign_table(&canonical_model("openai/gpt-5.6-terra")), Some("codex"));
    }

    // A Claude id whose spelling comes from a proxy must still price as Anthropic.
    #[test]
    fn claude_pricing_unchanged_for_known_anthropic_ids() {
        let opus5 = get_claude_pricing("claude-opus-5");
        assert!((opus5.input - 5.0).abs() < 0.001);
        assert!((opus5.output - 25.0).abs() < 0.001);
        // Vendor-prefixed Claude ids price identically to the bare id.
        let prefixed = get_claude_pricing("kiro/claude-opus-5");
        assert!((prefixed.input - opus5.input).abs() < 0.001);
        assert!((prefixed.output - opus5.output).abs() < 0.001);
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
