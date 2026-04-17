use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::traits::TokenProvider;
use super::types::AllStats;

// --- Cache infrastructure (stub — minimal for future use) ---

struct StubCache {
    stats: AllStats,
    computed_at: Instant,
}

static STATS_CACHE: Mutex<Option<StubCache>> = Mutex::new(None);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Invalidate cache — called by file watcher.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse.
pub fn get_cached_stats() -> Option<AllStats> {
    STATS_CACHE.lock().ok()?.as_ref().map(|c| c.stats.clone())
}

// --- Provider ---

/// GLM (Zhipu AI) provider stub.
/// Currently no widely-used CLI agent with local session storage exists for GLM.
/// This provider is a placeholder that returns empty stats and is_available() = false
/// until a GLM CLI tool is established.
///
/// Expected future session path: ~/.glm/sessions/ or ~/.zhipu/sessions/
/// Expected wire format: JSONL with OpenAI-compatible usage fields
pub struct GlmProvider {
    #[allow(dead_code)]
    glm_dir: PathBuf,
    #[allow(dead_code)]
    zhipu_dir: PathBuf,
}

impl GlmProvider {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            glm_dir: home.join(".glm"),
            zhipu_dir: home.join(".zhipu"),
        }
    }
}

impl TokenProvider for GlmProvider {
    fn name(&self) -> &str {
        "GLM"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);

        if !was_invalidated {
            if let Ok(cache) = STATS_CACHE.lock() {
                if let Some(ref cached) = *cache {
                    if cached.computed_at.elapsed() < CACHE_TTL {
                        return Ok(cached.stats.clone());
                    }
                }
            }
        }

        let stats = AllStats {
            daily: vec![],
            model_usage: HashMap::new(),
            total_sessions: 0,
            total_messages: 0,
            first_session_date: None,
            analytics: None,
        };

        if let Ok(mut cache) = STATS_CACHE.lock() {
            *cache = Some(StubCache {
                stats: stats.clone(),
                computed_at: Instant::now(),
            });
        }

        Ok(stats)
    }

    fn is_available(&self) -> bool {
        // Gated off until parsing is implemented. Detecting ~/.glm or ~/.zhipu
        // would surface the toggle while fetch_stats() still returns an empty
        // AllStats — users would see "0 tokens" with no explanation. Flip this
        // to an actual directory check once wire.jsonl parsing lands.
        false
    }
}
