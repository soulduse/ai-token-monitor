# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [Türkçe](README.tr.md) | [Italiano](README.it.md)**

![AI Token Monitor — 在選單列即時追蹤 AI 程式開發工具的權杖與費用](images/hero.png)

**AI Token Monitor** 是一款輕量的 macOS / Windows 系統托盤應用,全天候回答一個問題:*我的 AI 程式開發工具到底花了多少錢?* 它讀取 **Claude Code**、**Codex**、**OpenCode**、**GJC**、**Grok** 與 **Kiro** 本來就會寫入的本機工作階段日誌,依各模型單價(含快取讀取)為每個權杖計費,並把今日支出直接顯示在時鐘旁邊——圖表、方案限額提醒、可選的排行榜、聊天與 Webhook 通知都只需一次點擊。

- **零設定** — 不需要 API 金鑰、不需要代理。只要執行過一次 Claude Code 或 Codex,即刻可用。
- **支出一目了然** — 選單列 / 系統托盤即時顯示費用,點擊即可開啟完整儀表板。
- **搶先於限額** — 即時顯示 5 小時工作階段與每週方案限額進度條,並在觸頂前透過 Discord / Slack / Telegram 提醒。
- **隱私優先** — 預設 100% 離線;Cursor 公開資料、排行榜、聊天與 Webhook 均為嚴格可選。

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

## 運作原理

![AI Token Monitor 運作原理 — 讀取本機工作階段日誌,在本機解析計費,在托盤與儀表板中顯示](images/how-it-works.png)

1. **讀取本機工作階段日誌** — 監看 AI CLI 本來就會寫入的 JSONL 檔案(詳細路徑見[資料來源](#資料來源))
2. **本機解析與計費** — Rust 引擎對項目去重,套用各模型單價(含快取讀取),檔案一變動立即重新彙總
3. **隨處可見** — 托盤費用顯示、儀表板圖表、方案限額提醒列,以及可選的 Webhook 通知

以上全部在你的電腦上完成。除非你主動啟用 Cursor 公開資料、排行榜、聊天或 Webhook,應用程式**不會發出任何網路請求**。Cursor 功能只讀取你設定的公開資料 URL；分享功能只傳送彙總數據,絕不會分享程式碼或對話內容。

## 主要功能

![功能亮點 — 追蹤與視覺化、競爭與分享、控制預算、個人化設定](images/features.png)

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
| **Grok** | `~/.grok/logs/unified.jsonl` | 來自 `shell.turn.inference_done` 的每次請求實測 token；模型與專案從 `~/.grok/sessions` 關聯。Grok 會截斷此滾動日誌的開頭，因此將每日彙總累積到本機快照。 支援 macOS、Linux 與 Windows（`%USERPROFILE%\\.grok`）。SuperGrok 每週額度從 `billing: fetched credits config` 讀取。 |
| **Kiro** | `~/.kiro/sessions/cli/*.json` + `data.sqlite3` | **計費單位是點數而非 token** — Kiro 以每輪「工作量」計量，且不在任何位置記錄 token 數，因此成本由點數換算（× $0.04，超額費率）。互動式與非互動式執行分別寫入鍵名不同的兩個儲存，兩者都會讀取。使用 Auto 的輪次不會記錄實際使用的模型。 |

**網路請求**:僅在啟用 Cursor 公開資料(讀取已設定的公開頁面)、排行榜/聊天(向 Supabase 傳送彙總資料)、觸發 Webhook 或設定 AI 翻譯供應商時發起。未使用這些功能時,應用完全離線運作。

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
