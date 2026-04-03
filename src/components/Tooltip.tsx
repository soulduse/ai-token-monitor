import { formatTokens, formatCost, formatDate } from "../lib/format";

interface Props {
  date: string;
  tokens: number;
  cost: number;
  x: number;
  y: number;
  visible: boolean;
}

export function Tooltip({ date, tokens, cost, x, y, visible }: Props) {
  if (!visible) return null;

  return (
    <div style={{
      position: "fixed",
      left: x,
      top: y - 50,
      transform: "translateX(-50%)",
      background: "var(--text-primary)",
      color: "var(--bg-primary)",
      padding: "6px 10px",
      borderRadius: "var(--radius-sm)",
      fontSize: 11,
      fontWeight: 600,
      whiteSpace: "nowrap",
      pointerEvents: "none",
      zIndex: 1100,
      boxShadow: "0 4px 12px rgba(0,0,0,0.15)",
    }}>
      <div>{formatDate(date)}</div>
      <div>{formatTokens(tokens)} tokens &middot; {formatCost(cost)}</div>
    </div>
  );
}
