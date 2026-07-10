import { useCallback, useEffect, useLayoutEffect, useState } from "react";
import type { CSSProperties } from "react";
import { useI18n } from "../i18n/I18nContext";
import {
  useOnboarding,
  loadOnboardingState,
  saveOnboardingState,
} from "../contexts/OnboardingContext";
import type { TabType } from "./TabBar";

interface Props {
  activeTab: TabType;
  onRequestTab: (tab: TabType) => void;
}

type TourStepId =
  | "welcome"
  | "sources"
  | "today"
  | "heatmap"
  | "analytics"
  | "leaderboard"
  | "chat"
  | "settings"
  | "done";

interface TourStep {
  id: TourStepId;
  /** data-tour attribute of the highlighted element; none = centered card. */
  anchor?: string;
  /** Tab that must be active while this step is showing. */
  tab: TabType;
}

const STEPS: TourStep[] = [
  { id: "welcome", tab: "overview" },
  { id: "sources", anchor: "source-selector", tab: "overview" },
  { id: "today", anchor: "today-summary", tab: "overview" },
  { id: "heatmap", anchor: "heatmap", tab: "overview" },
  { id: "analytics", anchor: "tab-analytics", tab: "analytics" },
  { id: "leaderboard", anchor: "tab-leaderboard", tab: "leaderboard" },
  { id: "chat", anchor: "tab-chat", tab: "chat" },
  { id: "settings", anchor: "settings-button", tab: "overview" },
  { id: "done", tab: "overview" },
];
const LAST = STEPS.length - 1;

const DIM = "rgba(0, 0, 0, 0.55)";
const CARD_EST_HEIGHT = 170;

function findAnchor(step: TourStep): Element | null {
  return step.anchor
    ? document.querySelector(`[data-tour="${step.anchor}"]`)
    : null;
}

/** Gate component: decides whether the tour should auto-start (first run,
 *  popover visible, no stored record) and mounts the tour only while active,
 *  so each activation starts fresh from the welcome step. */
export function OnboardingOverlay({ activeTab, onRequestTab }: Props) {
  const { active, start } = useOnboarding();

  useEffect(() => {
    if (loadOnboardingState() !== null) return;
    if (document.visibilityState === "visible") {
      start();
      return;
    }
    // Popover mounted hidden (tray app) — wait for the first real open.
    const onVis = () => {
      if (document.visibilityState === "visible") {
        document.removeEventListener("visibilitychange", onVis);
        start();
      }
    };
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  }, [start]);

  if (!active) return null;
  return <Tour activeTab={activeTab} onRequestTab={onRequestTab} />;
}

function Tour({ activeTab, onRequestTab }: Props) {
  const t = useI18n();
  const { stop } = useOnboarding();
  const [stepIndex, setStepIndex] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const step = STEPS[stepIndex];

  const finish = useCallback(
    (result: "completed" | "skipped") => {
      saveOnboardingState(
        result === "completed"
          ? { version: 1, completedAt: Date.now() }
          : { version: 1, skippedAtStep: stepIndex },
      );
      onRequestTab("overview");
      stop();
    },
    [stepIndex, onRequestTab, stop],
  );

  // Navigate while skipping steps whose anchor is absent (e.g. SourceSelector
  // renders null when only one provider is available).
  const goTo = useCallback((index: number, dir: 1 | -1) => {
    let i = index;
    while (i > 0 && i < LAST && STEPS[i].anchor && !findAnchor(STEPS[i])) {
      i += dir;
    }
    setStepIndex(Math.max(0, Math.min(LAST, i)));
  }, []);

  // Keep the required tab active. useLayoutEffect so the switch happens before
  // paint and the measuring effect below re-runs with final layout.
  useLayoutEffect(() => {
    onRequestTab(step.tab);
  }, [step.tab, onRequestTab]);

  // Measure the anchor on step entry / tab switch.
  useLayoutEffect(() => {
    if (!step.anchor) {
      setRect(null);
      return;
    }
    const el = findAnchor(step);
    if (!el) {
      setRect(null);
      return;
    }
    // "instant" overrides PopoverShell's smooth scrollBehavior so the rect
    // read right after is final.
    el.scrollIntoView({ block: "nearest", behavior: "instant" });
    const r = el.getBoundingClientRect();
    setRect(r.width > 0 ? r : null);
  }, [step, activeTab]);

  // Track the anchor while the step is up: container scroll (capture — the
  // scroller is PopoverShell's inner div), resize, popover re-shown.
  useEffect(() => {
    if (!step.anchor) return;
    let raf = 0;
    const remeasure = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const el = findAnchor(step);
        if (!el) {
          // Anchor vanished mid-step (e.g. provider availability changed).
          goTo(stepIndex + 1, 1);
          return;
        }
        const r = el.getBoundingClientRect();
        setRect(r.width > 0 ? r : null);
      });
    };
    const onVis = () => {
      if (document.visibilityState === "visible") remeasure();
    };
    window.addEventListener("scroll", remeasure, true);
    window.addEventListener("resize", remeasure);
    document.addEventListener("visibilitychange", onVis);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("scroll", remeasure, true);
      window.removeEventListener("resize", remeasure);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [step, stepIndex, goTo]);

  // Escape ends the tour but must not close the popover window: capture-phase
  // + preventDefault, same pattern as OAuthFallbackModal / SettingsOverlay.
  useEffect(() => {
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        finish(stepIndex === LAST ? "completed" : "skipped");
      }
    };
    document.addEventListener("keydown", handleEsc, true);
    return () => document.removeEventListener("keydown", handleEsc, true);
  }, [finish, stepIndex]);

  const spotlight = step.anchor != null && rect != null;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1200,
        background: spotlight ? "transparent" : DIM,
        display: spotlight ? "block" : "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      {spotlight && (
        <div
          style={{
            position: "fixed",
            top: rect.top - 6,
            left: rect.left - 6,
            width: rect.width + 12,
            height: rect.height + 12,
            borderRadius: 10,
            boxShadow: `0 0 0 9999px ${DIM}`,
            pointerEvents: "none",
            transition:
              "top 0.25s ease, left 0.25s ease, width 0.25s ease, height 0.25s ease",
          }}
        />
      )}
      {step.id === "welcome" ? (
        <CenteredCard
          title={t("onboarding.welcome.title")}
          body={t("onboarding.welcome.body")}
          primaryLabel={t("onboarding.welcome.start")}
          onPrimary={() => goTo(1, 1)}
          secondaryLabel={t("onboarding.skip")}
          onSecondary={() => finish("skipped")}
        />
      ) : step.id === "done" ? (
        <CenteredCard
          title={t("onboarding.done.title")}
          body={t("onboarding.done.body")}
          progress={`${LAST}/${LAST}`}
          primaryLabel={t("onboarding.done.close")}
          onPrimary={() => finish("completed")}
        />
      ) : (
        <TooltipCard
          rect={rect}
          title={t(`onboarding.step.${step.id}.title`)}
          body={t(`onboarding.step.${step.id}.body`)}
          progress={`${stepIndex}/${LAST}`}
          prevLabel={t("onboarding.prev")}
          nextLabel={t("onboarding.next")}
          skipLabel={t("onboarding.skip")}
          onPrev={() => goTo(stepIndex - 1, -1)}
          onNext={() => goTo(stepIndex + 1, 1)}
          onSkip={() => finish("skipped")}
        />
      )}
    </div>
  );
}

