import type { ReactNode } from "react";
import type { ArticleListItem } from "../api";
import type { DifficultyLevel } from "../difficulty";
import ArticleRow from "./ArticleRow";

export type SourceSection = {
  source: string;
  category: string;
  articles: ArticleListItem[];
};

type Props = {
  section: SourceSection;
  /** article id → local difficulty level (computed by the parent). */
  difficultyById: Map<string, DifficultyLevel | null>;
  /** Collapse state is owned by the parent (Home) so 全部折叠 can work. */
  collapsed: boolean;
  onToggleCollapsed: () => void;
  /** Forwarded to ArticleRow: hide tags until the 标签 toggle is on. */
  showTags?: boolean;
  /** Keyboard navigation highlight (article id), forwarded to ArticleRow. */
  highlightedId?: string | null;
  /** Show each row's source. Useful when the board mixes publications. */
  showSource?: boolean;
  badgeFor?: (article: ArticleListItem) => string | undefined;
  /** Rendered above the article list (book picks inside 今日推荐). */
  extra?: ReactNode;
  extraCount?: number;
};

/** One per-source board on the home page: header + article list. */
export default function SourceBoard({
  section,
  difficultyById,
  collapsed,
  onToggleCollapsed,
  showTags,
  highlightedId,
  showSource,
  badgeFor,
  extra,
  extraCount = 0,
}: Props) {
  const countLabel = [
    section.articles.length > 0 ? `${section.articles.length} 篇` : "",
    extraCount > 0 ? `${extraCount} 本` : "",
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <section className="source-board">
      <button
        type="button"
        className="source-board-head"
        aria-expanded={!collapsed}
        onClick={onToggleCollapsed}
      >
        <span className="source-board-chevron" aria-hidden>
          ▾
        </span>
        <h2>{section.source}</h2>
        <span className="muted">{countLabel || "0 篇"}</span>
      </button>
      {collapsed ? null : (
        <>
          {extra}
          {section.articles.length > 0 && (
            <ul className="article-list">
              {section.articles.map((a) => (
                <ArticleRow
                  key={a.id}
                  article={a}
                  difficulty={difficultyById.get(a.id) ?? null}
                  showSource={showSource}
                  showTags={showTags}
                  highlighted={a.id === highlightedId}
                  badge={badgeFor?.(a)}
                />
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}
