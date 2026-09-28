import { useCallback, useEffect, useRef, useState } from "react";
import {
  queryCache as defaultCache,
  queryKey,
  type FetchOptions,
  type QueryCache,
  type QueryKeyInput,
} from "./queryCache";

export { queryKey };

// Query-domain React binding over ./queryCache.ts. Pages either use the
// `useQuery` hook (replace-model reads: article, memory lists, stats) or the
// imperative `peekQuery`/`fetchQuery` pair (Home's append-pagination keeps its
// own state machine but paints from the same cache). Mutations stay direct
// `api.*` calls and converge only on `invalidateQueries`.

function normalizeKey(key: QueryKeyInput | string): string {
  return typeof key === "string" ? key : queryKey(...key);
}

/** Sync read of last good data (stale allowed). Never fetches. */
export function peekQuery<T>(
  key: QueryKeyInput | string,
  cache: QueryCache = defaultCache,
): T | undefined {
  return cache.peek<T>(normalizeKey(key));
}

/**
 * Fresh-or-fetch through the shared cache (deduped, stored, subscribers
 * notified). With the default staleTime of 0 it always revalidates —
 * callers paint `peekQuery` first for the instant stale paint.
 */
export function fetchQuery<T>(
  key: QueryKeyInput | string,
  fetcher: () => Promise<T>,
  opts: FetchOptions = {},
  cache: QueryCache = defaultCache,
): Promise<T> {
  return cache.fetch<T>(normalizeKey(key), fetcher, opts);
}

/**
 * Drop every entry under a key prefix (segment-aware, see
 * `QueryCache.invalidate`) and reload mounted subscribers. No subscribers +
 * no entries = harmless no-op.
 */
export function invalidateQueries(
  key: QueryKeyInput | string,
  cache: QueryCache = defaultCache,
): number {
  return cache.invalidate(normalizeKey(key));
}

export type UseQueryOptions = {
  /** ms a result stays fresh; default 0 = revalidate on every mount. */
  staleTime?: number;
  /** false = disabled: no fetch, data reads undefined. */
  enabled?: boolean;
};

export type UseQueryResult<T> = {
  data: T | undefined;
  error: unknown;
  /** First load in flight with nothing cached to show. */
  isLoading: boolean;
  /** Any fetch in flight (initial or background revalidation). */
  isValidating: boolean;
  /** Reload now, skipping the freshness read. */
  refetch: () => Promise<void>;
};

/**
 * Replace-model read with stale-while-revalidate: mounts paint the cached
 * value instantly (when present) and revalidate in the background unless it
 * is still fresh. `null` key = disabled. Invalidation while mounted drops to
 * empty and reloads automatically.
 */
export function useQuery<T>(
  key: QueryKeyInput | null,
  fetcher: () => Promise<T>,
  opts: UseQueryOptions = {},
  cache: QueryCache = defaultCache,
): UseQueryResult<T> {
  const { staleTime = 0, enabled = true } = opts;
  const keyStr = key === null ? null : queryKey(...key);
  // Latest-fetcher ref: `run` must not depend on the (per-render new)
  // fetcher closure, or the fetch effect would loop. Synced in an effect
  // declared before the fetch effect, so `run` always sees it fresh; event
  // handlers (refetch) only run post-commit.
  const fetcherRef = useRef(fetcher);
  useEffect(() => {
    fetcherRef.current = fetcher;
  });

  const readSnapshot = useCallback((): {
    data: T | undefined;
    error: unknown;
  } => {
    if (keyStr === null || !enabled) return { data: undefined, error: undefined };
    return { data: cache.peek<T>(keyStr), error: cache.peekError(keyStr) };
  }, [cache, keyStr, enabled]);

  const [snap, setSnap] = useState(readSnapshot);
  const [validating, setValidating] = useState(
    () =>
      enabled &&
      keyStr !== null &&
      readSnapshot().data === undefined &&
      readSnapshot().error === undefined,
  );
  // Key (or enabled) changed since last render: resync before paint so a
  // kind/tab/article switch never flashes the previous query's rows — or a
  // spurious "missing" — for a frame. Sanctioned render-phase adjustment,
  // guarded by identity. `validating` is synced too: an empty snapshot for
  // an enabled key means a fetch is (about to be) in flight.
  const identity = `${enabled ? "1" : "0"}:${keyStr ?? ""}`;
  const [prevIdentity, setPrevIdentity] = useState(identity);
  if (prevIdentity !== identity) {
    setPrevIdentity(identity);
    const next = readSnapshot();
    setSnap(next);
    setValidating(
      enabled &&
        keyStr !== null &&
        next.data === undefined &&
        next.error === undefined,
    );
  }

  // Run generation: a stale flight settling must not clear a newer run's
  // `validating` nor paint over a newer key.
  const runId = useRef(0);

  const run = useCallback(
    async (force: boolean): Promise<void> => {
      if (keyStr === null || !enabled) return;
      const my = ++runId.current;
      setValidating(true);
      try {
        await cache.fetch<T>(keyStr, () => fetcherRef.current(), {
          staleTime,
          force,
        });
      } catch {
        // The error lands in snap via peekError below; nothing to show here.
      } finally {
        if (runId.current === my) {
          setSnap(readSnapshot());
          setValidating(false);
        }
      }
    },
    [cache, keyStr, enabled, staleTime, readSnapshot],
  );

  useEffect(() => {
    if (keyStr === null || !enabled) return;
    const unsub = cache.subscribe(keyStr, (reason) => {
      if (reason === "invalidated") {
        // Entry deleted: drop to empty and reload. (The invalidating call
        // itself usually comes from this component's own mutation.)
        setSnap({ data: undefined, error: undefined });
        void run(true);
      } else {
        setSnap(readSnapshot());
      }
    });
    // Paint-then-validate: sync first (a sibling may have populated the
    // entry), then ensure freshness.
    setSnap(readSnapshot());
    void run(false);
    return unsub;
  }, [cache, keyStr, enabled, run, readSnapshot]);

  const refetch = useCallback(() => run(true), [run]);

  const hasResult = snap.data !== undefined || snap.error !== undefined;
  return {
    data: snap.data,
    error: snap.error,
    isLoading: enabled && keyStr !== null && !hasResult && validating,
    isValidating: validating,
    refetch,
  };
}
