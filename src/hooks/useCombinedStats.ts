import { useMemo } from "react";
import { useTokenStats } from "./useTokenStats";
import type { AllStats, DailyUsage, ModelUsage } from "../lib/types";

interface UseCombinedStatsProps {
  includeClaude: boolean;
  includeCodex: boolean;
}

export function useCombinedStats({ includeClaude, includeCodex }: UseCombinedStatsProps) {
  const claude = useTokenStats("claude");
  const codex = useTokenStats("codex");

  const stats = useMemo<AllStats | null>(() => {
    const sources: (AllStats | null)[] = [];
    if (includeClaude) sources.push(claude.stats);
    if (includeCodex) sources.push(codex.stats);

    const validStats = sources.filter((s): s is AllStats => s !== null);
    if (validStats.length === 0) return includeClaude ? claude.stats : codex.stats;
    if (validStats.length === 1) return validStats[0];

    return mergeStats(validStats);
  }, [claude.stats, codex.stats, includeClaude, includeCodex]);

  const loading = (includeClaude && claude.loading) || (includeCodex && codex.loading);
  const error = includeClaude ? claude.error : codex.error;

  return { stats, loading, error };
}

function mergeStats(statsList: AllStats[]): AllStats {
  const dailyMap = new Map<string, DailyUsage>();
  const modelUsage: Record<string, ModelUsage> = {};
  let totalMessages = 0;
  let totalSessions = 0;
  let firstDate: string | null = null;

  for (const s of statsList) {
    totalMessages += s.total_messages;
    totalSessions += s.total_sessions;

    if (s.first_session_date && (!firstDate || s.first_session_date < firstDate)) {
      firstDate = s.first_session_date;
    }

    for (const d of s.daily) {
      const existing = dailyMap.get(d.date);
      if (existing) {
        for (const [model, tokens] of Object.entries(d.tokens)) {
          existing.tokens[model] = (existing.tokens[model] ?? 0) + tokens;
        }
        existing.cost_usd += d.cost_usd;
        existing.messages += d.messages;
        existing.sessions += d.sessions;
        existing.tool_calls += d.tool_calls;
        existing.input_tokens += d.input_tokens;
        existing.output_tokens += d.output_tokens;
        existing.cache_read_tokens += d.cache_read_tokens;
        existing.cache_write_tokens += d.cache_write_tokens;
      } else {
        dailyMap.set(d.date, {
          date: d.date,
          tokens: { ...d.tokens },
          cost_usd: d.cost_usd,
          messages: d.messages,
          sessions: d.sessions,
          tool_calls: d.tool_calls,
          input_tokens: d.input_tokens,
          output_tokens: d.output_tokens,
          cache_read_tokens: d.cache_read_tokens,
          cache_write_tokens: d.cache_write_tokens,
        });
      }
    }

    for (const [model, usage] of Object.entries(s.model_usage)) {
      const e = modelUsage[model];
      if (e) {
        e.input_tokens += usage.input_tokens;
        e.output_tokens += usage.output_tokens;
        e.cache_read += usage.cache_read;
        e.cache_write += usage.cache_write;
        e.cost_usd += usage.cost_usd;
      } else {
        modelUsage[model] = { ...usage };
      }
    }
  }

  const daily = Array.from(dailyMap.values()).sort((a, b) => a.date.localeCompare(b.date));

  return {
    daily,
    model_usage: modelUsage,
    total_sessions: totalSessions,
    total_messages: totalMessages,
    first_session_date: firstDate,
  };
}
