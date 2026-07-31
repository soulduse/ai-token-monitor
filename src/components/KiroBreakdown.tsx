import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "../i18n/I18nContext";

/** Mirrors `providers::kiro::KiroTurn`. */
interface KiroTurn {
  ended_at: string;
  date: string;
  model: string;
  credits: number;
  cost_usd: number;
  end_reason: string;
  cancelled: boolean;
  requests: number;
  tool_calls: number;
  context_percent: number | null;
  duration_secs: number;
  project_path: string;
  project: string;
  source: string;
  session_id: string;
}

interface KiroModelRollup {
  model: string;
  credits: number;
  cost_usd: number;
  turns: number;
  requests: number;
  is_auto: boolean;
}

interface KiroDayRollup {
  date: string;
  credits: number;
  cost_usd: number;
  turns: number;
  cancelled_turns: number;
}

interface KiroProjectRollup {
  project: string;
  project_path: string;
  credits: number;
  cost_usd: number;
  turns: number;
}

/** Mirrors `providers::kiro::KiroBreakdown`. */
interface KiroBreakdownData {
  total_credits: number;
  total_cost_usd: number;
  credit_rate_usd: number;
  total_turns: number;
  total_requests: number;
  total_tool_calls: number;
  total_sessions: number;
  cancelled_turns: number;
  cancelled_credits: number;
  auto_credits: number;
  first_turn_date: string | null;
  last_turn_date: string | null;
  by_model: KiroModelRollup[];
  by_day: KiroDayRollup[];
  by_project: KiroProjectRollup[];
  recent_turns: KiroTurn[];
  session_store_turns: number;
  sqlite_store_turns: number;
  tokens_unavailable: boolean;
}

const card = {
  background: "var(--bg-card)",
  borderRadius: "var(--radius-lg)",
  boxShadow: "var(--shadow-card)",
  padding: 12,
} as const;

const label = {
  fontSize: 10,
  fontWeight: 700,
  color: "var(--text-muted)",
  letterSpacing: "0.4px",
  textTransform: "uppercase",
} as const;

const credits = (n: number) => n.toFixed(n < 1 ? 4 : 2);
const usd = (n: number) => `$${n.toFixed(n < 1 ? 4 : 2)}`;

function Stat({ title, value, sub }: { title: string; value: string; sub?: string }) {
  return (
    <div style={{ ...card, flex: 1, minWidth: 96 }}>
      <div style={label}>{title}</div>
      <div style={{ fontSize: 18, fontWeight: 800, color: "var(--text-primary)", marginTop: 2 }}>
        {value}
      </div>
      {sub && (
        <div style={{ fontSize: 10, color: "var(--text-muted)", fontWeight: 500, marginTop: 2 }}>
          {sub}
        </div>
      )}
    </div>
  );
}

/** Horizontal proportion bar used by the model/project/day rollups. */
function Bar({ ratio, muted }: { ratio: number; muted?: boolean }) {
  return (
    <div style={{ height: 4, background: "var(--heat-0)", borderRadius: 2, overflow: "hidden" }}>
      <div
        style={{
          width: `${Math.max(1, ratio * 100)}%`,
          height: "100%",
          background: muted ? "var(--text-muted)" : "var(--accent-purple)",
          borderRadius: 2,
        }}
      />
    </div>
  );
}

