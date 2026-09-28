import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import { api } from "./api";
import { shouldRecordOpen } from "./learningStats";
import { useQuery } from "./query";

// 文章域：加载 hook + 上次阅读/滚动位置记忆（原 lastArticle.ts）。

const LAST_KEY = "shiyan:last-article";
const SCROLL_PREFIX = "shiyan:scroll:";

/** In-memory fallback: keeps functions total in tests / non-DOM contexts. */
const storageMemory = new Map<string, string>();

function backingStorage(kind: "local" | "session"): Storage | null {
  try {
    const s = kind === "local" ? window.localStorage : window.sessionStorage;
    return s ?? null;
  } catch {
    return null;
  }
}

function readStored(kind: "local" | "session", key: string): string | null {
  const s = backingStorage(kind);
  return s ? s.getItem(key) : (storageMemory.get(`${kind}:${key}`) ?? null);
}

function writeStored(kind: "local" | "session", key: string, value: string) {
  const s = backingStorage(kind);
  if (s) {
    s.setItem(key, value);
  } else {
    storageMemory.set(`${kind}:${key}`, value);
  }
}

/** Test helper: clear the in-memory fallback. */
export function resetLastArticleMemory() {
  storageMemory.clear();
}

export function rememberLastArticle(id: string) {
  if (!id) return;
  writeStored("local", LAST_KEY, id);
}

export function getLastArticleId(): string | null {
  const id = readStored("local", LAST_KEY);
  return id && id.length > 0 ? id : null;
}

/** Router path for resuming the last article, or null when there is none. */
export function lastArticlePath(): string | null {
  const id = getLastArticleId();
  return id ? `/article/${id}` : null;
}

export function saveScroll(id: string, y: number) {
  if (!id) return;
  writeStored("session", `${SCROLL_PREFIX}${id}`, String(Math.max(0, Math.round(y))));
}

export function loadScroll(id: string): number | null {
  if (!id) return null;
  const raw = readStored("session", `${SCROLL_PREFIX}${id}`);
  if (raw == null) return null;
  const y = Number(raw);
  return Number.isFinite(y) ? y : null;
}

export type ArticleViewState = "loading" | "missing" | "error" | "ready";

/** Derive Reader empty-states. A loaded article stays `ready` even if a later action fails. */
export function articleViewState(input: {
  loading: boolean;
  article: { id: string } | null;
  error: string | null;
}): ArticleViewState {
  if (input.article) return "ready";
  if (input.loading) return "loading";
  if (input.error) return "error";
  return "missing";
}

export function translationsMap(
  rows: { scope_key: string; translated_text: string }[],
): Record<string, string> {
  const map: Record<string, string> = {};
  for (const r of rows) {
    map[r.scope_key] = r.translated_text;
  }
  return map;
}

export function useArticle(id: string | undefined) {
  // Replace-model read: remounts (Reader → Home → Reader) paint the cached
  // view instantly and revalidate in the background. Stale-response guarding
  // now lives in useQuery's run generation — no local seq needed.
  const viewQuery = useQuery(
    id ? ["article", id] : null,
    () => api.getArticleView(id ?? ""),
  );
  const serverView = viewQuery.data ?? null;
  const article = serverView?.article ?? null;
  const paragraphs = serverView?.paragraphs ?? [];

  // Translation overlay: Reader merges paragraph/full-text translations into
  // the server map via functional updates. The overlay always holds the FULL
  // merged map and wins over the base, so a background revalidation can only
  // add server rows, never clobber local ones. Reset per article.
  const [overlay, setOverlay] = useState<Record<string, string>>({});
  const [actionError, setActionError] = useState<string | null>(null);
  const [prevId, setPrevId] = useState(id);
  if (prevId !== id) {
    setPrevId(id);
    setOverlay({});
    setActionError(null);
  }
  const viewRef = useRef(serverView);
  // Effect-synced (never written during render): setTranslations only runs
  // from event handlers and the translate-progress listener, all post-commit,
  // so the ref is always fresh where it is read.
  useEffect(() => {
    viewRef.current = serverView;
  }, [serverView]);

  const baseTranslations = useMemo(
    () => translationsMap(serverView?.translations ?? []),
    [serverView],
  );
  const translations = useMemo(
    () => ({ ...baseTranslations, ...overlay }),
    [baseTranslations, overlay],
  );

  const setTranslations: Dispatch<SetStateAction<Record<string, string>>> =
    useCallback((action) => {
      setOverlay((prev) => {
        const merged = {
          ...translationsMap(viewRef.current?.translations ?? []),
          ...prev,
        };
        return typeof action === "function" ? action(merged) : action;
      });
    }, []);

  // Load errors surface only when there is no article to show (the
  // articleViewState contract — a loaded article stays `ready` even if a
  // later action failed); action errors always surface.
  const loadError = viewQuery.error == null ? null : String(viewQuery.error);
  const error = actionError ?? (article ? null : loadError);

  const recordedId = useRef<string | undefined>(undefined);
  useEffect(() => {
    const openId = shouldRecordOpen(recordedId.current, article);
    if (!openId) return;
    recordedId.current = openId;
    void api.markArticleOpened(openId).catch(() => {
      recordedId.current = undefined;
    });
  }, [article]);

  const view = articleViewState({
    loading: viewQuery.isLoading,
    article,
    error,
  });
  return {
    article,
    paragraphs,
    translations,
    setTranslations,
    error,
    setError: setActionError,
    loading: viewQuery.isLoading,
    view,
  };
}
