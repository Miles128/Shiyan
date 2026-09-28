import { useEffect, useMemo, useRef, useState } from "react";
import { useShell } from "../store";
import { useToast } from "./Toaster";
import { api, type FeedCategory, type FeedSource } from "../api";

/** Stable accent per category so the tree and the collapsed rail read alike. */
const CATEGORY_COLOR: Record<string, string> = {
  tech: "#0f5c4c",
  finance: "#b58105",
  world: "#2f5d8a",
  other: "#6f6a63",
};
/** Extra accents for user-created categories, picked deterministically. */
const CUSTOM_COLORS = ["#7a4fa3", "#b5542d", "#2d7db5", "#4fa372", "#a34f6e", "#6e7a2d"];
/** Fixed top-level order for the built-in categories (mirrors the DB seed). */
const CATEGORY_ORDER: { id: string; label: string }[] = [
  { id: "tech", label: "科技" },
  { id: "finance", label: "财经" },
  { id: "world", label: "国际" },
  { id: "other", label: "其他" },
];
/** Drag-to-resize width: persisted so the choice survives route changes. */
const WIDTH_KEY = "shiyan.sidebarWidth";
const MIN_SIDEBAR_W = 200;
const MAX_SIDEBAR_W = 480;
const DEFAULT_SIDEBAR_W = 248;

function loadSidebarWidth(): number {
  try {
    const n = Number(localStorage.getItem(WIDTH_KEY));
    if (Number.isFinite(n) && n > 0) {
      return Math.min(MAX_SIDEBAR_W, Math.max(MIN_SIDEBAR_W, n));
    }
  } catch {
    /* ignore */
  }
  return DEFAULT_SIDEBAR_W;
}

function dotColor(category: string): string {
  const direct = CATEGORY_COLOR[category];
  if (direct) return direct;
  let h = 0;
  for (const c of category) h = (h * 31 + c.codePointAt(0)!) | 0;
  return CUSTOM_COLORS[Math.abs(h) % CUSTOM_COLORS.length];
}

type Props = {
  /** Narrow icon rail (Reader). Full tree otherwise. */
  collapsed: boolean;
  onExpand: () => void;
};

/**
 * Codex-style minimal tree: 今日推荐 + one node per category, each expandable
 * via a +/− sign to reveal its member sources. Category nodes toggle expansion
 * only (they never filter); sources are draggable within their own category,
 * which is what drives the per-category ranking bonus.
 */
