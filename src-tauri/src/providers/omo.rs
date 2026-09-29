//! OmO native provider: usage stats from the OmO coding agent's session logs.
//!
//! OmO is a pi-coding-agent derivative; parsing, dedup and the incremental
//! scan live in the shared `pi_session` engine.
//!
//! Layout (verified on real logs):
//! - agent dir = `$OMO_CODING_AGENT_DIR` (if set and non-empty after trimming)
//!   else `~/.omo/agent`. The `omo` launcher (`bin/lib/agent-dir.js`
//!   `canonicalAgentDir`) trims and `path.resolve`s the value WITHOUT `~`
//!   expansion, so `~` is kept literal here too.
//! - main sessions: `<agent dir>/sessions/*/*.jsonl` (fixed depth 1 subdir)
//! - custom session dir = `--session-dir` > `$OMO_CODING_AGENT_SESSION_DIR` >
//!   `sessionDir` in `<agent dir>/settings.jsonc` (else `settings.json`); the
//!   senpi engine then writes FLAT `<dir>/<ts>_<id>.jsonl`. Env and settings
//!   are scanned next to the default root; `--session-dir` cannot be discovered.
//! - subagent child sessions live OUTSIDE the agent dir at
//!   `<cwd>/.omo/senpi-task/children/st_*/sessions/st_*/*.jsonl`, where `<cwd>`
//!   comes from main session headers (line 1)
//! - `reasoning` tokens are already included in `output`; never added again.

use std::path::{Path, PathBuf};

use super::pi_session::{self, SessionDirs, SessionLayout, SessionStatsCache};
use super::traits::TokenProvider;
use super::types::AllStats;

#[derive(Clone, Copy, Default)]
pub(super) struct Omo;

impl SessionLayout for Omo {
    const NAME: &'static str = "OmO";
    const LOG_TAG: &'static str = "OMO";
    const FALLBACK_MODEL: &'static str = "omo";
    const CHILDREN_GLOB: Option<&'static str> =
        Some(".omo/senpi-task/children/st_*/sessions/st_*/*.jsonl");
    // senpi settings-manager: `settings.jsonc` wins when present, and both are
    // parsed as JSONC.
    const SETTINGS_FILES: &'static [&'static str] = &["settings.jsonc", "settings.json"];
    const SETTINGS_JSONC: bool = true;
}

#[cfg(test)]
type ScanState = pi_session::ScanState<Omo>;

// --- Dir resolution ---

/// Pure resolver: `$OMO_CODING_AGENT_DIR` trimmed when non-empty, else
/// `~/.omo/agent`. No `~` expansion, matching the omo launcher.
fn resolve_agent_dir(env_value: Option<&str>) -> PathBuf {
    match env_value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()
            .unwrap_or_default()
            .join(".omo")
            .join("agent"),
    }
}

/// Default sessions root: env-or-default agent dir + `sessions`.
#[cfg(test)]
fn default_sessions_root() -> PathBuf {
    resolve_agent_dir(std::env::var("OMO_CODING_AGENT_DIR").ok().as_deref()).join("sessions")
}

/// Session dirs the watcher follows: the default root and the custom flat dir,
/// whichever exist.
pub fn watch_dirs() -> Vec<PathBuf> {
    OmoProvider::new().session_dirs().existing().cloned().collect()
}

// --- Cache ---

static CACHE: SessionStatsCache<Omo> = SessionStatsCache::new();

/// Invalidate cache — called by the file watcher on agent-dir changes.
pub fn invalidate_stats_cache() {
    CACHE.invalidate();
}

/// Return cached stats without triggering a re-parse (used by tray updates).
pub fn get_cached_stats() -> Option<AllStats> {
    CACHE.cached_stats()
}

// --- Provider ---

pub struct OmoProvider {
    agent_dir: PathBuf,
}

