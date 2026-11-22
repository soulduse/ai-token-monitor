import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { AllStats, FxRates } from "../../lib/types";
import { useI18n } from "../../i18n/I18nContext";
import { useSettings } from "../../contexts/SettingsContext";
import { useShareImage } from "../../hooks/useShareImage";
import { useFxRates } from "../../hooks/useFxRates";
import { useSubscriptions, type DetectedSubscriptions } from "../../hooks/useSubscriptions";
import { PLAN_LABELS } from "../../lib/plans";
import {
  FEATURED_CURRENCIES,
  currencyToUsd,
  defaultCurrency,
  parseAmount,
  formatMoney,
  resolveCurrency,
  usdToCurrency,
} from "../../lib/currency";
import {
  buildPeriods,
  formatMultiple,
  multipleEmoji,
  multipleGauge,
  proratedSubscription,
  subscriptionMultiple,
  type WhatIfPeriodId,
} from "../../lib/whatIf";

interface Props {
  onClose: () => void;
  stats: AllStats;
}

/** 빠른 선택 금액(USD/월). 플랜 정가와 같은 값이라 대부분 한 번에 고른다. */
const QUICK_AMOUNTS_USD = [20, 100, 200, 400];

export function WhatIfOverlay({ onClose, stats }: Props) {
  const t = useI18n();
  const { prefs, updatePrefs } = useSettings();
  const rates = useFxRates();
  const subs = useSubscriptions(stats, rates);
  const cardRef = useRef<HTMLDivElement>(null);
  const { capture, captured } = useShareImage(cardRef);
  const [selected, setSelected] = useState<WhatIfPeriodId>("last30");
  const [editing, setEditing] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const currency = resolveCurrency(prefs.display_currency ?? defaultCurrency(prefs.language), rates);
  const money = (usd: number) => formatMoney(usdToCurrency(usd, currency, rates), currency);

  const periods = useMemo(() => buildPeriods(stats.daily), [stats.daily]);
  const period = periods.find((p) => p.id === selected) ?? periods[0];
  const multiple = subscriptionMultiple(period, subs.monthlyUsd);

  useEffect(() => {
    // Capture phase + preventDefault: main.tsx's window-level Escape handler would otherwise hide the window.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      if (editing) setEditing(false);
      else onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [editing, onClose]);

  const currencyOptions = useMemo(() => {
    const available = rates ? Object.keys(rates.rates) : ["USD"];
    const featured = FEATURED_CURRENCIES.filter((c) => available.includes(c));
    const rest = available.filter((c) => !featured.includes(c)).sort();
    return [...featured, ...rest];
  }, [rates]);

  const maxDaily = Math.max(...period.dailyCostUsd, 0.0001);

  return (
    <>
      <div onClick={onClose} style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.6)", zIndex: 60 }} />
      <div
        onClick={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
        style={{
        position: "fixed",
        inset: 0,
        zIndex: 61,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        padding: "16px 12px",
        overflowY: "auto",
      }}>
        <button onClick={onClose} aria-label={t("whatIf.close")} style={closeButtonStyle}>✕</button>

        <div ref={cardRef} style={cardStyle}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
            <span style={{ fontSize: 14, fontWeight: 800, color: "var(--text-primary)" }}>💸 {t("whatIf.title")}</span>
            <select
              value={currency}
              onChange={(e) => updatePrefs({ display_currency: e.target.value })}
              aria-label={t("whatIf.currency")}
              style={selectStyle}
            >
              {currencyOptions.map((c) => <option key={c} value={c}>{c}</option>)}
            </select>
          </div>

          <div style={{ display: "flex", gap: 5, flexWrap: "wrap", marginTop: 10 }}>
            {periods.map((p) => (
              <button key={p.id} onClick={() => setSelected(p.id)} style={chipStyle(p.id === selected)}>
                {t(`whatIf.period.${p.id}`)}
              </button>
            ))}
          </div>

          <div style={{ fontSize: 10, color: "var(--text-secondary)", marginTop: 12, fontWeight: 600 }}>
            {t("whatIf.apiCost")}
          </div>
          <div style={{ fontSize: 30, fontWeight: 800, letterSpacing: "-0.5px", color: "var(--text-primary)", lineHeight: 1.15 }}>
            {money(period.costUsd)}
          </div>
          {currency !== "USD" && (
            <div style={{ fontSize: 10, color: "var(--text-secondary)" }}>{formatMoney(period.costUsd, "USD")}</div>
          )}

          {multiple !== null && (
            <>
              <div style={{ fontSize: 12, color: "var(--text-secondary)", marginTop: 8 }}>
                {t("whatIf.vsSubscription", { amount: money(proratedSubscription(period, subs.monthlyUsd)) })}{" "}
                <b style={{ color: "var(--accent-orange)", fontSize: 14 }}>{formatMultiple(multiple)}×</b>{" "}
                {multipleEmoji(multiple)}
              </div>
              <div style={{ height: 8, borderRadius: 999, background: "var(--heat-0, rgba(127,127,127,0.18))", overflow: "hidden", marginTop: 6 }}>
                <div style={{
                  width: `${multipleGauge(multiple) * 100}%`,
                  height: "100%",
                  borderRadius: 999,
                  background: "linear-gradient(90deg, var(--accent-mint), var(--accent-orange))",
                  transition: "width 0.3s ease",
                }} />
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", fontSize: 9, color: "var(--text-secondary)", marginTop: 2 }}>
                <span>1×</span><span>10×</span><span>50×+</span>
              </div>
            </>
          )}

          {period.dailyCostUsd.length > 1 && (
            <div style={{ display: "flex", alignItems: "flex-end", gap: 2, height: 36, marginTop: 10 }}>
              {period.dailyCostUsd.map((c, i) => (
                <div key={i} style={{
                  flex: 1,
                  height: `${Math.max(2, (c / maxDaily) * 100)}%`,
                  background: "var(--accent-purple)",
                  opacity: c > 0 ? 0.8 : 0.2,
                  borderRadius: "2px 2px 0 0",
                }} />
              ))}
            </div>
          )}

          <table style={{ width: "100%", borderCollapse: "collapse", marginTop: 10, fontSize: 12, fontVariantNumeric: "tabular-nums" }}>
            <tbody>
              {periods.map((p) => {
                const m = subscriptionMultiple(p, subs.monthlyUsd);
                const on = p.id === selected;
                return (
                  <tr key={p.id} onClick={() => setSelected(p.id)} style={{ cursor: "pointer", color: on ? "var(--accent-purple)" : "var(--text-primary)", fontWeight: on ? 700 : 500 }}>
                    <td style={cellStyle}>{t(`whatIf.period.${p.id}`)}</td>
                    <td style={{ ...cellStyle, textAlign: "right" }}>{money(p.costUsd)}</td>
                    {subs.monthlyUsd > 0 && (
                      <td style={{ ...cellStyle, textAlign: "right", width: 48, color: on ? undefined : "var(--text-secondary)" }}>
                        {m === null ? "—" : `${formatMultiple(m)}×`}
                      </td>
                    )}
                  </tr>
                );
              })}
            </tbody>
          </table>

          <SubscriptionBox
            subs={subs}
            money={money}
            expanded={expanded}
            onToggle={() => setExpanded((v) => !v)}
            onEdit={() => setEditing(true)}
          />

          <div style={{ fontSize: 9, color: "var(--text-secondary)", marginTop: 8, lineHeight: 1.4 }}>
            {t("whatIf.footnote")}
            {subs.monthlyUsd > 0 && ` ${t("whatIf.footnoteProrate")}`}
            {rates && currency !== "USD" && ` · ${t("whatIf.rateDate", { date: rates.date })}${rates.stale ? " ⚠" : ""}`}
          </div>
        </div>

        {editing && (
          <SubscriptionEditor
            rates={rates}
            currency={currency}
            current={prefs.subscription_override ?? null}
            onSave={(amount, cur) => {
              updatePrefs({
                subscription_override: { amount, currency: cur, amount_usd: currencyToUsd(amount, cur, rates) },
              });
              setEditing(false);
            }}
            onReset={() => {
              updatePrefs({ subscription_override: null });
              setEditing(false);
            }}
            onCancel={() => setEditing(false)}
          />
        )}

        <button onClick={capture} style={shareButtonStyle(captured)}>
          {captured ? (
            <>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="20 6 9 17 4 12"/>
              </svg>
              {t("receipt.copied")}
            </>
          ) : (
            <>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8"/>
                <polyline points="16 6 12 2 8 6"/>
                <line x1="12" y1="2" x2="12" y2="15"/>
              </svg>
              {t("whatIf.share")}
            </>
          )}
        </button>
      </div>
    </>
  );
}

function SubscriptionBox({ subs, money, expanded, onToggle, onEdit }: {
  subs: DetectedSubscriptions;
  money: (usd: number) => string;
  expanded: boolean;
  onToggle: () => void;
  onEdit: () => void;
}) {
  const t = useI18n();

  if (subs.source === "none") {
    return (
      <div style={boxStyle}>
        <div style={{ fontSize: 11, color: "var(--text-secondary)" }}>{t("whatIf.noSubscription")}</div>
        <button onClick={onEdit} style={{ ...linkStyle, marginTop: 4 }}>{t("whatIf.enterSubscription")} →</button>
      </div>
    );
  }

  const badge = subs.source === "override" ? t("whatIf.source.override")
    : subs.source === "teamai" ? t("whatIf.source.teamai")
    : t("whatIf.source.login");
  const activeCount = subs.lines.filter((l) => !l.inactive).reduce((a, l) => a + l.count, 0);

  return (
    <div style={boxStyle}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 8 }}>
        <span style={badgeStyle(subs.source === "override")}>{badge}</span>
        {subs.lines.length > 0 ? (
          <button onClick={onToggle} style={linkStyle}>
            {expanded ? t("whatIf.collapse") : t("whatIf.expand", { count: activeCount })}
          </button>
        ) : (
          <button onClick={onEdit} style={linkStyle}>{t("whatIf.edit")}</button>
        )}
      </div>

      {expanded && subs.lines.map((l) => (
        <div key={`${l.plan}:${l.inactive}`} style={{
          display: "flex",
          justifyContent: "space-between",
          fontSize: 11,
          marginTop: 4,
          color: "var(--text-primary)",
          opacity: l.inactive ? 0.45 : 1,
          textDecoration: l.inactive ? "line-through" : "none",
        }}>
          <span>
            {PLAN_LABELS[l.plan]}
            {l.inactive && <span style={{ color: "var(--text-secondary)" }}> · {t("whatIf.inactive")}</span>}
          </span>
          <span>
            {l.monthlyUsd === null ? t("whatIf.unpriced") : money(l.monthlyUsd / l.count)}
            <span style={{ color: "var(--text-secondary)" }}> × {l.count}</span>
          </span>
        </div>
      ))}

      <div style={{
        display: "flex",
        justifyContent: "space-between",
        fontSize: 12,
        fontWeight: 800,
        marginTop: 6,
        paddingTop: expanded ? 6 : 0,
        borderTop: expanded ? "1px solid rgba(127,127,127,0.2)" : "none",
        color: "var(--text-primary)",
      }}>
        <span>{t("whatIf.monthlyTotal")}</span>
        <span>{money(subs.monthlyUsd)}</span>
      </div>

      {subs.source !== "override" && (
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginTop: 4, fontSize: 10, color: "var(--text-secondary)" }}>
          <span>{subs.hasUnpriced ? t("whatIf.unpricedHint") : t("whatIf.listPriceHint")}</span>
          <button onClick={onEdit} style={linkStyle}>{t("whatIf.editAmount")}</button>
        </div>
      )}
    </div>
  );
}

