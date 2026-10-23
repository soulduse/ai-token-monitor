//! omp provider: usage stats from oh-my-pi (`@oh-my-pi/pi-coding-agent`, CLI
//! `omp`) session logs, parsed by the shared `pi_session` engine.
//!
//! oh-my-pi is a pi-coding-agent fork and writes the same session JSONL
//! (`usage{input, output, cacheRead, cacheWrite, totalTokens, cost{total}}`,
//! `model`, `responseId`). Layout (oh-my-pi 18.x `pi-utils/dirs.ts`,
//! `session/session-paths.ts`, `session/session-manager.ts`):
//! - agent dir = `~/.omp/agent`; a named profile uses
//!   `~/.omp/profiles/<name>/agent`
//! - on macOS/Linux, once `$XDG_DATA_HOME/omp` exists, data moves there and
//!   the `agent/` prefix is flattened: `$XDG_DATA_HOME/omp/sessions`,
//!   `$XDG_DATA_HOME/omp/profiles/<name>/sessions`
//! - sessions: `<agent dir>/sessions/<encoded cwd>/<ts>_<id>.jsonl` (fixed
//!   depth 1 subdir, like Pi)
//! - subagent and advisor transcripts live in the session's artifact dir:
//!   `<ts>_<id>/<SubId>.jsonl`, `<ts>_<id>/__advisor.jsonl`, nested
//!   subagents one level deeper — scanned via `NESTED_CHILD_DEPTH`
//! - `PI_CODING_AGENT_DIR` / `PI_CODING_AGENT_SESSION_DIR` overrides share
//!   their names with Pi. They are deliberately NOT followed here: the Pi
//!   provider already scans those dirs, and following them in both would
//!   count the same sessions twice.

use std::fs;
use std::path::{Path, PathBuf};

use super::pi_session::{SessionDirs, SessionLayout, SessionStatsCache};
use super::traits::TokenProvider;
use super::types::AllStats;

#[derive(Clone, Copy, Default)]
pub(super) struct Omp;

impl SessionLayout for Omp {
    const NAME: &'static str = "omp";
    const LOG_TAG: &'static str = "OMP";
    const FALLBACK_MODEL: &'static str = "omp";
    const CHILDREN_GLOB: Option<&'static str> = None;
    // No custom session dir is resolved (see module doc), so settings are
    // never read; kept to satisfy the layout.
    const SETTINGS_FILES: &'static [&'static str] = &["settings.json"];
    const SETTINGS_JSONC: bool = false;
    // `<SubId>.jsonl` and a nested subagent's own children below it.
    const NESTED_CHILD_DEPTH: usize = 2;
}

// --- Dir resolution ---

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Every sessions root omp may write to, default profile first: the
/// `~/.omp` tree (default + named profiles) and, when `$XDG_DATA_HOME/omp`
/// exists, the flattened XDG tree. Missing dirs are kept; callers filter.
fn sessions_roots(home: &Path, xdg_data_home: Option<&str>) -> Vec<PathBuf> {
    let base = home.join(".omp");
    let mut roots = vec![base.join("agent").join("sessions")];
    roots.extend(profile_dirs(&base).map(|p| p.join("agent").join("sessions")));

    if let Some(xdg) = xdg_data_home.filter(|v| !v.is_empty()) {
        let app = Path::new(xdg).join("omp");
        if app.is_dir() {
            roots.push(app.join("sessions"));
            roots.extend(profile_dirs(&app).map(|p| p.join("sessions")));
        }
    }
    roots
}

