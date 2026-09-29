//! Pi provider: usage stats from pi-coding-agent (`@mariozechner/pi-coding-agent`)
//! session logs, parsed by the shared `pi_session` engine.
//!
//! Layout (pi-mono `config.ts` / `session-manager.ts` / `main.ts`):
//! - agent dir = `$PI_CODING_AGENT_DIR` (if set and non-empty, leading `~`
//!   expanded) else `~/.pi/agent`
//! - sessions: `<agent dir>/sessions/--<cwd with / replaced by ->--/<ts>_<id>.jsonl`
//!   (fixed depth 1 subdir, one per project)
//! - custom session dir = `--session-dir` > `$PI_CODING_AGENT_SESSION_DIR` >
//!   `sessionDir` in `<agent dir>/settings.json`; pi then writes FLAT
//!   `<dir>/<ts>_<id>.jsonl`. The env var and settings are honoured and scanned
//!   next to the default root; `--session-dir` (per invocation) and the project
//!   `.pi/settings.json` (per cwd) cannot be discovered.
//! - forks (`--fork`, `/fork`) copy history into a new file, so the same
//!   `responseId` appears in several files; the engine counts it once.
//! - no child-session tree: Pi's subagent example runs children with
//!   `--no-session`, so nothing is written outside the sessions root.

use std::path::{Path, PathBuf};

use super::pi_session::{self, SessionDirs, SessionLayout, SessionStatsCache};
use super::traits::TokenProvider;
use super::types::AllStats;

#[derive(Clone, Copy, Default)]
pub(super) struct Pi;

impl SessionLayout for Pi {
    const NAME: &'static str = "Pi";
    const LOG_TAG: &'static str = "PI";
    const FALLBACK_MODEL: &'static str = "pi";
    const CHILDREN_GLOB: Option<&'static str> = None;
    // pi-mono settings-manager.ts: plain `JSON.parse` of `settings.json`.
    const SETTINGS_FILES: &'static [&'static str] = &["settings.json"];
    const SETTINGS_JSONC: bool = false;
}

// --- Dir resolution ---

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Pure resolver mirroring pi's `getAgentDir`: `$PI_CODING_AGENT_DIR` when set
/// and non-empty (leading `~` expanded), else `~/.pi/agent`.
fn resolve_agent_dir(env_value: Option<&str>, home: &Path) -> PathBuf {
    match env_value.filter(|v| !v.is_empty()) {
        Some(dir) => pi_session::expand_tilde(dir, home),
        None => home.join(".pi").join("agent"),
    }
}

