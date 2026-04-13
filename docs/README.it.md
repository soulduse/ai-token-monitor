# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Türkçe](README.tr.md)**

App per la barra di sistema di macOS e Windows che monitora in tempo reale l'utilizzo dei token, i costi e l'attività di **Claude Code**, **Codex** e **OpenCode**, con classifica, chat e notifiche webhook integrate.

| Overview | Analytics | Leaderboard |
|:---:|:---:|:---:|
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> | <img src="screenshots/leaderboard.png" width="280" /> |
| Utilizzo odierno, grafico 7 giorni, totali settimanali/mensili | Grafico attivita, trend 30 giorni, analisi per modello | Confronta il tuo utilizzo con altri sviluppatori |

## Download

**[Scarica l'ultima versione](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| Piattaforma | File | Note |
|-------------|------|------|
| **macOS** (Apple Silicon) | `.dmg` | Supporto Intel Mac in arrivo |
| **Windows** | Installer `.exe` | Windows 10+ (richiede WebView2, installato automaticamente) |

## Funzionalita

### Monitoraggio e visualizzazione
- **Monitoraggio token in tempo reale** — analizza i file JSONL delle sessioni di Claude Code, Codex e OpenCode per statistiche d'uso precise
- **Supporto multi-provider** — passa liberamente tra Claude / Codex / OpenCode, con modelli di costo specifici per provider
- **Directory di configurazione multiple** — aggrega account di lavoro e personali registrando piu percorsi root di Claude/Codex
- **Grafico giornaliero** — grafico a barre dei token o dei costi su 7/30 giorni con etichette sull'asse Y
- **Grafico attivita** — heatmap dei contributi in stile GitHub con vista 2D/3D e navigazione per anno
- **Navigazione per periodo** — esplora i totali settimanali/mensili con le frecce `< >`
- **Analisi per modello** — visualizzazione del rapporto Input/Output/Cache
- **Efficienza della cache** — grafico a ciambella con il tasso di hit della cache
- **Barra avvisi utilizzo** — sessione Claude Code di 5 ore + limiti settimanali in tempo reale (accesso Claude OAuth opzionale)

### Social e condivisione
- **Classifica** — confronta l'utilizzo giornaliero/settimanale/mensile con altri sviluppatori (GitHub OAuth, opt-in)
- **Griglia TOP 10 a 7 giorni** — cronologia del ranking a colpo d'occhio
- **Mini profilo** — heatmap attivita, serie consecutive, link a profili esterni
- **Badge** — stili Card / Compact / Flat Square, esportazione in PNG / SVG / Markdown / URL live da incorporare nel README di GitHub
- **Chat** — chat in-app per i membri della classifica con menzioni, risposte, allegati immagine, contatore non letti, indicatori di digitazione e traduzione AI
- **Report AI (Wrapped)** — scheda riepilogativa mensile/annuale con modello piu usato, giorno piu attivo e serie consecutive
- **Vista ricevuta** — riepilogo in stile ricevuta per oggi / settimana / mese / totale
- **Comparatore stipendio** — visualizza la spesa AI mensile come percentuale del tuo stipendio (caffe latte / Netflix / pollo)
- **Condivisione e esportazione** — copia il riepilogo in Markdown, cattura uno screenshot o copia un messaggio di condivisione dal menu dell'intestazione

### Notifiche e avvisi
- **Costo nel tray** — il costo odierno viene mostrato accanto all'icona nel tray (titolo nella barra dei menu macOS, tooltip su Windows)
- **Notifiche webhook** — avvisi su Discord, Slack e Telegram quando l'utilizzo supera le soglie o viene reimpostato
- **Aggiornamento automatico** — notifiche di aggiornamento in-app con barra di avanzamento del download

### Personalizzazione
- **4 temi** — GitHub (verde), Purple, Ocean, Sunset — con modalita Auto/Light/Dark
- **10 lingue** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **Formato numerico compatto / esteso** — `377.0K` vs `377,000`
- **Avvio automatico** — opzione di avvio automatico all'accensione del sistema
- **Traduzione AI** — registra la tua chiave API Gemini / OpenAI / Anthropic per tradurre i messaggi della chat (le chiavi sono criptate localmente)
- **Nascondi automaticamente** — la finestra si nasconde quando si clicca al di fuori

## Installazione dal codice sorgente

### Prerequisiti

- [Node.js](https://nodejs.org/) 18+
- Toolchain [Rust](https://rustup.rs/)
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code), [Codex](https://openai.com/index/introducing-codex/) o [OpenCode](https://opencode.ai) installato e utilizzato almeno una volta

### Build

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # modalita sviluppo
npm run tauri build   # build di produzione
```

## Utilizzo

### Uso di base

1. Avvia l'app: un'icona appare nel tray di sistema (barra dei menu macOS / barra delle applicazioni Windows)
2. Clicca sull'icona per aprire la dashboard
3. Passa tra le schede **Overview**, **Analytics**, **Leaderboard** e **Chat**

### Schede

| Scheda | Contenuto |
|--------|-----------|
| **Overview** | Riepilogo odierno, grafico 7 giorni, totali settimanali/mensili, heatmap 8 settimane, barra avvisi utilizzo |
| **Analytics** | Grafico attivita annuale (2D/3D), grafico 30 giorni, analisi per modello, efficienza cache |
| **Leaderboard** | Classifica degli utenti, griglia TOP 10 a 7 giorni, badge, mini profili |
| **Chat** | Chat in tempo reale con i membri della classifica — menzioni, risposte, immagini, traduzione AI |

### Azioni dell'intestazione

L'intestazione presenta un pulsante **condividi**, un menu a tendina **⋯ altro** e un pulsante **⚙ impostazioni**. Il menu altro contiene:

- **Repository GitHub** — apri il repository nel browser
- **Report AI** — scheda riepilogativa mensile/annuale
- **Ricevuta** — riepilogo utilizzo in stile ricevuta
- **Condividi app** — copia un messaggio di raccomandazione + link al repository
- **Cattura screenshot** — copia la vista corrente negli appunti

### Impostazioni

Le impostazioni sono organizzate in quattro schede:

| Scheda | Opzioni |
|--------|---------|
| **General** | Tema, lingua, aspetto, formato numerico, costo nella barra dei menu, avvio automatico, stipendio mensile, avvisi utilizzo, directory Claude/Codex, monitoraggio utilizzo Claude (OAuth) |
| **Account** | Accesso GitHub, opt-in classifica, link profilo |
| **AI** | Chiavi API Gemini / OpenAI / Anthropic per la traduzione in chat (criptate localmente) |
| **Webhooks** | URL webhook Discord / Slack / Telegram, soglie di avviso, finestre monitorate, notifiche di reset |

### Classifica e chat

1. Attiva **Share Usage Data** in Impostazioni → Account
2. Clicca **Sign in with GitHub**
3. Consulta la tua posizione nella scheda **Leaderboard** e partecipa alla scheda **Chat**

Dati condivisi: conteggio giornaliero dei token, costi, messaggi/sessioni. **Nessun codice o contenuto delle conversazioni viene condiviso.**

## Fonti dei dati

| Provider | Percorso | Note |
|----------|----------|------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | Conteggi sessioni/chiamate strumenti da `~/.claude/stats-cache.json`. Supporta root multiple. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | Supporta root multiple. |
| **OpenCode** | `~/.local/share/opencode/**/*.jsonl` | Costi per modello dal registro prezzi integrato. |

**Richieste di rete**: solo quando classifica/chat sono attivati (invio dati aggregati a Supabase) o quando scatta un webhook. Senza queste funzionalita, l'app funziona completamente offline. Le chiavi di traduzione AI, se configurate, inviano richieste direttamente al provider scelto.

## Architettura

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

## Piattaforme supportate

| Piattaforma | Stato | Note |
|-------------|-------|------|
| **macOS** | Supportato | Integrazione barra dei menu, nascondimento dal Dock, costo nel titolo del tray |
| **Windows** | Supportato | Integrazione tray di sistema, installer NSIS, costo nel tooltip |
| **Linux** | Non testato | Potrebbe funzionare dato che Tauri supporta Linux |

## Supporto

Se trovi utile questo progetto, considera di [offrirmi un caffe](https://ctee.kr/place/programmingzombie).

## Licenza

MIT
