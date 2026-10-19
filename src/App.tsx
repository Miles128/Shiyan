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
import {
  localDateKey,
  readDailyStudyDate,
  shouldForceDailyStudy,
} from "./dailyStudy";
import { useAppConfig, useVocab } from "./store";
import { useToast } from "./components/Toaster";
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

function IconBooks() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
      <path d="M8 7h8" />
      <path d="M8 11h6" />
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

export default function App() {
  const [progress, setProgress] = useState<RefreshProgress | null>(null);
  const [importingFile, setImportingFile] = useState(false);
  const toast = useToast();
  const navigate = useNavigate();
  const location = useLocation();
  const { cfg, ready, loadError, refresh } = useAppConfig();
  const { learningWordCount, vocabReady } = useVocab();
  const [dailyDoneOn, setDailyDoneOn] = useState<string | null>(() => readDailyStudyDate());

  useEffect(() => {
    const sync = () => setDailyDoneOn(readDailyStudyDate());
    window.addEventListener("shiyan-daily-study", sync);
    window.addEventListener("storage", sync);
    return () => {
      window.removeEventListener("shiyan-daily-study", sync);
      window.removeEventListener("storage", sync);
    };
  }, []);

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

  const placementPending = shouldForcePlacement(cfg);
  const forceDaily =
    vocabReady &&
    shouldForceDailyStudy({
      today: localDateKey(new Date()),
      completedOn: dailyDoneOn,
      wordCount: learningWordCount,
      placementPending,
    });

  useEffect(() => {
    if (!ready || loadError) return;
    if (placementPending) {
      if (!location.pathname.startsWith("/placement")) {
        navigate("/placement", { replace: true });
      }
      return;
    }
    if (!vocabReady || !forceDaily) return;
    if (location.pathname.startsWith("/daily")) return;
    navigate("/daily", { replace: true });
  }, [ready, loadError, placementPending, vocabReady, forceDaily, location.pathname, navigate]);

  // Top-bar page buttons act as toggles: clicking the active page goes home.
  function toggleNav(e: React.MouseEvent, to: string) {
    if (location.pathname === to) {
      e.preventDefault();
      navigate("/");
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

  const forcePlacement = placementPending;
  const hideNav =
    shouldHideAppNav({
      ready,
      loadError,
      forcePlacement,
    }) ||
    (forceDaily && location.pathname.startsWith("/daily"));
  const showPlacementNav = shouldShowPlacementNav({
    ready,
    loadError,
    forcePlacement,
  });
  const showBar = progress != null && progress.phase !== "done";
  const showDoneBriefly = progress?.phase === "done";

  return (
    <div className={`app-shell${progress ? " refreshing" : ""}`}>
      {hideNav ? (
        <header className="topbar" onMouseDown={beginWindowDrag}>
          <nav className="topbar-nav" data-tauri-drag-region>
            <span className="brand-mini" data-tauri-drag-region>
              拾言
            </span>
            {showPlacementNav && (
              <NavLink to="/placement" className="topbar-link">
                词汇测评
              </NavLink>
            )}
          </nav>
        </header>
      ) : (
        <>
          <header className="topbar" onMouseDown={beginWindowDrag}>
            <nav className="topbar-nav" data-tauri-drag-region>
              <span className="brand-mini" data-tauri-drag-region>
                拾言
              </span>
            </nav>
            <div className="topbar-actions">
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
                to="/books"
                className="topbar-btn"
                title="书库"
                aria-label="书库"
                onClick={(e) => toggleNav(e, "/books")}
              >
                <IconBooks />
              </NavLink>
              <NavLink
                to="/vocab"
                className="topbar-btn"
                title="生词库"
                aria-label="生词库"
                onClick={(e) => toggleNav(e, "/vocab")}
              >
                <IconVocab />
              </NavLink>
              <NavLink
                to="/settings"
                className="topbar-btn"
                title="设置"
                aria-label="设置"
                onClick={(e) => toggleNav(e, "/settings")}
              >
                <IconSettings />
              </NavLink>
            </div>
          </header>
        </>
      )}
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
      {(showBar || showDoneBriefly) && progress && (
        <div
          className={`refresh-progress ${progress.phase === "done" ? "done" : ""}`}
          role="status"
          aria-live="polite"
        >
          <div className="refresh-progress-meta">
            <span className="refresh-progress-label">{progress.label}</span>
            <span className="refresh-progress-pct">{Math.min(100, progress.percent)}%</span>
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
