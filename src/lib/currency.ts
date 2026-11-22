import type { FxRates } from "./types";

/** UI 언어로 표시 통화 초기값을 추정한다. TWD는 ECB 환율에 없어 USD로 둔다. */
const LOCALE_CURRENCY: Record<string, string> = {
  ko: "KRW",
  ja: "JPY",
  "zh-CN": "CNY",
  fr: "EUR",
  es: "EUR",
  de: "EUR",
  it: "EUR",
  tr: "TRY",
};

/** 선택 목록 맨 앞에 보일 주요 통화. 나머지는 알파벳순으로 뒤에 붙는다. */
export const FEATURED_CURRENCIES = ["USD", "KRW", "JPY", "EUR", "CNY", "GBP", "TRY", "INR"];

export function defaultCurrency(language: string): string {
  return LOCALE_CURRENCY[language] ?? "USD";
}

/** 환율을 모르면 USD로 내려간다 — 틀린 환산보다 정확한 달러가 낫다. */
export function resolveCurrency(wanted: string, rates: FxRates | null): string {
  if (wanted === "USD") return "USD";
  return rates?.rates[wanted] ? wanted : "USD";
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
