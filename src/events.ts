import type { RefreshResult } from "./api";

/**
 * Typed in-app event bus. Replaces ad-hoc `window.dispatchEvent(new CustomEvent(...))`
 * channels so producers/consumers share one payload contract.
 */
export type AppEvents = {
  "shiyan:refreshed": { result?: RefreshResult; error?: string };
};

type Handler<T> = (payload: T) => void;

const registry = new Map<keyof AppEvents, Set<Handler<never>>>();

/** Subscribe to an app event; returns an unsubscribe function. */
export function onEvent<K extends keyof AppEvents>(
  name: K,
  handler: Handler<AppEvents[K]>,
): () => void {
  let set = registry.get(name);
  if (!set) {
    set = new Set();
    registry.set(name, set);
  }
  set.add(handler as Handler<never>);
  return () => {
    set.delete(handler as Handler<never>);
  };
}

/** Publish an app event to all current subscribers. */
export function emitEvent<K extends keyof AppEvents>(
  name: K,
  payload: AppEvents[K],
): void {
  const set = registry.get(name);
  if (!set) return;
  for (const handler of set) {
    try {
      (handler as Handler<AppEvents[K]>)(payload);
    } catch (e) {
      // One bad subscriber must not break the rest (e.g. sidebar sync).
      console.error(`event handler for ${name} failed`, e);
    }
  }
}
