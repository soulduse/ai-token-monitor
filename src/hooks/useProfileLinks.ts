import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { supabase } from "../lib/supabase";

export interface ProfileLink {
  url: string;
  title: string | null;
  favicon_url: string | null;
}

const MAX_LINKS = 3;

export function useProfileLinks(userId: string | null, isMe: boolean) {
  const [links, setLinks] = useState<ProfileLink[]>([]);
  const [loading, setLoading] = useState(false);
  const [fetching, setFetching] = useState(false);

  // Fetch links from profiles table
  useEffect(() => {
    if (!userId) {
      setLinks([]);
      return;
    }

    let cancelled = false;
    setLoading(true);

    supabase
      .from("profiles")
      .select("links")
      .eq("id", userId)
      .single()
      .then(({ data }) => {
        if (cancelled) return;
        const rawLinks = data?.links;
        if (Array.isArray(rawLinks)) {
          setLinks(rawLinks.slice(0, MAX_LINKS));
        }
        setLoading(false);
      });

    return () => { cancelled = true; };
  }, [userId]);

  // Save links to Supabase
  const saveLinks = useCallback(async (newLinks: ProfileLink[]) => {
    if (!userId || !isMe) return;
    setLinks(newLinks);
    await supabase
      .from("profiles")
      .update({ links: newLinks })
      .eq("id", userId);
  }, [userId, isMe]);

  // Add a link by URL (fetches metadata via Tauri command)
  const addLink = useCallback(async (url: string): Promise<boolean> => {
    if (links.length >= MAX_LINKS) return false;

    setFetching(true);
    try {
      const meta = await invoke<{ url: string; title: string | null; favicon_url: string | null }>(
        "fetch_url_metadata",
        { url }
      );
      const newLink: ProfileLink = {
        url: meta.url,
        title: meta.title,
        favicon_url: meta.favicon_url,
      };
      await saveLinks([...links, newLink]);
      setFetching(false);
      return true;
    } catch {
      setFetching(false);
      return false;
    }
  }, [links, saveLinks]);

  // Remove a link by index
  const removeLink = useCallback(async (index: number) => {
    const newLinks = links.filter((_, i) => i !== index);
    await saveLinks(newLinks);
  }, [links, saveLinks]);

  return { links, loading, fetching, addLink, removeLink, canAdd: isMe && links.length < MAX_LINKS };
}
