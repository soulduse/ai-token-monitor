//! Antigravity CLI (`agy`) provider — EXPERIMENTAL.
//!
//! The CLI keeps one SQLite db per conversation at
//! `~/.gemini/antigravity-cli/conversations/<uuid>.db` (WAL mode). Usage lives
//! in undocumented protobuf blobs, decoded here by field number. The layout
//! below was reverse-engineered from real conversations (agy, 2026-07) and
//! checked against itself (output = thinking + response on every call):
//!
//! - `steps.metadata`, one row per step:
//!   - field 1 = `Timestamp { 1: unix seconds, 2: nanos }`
//!   - field 9 = per-model-call usage, present only on steps that called a
//!     model: `1` model enum, `2` input tokens, `3` output tokens (thinking
//!     `9` + response `10`), `11` response id
//! - `gen_metadata.data`, one row per generation: field 3 holds the request,
//!   with `3.28` = model name (`gemini-3.5-flash-low`) and `3.1` = the same
//!   model enum, so each db carries its own enum → name map
//!
//! No cached-token field has been observed, so cache reads count as 0. The
//! Antigravity IDE's `~/.gemini/antigravity/conversations/*.pb` files are
//! encrypted and are not read.
//!
//! If the format changes, a db that has generations but yields no usage is
//! logged once and contributes nothing — never a guessed number.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use chrono::TimeZone;
use rusqlite::{Connection, OpenFlags};

use super::pricing;
use super::resilience::{lock_unpoisoned, ParsingGuard};
use super::traits::TokenProvider;
use super::types::{AllStats, DailyUsage, ModelUsage};

// --- Cache infrastructure (mirrors gemini.rs) ---

/// (mtime, size) of a conversation db and of its `-wal` sidecar: between
/// checkpoints new rows only touch the WAL.
type DbMeta = ((SystemTime, u64), (SystemTime, u64));

struct IncrementalCache {
    stats: AllStats,
    computed_at: Instant,
    file_meta: HashMap<PathBuf, DbMeta>,
    file_entries: HashMap<PathBuf, Vec<(String, AgyEntry)>>,
}

static STATS_CACHE: Mutex<Option<IncrementalCache>> = Mutex::new(None);
static PARSING: AtomicBool = AtomicBool::new(false);
static CACHE_INVALIDATED: AtomicBool = AtomicBool::new(false);
static FORMAT_WARNED: AtomicBool = AtomicBool::new(false);
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Invalidate cache — called by the file watcher on conversation db changes.
pub fn invalidate_stats_cache() {
    CACHE_INVALIDATED.store(true, Ordering::Relaxed);
}

/// Return cached stats without triggering a re-parse (used by tray update).
pub fn get_cached_stats() -> Option<AllStats> {
    lock_unpoisoned(&STATS_CACHE)
        .as_ref()
        .map(|c| c.stats.clone())
}

// --- Entry type ---

/// One model call's token usage.
#[derive(Debug, Clone, PartialEq)]
struct AgyEntry {
    date: String,  // "YYYY-MM-DD" local timezone
    model: String, // normalized, effort suffix stripped
    session_id: String,
    input_tokens: u64,
    output_tokens: u64, // thinking + response
}

// --- Dir resolution ---

fn conversations_dir(home: &Path) -> PathBuf {
    home.join(".gemini")
        .join("antigravity-cli")
        .join("conversations")
}

/// The conversations dir, for the file watcher, when it exists.
pub fn watch_dirs() -> Vec<PathBuf> {
    let dir = conversations_dir(&dirs::home_dir().unwrap_or_default());
    if dir.is_dir() {
        vec![dir]
    } else {
        Vec::new()
    }
}

// --- Provider ---

pub struct AgyProvider {
    dir: PathBuf,
}

impl AgyProvider {
    pub fn new() -> Self {
        Self::with_dir(conversations_dir(&dirs::home_dir().unwrap_or_default()))
    }

    fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn collect_file_meta(&self) -> HashMap<PathBuf, DbMeta> {
        let stat = |p: &Path| {
            fs::metadata(p)
                .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
                .unwrap_or((SystemTime::UNIX_EPOCH, 0))
        };
        let mut meta = HashMap::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return meta;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.extension().and_then(|e| e.to_str()) != Some("db") {
                continue;
            }
            let mut wal = path.clone().into_os_string();
            wal.push("-wal");
            meta.insert(path.clone(), (stat(&path), stat(Path::new(&wal))));
        }
        meta
    }

    fn do_fetch_stats(&self) -> Result<AllStats, String> {
        let start = Instant::now();
        let current_meta = self.collect_file_meta();

        // Cloned, not taken: the tray keeps reading the old stats meanwhile.
        let (mut file_entries, old_meta) = match lock_unpoisoned(&STATS_CACHE).as_ref() {
            Some(c) => (c.file_entries.clone(), c.file_meta.clone()),
            None => (HashMap::new(), HashMap::new()),
        };
        file_entries.retain(|path, _| current_meta.contains_key(path));

        let mut parsed = 0;
        let mut failed: Vec<PathBuf> = Vec::new();
        for (path, meta) in &current_meta {
            if old_meta.get(path) == Some(meta) && file_entries.contains_key(path) {
                continue;
            }
            parsed += 1;
            match parse_db(path) {
                Ok(entries) => {
                    file_entries.insert(path.clone(), entries);
                }
                Err(e) => {
                    // Keep the last good parse (e.g. db locked mid-write), and
                    // leave the file's meta unrecorded so the next fetch
                    // retries it even if the db never changes again.
                    eprintln!("[AGY] {}: {}", path.display(), e);
                    failed.push(path.clone());
                }
            }
        }
        let mut paths: Vec<&PathBuf> = file_entries.keys().collect();
        paths.sort();
        let mut entries: HashMap<String, AgyEntry> = HashMap::new();
        for path in paths {
            for (key, entry) in &file_entries[path] {
                entries.entry(key.clone()).or_insert_with(|| entry.clone());
            }
        }
        let stats = build_stats(&entries);
        let file_entries_len = current_meta.len();
        let mut current_meta = current_meta;
        for path in &failed {
            current_meta.remove(path);
        }

        *lock_unpoisoned(&STATS_CACHE) = Some(IncrementalCache {
            stats: stats.clone(),
            computed_at: Instant::now(),
            file_meta: current_meta,
            file_entries,
        });
        eprintln!(
            "[PERF][AGY] fetch_stats: {} dbs ({} parsed) in {:?}",
            file_entries_len,
            parsed,
            start.elapsed()
        );
        Ok(stats)
    }
}

impl Default for AgyProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenProvider for AgyProvider {
    fn name(&self) -> &str {
        "Antigravity CLI"
    }

    fn fetch_stats(&self) -> Result<AllStats, String> {
        let was_invalidated = CACHE_INVALIDATED.swap(false, Ordering::Relaxed);
        if !was_invalidated {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                if cached.computed_at.elapsed() < CACHE_TTL {
                    return Ok(cached.stats.clone());
                }
            }
        }

        let Some(_parsing) = ParsingGuard::try_acquire(&PARSING) else {
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            std::thread::sleep(Duration::from_millis(100));
            if let Some(ref cached) = *lock_unpoisoned(&STATS_CACHE) {
                return Ok(cached.stats.clone());
            }
            return Err("Antigravity CLI stats computation in progress".to_string());
        };

        self.do_fetch_stats()
    }

    fn is_available(&self) -> bool {
        self.dir.is_dir()
    }
}

// --- Db parsing ---

/// Every model call in one conversation db, keyed for cross-file dedup.
fn parse_db(path: &Path) -> Result<Vec<(String, AgyEntry)>, String> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("open failed: {}", e))?;
    // One snapshot for both reads (WAL).
    conn.execute_batch("BEGIN")
        .map_err(|e| format!("read failed: {}", e))?;

    let blobs = |sql: &str| -> Result<Vec<(i64, Vec<u8>)>, String> {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| format!("query failed: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|e| format!("query failed: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("read failed: {}", e))?;
        Ok(rows)
    };
    let generations = blobs("SELECT idx, data FROM gen_metadata WHERE data IS NOT NULL")?;
    let steps = blobs("SELECT idx, metadata FROM steps WHERE metadata IS NOT NULL")?;

    let session_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "agy-session".to_string());
    let entries = conversation_entries(
        &session_id,
        generations.iter().map(|(_, b)| b.as_slice()),
        steps.iter().map(|(idx, b)| (*idx, b.as_slice())),
    );

    if entries.is_empty() && !generations.is_empty() && !FORMAT_WARNED.swap(true, Ordering::Relaxed)
    {
        eprintln!(
            "[AGY] {} has {} generations but no readable usage — the Antigravity CLI format may have changed; usage from such conversations is not counted",
            path.display(),
            generations.len()
        );
    }
    Ok(entries)
}

