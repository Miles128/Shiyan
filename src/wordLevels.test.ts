import { beforeAll, describe, expect, it } from "vitest";
import {
  annotateText,
  ensureChunksLoaded,
  ensureLexiconLoaded,
  isHardWord,
  isOutOfLexiconHard,
  isProperNounSurface,
  isSuperHardWord,
  lookupChunk,
  lookupWord,
  properNounGloss,
  type AnnotatedSpan,
  type DifficultyPrefs,
} from "./wordLevels";

const prefsB1: DifficultyPrefs = { cefrLevel: "B1", freqBand: 3000 };
const prefsC2: DifficultyPrefs = { cefrLevel: "C2", freqBand: 20000 };

beforeAll(async () => {
  await ensureLexiconLoaded();
});

function plain(spans: AnnotatedSpan[]): string {
  return spans.map((s) => s.text).join("");
}

describe("isHardWord", () => {
  it("flags words above the freq band", () => {
    expect(isHardWord({ cefr: "B1", rank: 9000 }, prefsB1)).toBe(true);
  });

  it("flags words above the CEFR level", () => {
    expect(isHardWord({ cefr: "C2", rank: 500 }, prefsB1)).toBe(true);
  });

  it("keeps easy words unflagged", () => {
    expect(isHardWord({ cefr: "A2", rank: 800 }, prefsB1)).toBe(false);
  });
});

describe("isSuperHardWord", () => {
  // B1 user (rank 3 on CEFR ladder): 2 steps up = C1 (rank 5).
  it("fires when the word is ≥2 CEFR steps above the learner", () => {
    expect(isSuperHardWord({ cefr: "C1", rank: 500 }, prefsB1)).toBe(true);
    expect(isSuperHardWord({ cefr: "C2", rank: 500 }, prefsB1)).toBe(true);
  });

  it("stays silent at only 1 CEFR step above", () => {
    // B2 for a B1 user is hard but not super-hard.
    expect(isSuperHardWord({ cefr: "B2", rank: 100 }, prefsB1)).toBe(false);
    // ...however rare it is: rank alone must not buy a gloss.
    expect(isSuperHardWord({ cefr: "B2", rank: 49000 }, prefsB1)).toBe(false);
  });

  // The regression that motivated the pure-CEFR rule: this lexicon's median
  // rank is ~10400, so a band-relative cut flagged most of A1/A2 as super-hard
  // (`sofa` 6147, `software` 6138, `t-shirt` 6128) and glossed them.
  it("ignores frequency rank entirely", () => {
    expect(isSuperHardWord({ cefr: "A2", rank: 6500 }, prefsB1)).toBe(false);
    expect(isSuperHardWord({ cefr: "A1", rank: 21000 }, prefsB1)).toBe(false);
    for (const w of ["sofa", "software", "t-shirt"]) {
      const entry = lookupWord(w)!;
      expect(isHardWord(entry, prefsB1)).toBe(true); // still underlined
      expect(isSuperHardWord(entry, prefsB1)).toBe(false); // but never glossed
    }
  });

  it("is a strict subset of isHardWord", () => {
    // Any word superHard must also be hard.
    const samples = [
      { cefr: "C1" as const, rank: 500 },
      { cefr: "A2" as const, rank: 6500 },
      { cefr: "B2" as const, rank: 100 },
      { cefr: "A1" as const, rank: 100 },
    ];
    for (const s of samples) {
      if (isSuperHardWord(s, prefsB1)) expect(isHardWord(s, prefsB1)).toBe(true);
    }
  });

  it("keeps the ceiling prefs (C2 / 20000) essentially superHard-free", () => {
    // Realistic news word: C1 rank 4500 → still within reach of C2/20k user.
    expect(isSuperHardWord({ cefr: "C1", rank: 4500 }, prefsC2)).toBe(false);
    // Nothing is 2 CEFR steps above C2, so the ceiling user gets no glosses.
    expect(isSuperHardWord({ cefr: "C2", rank: 49000 }, prefsC2)).toBe(false);
  });
});

