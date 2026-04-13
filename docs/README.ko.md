# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Türkçe](README.tr.md) | [Italiano](README.it.md)**

macOS / Windows 시스템 트레이에서 **Claude Code**, **Codex**, **OpenCode**의 토큰 사용량·비용·활동을 실시간으로 추적하는 앱입니다. 리더보드, 채팅, 웹훅 알림까지 한 화면에서 확인할 수 있습니다.

| Overview | Analytics | Leaderboard |
|:---:|:---:|:---:|
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> | <img src="screenshots/leaderboard.png" width="280" /> |
| 오늘의 사용량, 7일 차트, 주간/월간 집계 | 활동 그래프, 30일 트렌드, 모델별 분석 | 다른 개발자와 사용량 비교 |

## 다운로드

**[최신 릴리즈 다운로드](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| 플랫폼 | 파일 | 비고 |
|--------|------|------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac 지원 예정 |
| **Windows** | `.exe` 인스톨러 | Windows 10+ (WebView2 필요, 자동 설치) |

## 주요 기능

### 추적 & 시각화
- **실시간 토큰 추적** — Claude Code / Codex / OpenCode 세션 JSONL 파일을 직접 파싱해 정확한 사용량 집계
- **멀티 프로바이더** — Claude / Codex / OpenCode 간 자유 전환, 프로바이더별 가격 모델 적용
- **여러 설정 디렉토리** — 업무/개인 계정을 동시에 합산하도록 Claude/Codex 루트 경로를 여러 개 등록
- **일별 차트** — 7/30일 토큰 사용량 또는 비용 바 차트 (Y축 레이블 포함)
- **활동 그래프** — GitHub 스타일 컨트리뷰션 히트맵 (2D/3D 토글, 연도 네비게이션)
- **기간 네비게이션** — 주간/월간 집계를 `< >` 화살표로 과거까지 탐색
- **모델별 분석** — Input/Output/Cache 비율 시각화
- **캐시 효율** — 캐시 히트율 도넛 차트
- **사용량 알림 바** — Claude Code 5시간 세션 + 주간 한도 실시간 표시 (선택적 Claude OAuth 연동)

### 소셜 & 공유
- **리더보드** — 일/주/월 사용량을 다른 개발자와 비교 (GitHub OAuth, opt-in)
- **7일 TOP 10 그리드** — 한눈에 보는 랭킹 히스토리
- **미니 프로필** — 활동 히트맵, 연속 접속일, 외부 프로필 링크
- **배지** — Card / Compact / Flat Square 스타일, PNG / SVG / Markdown / 라이브 URL로 내보내 GitHub README에 임베드 가능
- **채팅** — 리더보드 멤버용 인앱 채팅 (멘션, 답글, 이미지 첨부, 읽지 않음 배지, 타이핑 인디케이터, AI 번역)
- **나의 AI 리포트** — 월간/연간 회고 카드 (최다 사용 모델, 가장 바쁜 하루, 연속 기록)
- **영수증 뷰** — 오늘 / 주간 / 월간 / 전체 기간의 영수증 스타일 요약
- **월급 비교기** — AI 지출을 월급 대비 비율(라떼/넷플릭스/치킨)로 환산
- **공유 & 내보내기** — 헤더 메뉴에서 마크다운 요약 복사, 스크린샷 캡처, 앱 공유 메시지 복사

### 알림
- **트레이 비용** — 오늘 비용을 트레이 아이콘 옆에 표시 (macOS 메뉴바 타이틀, Windows 툴팁)
- **웹훅 알림** — 사용량 임계치 도달 또는 리셋 시 Discord / Slack / Telegram으로 알림
- **자동 업데이터** — 인앱 업데이트 안내 + 다운로드 진행률

### 커스터마이즈
- **4가지 테마** — GitHub (초록), Purple, Ocean, Sunset + Auto/Light/Dark 모드
- **10개 언어** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **숫자 포맷** — 약식(`377.0K`) / 전체(`377,000`) 전환
- **자동 시작** — 부팅 시 자동 실행 옵션
- **AI 번역** — Gemini / OpenAI / Anthropic API 키를 등록하면 채팅 메시지 번역 (키는 로컬에 암호화 저장)
- **자동 숨김** — 창 밖 클릭 시 자동 숨김

## 소스에서 설치

### 사전 요구사항

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) 툴체인
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code), [Codex](https://openai.com/index/introducing-codex/), 또는 [OpenCode](https://opencode.ai) 중 하나 이상이 설치되어 있고 최소 1회 이상 사용한 상태

### 빌드

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # 개발 모드
npm run tauri build   # 프로덕션 빌드
```

## 사용 방법

### 기본 사용

1. 앱을 실행하면 시스템 트레이(macOS 메뉴바 / Windows 작업 표시줄)에 아이콘이 나타납니다
2. 아이콘을 클릭하면 사용량 대시보드가 표시됩니다
3. **Overview**, **Analytics**, **Leaderboard**, **Chat** 탭으로 전환

### 탭 설명

| 탭 | 내용 |
|-----|------|
| **Overview** | 오늘의 요약, 7일 차트, 주간/월간 집계, 8주 히트맵, 사용량 알림 바 |
| **Analytics** | 연간 활동 그래프 (2D/3D), 30일 차트, 모델별 사용량, 캐시 효율 |
| **Leaderboard** | 사용량 랭킹, 7일 TOP 10 그리드, 배지, 미니 프로필 |
| **Chat** | 리더보드 멤버들과 실시간 채팅 — 멘션, 답글, 이미지, AI 번역 |

### 헤더 액션

헤더에는 **공유 버튼**, **⋯ 메뉴**, **⚙ 설정** 버튼이 있습니다. 더 보기 메뉴에는 다음 항목이 포함됩니다:

- **GitHub 저장소 보기** — 브라우저로 저장소 열기
- **나의 AI 리포트** — 월간/연간 회고 카드
- **영수증** — 영수증 스타일 사용량 요약
- **앱 공유하기** — 추천 메시지 + 저장소 링크 클립보드 복사
- **스크린샷 캡처** — 현재 화면을 클립보드에 복사

### 설정

설정은 4개의 탭으로 구성되어 있습니다:

| 탭 | 옵션 |
|-----|------|
| **General** | 테마, 언어, 외관, 숫자 포맷, 메뉴바 비용, 자동 시작, 월급, 사용량 알림, Claude/Codex 디렉토리, Claude 사용량 추적(OAuth) |
| **Account** | GitHub 로그인, 리더보드 opt-in, 프로필 링크 |
| **AI** | Gemini / OpenAI / Anthropic API 키 (채팅 번역, 로컬 암호화 저장) |
| **Webhooks** | Discord / Slack / Telegram 웹훅 URL, 알림 임계치, 모니터링 윈도우, 리셋 알림 |

### 리더보드 & 채팅

1. 설정 → Account에서 "Share Usage Data" 활성화
2. "Sign in with GitHub" 클릭
3. Leaderboard 탭에서 순위 확인, Chat 탭에서 대화 참여

공유되는 데이터: 일별 총 토큰 수, 비용, 메시지/세션 수. **코드나 대화 내용은 공유되지 않습니다.**

## 데이터 소스

| 프로바이더 | 경로 | 비고 |
|-----------|------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | `~/.claude/stats-cache.json`에서 세션/툴 호출 수 보조. 여러 루트 지원. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | 여러 루트 지원. |
| **OpenCode** | `~/.local/share/opencode/**/*.jsonl` | 내장 가격 레지스트리 기반 모델별 비용 계산. |

**네트워크 요청**: 리더보드/채팅 opt-in 시 Supabase로 집계 데이터 전송, 웹훅 발화 시 외부 전송. 이 기능을 쓰지 않으면 앱은 완전히 오프라인으로 동작합니다. AI 번역 키를 설정한 경우에만 해당 프로바이더로 직접 요청이 전송됩니다.

## 아키텍처

```
┌────────────────────────────────────┐
│  Frontend (React 19 + Vite)        │
│  ├── PopoverShell / Header         │
│  ├── TabBar (4탭)                  │
│  ├── TodaySummary / DailyChart     │
│  ├── ActivityGraph (2D/3D) / Heatmap│
│  ├── ModelBreakdown / CacheEfficiency│
│  ├── Leaderboard + Grid + Badges   │
│  ├── Chat + MentionAutocomplete    │
│  ├── MiniProfile / Wrapped / Receipt│
│  ├── SalaryComparator / UsageAlertBar│
│  └── SettingsOverlay (4탭)         │
├────────────────────────────────────┤
│  Backend (Tauri v2 / Rust)         │
│  ├── JSONL 세션 파서 (Claude/Codex/OpenCode)│
│  ├── 파일 감시 (notify)            │
│  ├── 트레이 아이콘 + 비용 표시     │
│  ├── 자동 업데이터                 │
│  ├── 웹훅 디스패처                 │
│  └── 설정 + 암호화 시크릿          │
├────────────────────────────────────┤
│  외부 서비스 (opt-in)              │
│  ├── Supabase (리더보드 + 채팅)    │
│  ├── Discord / Slack / Telegram    │
│  └── Gemini / OpenAI / Anthropic   │
└────────────────────────────────────┘
```

## 플랫폼 지원

| 플랫폼 | 상태 | 비고 |
|--------|------|------|
| **macOS** | 지원 | 메뉴바 통합, Dock 숨김, 트레이 비용 타이틀 |
| **Windows** | 지원 | 시스템 트레이 통합, NSIS 인스톨러, 툴팁 비용 표시 |
| **Linux** | 미테스트 | Tauri가 Linux를 지원하므로 동작 가능성 있음 |

## 후원

이 프로젝트가 유용하다면 [커피 한 잔 사주기](https://ctee.kr/place/programmingzombie)로 응원해주세요.

## 라이선스

MIT