/// Pure decode of one conversation: generation blobs give the enum → model
/// name map, step blobs give one entry per model call.
fn conversation_entries<'a>(
    session_id: &str,
    generations: impl Iterator<Item = &'a [u8]>,
    steps: impl Iterator<Item = (i64, &'a [u8])>,
) -> Vec<(String, AgyEntry)> {
    let mut models: HashMap<u64, String> = HashMap::new();
    let mut primary: Option<String> = None;
    for blob in generations {
        let Some(request) = Fields::parse(blob).and_then(|f| f.message(3)) else {
            continue;
        };
        let Some(name) = request.string(28).filter(|n| !n.is_empty()) else {
            continue;
        };
        let model = normalize_model(name);
        if let Some(id) = request.varint(1) {
            models.entry(id).or_insert_with(|| model.clone());
        }
        primary.get_or_insert(model);
    }

    let mut entries = Vec::new();
    for (idx, blob) in steps {
        let Some(step) = Fields::parse(blob) else {
            continue;
        };
        let Some(usage) = step.message(9) else {
            continue;
        };
        let input = usage.varint(2).unwrap_or(0);
        let output = usage.varint(3).unwrap_or(0);
        if input == 0 && output == 0 {
            continue;
        }
        let Some(date) = step
            .message(1)
            .and_then(|ts| ts.varint(1))
            .and_then(|secs| local_date(secs as i64))
        else {
            continue;
        };
        // Auxiliary calls (e.g. title generation) use an enum with no
        // generation row; bill them as the conversation's model.
        let model = usage
            .varint(1)
            .and_then(|id| models.get(&id).cloned())
            .or_else(|| primary.clone())
            .unwrap_or_else(|| "agy".to_string());
        let key = usage
            .string(11)
            .filter(|id| !id.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("{}:{}", session_id, idx));
        entries.push((
            key,
            AgyEntry {
                date,
                model,
                session_id: session_id.to_string(),
                input_tokens: input,
                output_tokens: output,
            },
        ));
    }
    entries
}

/// `gemini-3.5-flash-extra-low` → `gemini-3-5-flash`: the suffix is the
/// thinking level picked in the CLI, not a separately priced model.
fn normalize_model(name: &str) -> String {
    let mut base = name;
    for suffix in [
        "-extra-low",
        "-extra-high",
        "-minimal",
        "-low",
        "-medium",
        "-high",
    ] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped;
            break;
        }
    }
    pricing::normalize_model_id(base)
}

fn local_date(secs: i64) -> Option<String> {
    chrono::Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
}

/// Per-request (input, output) rates in USD per million tokens. Antigravity
/// defaults to Gemini, so anything not recognizably Claude/GPT (including the
/// `agy` fallback) is priced from the Gemini table.
fn rates(model: &str, input_tokens: u64) -> (f64, f64) {
    let other_vendor = ["claude", "sonnet", "opus", "haiku", "gpt"]
        .iter()
        .any(|family| model.contains(family));
    if other_vendor {
        let p = pricing::get_opencode_pricing(model);
        (p.input, p.output)
    } else {
        let (input, output, _) = pricing::get_gemini_pricing(model).tier_for(input_tokens);
        (input, output)
    }
}

