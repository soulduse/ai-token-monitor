# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **[한국어](docs/README.ko.md) | [日本語](docs/README.ja.md) | [简体中文](docs/README.zh-CN.md) | [繁體中文](docs/README.zh-TW.md) | [Türkçe](docs/README.tr.md) | [Italiano](docs/README.it.md)**

A system tray app for macOS and Windows that tracks **Claude Code**, **Codex**, and **OpenCode** token usage, cost, and activity in real time — with a built-in leaderboard, chat, and webhook alerts.

<table>
  <tr>
    <th width="33%">Overview</th>
    <th width="33%">Analytics</th>
    <th width="33%">Leaderboard</th>
  </tr>
  <tr>
    <td><img src="docs/screenshots/overview.png" width="280" /></td>
    <td><img src="docs/screenshots/analytics.png" width="280" /></td>
    <td><img src="docs/screenshots/leaderboard.png" width="280" /></td>
  </tr>
  <tr>
    <td align="center">Today's usage, 7-day chart, weekly/monthly totals</td>
    <td align="center">Activity graph, 30-day trends, model breakdown</td>
    <td align="center">Compare usage with other developers</td>
  </tr>
</table>

## Download

**[Download Latest Release](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| Platform | File | Notes |
|----------|------|-------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac support coming soon |
| **Windows** | `.exe` installer | Windows 10+ (WebView2 required, auto-installed) |

## Features

### Tracking & Visualization
- **Real-time token tracking** — parses session JSONL files from Claude Code, Codex, and OpenCode for accurate usage stats
- **Multi-provider support** — switch between Claude / Codex / OpenCode sources, with per-provider cost models
- **Multiple config directories** — aggregate work + personal accounts by adding several Claude/Codex roots
- **Daily chart** — 7/30 day token or cost bar chart with Y-axis labels
- **Activity graph** — GitHub-style contribution heatmap with 2D/3D toggle and year navigation
- **Period navigation** — browse weekly/monthly totals with `< >` arrows
- **Model breakdown** — Input/Output/Cache ratio visualization
- **Cache efficiency** — donut chart showing cache hit ratio
- **Usage alerts bar** — live 5-hour session + weekly plan limits (optional Claude OAuth sign-in)

### Social & Sharing
- **Leaderboard** — compare daily/weekly/monthly usage with other developers (GitHub OAuth, opt-in)
- **7-day TOP 10 grid** — at-a-glance ranking history
- **Mini profile** — activity heatmap, streaks, external profile links
- **Badges** — card / compact / flat-square styles, export as PNG / SVG / Markdown, or embed a live badge URL in your GitHub README
- **Chat** — in-app chat for leaderboard members with mentions, replies, image attachments, unread counter, typing indicators, and AI translation
- **AI Report (Wrapped)** — monthly/yearly recap card with top model, busiest day, and streaks
- **Receipt view** — receipt-style usage summary for today / week / month / all-time
- **Salary comparator** — see your monthly AI spend as a share of your salary (lattes / Netflix / chicken)
- **Share & export** — copy summary markdown, capture screenshot, or copy an app share message from the header menu

### Notifications & Alerts
- **Tray cost** — today's cost shown next to the tray icon (macOS menu bar title, Windows tooltip)
- **Webhook notifications** — Discord, Slack, and Telegram alerts when usage crosses thresholds or resets
- **Auto-updater** — in-app update notifications with download progress

### Customization
- **4 themes** — GitHub (green), Purple, Ocean, Sunset — with Auto/Light/Dark mode
- **10 languages** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **Compact / full number format** — `377.0K` vs `377,000`
- **Launch on startup** — optional auto-start on boot
- **AI translation** — bring your own Gemini / OpenAI / Anthropic API key to translate chat messages (keys are encrypted locally)
- **Auto-hide window** — hides when clicking outside

## Install from Source

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) toolchain
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code), [Codex](https://openai.com/index/introducing-codex/), or [OpenCode](https://opencode.ai) installed and used at least once

### Build

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # development mode
npm run tauri build   # production build
```

## Usage

### Basics

1. Launch the app — an icon appears in the system tray (macOS menu bar / Windows taskbar)
2. Click the icon to open the dashboard
3. Switch between **Overview**, **Analytics**, **Leaderboard**, and **Chat** tabs

### Tabs

| Tab | Content |
|-----|---------|
| **Overview** | Today's summary, 7-day chart, weekly/monthly totals, 8-week heatmap, usage alert bar |
| **Analytics** | Full-year activity graph (2D/3D), 30-day chart, model breakdown, cache efficiency |
| **Leaderboard** | Rank against other users, 7-day TOP 10 grid, badges, mini profiles |
| **Chat** | Realtime chat with leaderboard members — mentions, replies, images, AI translation |

### Header actions

The header exposes a **share** button, a **⋯ more** dropdown, and a **⚙ settings** button. The more menu contains:

- **GitHub repository** — open the repo in your browser
- **AI Report** — see your monthly/yearly recap
- **Receipt** — receipt-style usage summary
- **Share app** — copy a recommendation message + repo link
- **Capture screenshot** — copy the current view to the clipboard

### Settings

Settings is organized into four tabs:

| Tab | Options |
|-----|---------|
| **General** | Theme, language, appearance, number format, menu bar cost, launch on startup, monthly salary, usage alerts, Claude/Codex directories, Claude usage tracking (OAuth) |
| **Account** | GitHub sign-in, leaderboard opt-in, profile links |
| **AI** | Gemini / OpenAI / Anthropic API keys for chat translation (encrypted locally) |
| **Webhooks** | Discord / Slack / Telegram webhook URLs, alert thresholds, monitored windows, reset notifications |

### Leaderboard & chat

1. Enable **Share Usage Data** in Settings → Account
2. Click **Sign in with GitHub**
3. See your rank in the **Leaderboard** tab and join the **Chat** tab

Shared data: daily token count, cost, messages/sessions. **No code or conversation content is shared.**

## Data Sources

| Provider | Path | Notes |
|----------|------|-------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | Session/tool-call counts from `~/.claude/stats-cache.json`. Supports multiple roots. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | Supports multiple roots. |
| **OpenCode** | `~/.local/share/opencode/**/*.jsonl` | Per-model pricing from bundled registry. |

**Network requests**: only when leaderboard/chat is opted in (sends aggregated data to Supabase) or when a webhook fires. Without these features, the app runs completely offline. AI translation keys, if set, call the provider you chose directly.

## Architecture

```
┌────────────────────────────────────┐
│  Frontend (React 19 + Vite)        │
│  ├── PopoverShell / Header         │
│  ├── TabBar (4 tabs)               │
│  ├── TodaySummary / DailyChart     │
│  ├── ActivityGraph (2D/3D) / Heatmap│
│  ├── ModelBreakdown / CacheEfficiency│
│  ├── Leaderboard + Grid + Badges   │
│  ├── Chat + MentionAutocomplete    │
│  ├── MiniProfile / Wrapped / Receipt│
│  ├── SalaryComparator / UsageAlertBar│
│  └── SettingsOverlay (4 tabs)      │
├────────────────────────────────────┤
│  Backend (Tauri v2 / Rust)         │
│  ├── JSONL session parsers (Claude/Codex/OpenCode)│
│  ├── File watcher (notify)         │
│  ├── Tray icon + cost display      │
│  ├── Auto-updater                  │
│  ├── Webhook dispatcher            │
│  └── Preferences + encrypted secrets│
├────────────────────────────────────┤
│  External services (opt-in)        │
│  ├── Supabase (leaderboard + chat) │
│  ├── Discord / Slack / Telegram    │
│  └── Gemini / OpenAI / Anthropic   │
└────────────────────────────────────┘
```

## Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| **macOS** | Supported | Menu bar integration, dock hiding, tray cost title |
| **Windows** | Supported | System tray integration, NSIS installer, tooltip cost display |
| **Linux** | Untested | May work since Tauri supports Linux |

## Support

If you find this project useful, consider [buying me a coffee](https://ctee.kr/place/programmingzombie).

## License

MIT
