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
  include_claude: true,
  include_codex: false,
  include_opencode: false,
  include_kimi: false,
  include_glm: false,
  include_gjc: false,
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

export function installMockTauri(): void {
  const stats = fixtureStats();
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
    get_preferences: () => mockPrefs,
    get_ai_keys: () => null,
    get_pricing_table: () => ({
      version: "mock",
      last_updated: toLocalDateStr(new Date()),
      claude: [{ model: "claude-fable-5", input: "$5", output: "$25", cache_read: "$0.5", cache_write: "$6.25" }],
      codex: [],
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
