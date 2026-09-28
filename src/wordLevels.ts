/**
 * Attribution for `src/data/word-levels.json`:
 * CEFR-J Vocabulary Profile 1.5, FrequencyWords (OpenSubtitles 2018),
 * ECDICT first sense. Format: [term, cefr, rank] or [term, cefr, rank, zh]
 *
 * CEFR ordered from easiest to hardest.
 */
export const CEFR_LEVELS = ["A1", "A2", "B1", "B2", "C1", "C2"] as const;
export type CefrLevel = (typeof CEFR_LEVELS)[number];

export const FREQ_BANDS = [1000, 3000, 5000, 10000, 20000] as const;
export type FreqBand = (typeof FREQ_BANDS)[number];

export type WordLevelEntry = {
  cefr: CefrLevel;
  rank: number;
  /** Optional bundled Chinese gloss (may be empty). */
  zh?: string;
  /** Set on a spaced alias: the hyphenated headword it was derived from. */
  term?: string;
};

export type DifficultyPrefs = {
  cefrLevel: CefrLevel;
  freqBand: FreqBand;
};

const CEFR_RANK: Record<CefrLevel, number> = {
  A1: 1,
  A2: 2,
  B1: 3,
  B2: 4,
  C1: 5,
  C2: 6,
};

type RawRow = [string, string, number] | [string, string, number, string];

const lexicon = new Map<string, WordLevelEntry>();
const phraseList: string[] = [];
let loadPromise: Promise<void> | null = null;
let loaded = false;

function ingestRows(rawLevels: RawRow[]) {
  lexicon.clear();
  phraseList.length = 0;
  for (const row of rawLevels) {
    // One malformed bundled row must not abort the whole lexicon: previously
    // normalizeKey(undefined) threw here, leaving every difficulty/underline
    // feature silently dead (callers swallow the load error). Validate shape
    // first, skip the bad row.
    if (!Array.isArray(row) || row.length < 3) continue;
    const [term, cefr, rank, zh] = row as unknown[];
    if (
      typeof term !== "string" ||
      typeof cefr !== "string" ||
      typeof rank !== "number" ||
      !Number.isFinite(rank)
    ) {
      continue;
    }
    if (!CEFR_RANK[cefr as CefrLevel]) continue;
    const key = normalizeKey(term);
    if (!key) continue;
    lexicon.set(key, {
      cefr: cefr as CefrLevel,
      rank,
      zh: typeof zh === "string" ? zh.trim() || undefined : undefined,
    });
    if (key.includes(" ")) phraseList.push(key);
  }
  phraseList.sort((a, b) => b.length - a.length);
  ingestAliases();
  loaded = true;
}

/** Lazy-load bundled CEFR + frequency lexicon (large JSON). */
export function ensureLexiconLoaded(): Promise<void> {
  if (loaded) return Promise.resolve();
  if (loadPromise) return loadPromise;
  loadPromise = import("./data/word-levels.json")
    .then((mod) => {
      ingestRows(mod.default as RawRow[]);
    })
    .catch((err) => {
      loadPromise = null;
      throw err;
    });
  return loadPromise;
}

