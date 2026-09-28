import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import {
  shouldForcePlacement,
  shouldHideAppNav,
  shouldShowPlacementNav,
} from "./placement/engine";
import { useAppConfig, useFilters, useSearchQuery } from "./store";
import { useToast } from "./components/Toaster";
import { emitEvent } from "./events";
import Sidebar from "./components/Sidebar";
import { applyTheme, isThemePref } from "./theme";
import { api } from "./api";
import { type RefreshProgress } from "./api";
import "./App.css";

/**
 * Deterministic window drag: the injected data-tauri-drag-region script only
 * reacts when the exact mousedown target carries the attribute, which child
 * elements (nav, brand, spacing) defeat. Call startDragging() ourselves
 * unless the press landed on an interactive control.
 */
function beginWindowDrag(e: { button: number; target: EventTarget | null }) {
  if (e.button !== 0) return;
  const el = e.target as HTMLElement | null;
  if (el?.closest("button, a, input, select, textarea, [role='button']")) return;
  void getCurrentWindow().startDragging();
}

function IconImport() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M12 3v12" />
      <path d="m7 10 5 5 5-5" />
      <path d="M4 19h16" />
    </svg>
  );
}

function IconVocab() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
    </svg>
  );
}

function IconSettings() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.06l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .06-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.06-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.06H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.06l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.06 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function IconStats() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M3 3v18h18" />
      <path d="M7 15v3" />
      <path d="M12 10v8" />
      <path d="M17 6v12" />
    </svg>
  );
}

function IconRefresh() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M21 12a9 9 0 1 1-2.64-6.36" />
      <path d="M21 3v6h-6" />
    </svg>
  );
}

function IconSearch() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </svg>
  );
}

function IconFilter() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M3 5h18l-7 8v5l-4 2v-7z" />
    </svg>
  );
}

function IconClose() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden>
      <line x1="6" y1="6" x2="18" y2="18" />
      <line x1="18" y1="6" x2="6" y2="18" />
    </svg>
  );
}

function IconSidebar() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M9 4v16" />
    </svg>
  );
}

