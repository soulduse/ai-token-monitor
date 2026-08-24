/**
 * Dev-only Tauri IPC mock so the UI can run in a plain browser for QA:
 *
 *   npm run dev  →  http://localhost:1420/?mockTauri=1
 *
 * Installed exclusively from main.tsx behind `import.meta.env.DEV` + the
 * `mockTauri` query flag, so it is dead-code-eliminated from release builds.
 * Sets `window.__TAURI_MOCK__` so server-write paths (snapshot upload /
 * backfill) refuse to send fixture data to production Supabase.
 *
 * Scope note: only the Tauri IPC layer is mocked — Supabase itself stays
 * live. Signing in during a mock session performs the user's own real,
 * authenticated actions (profile upsert, chat). Fixture *stats* are blocked
 * from upload; don't treat the rest of the session as a sandbox.
 */
import type { AllStats, DailyUsage, UserPreferences } from "../lib/types";
import { toLocalDateStr } from "../lib/format";

const DAY_MS = 86_400_000;

function fixtureStats(): AllStats {
  const daily: DailyUsage[] = [];
  // 10 days of data ending today, with a gap 3 days ago (tests the
  // "no data for this day" navigation state).
  for (let back = 9; back >= 0; back--) {
    if (back === 3) continue;
    const date = toLocalDateStr(new Date(Date.now() - back * DAY_MS));
    const scale = 1 + ((9 - back) % 4);
    daily.push({
      date,
      tokens: {
        "claude-fable-5": 120_000_000 * scale,
        "claude-opus-4-8": 80_000_000 * scale,
        "claude-sonnet-5": 2_000_000 * scale,
      },
      cost_usd: 310.5 * scale,
      messages: 800 * scale,
      sessions: 3 + scale,
      tool_calls: 1200 * scale,
      input_tokens: 1_500_000 * scale,
      output_tokens: 900_000 * scale,
      cache_read_tokens: 150_000_000 * scale,
      cache_write_tokens: 9_000_000 * scale,
    });
  }
  return {
    daily,
    model_usage: {
      "claude-fable-5": { input_tokens: 9_000_000, output_tokens: 5_400_000, cache_read: 1_800_000_000, cache_write: 60_000_000, cost_usd: 4_100 },
      "claude-opus-4-8": { input_tokens: 6_000_000, output_tokens: 3_600_000, cache_read: 1_200_000_000, cache_write: 40_000_000, cost_usd: 2_400 },
      "claude-sonnet-5": { input_tokens: 400_000, output_tokens: 200_000, cache_read: 28_000_000, cache_write: 1_500_000, cost_usd: 95 },
    },
    total_sessions: 60,
    total_messages: 16_000,
    first_session_date: daily[0]?.date ?? null,
    rate_limits: null,
  };
}

const mockPrefs: UserPreferences = {
  number_format: "compact",
  show_tray_cost: true,
  leaderboard_opted_in: false,
  leaderboard_upload_enabled: true,
  include_claude: true,
  include_codex: false,
  include_opencode: false,
  include_kimi: false,
  include_glm: false,
  include_gjc: false,
  include_grok: false,
  include_kiro: false,
  theme: "github",
  color_mode: "dark",
  language: "en",
  config_dirs: ["~/.claude"],
  codex_dirs: ["~/.codex"],
  gjc_dirs: ["~/.gjc"],
  salary_enabled: false,
  usage_alerts_enabled: true,
  usage_tracking_enabled: false,
  autostart_enabled: false,
  quick_action_items: [],
};

const KIRO_CREDIT_RATE_USD = 0.04;

/**
 * Kiro's fixture keeps every token counter at 0, exactly like the real provider:
 * Kiro meters credits and records no token counts anywhere, so a QA fixture with
 * plausible-looking token numbers would misrepresent what the provider can know.
 */