describe("isOutOfLexiconHard", () => {
  it("treats a missing word as hard once it is long enough to be real", () => {
    // Advanced news vocabulary absent from the 21k CEFR headword list.
    for (const w of ["disinflation", "sprawling", "sanctions", "indiscriminate"]) {
      expect(lookupWord(w)).toBeNull();
      expect(isOutOfLexiconHard(w)).toBe(true);
    }
  });

  it("ignores short unknowns (inflections, typos, clitics)", () => {
    expect(isOutOfLexiconHard("eased")).toBe(false);
    expect(isOutOfLexiconHard("reign")).toBe(false);
  });

  it("leaves a transparent compound alone — its parts are all known", () => {
    // The learner reads `left-wing` from its parts, so the missing headword
    // entry says nothing about difficulty.
    for (const w of ["two-month", "left-wing", "open-source"]) {
      expect(lookupWord(w)).toBeNull();
      expect(isOutOfLexiconHard(w)).toBe(false);
    }
    // ...but a compound with an unknown part still counts.
    expect(isOutOfLexiconHard("multi-year")).toBe(true);
  });
});

describe("proper nouns (surface detection)", () => {
  it("flags an initialism whatever its position", () => {
    expect(isProperNounSurface("NATO", true)).toBe(true);
    expect(isProperNounSurface("GDP", false)).toBe(true);
  });

  it("flags a capitalized non-sentence-initial word", () => {
    expect(isProperNounSurface("Brussels", false)).toBe(true);
  });

  it("does not flag sentence-initial capitalization", () => {
    // "Brussels" opening a sentence carries no evidence either way.
    expect(isProperNounSurface("Brussels", true)).toBe(false);
  });

  it("never flags a lowercase word", () => {
    expect(isProperNounSurface("brussels", false)).toBe(false);
  });

  it("keeps a name-tagged gloss and drops a common-noun one", () => {
    // 普京（人名） is the name reading — keep it.
    expect(properNounGloss(lookupWord("putin"))).toContain("普京");
    expect(properNounGloss(lookupWord("kennedy"))).toContain("肯尼迪");
    // 王牌，法宝 / 火鸡，无用的家伙 are ordinary words that collide with a
    // name — showing them under a capitalized token is simply wrong.
    expect(properNounGloss(lookupWord("trump"))).toBeUndefined();
    expect(properNounGloss(lookupWord("turkey"))).toBeUndefined();
    // Single-sense city entries are unambiguous, so they stay.
    expect(properNounGloss(lookupWord("brussels"))).toBe("布鲁塞尔");
  });

  it("suppresses the wrong gloss on a real proper noun in running text", () => {
    const spans = annotateText("Officials in Brussels met Donald Trump.", prefsB1, []);
    const trump = spans.find((s) => s.type === "token" && s.text === "Trump");
    expect(trump).toBeDefined();
    if (trump?.type === "token") {
      expect(trump.proper).toBe(true);
      expect(trump.zh).toBeUndefined();
    }
  });

  it("keeps a known name gloss and marks it hard", () => {
    const spans = annotateText("He denied it, said Vladimir Putin.", prefsB1, []);
    const putin = spans.find((s) => s.type === "token" && s.text === "Putin");
    expect(putin).toBeDefined();
    if (putin?.type === "token") {
      expect(putin.proper).toBe(true);
      expect(putin.zh).toContain("普京");
      expect(putin.hard).toBe(true);
    }
  });

  it("does not gloss a capitalized word the learner already has", () => {
    // `Better` / `Years` / `London` are inside a B1 band, so the capital is
    // not a reason to hang 较好的 / 年代 / 伦敦 under them.
    const spans = annotateText(
      "The plan promises Better Years in London.",
      prefsB1,
      [],
    );
    for (const word of ["Better", "Years", "London"]) {
      const span = spans.find((s) => s.type === "token" && s.text === word);
      expect(span).toMatchObject({
        type: "token",
        proper: true,
        hard: false,
        zh: undefined,
      });
    }
  });

  it("keeps a place gloss when the name is above the learner", () => {
    const prefsA1: DifficultyPrefs = { cefrLevel: "A1", freqBand: 1000 };
    const spans = annotateText("She met him in London.", prefsA1, []);
    const london = spans.find((s) => s.type === "token" && s.text === "London");
    expect(london).toMatchObject({
      type: "token",
      proper: true,
      hard: true,
      zh: "伦敦",
    });
  });

  it("leaves an unknown name unhardened — a name is not a vocabulary gap", () => {
    const spans = annotateText("Nvidia and OpenAI shipped the model.", prefsB1, []);
    for (const name of ["Nvidia", "OpenAI"]) {
      const span = spans.find((s) => s.type === "token" && s.text === name);
      expect(span).toBeDefined();
      if (span?.type === "token") {
        expect(span.hard).toBe(false);
        expect(span.superHard).toBe(false);
        expect(span.zh).toBeUndefined();
      }
    }
  });

  it("still underlines an out-of-lexicon common word next to a name", () => {
    const text = "Nvidia blamed disinflation and sprawling sanctions.";
    const spans = annotateText(text, prefsB1, []);
    for (const word of ["disinflation", "sprawling", "sanctions"]) {
      const span = spans.find((s) => s.type === "token" && s.text === word);
      expect(span).toBeDefined();
      if (span?.type === "token") {
        expect(span.hard).toBe(true);
        expect(span.proper).toBe(false);
      }
    }
  });

  it("never treats a closed-class word as a name", () => {
    // `the` has a single-cell gloss (那), so a name reading would hang it
    // under the article. It capitalizes for grammar in every case below.
    for (const text of [
      '"The company said it would cut jobs.',
      "Growth slowed — The cause was tariffs.",
      "(The figures are provisional.)",
      "He told The Times and The Guardian.",
    ]) {
      const the = annotateText(text, prefsB1, []).find(
        (s) => s.type === "token" && s.text === "The",
      );
      expect(the).toBeDefined();
      if (the?.type === "token") {
        expect(the.proper).toBe(false);
        expect(the.hard).toBe(false);
      }
    }
    const a = annotateText('She said "A deal is close."', prefsB1, []).find(
      (s) => s.type === "token" && s.text === "A",
    );
    if (a?.type === "token") expect(a.proper).toBe(false);
  });

  it("reads an opening quote / dash as a clause start", () => {
    // Previously the quoted capital looked like a name, which also cost an
    // out-of-lexicon word its underline (a name is never a vocabulary gap).
    const spans = annotateText('"Disinflation is coming," he said.', prefsB1, []);
    const word = spans.find(
      (s) => s.type === "token" && s.text === "Disinflation",
    );
    expect(word).toBeDefined();
    if (word?.type === "token") {
      expect(word.proper).toBe(false);
      expect(word.hard).toBe(true);
      expect(word.zh).toBeUndefined(); // no dictionary row: still no wrong gloss
    }
  });

  it("does not read a clause boundary as a name", () => {
    // A capitalized common noun after `:` / `;` is conventional, and its
    // gloss is correct — treating it as a name would strip it.
    const spans = annotateText("In short: Officials said; Sovereign bonds fell.", prefsB1, []);
    for (const word of ["Officials", "Sovereign"]) {
      const span = spans.find((s) => s.type === "token" && s.text === word);
      expect(span).toBeDefined();
      if (span?.type === "token") {
        expect(span.proper).toBe(false);
        expect(span.zh).toBeTruthy();
      }
    }
  });

  it("sees through an abbreviation dot: Mr. Smith is still a name", () => {
    const spans = annotateText("Mr. Smith met Dr. Biden.", prefsB1, []);
    const smith = spans.find((s) => s.type === "token" && s.text === "Smith");
    expect(smith).toBeDefined();
    if (smith?.type === "token") expect(smith.proper).toBe(true);
  });

  it("treats an honorific as conventional text, never as a name", () => {
    // `Ms` must not resolve to 美国微软公司, and a title is not a vocab gap.
    const spans = annotateText("Mr. Smith met Ms. O'Neill. Dr. Biden spoke.", prefsB1, []);
    for (const title of ["Mr", "Ms", "Dr"]) {
      const span = spans.find((s) => s.type === "token" && s.text === title);
      expect(span).toBeDefined();
      if (span?.type === "token") {
        expect(span.proper).toBe(false);
        expect(span.hard).toBe(false);
        expect(span.zh ?? "").not.toContain("微软");
      }
    }
    // `Mr` / `Dr` do carry a correct gloss.
    expect(spans.find((s) => s.type === "token" && s.text === "Dr")).toMatchObject({
      zh: "博士",
    });
  });
});

