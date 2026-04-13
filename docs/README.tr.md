# AI Token Monitor

[![Release](https://img.shields.io/github/v/release/soulduse/ai-token-monitor)](https://github.com/soulduse/ai-token-monitor/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

> **[English](../README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [Italiano](README.it.md)**

macOS ve Windows sistem tepsisinde **Claude Code**, **Codex** ve **OpenCode** token kullanımını, maliyetini ve etkinliğini gerçek zamanlı olarak izleyen bir uygulamadır. Liderlik tablosu, sohbet ve webhook bildirimleri tek bir ekranda sunulur.

| Overview | Analytics | Leaderboard |
|:---:|:---:|:---:|
| <img src="screenshots/overview.png" width="280" /> | <img src="screenshots/analytics.png" width="280" /> | <img src="screenshots/leaderboard.png" width="280" /> |
| Bugünün kullanımı, 7 günlük grafik, haftalık/aylık toplamlar | Etkinlik grafiği, 30 günlük trendler, model bazlı analiz | Kullanımınızı diğer geliştiricilerle karşılaştırın |

## İndirme

**[Son Sürümü İndir](https://github.com/soulduse/ai-token-monitor/releases/latest)**

| Platform | Dosya | Notlar |
|----------|-------|--------|
| **macOS** (Apple Silicon) | `.dmg` | Intel Mac desteği yakında |
| **Windows** | `.exe` yükleyici | Windows 10+ (WebView2 gerekli, otomatik kurulur) |

## Özellikler

### İzleme ve Görselleştirme
- **Gerçek zamanlı token izleme** — Claude Code / Codex / OpenCode oturum JSONL dosyalarını doğrudan ayrıştırarak kesin kullanım istatistikleri sunar
- **Çoklu sağlayıcı desteği** — Claude / Codex / OpenCode arasında geçiş yapın; her sağlayıcı için ayrı maliyet modeli
- **Birden fazla yapılandırma dizini** — iş ve kişisel hesapları birleştirmek için birden fazla Claude/Codex kök dizini ekleyin
- **Günlük grafik** — 7/30 günlük token veya maliyet çubuk grafiği (Y ekseni etiketleriyle)
- **Etkinlik grafiği** — GitHub tarzı katkı ısı haritası, 2D/3D geçişi ve yıl gezintisi
- **Dönem gezintisi** — `< >` oklarıyla haftalık/aylık toplamları geriye doğru tarayın
- **Model bazlı analiz** — Input/Output/Cache oranı görselleştirmesi
- **Önbellek verimliliği** — önbellek isabet oranını gösteren halka grafik
- **Kullanım uyarı çubuğu** — Claude Code 5 saatlik oturum + haftalık plan limitleri canlı gösterimi (isteğe bağlı Claude OAuth bağlantısı)

### Sosyal ve Paylaşım
- **Liderlik tablosu** — günlük/haftalık/aylık kullanımınızı diğer geliştiricilerle karşılaştırın (GitHub OAuth, isteğe bağlı)
- **7 günlük TOP 10 tablosu** — sıralama geçmişini tek bakışta görün
- **Mini profil** — etkinlik ısı haritası, ardışık gün serisi, harici profil bağlantıları
- **Rozetler** — Card / Compact / Flat Square stilleri, PNG / SVG / Markdown / canlı URL olarak dışa aktarın ve GitHub README'nize gömün
- **Sohbet** — liderlik tablosu üyeleri için uygulama içi sohbet (bahsetmeler, yanıtlar, görsel ekleri, okunmamış sayacı, yazıyor göstergesi ve yapay zeka çevirisi)
- **Yapay Zeka Raporu (Wrapped)** — en çok kullanılan model, en yoğun gün ve ardışık seri bilgilerini içeren aylık/yıllık özet kartı
- **Fiş görünümü** — bugün / haftalık / aylık / tüm zamanlar için fiş tarzı kullanım özeti
- **Maaş karşılaştırıcı** — aylık yapay zeka harcamanızı maaşınıza oranla görün (latte / Netflix / tavuk)
- **Paylaş ve dışa aktar** — başlık menüsünden markdown özeti kopyalayın, ekran görüntüsü alın veya uygulama paylaşım mesajını kopyalayın

### Bildirimler ve Uyarılar
- **Tepsi maliyeti** — bugünün maliyeti tepsi simgesinin yanında gösterilir (macOS menü çubuğu başlığı, Windows araç ipucu)
- **Webhook bildirimleri** — kullanım eşikleri aşıldığında veya sıfırlandığında Discord / Slack / Telegram uyarıları
- **Otomatik güncelleyici** — uygulama içi güncelleme bildirimleri ve indirme ilerleme durumu

### Özelleştirme
- **4 tema** — GitHub (yeşil), Purple, Ocean, Sunset + Auto/Light/Dark modu
- **10 dil** — English, 한국어, 日本語, 简体中文, 繁體中文, Français, Español, Deutsch, Türkçe, Italiano
- **Sayı biçimi** — kısa (`377.0K`) / tam (`377,000`) arasında geçiş
- **Başlangıçta çalıştır** — açılışta otomatik başlatma seçeneği
- **Yapay zeka çevirisi** — Gemini / OpenAI / Anthropic API anahtarınızı ekleyerek sohbet mesajlarını çevirin (anahtarlar yerel olarak şifrelenir)
- **Otomatik gizle** — pencere dışına tıklandığında otomatik gizleme

## Kaynaktan Kurulum

### Ön Koşullar

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) araç zinciri
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- [Claude Code](https://claude.ai/claude-code), [Codex](https://openai.com/index/introducing-codex/) veya [OpenCode](https://opencode.ai) kurulu ve en az bir kez kullanılmış olmalıdır

### Derleme

```bash
git clone https://github.com/soulduse/ai-token-monitor.git
cd ai-token-monitor
npm install
npm run tauri dev     # geliştirme modu
npm run tauri build   # üretim derlemesi
```

## Kullanım

### Temel Kullanım

1. Uygulamayı başlatın — sistem tepsisinde (macOS menü çubuğu / Windows görev çubuğu) bir simge belirir
2. Simgeye tıklayarak kullanım panosunu açın
3. **Overview**, **Analytics**, **Leaderboard** ve **Chat** sekmeleri arasında geçiş yapın

### Sekmeler

| Sekme | İçerik |
|-------|--------|
| **Overview** | Bugünün özeti, 7 günlük grafik, haftalık/aylık toplamlar, 8 haftalık ısı haritası, kullanım uyarı çubuğu |
| **Analytics** | Yıllık etkinlik grafiği (2D/3D), 30 günlük grafik, model bazlı kullanım, önbellek verimliliği |
| **Leaderboard** | Kullanım sıralaması, 7 günlük TOP 10 tablosu, rozetler, mini profiller |
| **Chat** | Liderlik tablosu üyeleriyle gerçek zamanlı sohbet — bahsetmeler, yanıtlar, görseller, yapay zeka çevirisi |

### Başlık Eylemleri

Başlıkta bir **paylaş** düğmesi, bir **⋯ daha fazla** açılır menüsü ve bir **⚙ ayarlar** düğmesi bulunur. Daha fazla menüsü şunları içerir:

- **GitHub deposu** — tarayıcınızda depoyu açın
- **Yapay Zeka Raporu** — aylık/yıllık özet kartınızı görün
- **Fiş** — fiş tarzı kullanım özeti
- **Uygulamayı paylaş** — öneri mesajı + depo bağlantısını panoya kopyalayın
- **Ekran görüntüsü al** — mevcut görünümü panoya kopyalayın

### Ayarlar

Ayarlar dört sekmeden oluşur:

| Sekme | Seçenekler |
|-------|------------|
| **General** | Tema, dil, görünüm, sayı biçimi, menü çubuğu maliyeti, başlangıçta çalıştır, aylık maaş, kullanım uyarıları, Claude/Codex dizinleri, Claude kullanım takibi (OAuth) |
| **Account** | GitHub oturumu, liderlik tablosu katılımı, profil bağlantıları |
| **AI** | Gemini / OpenAI / Anthropic API anahtarları (sohbet çevirisi, yerel şifreli depolama) |
| **Webhooks** | Discord / Slack / Telegram webhook URL'leri, uyarı eşikleri, izleme pencereleri, sıfırlama bildirimleri |

### Liderlik Tablosu ve Sohbet

1. Ayarlar → Account bölümünde "Share Usage Data" seçeneğini etkinleştirin
2. "Sign in with GitHub" düğmesine tıklayın
3. **Leaderboard** sekmesinde sıralamanızı görün, **Chat** sekmesinde sohbete katılın

Paylaşılan veriler: günlük toplam token sayısı, maliyet, mesaj/oturum sayısı. **Kod veya konuşma içeriği paylaşılmaz.**

## Veri Kaynakları

| Sağlayıcı | Yol | Notlar |
|------------|-----|--------|
| **Claude Code** | `~/.claude/projects/**/*.jsonl` | `~/.claude/stats-cache.json` üzerinden oturum/araç çağrı sayıları. Birden fazla kök dizin desteklenir. |
| **Codex** | `~/.codex/sessions/**/*.jsonl` | Birden fazla kök dizin desteklenir. |
| **OpenCode** | `~/.local/share/opencode/**/*.jsonl` | Yerleşik fiyatlandırma kaydına dayalı model bazlı maliyet hesaplaması. |

**Ağ istekleri**: yalnızca liderlik tablosu/sohbet etkinleştirildiğinde (Supabase'e toplu veri gönderilir) veya bir webhook tetiklendiğinde gerçekleşir. Bu özellikler kullanılmadığında uygulama tamamen çevrimdışı çalışır. Yapay zeka çeviri anahtarı ayarlandıysa, yalnızca seçtiğiniz sağlayıcıya doğrudan istek gönderilir.

## Mimari

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

## Platform Desteği

| Platform | Durum | Notlar |
|----------|-------|--------|
| **macOS** | Destekleniyor | Menü çubuğu entegrasyonu, Dock gizleme, tepsi maliyet başlığı |
| **Windows** | Destekleniyor | Sistem tepsisi entegrasyonu, NSIS yükleyici, araç ipucu maliyet gösterimi |
| **Linux** | Test edilmedi | Tauri Linux'u desteklediği için çalışma olasılığı var |

## Destek

Bu projeyi faydalı buluyorsanız [bir kahve ısmarlayarak](https://ctee.kr/place/programmingzombie) destek olabilirsiniz.

## Lisans

MIT