function SubscriptionEditor({ rates, currency, current, onSave, onReset, onCancel }: {
  rates: FxRates | null;
  currency: string;
  current: { amount: number; currency: string } | null;
  onSave: (amount: number, currency: string) => void;
  onReset: () => void;
  onCancel: () => void;
}) {
  const t = useI18n();
  const [custom, setCustom] = useState(
    current && current.currency === currency ? String(current.amount) : "",
  );
  const currentUsd = current ? currencyToUsd(current.amount, current.currency, rates) : null;
  const customAmount = parseAmount(custom);

  return (
    <div style={{ ...cardStyle, marginTop: 10 }}>
      <div style={{ fontSize: 13, fontWeight: 800, color: "var(--text-primary)" }}>{t("whatIf.editor.title")}</div>
      <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 4 }}>{t("whatIf.editor.quick")}</div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 6, marginTop: 6 }}>
        {QUICK_AMOUNTS_USD.map((usd) => {
          const on = currentUsd !== null && Math.abs(currentUsd - usd) < 0.5 && current?.currency === "USD";
          return (
            <button key={usd} onClick={() => onSave(usd, "USD")} style={amountButtonStyle(on)}>
              ${usd}
              {currency !== "USD" && rates && (
                <span style={{ display: "block", fontSize: 9, fontWeight: 500, color: "var(--text-secondary)" }}>
                  ≈{formatMoney(usdToCurrency(usd, currency, rates), currency)}
                </span>
              )}
            </button>
          );
        })}
      </div>
      <div style={{ fontSize: 11, color: "var(--text-secondary)", marginTop: 10 }}>{t("whatIf.editor.custom", { currency })}</div>
      <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
        <input
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          inputMode="decimal"
          placeholder={currency === "KRW" ? "290000" : "200"}
          style={inputStyle}
        />
        <button
          disabled={!(customAmount > 0)}
          onClick={() => onSave(customAmount, currency)}
          style={{ ...amountButtonStyle(true), padding: "0 14px", opacity: customAmount > 0 ? 1 : 0.4 }}
        >
          {t("whatIf.editor.save")}
        </button>
      </div>
      <div style={{ fontSize: 10, color: "var(--text-secondary)", marginTop: 8 }}>{t("whatIf.editor.hint")}</div>
      <div style={{ display: "flex", justifyContent: "space-between", marginTop: 10 }}>
        <button onClick={onCancel} style={linkStyle}>{t("whatIf.editor.cancel")}</button>
        {current && <button onClick={onReset} style={linkStyle}>{t("whatIf.editor.reset")}</button>}
      </div>
    </div>
  );
}

