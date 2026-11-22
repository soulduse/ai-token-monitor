import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { FxRates } from "../lib/types";

/** USD 기준 환율. 백엔드가 12시간 캐시하므로 화면을 열 때 한 번만 묻는다. */
export function useFxRates(): FxRates | null {
  const [rates, setRates] = useState<FxRates | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<FxRates | null>("get_fx_rates")
      .then((r) => {
        if (alive) setRates(r);
      })
      .catch(() => {
        /* 환율 없이도 USD로 표시된다 */
      });
    return () => {
      alive = false;
    };
  }, []);

  return rates;
}
