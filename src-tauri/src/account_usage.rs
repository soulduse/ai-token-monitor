//! Per-account Claude usage snapshots.
//!
//! The OAuth usage API only ever reflects the account whose token is currently
//! active, and session JSONL entries carry no account identity at all. To let
//! users who rotate between multiple Claude accounts compare limits across
//! them, every successful usage fetch is recorded here keyed by the
//! `accountUuid` that Claude Code writes to `.claude.json` (`oauthAccount`).
//! Switching accounts therefore accumulates one "last known" snapshot per
//! account, persisted across restarts.
//!
//! Attribution happens at fetch time: a snapshot always belongs to the account
//! that was active when the fetch succeeded. The split-account UI renders only
//! these snapshots (never the live in-memory cache), so data can never appear
//! under the wrong account label right after a switch.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::oauth_usage::OAuthUsage;

/// Identity of the account currently signed into Claude Code, as recorded in
/// `.claude.json`. Email/display name are for local display only and must
/// never leave the machine (leaderboard uploads don't touch this module).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountIdentity {
    pub account_uuid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

/// Last known usage for one account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub account_uuid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub usage: OAuthUsage,
    /// RFC3339 timestamp of the fetch that produced this snapshot. The UI
    /// derives "checked N ago" staleness from this for inactive accounts.
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountUsageSnapshots {
    pub accounts: Vec<AccountSnapshot>,
    pub active_account_uuid: Option<String>,
}

static STORE: Mutex<Option<AccountUsageSnapshots>> = Mutex::new(None);

fn store_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("ai-token-monitor-accounts.json")
}

/// `.claude.json` lives next to (not inside) the config dir by default, but
/// moves into `$CLAUDE_CONFIG_DIR` when that override is set — mirroring how
/// oauth_usage.rs resolves `.credentials.json`.
fn claude_json_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        let dir = dir.trim();
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join(".claude.json"));
        }
    }
    dirs::home_dir().map(|h| h.join(".claude.json"))
}

/// Read the identity of the currently active Claude Code account.
/// Returns None when nobody is signed in or the file predates `oauthAccount`.
pub fn read_active_identity() -> Option<AccountIdentity> {
    let content = std::fs::read_to_string(claude_json_path()?).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    parse_identity(&value)
}

fn parse_identity(value: &serde_json::Value) -> Option<AccountIdentity> {
    let account = value.get("oauthAccount")?;
    let account_uuid = account.get("accountUuid")?.as_str()?.trim().to_string();
    if account_uuid.is_empty() {
        return None;
    }
    let string_field = |key: &str| -> Option<String> {
        account
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
    };
    Some(AccountIdentity {
        account_uuid,
        email: string_field("emailAddress"),
        display_name: string_field("displayName"),
    })
}

fn load_store_from_disk() -> AccountUsageSnapshots {
    match std::fs::read_to_string(store_path()) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            eprintln!("[ACCOUNTS] failed to parse snapshot store, starting fresh: {e}");
            AccountUsageSnapshots::default()
        }),
        Err(_) => AccountUsageSnapshots::default(),
    }
}

fn save_store_to_disk(store: &AccountUsageSnapshots) {
    let Ok(json) = serde_json::to_string_pretty(store) else {
        return;
    };
    if let Err(e) = std::fs::write(store_path(), json) {
        eprintln!("[ACCOUNTS] failed to persist snapshot store: {e}");
    }
}

/// Run `f` against the lazily-loaded store; persist afterwards when `f`
/// reports it mutated the store.
fn with_store<T>(f: impl FnOnce(&mut AccountUsageSnapshots) -> (T, bool)) -> Option<T> {
    let mut guard = STORE.lock().ok()?;
    let store = guard.get_or_insert_with(load_store_from_disk);
    let (result, dirty) = f(store);
    if dirty {
        save_store_to_disk(store);
    }
    Some(result)
}

/// Record a successful usage fetch under the currently active account.
/// Silently a no-op when no account identity is readable — the default
/// (active-only) view keeps working without it.
pub fn record_snapshot(usage: &OAuthUsage) {
    let Some(identity) = read_active_identity() else {
        return;
    };
    let snapshot = AccountSnapshot {
        account_uuid: identity.account_uuid.clone(),
        email: identity.email,
        display_name: identity.display_name,
        usage: usage.clone(),
        updated_at: chrono::Local::now().to_rfc3339(),
    };
    with_store(|store| {
        upsert_snapshot(store, snapshot);
        ((), true)
    });
}

fn upsert_snapshot(store: &mut AccountUsageSnapshots, snapshot: AccountSnapshot) {
    store.active_account_uuid = Some(snapshot.account_uuid.clone());
    match store
        .accounts
        .iter_mut()
        .find(|a| a.account_uuid == snapshot.account_uuid)
    {
        Some(existing) => *existing = snapshot,
        None => store.accounts.push(snapshot),
    }
}