function fixtureKiroStats(): AllStats {
  const daily: DailyUsage[] = [];
  for (let back = 4; back >= 0; back--) {
    const date = toLocalDateStr(new Date(Date.now() - back * DAY_MS));
    const credits = 1.2 * (1 + ((4 - back) % 3));
    daily.push({
      date,
      tokens: {},
      cost_usd: credits * KIRO_CREDIT_RATE_USD,
      messages: 4 + back,
      sessions: 1 + (back % 2),
      tool_calls: 3 * (back + 1),
      input_tokens: 0,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_write_tokens: 0,
    });
  }
  return {
    daily,
    model_usage: {
      "claude-sonnet-4.5": { input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.62 },
      "claude-haiku-4.5": { input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.11 },
      auto: { input_tokens: 0, output_tokens: 0, cache_read: 0, cache_write: 0, cost_usd: 0.07 },
    },
    total_sessions: 6,
    total_messages: 24,
    first_session_date: daily[0]?.date ?? null,
    rate_limits: null,
  };
}

/** Mirrors `providers::kiro::KiroBreakdown` so the Kiro sub-tab renders in QA mode. */
function fixtureKiroBreakdown() {
  const stats = fixtureKiroStats();
  const days = stats.daily.map((d) => ({
    date: d.date,
    credits: d.cost_usd / KIRO_CREDIT_RATE_USD,
    cost_usd: d.cost_usd,
    turns: d.messages,
    cancelled_turns: d.date === stats.daily[1]?.date ? 1 : 0,
  }));
  const totalCredits = days.reduce((s, d) => s + d.credits, 0);
  const today = stats.daily[stats.daily.length - 1]?.date ?? toLocalDateStr(new Date());
  return {
    total_credits: totalCredits,
    total_cost_usd: totalCredits * KIRO_CREDIT_RATE_USD,
    credit_rate_usd: KIRO_CREDIT_RATE_USD,
    total_turns: days.reduce((s, d) => s + d.turns, 0),
    total_requests: 31,
    total_tool_calls: 45,
    total_sessions: 6,
    cancelled_turns: 1,
    cancelled_credits: 0.84,
    auto_credits: 1.795,
    first_turn_date: stats.daily[0]?.date ?? null,
    last_turn_date: today,
    by_model: [
      { model: "claude-sonnet-4.5", credits: totalCredits * 0.62, cost_usd: totalCredits * 0.62 * KIRO_CREDIT_RATE_USD, turns: 12, requests: 18, is_auto: false },
      { model: "claude-haiku-4.5", credits: totalCredits * 0.23, cost_usd: totalCredits * 0.23 * KIRO_CREDIT_RATE_USD, turns: 7, requests: 9, is_auto: false },
      { model: "auto", credits: totalCredits * 0.15, cost_usd: totalCredits * 0.15 * KIRO_CREDIT_RATE_USD, turns: 5, requests: 4, is_auto: true },
    ],
    by_day: days,
    by_project: [
      { project: "ai-token-monitor", project_path: "/Users/qa/github/ai-token-monitor", credits: totalCredits * 0.7, cost_usd: totalCredits * 0.7 * KIRO_CREDIT_RATE_USD, turns: 16 },
      { project: "scratch", project_path: "/tmp/scratch", credits: totalCredits * 0.3, cost_usd: totalCredits * 0.3 * KIRO_CREDIT_RATE_USD, turns: 8 },
    ],
    recent_turns: [
      { ended_at: new Date(Date.now() - 3 * 60_000).toISOString(), date: today, model: "claude-sonnet-4.5", credits: 3.87, cost_usd: 3.87 * KIRO_CREDIT_RATE_USD, end_reason: "UserTurnEnd", cancelled: false, requests: 4, tool_calls: 9, context_percent: 18.4, duration_secs: 92, project_path: "/Users/qa/github/ai-token-monitor", project: "ai-token-monitor", source: "session", session_id: "qa-session-1" },
      { ended_at: new Date(Date.now() - 22 * 60_000).toISOString(), date: today, model: "auto", credits: 1.795, cost_usd: 1.795 * KIRO_CREDIT_RATE_USD, end_reason: "UserTurnEnd", cancelled: false, requests: 2, tool_calls: 3, context_percent: 9.1, duration_secs: 41, project_path: "/tmp/scratch", project: "scratch", source: "sqlite", session_id: "qa-session-2" },
      { ended_at: new Date(Date.now() - 55 * 60_000).toISOString(), date: today, model: "claude-haiku-4.5", credits: 0.84, cost_usd: 0.84 * KIRO_CREDIT_RATE_USD, end_reason: "Cancelled", cancelled: true, requests: 1, tool_calls: 0, context_percent: null, duration_secs: 12, project_path: "/Users/qa/github/ai-token-monitor", project: "ai-token-monitor", source: "session", session_id: "qa-session-1" },
    ],
    session_store_turns: 14,
    sqlite_store_turns: 10,
    tokens_unavailable: true,
  };
}

