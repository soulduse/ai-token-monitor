use super::*;
use rusqlite::{params, Connection};
use std::sync::atomic::AtomicUsize;

const CREATED: i64 = 1_775_044_800_000;

struct TestDb {
    provider: OpenCodeProvider,
    conn: Connection,
}

impl TestDb {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let data_dir = std::env::temp_dir().join(format!(
            "atm-opencode-storage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&data_dir).unwrap();
        let provider = OpenCodeProvider { data_dir };
        let conn = Connection::open(provider.db_path()).unwrap();
        Self { provider, conn }
    }

    fn legacy(&self, table: &str) {
        self.conn.execute_batch(&format!(
            "CREATE TABLE {table} (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL)"
        )).unwrap();
    }

    fn v2(&self) {
        // OpenCode v2.0.22 and v2.2.6: packages/core/src/session/sql.ts.
        self.conn
            .execute_batch(
                "CREATE TABLE session_message (
                id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL,
                seq INTEGER NOT NULL, time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL, data TEXT NOT NULL
            )",
            )
            .unwrap();
    }

    fn insert_v1(&self, table: &str, id: &str, data: Value) {
        self.conn
            .execute(
                &format!("INSERT INTO {table} VALUES (?1, 'session-1', ?2)"),
                params![id, data.to_string()],
            )
            .unwrap();
    }

    fn insert_v2(&self, id: &str, kind: &str, data: Value) {
        self.conn
            .execute(
                "INSERT INTO session_message VALUES (?1, 'session-1', ?2, 0, ?3, ?3, ?4)",
                params![id, kind, CREATED, data.to_string()],
            )
            .unwrap();
    }

    fn stats(&self) -> AllStats {
        OpenCodeProvider::build_stats(&self.provider.parse_sqlite().unwrap())
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // Close before removing the directory so cleanup also works on Windows.
        let conn = std::mem::replace(&mut self.conn, Connection::open_in_memory().unwrap());
        drop(conn);
        fs::remove_dir_all(&self.provider.data_dir).unwrap();
    }
}

fn v1_message(input: u64) -> Value {
    serde_json::json!({
        "role": "assistant", "modelID": "anthropic/claude-sonnet-4-5",
        "time": {"created": CREATED},
        "tokens": {"input": input, "output": 200, "reasoning": 50,
                   "cache": {"read": 100, "write": 25}}
    })
}

fn v2_message(input: u64) -> Value {
    // id/type/session_id are stored in columns, not in the data JSON.
    serde_json::json!({
        "model": {"id": "anthropic/claude-sonnet-4-5", "providerID": "anthropic"},
        "time": {"created": CREATED},
        "tokens": {"input": input, "output": 200, "reasoning": 50,
                   "cache": {"read": 100, "write": 25}}
    })
}

#[test]
fn legacy_sqlite_tables_preserve_usage_and_column_session_ids() {
    for table in ["messages", "message", "message_v2"] {
        let db = TestDb::new();
        db.legacy(table);
        db.insert_v1(table, "msg-1", v1_message(1000));
        db.insert_v1(table, "msg-2", v1_message(500));
        let stats = db.stats();
        assert_eq!(stats.total_messages, 2);
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.daily[0].input_tokens, 1500);
        assert_eq!(stats.daily[0].output_tokens, 500);
    }
}

#[test]
fn legacy_two_column_table_still_uses_json_session_id() {
    let db = TestDb::new();
    db.conn
        .execute_batch("CREATE TABLE messages (id TEXT PRIMARY KEY, data TEXT)")
        .unwrap();
    let mut data = v1_message(1000);
    data["sessionID"] = serde_json::json!("session-json");
    db.conn
        .execute(
            "INSERT INTO messages VALUES ('msg-1', ?1)",
            [data.to_string()],
        )
        .unwrap();
    let entries = db.provider.parse_sqlite().unwrap();
    assert_eq!(entries[0].session_id, "session-json");
    assert_eq!(entries[0].input_tokens, 1000);
}

