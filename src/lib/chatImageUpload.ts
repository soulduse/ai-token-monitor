import { supabase } from "./supabase";

const MAX_UPLOAD_SIZE = 400 * 1024; // 400KB target after resize
const ALLOWED_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

export interface UploadResult {
  url?: string;
  error?: string;
}

function getExtension(mimeType: string): string {
  const map: Record<string, string> = {
    "image/png": "png",
    "image/jpeg": "jpg",
    "image/gif": "gif",
    "image/webp": "webp",
  };
  return map[mimeType] ?? "png";
}

/**
 * Resize an image blob to fit within MAX_UPLOAD_SIZE (400KB).
 * Uses canvas to progressively reduce quality/dimensions.
 * GIF is passed through without resize (animation would be lost).
 */
async function resizeImage(blob: Blob): Promise<Blob> {
  if (blob.type === "image/gif" || blob.size <= MAX_UPLOAD_SIZE) return blob;

  const img = new Image();
  const url = URL.createObjectURL(blob);

  try {
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve();
      img.onerror = reject;
      img.src = url;
    });

    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d")!;
    let { width, height } = img;

    // Scale down dimensions if very large
    const MAX_DIM = 1200;
    if (width > MAX_DIM || height > MAX_DIM) {
      const ratio = Math.min(MAX_DIM / width, MAX_DIM / height);
      width = Math.round(width * ratio);
      height = Math.round(height * ratio);
    }

    canvas.width = width;
    canvas.height = height;
    ctx.drawImage(img, 0, 0, width, height);

    // Try JPEG at decreasing quality until under target size
    for (const quality of [0.8, 0.6, 0.4, 0.25]) {
      const result = await new Promise<Blob>((resolve) =>
        canvas.toBlob((b) => resolve(b!), "image/jpeg", quality),
      );
      if (result.size <= MAX_UPLOAD_SIZE) return result;
    }

    // Last resort: scale down further
    const scale = Math.sqrt(MAX_UPLOAD_SIZE / blob.size) * 0.8;
    canvas.width = Math.round(width * scale);
    canvas.height = Math.round(height * scale);
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height);

    return await new Promise<Blob>((resolve) =>
      canvas.toBlob((b) => resolve(b!), "image/jpeg", 0.5),
    );
  } finally {
    URL.revokeObjectURL(url);
  }
}

/**
 * Upload an image blob to Supabase Storage (chat-images bucket).
 * Auto-resizes to fit within 400KB before upload.
 * Path: {userId}/{timestamp}_{random}.{ext}
 */
export async function uploadChatImage(blob: Blob, userId: string): Promise<UploadResult> {
  if (!supabase) return { error: "Not available" };

  if (!ALLOWED_TYPES.includes(blob.type)) {
    return { error: "Unsupported image type" };
  }

  const resized = await resizeImage(blob);
  const ext = resized.type === "image/jpeg" ? "jpg" : getExtension(blob.type);
  const rand = crypto.randomUUID();
  const path = `${userId}/${Date.now()}_${rand}.${ext}`;

  const { error } = await supabase.storage
    .from("chat-images")
    .upload(path, resized, { contentType: resized.type });

  if (error) {
    return { error: error.message };
  }

  const { data: urlData } = supabase.storage
    .from("chat-images")
    .getPublicUrl(path);

  return { url: urlData.publicUrl };
}

/**
 * Convert a base64 data URL or raw base64 string to a Blob.
 */
export function base64ToBlob(base64: string, mimeType = "image/png"): Blob {
  // Strip data URL prefix if present
  const raw = base64.includes(",") ? base64.split(",")[1] : base64;
  const bytes = atob(raw);
  const arr = new Uint8Array(bytes.length);
  for (let i = 0; i < bytes.length; i++) {
    arr[i] = bytes.charCodeAt(i);
  }
  return new Blob([arr], { type: mimeType });
}
