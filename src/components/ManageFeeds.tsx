import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, FeedCategory, FeedSource } from "../api";
import FeedDiscoverSection, {
  type DiscoverRow,
} from "./FeedDiscoverSection";
import { categoryLabel } from "../readerUtils";
import { useFeeds } from "../store";
import { useToast } from "./Toaster";

type AddTab = "discover" | "paste" | "article";

/** Subscription manager, embedded in the Settings page (订阅 tab). */
export default function ManageFeeds() {
  const navigate = useNavigate();
  const toast = useToast();
  // The sidebar already owns the feed list; subscribing, muting or deleting
  // here reloads that one copy so the tree updates without waiting for the
  // next refresh. Only the category table is local to this page.
  const { feeds, reloadFeeds } = useFeeds();
  const [categories, setCategories] = useState<FeedCategory[]>([]);
  const [categoryId, setCategoryId] = useState("all");
  const [newCatLabel, setNewCatLabel] = useState("");
  const [addingCat, setAddingCat] = useState(false);
  const [addTab, setAddTab] = useState<AddTab>("discover");
  const [discovering, setDiscovering] = useState(false);
  const [candidates, setCandidates] = useState<DiscoverRow[]>([]);
  const [pasteUrl, setPasteUrl] = useState("");
  const [pasteName, setPasteName] = useState("");
  const [pasting, setPasting] = useState(false);
  const [articleUrl, setArticleUrl] = useState("");
  const [importingArticle, setImportingArticle] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [cats] = await Promise.all([api.listFeedCategories(), reloadFeeds()]);
      setCategories(cats);
    } catch (e) {
      setError(String(e));
    }
  }, [reloadFeeds]);

  useEffect(() => {
    void load();
  }, [load]);

  const filteredFeeds = useMemo(() => {
    if (categoryId === "all") return feeds;
    return feeds.filter((f) => f.category === categoryId);
  }, [feeds, categoryId]);

  const discoverCategoryId =
    categoryId === "all" ? categories[0]?.id ?? "tech" : categoryId;

  const subscribedUrls = useMemo(
    () => new Set(feeds.filter((f) => f.enabled).map((f) => f.url)),
    [feeds],
  );

  async function toggleFeed(id: string, enabled: boolean) {
    setError(null);
    try {
      await api.setFeedEnabled(id, enabled);
      await reloadFeeds();
    } catch (e) {
      setError(`切换订阅状态失败：${String(e)}`);
    }
  }

  async function onDeleteFeed(feed: FeedSource) {
    if (
      !window.confirm(
        `删除订阅源「${feed.name}」？已收录的文章会保留，但不再拉取更新。`,
      )
    ) {
      return;
    }
    setError(null);
    try {
      await api.deleteFeedSource(feed.id);
      await reloadFeeds();
      toast.ok(`已删除订阅源：${feed.name}`);
    } catch (e) {
      setError(`删除订阅源失败：${String(e)}`);
    }
  }

  async function onAddCategory(e: FormEvent) {
    e.preventDefault();
    const label = newCatLabel.trim();
    if (!label) return;
    setAddingCat(true);
    setError(null);
    try {
      const cat = await api.addFeedCategory(label);
      setNewCatLabel("");
      setCategories((prev) => [...prev, cat]);
      setCategoryId(cat.id);
      toast.ok(`已添加分类「${cat.label}」`);
    } catch (err) {
      setError(`添加分类失败：${String(err)}`);
    } finally {
      setAddingCat(false);
    }
  }

  async function onDiscover() {
    setDiscovering(true);
    setError(null);
    setCandidates([]);
    try {
      const list = await api.discoverFeeds(discoverCategoryId);
      const rows: DiscoverRow[] = list.map((c) => ({
        ...c,
        subscribed: subscribedUrls.has(c.url.trim()),
      }));
      setCandidates(rows);
      // 校验进度直接体现在每行状态（校验中…/可用/不可用），不弹 toast。
      for (let i = 0; i < rows.length; i++) {
        setCandidates((prev) =>
          prev.map((r, idx) => (idx === i ? { ...r, validating: true } : r)),
        );
        try {
          const validation = await api.validateFeed(rows[i].url);
          setCandidates((prev) =>
            prev.map((r, idx) =>
              idx === i ? { ...r, validating: false, validation } : r,
            ),
          );
        } catch (err) {
          setCandidates((prev) =>
            prev.map((r, idx) =>
              idx === i
                ? {
                    ...r,
                    validating: false,
                    validation: {
                      ok: false,
                      title: null,
                      entry_count: 0,
                      error: String(err),
                    },
                  }
                : r,
            ),
          );
        }
      }
      // 校验结果直接体现在每行状态，不弹 toast。
    } catch (e) {
      setError(`发现订阅源失败：${String(e)}`);
    } finally {
      setDiscovering(false);
    }
  }

  async function onSubscribeCandidate(row: DiscoverRow) {
    setError(null);
    try {
      if (!row.validation?.ok) {
        const v = await api.validateFeed(row.url);
        if (!v.ok) {
          setError(v.error ?? "该源无法校验");
          return;
        }
      }
      await api.subscribeFeed({
        name: row.name,
        category: discoverCategoryId,
        url: row.url,
        description: row.description,
      });
      setCandidates((prev) =>
        prev.map((c) => (c.url === row.url ? { ...c, subscribed: true } : c)),
      );
      await reloadFeeds();
      toast.ok(`已订阅：${row.name}`);
    } catch (e) {
      setError(`订阅失败：${String(e)}`);
    }
  }

  async function onImportArticle(e: FormEvent) {
    e.preventDefault();
    const url = articleUrl.trim();
    if (!url) return;
    setImportingArticle(true);
    setError(null);
    try {
      const article = await api.importArticleUrl(url);
      setArticleUrl("");
      // 落地阅读页本身就是反馈，不弹 toast。
      // Same behavior as file import: land the reader on the fresh article.
      navigate(`/article/${article.id}`);
    } catch (err) {
      setError(`导入失败：${String(err)}`);
    } finally {
      setImportingArticle(false);
    }
  }

  async function onPasteSubscribe(e: FormEvent) {
    e.preventDefault();
    const url = pasteUrl.trim();
    if (!url) return;
    setPasting(true);
    setError(null);
    try {
      const v = await api.validateFeed(url);
      if (!v.ok) {
        setError(v.error ?? "RSS 校验失败");
        return;
      }
      const name =
        pasteName.trim() || v.title?.trim() || url.replace(/^https?:\/\//, "");
      await api.subscribeFeed({
        name,
        category: discoverCategoryId,
        url,
        description: "",
      });
      setPasteUrl("");
      setPasteName("");
      await reloadFeeds();
      toast.ok(`已订阅：${name}`);
    } catch (err) {
      setError(`订阅失败：${String(err)}`);
    } finally {
      setPasting(false);
    }
  }

  return (
    <div className="feeds-page">
      {error && <p className="banner err">{error}</p>}

      <div className="feeds-cat-row">
        <button
          type="button"
          className={categoryId === "all" ? "tab active" : "tab"}
          onClick={() => setCategoryId("all")}
        >
          全部
        </button>
        {categories.map((c) => (
          <button
            key={c.id}
            type="button"
            className={categoryId === c.id ? "tab active" : "tab"}
            onClick={() => setCategoryId(c.id)}
          >
            {c.label}
          </button>
        ))}
        <form className="feeds-cat-add" onSubmit={(e) => void onAddCategory(e)}>
          <input
            value={newCatLabel}
            onChange={(e) => setNewCatLabel(e.target.value)}
            placeholder="新建分类"
            disabled={addingCat}
          />
          <button
            className="btn small"
            type="submit"
            disabled={addingCat || !newCatLabel.trim()}
          >
            {addingCat ? "添加中…" : "+ 分类"}
          </button>
        </form>
      </div>

      <section className="feeds-drawer-section">
        <h3>我的订阅</h3>
        <ul className="feed-list">
          {filteredFeeds.length === 0 && (
            <li className="muted">该分类下暂无订阅</li>
          )}
          {filteredFeeds.map((f) => (
            <li key={f.id} className="feed-row-item">
              <label
                className="feed-enabled"
                title={f.enabled ? "已启用" : "已停用"}
              >
                <input
                  type="checkbox"
                  checked={f.enabled}
                  onChange={(e) => void toggleFeed(f.id, e.target.checked)}
                />
              </label>
              <div className="feed-row-main">
                <div className="feed-row-title">
                  <strong>{f.name}</strong>
                  <span className={f.origin === "user" ? "pill" : "pill muted-pill"}>
                    {f.origin === "user" ? "自订" : "精选"}
                  </span>
                  <span className="feed-url muted">{f.url}</span>
                  {f.description && (
                    <span className="muted feed-desc">{f.description}</span>
                  )}
                </div>
              </div>
              {f.origin === "user" && (
                <button
                  type="button"
                  className="icon-btn feed-delete"
                  title="删除订阅源"
                  aria-label={`删除订阅源 ${f.name}`}
                  onClick={() => void onDeleteFeed(f)}
                >
                  ✕
                </button>
              )}
            </li>
          ))}
        </ul>
        <p className="muted feed-hint">精选源不支持删除，可取消勾选停用。</p>
      </section>

      <details className="feeds-add-block" open>
        <summary>添加订阅 / 导入文章</summary>
        <div className="tabs feeds-add-tabs">
          {(
            [
              ["discover", "发现新源"],
              ["paste", "粘贴 RSS"],
              ["article", "导入文章"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={addTab === id ? "tab active" : "tab"}
              onClick={() => setAddTab(id)}
            >
              {label}
            </button>
          ))}
        </div>

        {addTab === "discover" && (
          <FeedDiscoverSection
            categoryLabel={categoryLabel(discoverCategoryId, categories)}
            discovering={discovering}
            candidates={candidates}
            onDiscover={() => void onDiscover()}
            onSubscribe={(row) => void onSubscribeCandidate(row)}
          />
        )}

        {addTab === "paste" && (
          <form className="feeds-paste" onSubmit={(e) => void onPasteSubscribe(e)}>
            <input
              value={pasteName}
              onChange={(e) => setPasteName(e.target.value)}
              placeholder="名称（可选）"
              disabled={pasting}
            />
            <input
              value={pasteUrl}
              onChange={(e) => setPasteUrl(e.target.value)}
              placeholder="https://…/rss.xml"
              disabled={pasting}
            />
            <button
              className="btn primary"
              type="submit"
              disabled={pasting || !pasteUrl.trim()}
            >
              {pasting ? "订阅中…" : "订阅"}
            </button>
          </form>
        )}

        {addTab === "article" && (
          <form className="feeds-paste" onSubmit={(e) => void onImportArticle(e)}>
            <input
              type="url"
              value={articleUrl}
              onChange={(e) => setArticleUrl(e.target.value)}
              placeholder="https://example.com/some-article"
              disabled={importingArticle}
            />
            <button
              className="btn primary"
              type="submit"
              disabled={importingArticle || !articleUrl.trim()}
            >
              {importingArticle ? "导入中…" : "导入"}
            </button>
          </form>
        )}
      </details>
    </div>
  );
}
