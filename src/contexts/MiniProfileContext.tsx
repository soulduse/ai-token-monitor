import { createContext, useContext, useState, useCallback } from "react";
import type { ReactNode } from "react";

export interface MiniProfileTarget {
  user_id: string;
  nickname: string;
  avatar_url: string | null;
}

interface MiniProfileContextType {
  target: MiniProfileTarget | null;
  open: (target: MiniProfileTarget) => void;
  close: () => void;
}

const MiniProfileContext = createContext<MiniProfileContextType>({
  target: null,
  open: () => {},
  close: () => {},
});

export function MiniProfileProvider({ children }: { children: ReactNode }) {
  const [target, setTarget] = useState<MiniProfileTarget | null>(null);
  const open = useCallback((t: MiniProfileTarget) => setTarget(t), []);
  const close = useCallback(() => setTarget(null), []);

  return (
    <MiniProfileContext.Provider value={{ target, open, close }}>
      {children}
    </MiniProfileContext.Provider>
  );
}

export function useMiniProfile() {
  return useContext(MiniProfileContext);
}
