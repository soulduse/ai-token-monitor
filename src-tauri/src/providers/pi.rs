//! Pi provider: usage stats from pi-coding-agent (`@mariozechner/pi-coding-agent`)
//! session logs, parsed by the shared `pi_session` engine.
//!
//! Layout (pi-mono `config.ts` / `session-manager.ts`):
//! - agent dir = `$PI_CODING_AGENT_DIR` (if set and non-empty) else `~/.pi/agent`
//! - sessions: `<agent dir>/sessions/--<cwd with / replaced by ->--/<ts>_<id>.jsonl`
//!   (fixed depth 1 subdir, one per project)
//! - forks (`--fork`, `/fork`) copy history into a new file, so the same
//!   `responseId` appears in several files; the engine counts it once.
//! - no child-session tree: Pi's subagent example runs children with
//!   `--no-session`, so nothing is written outside the sessions root.

use std::path::PathBuf;

use super::pi_session::{self, SessionLayout, SessionStatsCache};
use super::traits::TokenProvider;
use super::types::AllStats;

#[derive(Clone, Copy, Default)]
pub(super) struct Pi;

impl SessionLayout for Pi {
    const NAME: &'static str = "Pi";
    const LOG_TAG: &'static str = "PI";
    const FALLBACK_MODEL: &'static str = "pi";
    const CHILDREN_GLOB: Option<&'static str> = None;
}

// --- Agent dir resolution ---

/// Pure resolver: `$PI_CODING_AGENT_DIR` when set and non-empty, else `~/.pi/agent`.
fn resolve_agent_dir(env_value: Option<&str>) -> PathBuf {
    pi_session::resolve_agent_dir(env_value, ".pi")
}

/// Sessions root scanned by the provider: env-or-default agent dir + `sessions`.
pub fn default_sessions_root() -> PathBuf {
    resolve_agent_dir(std::env::var("PI_CODING_AGENT_DIR").ok().as_deref()).join("sessions")
}

// --- Cache ---

static CACHE: SessionStatsCache<Pi> = SessionStatsCache::new();

/// Invalidate cache — called by the file watcher on agent-dir changes.
pub fn invalidate_stats_cache() {
    CACHE.invalidate();
}

/// Return cached stats without triggering a re-parse (used by tray updates).
pub fn get_cached_stats() -> Option<AllStats> {
    CACHE.cached_stats()
}

// --- Provider ---

pub struct PiProvider {
    agent_dir: PathBuf,
}

impl PiProvider {
    pub fn new() -> Self {
        Self::with_agent_dir(resolve_agent_dir(
            std::env::var("PI_CODING_AGENT_DIR").ok().as_deref(),
        ))
    }

    pub fn with_agent_dir(dir: PathBuf) -> Self {
        Self { agent_dir: dir }
    }

    fn sessions_root(&self) -> PathBuf {
        self.agent_dir.join("sessions")
    }
}

impl Default for PiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenProvider for PiProvider {
    fn name(&self) -> &str {
        "Pi"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        CACHE.fetch(&self.sessions_root())
    }

    fn is_available(&self) -> bool {
        self.sessions_root().exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    type ScanState = pi_session::ScanState<Pi>;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pi-provider-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(path, contents).expect("write file");
    }

    fn session_header(cwd: &Path, id: &str, parent: Option<&Path>) -> String {
        let mut header = json!({
            "type": "session",
            "version": 3,
            "id": id,
            "timestamp": "2026-09-01T09:00:00.000Z",
            "cwd": cwd.to_string_lossy(),
        });
        if let Some(parent) = parent {
            header["parentSession"] = json!(parent.to_string_lossy());
        }
        header.to_string()
    }

