import { Link } from "react-router-dom";
import { api } from "../api";
import { useQuery } from "../query";
import PageBack from "../components/PageBack";

function fmtMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes} 分钟`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m === 0 ? `${h} 小时` : `${h} 小时 ${m} 分`;
}

function fmtWords(words: number): string {
  if (words < 1000) return String(words);
  return `${(words / 1000).toFixed(1)}k`;
}

/** Reading statistics: daily activity, totals and library state. */
export default function Stats() {
  // Mount paints the cached stats instantly when present (shared with the
  // Home header's learning-stats entry point only by backend, not by key —
  // different command, different entry), then revalidates.
  const statsQuery = useQuery(["reading-stats"], () => api.getReadingStats());
  const stats = statsQuery.data ?? null;
  // Surface fetch errors only when there is nothing to show (same contract
  // as the article view: cached content is never covered by a banner).
  const loadError = statsQuery.error == null ? null : String(statsQuery.error);
  const error = stats === null ? loadError : null;

  if (error) {
    return (
      <div className="page">
        <p className="banner err">{error}</p>
      </div>
    );
  }
  if (!stats) {
    return (
      <div className="page">
        <p className="muted">加载中…</p>
      </div>
    );
  }

  const maxMinutes = Math.max(1, ...stats.days.map((d) => d.minutes));
  const maxSourceMinutes = Math.max(
    1,
    ...stats.top_sources.map((s) => s.minutes),
  );

  return (
    <div className="page">
      <header className="page-header page-header-slim">
        <div>
          <p className="muted">阅读统计 · 近 14 天</p>
        </div>
        <PageBack />
      </header>

      {stats.articles_total === 0 && (
        <p className="muted">还没有阅读记录。读完几篇后，这里会出现阅读统计。</p>
      )}

      <section className="stat-grid">
        <div className="stat-card">
          <span className="stat-value">{fmtMinutes(stats.minutes_total)}</span>
          <span className="stat-label">
            累计阅读时长 · 本周 {fmtMinutes(stats.minutes_7d)}
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{stats.articles_total}</span>
          <span className="stat-label">
            读过的文章 · 本周 {stats.articles_7d} 篇
          </span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{stats.streak_days} 天</span>
          <span className="stat-label">连续阅读</span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{fmtWords(stats.words_total)}</span>
          <span className="stat-label">累计读过词数</span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{stats.completed_total}</span>
          <span className="stat-label">读完（滚到文末）</span>
        </div>
        <div className="stat-card">
          <span className="stat-value">{stats.liked_total}</span>
          <span className="stat-label">收藏</span>
        </div>
      </section>

      <section className="settings-section">
        <h2>每日阅读</h2>
        <div className="day-chart">
          {stats.days.map((day) => (
            <div className="day-col" key={day.date} title={`${day.date}：${day.articles} 篇 / ${day.minutes} 分钟`}>
              <div className="day-bar-track">
                <div
                  className="day-bar"
                  style={{ height: `${(day.minutes / maxMinutes) * 100}%` }}
                />
              </div>
              <span className="day-label">{day.date.slice(5)}</span>
            </div>
          ))}
        </div>
        <p className="muted">
          柱高表示当天阅读时长；悬停可看当天篇数。近 14 天合计{" "}
          {stats.days.reduce((n, d) => n + d.articles, 0)} 篇 ·{" "}
          {fmtMinutes(stats.days.reduce((n, d) => n + d.minutes, 0))}
        </p>
      </section>

      <section className="settings-section">
        <h2>常读来源</h2>
        {stats.top_sources.length === 0 && <p className="muted">还没有阅读记录。</p>}
        <ul className="source-stats">
          {stats.top_sources.map((s) => (
            <li key={s.name}>
              <div className="source-stat-head">
                <strong>{s.name}</strong>
                <span className="muted">
                  {s.articles} 篇 · {fmtMinutes(s.minutes)}
                </span>
              </div>
              <div className="source-stat-track">
                <div
                  className="source-stat-fill"
                  style={{ width: `${(s.minutes / maxSourceMinutes) * 100}%` }}
                />
              </div>
            </li>
          ))}
        </ul>
      </section>

      <section className="settings-section">
        <h2>学习库</h2>
        <div className="stat-grid">
          <div className="stat-card">
            <span className="stat-value">{stats.vocab_learning}</span>
            <span className="stat-label">生词学习中</span>
          </div>
          <div className="stat-card">
            <span className="stat-value">{stats.vocab_mastered}</span>
            <span className="stat-label">生词已掌握</span>
          </div>
          <div className="stat-card">
            <span className="stat-value">{stats.phrases_learning}</span>
            <span className="stat-label">短语学习中</span>
          </div>
          <div className="stat-card">
            <span className="stat-value">{stats.phrases_mastered}</span>
            <span className="stat-label">短语已掌握</span>
          </div>
          <div className="stat-card">
            <span className="stat-value">{stats.due_today}</span>
            <span className="stat-label">
              今日待复习 · <Link to="/vocab">去复习</Link>
            </span>
          </div>
        </div>
      </section>
    </div>
  );
}
