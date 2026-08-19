import { useMemo } from "react";
import { useTokenStats } from "./useTokenStats";
import type { AllStats, AnalyticsData, DailyUsage, ModelUsage, ProjectUsage } from "../lib/types";

interface UseCombinedStatsProps {
  includeClaude: boolean;
  includeCodex: boolean;
  includeOpencode: boolean;
  includeKimi: boolean;
  includeGlm: boolean;
  includeGjc: boolean;
  includeGrok: boolean;
  includeKiro: boolean;
}

export function useCombinedStats({ includeClaude, includeCodex, includeOpencode, includeKimi, includeGlm, includeGjc, includeGrok, includeKiro }: UseCombinedStatsProps) {
  const claude = useTokenStats("claude");
  const codex = useTokenStats("codex");
  const opencode = useTokenStats("opencode");
  const kimi = useTokenStats("kimi");
  const glm = useTokenStats("glm");
  const gjc = useTokenStats("gjc");
  const grok = useTokenStats("grok");
  const kiro = useTokenStats("kiro");

  const stats = useMemo<AllStats | null>(() => {
    const sources: (AllStats | null)[] = [];
    if (includeClaude) sources.push(claude.stats);
    if (includeCodex) sources.push(codex.stats);
    if (includeOpencode) sources.push(opencode.stats);
    if (includeKimi) sources.push(kimi.stats);
    if (includeGlm) sources.push(glm.stats);
    if (includeGjc) sources.push(gjc.stats);
    if (includeGrok) sources.push(grok.stats);
    if (includeKiro) sources.push(kiro.stats);

    const validStats = sources.filter((s): s is AllStats => s !== null);
    // Every pushed source was null, so any per-provider fallback here would just
    // hand back one of those same nulls.
    if (validStats.length === 0) return null;
    if (validStats.length === 1) return validStats[0];

    return mergeStats(validStats);
  }, [claude.stats, codex.stats, opencode.stats, kimi.stats, glm.stats, gjc.stats, grok.stats, kiro.stats, includeClaude, includeCodex, includeOpencode, includeKimi, includeGlm, includeGjc, includeGrok, includeKiro]);

  const loading = (includeClaude && claude.loading) || (includeCodex && codex.loading) || (includeOpencode && opencode.loading) || (includeKimi && kimi.loading) || (includeGlm && glm.loading) || (includeGjc && gjc.loading) || (includeGrok && grok.loading) || (includeKiro && kiro.loading);
  const error = useMemo(() => {
    if (stats) return null;

    if (includeClaude && claude.error) return claude.error;
    if (includeCodex && codex.error) return codex.error;
    if (includeOpencode && opencode.error) return opencode.error;
    if (includeKimi && kimi.error) return kimi.error;
    if (includeGlm && glm.error) return glm.error;
    if (includeGjc && gjc.error) return gjc.error;
    if (includeGrok && grok.error) return grok.error;
    if (includeKiro && kiro.error) return kiro.error;

    return null;
  }, [stats, includeClaude, includeCodex, includeOpencode, includeKimi, includeGlm, includeGjc, includeGrok, includeKiro, claude.error, codex.error, opencode.error, kimi.error, glm.error, gjc.error, grok.error, kiro.error]);

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
        // A combined day counts as restored only when every contributing
        // provider's day was restored — any local slice makes it local.
        existing.hydrated = !!existing.hydrated && !!d.hydrated;
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
          hydrated: !!d.hydrated,
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

  const analytics = mergeAnalytics(statsList);
  const rate_limits = statsList.find(s => s.rate_limits)?.rate_limits;

  return {
    daily,
    model_usage: modelUsage,
    total_sessions: totalSessions,
    total_messages: totalMessages,
    first_session_date: firstDate,
    analytics,
    rate_limits,
  };
}

function mergeCounts(
  into: Map<string, number>,
  rows: { name: string; count: number }[],
) {
  for (const row of rows) {
    into.set(row.name, (into.get(row.name) ?? 0) + row.count);
  }
}

function sortedCounts(map: Map<string, number>): { name: string; count: number }[] {
  return [...map.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count);
}

function mergeAnalytics(statsList: AllStats[]): AnalyticsData | undefined {
  const pieces = statsList
    .map((s) => s.analytics)
    .filter((a): a is AnalyticsData => !!a);
  if (pieces.length === 0) return undefined;
  if (pieces.length === 1) return pieces[0];

  const projects = new Map<string, ProjectUsage>();
  const tools = new Map<string, number>();
  const shells = new Map<string, number>();
  const mcps = new Map<string, number>();
  const activities = new Map<string, { category: string; cost_usd: number; messages: number }>();

  for (const a of pieces) {
    for (const p of a.project_usage) {
      const existing = projects.get(p.name);
      if (existing) {
        existing.cost_usd += p.cost_usd;
        existing.tokens += p.tokens;
        existing.sessions += p.sessions;
        existing.messages += p.messages;
      } else {
        projects.set(p.name, { ...p });
      }
    }
    mergeCounts(tools, a.tool_usage);
    mergeCounts(shells, a.shell_commands);
    for (const m of a.mcp_usage) {
      mcps.set(m.server, (mcps.get(m.server) ?? 0) + m.calls);
    }
    for (const act of a.activity_breakdown) {
      const existing = activities.get(act.category);
      if (existing) {
        existing.cost_usd += act.cost_usd;
        existing.messages += act.messages;
      } else {
        activities.set(act.category, { ...act });
      }
    }
  }

  return {
    project_usage: [...projects.values()].sort((a, b) => b.cost_usd - a.cost_usd),
    tool_usage: sortedCounts(tools),
    shell_commands: sortedCounts(shells),
    mcp_usage: [...mcps.entries()]
      .map(([server, calls]) => ({ server, calls }))
      .sort((a, b) => b.calls - a.calls),
    activity_breakdown: [...activities.values()].sort((a, b) => b.cost_usd - a.cost_usd),
  };
}
