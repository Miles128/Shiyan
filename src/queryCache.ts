// Query-domain: tiny stale-while-revalidate cache for Tauri command reads.
//
// Pure module (no react, no api imports): safe to import from *.test.ts.
// The React binding lives in ./query.ts; behavior policy (fresh-or-fetch,
// in-flight dedup, prefix invalidation) lives here so it is unit-testable.

/** Cache key segments, e.g. ["articles", "ranked", { search: "fed" }]. */
export type QueryKeyInput = readonly unknown[];

/** JSON with sorted object keys, so `{a:1,b:2}` and `{b:2,a:1}` share a key. */
export function stableStringify(value: unknown): string {
  if (value === null || typeof value !== "object") {
    const s = JSON.stringify(value);
    // JSON.stringify(undefined) → undefined (functions/symbols likewise).
    return s === undefined ? "undefined" : s;
  }
  if (Array.isArray(value)) {
    return `[${value.map((v) => stableStringify(v) ?? "null").join(",")}]`;
  }
  const record = value as Record<string, unknown>;
  const body = Object.keys(record)
    .sort()
    // Mirror JSON: undefined / function / symbol fields are absent, so
    // `{search: undefined}` keys exactly like `{}` (both mean backend None).
    .filter((k) => {
      const v = record[k];
      return (
        v !== undefined && typeof v !== "function" && typeof v !== "symbol"
      );
    })
    .map((k) => `${JSON.stringify(k)}:${stableStringify(record[k])}`)
    .join(",");
  return `{${body}}`;
}

/** Stable string key for a segment list. `[]` → `""` (matches nothing). */
export function queryKey(...parts: QueryKeyInput): string {
  return parts.map(stableStringify).join("|");
}

export type NotifyReason = "settled" | "invalidated";
export type QueryListener = (reason: NotifyReason) => void;

type Entry = {
  data: unknown;
  hasData: boolean;
  error: unknown;
  /** ms epoch of the last SUCCESSFUL fetch (drives staleTime). */
  updatedAt: number;
  inflight: Promise<unknown> | null;
};

export type FetchOptions = {
  /**
   * ms a successful result stays fresh; default 0 = always revalidate.
   * `Infinity` stays fresh until invalidated.
   */
  staleTime?: number;
  /** Skip the freshness read, always fetch (still dedups + stores + notifies). */
  force?: boolean;
};

export type QueryCacheOptions = {
  /** Oldest-settled entries beyond this are evicted (default 100). */
  maxEntries?: number;
  /** Clock override for tests. */
  now?: () => number;
};

export class QueryCache {
  private entries = new Map<string, Entry>();
  private listeners = new Map<string, Set<QueryListener>>();
  private readonly maxEntries: number;
  private readonly now: () => number;

  constructor(opts: QueryCacheOptions = {}) {
    this.maxEntries = opts.maxEntries ?? 100;
    this.now = opts.now ?? (() => Date.now());
  }

  /** Sync read: last good data, even stale. Never triggers a fetch. */
  peek<T>(key: string): T | undefined {
    const e = this.entries.get(key);
    return e?.hasData ? (e.data as T) : undefined;
  }

  /** Last fetch error for a key (cleared on the next success). */
  peekError(key: string): unknown {
    return this.entries.get(key)?.error;
  }

  /**
   * Fresh-or-fetch with in-flight dedup: concurrent callers share one
   * promise (also collapses React StrictMode double-mounts into one invoke).
   * Success stores + notifies "settled"; failure stores the error, notifies,
   * and rethrows — last good data stays peekable, never poisoned.
   */
  fetch<T>(
    key: string,
    fetcher: () => Promise<T>,
    opts: FetchOptions = {},
  ): Promise<T> {
    const staleTime = opts.staleTime ?? 0;
    const existing = this.entries.get(key);
    if (
      !opts.force &&
      existing?.hasData &&
      staleTime > 0 &&
      this.now() - existing.updatedAt <= staleTime
    ) {
      return Promise.resolve(existing.data as T);
    }
    if (existing?.inflight) {
      return existing.inflight as Promise<T>;
    }
    const entry: Entry = existing ?? {
      data: undefined,
      hasData: false,
      error: undefined,
      updatedAt: 0,
      inflight: null,
    };
    // Publish the entry BEFORE the flight starts: the async body invokes
    // fetcher() synchronously, and a concurrent fetch() for the same key
    // must observe `inflight` to share it instead of starting a second one.
    if (!existing) this.entries.set(key, entry);
    const flight = (async (): Promise<T> => {
      try {
        const data = await fetcher();
        entry.data = data;
        entry.hasData = true;
        entry.error = undefined;
        entry.updatedAt = this.now();
        return data;
      } catch (err) {
        entry.error = err;
        throw err;
      } finally {
        entry.inflight = null;
        this.touch(key, entry);
        this.emit(key, "settled");
      }
    })();
    entry.inflight = flight;
    return flight;
  }

  /** Re-run the subscriber callback on settle/invalidate; returns unsub. */
  subscribe(key: string, fn: QueryListener): () => void {
    let set = this.listeners.get(key);
    if (!set) {
      set = new Set();
      this.listeners.set(key, set);
    }
    set.add(fn);
    return () => {
      set.delete(fn);
      if (set.size === 0) this.listeners.delete(key);
    };
  }

  /**
   * Delete every entry under a key prefix and tell subscribers to reload.
   * Segment-aware: `articles` matches `articles|ranked|…` but not
   * `articles2`. Also notifies listener-only keys (subscribed before any
   * fetch settled). Returns the number of keys touched.
   */
  invalidate(prefix: string): number {
    const match = (key: string) =>
      key === prefix || key.startsWith(`${prefix}|`);
    const keys = new Set<string>();
    for (const key of this.entries.keys()) if (match(key)) keys.add(key);
    for (const key of this.listeners.keys()) if (match(key)) keys.add(key);
    for (const key of keys) {
      this.entries.delete(key);
      this.emit(key, "invalidated");
    }
    return keys.size;
  }

  /** Entry count (eviction accounting in tests). */
  get size(): number {
    return this.entries.size;
  }

  /** Drop everything (tests / logout-style resets). */
  clear(): void {
    this.entries.clear();
  }

  /** Refresh LRU order + enforce the cap. */
  private touch(key: string, entry: Entry): void {
    this.entries.delete(key);
    this.entries.set(key, entry);
    while (this.entries.size > this.maxEntries) {
      const oldest = this.entries.keys().next();
      if (oldest.done) break;
      this.entries.delete(oldest.value);
    }
  }

  private emit(key: string, reason: NotifyReason): void {
    const set = this.listeners.get(key);
    if (!set) return;
    for (const fn of [...set]) {
      try {
        fn(reason);
      } catch {
        // A subscriber bug must not break the cache for everyone else.
      }
    }
  }
}

/** App-wide singleton used by ./query.ts. Tests construct their own. */
export const queryCache = new QueryCache();