export default function App() {
  const [progress, setProgress] = useState<RefreshProgress | null>(null);
  const [importingFile, setImportingFile] = useState(false);
  /** Home search input is collapsed until opened or text is present. */
  const [searchOpen, setSearchOpen] = useState(false);
  const toast = useToast();
  const navigate = useNavigate();
  const location = useLocation();
  const { cfg, ready, loadError, refresh } = useAppConfig();
  const { query, setQuery } = useSearchQuery();
  const { filtersOpen, setFiltersOpen } = useFilters();

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    let hideTimer: number | undefined;

    void listen<RefreshProgress>("refresh-progress", (event) => {
      const next = event.payload;
      setProgress(next);
      if (hideTimer) window.clearTimeout(hideTimer);
      if (next.phase === "done") {
        hideTimer = window.setTimeout(() => setProgress(null), 1200);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
      if (hideTimer) window.clearTimeout(hideTimer);
    };
  }, []);

  useEffect(() => {
    applyTheme(isThemePref(cfg.theme) ? cfg.theme : "system");
  }, [cfg.theme]);

  useEffect(() => {
    if (!ready || loadError) return;
    if (location.pathname.startsWith("/placement")) return;
    if (shouldForcePlacement(cfg)) {
      navigate("/placement", { replace: true });
    }
  }, [cfg, ready, loadError, location.pathname, navigate]);

  // NavLinks behave by platform convention: clicking the active page is a
  // no-op. Home is one click away on the brand (拾言).

  // Subscription refresh lives here (topbar) but reloads Home via the shared
  // event, so the list page owns no refresh chrome of its own.
  async function onRefresh() {
    if (progress && progress.phase !== "done") return;
    try {
      const result = await api.refreshFeeds();
      emitEvent("shiyan:refreshed", { result });
    } catch (e) {
      emitEvent("shiyan:refreshed", { error: String(e) });
    }
  }

  async function onCancelRefresh() {
    try {
      await api.cancelRefresh();
    } catch (e) {
      toast.err(String(e));
    }
  }

  async function onImportFile() {
    if (importingFile) return;
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: false,
        filters: [{ name: "文档", extensions: ["txt", "pdf", "docx"] }],
      });
    } catch (e) {
      toast.err(String(e));
      return;
    }
    if (selected === null) return;
    const path = Array.isArray(selected) ? selected[0] : selected;
    if (!path) return;
    setImportingFile(true);
    try {
      const article = await api.importArticleFile(path);
      navigate(`/article/${article.id}`);
    } catch (e) {
      toast.err(String(e));
    } finally {
      setImportingFile(false);
    }
  }

  const forcePlacement = shouldForcePlacement(cfg);
  const hideNav = shouldHideAppNav({
    ready,
    loadError,
    forcePlacement,
  });
  const showPlacementNav = shouldShowPlacementNav({
    ready,
    loadError,
    forcePlacement,
  });
  const showBar = progress != null && progress.phase !== "done";
  const showDoneBriefly = progress?.phase === "done";
  /** A refresh is in flight (drives the topbar refresh/cancel buttons). */
  const refreshing = showBar;

  // Sidebar lives in the browse shell only. Home shows the full panel; the
  // Reader collapses it to a glanceable priority rail; utility pages (Vocab /
  // Stats / Settings / Placement) hide it for full width.
  const isHome = location.pathname === "/";
  const isReader = location.pathname.startsWith("/article/");
  const showSidebar = !hideNav && (isHome || isReader);
  // Manual expand/collapse within the Reader; reset when leaving the route.
  const [railExpanded, setRailExpanded] = useState(false);
  useEffect(() => {
    if (!isReader) setRailExpanded(false);
  }, [isReader]);
  const sidebarCollapsed = isReader && !railExpanded;

  return (
    <div className={`app-shell${showSidebar ? " with-sidebar" : ""}${progress ? " refreshing" : ""}`}>
      {showSidebar && (
        <Sidebar
          collapsed={sidebarCollapsed}
          onExpand={() => setRailExpanded(true)}
        />
      )}
      <div className="app-col">
      <header className="topbar" onMouseDown={beginWindowDrag}>
        <nav className="topbar-nav" data-tauri-drag-region>
          {!hideNav && isReader && (
            <button
              type="button"
              className="topbar-btn"
              onClick={() => setRailExpanded((v) => !v)}
              title={railExpanded ? "收起侧边栏" : "展开侧边栏"}
              aria-label="切换侧边栏"
            >
              <IconSidebar />
            </button>
          )}
          <NavLink
            to="/"
            className="brand-mini"
            data-tauri-drag-region
            title="回主界面"
            aria-label="回主界面"
          >
            拾言
          </NavLink>
          {hideNav && showPlacementNav && (
            <NavLink to="/placement" className="topbar-link">
              词汇测评
            </NavLink>
          )}
        </nav>
        {!hideNav && (
          <div className="topbar-actions">
            {/* Contextual cluster: buttons vary by page, globals below never move. */}
            {isHome && (
              <>
                <button
                  type="button"
                  className={`topbar-btn${refreshing ? " spin" : ""}`}
                  onClick={() => void onRefresh()}
                  disabled={refreshing}
                  title="刷新订阅"
                  aria-label="刷新订阅"
                >
                  <IconRefresh />
                </button>
                {refreshing && (
                  <button
                    type="button"
                    className="topbar-btn"
                    onClick={() => void onCancelRefresh()}
                    title="取消刷新（已下载的保留）"
                    aria-label="取消刷新"
                  >
                    <IconClose />
                  </button>
                )}
                <button
                  type="button"
                  className={`topbar-btn${filtersOpen ? " active" : ""}`}
                  onClick={() => setFiltersOpen(!filtersOpen)}
                  title="筛选"
                  aria-label="筛选"
                  aria-expanded={filtersOpen}
                >
                  <IconFilter />
                </button>
                <div className={`home-search${searchOpen || query ? " open" : ""}`}>
                  <button
                    type="button"
                    className="topbar-btn"
                    onClick={() => {
                      if (query) {
                        setQuery("");
                      }
                      setSearchOpen((v) => !v);
                    }}
                    title="搜索文章"
                    aria-label="搜索文章"
                  >
                    <IconSearch />
                  </button>
                  {searchOpen && (
                    <input
                      className="search-input"
                      type="search"
                      autoFocus
                      placeholder="搜索全库标题 / 简介 / 来源 / 标签"
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Escape") {
                          setQuery("");
                          setSearchOpen(false);
                        }
                      }}
                      onBlur={() => {
                        if (!query) setSearchOpen(false);
                      }}
                    />
                  )}
                </div>
                <span className="topbar-divider" aria-hidden />
              </>
            )}
            <button
              type="button"
              className="topbar-btn"
              onClick={() => void onImportFile()}
              disabled={importingFile}
              title="导入文件（txt / pdf / docx）"
              aria-label="导入文件"
            >
              <IconImport />
            </button>
            <NavLink
              to="/vocab"
              className="topbar-btn"
              title="生词库"
              aria-label="生词库"
            >
              <IconVocab />
            </NavLink>
            <NavLink
              to="/stats"
              className="topbar-btn"
              title="统计"
              aria-label="统计"
            >
              <IconStats />
            </NavLink>
            <NavLink
              to="/settings"
              className="topbar-btn"
              title="设置"
              aria-label="设置"
            >
              <IconSettings />
            </NavLink>
          </div>
        )}
      </header>
      <main className="main">
        {loadError && (
          <div className="banner err with-action" role="status">
            <span>配置加载失败：{loadError}</span>
            <button type="button" className="btn small" onClick={() => void refresh()}>
              重试
            </button>
          </div>
        )}
        <Outlet />
      </main>
      </div>
      {(showBar || showDoneBriefly) && progress && (
        <div
          className={`refresh-progress ${progress.phase === "done" ? "done" : ""}`}
          role="status"
          aria-live="polite"
        >
          <div className="refresh-progress-meta">
            <span className="refresh-progress-label">{progress.label}</span>
            <span className="refresh-progress-side">
              <span className="refresh-progress-articles">
                新增 {progress.articles} 篇
              </span>
              <span className="refresh-progress-pct">{Math.min(100, progress.percent)}%</span>
            </span>
          </div>
          <div className="refresh-progress-track">
            <div
              className="refresh-progress-fill"
              style={{ width: `${Math.min(100, progress.percent)}%` }}
            />
          </div>
        </div>
      )}
    </div>
  );
}
