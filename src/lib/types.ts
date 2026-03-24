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
}
