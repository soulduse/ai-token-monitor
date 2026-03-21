# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md)**

macOS メニューバーで Claude Code のトークン使用量とコストをリアルタイムに追跡するアプリです。

<p align="center">
  <img src="screenshots/overview.png" width="280" alt="Overview" />
  <img src="screenshots/analytics.png" width="280" alt="Analytics" />
  <img src="screenshots/leaderboard.png" width="280" alt="Leaderboard" />
</p>
<p align="center">
  <b>Overview</b> — 今日の使用量、7日間チャート、週間/月間集計 &nbsp;&nbsp;
  <b>Analytics</b> — アクティビティグラフ、30日間トレンド、モデル別分析 &nbsp;&nbsp;
  <b>Leaderboard</b> — 他のユーザーと比較
</p>

## ダウンロード

**[最新リリースをダウンロード (.dmg)](https://github.com/soulduse/ai-token-monitor/releases/latest)**

> macOS (Apple Silicon) 専用。Intel Mac 対応予定。

## 主な機能

- **リアルタイムトークン追跡** — Claude Code セッション JSONL ファイルを直接パースし、正確な使用量を表示
- **コスト計算** — モデル別（Opus、Sonnet、Haiku）の料金に基づく自動コスト算出
- **日別チャート** — 7/30 日間のトークンまたはコストの棒グラフ（Y 軸ラベル付き）
- **アクティビティグラフ** — GitHub スタイルのコントリビューションヒートマップ（2D/3D 切替、年ナビゲーション）
- **期間ナビゲーション** — `< >` 矢印で週間/月間集計を過去期間まで閲覧
- **モデル別分析** — Input/Output/Cache 比率の可視化
- **キャッシュ効率** — キャッシュヒット率のドーナツチャート
- **メニューバーコスト** — トレイアイコンの横に今日のコストをリアルタイム表示 ($45)
- **4 つのテーマ** — GitHub（グリーン）、Purple、Ocean、Sunset — ダークモード対応
- **スクリーンショット** — アプリウィンドウをクリップボードにキャプチャ
- **クリップボードエクスポート** — 使用量サマリーを Markdown でコピー
- **リーダーボード** — 他のユーザーと使用量を比較（GitHub OAuth、オプトイン）
- **自動非表示** — ウィンドウ外をクリックすると自動的に非表示

## ソースからインストール

### 前提条件

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) ツールチェイン
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code) がインストール済みで、1 回以上使用していること

### ビルド

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 開発モード
npm run tauri build   # プロダクションビルド
```

## 使い方

### 基本的な使い方

1. アプリを起動すると、macOS メニューバーにアイコンが表示されます
2. アイコンをクリックして使用量ダッシュボードを開きます
3. **Overview**、**Analytics**、**Leaderboard** タブを切り替えます

### タブ説明

| タブ | 内容 |
|------|------|
| **Overview** | 今日のサマリー、7 日間チャート、週間/月間集計、8 週間ヒートマップ |
| **Analytics** | 年間アクティビティグラフ（2D/3D）、30 日間チャート、モデル別分析、キャッシュ効率 |
| **Leaderboard** | 他のユーザーと使用量を比較（オプトイン） |

### 設定

右上の歯車アイコンをクリックして設定：
- **Theme**：GitHub / Purple / Ocean / Sunset テーマ切替
- **Number Format**：短縮表示 (377.0K) と完全表示 (377,000) の切替
- **Menu Bar Cost**：メニューバーに今日のコストを表示/非表示
- **Leaderboard**：使用量データの共有オプトイン + GitHub ログイン

### リーダーボード

1. 設定で "Share Usage Data" を有効にする
2. "Sign in with GitHub" をクリック
3. Leaderboard タブで Today / This Week のランキングを確認

共有されるデータ：日別トークン合計、コスト、メッセージ/セッション数（コードや会話内容は共有されません）

## データソース

アプリは `~/.claude/projects/**/*.jsonl` ファイルを直接読み取り、トークン使用量を集計します。
補足的に `~/.claude/stats-cache.json` からセッション/ツール呼び出し回数を取得します。

**ネットワークリクエスト**：リーダーボードをオプトインした場合のみ（Supabase に集計データを送信）。
リーダーボードを使用しない場合、アプリは完全にオフラインで動作します。

## アーキテクチャ

```
┌──────────────────────────────┐
│  Frontend (React 19 + Vite)  │
│  ├── PopoverShell            │
│  ├── TabBar (3 タブ)          │
│  ├── TodaySummary            │
│  ├── DailyChart (SVG)        │
│  ├── ActivityGraph (2D/3D)   │
│  ├── Heatmap                 │
│  ├── ModelBreakdown          │
│  ├── CacheEfficiency         │
│  └── Leaderboard             │
├──────────────────────────────┤
│  Backend (Tauri v2 / Rust)   │
│  ├── JSONL セッションパーサー  │
│  ├── ファイル監視 (notify)     │
│  ├── トレイアイコン + コスト表示│
│  └── 設定 (JSON)              │
├──────────────────────────────┤
│  データソース                  │
│  └── ~/.claude/projects/     │
│      └── **/*.jsonl           │
└──────────────────────────────┘
```

## プラットフォーム対応

| プラットフォーム | 状態 | 備考 |
|-----------------|------|------|
| **macOS** | 対応済み | メニューバー統合、Dock 非表示、トレイコスト表示 |
| **Windows** | 予定 | コアロジックはクロスプラットフォーム。macOS 固有コードは `#[cfg(target_os)]` で分離 |
| **Linux** | 未テスト | Tauri が Linux をサポートしているため、基本動作する可能性あり |

## サポート

このプロジェクトが役に立ったら、[コーヒーをおごる](https://ctee.kr/place/programmingzombie)で応援してください。

## ライセンス

MIT
