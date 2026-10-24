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
 * Words absent from the lexicon are not auto-underlined here — the caller
 * applies the out-of-lexicon rule (see `isOutOfLexiconHard`).
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
 * Super-hard = a tier above `isHardWord`, and the sole gate for the automatic
 * Chinese gloss, so it must be *precise*: every word it admits gets a Chinese
 * annotation hanging under it in the reading column.
 *
 * CEFR is a difficulty scale, so it is the only criterion here — ≥2 steps above
 * the learner's level (B1 user → C1 or higher). Frequency rank is deliberately
 * NOT a second trigger: this lexicon's median rank is ~10400, so any band-
 * relative cut (`rank > band * 2`) admits a large slice of the A1/A2 vocabulary
 * (`sofa`, `software`, `t-shirt`) and swamps the CEFR signal. Rank stays in
 * `isHardWord`, where it only drives an underline.
 *
 * Every super-hard word is also a hard word; the reverse does not hold.
 */
export function isSuperHardWord(
  entry: WordLevelEntry,
  prefs: DifficultyPrefs,
): boolean {
  return cefrStepsAbove(entry, prefs.cefrLevel) >= 2;
}

/**
 * Minimum letters for a word missing from the lexicon to count as hard. The
 * lexicon is a ~21k headword list, so a rare long word (`disinflation`,
 * `sanctions`, `sprawling`) is genuinely out of reach and must not be skipped
 * silently. Short unknowns are usually inflections, typos or clitics, and
 * underlining those is pure noise.
 */
const MIN_OOV_HARD_CHARS = 7;

/**
 * A hyphenated token whose every part is itself a known headword is a
 * transparent compound (`state-of-the-art`, `well-known`, `two-month`) — the
 * learner can read it from the parts, so absence from the lexicon says nothing
 * about its difficulty. 26 of the 27 common news compounds behave this way.
 */
function isTransparentCompound(surface: string): boolean {
  if (!surface.includes("-")) return false;
  const parts = surface.split("-").filter(Boolean);
  if (parts.length < 2) return false;
  return parts.every((p) => lexicon.has(normalizeKey(p)));
}

/**
 * A word the lexicon does not carry is treated as hard. Absence from a CEFR
 * list *is* the evidence: the list is built from graded headwords, so anything
 * outside it is either advanced vocabulary or out of scope entirely, and both
 * are above a B1 reader. Proper nouns are excluded — a name is not a vocabulary
 * gap, and guessing a gloss for `Nvidia` would be wrong more often than right.
 */
export function isOutOfLexiconHard(surface: string): boolean {
  if (isTransparentCompound(surface)) return false;
  return surface.replace(/[^A-Za-z]/g, "").length >= MIN_OOV_HARD_CHARS;
}

/* ─────────────────────────────────────────────────────────────────
 * 专有名词 (proper nouns): surface detection + gloss disambiguation.
 *
 * The lexicon is keyed by lowercase headwords, so `Trump` resolves to
 * `trump` → 王牌，法宝 and `Turkey` to 火鸡，无用的家伙. Detecting the
 * proper noun on the *surface* lets us drop those common-noun senses,
 * while still surfacing the ~1400 entries whose gloss IS a name
 * (普京（人名）, 肯尼迪（姓氏）, 布鲁塞尔).
 * ───────────────────────────────────────────────────────────────── */

/** Gloss markers ECDICT uses for a person / place / work name. */
const NAME_GLOSS_MARKERS: readonly string[] = [
  "人名",
  "姓氏",
  "男子名",
  "女子名",
  "男人名",
  "女性名",
  "男名",
  "女名",
  "姓名",
  "姓氏名",
  "姓",
  "地名",
  "城市名",
  "市名",
  "郡名",
  "神话人物",
  "先知",
  "篮球队名",
  "球队名",
  "俱乐部",
  "公司名",
  "电影名",
  "电视剧名",
  "歌手",
  "作家",
  "诗人",
  "总统",
  "国王",
  "女王",
  "公爵",
  "伯爵",
  "男爵",
  "皇后",
  "公主",
  "汉语拼音",
];

/** True when the gloss explicitly tags the entry as a name. */
function isNameGloss(zh: string | undefined): boolean {
  if (!zh) return false;
  return NAME_GLOSS_MARKERS.some((m) => zh.includes(m));
}

/**
 * Sense separators ECDICT uses inside one gloss cell. Their presence means the
 * cell enumerates several distinct words of speech, so the first item is a
 * common-noun reading and unsafe to show for a capitalized token.
 */
const SENSE_SPLIT_RE = /[，,；;、]/;

/** Abbreviations whose trailing dot does NOT end a sentence (`Mr. Smith`). */
const TITLE_ABBREV_RE = /^(mr|mrs|ms|dr|prof|st|jr|sr|vs|etc|no|fig|inc|ltd|co)$/;

