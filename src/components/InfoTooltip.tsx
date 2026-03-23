import { useState, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

export function InfoTooltip({ children }: Props) {
  const [show, setShow] = useState(false);

  return (
    <span style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
      <span
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
        <div style={{
          position: "absolute",
          bottom: "calc(100% + 6px)",
          left: 0,
          background: "var(--text-primary)",
          color: "var(--bg-primary)",
          padding: "8px 10px",
          borderRadius: 8,
          fontSize: 10,
          lineHeight: 1.5,
          whiteSpace: "normal",
          width: 220,
          zIndex: 100,
          boxShadow: "0 4px 12px rgba(0,0,0,0.2)",
          pointerEvents: "none",
        }}>
          {children}
        </div>
      )}
    </span>
  );
}
