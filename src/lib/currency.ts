import type { FxRates } from "./types";
import { COUNTRY_CURRENCY, ZONE_COUNTRY } from "./regionData";

/** 지역 정보가 없을 때 UI 언어로 고르는 통화. */
const LOCALE_CURRENCY: Record<string, string> = {
  ko: "KRW",
  ja: "JPY",
  "zh-CN": "CNY",
  "zh-TW": "TWD",
  fr: "EUR",
  es: "EUR",
  de: "EUR",
  it: "EUR",
  tr: "TRY",
};

/** 선택 목록 맨 앞에 보일 주요 통화. 나머지는 알파벳순으로 뒤에 붙는다. */
const FEATURED_CURRENCIES = ["USD", "KRW", "JPY", "EUR", "CNY", "TWD", "GBP", "TRY", "INR"];

/**
 * 런타임이 아는 ISO 4217 법정통화 목록. 환율 응답에 섞여 오는 암호화폐 코드(BTC 등)를
 * 걸러내는 데 쓴다. 목록을 못 얻는 오래된 런타임이면 null → 거르지 않는다.
 */
const ISO_CURRENCIES: Set<string> | null = (() => {
  const supported = (Intl as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf;
  try {
    return supported ? new Set(supported("currency")) : null;
  } catch {
    return null;
  }
})();

function isSelectable(code: string, rates: FxRates | null): boolean {
  if (code === "USD") return true;
  return Boolean(rates?.rates[code]) && (ISO_CURRENCIES?.has(code) ?? true);
}

function currentTimeZone(): string | undefined {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    return undefined;
  }
}

/**
 * 사용자가 통화를 고른 적 없을 때의 기본값. 네트워크 없이 기기 정보만 쓴다:
 * OS 지역 설정의 국가 → 시간대의 국가 → UI 언어 → USD.
 * (영어 UI를 쓰는 한국 사용자도 KRW, 스페인어 UI의 멕시코 사용자는 MXN)
 */
export function guessCurrency(
  systemRegion: string | null | undefined,
  language: string,
  timeZone: string | undefined = currentTimeZone(),
): string {
  const fromRegion = systemRegion ? COUNTRY_CURRENCY[systemRegion] : undefined;
  const fromZone = timeZone ? COUNTRY_CURRENCY[ZONE_COUNTRY[timeZone]] : undefined;
  return fromRegion ?? fromZone ?? LOCALE_CURRENCY[language] ?? "USD";
}

/** 환율을 모르거나 법정통화가 아니면 USD로 내려간다 — 틀린 환산보다 정확한 달러가 낫다. */
export function resolveCurrency(wanted: string, rates: FxRates | null): string {
  return isSelectable(wanted, rates) ? wanted : "USD";
}

/** 통화 선택 목록: 주요 통화 먼저, 나머지는 알파벳순. 환율이 있는 법정통화만. */
export function selectableCurrencies(rates: FxRates | null): string[] {
  const codes = rates ? Object.keys(rates.rates).filter((c) => isSelectable(c, rates)) : ["USD"];
  const featured = FEATURED_CURRENCIES.filter((c) => codes.includes(c));
  const rest = codes.filter((c) => !featured.includes(c)).sort();
  return [...featured, ...rest];
}

export function usdToCurrency(usd: number, currency: string, rates: FxRates | null): number {
  if (currency === "USD") return usd;
  const rate = rates?.rates[currency];
  return rate ? usd * rate : usd;
}

/**
 * 사용자가 입력한 금액 문자열 → 숫자. "290,000"·"290 000"은 천 단위 구분,
 * "19,99"처럼 끝의 쉼표 뒤가 1~2자리면 소수점으로 본다.
 */
export function parseAmount(input: string): number {
  let s = input.replace(/[^\d.,]/g, "");
  if (/,\d{1,2}$/.test(s) && !s.includes(".")) {
    const i = s.lastIndexOf(",");
    s = s.slice(0, i).replace(/,/g, "") + "." + s.slice(i + 1);
  } else {
    s = s.replace(/,/g, "");
  }
  const n = parseFloat(s);
  return Number.isFinite(n) ? n : NaN;
}

export function currencyToUsd(amount: number, currency: string, rates: FxRates | null): number | null {
  if (currency === "USD") return amount;
  const rate = rates?.rates[currency];
  return rate ? amount / rate : null;
}

const formatters = new Map<string, Intl.NumberFormat>();
const ZERO_DECIMAL = new Set(["KRW", "JPY", "HUF", "IDR", "ISK"]);

/** 통화 기호 + 천 단위 구분. 원·엔처럼 소수 없는 통화는 정수로. */
export function formatMoney(amount: number, currency: string): string {
  const digits = Math.abs(amount) >= 1000 || ZERO_DECIMAL.has(currency) ? 0 : 2;
  const key = `${currency}:${digits}`;
  let fmt = formatters.get(key);
  if (!fmt) {
    fmt = new Intl.NumberFormat(undefined, {
      style: "currency",
      currency,
      currencyDisplay: "narrowSymbol",
      minimumFractionDigits: 0,
      maximumFractionDigits: digits,
    });
    formatters.set(key, fmt);
  }
  return fmt.format(amount);
}
