import type { FeedDiscoverCandidate, FeedValidation } from "../api";

export type DiscoverRow = FeedDiscoverCandidate & {
  validation?: FeedValidation;
  validating?: boolean;
  subscribed?: boolean;
};

type Props = {
  categoryLabel: string;
  discovering: boolean;
  candidates: DiscoverRow[];
  onDiscover: () => void;
  onSubscribe: (row: DiscoverRow) => void;
};

export default function FeedDiscoverSection({
  categoryLabel,
  discovering,
  candidates,
  onDiscover,
  onSubscribe,
}: Props) {
  return (
    <section className="feeds-drawer-section">
      <div className="feeds-section-head">
        <h3>
          按分类发现
          <span className="muted"> · 将加入 {categoryLabel} 分类</span>
        </h3>
        <button
          type="button"
          className="btn primary"
          onClick={onDiscover}
          disabled={discovering}
        >
          {discovering ? "搜索中…" : "用 AI 搜索推荐源"}
        </button>
      </div>
      <ul className="discover-list">
        {candidates.map((c) => {
          const ok = c.validation?.ok;
          const status = c.validating
            ? "校验中…"
            : c.validation
              ? ok
                ? `可用 · ${c.validation.entry_count} 条`
                : c.validation.error ?? "不可用"
              : "待校验";
          return (
            <li key={c.url} className="discover-row">
              <div>
                <strong>{c.name}</strong>
                {c.description ? (
                  <p className="muted discover-desc">{c.description}</p>
                ) : null}
                <p className="feed-url muted">{c.url}</p>
                <p className={ok ? "muted" : "banner err inline-status"}>
                  {status}
                </p>
              </div>
              <button
                type="button"
                className="btn"
                disabled={c.subscribed || c.validating || ok === false}
                onClick={() => onSubscribe(c)}
              >
                {c.subscribed ? "已订阅" : "订阅"}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
