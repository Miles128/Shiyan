import type { FeedSource } from "./api/types";

// 侧边栏订阅树的派生逻辑：分类分组、分类内按库内文章数排序、
// 零文章源自动隐藏（用户自订源豁免，刚订阅未抓到文章也可见）。
// 保持纯函数，供 Sidebar / 测试消费。

/** Fixed top-level order for the built-in categories (mirrors the DB seed). */
export const BUILTIN_CATEGORY_ORDER: { id: string; label: string }[] = [
  { id: "tech", label: "科技" },
  { id: "finance", label: "财经" },
  { id: "world", label: "国际" },
  { id: "other", label: "其他" },
];

/** A source row with its library article count attached (for the count badge). */
export type SidebarFeed = FeedSource & { articleCount: number };

export type SidebarGroup = {
  cat: string;
  label: string;
  feeds: SidebarFeed[];
};

/**
 * Group feeds into the sidebar tree. Within a category:
 * enabled first (muting still sinks visibly), then article count desc.
 * Ties keep the incoming order, which `list_feeds` already returns as
 * priority DESC — so the drag-saved order survives as a stable tiebreak.
 * Sources with no articles are hidden, except user-subscribed ones
 * (origin === "user") so a fresh subscription doesn't vanish before its
 * first refresh. Categories left with no visible sources disappear too.
 */
export function groupSidebarFeeds(
  feeds: FeedSource[],
  cats: { id: string; label: string }[],
  counts: Record<string, number>,
): SidebarGroup[] {
  const labelById = new Map<string, string>();
  for (const c of BUILTIN_CATEGORY_ORDER) labelById.set(c.id, c.label);
  for (const c of cats) labelById.set(c.id, c.label);

  const visible = feeds.map((f) => ({ ...f, articleCount: counts[f.name] ?? 0 }))
    .filter((f) => f.origin === "user" || f.articleCount > 0);

  const byCat = new Map<string, SidebarFeed[]>();
  for (const f of visible) {
    const arr = byCat.get(f.category);
    if (arr) arr.push(f);
    else byCat.set(f.category, [f]);
  }

  // Built-ins in seed order, then user-created categories in DB order.
  // Feeds whose category is unknown to the DB still get a group so
  // nothing disappears.
  const order: string[] = [
    ...BUILTIN_CATEGORY_ORDER.map((c) => c.id),
    ...cats
      .filter((c) => !BUILTIN_CATEGORY_ORDER.some((b) => b.id === c.id))
      .map((c) => c.id),
  ];
  for (const cat of byCat.keys()) if (!order.includes(cat)) order.push(cat);

  return order
    .filter((cat) => (byCat.get(cat)?.length ?? 0) > 0)
    .map((cat) => {
      const list = byCat.get(cat)!;
      // Array#sort is stable, so equal keys keep the priority order above.
      const sorted = [...list].sort((a, b) => {
        if (a.enabled !== b.enabled) return a.enabled ? -1 : 1;
        return b.articleCount - a.articleCount;
      });
      return { cat, label: labelById.get(cat) ?? cat, feeds: sorted };
    });
}
