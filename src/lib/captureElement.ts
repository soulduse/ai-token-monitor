import { Image } from "@tauri-apps/api/image";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import html2canvas from "html2canvas";

async function renderElement(element: HTMLElement): Promise<HTMLCanvasElement> {
  await document.fonts?.ready;

  const rect = element.getBoundingClientRect();
  const contentWidth = Math.ceil(Math.max(rect.width, element.scrollWidth));
  const contentHeight = Math.ceil(Math.max(rect.height, element.scrollHeight));
  const exportPadding = 16;
  const width = contentWidth + exportPadding * 2;
  const height = contentHeight + exportPadding * 2;
  const backgroundColor = getComputedStyle(document.documentElement)
    .getPropertyValue("--bg-primary")
    .trim() || null;

  return html2canvas(element, {
    backgroundColor,
    scale: 2,
    useCORS: true,
    logging: false,
    width,
    height,
    windowWidth: Math.max(document.documentElement.clientWidth, width),
    windowHeight: Math.max(document.documentElement.clientHeight, height),
    onclone: (_document, clonedElement) => {
      // Give exported cards breathing room without changing the in-app layout.
      clonedElement.style.boxSizing = "border-box";
      clonedElement.style.width = `${width}px`;
      clonedElement.style.padding = `${exportPadding}px`;
      clonedElement.style.background = backgroundColor ?? "transparent";

      // Freeze charts and bars at their current visual state so transitions do
      // not leave a partially animated frame in the exported image.
      clonedElement.querySelectorAll<HTMLElement>("*").forEach((child) => {
        child.style.animation = "none";
        child.style.transition = "none";
      });
    },
  });
}

function canvasToPngBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error("Failed to encode capture as PNG"));
    }, "image/png");
  });
}

export async function copyElementAsImage(element: HTMLElement): Promise<void> {
  const canvas = await renderElement(element);
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) throw new Error("Failed to read capture canvas");

  const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
  const image = await Image.new(pixels.data, canvas.width, canvas.height);

  try {
    // The clipboard plugin handles the platform-specific bitmap conversion on
    // both macOS and Windows. Passing a Tauri Image also avoids moving a large
    // PNG byte array through a custom command.
    await writeImage(image);
  } finally {
    await image.close();
  }
}

export async function saveElementAsPng(
  element: HTMLElement,
  defaultName: string,
): Promise<boolean> {
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: "PNG Image", extensions: ["png"] }],
  });
  if (!path) return false;

  const canvas = await renderElement(element);
  const blob = await canvasToPngBlob(canvas);
  const pngData = Array.from(new Uint8Array(await blob.arrayBuffer()));
  await invoke("save_png_to_file", { pngData, path });
  return true;
}