export default function Sidebar({ collapsed, onExpand }: Props) {
  const {
    feeds,
    reloadFeeds,
    commitFeedOrder,
    focusSource,
    setFocusSource,
  } = useShell();
  const toast = useToast();

  // Categories are DB-driven (built-ins + user-created). Reloaded whenever
  // feeds reload so a new category appears without an app restart.
  const [cats, setCats] = useState<FeedCategory[]>([]);
  useEffect(() => {
    let alive = true;
    void api
      .listFeedCategories()
      .then((list) => {
        if (alive) setCats(list);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [feeds, reloadFeeds]);

  const labelById = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of CATEGORY_ORDER) m.set(c.id, c.label);
    for (const c of cats) m.set(c.id, c.label);
    return m;
  }, [cats]);

  // Expanded branches; unknown keys default to open (`!== false` below).
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const toggle = (key: string) => setOpen((s) => ({ ...s, [key]: !s[key] }));

  // Pointer-based sortable. Drag lives entirely in refs + imperative transforms:
  // no React state is touched mid-gesture (re-rendering aborts WKWebView drags).
  // The grabbed row tracks the cursor; siblings slide via CSS transitions to open
  // a gap, then the drop commits the same order React re-renders → no snap-back.
  type SortState = {
    cat: string;
    fromIndex: number;
    rows: HTMLElement[];
    tops: number[];
    pitch: number;
    startY: number;
    target: number;
    started: boolean;
    move: (e: PointerEvent) => void;
    up: (e: PointerEvent) => void;
  };
  const sort = useRef<SortState | null>(null);
  // A drag that ends still fires click; suppress the select that would follow.
  const didDrag = useRef(false);
  // Teardown for whichever pointer drag is live (resize / sort). Kept in a ref
  // so an unmount mid-drag still removes the window listeners and the body
  // modifier class (is-resizing disables user selection app-wide).
  const dragCleanup = useRef<(() => void) | null>(null);
  useEffect(() => () => dragCleanup.current?.(), []);

  // Right-edge drag handle: the sidebar width follows the pointer live and the
  // final width is written to localStorage on release.
  const [width, setWidth] = useState(loadSidebarWidth);

  function beginResize(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    const startX = e.clientX;
    const startW = width;
    let current = startW;
    const move = (ev: PointerEvent) => {
      current = Math.min(
        MAX_SIDEBAR_W,
        Math.max(MIN_SIDEBAR_W, startW + (ev.clientX - startX)),
      );
      setWidth(current);
    };
    const cleanup = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      document.body.classList.remove("is-resizing");
      dragCleanup.current = null;
    };
    const up = () => {
      cleanup();
      try {
        localStorage.setItem(WIDTH_KEY, String(current));
      } catch {
        /* ignore */
      }
    };
    document.body.classList.add("is-resizing");
    dragCleanup.current = cleanup;
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  // Group feeds by category: built-ins in seed order, then user-created
  // categories in DB order (builtin DESC, label). Feeds whose category is
  // unknown to the DB still get a group so nothing disappears.
  const grouped = useMemo(() => {
    const byCat = new Map<string, FeedSource[]>();
    for (const f of feeds) {
      const arr = byCat.get(f.category);
      if (arr) arr.push(f);
      else byCat.set(f.category, [f]);
    }
    const order: string[] = [
      ...CATEGORY_ORDER.map((c) => c.id),
      ...cats.filter((c) => !CATEGORY_ORDER.some((b) => b.id === c.id)).map((c) => c.id),
    ];
    for (const cat of byCat.keys()) if (!order.includes(cat)) order.push(cat);
    return order
      .filter((cat) => (byCat.get(cat)?.length ?? 0) > 0)
      .map((cat) => {
        const list = byCat.get(cat)!;
        // enabled first, then disabled — muting de-prioritises visibly.
        const sorted = [
          ...list.filter((f) => f.enabled),
          ...list.filter((f) => !f.enabled),
        ];
        return { cat, label: labelById.get(cat) ?? cat, feeds: sorted };
      });
  }, [feeds, cats, labelById]);

  function commitMove(cat: string, from: number, to: number) {
    const group = grouped.find((g) => g.cat === cat);
    if (!group || from === to) return;
    const next = group.feeds.slice();
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    // Rebuild the full flat order (all categories, in tree order) so priority is
    // recomputed for every block, not just the touched one.
    const ids: string[] = [];
    for (const g of grouped) {
      if (g.cat === cat) ids.push(...next.map((f) => f.id));
      else ids.push(...g.feeds.map((f) => f.id));
    }
    void commitFeedOrder(ids).catch((e) => toast.err(`保存排序失败：${String(e)}`));
  }

  function layout(s: SortState, dy: number) {
    const n = s.rows.length;
    s.target = Math.max(0, Math.min(n - 1, s.fromIndex + Math.round(dy / s.pitch)));
    for (let i = 0; i < n; i++) {
      const row = s.rows[i];
      if (i === s.fromIndex) continue;
      let shift = 0;
      if (s.fromIndex < s.target && i > s.fromIndex && i <= s.target) shift = -s.pitch;
      else if (s.fromIndex > s.target && i >= s.target && i < s.fromIndex) shift = s.pitch;
      row.style.transform = `translateY(${shift}px)`;
    }
    const grabbed = s.rows[s.fromIndex];
    grabbed.style.transition = "none";
    grabbed.style.transform = `translateY(${dy}px)`;
  }

  function resetSort(s: SortState) {
    for (const row of s.rows) {
      row.style.transform = "";
      row.style.transition = "";
      row.classList.remove("is-dragging");
    }
    document.body.classList.remove("is-sorting");
    window.removeEventListener("pointermove", s.move);
    window.removeEventListener("pointerup", s.up);
    window.removeEventListener("pointercancel", s.up);
    sort.current = null;
    dragCleanup.current = null;
  }

  function beginSort(
    e: React.PointerEvent<HTMLElement>,
    cat: string,
    index: number,
  ) {
    if (e.button !== 0) return;
    const rowEl = e.currentTarget;
    const container = rowEl.parentElement;
    if (!container) return;
    const rows = Array.from(container.children) as HTMLElement[];
    if (rows.length < 2) return;
    didDrag.current = false;
    const tops = rows.map((r) => r.getBoundingClientRect().top);
    const pitch = rows.length > 1 ? tops[1] - tops[0] : rowEl.offsetHeight + 1;
    const startY = e.clientY;

    const s: SortState = {
      cat,
      fromIndex: index,
      rows,
      tops,
      pitch,
      startY,
      target: index,
      started: false,
      move: () => {},
      up: () => {},
    };
    s.move = (ev: PointerEvent) => {
      const dy = ev.clientY - s.startY;
      if (!s.started) {
        // A tiny threshold keeps plain clicks (select) from becoming a drag.
        if (Math.abs(dy) < 4) return;
        s.started = true;
        didDrag.current = true;
        document.body.classList.add("is-sorting");
        rows[s.fromIndex].classList.add("is-dragging");
        ev.preventDefault();
      }
      layout(s, dy);
    };
    s.up = () => {
      const finished = { ...s };
      resetSort(s);
      if (finished.started) commitMove(finished.cat, finished.fromIndex, finished.target);
    };
    sort.current = s;
    dragCleanup.current = () => resetSort(s);
    window.addEventListener("pointermove", s.move, { passive: false });
    window.addEventListener("pointerup", s.up);
    window.addEventListener("pointercancel", s.up);
  }

  if (collapsed) {

    return (
      <aside className="sidebar sidebar-rail" aria-label="订阅源（收起）">
        <button
          type="button"
          className="rail-expand"
          onClick={onExpand}
          title="展开侧边栏"
          aria-label="展开侧边栏"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="m9 18 6-6-6-6" />
          </svg>
        </button>
      </aside>
    );
  }

  return (
    <aside
      className="sidebar"
      aria-label="订阅源"
      style={{ width, flexBasis: width }}
    >
      <div
        className="sidebar-resizer"
        onPointerDown={beginResize}
        role="separator"
        aria-orientation="vertical"
        aria-label="调整侧边栏宽度"
        title="拖动调整侧边栏宽度"
      />
      <div className="sidebar-header">
        <span className="sidebar-wordmark" data-tauri-drag-region>
          订阅
        </span>
      </div>

      <div className="sidebar-scroll">
        {/* 今日推荐: the default main-list view — a plain highlight row. */}
        <div className="tree-node">
          <button
            type="button"
            className={"tree-branch" + (focusSource === null ? " active" : "")}
            onClick={() => setFocusSource(null)}
          >
            <span className="tree-label">今日推荐</span>
          </button>
        </div>

        {grouped.map((g) => (
          <div key={g.cat} className="tree-node">
            <button
              type="button"
              className="tree-branch"
              onClick={() => toggle(g.cat)}
              aria-expanded={open[g.cat] !== false}
            >
              <span className="tree-sign">{open[g.cat] !== false ? "−" : "+"}</span>
              <span className="tree-label">{g.label}</span>
            </button>
            {open[g.cat] !== false && (
              <div className="tree-children">
                {g.feeds.map((f, index) => (
                  <div
                    key={f.id}
                    className={
                      "tree-leaf" +
                      (f.enabled ? "" : " is-disabled") +
                      (focusSource === f.name ? " active" : "")
                    }
                    onClick={() => {
                      if (didDrag.current) {
                        didDrag.current = false;
                        return;
                      }
                      setFocusSource(focusSource === f.name ? null : f.name);
                    }}
                    onPointerDown={(e) => beginSort(e, g.cat, index)}
                  >
                    <span
                      className="source-dot"
                      style={{ background: dotColor(f.category) }}
                    />
                    <span className="source-name" title={f.name}>
                      {f.name}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}

      </div>
    </aside>
  );
}