const cardShellStyle = {
  background: "var(--bg-card)",
  borderRadius: "var(--radius-lg)",
  border: "1px solid rgba(124, 92, 252, 0.15)",
  boxShadow: "var(--shadow-soft), var(--shadow-card)",
  padding: 16,
  display: "flex",
  flexDirection: "column" as const,
  gap: 10,
  animation: "tour-card-in 0.25s ease",
};

const primaryBtnStyle = {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 6,
  padding: "8px 14px",
  fontSize: 12,
  fontWeight: 700,
  border: "none",
  borderRadius: "var(--radius-md)",
  cursor: "pointer",
  background: "var(--accent-purple)",
  color: "#fff",
};

const ghostBtnStyle = {
  padding: "8px 10px",
  fontSize: 12,
  fontWeight: 700,
  border: "none",
  borderRadius: "var(--radius-md)",
  cursor: "pointer",
  background: "transparent",
  color: "var(--text-secondary)",
};

function CenteredCard({
  title,
  body,
  progress,
  primaryLabel,
  onPrimary,
  secondaryLabel,
  onSecondary,
}: {
  title: string;
  body: string;
  progress?: string;
  primaryLabel: string;
  onPrimary: () => void;
  secondaryLabel?: string;
  onSecondary?: () => void;
}) {
  return (
    <div
      role="dialog"
      aria-label={title}
      style={{ ...cardShellStyle, width: "min(320px, calc(100vw - 48px))" }}
    >
      <div style={{ fontSize: 14, fontWeight: 800, color: "var(--text-primary)" }}>
        {title}
      </div>
      <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5, color: "var(--text-secondary)" }}>
        {body}
      </p>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        {progress && (
          <span style={{ fontSize: 11, fontWeight: 700, color: "var(--text-secondary)" }}>
            {progress}
          </span>
        )}
        <div style={{ flex: 1 }} />
        {secondaryLabel && onSecondary && (
          <button onClick={onSecondary} style={ghostBtnStyle}>
            {secondaryLabel}
          </button>
        )}
        <button onClick={onPrimary} style={primaryBtnStyle}>
          {primaryLabel}
        </button>
      </div>
    </div>
  );
}

function TooltipCard({
  rect,
  title,
  body,
  progress,
  prevLabel,
  nextLabel,
  skipLabel,
  onPrev,
  onNext,
  onSkip,
}: {
  rect: DOMRect | null;
  title: string;
  body: string;
  progress: string;
  prevLabel: string;
  nextLabel: string;
  skipLabel: string;
  onPrev: () => void;
  onNext: () => void;
  onSkip: () => void;
}) {
  // Full-width card in a 440px popover: only the above/below decision matters.
  const placement: CSSProperties = rect
    ? rect.bottom + CARD_EST_HEIGHT + 12 < window.innerHeight
      ? { top: rect.bottom + 12 }
      : rect.top - CARD_EST_HEIGHT - 12 > 0
        ? { bottom: window.innerHeight - rect.top + 12 }
        : { bottom: 16 }
    : { bottom: 16 };

  return (
    <div
      role="dialog"
      aria-label={title}
      style={{ ...cardShellStyle, position: "fixed", left: 16, right: 16, ...placement }}
    >
      <div style={{ fontSize: 13, fontWeight: 800, color: "var(--text-primary)" }}>
        {title}
      </div>
      <p style={{ margin: 0, fontSize: 12, lineHeight: 1.5, color: "var(--text-secondary)" }}>
        {body}
      </p>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span style={{ fontSize: 11, fontWeight: 700, color: "var(--text-secondary)" }}>
          {progress}
        </span>
        <div style={{ flex: 1 }} />
        <button onClick={onSkip} style={ghostBtnStyle}>
          {skipLabel}
        </button>
        <button onClick={onPrev} style={ghostBtnStyle}>
          {prevLabel}
        </button>
        <button onClick={onNext} style={primaryBtnStyle}>
          {nextLabel}
        </button>
      </div>
    </div>
  );
}
