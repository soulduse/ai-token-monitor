export interface DailyUsage {
  date: string;
  tokens: Record<string, number>;
  cost_usd: number;
  messages: number;
  sessions: number;
  tool_calls: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  /** True when the day was restored from the server instead of parsed from
   *  local logs. Upload/backfill paths must skip hydrated days. */
  hydrated?: boolean;
}

export interface ModelUsage {
  input_tokens: number;
  output_tokens: number;
  cache_read: number;
  cache_write: number;
  cost_usd: number;
}

export interface ProjectUsage {
  name: string;
  cost_usd: number;
  tokens: number;
  sessions: number;
  messages: number;
}

export interface ToolCount {
  name: string;
  count: number;
}

export interface McpServerUsage {
  server: string;
  calls: number;
}

export interface ActivityCategory {
  category: string;
  cost_usd: number;
  messages: number;
}

export interface AnalyticsData {
  project_usage: ProjectUsage[];
  tool_usage: ToolCount[];
  shell_commands: ToolCount[];
  mcp_usage: McpServerUsage[];
  activity_breakdown: ActivityCategory[];
}

export interface RateLimitWindow {
  used_percent: number;
  window_minutes: number;
  resets_at: number;
}

export interface CodexRateLimits {
  limit_id?: string | null;
  limit_name?: string | null;
  plan_type?: string | null;
  primary?: RateLimitWindow | null;
  secondary?: RateLimitWindow | null;
  rate_limit_reached_type?: string | null;
  source: "oauth" | "jsonl" | string;
}

export interface AllStats {
  daily: DailyUsage[];
  model_usage: Record<string, ModelUsage>;
  total_sessions: number;
  total_messages: number;
  first_session_date: string | null;
  analytics?: AnalyticsData;
  rate_limits?: CodexRateLimits | null;
}

export type LeaderboardProvider = "claude" | "codex" | "opencode" | "kimi" | "glm" | "gjc" | "grok" | "kiro";

/** SuperGrok / unified-billing snapshot from Grok CLI's rolling log. */
export interface GrokCredits {
  subscription_tier: string | null;
  /** 0–100 */
  credit_usage_percent: number | null;
  period_start: string | null;
  period_end: string | null;
  on_demand_cap: number;
  on_demand_used: number;
  prepaid_balance: number;
  fetched_at: string;
}

export interface UserPreferences {
  number_format: "compact" | "full";
  show_tray_cost: boolean;
  leaderboard_opted_in: boolean;
  /** Whether THIS machine uploads its usage — participation stays per-account, uploads are per-device. */
  leaderboard_upload_enabled?: boolean;
  device_id?: string;
  include_claude: boolean;
  include_codex: boolean;
  include_opencode: boolean;
  include_kimi: boolean;
  include_glm: boolean;
  include_gjc: boolean;
  include_grok: boolean;
  include_kiro: boolean;
  theme: "github" | "purple" | "ocean" | "sunset";
  color_mode: "system" | "light" | "dark";
  language: "en" | "ko" | "ja" | "zh-CN" | "zh-TW" | "fr" | "es" | "de" | "tr" | "it";
  config_dirs: string[];
  codex_dirs: string[];
  gjc_dirs: string[];
  salary_enabled: boolean;
  monthly_salary?: number;
  usage_alerts_enabled: boolean;
  usage_tracking_enabled: boolean;
  ai_keys?: {
    gemini?: string;
    openai?: string;
    anthropic?: string;
    kiro?: string;
    webhook_discord_url?: string;
    webhook_slack_url?: string;
    webhook_telegram_bot_token?: string;
    webhook_telegram_chat_id?: string;
  };
  ai_model?: string;
  webhook_config?: WebhookConfig;
  autostart_enabled: boolean;
  quick_action_items: string[];
}

export interface WebhookConfig {
  discord_enabled: boolean;
  slack_enabled: boolean;
  telegram_enabled: boolean;
  thresholds: number[];
  notify_on_reset: boolean;
  monitored_windows: MonitoredWindows;
}

export interface MonitoredWindows {
  five_hour: boolean;
  seven_day: boolean;
  seven_day_sonnet: boolean;
  seven_day_opus: boolean;
  extra_usage: boolean;
}

export interface UsageWindow {
  utilization: number;
  // null when the window has no scheduled reset (e.g. a scoped weekly window at 0%)
  resets_at: string | null;
}

// A weekly window scoped to a specific model (e.g. Sonnet, Opus, Fable).
// These come from the API's `limits` array (active `weekly_scoped` entries) and
// may be added/removed at any time, so the UI iterates over this list rather
// than hard-coding models. `model` is the human-readable display name.
export interface ModelWindow extends UsageWindow {
  model: string;
}

export interface ExtraUsage {
  is_enabled: boolean;
  monthly_limit: number;
  used_credits: number;
  utilization: number;
}

export interface OAuthUsage {
  five_hour: UsageWindow | null;
  seven_day: UsageWindow | null;
  seven_day_sonnet: UsageWindow | null;
  seven_day_opus: UsageWindow | null;
  // Every per-model weekly window the API returned (sonnet/opus/fable/...).
  // Optional for backward-compat with any cached payload lacking the field.
  seven_day_models?: ModelWindow[];
  extra_usage: ExtraUsage | null;
  fetched_at: string;
  is_stale: boolean;
}

// Mirrors the Rust OAuthUsageStatus enum (serde rename_all = "snake_case").
// "available": usage is cached and renderable.
// "no_credentials": user has not signed into Claude Code — normal for
//   Codex-only users; must not be surfaced as an error.
// "unavailable": credentials exist but no usage cached yet (first poll pending
//   or a failed fetch) — worth offering a refresh.
export type OAuthUsageStatus = "available" | "no_credentials" | "unavailable";