    fn assistant_line(response_id: &str, timestamp: &str, model: Option<&str>, input: u64) -> String {
        let mut message = json!({
            "role": "assistant", "responseId": response_id,
            "usage": {"input": input, "output": 4, "cacheRead": 100, "cacheWrite": 6,
                      "totalTokens": input + 110, "reasoning": 2,
                      "cost": {"input": 0.0, "output": 0.0, "cacheRead": 0.0, "cacheWrite": 0.0, "total": 0.5}}
        });
        if let Some(model) = model {
            message["model"] = json!(model);
        }
        json!({"type": "message", "id": "a1b2c3d4", "parentId": null, "timestamp": timestamp, "message": message})
            .to_string()
    }

    /// Pi's real layout: one `--<encoded cwd>--` dir per project, files named
    /// `<ts>_<id>.jsonl`. A fork copies history (same responseIds, re-stamped
    /// later) into a new file; the copy counts once, on the original day.
    /// OmO's child tree under the project cwd is NOT Pi's and is ignored.
    #[test]
    fn pi_layout_counts_forks_once_and_ignores_child_trees() {
        let root = temp_root("layout");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let project_dir = sessions.join("--tmp-proj--");
        let original = project_dir.join("2026-09-01T09-00-00-000Z_0001.jsonl");
        let fork = project_dir.join("2026-09-05T09-00-00-000Z_0002.jsonl");

        write_file(
            &original,
            &[
                session_header(&cwd, "0001", None),
                assistant_line("msg_1", "2026-09-01T12:00:00.000Z", Some("claude-opus-4.8"), 10),
                assistant_line("msg_2", "2026-09-01T12:01:00.000Z", None, 20),
            ]
            .join("\n"),
        );
        write_file(
            &fork,
            &[
                session_header(&cwd, "0002", Some(&original)),
                assistant_line("msg_1", "2026-09-05T12:00:00.000Z", Some("claude-opus-4.8"), 10),
                assistant_line("msg_3", "2026-09-05T12:02:00.000Z", Some("claude-opus-4.8"), 30),
            ]
            .join("\n"),
        );
        // Would be an OmO child session; Pi has no child discovery.
        write_file(
            &cwd.join(".omo/senpi-task/children/st_1/sessions/st_1/c.jsonl"),
            &[session_header(&cwd, "child", None), assistant_line("msg_c", "2026-09-01T12:00:00.000Z", None, 900)]
                .join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 3, "msg_1 (forked copy) counts once");
        let input: u64 = stats.daily.iter().map(|d| d.input_tokens).sum();
        let output: u64 = stats.daily.iter().map(|d| d.output_tokens).sum();
        assert_eq!(input, 60, "10 + 20 + 30; child tree excluded");
        assert_eq!(output, 12, "reasoning is part of output, never added");
        let tokens: u64 = stats.daily.iter().flat_map(|d| d.tokens.values()).sum();
        assert_eq!(tokens, 60 + 3 * 110);
        assert!((stats.daily.iter().map(|d| d.cost_usd).sum::<f64>() - 1.5).abs() < 1e-9);

        let mut keys: Vec<&str> = stats.model_usage.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["claude-opus-4-8", "pi"], "missing model falls back to \"pi\"");
        assert_eq!(stats.model_usage["claude-opus-4-8"].input_tokens, 40);

        // msg_1 stays on the original's day: two days total, not three.
        assert_eq!(stats.daily.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_dir_resolution_honors_env_and_falls_back_to_home() {
        let home = dirs::home_dir().expect("home dir available in tests");

        assert_eq!(resolve_agent_dir(Some("/custom/pi")), PathBuf::from("/custom/pi"));
        assert_eq!(
            resolve_agent_dir(Some("")),
            home.join(".pi").join("agent"),
            "empty env value is treated as unset"
        );
        assert_eq!(resolve_agent_dir(None), home.join(".pi").join("agent"));

        let env_value = std::env::var("PI_CODING_AGENT_DIR")
            .ok()
            .filter(|v| !v.is_empty());
        assert_eq!(
            default_sessions_root(),
            resolve_agent_dir(env_value.as_deref()).join("sessions")
        );

        let provider = PiProvider::with_agent_dir(PathBuf::from("/opt/pi-agent"));
        assert_eq!(provider.sessions_root(), PathBuf::from("/opt/pi-agent/sessions"));
    }
}
