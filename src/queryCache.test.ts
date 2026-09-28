import { describe, expect, it, vi } from "vitest";
import {
  QueryCache,
  queryKey,
  stableStringify,
} from "./queryCache";

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("stableStringify", () => {
  it("ignores object key order", () => {
    expect(stableStringify({ b: 2, a: 1 })).toBe(stableStringify({ a: 1, b: 2 }));
  });

  it("handles nesting, arrays and primitives", () => {
    expect(stableStringify({ q: ["x", 1], n: null })).toBe(
      '{"n":null,"q":["x",1]}',
    );
    expect(stableStringify("a|b")).toBe('"a|b"');
    expect(stableStringify(undefined)).toBe("undefined");
  });

  it("treats undefined fields like absent ones (both mean backend None)", () => {
    expect(queryKey("list", { a: 1, b: undefined })).toBe(
      queryKey("list", { a: 1 }),
    );
  });
});

describe("queryKey", () => {
  it("joins segments and keeps prefixes matchable", () => {
    const head = queryKey("articles");
    const k = queryKey("articles", "ranked", { search: "fed" });
    // Prefixes are built through queryKey too, so quoting is consistent;
    // segment matching itself is locked by the invalidate test below.
    expect(k.startsWith(`${head}|`)).toBe(true);
    expect(k.startsWith(`${queryKey("articles2")}|`)).toBe(false);
  });
});

describe("QueryCache.fetch", () => {
  it("caches successes and serves fresh ones without refetching", async () => {
    const now = 1000;
    const cache = new QueryCache({ now: () => now });
    const fetcher = vi.fn(async () => "v1");
    await expect(cache.fetch("k", fetcher, { staleTime: 60_000 })).resolves.toBe("v1");
    await expect(cache.fetch("k", fetcher, { staleTime: 60_000 })).resolves.toBe("v1");
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(cache.peek("k")).toBe("v1");
  });

  it("refetches once the entry goes stale", async () => {
    let now = 0;
    const cache = new QueryCache({ now: () => now });
    const fetcher = vi.fn(async () => "v");
    await cache.fetch("k", fetcher, { staleTime: 100 });
    now = 50;
    await cache.fetch("k", fetcher, { staleTime: 100 });
    expect(fetcher).toHaveBeenCalledTimes(1);
    now = 101;
    await cache.fetch("k", fetcher, { staleTime: 100 });
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("always revalidates with the default staleTime of 0", async () => {
    const cache = new QueryCache();
    const fetcher = vi.fn(async () => 1);
    await cache.fetch("k", fetcher);
    await cache.fetch("k", fetcher);
    expect(fetcher).toHaveBeenCalledTimes(2);
  });

  it("dedups concurrent flights into one fetcher call", async () => {
    const cache = new QueryCache();
    const gate = deferred<string>();
    const fetcher = vi.fn(() => gate.promise);
    const p1 = cache.fetch("k", fetcher);
    const p2 = cache.fetch("k", fetcher);
    gate.resolve("shared");
    await expect(p1).resolves.toBe("shared");
    await expect(p2).resolves.toBe("shared");
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("force skips a fresh entry but still stores and notifies", async () => {
    const cache = new QueryCache();
    const seen: string[] = [];
    cache.subscribe("k", () => seen.push("settled"));
    await cache.fetch("k", async () => "v1", { staleTime: 60_000 });
    await cache.fetch("k", async () => "v2", { staleTime: 60_000, force: true });
    expect(cache.peek("k")).toBe("v2");
    expect(seen).toEqual(["settled", "settled"]);
  });

  it("stores the error, rethrows, and keeps last good data peekable", async () => {
    const cache = new QueryCache();
    await cache.fetch("k", async () => "good");
    await expect(cache.fetch("k", async () => {
      throw new Error("boom");
    })).rejects.toThrow("boom");
    // No poison: the previous success is still what subscribers paint.
    expect(cache.peek("k")).toBe("good");
    expect(String(cache.peekError("k"))).toContain("boom");
    // The failed attempt did not refresh updatedAt → next fetch retries.
    const fetcher = vi.fn(async () => "recovered");
    await expect(cache.fetch("k", fetcher)).resolves.toBe("recovered");
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(cache.peekError("k")).toBeUndefined();
  });

  it("a first-attempt failure leaves nothing peekable", async () => {
    const cache = new QueryCache();
    await expect(cache.fetch("k", async () => {
      throw new Error("nope");
    })).rejects.toThrow("nope");
    expect(cache.peek("k")).toBeUndefined();
  });
});

describe("QueryCache.subscribe", () => {
  it("notifies on settle and stops after unsubscribe", async () => {
    const cache = new QueryCache();
    const reasons: string[] = [];
    const unsub = cache.subscribe("k", (r) => reasons.push(r));
    await cache.fetch("k", async () => 1);
    unsub();
    await cache.fetch("k", async () => 2, { force: true });
    expect(reasons).toEqual(["settled"]);
  });

  it("a throwing subscriber does not break the cache", async () => {
    const cache = new QueryCache();
    cache.subscribe("k", () => {
      throw new Error("subscriber bug");
    });
    await expect(cache.fetch("k", async () => "ok")).resolves.toBe("ok");
    expect(cache.peek("k")).toBe("ok");
  });
});

describe("QueryCache.invalidate", () => {
  it("matches by segment, not by string prefix", () => {
    const cache = new QueryCache();
    const notified: string[] = [];
    for (const k of ["articles", "articles|x", "articles2", "memory"]) {
      cache.subscribe(k, () => notified.push(k));
    }
    const n = cache.invalidate("articles");
    expect(n).toBe(2);
    expect(notified.sort()).toEqual(["articles", "articles|x"]);
  });

  it("deletes entries so the next fetch goes to the backend", async () => {
    const cache = new QueryCache();
    const fetcher = vi.fn(async () => "v");
    await cache.fetch("a|1", fetcher, { staleTime: 60_000 });
    expect(fetcher).toHaveBeenCalledTimes(1);
    cache.invalidate("a");
    await cache.fetch("a|1", fetcher, { staleTime: 60_000 });
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(cache.peek("a|1")).toBe("v");
  });

  it("notifies listener-only keys with no entry yet", () => {
    const cache = new QueryCache();
    let fired = 0;
    cache.subscribe("m|x", () => fired++);
    cache.invalidate("m");
    expect(fired).toBe(1);
  });
});

describe("QueryCache cap", () => {
  it("evicts the least-recently-settled entry beyond maxEntries", async () => {
    const cache = new QueryCache({ maxEntries: 2 });
    await cache.fetch("a", async () => 1);
    await cache.fetch("b", async () => 2);
    await cache.fetch("c", async () => 3);
    expect(cache.size).toBe(2);
    expect(cache.peek("a")).toBeUndefined();
    expect(cache.peek("b")).toBe(2);
    expect(cache.peek("c")).toBe(3);
  });
});
