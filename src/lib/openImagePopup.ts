import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

let popupCounter = 0;

/**
 * Open an image in a separate popup window using Tauri WebviewWindow.
 * Falls back to opening in external browser if Tauri API is unavailable.
 */
export async function openImagePopup(imageUrl: string): Promise<void> {
  try {
    const label = `image_popup_${++popupCounter}`;
    const popup = new WebviewWindow(label, {
      url: imageUrl,
      title: "Image Preview",
      width: 800,
      height: 600,
      center: true,
      resizable: true,
      decorations: true,
    });

    popup.once("tauri://error", async () => {
      // Fallback to external browser
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      openUrl(imageUrl);
    });
  } catch {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    openUrl(imageUrl);
  }
}