const cardStyle: CSSProperties = {
  width: "100%",
  maxWidth: 360,
  background: "var(--bg-card)",
  borderRadius: "var(--radius-lg)",
  padding: 16,
  boxShadow: "var(--shadow-card)",
  marginTop: 36,
};

const cellStyle: CSSProperties = {
  padding: "6px 0",
  borderBottom: "1px dashed rgba(127,127,127,0.25)",
};

const boxStyle: CSSProperties = {
  marginTop: 12,
  background: "var(--bg-primary)",
  borderRadius: "var(--radius-md)",
  padding: "10px 12px",
};

const linkStyle: CSSProperties = {
  background: "none",
  border: "none",
  padding: 0,
  cursor: "pointer",
  color: "var(--accent-purple)",
  fontSize: 11,
  fontWeight: 700,
};

const selectStyle: CSSProperties = {
  background: "var(--bg-primary)",
  color: "var(--text-primary)",
  border: "1px solid rgba(127,127,127,0.3)",
  borderRadius: "var(--radius-sm)",
  fontSize: 11,
  fontWeight: 700,
  padding: "3px 6px",
};

const inputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  background: "var(--bg-primary)",
  color: "var(--text-primary)",
  border: "1px solid rgba(127,127,127,0.3)",
  borderRadius: "var(--radius-sm)",
  fontSize: 12,
  padding: "7px 8px",
};