export function KiroBreakdown() {
  const t = useI18n();
  const [data, setData] = useState<KiroBreakdownData | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    invoke<KiroBreakdownData>("get_kiro_breakdown")
      .then((d) => {
        setData(d);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    load();
    // The Rust side re-parses on file-watcher events and emits this; refresh so
    // a turn that just landed shows up without a tab switch. Kiro's session
    // `.json` is only rewritten when a turn ends, so the poll is the backstop
    // for the case where no watched path changed in between.
    const unlisten = listen("stats-updated", load).catch(() => null);
    const interval = setInterval(load, 60_000);
    return () => {
      unlisten.then((fn) => fn?.());
      clearInterval(interval);
    };
  }, [load]);

  if (error) {
    return (
      <div style={{ ...card, fontSize: 11, color: "var(--text-muted)", fontWeight: 500 }}>
        {error}
      </div>
    );
  }
  if (!data) {
    return (
      <div style={{ ...card, fontSize: 11, color: "var(--text-muted)", fontWeight: 500 }}>
        {t("kiro.loading")}
      </div>
    );
  }
  if (data.total_turns === 0) {
    return (
      <div style={{ ...card, fontSize: 11, color: "var(--text-muted)", fontWeight: 500 }}>
        {t("kiro.empty")}
      </div>
    );
  }

  const maxModel = Math.max(...data.by_model.map((m) => m.credits), 1);
  const maxProject = Math.max(...data.by_project.map((p) => p.credits), 1);
  const maxDay = Math.max(...data.by_day.map((d) => d.credits), 1);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {/* Kiro bills credits, not tokens. Say so up front so the absent token
          charts read as a property of Kiro rather than a broken provider. */}
      <div
        style={{
          ...card,
          background: "rgba(124, 92, 252, 0.05)",
          border: "1px solid rgba(124, 92, 252, 0.15)",
          fontSize: 11,
          lineHeight: 1.6,
          color: "var(--text-secondary)",
          fontWeight: 500,
        }}
      >
        {t("kiro.creditsNotice")}
      </div>

      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <Stat
          title={t("kiro.stat.credits")}
          value={credits(data.total_credits)}
          sub={`${usd(data.total_cost_usd)} @ ${usd(data.credit_rate_usd)}/cr`}
        />
        <Stat
          title={t("kiro.stat.turns")}
          value={String(data.total_turns)}
          sub={t("kiro.stat.turnsSub")
            .replace("{requests}", String(data.total_requests))
            .replace("{sessions}", String(data.total_sessions))}
        />
        <Stat title={t("kiro.stat.tools")} value={String(data.total_tool_calls)} />
      </div>

      {/* Two facts a naive reading of the numbers would get wrong. */}
      {(data.cancelled_turns > 0 || data.auto_credits > 0) && (
        <div style={{ ...card, display: "flex", flexDirection: "column", gap: 6 }}>
          {data.cancelled_turns > 0 && (
            <div style={{ fontSize: 11, color: "var(--text-secondary)", fontWeight: 500 }}>
              {t("kiro.cancelledNote")
                .replace("{turns}", String(data.cancelled_turns))
                .replace("{credits}", credits(data.cancelled_credits))}
            </div>
          )}
          {data.auto_credits > 0 && (
            <div style={{ fontSize: 11, color: "var(--text-secondary)", fontWeight: 500 }}>
              {t("kiro.autoNote").replace("{credits}", credits(data.auto_credits))}
            </div>
          )}
        </div>
      )}

      <section style={card}>
        <div style={{ ...label, marginBottom: 8 }}>{t("kiro.byModel")}</div>
        {data.by_model.map((m) => (
          <div key={m.model} style={{ marginBottom: 8 }}>
            <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, marginBottom: 3 }}>
              <span style={{ fontWeight: 700, color: "var(--text-primary)" }}>
                {m.is_auto ? t("kiro.autoModel") : m.model}
              </span>
              <span style={{ fontWeight: 600, color: "var(--text-secondary)", fontVariantNumeric: "tabular-nums" }}>
                {credits(m.credits)} cr · {usd(m.cost_usd)} · {m.turns}t
              </span>
            </div>
            <Bar ratio={m.credits / maxModel} muted={m.is_auto} />
          </div>
        ))}
      </section>

      <section style={card}>
        <div style={{ ...label, marginBottom: 8 }}>{t("kiro.byProject")}</div>
        {data.by_project.map((p) => (
          <div key={p.project_path} style={{ marginBottom: 8 }}>
            <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, marginBottom: 3 }}>
              <span
                style={{ fontWeight: 700, color: "var(--text-primary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                title={p.project_path}
              >
                {p.project}
              </span>
              <span style={{ fontWeight: 600, color: "var(--text-secondary)", fontVariantNumeric: "tabular-nums", flexShrink: 0, marginLeft: 8 }}>
                {credits(p.credits)} cr · {usd(p.cost_usd)}
              </span>
            </div>
            <Bar ratio={p.credits / maxProject} />
          </div>
        ))}
      </section>

      <section style={card}>
        <div style={{ ...label, marginBottom: 8 }}>{t("kiro.byDay")}</div>
        {data.by_day.map((d) => (
          <div key={d.date} style={{ marginBottom: 8 }}>
            <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, marginBottom: 3 }}>
              <span style={{ fontWeight: 700, color: "var(--text-primary)" }}>{d.date}</span>
              <span style={{ fontWeight: 600, color: "var(--text-secondary)", fontVariantNumeric: "tabular-nums" }}>
                {credits(d.credits)} cr · {d.turns}t
                {d.cancelled_turns > 0 && ` · ${d.cancelled_turns}✕`}
              </span>
            </div>
            <Bar ratio={d.credits / maxDay} />
          </div>
        ))}
      </section>

      <section style={card}>
        <div style={{ ...label, marginBottom: 8 }}>{t("kiro.recentTurns")}</div>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {data.recent_turns.map((turn) => (
            <div
              key={`${turn.session_id}-${turn.ended_at}`}
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "baseline",
                gap: 8,
                paddingBottom: 6,
                borderBottom: "1px solid var(--heat-0)",
              }}
            >
              <div style={{ minWidth: 0 }}>
                <div style={{ fontSize: 11, fontWeight: 700, color: "var(--text-primary)" }}>
                  {turn.cancelled && (
                    <span style={{ color: "var(--text-muted)", marginRight: 4 }}>✕</span>
                  )}
                  {turn.model === "auto" ? t("kiro.autoModel") : turn.model}
                </div>
                <div
                  style={{ fontSize: 10, color: "var(--text-muted)", fontWeight: 500, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                  title={`${turn.project_path} · ${turn.end_reason} · ${turn.source}`}
                >
                  {new Date(turn.ended_at).toLocaleString()} · {turn.project} · {turn.requests}req
                  {turn.tool_calls > 0 && ` · ${turn.tool_calls}tool`}
                  {turn.context_percent != null && ` · ctx ${turn.context_percent.toFixed(1)}%`}
                </div>
              </div>
              <div style={{ fontSize: 11, fontWeight: 700, color: "var(--text-primary)", fontVariantNumeric: "tabular-nums", flexShrink: 0 }}>
                {credits(turn.credits)} cr
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* Where the numbers came from, and why they are a floor. */}
      <div style={{ ...card, fontSize: 10, color: "var(--text-muted)", fontWeight: 500, lineHeight: 1.6 }}>
        {t("kiro.sourceNote")
          .replace("{session}", String(data.session_store_turns))
          .replace("{sqlite}", String(data.sqlite_store_turns))}
        <div style={{ marginTop: 4 }}>{t("kiro.undercountNote")}</div>
      </div>
    </div>
  );
}
