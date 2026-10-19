import { useCallback, useEffect, useRef, useState } from "react";
import type { AppConfig } from "./api";
import { normalizeConfig, useAppConfig } from "./store";

const SAVE_DEBOUNCE_MS = 800;

/**
 * 设置表单的本地编辑缓冲（Settings 与 Aa 面板共用的配置编辑域）：
 * 首次就绪时从 store 播种（之后的 store 更新不得覆盖编辑中的内容），
 * 下拉/chip 选择立即保存，文本输入防抖保存，离开页面时冲刷未保存的改动。
 * 规范化只发生在播种与 store 的 `save`，改动期间不再逐键 normalize——
 * 控件给出的值本就是合法档位。
 */
export function useConfigForm(onError: (message: string) => void) {
  const { cfg: savedCfg, ready, save } = useAppConfig();
  const onErrorRef = useRef(onError);
  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  const [cfg, setCfg] = useState<AppConfig>(savedCfg);
  const cfgRef = useRef(cfg);
  const seededRef = useRef(false);
  const saveTimer = useRef<number | undefined>(undefined);
  const pendingSaveRef = useRef(false);

  // Only seed local state on first ready; subsequent `savedCfg` changes
  // (from our own saves) must not overwrite in-progress edits.
  useEffect(() => {
    if (ready && !seededRef.current) {
      seededRef.current = true;
      const next = normalizeConfig(savedCfg);
      cfgRef.current = next;
      setCfg(next);
    }
  }, [ready, savedCfg]);

  const persist = useCallback(
    async (next: AppConfig) => {
      try {
        await save(next);
      } catch (e) {
        onErrorRef.current(String(e));
      }
    },
    [save],
  );

  /** Apply a patch to local state; selects persist right away. */
  const updateNow = useCallback(
    (patch: Partial<AppConfig>) => {
      const next = { ...cfgRef.current, ...patch };
      cfgRef.current = next;
      setCfg(next);
      void persist(next);
    },
    [persist],
  );

  /** Text/number inputs debounce so typing doesn't hit disk per keystroke. */
  const updateSoon = useCallback(
    (patch: Partial<AppConfig>) => {
      const next = { ...cfgRef.current, ...patch };
      cfgRef.current = next;
      setCfg(next);
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      pendingSaveRef.current = true;
      saveTimer.current = window.setTimeout(() => {
        saveTimer.current = undefined;
        pendingSaveRef.current = false;
        void persist(cfgRef.current);
      }, SAVE_DEBOUNCE_MS);
    },
    [persist],
  );

  // Flush a pending debounced save when leaving the page.
  useEffect(() => {
    return () => {
      if (saveTimer.current) {
        window.clearTimeout(saveTimer.current);
        saveTimer.current = undefined;
        if (pendingSaveRef.current) {
          void save(cfgRef.current).catch(() => undefined);
          pendingSaveRef.current = false;
        }
      }
    };
  }, [save]);

  return { cfg, updateNow, updateSoon };
}