export function installMockTauri(): void {
  const stats = fixtureStats();
  const kiroStats = fixtureKiroStats();
  const kiroBreakdown = fixtureKiroBreakdown();
  let callbackId = 0;

  // Flag checked by isMockTauriSession() to block server writes in QA mode.
  (window as unknown as Record<string, unknown>).__TAURI_MOCK__ = true;

  const handlers: Record<string, (args?: unknown) => unknown> = {
    get_all_stats: () => stats,
    get_codex_stats: () => stats,
    get_opencode_stats: () => stats,
    get_kimi_stats: () => stats,
    get_glm_stats: () => stats,
    get_gjc_stats: () => stats,
    get_grok_stats: () => stats,
    get_kiro_stats: () => kiroStats,
    get_kiro_breakdown: () => kiroBreakdown,
    is_kiro_available: () => true,
    is_grok_available: () => true,
    get_grok_usage: () => ({
      subscription_tier: "SuperGrok Lite",
      credit_usage_percent: 72.5,
      period_start: `${toLocalDateStr(new Date())}T00:00:00.000Z`,
      period_end: new Date(Date.now() + 3 * DAY_MS).toISOString(),
      on_demand_cap: 0,
      on_demand_used: 0,
      prepaid_balance: 0,
      fetched_at: new Date().toISOString(),
    }),
    get_preferences: () => mockPrefs,
    get_ai_keys: () => null,
    get_pricing_table: () => ({
      version: "mock",
      last_updated: toLocalDateStr(new Date()),
      claude: [{ model: "claude-fable-5", input: "$5", output: "$25", cache_read: "$0.5", cache_write: "$6.25" }],
      codex: [],
      grok: [{ model: "Grok 4.6", input: "$2", output: "$6", cache_read: "$0.5", cache_write: "—" }],
    }),
    get_oauth_usage: () => null,
    get_oauth_usage_status: () => "no_credentials",
    get_oauth_rate_limit_remaining: () => null,
    get_stable_device_id: () => "mock-device-id",
    "plugin:app|version": () => "0.0.0-mock",
    "plugin:autostart|is_enabled": () => false,
    "plugin:event|listen": () => ++callbackId,
    "plugin:event|unlisten": () => null,
    "plugin:event|emit": () => null,
    "plugin:image|new": () => ++callbackId,
    "plugin:resources|close": () => null,
    "plugin:clipboard-manager|write_image": () => null,
    "plugin:dialog|save": () => "/tmp/ai-token-monitor-capture.png",
    // Explicit no-ops for side-effect commands exercised during QA. They
    // resolve like the real commands do, but perform nothing in a browser.
    heartbeat: () => null,
    set_preferences: () => null,
    set_dialog_open: () => null,
    enable_usage_tracking: () => null,
    copy_png_to_clipboard: () => null,
    save_png_to_file: () => null,
  };

  // Tauri's event API cleanup path calls this directly (outside invoke).
  (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => {},
  };

  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { label: "main" },
      currentWebviewWindow: { label: "main" },
    },
    transformCallback: (_cb: unknown, _once?: boolean) => ++callbackId,
    invoke: (cmd: string, args?: unknown) => {
      const handler = handlers[cmd];
      if (!handler) {
        // Loud (but non-throwing) so a QA run can't silently treat an
        // unimplemented command as success.
        console.warn(`[mockTauri] unhandled command: ${cmd} — resolving null`);
        return Promise.resolve(null);
      }
      return Promise.resolve(handler(args));
    },
  };

  console.info("[mockTauri] installed — browser QA mode");
}
