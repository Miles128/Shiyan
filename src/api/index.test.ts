import { describe, expect, it } from "vitest";
import { api } from "./index";
import { apiArticles } from "./articles";
import { apiConfig } from "./config";
import { apiData } from "./data";
import { apiFeeds } from "./feeds";
import { apiMemory } from "./memory";

/**
 * The `api` facade spreads five modules flat: a duplicate key in any two
 * modules would silently shadow one command. Lock the invariant instead of
 * discovering it in production.
 */
describe("api facade", () => {
  it("has no key collisions across modules", () => {
    const seen = new Map<string, string>();
    const collisions: string[] = [];
    const modules = { apiConfig, apiFeeds, apiArticles, apiMemory, apiData };
    for (const [name, mod] of Object.entries(modules)) {
      for (const key of Object.keys(mod)) {
        const prev = seen.get(key);
        if (prev) collisions.push(`${key} in ${prev} and ${name}`);
        else seen.set(key, name);
      }
    }
    expect(collisions).toEqual([]);
  });

  it("exposes every module key on the facade", () => {
    for (const mod of [apiConfig, apiFeeds, apiArticles, apiMemory, apiData]) {
      for (const key of Object.keys(mod)) {
        expect((api as Record<string, unknown>)[key]).toBe(
          (mod as Record<string, unknown>)[key],
        );
      }
    }
  });
});
