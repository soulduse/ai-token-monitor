import { invoke } from "@tauri-apps/api/core";
import { supabase } from "./supabase";

interface HydratedRow {
  date: string;
  total_tokens: number;
  cost_usd: number;
  messages: number;
  sessions: number;
}

const PAGE_SIZE = 1000;

/**
 * Fetch the signed-in user's daily aggregates from Supabase (`daily_snapshots`)
 * and hand them to the Rust side, which merges them into provider stats for
 * dates that have no local data ("restore my history on a new machine").
 *
 * Returns the number of daily rows restored.
 */
export async function restoreServerHistory(userId: string): Promise<number> {
  if (!supabase) return 0;

  const providers: Record<string, HydratedRow[]> = {};
  let total = 0;

  // Advance by however many rows the server actually returned: PostgREST may cap
  // a page below PAGE_SIZE (max_rows), so only an empty page means "done".
  for (let from = 0; ; ) {
    const { data, error } = await supabase
      .from("daily_snapshots")
      .select("provider,date,total_tokens,cost_usd,messages,sessions")
      .eq("user_id", userId)
      .order("date", { ascending: true })
      .order("provider", { ascending: true })
      .range(from, from + PAGE_SIZE - 1);
    if (error) throw error;
    if (!data || data.length === 0) break;

    for (const r of data) {
      (providers[r.provider] ??= []).push({
        date: r.date,
        total_tokens: Number(r.total_tokens),
        cost_usd: Number(r.cost_usd),
        messages: r.messages ?? 0,
        sessions: r.sessions ?? 0,
      });
      total++;
    }
    from += data.length;
  }

  await invoke("set_server_history", {
    store: { user_id: userId, providers },
  });
  return total;
}

/** Drop restored data (called on sign-out so another account never sees it). */
export async function clearServerHistory(): Promise<void> {
  await invoke("clear_server_history");
}
