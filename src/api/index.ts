// Backward-compatible facade: `import { api, Article, ... } from "../api"`.
export * from "./types";
export type { AddMemoryInput, MemoryKind } from "./memory";

import { apiArticles } from "./articles";
import { apiBooks } from "./books";
import { apiConfig } from "./config";
import { apiData } from "./data";
import { apiFeeds } from "./feeds";
import { apiMemory } from "./memory";

export const api = {
  ...apiConfig,
  ...apiFeeds,
  ...apiArticles,
  ...apiBooks,
  ...apiMemory,
  ...apiData,
};
