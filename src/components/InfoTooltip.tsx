import { useState, useRef, useLayoutEffect, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  wide?: boolean;
}

export function InfoTooltip({ children, wide }: Props) {
  const [show, setShow] = useState(false);
  const triggerRef = useRef<HTMLSpanElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const [style, setStyle] = useState<React.CSSProperties>({});

  useLayoutEffect(() => {
    if (!show || !triggerRef.current || !tooltipRef.current) return;

    const trigger = triggerRef.current.getBoundingClientRect();
    const tooltip = tooltipRef.current;
    const tooltipHeight = tooltip.scrollHeight;
    const padding = 8;
    const tooltipWidth = wide ? Math.min(360, window.innerWidth - padding * 2) : 220;

    // Vertical: prefer above, fall back to below
    let top: number;
    if (trigger.top - tooltipHeight - 6 > 0) {
      top = trigger.top - tooltipHeight - 6;
    } else {
      top = trigger.bottom + 6;
    }

    // Horizontal: center on window for wide, left-align for normal
    let left: number;
    if (wide) {
      left = padding;
    } else {
      left = Math.max(padding, Math.min(trigger.left, window.innerWidth - tooltipWidth - padding));
    }

    setStyle({
      position: "fixed",
      top,
      left,
      width: tooltipWidth,
    });
  }, [show, wide]);

  return (
    <span style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
      <span
        ref={triggerRef}
        onMouseEnter={() => setShow(true)}
        onMouseLeave={() => setShow(false)}
        style={{
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          width: 14,
          height: 14,
          borderRadius: 7,
          fontSize: 9,
          fontWeight: 700,
          background: "var(--heat-0)",
          color: "var(--text-secondary)",
          cursor: "help",
          marginLeft: 4,
          flexShrink: 0,
        }}
      >
        ?
      </span>
      {show && (
        <div
          ref={tooltipRef}
          style={{
            ...style,
            background: "var(--text-primary)",
            color: "var(--bg-primary)",
            padding: "8px 10px",
            borderRadius: 8,
            fontSize: 10,
            lineHeight: 1.5,
            whiteSpace: "normal",
            maxHeight: "70vh",
            overflowY: "auto",
            zIndex: 100,
            boxShadow: "0 4px 12px rgba(0,0,0,0.2)",
            pointerEvents: "auto",
          }}
        >
          {children}
        </div>
      )}
    </span>
  );
}
