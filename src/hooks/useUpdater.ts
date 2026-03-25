import { useState, useEffect, useRef, useCallback } from "react";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdaterState {
  updateAvailable: boolean;
  version: string;
  downloading: boolean;
  downloaded: boolean;
  progress: number;
  error: string | null;
  download: () => void;
  install: () => void;
  dismiss: () => void;
}

export function useUpdater(): UpdaterState {
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [version, setVersion] = useState("");
  const [downloading, setDownloading] = useState(false);
  const [downloaded, setDownloaded] = useState(false);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const updateRef = useRef<Update | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function checkForUpdate() {
      try {
        const update = await check();
        if (cancelled) return;
        if (update) {
          updateRef.current = update;
          setVersion(update.version);
          setUpdateAvailable(true);
        }
      } catch (e) {
        // Silently ignore update check failures (e.g. no network, no latest.json)
        console.warn("[updater] check failed:", e);
      }
    }

    checkForUpdate();

    return () => {
      cancelled = true;
    };
  }, []);

  const download = useCallback(async () => {
    const update = updateRef.current;
    if (!update || downloading) return;

    setDownloading(true);
    setError(null);
    setProgress(0);

    let contentLength = 0;
    let downloaded = 0;

    try {
      // downloadAndInstall() downloads AND installs the update in one call.
      // After it resolves, the user clicks "Restart" which calls relaunch().
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          contentLength = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (contentLength > 0) {
            setProgress(Math.round((downloaded / contentLength) * 100));
          }
        } else if (event.event === "Finished") {
          setProgress(100);
        }
      });
      setDownloaded(true);
      setDownloading(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setDownloading(false);
    }
  }, [downloading]);

  const install = useCallback(async () => {
    await relaunch();
  }, []);

  const dismiss = useCallback(() => {
    setDismissed(true);
    setError(null);
  }, []);

  return {
    updateAvailable: updateAvailable && !dismissed,
    version,
    downloading,
    downloaded,
    progress,
    error,
    download,
    install,
    dismiss,
  };
}
