interface Props {
  color: string;
  isToday?: boolean;
  onMouseEnter: (e: React.MouseEvent) => void;
  onMouseLeave: () => void;
}

export function HeatmapCell({ color, isToday, onMouseEnter, onMouseLeave }: Props) {
  return (
    <div
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      style={{
        width: 14,
        height: 14,
        borderRadius: 3,
        background: color,
        cursor: "pointer",
        transition: "transform 0.1s ease, box-shadow 0.1s ease",
        ...(isToday ? {
          outline: "2px solid var(--accent-purple)",
          outlineOffset: -1,
        } : {}),
      }}
      onMouseOver={(e) => {
        (e.currentTarget as HTMLElement).style.transform = "scale(1.3)";
        (e.currentTarget as HTMLElement).style.boxShadow = `0 0 6px ${color}`;
      }}
      onMouseOut={(e) => {
        (e.currentTarget as HTMLElement).style.transform = "scale(1)";
        (e.currentTarget as HTMLElement).style.boxShadow = "none";
      }}
    />
  );
}
