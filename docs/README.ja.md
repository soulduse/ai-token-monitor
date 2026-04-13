# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Türkçe](README.tr.md) | [Italiano](README.it.md)**

macOS と Windows のシステムトレイで **Claude Code**、**Codex**、**OpenCode** のトークン使用量・コスト・アクティビティをリアルタイムに追跡するアプリです。リーダーボード、チャット、Webhook 通知まで 1 画面で確認できます。

| Overview | Analytics | Leaderboard |
|:---:|:---:|:---:|
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> | <img src="screenshots/leaderboard.png" width="280" /> |
| 今日の使用量、7日間チャート、週間/月間集計 | アクティビティグラフ、30日間トレンド、モデル別分析 | 他の開発者と使用量を比較 |

## ダウンロード

**[最新リリースをダウンロード](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| プラットフォーム | ファイル | 備考 |
|-----------------|---------|------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac 対応予定 |
| **Windows** | `.exe` インストーラー | Windows 10+（WebView2 必要、自動インストール） |

## 主な機能

### 追跡 & 可視化
- **リアルタイムトークン追跡** — Claude Code / Codex / OpenCode のセッション JSONL を直接パースして正確に集計
- **マルチプロバイダー対応** — Claude / Codex / OpenCode を切替、プロバイダー別の価格モデルを適用
- **複数の設定ディレクトリ** — 仕事用 + 個人用アカウントを同時に集計できるよう Claude/Codex ルートを複数追加可能
- **日別チャート** — 7/30 日間のトークンまたはコストの棒グラフ（Y 軸ラベル付き）
- **アクティビティグラフ** — GitHub スタイルのコントリビューションヒートマップ（2D/3D 切替、年ナビゲーション）
- **期間ナビゲーション** — `< >` 矢印で週間/月間集計を過去までブラウズ
- **モデル別分析** — Input/Output/Cache 比率の可視化
- **キャッシュ効率** — キャッシュヒット率のドーナツチャート
- **使用量アラートバー** — Claude Code の 5 時間セッション + 週間プラン上限をライブ表示（任意で Claude OAuth 連携）

### ソーシャル & 共有
- **リーダーボード** — 日/週/月の使用量を他の開発者と比較（GitHub OAuth、オプトイン）
- **7 日間 TOP 10 グリッド** — 一目で分かるランキング履歴
- **ミニプロフィール** — アクティビティヒートマップ、連続日数、外部プロフィールリンク
- **バッジ** — Card / Compact / Flat Square スタイル、PNG / SVG / Markdown / ライブ URL として出力し GitHub README に埋め込み可能
- **チャット** — リーダーボードメンバー向けのアプリ内チャット（メンション、返信、画像添付、未読バッジ、入力中表示、AI 翻訳）
- **AI レポート (Wrapped)** — 月間/年間のまとめカード（よく使うモデル、もっとも忙しかった日、連続記録）
- **レシートビュー** — 今日 / 週 / 月 / 全期間のレシート風サマリー
- **給与コンパレーター** — AI への支出を月給に占める割合（ラテ/Netflix/チキン換算）で表示
- **共有 & エクスポート** — ヘッダーメニューから Markdown サマリーのコピー、スクリーンショット、アプリ共有メッセージのコピー

### 通知
- **トレイコスト** — 今日のコストをトレイアイコンの横に表示（macOS メニューバータイトル / Windows ツールチップ）
- **Webhook 通知** — 使用量が閾値に達したり、リセットされた際に Discord / Slack / Telegram へ通知
- **自動アップデーター** — アプリ内アップデート通知 + ダウンロード進捗

### カスタマイズ
- **4 つのテーマ** — GitHub（グリーン）、Purple、Ocean、Sunset + Auto/Light/Dark モード
- **10 言語対応** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **数値フォーマット** — 短縮（`377.0K`）/ 完全（`377,000`）切替
- **自動起動** — 起動時に自動実行
- **AI 翻訳** — Gemini / OpenAI / Anthropic の API キーを登録するとチャットメッセージを翻訳（キーはローカルで暗号化保存）
- **自動非表示** — ウィンドウ外クリックで自動的に非表示

## ソースからインストール

### 前提条件

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) ツールチェイン
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code)、[Codex](https://openai.com/index/introducing-codex/)、または [OpenCode](https://opencode.ai) のいずれかがインストール済みで、1 回以上使用していること

### ビルド

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 開発モード
npm run tauri build   # プロダクションビルド
```

## 使い方

### 基本

1. アプリを起動するとシステムトレイ（macOS メニューバー / Windows タスクバー）にアイコンが表示されます
2. アイコンをクリックしてダッシュボードを開きます
3. **Overview**、**Analytics**、**Leaderboard**、**Chat** タブを切り替えます

### タブ説明

| タブ | 内容 |
|------|------|
| **Overview** | 今日のサマリー、7 日間チャート、週間/月間集計、8 週間ヒートマップ、使用量アラートバー |
| **Analytics** | 年間アクティビティグラフ（2D/3D）、30 日間チャート、モデル別分析、キャッシュ効率 |
| **Leaderboard** | 使用量ランキング、7 日間 TOP 10 グリッド、バッジ、ミニプロフィール |
| **Chat** | リーダーボードメンバーとのリアルタイムチャット — メンション、返信、画像、AI 翻訳 |

### ヘッダーアクション

ヘッダーには **共有ボタン**、**⋯ メニュー**、**⚙ 設定** ボタンがあります。メニューには以下が含まれます：

- **GitHub リポジトリを開く** — ブラウザでリポジトリを開く
- **AI レポート** — 月間/年間のまとめカード
- **レシート** — レシート風の使用量サマリー
- **アプリを共有** — おすすめメッセージ + リポジトリリンクをクリップボードへコピー
- **スクリーンショットをキャプチャ** — 現在のビューをクリップボードへコピー

### 設定

設定は 4 つのタブで構成されています：

| タブ | オプション |
|------|-----------|
| **General** | テーマ、言語、外観、数値フォーマット、メニューバーコスト、自動起動、月給、使用量アラート、Claude/Codex ディレクトリ、Claude 使用量追跡（OAuth） |
| **Account** | GitHub サインイン、リーダーボードオプトイン、プロフィールリンク |
| **AI** | Gemini / OpenAI / Anthropic API キー（チャット翻訳、ローカル暗号化保存） |
| **Webhooks** | Discord / Slack / Telegram の Webhook URL、閾値、監視ウィンドウ、リセット通知 |

### リーダーボード & チャット

1. Settings → Account で "Share Usage Data" を有効化
2. "Sign in with GitHub" をクリック
3. Leaderboard タブで順位を確認し、Chat タブで会話に参加

共有されるデータ：日別トークン合計、コスト、メッセージ/セッション数。**コードや会話内容は共有されません。**

## データソース

| プロバイダー | パス | 備考 |
|------------|------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | `~/.claude/stats-cache.json` からセッション/ツール呼び出し数を補足。複数ルート対応。 |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | 複数ルート対応。 |
| **OpenCode** | `~/.local/share/opencode/**/*.jsonl` | 内蔵プライシングレジストリでモデル別コスト計算。 |

**ネットワークリクエスト**：リーダーボード/チャットをオプトインした場合のみ Supabase に集計データを送信し、Webhook 発火時に外部へ送信します。これらの機能を使わなければ、アプリは完全にオフラインで動作します。AI 翻訳キーを設定した場合のみ、該当プロバイダーへ直接リクエストが送信されます。

## アーキテクチャ

```
┌────────────────────────────────────┐
│  Frontend (React 19 + Vite)        │
│  ├── PopoverShell / Header         │
│  ├── TabBar (4 タブ)               │
│  ├── TodaySummary / DailyChart     │
│  ├── ActivityGraph (2D/3D) / Heatmap│
│  ├── ModelBreakdown / CacheEfficiency│
│  ├── Leaderboard + Grid + Badges   │
│  ├── Chat + MentionAutocomplete    │
│  ├── MiniProfile / Wrapped / Receipt│
│  ├── SalaryComparator / UsageAlertBar│
│  └── SettingsOverlay (4 タブ)      │
├────────────────────────────────────┤
│  Backend (Tauri v2 / Rust)         │
│  ├── JSONL セッションパーサー (Claude/Codex/OpenCode)│
│  ├── ファイル監視 (notify)         │
│  ├── トレイアイコン + コスト表示   │
│  ├── 自動アップデーター            │
│  ├── Webhook ディスパッチャー      │
│  └── 設定 + 暗号化シークレット     │
├────────────────────────────────────┤
│  外部サービス (オプトイン)          │
│  ├── Supabase (リーダーボード + チャット)│
│  ├── Discord / Slack / Telegram    │
│  └── Gemini / OpenAI / Anthropic   │
└────────────────────────────────────┘
```

## プラットフォーム対応

| プラットフォーム | 状態 | 備考 |
|-----------------|------|------|
| **macOS** | 対応済み | メニューバー統合、Dock 非表示、トレイコストタイトル |
| **Windows** | 対応済み | システムトレイ統合、NSIS インストーラー、ツールチップコスト表示 |
| **Linux** | 未テスト | Tauri が Linux をサポートしているため、基本動作する可能性あり |

## サポート

このプロジェクトが役に立ったら、[コーヒーをおごる](https://ctee.kr/place/programmingzombie)で応援してください。

## ライセンス

MIT
