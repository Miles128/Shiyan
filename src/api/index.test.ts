import { describe, expect, it } from "vitest";
import { api } from "./index";
import { apiArticles } from "./articles";
import { apiBooks } from "./books";
import { apiConfig } from "./config";
import { apiData } from "./data";
import { apiFeeds } from "./feeds";
import { apiMemory } from "./memory";

/**
 * The `api` facade spreads six modules flat: a duplicate key in any two
 * modules would silently shadow one command, and a module missing from the
 * spread would vanish from the facade at runtime while tsc stays happy.
 * Lock both invariants instead of discovering them in production.
 */
describe("api facade", () => {
  it("has no key collisions across modules and mirrors them all", () => {
    const seen = new Map<string, string>();
    const collisions: string[] = [];
    const modules = { apiConfig, apiFeeds, apiArticles, apiBooks, apiMemory, apiData };
    for (const [name, mod] of Object.entries(modules)) {
      for (const key of Object.keys(mod)) {
        const prev = seen.get(key);
        if (prev) collisions.push(`${key} in ${prev} and ${name}`);
        else seen.set(key, name);
      }
    }
    expect(collisions).toEqual([]);
    // Flat spread of exactly these modules: same key count, no more, no less.
    expect(Object.keys(api).length).toBe(seen.size);
  });
});
