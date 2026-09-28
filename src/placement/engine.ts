import type { CefrLevel, FreqBand } from "../wordLevels";

export const PLACEMENT_TOTAL = 50;
export const L_MIN = 400;
export const L_MAX = 25000;
export const L0 = 3000;

export type PoolItem = {
  term: string;
  rank: number;
  zh: string;
};

export function shouldForcePlacement(cfg: {
  vocab_placement_done?: boolean;
  vocab_placement_skipped?: boolean;
}): boolean {
  if (cfg.vocab_placement_done) return false;
  if (cfg.vocab_placement_skipped) return false;
  return true;
}

/** Hide Home/Vocab/Settings while placement is required. */
export function shouldHideAppNav(input: {
  ready: boolean;
  loadError: string | null;
  forcePlacement: boolean;
}): boolean {
  return input.ready && input.forcePlacement;
}

/** Config failed: do not auto-redirect, but still offer 测评. */
export function shouldShowPlacementNav(input: {
  ready: boolean;
  loadError: string | null;
  forcePlacement: boolean;
}): boolean {
  return input.ready && Boolean(input.loadError) && input.forcePlacement;
}

export function describePlacementResult(input: {
  saved: boolean;
  shownL: number;
  freqBand: number;
  cefrLevel: string;
}): { title: string; summary: string } {
  const bandLabel =
    input.freqBand >= 1000
      ? `${input.freqBand / 1000}k`
      : String(input.freqBand);
  if (!input.saved) {
    return {
      title: "测验完成",
      summary: `大约认识 ${input.shownL} 词 · 设置未写入，请重试`,
    };
  }
  return {
    title: "测验完成",
    summary: `大约认识 ${input.shownL} 词 · 已设为 ${bandLabel} / ${input.cefrLevel}`,
  };
}

/**
 * One shared copy for "my level" surfaces (Vocab header, Settings):
 * `我的词频水平：约 N 词 · 上次测验 <time>` or null when never tested.
 */
export function placementSummaryText(cfg: {
  vocab_placement_done?: boolean;
  vocab_placement_l?: number | null;
  vocab_placement_at?: string | null;
  freq_band?: number;
}): string | null {
  if (!cfg.vocab_placement_done) return null;
  const words = Math.round(cfg.vocab_placement_l ?? cfg.freq_band ?? 0);
  const when = cfg.vocab_placement_at
    ? ` · 上次测验 ${new Date(cfg.vocab_placement_at).toLocaleString()}`
    : "";
  return `我的词频水平：约 ${words} 词${when}`;
}

export function clampL(L: number): number {
  return Math.max(L_MIN, Math.min(L_MAX, L));
}

/** n is 1-based question index (1..50). */
export function updateL(
  L: number,
  d: number,
  correct: boolean,
  n: number,
): number {
  const decay = 1 - (n - 1) / PLACEMENT_TOTAL;
  const alpha = 0.18 * decay;
  const beta = 0.22 * decay;
  if (correct) {
    return clampL(L * (1 + alpha) * 0.7 + d * 1.15 * 0.3);
  }
  return clampL(L * (1 - beta) * 0.7 + d * 0.75 * 0.3);
}

const BANDS = [1000, 3000, 5000, 10000, 20000] as const satisfies readonly FreqBand[];
const CEFRS: CefrLevel[] = ["A2", "B1", "B2", "C1", "C2"];

export function mapLToBand(L: number): {
  freqBand: FreqBand;
  cefrLevel: CefrLevel;
} {
  for (let i = 0; i < BANDS.length - 1; i++) {
    const mid = Math.sqrt(BANDS[i]! * BANDS[i + 1]!);
    if (L < mid) {
      return { freqBand: BANDS[i]!, cefrLevel: CEFRS[i]! };
    }
  }
  return { freqBand: BANDS[BANDS.length - 1]!, cefrLevel: CEFRS[CEFRS.length - 1]! };
}

function normalizeZh(zh: string): string {
  return zh.trim().replace(/\s+/g, "").toLowerCase();
}

export function pickNext(
  pool: PoolItem[],
  used: Set<string>,
  L: number,
  rng: () => number = Math.random,
): PoolItem | null {
  const available = pool.filter((p) => !used.has(p.term));
  if (available.length === 0) return null;
  const jitter = 0.85 + rng() * 0.3;
  const t = Math.max(1, L * jitter);
  const logT = Math.log(t);
  const ranked = available
    .map((p) => ({
      p,
      dist: Math.abs(Math.log(Math.max(1, p.rank)) - logT),
    }))
    .sort((a, b) => a.dist - b.dist);
  const k = Math.min(25, ranked.length);
  const idx = Math.min(k - 1, Math.floor(rng() * k));
  return ranked[idx]!.p;
}

export function buildChoices(
  item: PoolItem,
  pool: PoolItem[],
  rng: () => number = Math.random,
): { options: string[]; correctIndex: number } {
  const correctKey = normalizeZh(item.zh);
  const distractors: string[] = [];
  const seen = new Set<string>([correctKey]);

  const near = [...pool]
    .filter((p) => p.term !== item.term)
    .sort((a, b) => Math.abs(a.rank - item.rank) - Math.abs(b.rank - item.rank));

  for (const p of near) {
    const key = normalizeZh(p.zh);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    distractors.push(p.zh.trim());
    if (distractors.length >= 3) break;
  }

  // Fallback: any remaining glosses if neighborhood exhausted
  if (distractors.length < 3) {
    for (const p of pool) {
      if (p.term === item.term) continue;
      const key = normalizeZh(p.zh);
      if (!key || seen.has(key)) continue;
      seen.add(key);
      distractors.push(p.zh.trim());
      if (distractors.length >= 3) break;
    }
  }

  const options = [item.zh.trim(), ...distractors.slice(0, 3)];
  while (options.length < 4) {
    options.push(`（干扰 ${options.length}）`);
  }

  // Fisher–Yates shuffle
  for (let i = options.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    const tmp = options[i]!;
    options[i] = options[j]!;
    options[j] = tmp;
  }

  const correctIndex = options.findIndex((o) => normalizeZh(o) === correctKey);
  return {
    options,
    correctIndex: correctIndex >= 0 ? correctIndex : 0,
  };
}
