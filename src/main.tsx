import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import "./styles/global.css";

// Dev-only browser QA harness: mocks the Tauri IPC layer so the UI renders
// with fixture data outside the Tauri shell. Statically eliminated from
// release builds via the `import.meta.env.DEV` guard.
if (import.meta.env.DEV && new URLSearchParams(window.location.search).get("mockTauri") === "1") {
  const { installMockTauri } = await import("./dev/mockTauri");
  installMockTauri();
}

// Disable default browser context menu so custom menus work
document.addEventListener("contextmenu", (e) => e.preventDefault());

// ---------------------------------------------------------------------------
// Everything below lives at module level, OUTSIDE React, so it keeps working
// even if the React tree crashes and unmounts.
// ---------------------------------------------------------------------------

// Webview liveness heartbeat: the Rust side shows the window, waits briefly,
// and reloads the webview if no beat arrived (dead WKWebView content process
// renders an uncloseable blank white popover). Beat on an interval and
// immediately on every popover open (visibilitychange→visible).
const sendHeartbeat = () => {
  invoke("heartbeat").catch(() => {});
};
sendHeartbeat();
setInterval(sendHeartbeat, 15_000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") sendHeartbeat();
});
// The Rust watchdog eval()s this right after showing the window: an alive
// page answers instantly, a dead content process silently drops the eval.
// Direct ping, because visibilitychange is not guaranteed to fire across
// native NSWindow orderOut/orderFrontRegardless transitions.
(window as unknown as { __heartbeat: () => void }).__heartbeat = sendHeartbeat;

// Escape → hide window. Bubble phase on window: modal/overlay handlers use
// capture phase on document and call preventDefault() to keep the window open
// while closing only themselves.
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !e.defaultPrevented) {
    invoke("hide_window").catch(() => {});
  }
});

// Crash fallback: when an uncaught error unmounts the React root, show a
// minimal vanilla-DOM card so the user can reload or close instead of being
// stuck on a blank window.
function showCrashFallback() {
  const root = document.getElementById("root");
  if (!root || root.childElementCount > 0) return; // React tree still alive
  if (document.getElementById("crash-fallback")) return;

  const ko = navigator.language.startsWith("ko");
  const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;

  const overlay = document.createElement("div");
  overlay.id = "crash-fallback";
  overlay.style.cssText = [
    "position:fixed", "inset:0", "display:flex", "align-items:center",
    "justify-content:center", "padding:24px",
    `background:${dark ? "#0d1117" : "#ffffff"}`,
    `color:${dark ? "#e6edf3" : "#1f2328"}`,
    "font-family:-apple-system,BlinkMacSystemFont,sans-serif",
  ].join(";");

  const card = document.createElement("div");
  card.style.cssText = "max-width:300px;text-align:center;display:flex;flex-direction:column;gap:12px;";

  const title = document.createElement("div");
  title.style.cssText = "font-size:14px;font-weight:700;";
  title.textContent = ko ? "문제가 발생했어요" : "Something went wrong";

  const body = document.createElement("div");
  body.style.cssText = `font-size:12px;line-height:1.5;color:${dark ? "#9ca3af" : "#656d76"};`;
  body.textContent = ko
    ? "화면을 그리는 중 오류가 발생했습니다. 새로고침하면 대부분 해결됩니다."
    : "An error occurred while rendering. Reloading usually fixes it.";

  const row = document.createElement("div");
  row.style.cssText = "display:flex;gap:8px;justify-content:center;";

  const reloadBtn = document.createElement("button");
  reloadBtn.textContent = ko ? "새로고침" : "Reload";
  reloadBtn.style.cssText = "padding:8px 16px;font-size:12px;font-weight:700;border:none;border-radius:8px;cursor:pointer;background:#2da44e;color:#fff;";
  reloadBtn.addEventListener("click", () => location.reload());

  const closeBtn = document.createElement("button");
  closeBtn.textContent = ko ? "닫기" : "Close";
  closeBtn.style.cssText = `padding:8px 16px;font-size:12px;font-weight:700;border:none;border-radius:8px;cursor:pointer;background:transparent;color:${dark ? "#9ca3af" : "#656d76"};`;
  closeBtn.addEventListener("click", () => invoke("hide_window").catch(() => {}));

  row.append(reloadBtn, closeBtn);
  card.append(title, body, row);
  overlay.append(card);
  document.body.appendChild(overlay);
}

window.addEventListener("error", () => setTimeout(showCrashFallback, 50));
window.addEventListener("unhandledrejection", () => setTimeout(showCrashFallback, 50));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
