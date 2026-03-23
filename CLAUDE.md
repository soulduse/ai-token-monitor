# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

AI Token Monitor - macOS 메뉴 바에서 Claude Code 토큰 사용량과 비용을 실시간 추적하는 Tauri v2 데스크톱 앱. `~/.claude/projects/**/*.jsonl` 파일을 파싱하여 통계를 산출하며, 리더보드 opt-in 시에만 네트워크 요청(Supabase)을 수행한다.

## Build & Run Commands

```bash
npm install                # 의존성 설치
npm run tauri dev          # 개발 모드 (Vite + Tauri 동시 실행)
npm run tauri build        # 프로덕션 빌드 (DMG 생성)
npm run build              # 프론트엔드만 빌드 (tsc + vite build)
```

Vite dev 서버는 포트 1420 고정 (`strictPort: true`). Tauri가 이 포트를 기대하므로 변경 불가.

## Architecture

**Tauri v2 (Rust 백엔드) + React 19 (프론트엔드)** 구조. IPC(`invoke`/`listen`)로 통신.

### Data Flow

```
~/.claude/projects/**/*.jsonl  ──→  Rust File Watcher (notify, 2s debounce)
                                         │
                                         ▼
                                    emit "stats-updated"
                                         │
                                         ▼
                              React useTokenStats hook
                                         │
                                         ▼
                              invoke("get_all_stats")
                                         │
                                         ▼
                              ClaudeCodeProvider::fetch_stats()
                              (메모리 캐시 TTL 60s)
                                         │
                                         ▼
                              AllStats → UI 렌더링
```

### IPC Commands (Rust → React)

| Command | 설명 |
|---------|------|
| `get_all_stats()` | 전체 통계 반환 (daily[], model_usage, sessions/messages) |
| `get_preferences()` / `set_preferences()` | 사용자 설정 JSON 읽기/쓰기 |
| `capture_window()` | macOS CoreGraphics로 윈도우 캡처 → 클립보드 |
| `hide_window()` / `quit_app()` | 창 제어 |

### Backend Events (Rust emit)

- `stats-updated` — 파일 변경 감지 시 프론트엔드에 알림
- `pre-stats` — 트레이 클릭 시 즉시 캐시된 데이터 전달 (성능 최적화)

### Key Abstractions

- **`TokenProvider` trait** (`src-tauri/src/providers/traits.rs`): `name()`, `fetch_stats()`, `is_available()` — 새 데이터 소스 추가 시 이 트레이트 구현
- **`ClaudeCodeProvider`** (`src-tauri/src/providers/claude_code.rs`): JSONL 파싱, 비용 계산, 캐싱 구현체
- **`SettingsContext`** (`src/contexts/SettingsContext.tsx`): 테마, 숫자 포맷, 리더보드 opt-in 등 사용자 설정 관리
- **`AuthContext`** (`src/contexts/AuthContext.tsx`): GitHub OAuth 인증 상태

### Frontend Tab Structure

3-탭 구조: **Overview** (TodaySummary, DailyChart, PeriodTotals, Heatmap) → **Analytics** (ActivityGraph, DailyChart 30d, ModelBreakdown, CacheEfficiency) → **Leaderboard** (Supabase 기반 순위표). Analytics/Leaderboard는 lazy mount (최초 방문 시에만 마운트).

## Type System

프론트엔드(`src/lib/types.ts`)와 백엔드(`src-tauri/src/providers/types.rs`)의 타입 정의가 일치해야 함. `DailyUsage`, `ModelUsage`, `AllStats`, `UserPreferences` 인터페이스가 양쪽에 미러링됨. 한쪽을 변경하면 반드시 다른 쪽도 업데이트.

## Styling

CSS 변수 기반 4개 테마 (`src/styles/global.css`): github, purple, ocean, sunset. `data-theme` 속성 + `prefers-color-scheme: dark` 미디어 쿼리로 다크모드 자동 지원. Tailwind 미사용 — 순수 CSS 변수.

## macOS-Specific Code

`#[cfg(target_os = "macos")]`로 게이팅된 Cocoa/ObjC 코드 존재 (`lib.rs`, `commands.rs`). 윈도우 포지셔닝, 앱 활성화, 스크린샷 캡처 등. 비-macOS 빌드 시 해당 기능은 스텁으로 대체.

## Model Pricing (per million tokens)

`claude_code.rs`의 `ModelPricing`에 정의. 모델명 매칭은 접두사 기반 (`claude-sonnet-4` 등). 새 모델 추가 시 `model_pricing()` 함수 업데이트.

## Preferences Storage

사용자 설정은 `~/.claude/ai-token-monitor-prefs.json`에 JSON으로 저장. Rust의 `commands.rs`에서 직접 파일 읽기/쓰기.

## Supabase (Leaderboard)

`supabase/` 디렉토리에 스키마 정의. 리더보드 opt-in 시에만 활성화. `useLeaderboardSync` 훅이 Supabase에 사용량 데이터 동기화.
