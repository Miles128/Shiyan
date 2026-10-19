import { describe, expect, it } from "vitest";
import type { MemoryItem } from "./api/types";
import {
  buildDailyQueue,
  localDateKey,
  shouldForceDailyStudy,
} from "./dailyStudy";

function word(id: string, created_at: string, kind = "word"): MemoryItem {
  return {
    id,
    kind,
    term: id,
    definition_zh: "",
    word_type: "",
    collocations: [],
    context_sentence: "",
    article_id: null,
    status: "learning",
    interval_days: 0,
    reps: 0,
    consecutive_know: 0,
    next_review_at: created_at,
    created_at,
  };
}

describe("localDateKey", () => {
  it("uses the local calendar day", () => {
    expect(localDateKey(new Date(2026, 9, 19, 1, 0, 0))).toBe("2026-10-19");
  });
});

describe("shouldForceDailyStudy", () => {
  const base = {
    today: "2026-10-19",
    completedOn: null as string | null,
    wordCount: 4,
    placementPending: false,
  };

  it("forces a session on the first open of the day", () => {
    expect(shouldForceDailyStudy(base)).toBe(true);
  });

  it("stays quiet after today's session, with an empty bank, or during placement", () => {
    expect(shouldForceDailyStudy({ ...base, completedOn: "2026-10-19" })).toBe(false);
    expect(shouldForceDailyStudy({ ...base, completedOn: "2026-10-18" })).toBe(true);
    expect(shouldForceDailyStudy({ ...base, wordCount: 0 })).toBe(false);
    expect(shouldForceDailyStudy({ ...base, placementPending: true })).toBe(false);
  });
});

describe("buildDailyQueue", () => {
  it("reviews due words first, then fills with the newest learning words", () => {
    const queue = buildDailyQueue({
      due: [word("due", "2026-10-01T00:00:00Z"), word("phrase-due", "2026-10-02T00:00:00Z", "phrase")],
      learning: [
        word("old", "2026-09-01T00:00:00Z"),
        word("new", "2026-10-18T00:00:00Z"),
        word("due", "2026-10-01T00:00:00Z"),
      ],
      limit: 3,
    });
    expect(queue.map((item) => item.id)).toEqual(["due", "new", "old"]);
  });

  it("does not pull extra learning words once the cap is full of due cards", () => {
    const due = Array.from({ length: 4 }, (_, i) => word(`d${i}`, `2026-10-0${i + 1}T00:00:00Z`));
    const queue = buildDailyQueue({
      due,
      learning: [word("extra", "2026-10-19T00:00:00Z")],
      limit: 3,
    });
    expect(queue.map((item) => item.id)).toEqual(["d0", "d1", "d2"]);
  });
});