const closeButtonStyle: CSSProperties = {
  position: "fixed",
  top: 12,
  right: 12,
  background: "rgba(255,255,255,0.15)",
  border: "none",
  borderRadius: 20,
  width: 32,
  height: 32,
  cursor: "pointer",
  color: "#fff",
  fontSize: 16,
  fontWeight: 700,
  zIndex: 62,
};

function chipStyle(on: boolean): CSSProperties {
  return {
    fontSize: 11,
    fontWeight: 700,
    padding: "4px 9px",
    borderRadius: 999,
    cursor: "pointer",
    border: on ? "1px solid var(--accent-purple)" : "1px solid rgba(127,127,127,0.3)",
    background: on ? "var(--accent-purple)" : "transparent",
    color: on ? "#fff" : "var(--text-secondary)",
  };
}

function badgeStyle(manual: boolean): CSSProperties {
  return {
    fontSize: 10,
    fontWeight: 700,
    padding: "1px 8px",
    borderRadius: 999,
    // 테마 팔레트와 어긋나지 않게 배경 대신 글자색과 같은 테두리를 쓴다.
    border: "1px solid currentColor",
    color: manual ? "var(--accent-orange)" : "var(--accent-mint)",
  };
}

function amountButtonStyle(on: boolean): CSSProperties {
  return {
    padding: "8px 0",
    borderRadius: "var(--radius-sm)",
    cursor: "pointer",
    border: on ? "1px solid var(--accent-purple)" : "1px solid rgba(127,127,127,0.3)",
    background: on ? "var(--accent-purple)" : "var(--bg-primary)",
    color: on ? "#fff" : "var(--text-primary)",
    fontSize: 12,
    fontWeight: 800,
  };
}

function shareButtonStyle(captured: boolean): CSSProperties {
  return {
    marginTop: 12,
    padding: "8px 24px",
    borderRadius: 20,
    border: "none",
    cursor: "pointer",
    background: captured ? "var(--accent-mint)" : "linear-gradient(135deg, var(--accent-purple), var(--accent-pink))",
    color: "#fff",
    fontSize: 12,
    fontWeight: 700,
    display: "flex",
    alignItems: "center",
    gap: 6,
    transition: "all 0.2s ease",
  };
}
