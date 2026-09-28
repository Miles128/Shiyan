import { invoke } from "@tauri-apps/api/core";
import type {
  FeedCategory,
  FeedDiscoverCandidate,
  FeedSource,
  FeedValidation,
  RefreshResult,
} from "./types";

export const apiFeeds = {
  listFeeds: () => invoke<FeedSource[]>("list_feeds"),
  setFeedEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_feed_enabled", { id, enabled }),
  /** Persist the sidebar drag order (top-to-bottom feed ids). Drives home ranking. */
  reorderFeeds: (orderedIds: string[]) =>
    invoke<void>("reorder_feeds", { orderedIds }),
  /** Delete a user-subscribed feed; curated feeds are disable-only. */
  deleteFeedSource: (id: string) => invoke<void>("delete_feed_source", { id }),
  listFeedCategories: () => invoke<FeedCategory[]>("list_feed_categories"),
  addFeedCategory: (label: string) =>
    invoke<FeedCategory>("add_feed_category", { label }),
  discoverFeeds: (categoryId: string) =>
    invoke<FeedDiscoverCandidate[]>("discover_feeds", { categoryId }),
  validateFeed: (url: string) => invoke<FeedValidation>("validate_feed", { url }),
  subscribeFeed: (input: {
    name: string;
    category: string;
    url: string;
    description?: string;
  }) =>
    invoke<FeedSource>("subscribe_feed", {
      input: {
        name: input.name,
        category: input.category,
        url: input.url,
        description: input.description ?? null,
      },
    }),
  refreshFeeds: () => invoke<RefreshResult>("refresh_feeds"),
  cancelRefresh: () => invoke<void>("cancel_refresh"),
};