export function normalizeKey(term: string): string {
  return term
    .trim()
    .toLowerCase()
    .replace(/[’']/g, "'")
    .replace(/\s+/g, " ");
}

export function isCefrLevel(v: string): v is CefrLevel {
  return (CEFR_LEVELS as readonly string[]).includes(v);
}

export function isFreqBand(v: number): v is FreqBand {
  return (FREQ_BANDS as readonly number[]).includes(v);
}

/** Light inflection fallbacks for lookup. */
export function lookupWord(term: string): WordLevelEntry | null {
  const key = normalizeKey(term);
  if (!key) return null;
  const direct = lexicon.get(key);
  if (direct) return direct;

  for (const c of lemmaCandidates(key)) {
    const hit = lexicon.get(c);
    if (hit) return hit;
  }
  return null;
}

/**
 * Rule-based base-form candidates for a surface form, most likely first.
 * Single implementation shared by lexicon lookup (`lookupWord`,
 * `findLemmaKey`) and the lookup lemma policy (`lemma.ts`): fix rules here
 * once instead of in two places. Kept small and conservative — only
 * structures unambiguous enough to be worth a base form.
 */
export function lemmaCandidates(word: string): string[] {
  const w = word.toLowerCase();
  if (w.length < 3 || w.includes(" ")) return [];
  const out: string[] = [];
  const push = (c: string) => {
    if (c && c !== w && c.length >= 2 && !out.includes(c)) out.push(c);
  };

  // possessives / contractions
  if (w.endsWith("'s")) push(w.slice(0, -2));
  if (w.endsWith("'")) push(w.slice(0, -1));

  // plurals
  if (w.endsWith("ies") && w.length > 3) push(`${w.slice(0, -3)}y`);
  if (w.endsWith("ves")) {
    push(`${w.slice(0, -3)}f`);
    push(`${w.slice(0, -3)}fe`);
  }
  if (w.endsWith("es")) {
    push(w.slice(0, -2));
    if (/(ch|sh|ss|x|z)es$/.test(w)) push(w.slice(0, -2));
  }
  if (w.endsWith("s") && !w.endsWith("ss") && !w.endsWith("us") && !w.endsWith("is")) {
    push(w.slice(0, -1));
  }
  if (w.endsWith("men")) push(`${w.slice(0, -3)}man`);

  // -ing (running → run, making → make)
  if (w.endsWith("ing") && w.length > 4) {
    const stem = w.slice(0, -3);
    if (stem.length >= 2 && stem[stem.length - 1] === stem[stem.length - 2]) {
      push(stem.slice(0, -1)); // doubled consonant
    }
    push(stem);
    push(`${stem}e`);
  }

  // -ed (walked → walk, studied → study, stopped → stop, liked → like)
  if (w.endsWith("ed") && w.length > 3) {
    const stem = w.slice(0, -2);
    if (w.endsWith("ied")) push(`${w.slice(0, -3)}y`);
    if (stem.length >= 2 && stem[stem.length - 1] === stem[stem.length - 2]) {
      push(stem.slice(0, -1));
    }
    push(stem);
    push(`${stem}e`);
  }

  // comparative / superlative (bigger → big, nicer → nice)
  if (w.endsWith("est") && w.length > 4) {
    const stem = w.slice(0, -3);
    if (stem.length >= 2 && stem[stem.length - 1] === stem[stem.length - 2]) {
      push(stem.slice(0, -1));
    }
    push(stem);
    push(`${stem}e`);
  }
  if (w.endsWith("er") && w.length > 3) {
    const stem = w.slice(0, -2);
    if (stem.length >= 2 && stem[stem.length - 1] === stem[stem.length - 2]) {
      push(stem.slice(0, -1));
    }
    push(stem);
    push(`${stem}e`);
  }

  // adverbs
  if (w.endsWith("ly") && w.length > 4) {
    push(w.slice(0, -2));
    if (w.endsWith("ily")) push(`${w.slice(0, -3)}y`);
    push(w.slice(0, -2) + "e");
  }

  return out;
}

/** CEFR steps above the learner's level; 0 when within reach. */
export function cefrStepsAbove(
  entry: WordLevelEntry,
  level: CefrLevel,
): number {
  return Math.max(0, CEFR_RANK[entry.cefr] - CEFR_RANK[level]);
}

/**
 * Hard if CEFR above user level OR frequency rank above user's known-band.
 * Words absent from the lexicon are not auto-underlined.
 */
export function isHardWord(
  entry: WordLevelEntry,
  prefs: DifficultyPrefs,
): boolean {
  const cefrHard = CEFR_RANK[entry.cefr] > CEFR_RANK[prefs.cefrLevel];
  const freqHard = entry.rank > prefs.freqBand;
  return cefrHard || freqHard;
}

/**
 * Super-hard = a tier above `isHardWord`, used to gate the automatic Chinese
 * gloss so only words genuinely out of reach get the extra annotation. A word
 * qualifies when either:
 *   • it is ≥2 CEFR steps above the learner's level (B1 user → C1 or higher), or
 *   • its frequency rank is past 2× the learner's known band (3000 → >6000).
 * Every super-hard word is also a hard word; the reverse does not hold.
 */
export function isSuperHardWord(
  entry: WordLevelEntry,
  prefs: DifficultyPrefs,
): boolean {
  return (
    cefrStepsAbove(entry, prefs.cefrLevel) >= 2 ||
    entry.rank > prefs.freqBand * 2
  );
}

/* ─────────────────────────────────────────────────────────────────
 * 词块 (lexical chunks): bundled src/data/chunks.json, mirroring the
 * word-levels pattern (lazy-imported read-only asset, matched in-memory).
 * Runtime row = [key, zh, en, type, [labels]]. Provenance + licenses in
 * src/data/chunks.SOURCES.md.
 * ───────────────────────────────────────────────────────────────── */
export type ChunkType = "idiom" | "phrasal" | "collocation" | "slang";
export type ChunkEntry = {
  /** Base key as stored in chunks.json. */
  key: string;
  /** Lemma-normalized, space-joined key — what text is matched against. */
  lemmaKey: string;
  type: ChunkType;
  zh?: string;
  en?: string;
  labels: string[];
};

const chunksByLemma = new Map<string, ChunkEntry>();
let chunksLoad: Promise<void> | null = null;
let chunksReady = false;

type ChunkRow = [string, string, string, string, string[]];
const CHUNK_TYPES: ReadonlySet<string> = new Set([
  "idiom",
  "phrasal",
  "collocation",
  "slang",
]);

/**
 * A chunk key spans ≥2 sub-words, where sub-words split on spaces OR hyphens.
 * So "put up with" (3) and "well-known" (2) qualify, but a bare word like
 * "bookworm" (1) does not. This is what lets hyphenated chunks like
 * `rubber-stamp` / `binge-watch` — a single TOKEN_RE token — still be chunks.
 */
function isChunkKey(k: string): boolean {
  return k.split(/[\s-]+/).filter(Boolean).length >= 2;
}

/**
 * Canonical base for chunk matching. Unlike `findLemmaKey` (which keeps a
 * surface form that is itself a lexicon headword), this prefers a reduced
 * base form even for inflections that are present in the lexicon — so text
 * "looking" and chunk "look" both canonicalize to "look" and match.
 */
function canonicalLemma(tok: string): string {
  for (const c of lemmaCandidates(tok)) {
    if (lexicon.has(c)) return c;
  }
  return tok;
}

/**
 * Space-joined canonical-lemma form of a normalized phrase key. Splits on BOTH
 * spaces and hyphens so `well-known` and `well known` collapse to the same key
 * — the basis for separator-agnostic (bidirectional) chunk matching.
 */
function lemmaPhraseKey(normKey: string): string {
  return normKey
    .split(/[\s-]+/)
    .filter(Boolean)
    .map((t) => canonicalLemma(t))
    .join(" ");
}

/** Lazy-load bundled chunks; awaits the lexicon first so lemma reduction works. */
export function ensureChunksLoaded(): Promise<void> {
  if (chunksReady) return Promise.resolve();
  if (chunksLoad) return chunksLoad;
  chunksLoad = ensureLexiconLoaded()
    .then(() => import("./data/chunks.json"))
    .then((mod) => {
      for (const row of mod.default as ChunkRow[]) {
        const [key, zh, en, type, labels] = row;
        const nk = normalizeKey(key);
        if (!nk || !isChunkKey(nk)) continue;
        const lemmaKey = lemmaPhraseKey(nk);
        chunksByLemma.set(lemmaKey, {
          key: nk,
          lemmaKey,
          type: (CHUNK_TYPES.has(type) ? type : "collocation") as ChunkType,
          zh: zh || undefined,
          en: en || undefined,
          labels: Array.isArray(labels) ? labels : [],
        });
      }
      chunksReady = true;
    })
    .catch((err) => {
      chunksLoad = null;
      throw err;
    });
  return chunksLoad;
}

/** Chunk lookup by surface or lemma form (empty until ensureChunksLoaded). */
export function lookupChunk(term: string): ChunkEntry | null {
  const nk = normalizeKey(term);
  if (!nk || !nk.includes(" ")) return null;
  return chunksByLemma.get(lemmaPhraseKey(nk)) ?? null;
}

export type AnnotatedSpan =
  | { type: "text"; text: string }
  | {
      type: "token";
      text: string;
      term: string;
      hard: boolean;
      /** A tier above `hard`; drives the automatic Chinese gloss. */
      superHard: boolean;
      learning: boolean;
      zh?: string;
      /** Omitted = single word. "chunk" = a lexical-chunk span (词块). */
      kind?: "word" | "chunk";
      chunkType?: ChunkType;
      labels?: string[];
    };

const TOKEN_RE = /[A-Za-z][A-Za-z'-]*|[^\sA-Za-z]+|\s+/g;

/** Surface text consumed by a span starting at token `start`. */
function joinToks(toks: { raw: string }[], start: number, count: number) {
  let raw = "";
  for (let k = start; k < start + count; k++) raw += toks[k]!.raw;
  return raw;
}

/**
 * Annotate plain text: longest lexical-chunk match (bundled chunks.json + the
 * learner's saved phrases, lemma-normalized), then the single-word CEFR pass.
 * Learning terms are marked even when not hard.
 */
export function annotateText(
  text: string,
  prefs: DifficultyPrefs,
  learningTerms: string[],
  knownTerms: string[] = [],
): AnnotatedSpan[] {
  const toks: { raw: string; isWord: boolean }[] = [];
  TOKEN_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = TOKEN_RE.exec(text)) !== null) {
    const raw = m[0];
    toks.push({ raw, isWord: /^[A-Za-z]/.test(raw) });
  }

  const learning = new Set<string>();
  const learningLemmas = new Set<string>();
  const known = new Set<string>();
  const knownLemmas = new Set<string>();
  for (const t of learningTerms) {
    const nk = normalizeKey(t);
    if (nk.length < 2) continue;
    learning.add(nk);
    if (nk.includes(" ")) learningLemmas.add(lemmaPhraseKey(nk));
  }
  for (const t of knownTerms) {
    const nk = normalizeKey(t);
    if (nk.length < 2) continue;
    known.add(nk);
    if (nk.includes(" ")) knownLemmas.add(lemmaPhraseKey(nk));
  }

  const spans: AnnotatedSpan[] = [];
  let i = 0;
  while (i < toks.length) {
    const t = toks[i]!;
    if (!t.isWord) {
      spans.push({ type: "text", text: t.raw });
      i += 1;
      continue;
    }

    const hit = matchChunkAt(toks, i, learningLemmas, knownLemmas);
    if (hit) {
      const knownHit = known.has(hit.rawKey) || knownLemmas.has(hit.lemmaKey);
      const learningHit =
        !knownHit &&
        (learning.has(hit.rawKey) || learningLemmas.has(hit.lemmaKey));
      spans.push({
        type: "token",
        text: joinToks(toks, i, hit.count),
        term: hit.entry?.key ?? hit.rawKey,
        hard: !knownHit,
        superHard: false,
        learning: learningHit,
        zh: hit.entry?.zh,
        kind: "chunk",
        chunkType: hit.entry?.type,
        labels: hit.entry?.labels,
      });
      i += hit.count;
      continue;
    }

    const phrase = matchLexiconPhraseAt(toks, i);
    if (phrase) {
      const headword = phrase.entry.term ?? phrase.key;
      const knownHit = known.has(phrase.key) || known.has(headword);
      spans.push({
        type: "token",
        text: joinToks(toks, i, phrase.count),
        term: headword,
        hard: !knownHit && isHardWord(phrase.entry, prefs),
        superHard: !knownHit && isSuperHardWord(phrase.entry, prefs),
        learning:
          !knownHit &&
          (learning.has(phrase.key) || learning.has(headword)),
        zh: phrase.entry.zh,
        kind: "word",
      });
      i += phrase.count;
      continue;
    }

    const key = normalizeKey(t.raw);
    const entry = lookupWord(key);
    const lemmaKey = findLemmaKey(key);
    const knownHit = known.has(key) || known.has(lemmaKey);
    spans.push({
      type: "token",
      text: t.raw,
      term: lemmaKey || key,
      hard: !knownHit && (entry ? isHardWord(entry, prefs) : false),
      superHard: !knownHit && (entry ? isSuperHardWord(entry, prefs) : false),
      learning: !knownHit && (learning.has(key) || learning.has(lemmaKey)),
      zh: entry?.zh,
      kind: "word",
    });
    i += 1;
  }

  return coalesceText(spans);
}

const MAX_CHUNK_TOKENS = 6;

/**
 * Flattened keys whose space-separated spelling is just as often a plain free
 * word combination as it is the compound — so the hyphen must be written by
 * the author for the compound reading to be claimed. Covers both directions:
 * a hyphenated chunk may not match spaced text, and a hyphenated headword gets
 * no spaced alias. `fast track` is a transit/bicycle lane as often as it is
 * 加速推进; `left hand` is a body part, never 惯用左手的.
 */
const AMBIGUOUS_SPACED_KEYS: ReadonlySet<string> = new Set([
  "also ran",
  "cold blood",
  "empty hand",
  "fast track",
  "know how",
  "left hand",
  "red hand",
  "right hand",
  "second hand",
  "single hand",
  // noun/adjective compounds whose unhyphenated spelling is the phrasal verb
  // instead: `take-off` 起飞 vs take off 脱下, `make-up` 化妆品 vs make up 编造.
  "back up",
  "check up",
  "close up",
  "cut off",
  "head up",
  "hard work",
  "make up",
  "mix up",
  "one man",
  "one night",
  "one time",
  "pick up",
  "set up",
  "stand up",
  "take off",
  "wake up",
]);

/** Closed-class words: they carry no meaning alone, so they can't anchor an alias. */
const CLOSED_CLASS: ReadonlySet<string> = new Set([
  "a", "an", "the", "and", "or", "but", "nor", "of", "to", "in", "on", "at",
  "by", "for", "from", "with", "as", "that", "this", "these", "those", "it",
  "its", "is", "are", "was", "were", "be", "been", "being", "he", "she",
  "they", "them", "their", "i", "you", "your", "we", "our", "my", "me", "us",
  "who", "which", "what", "when", "where", "how", "not", "no", "yes",
]);

/**
 * The unhyphenated spelling of a hyphenated headword, or null when it must not
 * be registered. CEFR-J stores `state-of-the-art` / `brother-in-law` hyphenated
 * while news prose often writes them apart; without an alias the spaced form
 * dissects into four easy words and nothing lights up. Requires two real
 * content words and no single-letter fragment, so stutters (`i-i`, `a-and`) and
 * interjections never become multi-word click targets.
 */
function spacedAliasOf(lexiconKey: string): string | null {
  if (!lexiconKey.includes("-")) return null;
  const parts = lexiconKey
    .split(/[\s-]+/)
    .filter(Boolean)
    .map((p) => canonicalLemma(p));
  if (parts.length < 2) return null;
  if (parts.some((p) => p.length < 2)) return null;
  if (parts.filter((p) => !CLOSED_CLASS.has(p)).length < 2) return null;
  const alias = parts.join(" ");
  return AMBIGUOUS_SPACED_KEYS.has(alias) ? null : alias;
}

/** Register spaced aliases for hyphenated headwords (after the lexicon is whole). */
function ingestAliases() {
  const found: [string, string, WordLevelEntry][] = [];
  for (const [key, entry] of lexicon) {
    const alias = spacedAliasOf(key);
    if (alias) found.push([alias, key, entry]);
  }
  for (const [alias, headword, entry] of found) {
    if (lexicon.has(alias)) continue;
    lexicon.set(alias, { ...entry, term: headword });
  }
}

/** True when any token in `words[0..n)` carries a hyphen in its surface form. */
function windowIsHyphenated(
  toks: { raw: string }[],
  words: number[],
  n: number,
): boolean {
  for (let a = 0; a < n; a++) if (toks[words[a]!]!.raw.includes("-")) return true;
  return false;
}

/**
 * Indices of the word tokens reachable from `start` — contiguous through
 * whitespace only (punctuation breaks the window), capped at `max` tokens.
 */
function wordWindow(
  toks: { raw: string; isWord: boolean }[],
  start: number,
  max: number,
): number[] {
  const words: number[] = [start];
  let j = start + 1;
  while (j < toks.length && words.length < max) {
    let k = j;
    let gap = true;
    while (k < toks.length && !toks[k]!.isWord) {
      if (!/^\s+$/.test(toks[k]!.raw)) {
        gap = false;
        break;
      }
      k += 1;
    }
    if (!gap || k >= toks.length) break;
    words.push(k);
    j = k + 1;
  }
  return words;
}

/** Lemma-normalized key of the first `n` tokens of `words`. */
function windowLemmaKey(
  toks: { raw: string }[],
  words: number[],
  n: number,
  splitHyphens: boolean,
): string {
  const subs: string[] = [];
  for (let a = 0; a < n; a++) {
    const norm = normalizeKey(toks[words[a]!]!.raw);
    for (const piece of splitHyphens ? norm.split("-") : [norm]) {
      if (piece) subs.push(canonicalLemma(piece));
    }
  }
  return subs.join(" ");
}

/**
 * Longest lexicon phrase starting at token `start`, reached through the spaced
 * aliases of hyphenated headwords (`state of the art` → `state-of-the-art`).
 * Only windows of ≥ 2 tokens are tried, and hyphens are not split here: a
 * hyphenated token already matches its own headword in the single-word pass.
 */
function matchLexiconPhraseAt(
  toks: { raw: string; isWord: boolean }[],
  start: number,
): { count: number; key: string; entry: WordLevelEntry } | null {
  const words = wordWindow(toks, start, MAX_CHUNK_TOKENS);
  for (let n = words.length; n >= 2; n -= 1) {
    const key = windowLemmaKey(toks, words, n, false);
    const entry = lexicon.get(key);
    if (entry) return { count: words[n - 1]! + 1 - start, key, entry };
  }
  return null;
}

/**
 * Longest lexical-chunk match starting at token `start`. Word tokens must be
 * contiguous through whitespace only (no punctuation between). Returns the
 * consumed token count + matched entry (null entry for a bare saved-phrase hit),
 * or null when nothing of length ≥ 2 matches.
 */
function matchChunkAt(
  toks: { raw: string; isWord: boolean }[],
  start: number,
  learningLemmas: Set<string>,
  knownLemmas: Set<string>,
): {
  count: number;
  entry: ChunkEntry | null;
  lemmaKey: string;
  rawKey: string;
} | null {
  if (
    chunksByLemma.size === 0 &&
    learningLemmas.size === 0 &&
    knownLemmas.size === 0
  ) {
    return null;
  }
  const words = wordWindow(toks, start, MAX_CHUNK_TOKENS);
  for (let n = Math.min(words.length, MAX_CHUNK_TOKENS); n >= 1; n -= 1) {
    const lemmaKey = windowLemmaKey(toks, words, n, true);
    if (
      AMBIGUOUS_SPACED_KEYS.has(lemmaKey) &&
      !windowIsHyphenated(toks, words, n)
    ) {
      continue;
    }
    const entry = chunksByLemma.get(lemmaKey);
    if (entry || learningLemmas.has(lemmaKey) || knownLemmas.has(lemmaKey)) {
      return {
        count: words[n - 1]! + 1 - start,
        entry: entry ?? null,
        lemmaKey,
        rawKey: lemmaKey,
      };
    }
  }
  return null;
}

function findLemmaKey(surface: string): string {
  if (lexicon.has(surface)) return surface;
  for (const c of lemmaCandidates(surface)) {
    if (lexicon.has(c)) return c;
  }
  return surface;
}

function coalesceText(spans: AnnotatedSpan[]): AnnotatedSpan[] {
  const out: AnnotatedSpan[] = [];
  for (const s of spans) {
    const last = out[out.length - 1];
    if (s.type === "text" && last?.type === "text") {
      last.text += s.text;
    } else {
      out.push(s);
    }
  }
  return out;
}

export type LexiconTerm = {
  term: string;
  cefr: CefrLevel;
  rank: number;
  zh?: string;
};

/** Snapshot of loaded lexicon (empty if not yet loaded). */
export function listLexiconTerms(): LexiconTerm[] {
  const out: LexiconTerm[] = [];
  for (const [term, e] of lexicon) {
    out.push({ term, cefr: e.cefr, rank: e.rank, zh: e.zh });
  }
  return out;
}