/// Session dirs the watcher follows: the default root and the custom flat dir,
/// whichever exist.
pub fn watch_dirs() -> Vec<PathBuf> {
    PiProvider::new().session_dirs().existing().cloned().collect()
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
            &home_dir(),
        ))
    }

    pub fn with_agent_dir(dir: PathBuf) -> Self {
        Self { agent_dir: dir }
    }

    fn sessions_root(&self) -> PathBuf {
        self.agent_dir.join("sessions")
    }

    /// Resolved on every call so a `sessionDir` edited in settings is picked
    /// up without a restart.
    fn session_dirs(&self) -> SessionDirs {
        self.session_dirs_with(
            std::env::var("PI_CODING_AGENT_SESSION_DIR").ok().as_deref(),
            &home_dir(),
        )
    }

    /// Pure part of [`Self::session_dirs`]: env value and home passed in.
    fn session_dirs_with(&self, session_env: Option<&str>, home: &Path) -> SessionDirs {
        let custom =
            pi_session::resolve_custom_session_dir::<Pi>(session_env, &self.agent_dir, home);
        SessionDirs::new(self.sessions_root(), custom)
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
        CACHE.fetch(&self.session_dirs())
    }

    fn is_available(&self) -> bool {
        self.session_dirs().any_exists()
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

    /// `~` expansion follows pi's `normalizePath`: bare `~` and leading `~/`
    /// only; `~user`, `$VAR` and a mid-path `~` stay literal.
    #[test]
    fn agent_dir_resolution_expands_leading_tilde_like_pi() {
        let home = Path::new("/home/u");

        assert_eq!(resolve_agent_dir(Some("/custom/pi"), home), PathBuf::from("/custom/pi"));
        assert_eq!(resolve_agent_dir(Some("~"), home), PathBuf::from("/home/u"));
        assert_eq!(resolve_agent_dir(Some("~/pi-agent"), home), PathBuf::from("/home/u/pi-agent"));
        assert_eq!(resolve_agent_dir(Some("~bob/pi"), home), PathBuf::from("~bob/pi"));
        assert_eq!(resolve_agent_dir(Some("$HOME/pi"), home), PathBuf::from("$HOME/pi"));
        assert_eq!(resolve_agent_dir(Some("/a/~/b"), home), PathBuf::from("/a/~/b"));
        assert_eq!(
            resolve_agent_dir(Some(""), home),
            PathBuf::from("/home/u/.pi/agent"),
            "empty env value is treated as unset"
        );
        assert_eq!(resolve_agent_dir(None, home), PathBuf::from("/home/u/.pi/agent"));
        assert_eq!(
            pi_session::expand_tilde("~\\pi", home),
            if cfg!(windows) { home.join("pi") } else { PathBuf::from("~\\pi") },
            "`~\\` is a home prefix on Windows only"
        );

        let provider = PiProvider::with_agent_dir(PathBuf::from("/opt/pi-agent"));
        assert_eq!(provider.sessions_root(), PathBuf::from("/opt/pi-agent/sessions"));
    }

    /// Custom session dir precedence as pi's `main.ts`: env (non-empty) over
    /// `sessionDir` in `<agent dir>/settings.json`; both `~`-expanded, relative
    /// results dropped, unparsable or missing settings ignored.
    #[test]
    fn custom_session_dir_from_env_or_settings() {
        let root = temp_root("custom-resolve");
        let agent = root.join("agent");
        let home = root.join("home");
        let resolve = |env: Option<&str>| pi_session::resolve_custom_session_dir::<Pi>(env, &agent, &home);

        assert_eq!(resolve(None), None, "no settings file");
        assert_eq!(resolve(Some("/env/sessions")), Some(PathBuf::from("/env/sessions")));

        write_file(&agent.join("settings.json"), "\u{feff}{\"theme\":\"dark\",\"sessionDir\":\"~/pi-sessions\"}");
        assert_eq!(resolve(None), Some(home.join("pi-sessions")), "BOM stripped, `~` expanded");
        assert_eq!(resolve(Some("")), Some(home.join("pi-sessions")), "empty env falls through");
        assert_eq!(resolve(Some("~/env")), Some(home.join("env")), "env wins over settings");
        assert_eq!(resolve(Some("rel/dir")), None, "relative dir depends on pi's cwd");

        write_file(&agent.join("settings.json"), "{\"sessionDir\": \"\"}");
        assert_eq!(resolve(None), None, "empty sessionDir is unset");
        // pi parses settings.json with plain JSON.parse: comments fail it.
        write_file(&agent.join("settings.json"), "{\"sessionDir\": \"/x\" // c\n}");
        assert_eq!(resolve(None), None);
        write_file(&agent.join("settings.jsonc"), "{\"sessionDir\": \"/jsonc\"}");
        assert_eq!(resolve(None), None, "pi never reads settings.jsonc");

        let _ = fs::remove_dir_all(&root);
    }

    /// With a custom dir, pi writes FLAT `<dir>/<ts>_<id>.jsonl`. It is scanned
    /// next to the default tree; a fork copied from the default root into the
    /// custom dir counts once, on the original day. Deeper files are ignored.
    #[test]
    fn flat_custom_dir_is_scanned_with_default_root_and_forks_dedup() {
        let root = temp_root("custom-scan");
        let sessions = root.join("agent").join("sessions");
        let custom = root.join("flat sessions [x]");
        let cwd = root.join("proj");
        let original = sessions.join("--tmp-proj--").join("2026-09-01T09-00-00-000Z_0001.jsonl");

        write_file(
            &original,
            &[
                session_header(&cwd, "0001", None),
                assistant_line("msg_1", "2026-09-01T12:00:00.000Z", Some("claude-opus-4.8"), 10),
            ]
            .join("\n"),
        );
        write_file(
            &custom.join("2026-09-05T09-00-00-000Z_0002.jsonl"),
            &[
                session_header(&cwd, "0002", Some(&original)),
                assistant_line("msg_1", "2026-09-05T12:00:00.000Z", Some("claude-opus-4.8"), 10),
                assistant_line("msg_2", "2026-09-05T12:01:00.000Z", Some("claude-opus-4.8"), 20),
            ]
            .join("\n"),
        );
        write_file(
            &custom.join("nested").join("x.jsonl"),
            &[session_header(&cwd, "n", None), assistant_line("msg_n", "2026-09-05T12:00:00.000Z", None, 900)]
                .join("\n"),
        );

        let dirs = SessionDirs::new(sessions.clone(), Some(custom.clone()));
        let mut state = ScanState::default();
        let stats = state.refresh_dirs(&dirs);
        assert_eq!(stats.total_messages, 2, "msg_1 counts once, nested file ignored");
        assert_eq!(stats.daily.iter().map(|d| d.input_tokens).sum::<u64>(), 30);
        assert_eq!(stats.daily.len(), 2, "msg_1 stays on the original's day");

        // Custom-only history (default root gone) is still found.
        fs::remove_dir_all(&sessions).expect("remove default root");
        let stats = state.refresh_dirs(&dirs);
        assert_eq!(stats.daily.iter().map(|d| d.input_tokens).sum::<u64>(), 30);

        // Dropping the override forgets the flat dir, as a fresh scan would.
        let stats = state.refresh_dirs(&SessionDirs::new(sessions.clone(), None));
        assert_eq!(stats.total_messages, 0);

        let _ = fs::remove_dir_all(&root);
    }

    /// Availability and watcher dirs cover the default root and the custom
    /// dir from settings.json, whichever exist, without duplicates.
    #[test]
    fn session_dirs_drive_availability_and_watch_list() {
        let root = temp_root("watch");
        let agent = root.join("agent");
        let home = root.join("home");
        let custom = home.join("pi-sessions");
        let provider = PiProvider::with_agent_dir(agent.clone());
        let watched = |env: Option<&str>| -> Vec<PathBuf> {
            provider.session_dirs_with(env, &home).existing().cloned().collect()
        };

        write_file(&agent.join("settings.json"), "{\"sessionDir\": \"~/pi-sessions\"}");
        assert!(!provider.session_dirs_with(None, &home).any_exists());
        assert!(watched(None).is_empty());

        fs::create_dir_all(&custom).expect("create custom dir");
        assert!(provider.session_dirs_with(None, &home).any_exists(), "custom dir alone is enough");
        assert_eq!(watched(None), vec![custom.clone()]);

        fs::create_dir_all(agent.join("sessions")).expect("create sessions root");
        assert_eq!(watched(None), vec![agent.join("sessions"), custom.clone()]);

        let root_str = agent.join("sessions").to_string_lossy().into_owned();
        assert_eq!(watched(Some(&root_str)), vec![agent.join("sessions")], "same dir watched once");

        let _ = fs::remove_dir_all(&root);
    }
}