describe("annotateText", () => {
  it("round-trips to the original text", () => {
    const text = "The cat sat on the mat while prey walked by.";
    expect(plain(annotateText(text, prefsB1, []))).toBe(text);
  });

  it("marks learning terms as token spans", () => {
    const spans = annotateText("The cat sat on the mat.", prefsB1, ["cat"]);
    const cat = spans.find((s) => s.type === "token" && s.text === "cat");
    expect(cat).toBeDefined();
    if (cat?.type === "token") {
      expect(cat.learning).toBe(true);
    }
  });

  it("marks hard words under stricter prefs", () => {
    const spans = annotateText("A warden watched the prey.", prefsB1, []);
    const prey = spans.find((s) => s.type === "token" && s.text === "prey");
    expect(prey).toBeDefined();
    if (prey?.type === "token") {
      expect(prey.hard).toBe(true);
    }
  });

  it("emits a token for every word so any word can be clicked", () => {
    const spans = annotateText("The cat sat.", prefsC2, []);
    const words = spans.filter((s) => s.type === "token");
    expect(words.map((s) => (s.type === "token" ? s.text : ""))).toEqual([
      "The",
      "cat",
      "sat",
    ]);
    expect(words.every((s) => s.type === "token" && !s.hard)).toBe(true);
  });

  it("stricter prefs mark at least as many hard tokens", () => {
    const text = "The cat sat on the mat while the warden watched the prey.";
    const hardCount = (prefs: DifficultyPrefs) =>
      annotateText(text, prefs, []).filter((s) => s.type === "token" && s.hard)
        .length;
    const b1 = hardCount(prefsB1);
    const c2 = hardCount(prefsC2);
    expect(b1).toBeGreaterThan(0);
    expect(b1).toBeGreaterThanOrEqual(c2);
  });
});