/** Text that ends a sentence or clause: the next capital is grammatical. */
const SENTENCE_END_RE = /[.!?…。！？:;：；]["'’”)\]]*$/;

/**
 * Punctuation that *opens* a clause: a quote, a bracket, a dash or an ellipsis.
 * `,"` / `("` / `—` are all tokens the tokenizer hands us whole, and a capital
 * letter right after them is still sentence-cased (`"The company said`), so
 * they must not be read as proper-noun evidence. Requires the token to contain
 * no letters or digits, so `GDP` or `18%` never qualify.
 */
const CLAUSE_OPEN_PUNCT_RE = /^[^\p{L}\p{N}]*["'“”‘’«»([{—–…][^\p{L}\p{N}]*$/u;

/**
 * True when the token at `index` opens a sentence or clause, so its
 * capitalization is conventional and carries no proper-noun evidence.
 *
 * A colon/semicolon counts: `In short: Officials said` capitalizes a common
 * noun, and treating it as a name would strip a correct gloss. An
 * abbreviation's dot does not count, so `Mr. Smith` still yields a name — the
 * tokenizer splits `Mr` and `.` into separate tokens, hence the two-step look.
 * An opening quote / bracket / dash counts too, else every quoted sentence
 * starts with a "name" (`"The Fed will hold"` → 那 under `The`).
 */
function isSentenceInitial(
  tokens: { raw: string; isWord: boolean }[],
  index: number,
): boolean {
  let k = index - 1;
  while (k >= 0 && /^\s+$/.test(tokens[k]!.raw)) k -= 1;
  if (k < 0) return true;
  const prev = tokens[k]!;
  if (
    !prev.isWord &&
    prev.raw.endsWith(".") &&
    k > 0 &&
    tokens[k - 1]!.isWord &&
    isAbbreviation(tokens[k - 1]!.raw)
  ) {
    return false;
  }
  if (prev.isWord) return false;
  return SENTENCE_END_RE.test(prev.raw) || CLAUSE_OPEN_PUNCT_RE.test(prev.raw);
}

/** `Mr` / `Dr` / `U` — a dot after one of these is an abbreviation mark. */
function isAbbreviation(raw: string): boolean {
  const w = raw.replace(/\.$/, "").toLowerCase();
  if (TITLE_ABBREV_RE.test(w)) return true;
  // Single letter, or an initialism like `U.S` split by the tokenizer.
  return w.length === 1 || /^[a-z](?:\.[a-z])+$/.test(w);
}

/** Titles and courtesy forms; capitalized but never proper nouns. */
const HONORIFIC_RE =
  /^(mr|mrs|ms|miss|dr|prof|st|ste|sir|madam|lord|lady|president|senator|governor|rev|hon|jr|sr|vs|etc|co|inc|ltd|corp|dept|est|fig|no|vol|ed|pp|al)$/i;

function isHonorific(raw: string): boolean {
  return HONORIFIC_RE.test(raw.replace(/\.$/, ""));
}

/**
 * Gloss for a title. The lexicon stores these correctly (`Mr` → 先生,
 * `Ms` → 美国微软公司 is the one bad row), so trust it rather than inventing
 * a reading; an entry whose gloss looks like a company or a stray technical
 * sense is dropped instead, because a wrong gloss under `Ms` is worse than
 * none.
 */
function honorificGloss(entry: WordLevelEntry | null): string | undefined {
  const zh = entry?.zh;
  if (!zh) return undefined;
  if (/公司|组织|局|部\b/.test(zh)) return undefined;
  return SENSE_SPLIT_RE.test(zh) ? zh.split(SENSE_SPLIT_RE)[0] : zh;
}

/**
 * Closed-class words: they carry no meaning alone, so they can never anchor a
 * chunk alias — and they never capitalize for naming either (`The Times` is a
 * name because of `Times`, not because of the article). Treating them as names
 * is what put 那 under `The` in `"The company said…"`.
 */
const CLOSED_CLASS: ReadonlySet<string> = new Set([
  "a", "an", "the", "and", "or", "but", "nor", "of", "to", "in", "on", "at",
  "by", "for", "from", "with", "as", "that", "this", "these", "those", "it",
  "its", "is", "are", "was", "were", "be", "been", "being", "he", "she",
  "they", "them", "their", "i", "you", "your", "we", "our", "my", "me", "us",
  "who", "which", "what", "when", "where", "how", "not", "no", "yes",
]);

/**
 * True when a token's *surface* marks it as a proper noun: an initialism
 * (NATO, GDP, AI — the only capitalized form it ever has), or a capitalized
 * word that is not sentence-initial. The latter is a heuristic, and the
 * accepted cost is a capitalized common word in mid-sentence position being
 * treated as a name; in exchange `Trump`/`Turkey` stop being glossed with a
 * common-noun sense.
 *
 * Closed-class words are excluded: `The` / `A` capitalize by grammar, and the
 * lexicon's single-cell gloss for them (`the` → 那) would otherwise be read as
 * a name reading and hung under the word.
 */
export function isProperNounSurface(
  raw: string,
  atSentenceStart: boolean,
): boolean {
  if (!/^[A-Z]/.test(raw)) return false;
  if (isHonorific(raw)) return false;
  if (CLOSED_CLASS.has(normalizeKey(raw))) return false;
  if (isInitialism(raw)) return true;
  return !atSentenceStart;
}

function isInitialism(raw: string): boolean {
  const text = raw.replace(/['’]s$/, "");
  if (text.length < 2) return false;
  return /^[A-Z][A-Z0-9.\-&/]*$/.test(text);
}

/**
 * The gloss to attach to a token detected as a proper noun.
 *
 * Kept when the lexicon itself tags it as a name (普京（人名）), or when the
 * gloss is unambiguous — a single cell with no sense separators reads as one
 * dedicated meaning, which is how cities and countries are stored (布鲁塞尔,
 * 伦敦). Suppressed when the cell enumerates common-noun senses (王牌，法宝 /
 * 火鸡，无用的家伙): there the entry is an ordinary word that merely collides
 * with a name, so showing it under a capitalized token is simply wrong.
 */
export function properNounGloss(entry: WordLevelEntry | null): string | undefined {
  const zh = entry?.zh;
  if (!zh) return undefined;
  if (isNameGloss(zh)) return zh;
  return SENSE_SPLIT_RE.test(zh) ? undefined : zh;
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
      /** Surface-detected proper noun; see `isProperNounSurface`. */
      proper?: boolean;
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
  for (const t of learningTerms) {
    const nk = normalizeKey(t);
    if (nk.length < 2) continue;
    learning.add(nk);
    if (nk.includes(" ")) learningLemmas.add(lemmaPhraseKey(nk));
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

    const hit = matchChunkAt(toks, i, learningLemmas);
    if (hit) {
      const learningHit =
        learning.has(hit.rawKey) || learningLemmas.has(hit.lemmaKey);
      spans.push({
        type: "token",
        text: joinToks(toks, i, hit.count),
        term: hit.entry?.key ?? hit.rawKey,
        hard: true,
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
      const proper = isProperNounSurface(t.raw, isSentenceInitial(toks, i));
      const hard = isHardWord(phrase.entry, prefs);
      spans.push({
        type: "token",
        text: joinToks(toks, i, phrase.count),
        term: headword,
        hard,
        superHard: !proper && isSuperHardWord(phrase.entry, prefs),
        learning:
          learning.has(phrase.key) || learning.has(headword),
        zh: proper
          ? isHardWord(phrase.entry, prefs)
            ? properNounGloss(phrase.entry)
            : undefined
          : phrase.entry.zh,
        kind: "word",
        proper,
      });
      i += phrase.count;
      continue;
    }

    const key = normalizeKey(t.raw);
    const entry = lookupWord(key);
    const lemmaKey = findLemmaKey(key);
    // Three surface classes, because a lowercase key alone cannot tell them
    // apart: an honorific (`Ms` → 女士, never 美国微软公司), a proper noun
    // (`Trump`, glossed only from its name reading), or an ordinary word.
    const honorific = isHonorific(t.raw);
    const proper = !honorific && isProperNounSurface(t.raw, isSentenceInitial(toks, i));
    const hard = entry
      ? isHardWord(entry, prefs)
      : !proper && isOutOfLexiconHard(t.raw);
    spans.push({
      type: "token",
      text: t.raw,
      term: lemmaKey || key,
      // A name is not a vocabulary gap: `Nvidia`/`Zelensky` stay clickable but
      // are neither underlined nor glossed. A name the lexicon *does* tag as
      // one (普京（人名）) keeps its gloss only when the word is above the
      // learner — an in-level word (`Better`, `London`) is not marked, capital
      // or not. An honorific is conventional text, so it is never difficulty.
      hard: honorific ? false : hard,
      superHard:
        !proper &&
        !honorific &&
        !!entry &&
        isSuperHardWord(entry, prefs),
      learning: learning.has(key) || learning.has(lemmaKey),
      zh: honorific
        ? honorificGloss(entry)
        : proper
          ? entry && isHardWord(entry, prefs)
            ? properNounGloss(entry)
            : undefined
          : entry?.zh,
      kind: "word",
      proper,
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
): {
  count: number;
  entry: ChunkEntry | null;
  lemmaKey: string;
  rawKey: string;
} | null {
  if (chunksByLemma.size === 0 && learningLemmas.size === 0) {
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
    if (entry || learningLemmas.has(lemmaKey)) {
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
