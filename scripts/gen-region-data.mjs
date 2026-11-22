// src/lib/regionData.ts 생성기 — "API였다면?" 기본 통화 추정용 표 두 개를 만든다.
//
//   node scripts/gen-region-data.mjs
//
// - 국가 → 통화: Unicode CLDR supplemental currencyData.json (현재 통용 법정통화)
// - 시간대 → 국가: IANA tz database zone.tab (/usr/share/zoneinfo/zone.tab, public domain)
//   + 같은 DB의 backward 별칭(Asia/Calcutta 등 — 브라우저 ICU가 예전 이름을 돌려줄 때가 있다)
// 런타임에는 네트워크를 쓰지 않도록 결과를 소스에 박아 둔다. 데이터가 바뀌면 다시 돌린다.
import { readFileSync, writeFileSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const TZ_BACKWARD_URL = "https://raw.githubusercontent.com/eggert/tz/main/backward";
const CLDR_URL =
  "https://raw.githubusercontent.com/unicode-org/cldr-json/main/cldr-json/cldr-core/supplemental/currencyData.json";
const ZONE_TAB = process.env.ZONE_TAB ?? "/usr/share/zoneinfo/zone.tab";
const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const cldr = await (await fetch(CLDR_URL)).json();
const regions = cldr.supplemental.currencyData.region;

// 각 지역에서 종료일(_to)이 없고 법정통화(_tender != false)인 첫 통화.
const countryCurrency = {};
for (const [region, entries] of Object.entries(regions)) {
  if (!/^[A-Z]{2}$/.test(region)) continue;
  for (const entry of entries) {
    const [code, info] = Object.entries(entry)[0];
    if (!info._to && info._tender !== "false") {
      countryCurrency[region] = code;
      break;
    }
  }
}

const zoneCountry = {};
for (const line of readFileSync(ZONE_TAB, "utf8").split("\n")) {
  if (!line || line.startsWith("#")) continue;
  const [country, , zone] = line.split("\t");
  if (country && zone) zoneCountry[zone] = country;
}

// 예전 이름 → 현재 이름 → 국가. zone.tab에 직접 있는 이름이 항상 우선.
const backward = await (await fetch(TZ_BACKWARD_URL)).text();
let aliases = 0;
for (const line of backward.split("\n")) {
  const [kind, target, alias] = line.split("#")[0].trim().split(/\s+/);
  if (kind !== "Link" || !target || !alias || zoneCountry[alias]) continue;
  const country = zoneCountry[target];
  if (country) {
    zoneCountry[alias] = country;
    aliases += 1;
  }
}

const sorted = (obj) => Object.fromEntries(Object.entries(obj).sort(([a], [b]) => a.localeCompare(b)));
const out = `// 자동 생성 파일 — 직접 고치지 말고 \`node scripts/gen-region-data.mjs\`를 다시 실행한다.
// 출처: Unicode CLDR ${cldr.supplemental.version._cldrVersion} currencyData, IANA tz zone.tab + backward

/** ISO 3166 국가 코드 → 현재 통용 ISO 4217 통화. */
export const COUNTRY_CURRENCY: Record<string, string> = ${JSON.stringify(sorted(countryCurrency))};

/** IANA 시간대 → ISO 3166 국가 코드. */
export const ZONE_COUNTRY: Record<string, string> = ${JSON.stringify(sorted(zoneCountry))};
`;
writeFileSync(join(root, "src/lib/regionData.ts"), out);
console.log(`countries=${Object.keys(countryCurrency).length} zones=${Object.keys(zoneCountry).length} (aliases ${aliases})`);
