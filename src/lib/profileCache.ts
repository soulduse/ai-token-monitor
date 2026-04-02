export type ProfileData = { nickname: string; avatar_url: string | null };

interface CachedProfile extends ProfileData {
  fetchedAt: number;
}

const PROFILE_TTL = 300_000; // 5 minutes

const cache = new Map<string, CachedProfile>();
const inflight = new Map<string, Promise<ProfileData>>();

export function getCachedProfile(uid: string): ProfileData | null {
  const entry = cache.get(uid);
  if (!entry) return null;
  if (Date.now() - entry.fetchedAt > PROFILE_TTL) {
    cache.delete(uid);
    return null;
  }
  return { nickname: entry.nickname, avatar_url: entry.avatar_url };
}

export function setCachedProfile(uid: string, profile: ProfileData): void {
  cache.set(uid, { ...profile, fetchedAt: Date.now() });
}

/** Return all non-expired cached profiles (for mention autocomplete) */
export function getAllCachedProfiles(): Map<string, ProfileData> {
  const result = new Map<string, ProfileData>();
  const now = Date.now();
  for (const [uid, entry] of cache) {
    if (now - entry.fetchedAt <= PROFILE_TTL) {
      result.set(uid, { nickname: entry.nickname, avatar_url: entry.avatar_url });
    }
  }
  return result;
}

/** Deduplicate concurrent fetches for the same uid */
export function getOrFetchProfile(
  uid: string,
  fetcher: () => Promise<ProfileData>,
): Promise<ProfileData> {
  const cached = getCachedProfile(uid);
  if (cached) return Promise.resolve(cached);
  if (inflight.has(uid)) return inflight.get(uid)!;

  const p = fetcher()
    .then((profile) => {
      setCachedProfile(uid, profile);
      return profile;
    })
    .catch(() => {
      return { nickname: "Unknown", avatar_url: null } as ProfileData;
    })
    .finally(() => {
      inflight.delete(uid);
    });
  inflight.set(uid, p);
  return p;
}
