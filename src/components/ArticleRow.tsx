import { Link } from "react-router-dom";
import type { ArticleListItem } from "../api";
import { articleLengthLabel, articleListBlurb } from "../homeDerived";
import {
  difficultyClassName,
  difficultyLabel,
  type DifficultyLevel,
} from "../difficulty";
import { articleIsRead } from "../learningStats";

type Props = {
  article: ArticleListItem;
  /** Local difficulty level (computed by the page). */
  difficulty: DifficultyLevel | null;
  /** Show the source name (library view). */
  showSource?: boolean;
  /** Keyboard navigation highlight (j/k on the home page). */
  highlighted?: boolean;
};

/** One article row, shared by the home boards and the library list. */
export default function ArticleRow({
  article,
  difficulty,
  showSource,
  highlighted = false,
}: Props) {
  const blurb = articleListBlurb(article);
  const lengthLabel = articleLengthLabel(article.word_count);
  const read = articleIsRead(article);
  return (
    <li data-article-row={article.id}>
      <Link
        to={`/article/${article.id}`}
        className={`article-row${read ? " is-read" : ""}${highlighted ? " is-active" : ""}`}
      >
        <div className="article-title-line">
          <h3 className="article-title-en">{article.title}</h3>
          {lengthLabel && <span className="article-length">{lengthLabel}</span>}
          {difficulty && (
            <span className={difficultyClassName(difficulty)}>
              {difficultyLabel(difficulty)}
            </span>
          )}
        </div>
        {showSource ? (
          <p className="article-row-source muted">
            {article.source}
            {article.liked ? " · ★ 收藏" : ""}
            {read ? " · 已读" : ""}
          </p>
        ) : null}
        {blurb ? <p className="article-summary-zh">{blurb}</p> : null}
      </Link>
    </li>
  );
}
