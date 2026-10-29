import { useState } from "react";
import { Link } from "react-router-dom";
import { useAppConfig, useVocab } from "../store";
import { api } from "../api";
import { placementSummaryText } from "../placement/engine";
import MemoryLibrary from "../components/MemoryLibrary";
import Tabs from "../components/Tabs";
import PageBack from "../components/PageBack";

type Library = "vocab" | "phrases";

export default function Vocab() {
  const { refreshLearningTerms } = useVocab();
  const { cfg } = useAppConfig();
  const placementDone = Boolean(cfg.vocab_placement_done);
  const placementSummary = placementSummaryText(cfg);
  const [library, setLibrary] = useState<Library>("vocab");
  const [exportMsg, setExportMsg] = useState<string | null>(null);

  async function exportCsv() {
    setExportMsg(null);
    try {
      const path = await api.exportVocabCsv();
      setExportMsg(path ? `已导出到 ${path}` : "已取消导出");
    } catch (e) {
      setExportMsg(String(e));
    }
  }

  return (
    <div className="page">
      <header className="page-header page-header-slim">
        <div>
          <p className="muted">
            {placementSummary ?? "生词与短语"}
          </p>
        </div>
        <div className="row-actions">
          <button className="btn small" onClick={() => void exportCsv()}>
            导出 CSV
          </button>
          <Link className="btn small" to="/placement">
            {placementDone ? "重新测词汇量" : "测一下词汇量"}
          </Link>
          <PageBack />
        </div>
      </header>
      {exportMsg && <p className="muted">{exportMsg}</p>}

      <Tabs
        items={[
          ["vocab", "生词"],
          ["phrases", "短语"],
        ] as const}
        value={library}
        onChange={setLibrary}
      />

      {library === "phrases" ? (
        <MemoryLibrary
          kind="phrase"
          searchPlaceholder="搜索短语 / 释义 / 例句"
          emptyText="还没有短语。阅读时选中一串词语，点「加入短语」即可收藏。"
          onChanged={() => void refreshLearningTerms()}
        />
      ) : (
        <MemoryLibrary
          kind="word"
          searchPlaceholder="搜索词条 / 释义 / 类型"
          emptyText="还没有生词。阅读时划词，点「加入生词库」即可收藏。"
          onChanged={() => void refreshLearningTerms()}
        />
      )}
    </div>
  );
}