#[test]
fn v2_assistant_and_compaction_usage_share_existing_aggregation() {
    let db = TestDb::new();
    db.v2();
    db.insert_v2("msg-1", "assistant", v2_message(1000));
    let mut compaction = v2_message(500);
    compaction["status"] = serde_json::json!("completed");
    db.insert_v2("msg-2", "compaction", compaction);
    db.insert_v2("msg-user", "user", v2_message(99999));
    db.insert_v2(
        "msg-pending",
        "assistant",
        serde_json::json!({"model": {"id": "unknown"}}),
    );
    db.conn.execute("INSERT INTO session_message VALUES ('msg-bad', 'session-1', 'assistant', 0, ?1, ?1, '{')", [CREATED]).unwrap();

    let stats = db.stats();
    assert_eq!(stats.total_messages, 2);
    assert_eq!(stats.total_sessions, 1);
    assert_eq!(stats.daily.len(), 1);
    let daily = &stats.daily[0];
    assert_eq!(
        daily.date,
        extract_date_from_message(&v1_message(1)).unwrap()
    );
    assert_eq!(daily.input_tokens, 1500);
    assert_eq!(daily.output_tokens, 500);
    assert_eq!(daily.cache_read_tokens, 200);
    assert_eq!(daily.cache_write_tokens, 50);
    assert_eq!(daily.tokens["claude-sonnet-4-5"], 2250);
    let expected = calculate_opencode_cost("claude-sonnet-4-5", 1000, 250, 100, 25)
        + calculate_opencode_cost("claude-sonnet-4-5", 500, 250, 100, 25);
    assert!((daily.cost_usd - expected).abs() < 1e-9);
    assert_eq!(stats.model_usage.len(), 1);
    assert!((stats.model_usage["claude-sonnet-4-5"].cost_usd - expected).abs() < 1e-9);
}

#[test]
fn migrated_messages_count_once_and_legacy_only_usage_survives() {
    let db = TestDb::new();
    db.legacy("message");
    db.v2();
    db.insert_v1("message", "msg-migrated", v1_message(1000));
    db.insert_v2("msg-migrated", "assistant", v2_message(1200));
    db.insert_v1("message", "msg-legacy-only", v1_message(300));
    db.insert_v2("msg-new", "assistant", v2_message(400));
    // v1 summaries migrate to a compaction with the USER message ID and no usage.
    db.insert_v1("message", "msg-summary", v1_message(200));
    db.insert_v2(
        "msg-compaction-user",
        "compaction",
        serde_json::json!({"status": "completed"}),
    );
    let stats = db.stats();
    assert_eq!(stats.total_messages, 4);
    assert_eq!(stats.total_sessions, 1);
    assert_eq!(stats.daily[0].input_tokens, 2100);
    // Re-reading a migrated DB must not accumulate or mutate its records.
    assert_eq!(db.stats().daily[0].cost_usd, stats.daily[0].cost_usd);
    assert_eq!(
        db.conn
            .query_row("SELECT count(*) FROM message", [], |row| row
                .get::<_, u32>(0))
            .unwrap(),
        3
    );
}

#[test]
fn unusable_v2_copy_does_not_erase_valid_legacy_usage() {
    let db = TestDb::new();
    db.legacy("message");
    db.v2();
    db.insert_v1("message", "msg-1", v1_message(1000));
    db.insert_v2("msg-1", "assistant", serde_json::json!({}));
    assert_eq!(db.stats().daily[0].input_tokens, 1000);
}

#[test]
fn v2_uses_column_timestamp_and_keeps_cache_only_usage() {
    let db = TestDb::new();
    db.v2();
    let mut data = v2_message(0);
    data.as_object_mut().unwrap().remove("time");
    data["tokens"]["output"] = serde_json::json!(0);
    data["tokens"]["reasoning"] = serde_json::json!(0);
    db.insert_v2("msg-cache", "assistant", data);
    let stats = db.stats();
    assert_eq!(stats.total_messages, 1);
    assert_eq!(stats.daily[0].tokens["claude-sonnet-4-5"], 125);
}

#[test]
fn unknown_or_broken_schema_reports_error_instead_of_empty_success() {
    let db = TestDb::new();
    db.conn
        .execute_batch("CREATE TABLE unrelated (id TEXT)")
        .unwrap();
    assert!(db.provider.parse_sqlite().is_err());
    db.conn
        .execute_batch("CREATE TABLE session_message (id TEXT)")
        .unwrap();
    assert!(db.provider.parse_sqlite().is_err());
}

#[test]
fn legacy_json_storage_still_parses() {
    let db = TestDb::new();
    let session_dir = db.provider.json_storage_dir().join("session-json");
    fs::create_dir_all(&session_dir).unwrap();
    fs::write(session_dir.join("msg-1.json"), v1_message(1000).to_string()).unwrap();
    let entries = db.provider.parse_json_storage().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].session_id, "session-json");
    assert_eq!(entries[0].input_tokens, 1000);
}

#[test]
fn wal_write_refreshes_cached_v2_stats_without_restart() {
    let db = TestDb::new();
    db.conn
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    db.v2();
    db.insert_v2("msg-1", "assistant", v2_message(1000));
    *lock_unpoisoned(&STATS_CACHE) = None;
    invalidate_stats_cache();
    assert_eq!(db.provider.fetch_stats().unwrap().total_messages, 1);
    db.insert_v2("msg-2", "assistant", v2_message(500));
    invalidate_stats_cache();
    let refreshed = db.provider.fetch_stats().unwrap();
    assert_eq!(refreshed.total_messages, 2);
    assert_eq!(refreshed.daily[0].input_tokens, 1500);
    *lock_unpoisoned(&STATS_CACHE) = None;
}