fn build_stats(entries: &HashMap<String, AgyEntry>) -> AllStats {
    let mut daily_map: HashMap<String, DailyUsage> = HashMap::new();
    let mut model_map: HashMap<String, ModelUsage> = HashMap::new();
    let mut daily_sessions: HashMap<String, HashSet<String>> = HashMap::new();
    let mut unique_sessions: HashSet<String> = HashSet::new();
    let mut first_date: Option<String> = None;

    for entry in entries.values() {
        unique_sessions.insert(entry.session_id.clone());
        if first_date.as_ref().map_or(true, |d| &entry.date < d) {
            first_date = Some(entry.date.clone());
        }

        let (input_rate, output_rate) = rates(&entry.model, entry.input_tokens);
        let cost = (entry.input_tokens as f64 / 1_000_000.0) * input_rate
            + (entry.output_tokens as f64 / 1_000_000.0) * output_rate;

        let daily = daily_map
            .entry(entry.date.clone())
            .or_insert_with(|| DailyUsage {
                hydrated: false,
                date: entry.date.clone(),
                tokens: HashMap::new(),
                cost_usd: 0.0,
                messages: 0,
                sessions: 0,
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            });
        daily.cost_usd += cost;
        daily.messages += 1;
        daily.input_tokens += entry.input_tokens;
        daily.output_tokens += entry.output_tokens;
        *daily.tokens.entry(entry.model.clone()).or_insert(0) +=
            entry.input_tokens + entry.output_tokens;
        daily_sessions
            .entry(entry.date.clone())
            .or_default()
            .insert(entry.session_id.clone());

        let model = model_map
            .entry(entry.model.clone())
            .or_insert_with(|| ModelUsage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read: 0,
                cache_write: 0,
                cost_usd: 0.0,
            });
        model.input_tokens += entry.input_tokens;
        model.output_tokens += entry.output_tokens;
        model.cost_usd += cost;
    }

    let mut daily_vec: Vec<DailyUsage> = daily_map.into_values().collect();
    for d in &mut daily_vec {
        if let Some(sessions) = daily_sessions.get(&d.date) {
            d.sessions = sessions.len() as u32;
        }
    }
    daily_vec.sort_by(|a, b| a.date.cmp(&b.date));

    AllStats {
        daily: daily_vec,
        model_usage: model_map,
        total_sessions: unique_sessions.len() as u32,
        total_messages: entries.len() as u32,
        first_session_date: first_date,
        analytics: None,
        rate_limits: None,
    }
}

// --- Minimal protobuf wire reader ---

#[derive(Debug, Clone, Copy)]
enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

/// Top-level fields of one protobuf message, in wire order. Only what this
/// provider needs: no schema, no packed repeated fields, groups rejected.
struct Fields<'a>(Vec<(u64, Value<'a>)>);

impl<'a> Fields<'a> {
    /// `None` when the bytes are not a well-formed message.
    fn parse(mut buf: &'a [u8]) -> Option<Self> {
        let mut fields = Vec::new();
        while !buf.is_empty() {
            let key = read_varint(&mut buf)?;
            let (number, wire_type) = (key >> 3, key & 7);
            if number == 0 {
                return None;
            }
            let value = match wire_type {
                0 => Value::Varint(read_varint(&mut buf)?),
                1 => {
                    buf = buf.get(8..)?;
                    Value::Fixed
                }
                2 => {
                    let len = usize::try_from(read_varint(&mut buf)?).ok()?;
                    let bytes = buf.get(..len)?;
                    buf = &buf[len..];
                    Value::Bytes(bytes)
                }
                5 => {
                    buf = buf.get(4..)?;
                    Value::Fixed
                }
                _ => return None,
            };
            fields.push((number, value));
        }
        Some(Self(fields))
    }

    fn first(&self, number: u64) -> Option<Value<'a>> {
        self.0.iter().find(|(n, _)| *n == number).map(|(_, v)| *v)
    }

    fn varint(&self, number: u64) -> Option<u64> {
        match self.first(number)? {
            Value::Varint(v) => Some(v),
            _ => None,
        }
    }

    fn bytes(&self, number: u64) -> Option<&'a [u8]> {
        match self.first(number)? {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    fn message(&self, number: u64) -> Option<Fields<'a>> {
        Fields::parse(self.bytes(number)?)
    }

    fn string(&self, number: u64) -> Option<&'a str> {
        std::str::from_utf8(self.bytes(number)?).ok()
    }
}