function chunkSpan(spans: AnnotatedSpan[], term: string) {
  return spans.find(
    (s) => s.type === "token" && s.kind === "chunk" && s.term === term,
  );
}

describe("lexical chunks (chunks.json)", () => {
  beforeAll(async () => {
    await ensureChunksLoaded();
  });

  it("lookupChunk resolves a bundled phrasal verb", () => {
    expect(lookupChunk("put up with")?.type).toBe("phrasal");
  });

  it("underlines a multi-token chunk and keeps its surface span", () => {
    const spans = annotateText("I learned to put up with noise.", prefsC2, []);
    const hit = chunkSpan(spans, "put up with");
    expect(hit).toBeDefined();
    if (hit?.type === "token") {
      expect(hit.hard).toBe(true);
      expect(hit.text).toBe("put up with");
    }
  });

  it("matches inflected forms (looking → look forward to)", () => {
    const spans = annotateText("We are looking forward to it.", prefsC2, []);
    const hit = chunkSpan(spans, "look forward to");
    expect(hit).toBeDefined();
    if (hit?.type === "token") expect(hit.text).toBe("looking forward to");
  });

  it("marks a saved learning chunk", () => {
    const spans = annotateText("You must bear in mind.", prefsC2, [
      "bear in mind",
    ]);
    const hit = chunkSpan(spans, "bear in mind");
    if (hit?.type === "token") expect(hit.learning).toBe(true);
  });

  it("still round-trips to the original text with chunks active", () => {
    const text = "They decided to cut corners, and it backfired.";
    expect(plain(annotateText(text, prefsC2, []))).toBe(text);
  });

  it("matches a hyphenated single-token chunk (rubber-stamp)", () => {
    const spans = annotateText("It was just a rubber-stamp approval.", prefsC2, []);
    const hit = chunkSpan(spans, "rubber-stamp");
    expect(hit).toBeDefined();
    if (hit?.type === "token") expect(hit.text).toBe("rubber-stamp");
  });

  it("matches a hyphenated chunk written with spaces (bidirectional)", () => {
    const spans = annotateText("The approval was a rubber stamp formality.", prefsC2, []);
    const hit = chunkSpan(spans, "rubber-stamp");
    expect(hit).toBeDefined();
    if (hit?.type === "token") expect(hit.text).toBe("rubber stamp");
  });

  it("matches an ambiguous hyphenated chunk when the text hyphenates it", () => {
    const spans = annotateText("The scheme was fast-track for graduates.", prefsC2, []);
    const hit = chunkSpan(spans, "fast-track");
    expect(hit).toBeDefined();
    if (hit?.type === "token") expect(hit.text).toBe("fast-track");
  });

  it("does not match an ambiguous chunk written as free words", () => {
    const text = "Cyclists used a fast track near the river.";
    const spans = annotateText(text, prefsC2, []);
    expect(chunkSpan(spans, "fast-track")).toBeUndefined();
    expect(plain(spans)).toBe(text);
  });
});
describe("spaced aliases of hyphenated headwords", () => {
  function wordSpan(spans: AnnotatedSpan[], term: string) {
    return spans.find(
      (s) => s.type === "token" && s.kind === "word" && s.term === term,
    );
  }

  it("annotates the unhyphenated spelling as its headword", () => {
    const spans = annotateText("a world class chip", prefsB1, []);
    const hit = wordSpan(spans, "world-class");
    expect(hit).toBeDefined();
    if (hit?.type === "token") {
      expect(hit.text).toBe("world class");
      expect(hit.hard).toBe(true);
      expect(hit.zh).toBeTruthy();
    }
  });

  it("resolves the spaced form through the dictionary lookup", () => {
    expect(lookupWord("world class")?.term).toBe("world-class");
  });

  it("leaves a free word combination to the single-word pass", () => {
    const text = "He raised his left hand and said it was hard work.";
    const spans = annotateText(text, prefsB1, []);
    expect(wordSpan(spans, "left-handed")).toBeUndefined();
    expect(wordSpan(spans, "hard-working")).toBeUndefined();
  });

  it("does not claim a spaced phrasal verb for the noun compound", () => {
    const text = "She had to take off her shoes.";
    expect(wordSpan(annotateText(text, prefsB1, []), "take-off")).toBeUndefined();
  });

  it("round-trips a paragraph full of compounds", () => {
    const text =
      "The long term, up to date deal was world class, so it won win support.";
    expect(plain(annotateText(text, prefsB1, []))).toBe(text);
  });
});

describe("hyphenated news compounds in the chunk table", () => {
  beforeAll(async () => {
    await ensureChunksLoaded();
  });

  it("matches the spaced spelling of a political compound", () => {
    const spans = annotateText("The far right took a hard line.", prefsC2, []);
    expect(chunkSpan(spans, "far-right")).toBeDefined();
    expect(chunkSpan(spans, "hard-line")).toBeDefined();
  });

  it("leaves the phrasal verb alone when only the noun is hyphenated", () => {
    const text = "Troops cut off the highway.";
    const spans = annotateText(text, prefsC2, []);
    expect(chunkSpan(spans, "cut-off")).toBeUndefined();
    expect(chunkSpan(annotateText("the water cut-off lasted hours.", prefsC2, []), "cut-off")).toBeDefined();
  });
});
