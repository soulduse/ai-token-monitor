# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [Türkçe](README.tr.md) | [Italiano](README.it.md)**

一款 macOS 和 Windows 系統托盤應用,可即時追蹤 **Claude Code**、**Codex** 和 **OpenCode** 的權杖使用量、費用與活動,並內建排行榜、聊天與 Webhook 提醒。

| 總覽 | 分析 | 排行榜 |
|:---:|:---:|:---:|
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> | <img src="screenshots/leaderboard.png" width="280" /> |
| 今日使用量、7 天圖表、週/月彙總 | 活動圖、30 天趨勢、模型分析 | 與其他開發者比較使用量 |

## 下載

**[下載最新版本](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| 平台 | 檔案 | 備註 |
|------|------|------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac 即將支援 |
| **Windows** | `.exe` 安裝程式 | Windows 10+(需要 WebView2,自動安裝) |

## 主要功能

### 追蹤與視覺化
- **即時權杖追蹤** — 直接解析 Claude Code / Codex / OpenCode 的工作階段 JSONL 檔案,準確統計使用量
- **多供應商支援** — 可在 Claude / Codex / OpenCode 之間切換,各供應商採用獨立價格模型
- **多設定目錄** — 可同時新增多個 Claude/Codex 根目錄,彙總工作與個人帳號使用量
- **每日圖表** — 7/30 天權杖或費用柱狀圖(含 Y 軸標籤)
- **活動圖** — GitHub 風格貢獻熱力圖(支援 2D/3D 切換與按年瀏覽)
- **期間導覽** — 使用 `< >` 箭頭瀏覽過去的週/月彙總
- **模型分析** — Input/Output/Cache 比例視覺化
- **快取效率** — 快取命中率環形圖
- **用量提醒列** — 即時顯示 Claude Code 5 小時工作階段與每週用量上限(可選 Claude OAuth 登入)

### 社交與分享
- **排行榜** — 與其他開發者比較日/週/月使用量(GitHub OAuth,需主動開啟)
- **7 天 TOP 10 網格** — 直觀呈現排名歷史
- **迷你個人資料** — 活動熱力圖、連續活躍天數、外部資料連結
- **徽章** — Card / Compact / Flat Square 樣式,可匯出為 PNG / SVG / Markdown 或動態 URL,嵌入 GitHub README
- **聊天** — 面向排行榜成員的應用內聊天,支援提及、回覆、圖片附件、未讀徽章、輸入中提示以及 AI 翻譯
- **AI 報告 (Wrapped)** — 月度/年度回顧卡片(最常用模型、最忙碌的一天、連續紀錄)
- **收據檢視** — 今日 / 本週 / 本月 / 全部 的收據式使用摘要
- **薪資比較** — 將 AI 花費換算為月薪佔比(拿鐵 / Netflix / 炸雞)
- **分享與匯出** — 透過頂部選單複製 Markdown 摘要、擷取螢幕截圖或應用分享訊息

### 提醒
- **托盤費用** — 在托盤圖示旁顯示今日費用(macOS 選單列標題,Windows 工具提示)
- **Webhook 通知** — 用量達到閾值或重置時透過 Discord / Slack / Telegram 通知
- **自動更新器** — 應用內更新提示,含下載進度

### 自訂
- **4 種主題** — GitHub(綠色)、Purple、Ocean、Sunset,並支援自動/淺色/深色模式
- **10 種語言** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **數字格式** — 精簡(`377.0K`)/ 完整(`377,000`)切換
- **開機自動啟動** — 可選開機時自動啟動
- **AI 翻譯** — 新增 Gemini / OpenAI / Anthropic API 金鑰後可翻譯聊天訊息(金鑰於本機加密儲存)
- **自動隱藏** — 點擊視窗外自動隱藏

## 從原始碼安裝

### 先決條件

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 工具鏈
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- 已安裝 [Claude Code](https://claude.ai/claude-code)、[Codex](https://openai.com/index/introducing-codex/) 或 [OpenCode](https://opencode.ai) 其中至少一個,且至少使用過一次

### 建置

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 開發模式
npm run tauri build   # 生產建置
```

## 使用方法

### 基本操作

1. 啟動應用程式後,系統托盤(macOS 選單列 / Windows 工作列)會出現圖示
2. 點擊圖示開啟使用量儀表板
3. 在 **總覽**、**分析**、**排行榜** 和 **聊天** 分頁之間切換

### 分頁說明

| 分頁 | 內容 |
|------|------|
| **總覽** | 今日摘要、7 天圖表、週/月彙總、8 週熱力圖、用量提醒列 |
| **分析** | 全年活動圖(2D/3D)、30 天圖表、模型分析、快取效率 |
| **排行榜** | 使用量排名、7 天 TOP 10 網格、徽章、迷你個人資料 |
| **聊天** | 與排行榜成員即時聊天 — 提及、回覆、圖片、AI 翻譯 |

### 頂部操作

頂部包含 **分享按鈕**、**⋯ 選單** 與 **⚙ 設定** 按鈕。選單包含以下項目:

- **查看 GitHub 儲存庫** — 在瀏覽器中開啟儲存庫
- **我的 AI 報告** — 月度/年度回顧卡片
- **收據** — 收據式使用摘要
- **分享此應用** — 複製推薦訊息 + 儲存庫連結到剪貼簿
- **擷取螢幕截圖** — 將目前畫面複製到剪貼簿

### 設定

設定分為 4 個分頁:

| 分頁 | 選項 |
|------|------|
| **一般** | 主題、語言、外觀、數字格式、選單列費用、開機自動啟動、月薪、用量提醒、Claude/Codex 目錄、Claude 用量追蹤(OAuth) |
| **帳戶** | GitHub 登入、排行榜公開、個人資料連結 |
| **AI** | Gemini / OpenAI / Anthropic API 金鑰(聊天翻譯,本機加密儲存) |
| **Webhooks** | Discord / Slack / Telegram Webhook URL、提醒閾值、監控視窗、重置通知 |

### 排行榜與聊天

1. 在 設定 → 帳戶 啟用 "分享使用資料"
2. 點擊 "使用 GitHub 登入"
3. 在排行榜分頁查看排名,在聊天分頁參與對話

分享的資料:每日權杖總量、費用、訊息/工作階段數。**不會分享程式碼或對話內容。**

## 資料來源

| 供應商 | 路徑 | 備註 |
|--------|------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | 從 `~/.claude/stats-cache.json` 補充工作階段/工具呼叫數。支援多個根目錄。 |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | 支援多個根目錄。 |
| **OpenCode** | `~/.local/share/opencode/**/*.jsonl` | 內建價格資料按模型計算費用。 |

**網路請求**:僅在啟用排行榜/聊天時(向 Supabase 傳送彙總資料)或 Webhook 觸發時才會發起網路請求。未使用這些功能時,應用完全離線運作。設定 AI 翻譯金鑰後,才會直接向對應供應商傳送請求。

## 架構

```
┌────────────────────────────────────┐
│  前端 (React 19 + Vite)            │
│  ├── PopoverShell / Header         │
│  ├── TabBar (4 分頁)               │
│  ├── TodaySummary / DailyChart     │
│  ├── ActivityGraph (2D/3D) / Heatmap│
│  ├── ModelBreakdown / CacheEfficiency│
│  ├── Leaderboard + Grid + Badges   │
│  ├── Chat + MentionAutocomplete    │
│  ├── MiniProfile / Wrapped / Receipt│
│  ├── SalaryComparator / UsageAlertBar│
│  └── SettingsOverlay (4 分頁)      │
├────────────────────────────────────┤
│  後端 (Tauri v2 / Rust)            │
│  ├── JSONL 工作階段解析器 (Claude/Codex/OpenCode)│
│  ├── 檔案監視 (notify)             │
│  ├── 托盤圖示 + 費用顯示           │
│  ├── 自動更新器                    │
│  ├── Webhook 分派器                │
│  └── 偏好設定 + 加密機密           │
├────────────────────────────────────┤
│  外部服務 (可選)                   │
│  ├── Supabase (排行榜 + 聊天)      │
│  ├── Discord / Slack / Telegram    │
│  └── Gemini / OpenAI / Anthropic   │
└────────────────────────────────────┘
```

## 平台支援

| 平台 | 狀態 | 備註 |
|------|------|------|
| **macOS** | 支援 | 選單列整合、隱藏 Dock、托盤費用標題 |
| **Windows** | 支援 | 系統托盤整合、NSIS 安裝程式、工具提示費用顯示 |
| **Linux** | 未測試 | Tauri 支援 Linux,基本功能可能可用 |

## 支援

如果您覺得此專案有用,歡迎 [請我喝杯咖啡](https://ctee.kr/place/programmingzombie)。

## 授權

MIT
