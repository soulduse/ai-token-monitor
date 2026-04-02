export type Segment =
  | { type: "text"; value: string }
  | { type: "url"; value: string }
  | { type: "mention"; value: string }; // value = nickname without @

const URL_RE = /(https?:\/\/[^\s<>"')\]]+|www\.[^\s<>"')\]]+)/gi;
const MENTION_RE = /@([a-zA-Z0-9_-]+)/g;

/** Strip trailing sentence punctuation from a URL match */
function stripTrailingPunctuation(url: string): { url: string; trailing: string } {
  const match = url.match(/[.,!?:;]+$/);
  if (!match) return { url, trailing: "" };
  return { url: url.slice(0, -match[0].length), trailing: match[0] };
}

/** Split text by URL matches into text/url segments */
function splitUrls(text: string): Segment[] {
  const segments: Segment[] = [];
  let lastIndex = 0;

  for (const match of text.matchAll(URL_RE)) {
    const idx = match.index!;
    if (idx > lastIndex) {
      segments.push({ type: "text", value: text.slice(lastIndex, idx) });
    }
    const { url, trailing } = stripTrailingPunctuation(match[0]);
    segments.push({ type: "url", value: url });
    if (trailing) {
      segments.push({ type: "text", value: trailing });
    }
    lastIndex = idx + match[0].length;
  }

  if (lastIndex < text.length) {
    segments.push({ type: "text", value: text.slice(lastIndex) });
  }

  return segments.length > 0 ? segments : [{ type: "text", value: text }];
}

/** Further split text segments by @mention matches */
function splitMentions(segments: Segment[], knownNicknames: Set<string>): Segment[] {
  if (knownNicknames.size === 0) return segments;

  const result: Segment[] = [];

  for (const seg of segments) {
    if (seg.type !== "text") {
      result.push(seg);
      continue;
    }

    let lastIndex = 0;
    const text = seg.value;

    for (const match of text.matchAll(MENTION_RE)) {
      const nickname = match[1];
      if (!knownNicknames.has(nickname.toLowerCase())) continue;

      const idx = match.index!;
      if (idx > lastIndex) {
        result.push({ type: "text", value: text.slice(lastIndex, idx) });
      }
      result.push({ type: "mention", value: nickname });
      lastIndex = idx + match[0].length;
    }

    if (lastIndex === 0) {
      // No accepted mentions — push original segment
      result.push(seg);
    } else if (lastIndex < text.length) {
      // Remaining text after last mention
      result.push({ type: "text", value: text.slice(lastIndex) });
    }
  }

  return result;
}

/**
 * Parse message content into segments of text, URLs, and @mentions.
 * URLs are parsed first to avoid detecting @ inside URLs as mentions.
 */
export function parseMessageContent(
  content: string,
  knownNicknames?: Set<string>,
): Segment[] {
  const urlSegments = splitUrls(content);
  if (knownNicknames && knownNicknames.size > 0) {
    return splitMentions(urlSegments, knownNicknames);
  }
  return urlSegments;
}
