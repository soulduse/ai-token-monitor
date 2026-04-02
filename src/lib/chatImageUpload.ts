import { supabase } from "./supabase";

const MAX_IMAGE_SIZE = 2 * 1024 * 1024; // 2MB
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
 * Upload an image blob to Supabase Storage (chat-images bucket).
 * Path: {userId}/{timestamp}_{random}.{ext}
 */
export async function uploadChatImage(blob: Blob, userId: string): Promise<UploadResult> {
  if (!supabase) return { error: "Not available" };

  if (!ALLOWED_TYPES.includes(blob.type)) {
    return { error: "Unsupported image type" };
  }

  if (blob.size > MAX_IMAGE_SIZE) {
    return { error: "Image too large (max 2MB)" };
  }

  const ext = getExtension(blob.type);
  const rand = crypto.randomUUID();
  const path = `${userId}/${Date.now()}_${rand}.${ext}`;

  const { error } = await supabase.storage
    .from("chat-images")
    .upload(path, blob, { contentType: blob.type });

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
