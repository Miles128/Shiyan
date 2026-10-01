import { typedInvoke } from "./invoke";

export const apiFeeds = {
  listFeeds: () => typedInvoke("list_feeds"),
  setFeedEnabled: (id: string, enabled: boolean) =>
    typedInvoke("set_feed_enabled", { id, enabled }),
  /** Persist the sidebar drag order (top-to-bottom feed ids). Drives home ranking. */
  reorderFeeds: (orderedIds: string[]) =>
    typedInvoke("reorder_feeds", { orderedIds }),
  /** Delete a user-subscribed feed; curated feeds are disable-only. */
  deleteFeedSource: (id: string) => typedInvoke("delete_feed_source", { id }),
  listFeedCategories: () => typedInvoke("list_feed_categories"),
  /** source name → article count; drives the sidebar's count sort + hiding. */
  listSourceArticleCounts: () => typedInvoke("list_source_article_counts"),
  addFeedCategory: (label: string) =>
    typedInvoke("add_feed_category", { label }),
  discoverFeeds: (categoryId: string) =>
    typedInvoke("discover_feeds", { categoryId }),
  validateFeed: (url: string) => typedInvoke("validate_feed", { url }),
  subscribeFeed: (input: {
    name: string;
    category: string;
    url: string;
    description?: string;
  }) =>
    typedInvoke("subscribe_feed", {
      input: {
        name: input.name,
        category: input.category,
        url: input.url,
        description: input.description ?? null,
      },
    }),
  refreshFeeds: () => typedInvoke("refresh_feeds"),
  cancelRefresh: () => typedInvoke("cancel_refresh"),
};
