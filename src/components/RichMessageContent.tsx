import { openUrl } from "@tauri-apps/plugin-opener";
import { parseMessageContent } from "../lib/parseMessageContent";

interface Props {
  content: string;
  isMe: boolean;
  knownNicknames?: Set<string>;
}

export function RichMessageContent({ content, isMe, knownNicknames }: Props) {
  const segments = parseMessageContent(content, knownNicknames);

  return (
    <>
      {segments.map((seg, i) => {
        if (seg.type === "url") {
          const href = seg.value.startsWith("www.") ? `https://${seg.value}` : seg.value;
          return (
            <span
              key={i}
              onClick={(e) => { e.stopPropagation(); openUrl(href); }}
              style={{
                textDecoration: "underline",
                cursor: "pointer",
                color: isMe ? "rgba(255,255,255,0.9)" : "var(--accent-purple)",
                wordBreak: "break-all",
              }}
            >
              {seg.value}
            </span>
          );
        }

        if (seg.type === "mention") {
          return (
            <span
              key={i}
              style={{
                fontWeight: 700,
                color: isMe ? "#fff" : "var(--accent-purple)",
                background: isMe ? "rgba(255,255,255,0.18)" : "rgba(124, 92, 252, 0.12)",
                borderRadius: 4,
                padding: "1px 4px",
                margin: "0 1px",
              }}
            >
              @{seg.value}
            </span>
          );
        }

        return <span key={i}>{seg.value}</span>;
      })}
    </>
  );
}