fn read_varint(buf: &mut &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = buf.split_first()?;
        *buf = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Encoder for fixtures ---

    fn varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn vfield(number: u64, v: u64) -> Vec<u8> {
        [varint(number << 3), varint(v)].concat()
    }

    fn bfield(number: u64, bytes: &[u8]) -> Vec<u8> {
        [
            varint((number << 3) | 2),
            varint(bytes.len() as u64),
            bytes.to_vec(),
        ]
        .concat()
    }

    /// A `gen_metadata.data` blob as observed: the request message at field 3
    /// carries the enum (3.1) and name (3.28), among unrelated fields of every
    /// wire type.
    fn generation(model_enum: u64, name: &str) -> Vec<u8> {
        let usage = [vfield(1, model_enum), vfield(2, 17884)].concat();
        let request = [
            vfield(1, model_enum),
            vfield(6, 65536),
            vec![0x09, 1, 2, 3, 4, 5, 6, 7, 8], // field 1, fixed64
            bfield(17, &bfield(2, &usage)),
            bfield(19, b"gemini-default"),
            bfield(28, name.as_bytes()),
            vec![0x1d, 1, 2, 3, 4], // field 3, fixed32
        ]
        .concat();
        [bfield(2, b"\x02"), bfield(3, &request)].concat()
    }

    /// A `steps.metadata` blob for a model call, shaped like the real ones.
    fn model_step(
        secs: u64,
        model_enum: u64,
        input: u64,
        thinking: u64,
        response: u64,
        id: &str,
    ) -> Vec<u8> {
        let mut usage = [
            vfield(1, model_enum),
            vfield(2, input),
            vfield(3, thinking + response),
            vfield(6, 24),
        ]
        .concat();
        usage.extend(bfield(
            8,
            &[bfield(1, b"sessionID"), bfield(2, b"-37")].concat(),
        ));
        if thinking > 0 {
            usage.extend(vfield(9, thinking));
        }
        usage.extend(vfield(10, response));
        if !id.is_empty() {
            usage.extend(bfield(11, id.as_bytes()));
        }
        [
            bfield(1, &[vfield(1, secs), vfield(2, 694_663_000)].concat()),
            vfield(3, 2),
            bfield(9, &usage),
            bfield(12, b"5e4affe4"),
        ]
        .concat()
    }

    fn user_step(secs: u64) -> Vec<u8> {
        [bfield(1, &vfield(1, secs)), vfield(3, 4)].concat()
    }

    /// Local noon of `date`, so the local date is unambiguous.
    fn secs_on(date: &str) -> u64 {
        let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        chrono::Local
            .from_local_datetime(&d.and_hms_opt(12, 0, 0).unwrap())
            .single()
            .unwrap()
            .timestamp() as u64
    }

    #[test]
    fn wire_reader_handles_all_wire_types_and_rejects_garbage() {
        let blob = generation(1187, "gemini-3.5-flash-extra-low");
        let f = Fields::parse(&blob)
            .expect("well-formed")
            .message(3)
            .expect("request");
        assert_eq!(f.string(28), Some("gemini-3.5-flash-extra-low"));
        assert_eq!(f.varint(1), Some(1187));
        assert_eq!(
            f.message(17)
                .and_then(|m| m.message(2))
                .and_then(|m| m.varint(1)),
            Some(1187)
        );
        assert_eq!(f.varint(28), None, "wrong wire type is not coerced");

        assert!(
            Fields::parse(&[0x0a, 0x05, 0x01]).is_none(),
            "truncated length-delimited"
        );
        assert!(Fields::parse(&[0x08, 0x80]).is_none(), "truncated varint");
        assert!(Fields::parse(&[0x0b]).is_none(), "group wire type");
        assert!(Fields::parse(&[0x00, 0x01]).is_none(), "field number 0");
        // An encrypted IDE blob (first bytes of a real one) is not a message.
        assert!(Fields::parse(&[0x93, 0xac, 0xae, 0x2d, 0x95, 0xec, 0x8b, 0x29]).is_none());
    }

    /// Mirrors a real conversation: user step, main call (thinking + text),
    /// an auxiliary call on an enum with no generation row.
    #[test]
    fn decodes_model_calls_with_names_and_dedup_keys() {
        let day = secs_on("2026-07-07");
        let generations = [generation(1020, "gemini-3.5-flash-low")];
        let steps = [
            (0, user_step(day)),
            (2, model_step(day, 1020, 17535, 128, 2, "ev1Map3tL")),
            (3, model_step(day, 1050, 45, 0, 2, "")),
        ];
        let entries = conversation_entries(
            "44d7110b",
            generations.iter().map(|g| g.as_slice()),
            steps.iter().map(|(i, b)| (*i, b.as_slice())),
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "ev1Map3tL");
        assert_eq!(
            entries[0].1,
            AgyEntry {
                date: "2026-07-07".into(),
                model: "gemini-3-5-flash".into(),
                session_id: "44d7110b".into(),
                input_tokens: 17535,
                output_tokens: 130,
            }
        );
        assert_eq!(
            entries[1].0, "44d7110b:3",
            "no response id → conversation:idx"
        );
        assert_eq!(
            entries[1].1.model, "gemini-3-5-flash",
            "unknown enum → conversation model"
        );
        assert_eq!(
            (entries[1].1.input_tokens, entries[1].1.output_tokens),
            (45, 2)
        );
    }

    #[test]
    fn model_names_drop_the_thinking_level_and_price_as_the_base_model() {
        assert_eq!(
            normalize_model("gemini-3.5-flash-extra-low"),
            "gemini-3-5-flash"
        );
        assert_eq!(normalize_model("gemini-3.5-flash-low"), "gemini-3-5-flash");
        assert_eq!(
            normalize_model("gemini-3.5-flash-lite"),
            "gemini-3-5-flash-lite"
        );
        assert_eq!(normalize_model("gemini-3.1-pro-high"), "gemini-3-1-pro");

        let flash = pricing::get_gemini_pricing("gemini-3-5-flash");
        let lite = pricing::get_gemini_pricing("gemini-3-5-flash-lite");
        assert!(
            flash.input > lite.input,
            "flash and flash-lite resolve to different rows"
        );
    }

    /// Fixture dbs built with the real schema: two conversations, a response
    /// copied into both counts once, and a db whose blobs no longer decode
    /// contributes nothing.
    #[test]
    fn fetches_stats_from_conversation_dbs() {
        let dir = std::env::temp_dir().join(format!("agy-provider-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let write_db = |name: &str, generations: &[Vec<u8>], steps: &[Vec<u8>]| {
            let conn = Connection::open(dir.join(name)).unwrap();
            conn.execute_batch(
                "CREATE TABLE `steps` (`idx` integer,`step_type` integer NOT NULL DEFAULT 0,`status` integer NOT NULL DEFAULT 0,`has_subtrajectory` numeric NOT NULL DEFAULT false,`metadata` blob,`error_details` blob,`permissions` blob,`task_details` blob,`render_info` blob,`step_payload` blob,`step_format` integer NOT NULL DEFAULT 0,PRIMARY KEY (`idx`));
                 CREATE TABLE `gen_metadata` (`idx` integer,`data` blob,`size` integer NOT NULL DEFAULT 0,PRIMARY KEY (`idx`));",
            )
            .unwrap();
            for (i, g) in generations.iter().enumerate() {
                conn.execute(
                    "INSERT INTO gen_metadata (idx, data) VALUES (?1, ?2)",
                    rusqlite::params![i as i64, g],
                )
                .unwrap();
            }
            for (i, s) in steps.iter().enumerate() {
                conn.execute(
                    "INSERT INTO steps (idx, metadata) VALUES (?1, ?2)",
                    rusqlite::params![i as i64, s],
                )
                .unwrap();
            }
        };
        let (d1, d2) = (secs_on("2026-07-07"), secs_on("2026-07-08"));
        write_db(
            "a.db",
            &[generation(1187, "gemini-3.5-flash-extra-low")],
            &[user_step(d1), model_step(d1, 1187, 1000, 0, 37, "r1")],
        );
        write_db(
            "b.db",
            &[generation(1187, "gemini-3.5-flash-extra-low")],
            &[
                model_step(d1, 1187, 1000, 0, 37, "r1"),
                model_step(d2, 1187, 2000, 10, 3, "r2"),
            ],
        );
        write_db(
            "broken.db",
            &[generation(1, "gemini-x")],
            &[b"\x0b\x0b".to_vec()],
        );
        fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let provider = AgyProvider::with_dir(dir.clone());
        assert!(provider.is_available());
        let stats = provider.do_fetch_stats().unwrap();

        assert_eq!(stats.total_messages, 2, "r1 copied into b.db counts once");
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.daily.len(), 2);
        let input: u64 = stats.daily.iter().map(|d| d.input_tokens).sum();
        let output: u64 = stats.daily.iter().map(|d| d.output_tokens).sum();
        assert_eq!((input, output), (3000, 50));

        let p = pricing::get_gemini_pricing("gemini-3-5-flash");
        let expected = 3000.0 / 1e6 * p.input + 50.0 / 1e6 * p.output;
        let cost: f64 = stats.daily.iter().map(|d| d.cost_usd).sum();
        assert!((cost - expected).abs() < 1e-12);
        assert!(stats.model_usage.contains_key("gemini-3-5-flash"));

        assert!(!AgyProvider::with_dir(dir.join("missing")).is_available());
        let _ = fs::remove_dir_all(&dir);
        *lock_unpoisoned(&STATS_CACHE) = None;
    }
}
