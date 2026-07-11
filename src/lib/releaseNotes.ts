/** Parses the release-notes markdown that the Release workflow writes into
 *  latest.json (surfaced as `update.body` by the updater plugin) into a short
 *  human-readable list for the what's-new popover.
 *
 *  Input shape (see .github release workflow):
 *    ## AI Token Monitor v0.19.37
 *    ### Bug Fixes
 *    - fix: 죽은 웹뷰 자동 복구 (#168) (7d9a882)
 *    ### Download
 *    - macOS ...   ← installer instructions, not a change — skipped
 */

export type ReleaseNoteCategory = "feature" | "fix" | "perf" | "other";

export interface ReleaseNoteItem {
  category: ReleaseNoteCategory;
  text: string;
}

const CATEGORY_EMOJI: Record<ReleaseNoteCategory, string> = {
  feature: "✨",
  fix: "🐛",
  perf: "⚡",
  other: "📦",
};

export function categoryEmoji(category: ReleaseNoteCategory): string {
  return CATEGORY_EMOJI[category];
}

function sectionCategory(title: string): ReleaseNoteCategory | null {
  if (/download/i.test(title)) return null; // installer instructions, not changes
  if (/feature/i.test(title)) return "feature";
  if (/fix/i.test(title)) return "fix";
  if (/perf/i.test(title)) return "perf";
  return "other";
}

/** Strips conventional-commit noise so users read plain descriptions:
 *  "fix(chat): show reactions (#168) (7d9a882)" → "show reactions". */
function cleanItemText(raw: string): string {
  return raw
    .replace(/^(feat|fix|perf|chore|refactor|docs|style|test|build|ci)(\([^)]*\))?!?:\s*/i, "")
    .replace(/\s*\(#\d+\)/g, "")
    .replace(/\s*\([0-9a-f]{7,10}\)\s*$/i, "")
    .trim();
}

export function parseReleaseNotes(body: string): ReleaseNoteItem[] {
  const items: ReleaseNoteItem[] = [];
  let category: ReleaseNoteCategory | null = "other";

  for (const line of body.split("\n")) {
    const trimmed = line.trim();
    const section = trimmed.match(/^#{2,4}\s+(.+)$/);
    if (section) {
      // The "## AI Token Monitor vX.Y.Z" heading is a title, not a section.
      category = /^ai token monitor/i.test(section[1])
        ? "other"
        : sectionCategory(section[1]);
      continue;
    }
    const bullet = trimmed.match(/^[-*]\s+(.+)$/);
    if (bullet && category !== null) {
      const text = cleanItemText(bullet[1]);
      if (text) items.push({ category, text });
    }
  }
  return items;
}