impl OmoProvider {
    pub fn new() -> Self {
        Self::with_agent_dir(resolve_agent_dir(
            std::env::var("OMO_CODING_AGENT_DIR").ok().as_deref(),
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
            std::env::var("OMO_CODING_AGENT_SESSION_DIR").ok().as_deref(),
            &dirs::home_dir().unwrap_or_default(),
        )
    }

    /// Pure part of [`Self::session_dirs`]: env value and home passed in.
    fn session_dirs_with(&self, session_env: Option<&str>, home: &Path) -> SessionDirs {
        let custom =
            pi_session::resolve_custom_session_dir::<Omo>(session_env, &self.agent_dir, home);
        SessionDirs::new(self.sessions_root(), custom)
    }
}

impl Default for OmoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenProvider for OmoProvider {
    fn name(&self) -> &str {
        "OmO"
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

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omo-provider-test-{}-{tag}", std::process::id()));
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

    /// Append one JSONL line; changes file SIZE so (mtime,size) diffing sees it
    /// without relying on mtime granularity.
    fn append_line(path: &Path, line: &str) {
        let mut content = fs::read_to_string(path).expect("read file for append");
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(line);
        content.push('\n');
        fs::write(path, content).expect("append line");
    }

    /// Line 1 of every session file: the session header carrying id and cwd.
    fn session_header(cwd: &Path, id: &str) -> String {
        json!({
            "type": "session",
            "version": 3,
            "id": id,
            "timestamp": "2026-09-01T09:00:00Z",
            "cwd": cwd.to_string_lossy(),
        })
        .to_string()
    }

    /// (input, output, cache_read, cache_write, messages, tokens) summed over
    /// all days — timezone-independent, fixtures never assert date strings.
    fn totals(stats: &AllStats) -> (u64, u64, u64, u64, u32, u64) {
        stats.daily.iter().fold(
            (0, 0, 0, 0, 0, 0),
            |(i, o, cr, cw, m, t), d| {
                (
                    i + d.input_tokens,
                    o + d.output_tokens,
                    cr + d.cache_read_tokens,
                    cw + d.cache_write_tokens,
                    m + d.messages,
                    t + d.tokens.values().sum::<u64>(),
                )
            },
        )
    }

    // (1) The same responseId reaches main and child logs (forked/resumed
    // sessions copy history). It must count once, deterministically from the
    // first file in sorted path order.
    #[test]
    fn response_id_duplicated_across_files_counts_once() {
        let root = temp_root("dedup");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-b");

        let usage = |input: u64, output: u64, total: u64| {
            json!({
                "type": "message",
                "id": "1111aaaa",
                "timestamp": "2026-09-01T10:00:00Z",
                "message": {
                    "role": "assistant",
                    "model": "claude-sonnet-4.6",
                    "responseId": "resp_dup",
                    "usage": {"input": input, "output": output, "cacheRead": 0, "cacheWrite": 0,
                              "totalTokens": total, "cost": {"total": 0.1}}
                }
            })
            .to_string()
        };

        write_file(
            &sessions.join("aaa").join("main.jsonl"),
            &[session_header(&cwd_a, "sess-a"), usage(100, 10, 110)].join("\n"),
        );
        write_file(
            &sessions.join("zzz").join("main.jsonl"),
            &[session_header(&cwd_b, "sess-b"), usage(200, 20, 220)].join("\n"),
        );
        // Child session under cwd_a: `agent` sorts before `proj-a`, so the main
        // file in sessions/aaa is the first occurrence of "resp_dup".
        write_file(
            &cwd_a.join(".omo/senpi-task/children/st_1/sessions/st_1/c.jsonl"),
            &[session_header(&cwd_a, "sess-child"), usage(999, 99, 1098)].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 1, "duplicated responseId must count once");
        assert_eq!(stats.total_sessions, 1);
        let (input, output, _cr, _cw, messages, tokens) = totals(&stats);
        assert_eq!(
            (input, output, messages, tokens),
            (100, 10, 1, 110),
            "first occurrence in sorted path order wins"
        );
        assert_eq!(stats.model_usage.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    // (2) reasoning is already part of output; adding it again would inflate
    // both output and totals.
    #[test]
    fn reasoning_is_already_included_in_output_and_never_added() {
        let root = temp_root("reasoning");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");

        let with_reasoning_and_total = json!({
            "type": "message", "id": "aaaa0001", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "r1",
                "usage": {"input": 10, "output": 5, "cacheRead": 2, "cacheWrite": 3,
                          "totalTokens": 20, "reasoning": 7, "cost": {"total": 0.25}}}
        })
        .to_string();
        let without_total_tokens = json!({
            "type": "message", "id": "aaaa0002", "timestamp": "2026-09-01T10:01:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "r2",
                "usage": {"input": 1, "output": 2, "cacheRead": 3, "cacheWrite": 4}}
        })
        .to_string();

        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[
                session_header(&cwd, "sess-1"),
                with_reasoning_and_total,
                without_total_tokens,
            ]
            .join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 2);
        let (input, output, cache_read, cache_write, _m, tokens) = totals(&stats);
        // output stays 5 + 2 — reasoning (7) must NOT be added on top.
        assert_eq!((input, output), (11, 7));
        assert_eq!((cache_read, cache_write), (5, 7));
        assert_eq!(tokens, input + output + cache_read + cache_write);
        // Precomputed cost is trusted as-is.
        assert!((stats.daily.iter().map(|d| d.cost_usd).sum::<f64>() - 0.25).abs() < 1e-9);

        let _ = fs::remove_dir_all(&root);
    }

