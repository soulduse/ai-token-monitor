# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [繁體中文](README.zh-TW.md)**

一款 macOS 菜单栏应用，实时追踪 Claude Code 的 Token 使用量和费用。

<p align="center">
  <img src="screenshots/overview.png" width="280" alt="Overview" />
  <img src="screenshots/analytics.png" width="280" alt="Analytics" />
  <img src="screenshots/leaderboard.png" width="280" alt="Leaderboard" />
</p>
<p align="center">
  <b>Overview</b> — 今日用量、7天图表、周/月汇总 &nbsp;&nbsp;
  <b>Analytics</b> — 活动图表、30天趋势、模型分析 &nbsp;&nbsp;
  <b>Leaderboard</b> — 与其他用户比较
</p>

## 下载

**[下载最新版本 (.dmg)](https://github.com/soulduse/ai-token-monitor/releases/latest)**

> 仅支持 macOS (Apple Silicon)。Intel Mac 支持即将推出。

## 主要功能

- **实时 Token 追踪** — 直接解析 Claude Code 会话 JSONL 文件，准确显示使用量
- **费用计算** — 基于模型定价（Opus、Sonnet、Haiku）自动计算费用
- **每日图表** — 7/30 天 Token 或费用柱状图，带 Y 轴标签
- **活动图表** — GitHub 风格贡献热力图，支持 2D/3D 切换和年份导航
- **周期导航** — 通过 `< >` 箭头浏览每周/每月汇总
- **模型分析** — Input/Output/Cache 比例可视化
- **缓存效率** — 缓存命中率环形图
- **菜单栏费用** — 在托盘图标旁显示今日费用 ($45)
- **4 种主题** — GitHub（绿色）、Purple、Ocean、Sunset — 支持深色模式
- **截图** — 将应用窗口截图复制到剪贴板
- **剪贴板导出** — 将使用量摘要以 Markdown 格式复制
- **排行榜** — 与其他用户比较使用量（GitHub OAuth，可选参与）
- **自动隐藏** — 点击窗口外部自动隐藏

## 从源码安装

### 前置要求

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 工具链
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- 已安装 [Claude Code](https://claude.ai/claude-code) 且至少使用过一次

### 构建

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 开发模式
npm run tauri build   # 生产构建
```

## 使用方法

### 基本使用

1. 启动应用后，macOS 菜单栏会出现图标
2. 点击图标打开使用量仪表板
3. 在 **Overview**、**Analytics** 和 **Leaderboard** 标签页之间切换

### 标签页说明

| 标签页 | 内容 |
|--------|------|
| **Overview** | 今日摘要、7 天图表、每周/每月汇总、8 周热力图 |
| **Analytics** | 全年活动图表（2D/3D）、30 天图表、模型分析、缓存效率 |
| **Leaderboard** | 与其他用户比较使用量（可选参与） |

### 设置

点击右上角齿轮图标进行配置：
- **Theme**：GitHub / Purple / Ocean / Sunset 主题切换
- **Number Format**：紧凑格式 (377.0K) 与完整格式 (377,000) 切换
- **Menu Bar Cost**：在菜单栏显示/隐藏今日费用
- **Leaderboard**：选择是否共享使用数据 + GitHub 登录

### 排行榜

1. 在设置中启用 "Share Usage Data"
2. 点击 "Sign in with GitHub"
3. 在 Leaderboard 标签页查看 Today / This Week 排名

共享数据：每日 Token 总数、费用、消息/会话数（不共享代码或对话内容）

## 数据来源

应用直接读取 `~/.claude/projects/**/*.jsonl` 文件来汇总 Token 使用量。
补充的会话/工具调用次数来自 `~/.claude/stats-cache.json`。

**网络请求**：仅在启用排行榜时发送（向 Supabase 发送汇总数据）。
不使用排行榜时，应用完全离线运行。

## 架构

```
┌──────────────────────────────┐
│  Frontend (React 19 + Vite)  │
│  ├── PopoverShell            │
│  ├── TabBar (3 标签页)        │
│  ├── TodaySummary            │
│  ├── DailyChart (SVG)        │
│  ├── ActivityGraph (2D/3D)   │
│  ├── Heatmap                 │
│  ├── ModelBreakdown          │
│  ├── CacheEfficiency         │
│  └── Leaderboard             │
├──────────────────────────────┤
│  Backend (Tauri v2 / Rust)   │
│  ├── JSONL 会话解析器         │
│  ├── 文件监视 (notify)        │
│  ├── 托盘图标 + 费用显示      │
│  └── 偏好设置 (JSON)          │
├──────────────────────────────┤
│  数据来源                     │
│  └── ~/.claude/projects/     │
│      └── **/*.jsonl           │
└──────────────────────────────┘
```

## 平台支持

| 平台 | 状态 | 备注 |
|------|------|------|
| **macOS** | 已支持 | 菜单栏集成、Dock 隐藏、托盘费用显示 |
| **Windows** | 计划中 | 核心逻辑跨平台。macOS 专用代码通过 `#[cfg(target_os)]` 隔离 |
| **Linux** | 未测试 | Tauri 支持 Linux，基本功能可能可用 |

## 赞助

如果您觉得这个项目有用，欢迎[请我喝杯咖啡](https://ctee.kr/place/programmingzombie)来支持开发。

## 许可证

MIT
