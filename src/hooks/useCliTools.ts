import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CliTool } from "../lib/translationMode";

/** Translation CLIs found on this machine, probed once per mount. */
export function useCliTools(): CliTool[] {
  const [tools, setTools] = useState<CliTool[]>([]);
  useEffect(() => {
    let active = true;
    invoke<CliTool[]>("detect_cli_tools")
      .then((detected) => {
        if (active) setTools(detected);
      })
      .catch(() => {
        if (active) setTools([]);
      });
    return () => {
      active = false;
    };
  }, []);
  return tools;
}
