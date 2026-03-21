# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)**

一款 macOS 選單列應用程式，即時追蹤 Claude Code 的 Token 使用量和費用。

<!-- TODO: 新增螢幕截圖 -->

## 下載

**[下載最新版本 (.dmg)](https://github.com/soulduse/ai-token-monitor/releases/latest)**

> 僅支援 macOS (Apple Silicon)。Intel Mac 支援即將推出。

## 主要功能

- **即時 Token 追蹤** — 直接解析 Claude Code 工作階段 JSONL 檔案，準確顯示使用量
- **費用計算** — 基於模型定價（Opus、Sonnet、Haiku）自動計算費用
- **每日圖表** — 7/30 天 Token 或費用長條圖，附 Y 軸標籤
- **活動圖表** — GitHub 風格貢獻熱力圖，支援 2D/3D 切換及年份導覽
- **週期導覽** — 透過 `< >` 箭頭瀏覽每週/每月彙總
- **模型分析** — Input/Output/Cache 比例視覺化
- **快取效率** — 快取命中率環形圖
- **選單列費用** — 在系統匣圖示旁顯示今日費用 ($45)
- **4 種主題** — GitHub（綠色）、Purple、Ocean、Sunset — 支援深色模式
- **螢幕截圖** — 將應用程式視窗截圖複製到剪貼簿
- **剪貼簿匯出** — 將使用量摘要以 Markdown 格式複製
- **排行榜** — 與其他使用者比較使用量（GitHub OAuth，可選加入）
- **自動隱藏** — 點選視窗外部自動隱藏

## 從原始碼安裝

### 前置需求

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 工具鏈
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- 已安裝 [Claude Code](https://claude.ai/claude-code) 且至少使用過一次

### 建置

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 開發模式
npm run tauri build   # 正式建置
```

## 使用方法

### 基本使用

1. 啟動應用程式後，macOS 選單列會出現圖示
2. 點選圖示開啟使用量儀表板
3. 在 **Overview**、**Analytics** 和 **Leaderboard** 分頁之間切換

### 分頁說明

| 分頁 | 內容 |
|------|------|
| **Overview** | 今日摘要、7 天圖表、每週/每月彙總、8 週熱力圖 |
| **Analytics** | 全年活動圖表（2D/3D）、30 天圖表、模型分析、快取效率 |
| **Leaderboard** | 與其他使用者比較使用量（可選加入） |

### 設定

點選右上角齒輪圖示進行設定：
- **Theme**：GitHub / Purple / Ocean / Sunset 主題切換
- **Number Format**：精簡格式 (377.0K) 與完整格式 (377,000) 切換
- **Menu Bar Cost**：在選單列顯示/隱藏今日費用
- **Leaderboard**：選擇是否分享使用資料 + GitHub 登入

### 排行榜

1. 在設定中啟用 "Share Usage Data"
2. 點選 "Sign in with GitHub"
3. 在 Leaderboard 分頁檢視 Today / This Week 排名

分享資料：每日 Token 總數、費用、訊息/工作階段數（不分享程式碼或對話內容）

## 資料來源

應用程式直接讀取 `~/.claude/projects/**/*.jsonl` 檔案來彙總 Token 使用量。
補充的工作階段/工具呼叫次數來自 `~/.claude/stats-cache.json`。

**網路請求**：僅在啟用排行榜時傳送（向 Supabase 傳送彙總資料）。
不使用排行榜時，應用程式完全離線運作。

## 架構

```
┌──────────────────────────────┐
│  Frontend (React 19 + Vite)  │
│  ├── PopoverShell            │
│  ├── TabBar (3 分頁)          │
│  ├── TodaySummary            │
│  ├── DailyChart (SVG)        │
│  ├── ActivityGraph (2D/3D)   │
│  ├── Heatmap                 │
│  ├── ModelBreakdown          │
│  ├── CacheEfficiency         │
│  └── Leaderboard             │
├──────────────────────────────┤
│  Backend (Tauri v2 / Rust)   │
│  ├── JSONL 工作階段解析器     │
│  ├── 檔案監視 (notify)        │
│  ├── 系統匣圖示 + 費用顯示    │
│  └── 偏好設定 (JSON)          │
├──────────────────────────────┤
│  資料來源                     │
│  └── ~/.claude/projects/     │
│      └── **/*.jsonl           │
└──────────────────────────────┘
```

## 平台支援

| 平台 | 狀態 | 備註 |
|------|------|------|
| **macOS** | 已支援 | 選單列整合、Dock 隱藏、系統匣費用顯示 |
| **Windows** | 規劃中 | 核心邏輯跨平台。macOS 專用程式碼透過 `#[cfg(target_os)]` 隔離 |
| **Linux** | 未測試 | Tauri 支援 Linux，基本功能可能可用 |

## 贊助

如果您覺得這個專案有用，歡迎[請我喝杯咖啡](https://ctee.kr/place/programmingzombie)來支持開發。

## 授權條款

MIT
