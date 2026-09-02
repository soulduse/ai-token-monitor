import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { copyElementAsImage, saveElementAsPng } from "../lib/captureElement";
import { useI18n } from "../i18n/I18nContext";
import type { AnalyticsSubTab } from "./AnalyticsSubTabs";

export interface AnalyticsCaptureSection {
  id: string;
  label: string;
}

interface Props {
  captureRootRef: RefObject<HTMLDivElement | null>;
  sections: AnalyticsCaptureSection[];
  subTab: AnalyticsSubTab;
}

type CaptureAction = "copy" | "save";
type Feedback = "copied" | "saved" | "failed" | null;

const FULL_PAGE_ID = "full-page";

function safeFilePart(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "") || "capture";
}

export function AnalyticsCaptureMenu({ captureRootRef, sections, subTab }: Props) {
  const t = useI18n();
  const [open, setOpen] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const busyRef = useRef(false);
  const feedbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (feedbackTimerRef.current) clearTimeout(feedbackTimerRef.current);
    };
  }, []);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setOpen(false);
      }
    };

    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  const showFeedback = useCallback((next: Exclude<Feedback, null>) => {
    setFeedback(next);
    if (feedbackTimerRef.current) clearTimeout(feedbackTimerRef.current);
    feedbackTimerRef.current = setTimeout(() => setFeedback(null), 2000);
  }, []);

  const resolveTarget = useCallback((id: string): HTMLElement | null => {
    const root = captureRootRef.current;
    if (!root) return null;
    if (id === FULL_PAGE_ID) return root;
    return root.querySelector<HTMLElement>(`[data-analytics-capture-section="${id}"]`);
  }, [captureRootRef]);

  const capture = useCallback(async (id: string, action: CaptureAction) => {
    if (busyRef.current) return;
    const target = resolveTarget(id);
    if (!target) {
      showFeedback("failed");
      return;
    }

    const key = `${action}:${id}`;
    busyRef.current = true;
    setBusyKey(key);
    try {
      if (action === "copy") {
        await copyElementAsImage(target);
        showFeedback("copied");
      } else {
        const targetName = id === FULL_PAGE_ID ? subTab : id;
        const saved = await saveElementAsPng(
          target,
          `ai-token-monitor-analytics-${safeFilePart(targetName)}.png`,
        );
        if (saved) showFeedback("saved");
      }
    } catch (error) {
      console.error("Analytics capture failed:", error);
      showFeedback("failed");
    } finally {
      busyRef.current = false;
      setBusyKey(null);
    }
  }, [resolveTarget, showFeedback, subTab]);

  const items = [
    { id: FULL_PAGE_ID, label: t("analytics.capture.fullPage") },
    ...sections,
  ];

  const feedbackLabel = feedback === "copied"
    ? t("badge.copied")
    : feedback === "saved"
      ? t("badge.saved")
      : feedback === "failed"
        ? t("header.captureFailed")
        : null;

  return (
    <div ref={menuRef} style={{ position: "relative", flexShrink: 0 }}>
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        title={t("analytics.capture.title")}
        aria-label={t("analytics.capture.title")}
        aria-haspopup="menu"
        aria-expanded={open}
        style={{
          width: 38,
          height: "100%",
          minHeight: 36,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          border: "none",
          borderRadius: "var(--radius-lg)",
          background: "var(--bg-card)",
          color: open ? "var(--accent-purple)" : "var(--text-secondary)",
          boxShadow: "var(--shadow-card)",
          cursor: "pointer",
          transition: "color 0.15s ease, background 0.15s ease",
        }}
      >
        <CameraIcon />
      </button>

      {open && (
        <div
          role="menu"
          aria-label={t("analytics.capture.title")}
          style={{
            position: "absolute",
            top: "calc(100% + 8px)",
            right: 0,
            zIndex: 60,
            width: 290,
            maxHeight: "min(430px, calc(100vh - 190px))",
            overflowY: "auto",
            padding: 6,
            border: "1px solid rgba(128,128,128,0.15)",
            borderRadius: 12,
            background: "var(--bg-card)",
            boxShadow: "0 12px 32px rgba(0,0,0,0.18), 0 2px 8px rgba(0,0,0,0.08)",
            animation: "headerMenuPop 0.16s cubic-bezier(.2,.9,.2,1) both",
          }}
        >
          <div style={{
            padding: "5px 8px 7px",
            fontSize: 11,
            fontWeight: 800,
            color: feedback === "failed" ? "var(--red, #ef4444)" : "var(--text-secondary)",
            textTransform: feedback ? "none" : "uppercase",
            letterSpacing: feedback ? 0 : "0.4px",
          }} aria-live="polite">
            {feedbackLabel ?? t("analytics.capture.title")}
          </div>

          {items.map((item, index) => (
            <div
              key={item.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                minHeight: 38,
                padding: "4px 5px 4px 10px",
                borderTop: index === 1 ? "1px solid rgba(128,128,128,0.12)" : "none",
                borderRadius: 8,
                color: "var(--text-primary)",
              }}
            >
              <span style={{
                flex: 1,
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                fontSize: 12,
                fontWeight: item.id === FULL_PAGE_ID ? 800 : 600,
              }}>
                {item.label}
              </span>
              <ActionButton
                label={t("badge.copyImage")}
                busy={busyKey === `copy:${item.id}`}
                disabled={busyKey !== null}
                onClick={() => capture(item.id, "copy")}
              >
                <CopyIcon />
              </ActionButton>
              <ActionButton
                label={t("badge.savePng")}
                busy={busyKey === `save:${item.id}`}
                disabled={busyKey !== null}
                onClick={() => capture(item.id, "save")}
              >
                <DownloadIcon />
              </ActionButton>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ActionButton({
  label,
  busy,
  disabled,
  onClick,
  children,
}: {
  label: string;
  busy: boolean;
  disabled: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      style={{
        width: 29,
        height: 29,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        flexShrink: 0,
        border: "none",
        borderRadius: 7,
        background: "rgba(128,128,128,0.1)",
        color: "var(--text-secondary)",
        cursor: disabled ? "default" : "pointer",
        opacity: disabled && !busy ? 0.4 : 1,
      }}
    >
      {busy ? <SpinnerIcon /> : children}
    </button>
  );
}

function CameraIcon() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M14.5 4l1.8 2H20a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h3.7l1.8-2z" />
      <circle cx="12" cy="12.5" r="3.5" />
    </svg>
  );
}

function CopyIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="9" y="9" width="11" height="11" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3v12" />
      <path d="M7 10l5 5 5-5" />
      <path d="M5 21h14" />
    </svg>
  );
}

function SpinnerIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" aria-hidden="true" style={{ animation: "miniProfileSpin 0.8s linear infinite" }}>
      <path d="M21 12a9 9 0 1 1-6.2-8.6" />
    </svg>
  );
}