/// `<base>/profiles/*` dirs, sorted for a stable scan order.
fn profile_dirs(base: &Path) -> impl Iterator<Item = PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(base.join("profiles"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
}

fn session_dirs_with(home: &Path, xdg_data_home: Option<&str>) -> SessionDirs {
    let mut roots = sessions_roots(home, xdg_data_home).into_iter();
    let root = roots.next().expect("default root is always present");
    SessionDirs::new(root, None).with_extra_roots(roots.collect())
}

/// Resolved on every call so a new profile is picked up without a restart.
fn session_dirs() -> SessionDirs {
    session_dirs_with(&home_dir(), std::env::var("XDG_DATA_HOME").ok().as_deref())
}

/// Session roots the watcher follows (recursively), whichever exist.
pub fn watch_dirs() -> Vec<PathBuf> {
    session_dirs().existing().cloned().collect()
}

// --- Cache ---

static CACHE: SessionStatsCache<Omp> = SessionStatsCache::new();

/// Invalidate cache — called by the file watcher on agent-dir changes.
pub fn invalidate_stats_cache() {
    CACHE.invalidate();
}

/// Return cached stats without triggering a re-parse (used by tray updates).
pub fn get_cached_stats() -> Option<AllStats> {
    CACHE.cached_stats()
}

// --- Provider ---

#[derive(Default)]
pub struct OmpProvider;

impl OmpProvider {
    pub fn new() -> Self {
        Self
    }
}

impl TokenProvider for OmpProvider {
    fn name(&self) -> &str {
        "omp"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        CACHE.fetch(&session_dirs())
    }

    fn is_available(&self) -> bool {
        session_dirs().any_exists()
    }
}

#[cfg(test)]
mod tests {
    use super::super::pi_session::ScanState;
    use super::*;
    use serde_json::json;

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("omp-provider-test-{}-{tag}", std::process::id()));
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

    fn session_header(cwd: &Path, id: &str) -> String {
        json!({
            "type": "session", "version": 3, "id": id,
            "timestamp": "2026-10-01T09:00:00.000Z", "cwd": cwd.to_string_lossy(),
        })
        .to_string()
    }

    /// An assistant line shaped like oh-my-pi's `AssistantMessage`
    /// (`pi-catalog` `Usage`): `reasoningTokens` is a subset of `output`.
    fn assistant_line(response_id: &str, timestamp: &str, model: &str, input: u64) -> String {
        json!({
            "type": "message", "id": "e1", "parentId": null, "timestamp": timestamp,
            "message": {
                "role": "assistant", "api": "anthropic-messages", "provider": "anthropic",
                "model": model, "responseId": response_id,
                "usage": {"input": input, "output": 4, "cacheRead": 100, "cacheWrite": 6,
                          "totalTokens": input + 110, "reasoningTokens": 2,
                          "cost": {"input": 0.0, "output": 0.0, "cacheRead": 0.0, "cacheWrite": 0.0, "total": 0.25}}
            }
        })
        .to_string()
    }

    /// Main sessions plus subagent/advisor transcripts in the artifact dir
    /// (two levels) count; a non-session tree beside them and a dir without a
    /// matching main file do not.
    #[test]
    fn counts_nested_subagent_sessions_of_main_sessions_only() {
        let root = temp_root("nested");
        let sessions = root.join("sessions");
        let cwd = root.join("proj");
        let project = sessions.join("-tmp-proj");
        let main = project.join("2026-10-01T09-00-00-000Z_0001.jsonl");
        let artifacts = project.join("2026-10-01T09-00-00-000Z_0001");
        let line = |id: &str, input| {
            assistant_line(id, "2026-10-01T12:00:00.000Z", "claude-opus-4-8", input)
        };

        write_file(
            &main,
            &[session_header(&cwd, "0001"), line("m1", 10)].join("\n"),
        );
        write_file(
            &artifacts.join("Sub1.jsonl"),
            &[session_header(&cwd, "s1"), line("s1", 20)].join("\n"),
        );
        write_file(&artifacts.join("__advisor.jsonl"), &line("adv", 30));
        write_file(&artifacts.join("Sub1/Sub2.jsonl"), &line("s2", 40));
        // Too deep, beside the session, or with no main file: not sessions.
        write_file(&artifacts.join("a/b/c/deep.jsonl"), &line("deep", 1000));
        write_file(
            &project.join("extensions/goal/x.history.jsonl"),
            &line("ext", 1000),
        );
        write_file(&project.join("orphan/Sub.jsonl"), &line("orphan", 1000));

        let stats = ScanState::<Omp>::default().refresh(&sessions);

        let input: u64 = stats.daily.iter().map(|d| d.input_tokens).sum();
        assert_eq!(input, 10 + 20 + 30 + 40);
        assert_eq!(stats.total_messages, 4);
        let output: u64 = stats.daily.iter().map(|d| d.output_tokens).sum();
        assert_eq!(output, 16, "reasoningTokens is part of output, never added");
        assert!((stats.daily.iter().map(|d| d.cost_usd).sum::<f64>() - 1.0).abs() < 1e-9);

        let _ = fs::remove_dir_all(&root);
    }

    /// A subagent transcript that grows while its parent file is untouched is
    /// still picked up on the next refresh: children are tracked as main files.
    #[test]
    fn nested_child_change_refreshes_without_parent_change() {
        let root = temp_root("incremental");
        let sessions = root.join("sessions");
        let project = sessions.join("-tmp-proj");
        let main = project.join("2026-10-01T09-00-00-000Z_0001.jsonl");
        let child = project.join("2026-10-01T09-00-00-000Z_0001/Sub1.jsonl");
        let line = |id: &str, input| assistant_line(id, "2026-10-01T12:00:00.000Z", "gpt-6", input);

        write_file(&main, &line("m1", 10));
        write_file(&child, &line("s1", 20));
        let mut state = ScanState::<Omp>::default();
        assert_eq!(state.refresh(&sessions).total_messages, 2);

        write_file(&child, &[line("s1", 20), line("s2", 5)].join("\n"));
        let stats = state.refresh(&sessions);
        assert_eq!(stats.total_messages, 3);
        assert_eq!(stats.daily.iter().map(|d| d.input_tokens).sum::<u64>(), 35);

        let _ = fs::remove_dir_all(&root);
    }

    /// Default + named profiles under `~/.omp`, and the flattened XDG tree
    /// only when `$XDG_DATA_HOME/omp` exists.
    #[test]
    fn sessions_roots_cover_profiles_and_xdg() {
        let home = temp_root("roots");
        let xdg = home.join("xdg");
        fs::create_dir_all(home.join(".omp/profiles/work")).unwrap();
        fs::create_dir_all(home.join(".omp/profiles/zz")).unwrap();
        write_file(&home.join(".omp/profiles/not-a-dir"), "");
        let xdg_str = xdg.to_string_lossy().into_owned();

        assert_eq!(
            sessions_roots(&home, Some(&xdg_str)),
            vec![
                home.join(".omp/agent/sessions"),
                home.join(".omp/profiles/work/agent/sessions"),
                home.join(".omp/profiles/zz/agent/sessions"),
            ],
            "XDG ignored until $XDG_DATA_HOME/omp exists"
        );

        fs::create_dir_all(xdg.join("omp/profiles/work")).unwrap();
        let roots = sessions_roots(&home, Some(&xdg_str));
        assert_eq!(
            roots[3..],
            [
                xdg.join("omp/sessions"),
                xdg.join("omp/profiles/work/sessions")
            ]
        );
        assert_eq!(
            sessions_roots(&home, Some("")).len(),
            3,
            "empty env = unset"
        );

        let _ = fs::remove_dir_all(&home);
    }

    /// Sessions in a profile root are counted alongside the default root, and
    /// both feed availability and the watcher.
    #[test]
    fn profile_sessions_are_scanned_and_watched() {
        let home = temp_root("profiles");
        let line = |id: &str, input| assistant_line(id, "2026-10-02T12:00:00.000Z", "gpt-6", input);
        write_file(
            &home.join(".omp/agent/sessions/-p/2026_a.jsonl"),
            &line("a", 1),
        );
        write_file(
            &home.join(".omp/profiles/work/agent/sessions/-p/2026_b.jsonl"),
            &line("b", 2),
        );

        let dirs = session_dirs_with(&home, None);
        assert!(dirs.any_exists());
        assert_eq!(dirs.existing().count(), 2);

        let stats = ScanState::<Omp>::default().refresh_dirs(&dirs);
        assert_eq!(stats.daily.iter().map(|d| d.input_tokens).sum::<u64>(), 3);

        assert!(!session_dirs_with(&home.join("nobody"), None).any_exists());

        let _ = fs::remove_dir_all(&home);
    }
}
