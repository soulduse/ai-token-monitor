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
}

export interface ModelUsage {
  input_tokens: number;
  output_tokens: number;
  cache_read: number;
  cache_write: number;
  cost_usd: number;
}

export interface AllStats {
  daily: DailyUsage[];
  model_usage: Record<string, ModelUsage>;
  total_sessions: number;
  total_messages: number;
  first_session_date: string | null;
}

export type LeaderboardProvider = "claude" | "codex";

export interface UserPreferences {
  number_format: "compact" | "full";
  show_tray_cost: boolean;
  leaderboard_opted_in: boolean;
  include_claude: boolean;
  include_codex: boolean;
  theme: "github" | "purple" | "ocean" | "sunset";
  color_mode: "system" | "light" | "dark";
  language: "en" | "ko" | "ja" | "zh-CN" | "zh-TW" | "fr" | "es" | "de";
  config_dirs: string[];
  monthly_salary?: number;
  usage_alerts_enabled: boolean;
  usage_tracking_enabled: boolean;
  ai_keys?: {
    gemini?: string;
    openai?: string;
    anthropic?: string;
  };
  ai_model?: string;
  device_id?: string;
}

export interface UsageWindow {
  utilization: number;
  resets_at: string;
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
  extra_usage: ExtraUsage | null;
  fetched_at: string;
  is_stale: boolean;
}