/// Snapshots for the UI: active account first, the rest newest-first. The
/// active uuid is re-read live from `.claude.json` so the badge is correct
/// immediately after an account switch, before the next poll lands.
pub fn get_snapshots() -> AccountUsageSnapshots {
    let live_active = read_active_identity().map(|i| i.account_uuid);
    with_store(|store| {
        let mut result = store.clone();
        if live_active.is_some() {
            result.active_account_uuid = live_active.clone();
        }
        sort_snapshots(&mut result);
        (result, false)
    })
    .unwrap_or_default()
}

fn sort_snapshots(snapshots: &mut AccountUsageSnapshots) {
    let active = snapshots.active_account_uuid.clone();
    snapshots.accounts.sort_by(|a, b| {
        let a_active = Some(&a.account_uuid) == active.as_ref();
        let b_active = Some(&b.account_uuid) == active.as_ref();
        b_active
            .cmp(&a_active)
            .then_with(|| parse_epoch_millis(&b.updated_at).cmp(&parse_epoch_millis(&a.updated_at)))
    });
}

fn parse_epoch_millis(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

/// Drop one account's snapshot (e.g. an account the user no longer uses).
/// Returns true when something was removed.
pub fn remove_snapshot(account_uuid: &str) -> bool {
    with_store(|store| {
        let before = store.accounts.len();
        store.accounts.retain(|a| a.account_uuid != account_uuid);
        let removed = store.accounts.len() != before;
        if store.active_account_uuid.as_deref() == Some(account_uuid) {
            store.active_account_uuid = None;
        }
        (removed, removed)
    })
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_fixture() -> OAuthUsage {
        OAuthUsage {
            five_hour: None,
            seven_day: None,
            seven_day_sonnet: None,
            seven_day_opus: None,
            seven_day_models: Vec::new(),
            extra_usage: None,
            fetched_at: "2026-07-14T10:00:00+09:00".to_string(),
            is_stale: false,
        }
    }

    fn snapshot_fixture(uuid: &str, updated_at: &str) -> AccountSnapshot {
        AccountSnapshot {
            account_uuid: uuid.to_string(),
            email: Some(format!("{uuid}@example.com")),
            display_name: None,
            usage: usage_fixture(),
            updated_at: updated_at.to_string(),
        }
    }

    #[test]
    fn parses_oauth_account_identity() {
        let value = serde_json::json!({
            "oauthAccount": {
                "accountUuid": "uuid-1",
                "emailAddress": "a@example.com",
                "displayName": "Alice",
                "organizationUuid": "org-1"
            }
        });

        let identity = parse_identity(&value).expect("identity");
        assert_eq!(identity.account_uuid, "uuid-1");
        assert_eq!(identity.email.as_deref(), Some("a@example.com"));
        assert_eq!(identity.display_name.as_deref(), Some("Alice"));
    }

    #[test]
    fn identity_requires_account_uuid() {
        // Signed-out / legacy .claude.json has no oauthAccount at all.
        assert!(parse_identity(&serde_json::json!({})).is_none());
        // An empty uuid must not create a phantom account bucket.
        let blank = serde_json::json!({ "oauthAccount": { "accountUuid": "  " } });
        assert!(parse_identity(&blank).is_none());
        // Blank email/display name collapse to None rather than empty strings.
        let sparse = serde_json::json!({
            "oauthAccount": { "accountUuid": "uuid-1", "emailAddress": "" }
        });
        let identity = parse_identity(&sparse).expect("identity");
        assert_eq!(identity.email, None);
    }

    #[test]
    fn upsert_replaces_same_account_and_appends_new() {
        let mut store = AccountUsageSnapshots::default();

        upsert_snapshot(&mut store, snapshot_fixture("a", "2026-07-14T10:00:00+09:00"));
        upsert_snapshot(&mut store, snapshot_fixture("b", "2026-07-14T11:00:00+09:00"));
        assert_eq!(store.accounts.len(), 2);
        assert_eq!(store.active_account_uuid.as_deref(), Some("b"));

        // Re-recording account "a" replaces its entry instead of duplicating,
        // and marks it active again.
        upsert_snapshot(&mut store, snapshot_fixture("a", "2026-07-14T12:00:00+09:00"));
        assert_eq!(store.accounts.len(), 2);
        assert_eq!(store.active_account_uuid.as_deref(), Some("a"));
        let a = store.accounts.iter().find(|s| s.account_uuid == "a").unwrap();
        assert_eq!(a.updated_at, "2026-07-14T12:00:00+09:00");
    }

    #[test]
    fn sorts_active_first_then_newest() {
        let mut snapshots = AccountUsageSnapshots {
            accounts: vec![
                snapshot_fixture("old", "2026-07-10T10:00:00+09:00"),
                snapshot_fixture("new", "2026-07-14T10:00:00+09:00"),
                snapshot_fixture("active", "2026-07-01T10:00:00+09:00"),
            ],
            active_account_uuid: Some("active".to_string()),
        };

        sort_snapshots(&mut snapshots);

        let order: Vec<&str> = snapshots
            .accounts
            .iter()
            .map(|s| s.account_uuid.as_str())
            .collect();
        assert_eq!(order, vec!["active", "new", "old"]);
    }
}
