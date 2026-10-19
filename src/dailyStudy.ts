import type { MemoryItem } from "./api/types";

export const DAILY_STUDY_KEY = "shiyan.dailyStudyOn";
/** Due cards first, then recently saved words, capped so the gate stays finishable. */
export const DAILY_QUEUE_LIMIT = 12;

export function localDateKey(now: Date): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function readDailyStudyDate(): string | null {
  try {
    const value = localStorage.getItem(DAILY_STUDY_KEY);
    return value && /^\d{4}-\d{2}-\d{2}$/.test(value) ? value : null;
  } catch {
    return null;
  }
}

export function markDailyStudyDone(today: string): void {
  try {
    localStorage.setItem(DAILY_STUDY_KEY, today);
  } catch {
    // The gate reads this key; a storage failure just asks again next launch.
  }
  window.dispatchEvent(new Event("shiyan-daily-study"));
}

/**
 * First launch of the local day, once placement is out of the way and the
 * word bank is non-empty. An empty bank does not trap the reader.
 */
export function shouldForceDailyStudy(input: {
  today: string;
  completedOn: string | null;
  wordCount: number;
  placementPending: boolean;
}): boolean {
  if (input.placementPending) return false;
  if (input.wordCount <= 0) return false;
  return input.completedOn !== input.today;
}

export function buildDailyQueue(input: {
  due: MemoryItem[];
  learning: MemoryItem[];
  limit?: number;
}): MemoryItem[] {
  const limit = input.limit ?? DAILY_QUEUE_LIMIT;
  const seen = new Set<string>();
  const out: MemoryItem[] = [];
  const push = (item: MemoryItem) => {
    if (out.length >= limit || seen.has(item.id)) return;
    if (item.kind !== "word") return;
    seen.add(item.id);
    out.push(item);
  };
  for (const item of input.due) push(item);
  const recent = [...input.learning].sort((a, b) =>
    b.created_at.localeCompare(a.created_at),
  );
  for (const item of recent) push(item);
  return out;
}
