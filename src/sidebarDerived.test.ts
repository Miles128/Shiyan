import { describe, expect, it } from "vitest";
import type { FeedSource } from "./api/types";
import { groupSidebarFeeds } from "./sidebarDerived";

function feed(
  id: string,
  opts: {
    name?: string;
    category?: string;
    enabled?: boolean;
    origin?: string;
  } = {},
): FeedSource {
  return {
    id,
    name: opts.name ?? id,
    category: opts.category ?? "tech",
    url: `https://example.com/${id}`,
    enabled: opts.enabled ?? true,
    origin: opts.origin ?? "curated",
    description: "",
    etag: "",
    last_fetched_at: null,
    last_new_article_at: null,
    fulltext_ratio: -1,
    priority: 0,
  };
}

describe("groupSidebarFeeds", () => {
  it("sorts within a category by article count desc, enabled first", () => {
    const groups = groupSidebarFeeds(
      [
        feed("few", { name: "Few Articles" }),
        feed("many", { name: "Many Articles" }),
        feed("muted-many", { name: "Muted Many", enabled: false }),
        feed("muted-more", { name: "Muted More", enabled: false }),
      ],
      [],
      { "Few Articles": 3, "Many Articles": 30, "Muted Many": 20, "Muted More": 40 },
    );
    expect(groups).toHaveLength(1);
    const names = groups[0].feeds.map((f) => f.name);
    // Enabled block is count-sorted; the whole disabled block sinks below it.
    expect(names).toEqual(["Many Articles", "Few Articles", "Muted More", "Muted Many"]);
  });

  it("hides curated sources with zero articles but keeps user ones", () => {
    const groups = groupSidebarFeeds(
      [
        feed("empty-curated", { name: "Empty Curated" }),
        feed("empty-user", { name: "Empty User", origin: "user" }),
        feed("has", { name: "Has Some" }),
      ],
      [],
      { "Has Some": 5 },
    );
    const names = groups[0].feeds.map((f) => f.name);
    expect(names).toEqual(["Has Some", "Empty User"]);
    // The exempted user feed still shows its honest zero count for the badge.
    expect(groups[0].feeds[1].articleCount).toBe(0);
  });

  it("drops a category whose sources are all hidden", () => {
    const groups = groupSidebarFeeds(
      [feed("a", { category: "world", name: "A" }), feed("b", { category: "tech", name: "B" })],
      [],
      { B: 2 },
    );
    expect(groups.map((g) => g.cat)).toEqual(["tech"]);
  });

  it("orders builtin categories first and unknown categories last", () => {
    const groups = groupSidebarFeeds(
      [
        feed("w", { category: "world", name: "W" }),
        feed("t", { category: "tech", name: "T" }),
        feed("x", { category: "custom", name: "X" }),
      ],
      [{ id: "custom", label: "自订" }],
      { W: 1, T: 1, X: 1 },
    );
    expect(groups.map((g) => g.cat)).toEqual(["tech", "world", "custom"]);
    expect(groups[2].label).toBe("自订");
  });

  it("keeps input order for equal counts (drag priority survives ties)", () => {
    const groups = groupSidebarFeeds(
      [feed("first", { name: "First" }), feed("second", { name: "Second" })],
      [],
      { First: 7, Second: 7 },
    );
    expect(groups[0].feeds.map((f) => f.name)).toEqual(["First", "Second"]);
  });
});