    // (3) Only assistant lines with non-null, non-all-zero usage count.
    #[test]
    fn skips_malformed_non_assistant_null_usage_and_all_zero_lines() {
        let root = temp_root("skips");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");

        let malformed = "{ not valid json".to_string();
        let user_role = json!({
            "type": "message", "id": "bbbb0002", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "user", "model": "claude-opus-4.8", "responseId": "u1",
                "usage": {"input": 50, "output": 5, "totalTokens": 55}}
        })
        .to_string();
        let null_usage = json!({
            "type": "message", "id": "bbbb0003", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "n1",
                "usage": null}
        })
        .to_string();
        let all_zero = json!({
            "type": "message", "id": "bbbb0004", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "z1",
                "usage": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0, "totalTokens": 0}}
        })
        .to_string();
        let valid = json!({
            "type": "message", "id": "bbbb0005", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "v1",
                "usage": {"input": 7, "output": 3, "totalTokens": 10}}
        })
        .to_string();

        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[
                session_header(&cwd, "sess-1"),
                malformed,
                user_role,
                null_usage,
                all_zero,
                valid,
            ]
            .join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 1, "only the valid assistant line counts");
        let (input, output, _cr, _cw, messages, tokens) = totals(&stats);
        assert_eq!((input, output, messages, tokens), (7, 3, 1, 10));

        let _ = fs::remove_dir_all(&root);
    }

    // (4) Child sessions are discovered only through main-session header cwds;
    // anything deeper than sessions/*/*.jsonl under the agent dir is not a
    // session.
    #[test]
    fn children_found_only_via_header_cwd_and_deep_main_files_ignored() {
        let root = temp_root("discovery");
        let sessions = root.join("agent").join("sessions");
        let cwd_x = root.join("proj-x");
        let cwd_y = root.join("proj-y");

        let usage = |response_id: &str, input: u64| {
            json!({
                "type": "message", "id": "cccc0001", "timestamp": "2026-09-01T10:00:00Z",
                "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": response_id,
                    "usage": {"input": input, "output": 1, "totalTokens": input + 1}}
            })
            .to_string()
        };

        // Counted: main session at fixed depth 1.
        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[session_header(&cwd_x, "sess-x"), usage("main-1", 10)].join("\n"),
        );
        // Ignored: depth-3 file under the main sessions tree.
        write_file(
            &sessions.join("w1").join("extensions/goal/x.history.jsonl"),
            &usage("deep", 500),
        );
        // Ignored: depth-0 file directly under sessions/.
        write_file(&sessions.join("top.jsonl"), &usage("top", 300));
        // Counted: child session reachable only via the main header's cwd.
        write_file(
            &cwd_x.join(".omo/senpi-task/children/st_9/sessions/st_9/c.jsonl"),
            &[session_header(&cwd_x, "child-x"), usage("child-x", 40)].join("\n"),
        );
        // Ignored: children under a cwd no main session references.
        write_file(
            &cwd_y.join(".omo/senpi-task/children/st_8/sessions/st_8/c.jsonl"),
            &[session_header(&cwd_y, "child-y"), usage("child-y", 600)].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 2);
        let (input, _o, _cr, _cw, messages, _t) = totals(&stats);
        assert_eq!((input, messages), (50, 2), "10 (main) + 40 (child via header cwd) only");

        let _ = fs::remove_dir_all(&root);
    }

    // (5) Incremental refresh: child dirs are re-globbed only for the cwds of
    // changed/new main files; with no main change the cache is served as-is.
    #[test]
    fn incremental_refresh_rescans_children_only_of_changed_main_cwds() {
        let root = temp_root("incremental");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-b");

        let usage = |response_id: &str, input: u64| {
            json!({
                "type": "message", "id": "dddd0001", "timestamp": "2026-09-01T10:00:00Z",
                "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": response_id,
                    "usage": {"input": input, "output": 1, "totalTokens": input + 1}}
            })
            .to_string()
        };

        let main_a = sessions.join("a").join("main.jsonl");
        let main_b = sessions.join("b").join("main.jsonl");
        let child_a = cwd_a.join(".omo/senpi-task/children/st_a/sessions/st_a/c.jsonl");
        let child_b = cwd_b.join(".omo/senpi-task/children/st_b/sessions/st_b/c.jsonl");

        write_file(&main_a, &[session_header(&cwd_a, "sess-a"), usage("m1", 1)].join("\n"));
        write_file(&main_b, &[session_header(&cwd_b, "sess-b"), usage("m2", 2)].join("\n"));
        write_file(&child_a, &[session_header(&cwd_a, "child-a"), usage("ca1", 10)].join("\n"));
        write_file(&child_b, &[session_header(&cwd_b, "child-b"), usage("cb1", 20)].join("\n"));

        let mut state = ScanState::default();
        let first = state.refresh(&sessions);
        assert_eq!(first.total_messages, 4);
        assert_eq!(totals(&first).0, 33);

        // Child-only change in cwd A: invisible until a main file of cwd A changes.
        append_line(&child_a, &usage("ca2", 100));
        let second = state.refresh(&sessions);
        assert_eq!(
            (totals(&second).0, second.total_messages),
            (33, 4),
            "child change must not be picked up without a main-file change"
        );

        // Change a child in cwd B AND a main file whose header cwd is A.
        append_line(&child_b, &usage("cb2", 200));
        append_line(&main_a, &usage("m3", 1000));
        let third = state.refresh(&sessions);

        // cwd A children re-scanned (ca2 picked up), main a re-parsed (m3
        // picked up), but cwd B was untouched (cb2 invisible).
        assert_eq!(third.total_messages, 6);
        assert_eq!(
            totals(&third).0,
            1133,
            "33 + 100 (ca2) + 1000 (m3); cb2 (200) excluded"
        );

        let _ = fs::remove_dir_all(&root);
    }

    // (6) Model ids land on the same normalized keys as every other provider;
    // missing model falls back to "omo".
    #[test]
    fn model_ids_normalized_with_omo_fallback() {
        let root = temp_root("normalize");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");

        let with_model = json!({
            "type": "message", "id": "eeee0001", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "n1",
                "usage": {"input": 5, "output": 1, "totalTokens": 6}}
        })
        .to_string();
        let without_model = json!({
            "type": "message", "id": "eeee0002", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "responseId": "n2",
                "usage": {"input": 6, "output": 1, "totalTokens": 7}}
        })
        .to_string();

        write_file(
            &sessions.join("w1").join("main.jsonl"),
            &[session_header(&cwd, "sess"), with_model, without_model].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        let mut keys: Vec<&str> = stats.model_usage.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["claude-opus-4-8", "omo"]);
        assert_eq!(stats.model_usage["claude-opus-4-8"].input_tokens, 5);
        assert_eq!(stats.model_usage["omo"].input_tokens, 6);

        let _ = fs::remove_dir_all(&root);
    }

    // A duplicated responseId whose copy is re-stamped later must stay on the
    // earlier day, even when the later copy sorts first by path.
    #[test]
    fn duplicated_response_keeps_earliest_date() {
        let root = temp_root("earliest");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let line = |timestamp: &str, input: u64| {
            json!({
                "type": "message", "id": "2222bbbb", "timestamp": timestamp,
                "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "resp_fork",
                    "usage": {"input": input, "output": 1, "totalTokens": input + 1}}
            })
            .to_string()
        };

        // `aaa` sorts first but holds the re-stamped (later) copy.
        write_file(
            &sessions.join("aaa").join("fork.jsonl"),
            &[session_header(&cwd, "sess-fork"), line("2026-09-05T12:00:00Z", 100)].join("\n"),
        );
        write_file(
            &sessions.join("zzz").join("orig.jsonl"),
            &[session_header(&cwd, "sess-orig"), line("2026-09-01T12:00:00Z", 200)].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 1);
        assert_eq!(totals(&stats).0, 200, "the earlier-dated copy wins");

        let _ = fs::remove_dir_all(&root);
    }

    // An empty responseId is not an id: blank-id responses must not collapse
    // into a single "" key.
    #[test]
    fn empty_response_id_falls_back_to_line_key() {
        let root = temp_root("empty-id");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let line = |id: &str| {
            json!({
                "type": "message", "id": id, "timestamp": "2026-09-01T10:00:00Z",
                "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "",
                    "usage": {"input": 10, "output": 1, "totalTokens": 11}}
            })
            .to_string()
        };

        write_file(
            &sessions.join("p").join("s.jsonl"),
            &[session_header(&cwd, "sess"), line("line-1"), line("line-2")].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);

        assert_eq!(stats.total_messages, 2);
        assert_eq!(totals(&stats).0, 20);

        let _ = fs::remove_dir_all(&root);
    }

    fn usage_line(response_id: &str, input: u64) -> String {
        json!({
            "type": "message", "id": "ffff0001", "timestamp": "2026-09-01T10:00:00Z",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": response_id,
                "usage": {"input": input, "output": 1, "totalTokens": input + 1}}
        })
        .to_string()
    }

    fn child_path(cwd: &Path, task: &str) -> PathBuf {
        cwd.join(format!(".omo/senpi-task/children/{task}/sessions/{task}/c.jsonl"))
    }

    /// An incremental refresh must land on exactly what a fresh scan (an app
    /// restart) computes for the same files.
    fn assert_matches_fresh(state: &mut ScanState, sessions: &Path, expected_input: u64) {
        let incremental = state.refresh(sessions);
        let fresh = ScanState::default().refresh(sessions);
        assert_eq!(totals(&fresh).0, expected_input, "fixture sanity: fresh scan");
        assert_eq!(
            (totals(&incremental), incremental.total_messages),
            (totals(&fresh), fresh.total_messages),
            "incremental refresh diverged from a fresh scan"
        );
    }

    // (8) Deleting the last main session of a project drops that project's
    // cached children, exactly as a fresh scan would.
    #[test]
    fn deleting_last_main_of_cwd_drops_its_children() {
        let root = temp_root("del-last-main");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let main = sessions.join("p").join("main.jsonl");
        write_file(&main, &[session_header(&cwd, "s"), usage_line("m", 1)].join("\n"));
        write_file(&child_path(&cwd, "st_1"), &[session_header(&cwd, "c"), usage_line("c", 10)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 11);
        fs::remove_file(&main).expect("remove main");
        assert_matches_fresh(&mut state, &sessions, 0);

        let _ = fs::remove_dir_all(&root);
    }

    // (9) A main session whose header cwd changes moves its project: the old
    // cwd's children go away (no longer referenced), the new cwd's appear.
    #[test]
    fn main_cwd_change_swaps_children_sets() {
        let root = temp_root("cwd-change");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-bbbb");
        let main = sessions.join("p").join("main.jsonl");
        write_file(&main, &[session_header(&cwd_a, "s"), usage_line("m", 2)].join("\n"));
        write_file(&child_path(&cwd_a, "st_a"), &[session_header(&cwd_a, "ca"), usage_line("ca", 10)].join("\n"));
        write_file(&child_path(&cwd_b, "st_b"), &[session_header(&cwd_b, "cb"), usage_line("cb", 100)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 12);
        write_file(&main, &[session_header(&cwd_b, "s"), usage_line("m", 2)].join("\n"));
        assert_matches_fresh(&mut state, &sessions, 102);

        let _ = fs::remove_dir_all(&root);
    }

    // (10) Deleting one of two mains that share a cwd is itself a signal for
    // that project: a child deleted meanwhile must disappear.
    #[test]
    fn deleted_main_rescans_children_of_its_cwd() {
        let root = temp_root("del-shared");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let main_1 = sessions.join("p").join("one.jsonl");
        let main_2 = sessions.join("p").join("two.jsonl");
        let child = child_path(&cwd, "st_1");
        write_file(&main_1, &[session_header(&cwd, "s1"), usage_line("m1", 1)].join("\n"));
        write_file(&main_2, &[session_header(&cwd, "s2"), usage_line("m2", 1)].join("\n"));
        write_file(&child, &[session_header(&cwd, "c"), usage_line("c", 10)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 12);
        fs::remove_file(&main_1).expect("remove main 1");
        fs::remove_file(&child).expect("remove child");
        assert_matches_fresh(&mut state, &sessions, 1);

        let _ = fs::remove_dir_all(&root);
    }

    // (11) A main moving away from a cwd that another main still references
    // is a signal for the OLD cwd too: its child written meanwhile is counted.
    #[test]
    fn main_leaving_cwd_rescans_old_cwd_still_referenced() {
        let root = temp_root("move-away");
        let sessions = root.join("agent").join("sessions");
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-bbbb");
        let mover = sessions.join("p").join("mover.jsonl");
        let stayer = sessions.join("p").join("stayer.jsonl");
        let child_a = child_path(&cwd_a, "st_a");
        write_file(&mover, &[session_header(&cwd_a, "s1"), usage_line("m1", 1)].join("\n"));
        write_file(&stayer, &[session_header(&cwd_a, "s2"), usage_line("m2", 2)].join("\n"));
        write_file(&child_a, &[session_header(&cwd_a, "c"), usage_line("c1", 10)].join("\n"));

        let mut state = ScanState::default();
        assert_eq!(totals(&state.refresh(&sessions)).0, 13);
        append_line(&child_a, &usage_line("c2", 20));
        write_file(&mover, &[session_header(&cwd_b, "s1"), usage_line("m1", 1)].join("\n"));
        assert_matches_fresh(&mut state, &sessions, 33);

        let _ = fs::remove_dir_all(&root);
    }

    // (12) The configured sessions root is a literal path, not a glob: a
    // bracketed directory name must not match its sibling `agent1`.
    #[test]
    fn sessions_root_with_glob_metacharacters_is_literal() {
        let root = temp_root("glob-root");
        let cwd = root.join("proj");
        let real = root.join("agent[1]").join("sessions");
        let decoy = root.join("agent1").join("sessions");
        write_file(&real.join("p").join("main.jsonl"), &[session_header(&cwd, "s"), usage_line("real", 1)].join("\n"));
        write_file(&decoy.join("p").join("main.jsonl"), &[session_header(&cwd, "d"), usage_line("decoy", 99)].join("\n"));

        let stats = ScanState::default().refresh(&real);
        assert_eq!((totals(&stats).0, stats.total_messages), (1, 1));

        let _ = fs::remove_dir_all(&root);
    }

    // (13) A malformed non-ASCII timestamp must fall back to the file date,
    // never panic on a byte slice and abort the whole refresh.
    #[test]
    fn malformed_unicode_timestamp_does_not_abort_refresh() {
        let root = temp_root("bad-ts");
        let sessions = root.join("agent").join("sessions");
        let cwd = root.join("proj");
        let bad = json!({
            "type": "message", "id": "abab0001", "timestamp": "2026-09-\u{65e5}",
            "message": {"role": "assistant", "model": "claude-opus-4.8", "responseId": "bad",
                "usage": {"input": 3, "output": 1, "totalTokens": 4}}
        })
        .to_string();
        write_file(
            &sessions.join("p").join("main.jsonl"),
            &[session_header(&cwd, "s"), bad, usage_line("good", 5)].join("\n"),
        );

        let stats = ScanState::default().refresh(&sessions);
        assert_eq!((totals(&stats).0, stats.total_messages), (8, 2));
        assert!(stats.daily.iter().all(|d| d.date.len() == 10 && d.date.is_ascii()));

        let _ = fs::remove_dir_all(&root);
    }

    /// Agent dir placed inside project A's children tree, so main sessions in
    /// `sessions/st_*/` are ALSO discovered as A's children. Main `a` refers to
    /// A, main `b` to B; returns (sessions root, main a, main b, cwd b).
    fn overlapping_discovery_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let cwd_a = root.join("proj-a");
        let cwd_b = root.join("proj-bbbb");
        let sessions = cwd_a.join(".omo/senpi-task/children/st_0/sessions");
        let main_a = sessions.join("st_a").join("a.jsonl");
        let main_b = sessions.join("st_b").join("b.jsonl");
        write_file(&main_a, &[session_header(&cwd_a, "sa"), usage_line("ma", 1)].join("\n"));
        write_file(&main_b, &[session_header(&cwd_b, "sb"), usage_line("mb", 2)].join("\n"));
        write_file(&child_path(&cwd_b, "st_b"), &[session_header(&cwd_b, "cb"), usage_line("cb", 100)].join("\n"));
        (sessions, main_a, main_b, cwd_b)
    }

    // (14) Dropping an unreferenced cwd's children must not evict a file that
    // is still a main session (agent dir nested inside a children tree).
    #[test]
    fn dropping_children_keeps_overlapping_main_sessions_after_delete() {
        let root = temp_root("overlap-del");
        let (sessions, main_a, _main_b, _cwd_b) = overlapping_discovery_fixture(&root);

        let mut state = ScanState::default();
        state.refresh(&sessions);
        fs::remove_file(&main_a).expect("remove main a");
        assert_matches_fresh(&mut state, &sessions, 102);

        let _ = fs::remove_dir_all(&root);
    }

    // (15) Same overlap, but main `a` moves to cwd B instead of being deleted.
    #[test]
    fn dropping_children_keeps_overlapping_main_sessions_after_cwd_change() {
        let root = temp_root("overlap-move");
        let (sessions, main_a, _main_b, cwd_b) = overlapping_discovery_fixture(&root);

        let mut state = ScanState::default();
        state.refresh(&sessions);
        write_file(&main_a, &[session_header(&cwd_b, "sa"), usage_line("ma", 1)].join("\n"));
        assert_matches_fresh(&mut state, &sessions, 103);

        let _ = fs::remove_dir_all(&root);
    }

    // (7) Env override is resolved purely (no process-env mutation in tests).
    #[test]
    fn agent_dir_resolution_honors_env_and_falls_back_to_home() {
        let home = dirs::home_dir().expect("home dir available in tests");

        assert_eq!(
            resolve_agent_dir(Some("/custom/agent")),
            PathBuf::from("/custom/agent")
        );
        assert_eq!(
            resolve_agent_dir(Some("")),
            home.join(".omo").join("agent"),
            "empty env value is treated as unset"
        );
        assert_eq!(resolve_agent_dir(None), home.join(".omo").join("agent"));

        // default_sessions_root = env-driven resolver + "sessions".
        let env_value = std::env::var("OMO_CODING_AGENT_DIR")
            .ok()
            .filter(|v| !v.is_empty());
        let expected_root = resolve_agent_dir(env_value.as_deref()).join("sessions");
        assert_eq!(default_sessions_root(), expected_root);

        // with_agent_dir pins the scanned dir; the sessions root hangs off it.
        let provider = OmoProvider::with_agent_dir(PathBuf::from("/opt/omo-agent"));
        assert_eq!(provider.agent_dir, PathBuf::from("/opt/omo-agent"));
        assert_eq!(provider.sessions_root(), PathBuf::from("/opt/omo-agent/sessions"));
    }

    // (18) The omo launcher trims the agent dir env and never expands `~`
    // (bin/lib/agent-dir.js canonicalAgentDir); the session dir env and
    // settings `sessionDir` ARE `~`-expanded by the senpi engine.
    #[test]
    fn agent_dir_is_trimmed_not_tilde_expanded_but_session_dir_is() {
        let home = dirs::home_dir().expect("home dir available in tests");
        assert_eq!(resolve_agent_dir(Some("  /custom/agent\n")), PathBuf::from("/custom/agent"));
        assert_eq!(resolve_agent_dir(Some("   ")), home.join(".omo").join("agent"));
        assert_eq!(resolve_agent_dir(Some("~/omo")), PathBuf::from("~/omo"));

        let root = temp_root("custom-resolve");
        let agent = root.join("agent");
        let fake_home = root.join("home");
        let resolve = |env: Option<&str>| pi_session::resolve_custom_session_dir::<Omo>(env, &agent, &fake_home);
        assert_eq!(resolve(Some("~/env")), Some(fake_home.join("env")));
        assert_eq!(resolve(None), None);

        // settings.json is JSONC for senpi: comments and trailing commas.
        write_file(
            &agent.join("settings.json"),
            "{\n  // where sessions go\n  \"sessionDir\": \"~/json\", /* a\n b */\n  \"url\": \"http://x//y\",\n}",
        );
        assert_eq!(resolve(None), Some(fake_home.join("json")));
        // settings.jsonc wins when present, even over a valid settings.json.
        write_file(&agent.join("settings.jsonc"), "{\"sessionDir\": \"/jsonc\",}");
        assert_eq!(resolve(None), Some(PathBuf::from("/jsonc")));
        write_file(&agent.join("settings.jsonc"), "{\"sessionDir\": \"/jsonc\" /* open");
        assert_eq!(resolve(None), None, "unterminated block comment is a parse error");

        let _ = fs::remove_dir_all(&root);
    }

    // (19) Main sessions in a flat custom dir are main sessions: their header
    // cwds still lead to child sessions, and a response copied from the
    // default root counts once.
    #[test]
    fn flat_custom_dir_mains_discover_children_and_dedup_with_root() {
        let root = temp_root("custom-scan");
        let sessions = root.join("agent").join("sessions");
        let custom = root.join("flat");
        let cwd = root.join("proj");
        write_file(&sessions.join("p").join("main.jsonl"), &[session_header(&cwd, "s"), usage_line("m1", 1)].join("\n"));
        write_file(
            &custom.join("2026-09-01T09-00-00-000Z_f.jsonl"),
            &[session_header(&cwd, "f"), usage_line("m1", 1), usage_line("m2", 10)].join("\n"),
        );
        write_file(&child_path(&cwd, "st_1"), &[session_header(&cwd, "c"), usage_line("c1", 100)].join("\n"));

        let dirs = SessionDirs::new(sessions.clone(), Some(custom.clone()));
        let stats = ScanState::default().refresh_dirs(&dirs);
        assert_eq!((totals(&stats).0, stats.total_messages), (111, 3));

        let provider = OmoProvider::with_agent_dir(root.join("agent"));
        let custom_str = custom.to_string_lossy().into_owned();
        let watched: Vec<PathBuf> =
            provider.session_dirs_with(Some(&custom_str), &root).existing().cloned().collect();
        assert_eq!(watched, vec![sessions, custom]);

        let _ = fs::remove_dir_all(&root);
    }
}
