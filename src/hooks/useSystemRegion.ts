import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * OS 지역 설정의 국가 코드("KR" 등). undefined = 아직 모름, null = 읽을 수 없음.
 * 백엔드가 첫 호출 때 한 번 읽고 캐시한다.
 */
export function useSystemRegion(): string | null | undefined {
  const [region, setRegion] = useState<string | null | undefined>(undefined);

  useEffect(() => {
    let alive = true;
    invoke<string | null>("get_system_region")
      .then((r) => {
        if (alive) setRegion(r ?? null);
      })
      .catch(() => {
        if (alive) setRegion(null);
      });
    return () => {
      alive = false;
    };
  }, []);

  return region;
}
