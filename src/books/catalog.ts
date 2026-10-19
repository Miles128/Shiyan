/** Curated Project Gutenberg shelf. Public-domain English only. */

export const BOOK_INTERESTS = [
  { id: "adventure", label: "冒险" },
  { id: "children", label: "童话" },
  { id: "society", label: "社会" },
  { id: "mystery", label: "推理" },
  { id: "science", label: "科学" },
  { id: "history", label: "历史" },
  { id: "philosophy", label: "思想" },
  { id: "romance", label: "情感" },
  { id: "nature", label: "自然" },
  { id: "politics", label: "政治" },
] as const;

export type BookInterestId = (typeof BOOK_INTERESTS)[number]["id"];

export const BOOK_LENGTHS = [
  { id: "short", label: "短篇", hint: "4 万词以内" },
  { id: "medium", label: "中篇", hint: "4–8 万词" },
  { id: "long", label: "长篇", hint: "8–14 万词" },
  { id: "epic", label: "巨著", hint: "14 万词以上" },
] as const;

export type BookLengthId = (typeof BOOK_LENGTHS)[number]["id"];

export type BookCefr = "A2" | "B1" | "B2" | "C1" | "C2";

export type CatalogBook = {
  id: string;
  title: string;
  author: string;
  /** Gutenberg ebook number. Plain text is fetched from the cache URL. */
  gutenbergId: string;
  cefr: BookCefr;
  /** Approximate English word count, used only for the length band. */
  words: number;
  interests: BookInterestId[];
  summaryZh: string;
  /**
   * Distinctive words that actually occur in the book. Today's picks match
   * these against the learner's recent words and the vocab bank.
   */
  terms: string[];
};

export function gutenbergTextUrl(gutenbergId: string): string {
  return `https://www.gutenberg.org/cache/epub/${gutenbergId}/pg${gutenbergId}.txt`;
}

export function bookLength(words: number): BookLengthId {
  if (words < 40_000) return "short";
  if (words < 80_000) return "medium";
  if (words < 140_000) return "long";
  return "epic";
}

export function bookLengthLabel(words: number): string {
  return BOOK_LENGTHS.find((band) => band.id === bookLength(words))?.label ?? "";
}

/** "约 12.2 万词" / "约 9800 词". */
export function formatWordCount(words: number): string {
  if (!Number.isFinite(words) || words <= 0) return "";
  if (words >= 10_000) {
    const text = (words / 10_000).toFixed(1).replace(/\.0$/, "");
    return `约 ${text} 万词`;
  }
  return `约 ${Math.round(words)} 词`;
}

export function interestLabel(id: string): string {
  return BOOK_INTERESTS.find((item) => item.id === id)?.label ?? id;
}

export const BOOK_CATALOG: CatalogBook[] = [
  {
    id: "alice",
    title: "Alice's Adventures in Wonderland",
    author: "Lewis Carroll",
    gutenbergId: "11",
    cefr: "A2",
    words: 26600,
    interests: ["children", "adventure"],
    summaryZh: "女孩爱丽丝掉进兔子洞，遇见疯帽匠和红桃皇后，故事短、句子也短。",
    terms: ["curious", "rabbit", "wonder", "caterpillar", "queen", "croquet", "nonsense", "shrinking", "adventure", "dream"],
  },
  {
    id: "oz",
    title: "The Wonderful Wizard of Oz",
    author: "L. Frank Baum",
    gutenbergId: "55",
    cefr: "A2",
    words: 39200,
    interests: ["children", "adventure"],
    summaryZh: "多萝西被龙卷风吹到奥兹国，和没脑子、没心、没勇气的伙伴一起上路。",
    terms: ["wizard", "scarecrow", "courage", "cyclone", "emerald", "heart", "brain", "coward", "witch", "journey"],
  },
  {
    id: "christmas-carol",
    title: "A Christmas Carol",
    author: "Charles Dickens",
    gutenbergId: "46",
    cefr: "B1",
    words: 28500,
    interests: ["society", "children"],
    summaryZh: "吝啬鬼斯克鲁奇在圣诞夜遇见三个鬼魂，开始懂得施舍与人情。",
    terms: ["ghost", "miser", "charity", "generosity", "christmas", "spirit", "poverty", "kindness", "regret", "feast"],
  },
  {
    id: "call-of-the-wild",
    title: "The Call of the Wild",
    author: "Jack London",
    gutenbergId: "215",
    cefr: "B1",
    words: 32000,
    interests: ["adventure", "nature"],
    summaryZh: "家犬巴克被卖到北极拉雪橇，在严寒里找回野性。篇幅短，情节紧。",
    terms: ["sled", "wilderness", "instinct", "wolf", "frost", "primitive", "toil", "master", "club", "survival"],
  },
  {
    id: "time-machine",
    title: "The Time Machine",
    author: "H. G. Wells",
    gutenbergId: "35",
    cefr: "B1",
    words: 32400,
    interests: ["science", "adventure"],
    summaryZh: "发明家乘时间机器去到遥远未来，看见人类分成地上与地下两种。",
    terms: ["future", "machine", "inventor", "civilization", "decay", "traveller", "experiment", "humanity", "darkness", "evolution"],
  },
  {
    id: "art-of-war",
    title: "The Art of War",
    author: "Sun Tzu",
    gutenbergId: "132",
    cefr: "B1",
    words: 10600,
    interests: ["politics", "philosophy"],
    summaryZh: "极短的兵法英译，讲地形、欺敌和将领。适合当作一组短文来读。",
    terms: ["strategy", "terrain", "victory", "general", "deception", "army", "enemy", "advantage", "discipline", "siege"],
  },
  {
    id: "peter-pan",
    title: "Peter Pan",
    author: "J. M. Barrie",
    gutenbergId: "16",
    cefr: "B1",
    words: 47400,
    interests: ["children", "adventure"],
    summaryZh: "不肯长大的彼得·潘带孩子们去永无岛，和海盗、仙女打交道。",
    terms: ["fairy", "pirate", "shadow", "youth", "adventure", "island", "flying", "childhood", "hook", "neverland"],
  },
  {
    id: "treasure-island",
    title: "Treasure Island",
    author: "Robert Louis Stevenson",
    gutenbergId: "120",
    cefr: "B1",
    words: 67300,
    interests: ["adventure", "mystery"],
    summaryZh: "少年吉姆拿到藏宝图，跟海盗一起出海。冒险词多，句子不算长。",
    terms: ["pirate", "treasure", "island", "schooner", "mutiny", "map", "captain", "parrot", "shore", "voyage"],
  },
  {
    id: "tom-sawyer",
    title: "The Adventures of Tom Sawyer",
    author: "Mark Twain",
    gutenbergId: "74",
    cefr: "B1",
    words: 70200,
    interests: ["adventure", "children"],
    summaryZh: "汤姆在密西西比河边逃学、刷墙、进山洞。口语多，适合 B1。",
    terms: ["fence", "river", "mischief", "cave", "schoolboy", "adventure", "treasure", "sunday", "island", "pirate"],
  },
  {
    id: "secret-garden",
    title: "The Secret Garden",
    author: "Frances Hodgson Burnett",
    gutenbergId: "113",
    cefr: "B1",
    words: 80600,
    interests: ["children", "nature"],
    summaryZh: "孤女玛丽在荒园里种花，也把生病的表哥慢慢带出屋子。",
    terms: ["garden", "orphan", "moor", "robin", "secret", "temper", "cousin", "bloom", "fresh", "key"],
  },
  {
    id: "around-the-world",
    title: "Around the World in Eighty Days",
    author: "Jules Verne",
    gutenbergId: "103",
    cefr: "B1",
    words: 63800,
    interests: ["adventure", "society"],
    summaryZh: "绅士福格打赌八十天环游世界，火车轮船一站接一站。",
    terms: ["journey", "wager", "steamer", "punctual", "globe", "passport", "delay", "detective", "elephant", "schedule"],
  },
  {
    id: "jungle-book",
    title: "The Jungle Book",
    author: "Rudyard Kipling",
    gutenbergId: "236",
    cefr: "B1",
    words: 51200,
    interests: ["children", "nature"],
    summaryZh: "狼孩毛利在丛林里跟老虎、蟒蛇和人类打交道。故事分成几则。",
    terms: ["jungle", "wolf", "tiger", "python", "pack", "law", "hunt", "village", "cobra", "man"],
  },
  {
    id: "sherlock",
    title: "The Adventures of Sherlock Holmes",
    author: "Arthur Conan Doyle",
    gutenbergId: "1661",
    cefr: "B2",
    words: 105000,
    interests: ["mystery", "society"],
    summaryZh: "十二个短篇探案。线索、推理和伦敦社会细节都不少，可以一篇篇读。",
    terms: ["detective", "clue", "deduction", "mystery", "suspect", "investigation", "crime", "observe", "client", "evidence"],
  },
  {
    id: "war-of-the-worlds",
    title: "The War of the Worlds",
    author: "H. G. Wells",
    gutenbergId: "36",
    cefr: "B2",
    words: 59800,
    interests: ["science", "adventure"],
    summaryZh: "火星人降落英国乡村，热射线和恐慌顺着公路蔓延。",
    terms: ["martian", "invasion", "cylinder", "panic", "artillery", "planet", "heat", "refugee", "machine", "destruction"],
  },
  {
    id: "pride",
    title: "Pride and Prejudice",
    author: "Jane Austen",
    gutenbergId: "1342",
    cefr: "B2",
    words: 122000,
    interests: ["romance", "society"],
    summaryZh: "伊丽莎白和达西因偏见与门第互相看错。对话多，适合练社交用语。",
    terms: ["prejudice", "pride", "acquaintance", "fortune", "estate", "manners", "proposal", "affection", "vanity", "civility"],
  },
  {
    id: "jane-eyre",
    title: "Jane Eyre",
    author: "Charlotte Brontë",
    gutenbergId: "1260",
    cefr: "B2",
    words: 183000,
    interests: ["romance", "society"],
    summaryZh: "家庭教师简在庄园里争取平等和自尊。情感词密，篇幅长。",
    terms: ["governess", "orphan", "independence", "passion", "manor", "conscience", "humble", "liberty", "duty", "rebellion"],
  },
  {
    id: "huck-finn",
    title: "Adventures of Huckleberry Finn",
    author: "Mark Twain",
    gutenbergId: "76",
    cefr: "B2",
    words: 110000,
    interests: ["adventure", "society"],
    summaryZh: "哈克和吉姆乘木筏沿河逃亡。方言多，也直接写到奴隶制。",
    terms: ["raft", "river", "freedom", "conscience", "runaway", "slavery", "shore", "current", "civilize", "lie"],
  },
  {
    id: "douglass",
    title: "Narrative of the Life of Frederick Douglass",
    author: "Frederick Douglass",
    gutenbergId: "23",
    cefr: "B2",
    words: 36300,
    interests: ["history", "politics"],
    summaryZh: "道格拉斯自述从奴隶制里学会读写、逃向自由。不长，但词很实。",
    terms: ["slavery", "freedom", "abolition", "literacy", "bondage", "master", "justice", "escape", "whip", "learning"],
  },
  {
    id: "gatsby",
    title: "The Great Gatsby",
    author: "F. Scott Fitzgerald",
    gutenbergId: "64317",
    cefr: "B2",
    words: 47200,
    interests: ["society", "romance"],
    summaryZh: "盖茨比用宴会和绿光追逐一段旧爱。中篇，句子漂亮，社会词多。",
    terms: ["wealth", "lavish", "longing", "party", "illusion", "mansion", "careless", "dream", "valley", "guest"],
  },
  {
    id: "leagues",
    title: "Twenty Thousand Leagues Under the Sea",
    author: "Jules Verne",
    gutenbergId: "164",
    cefr: "B2",
    words: 138000,
    interests: ["science", "adventure"],
    summaryZh: "尼摩船长的潜艇带着叙述者穿过深海。生物和航行词很多。",
    terms: ["submarine", "ocean", "captain", "specimen", "depth", "voyage", "crew", "island", "pearl", "electricity"],
  },
  {
    id: "frankenstein",
    title: "Frankenstein",
    author: "Mary Shelley",
    gutenbergId: "84",
    cefr: "C1",
    words: 75200,
    interests: ["science", "philosophy"],
    summaryZh: "科学家造出生命，又不敢面对它。书面语重，适合想再难一点的人。",
    terms: ["creature", "ambition", "misery", "wretched", "scientific", "horror", "solitude", "revenge", "creation", "despair"],
  },
  {
    id: "jekyll",
    title: "The Strange Case of Dr Jekyll and Mr Hyde",
    author: "Robert Louis Stevenson",
    gutenbergId: "43",
    cefr: "C1",
    words: 25500,
    interests: ["mystery", "philosophy"],
    summaryZh: "短篇，但用词偏书面。杰基尔用药物放出另一个自己。",
    terms: ["potion", "reputation", "horror", "transformation", "conscience", "laboratory", "dual", "violence", "respectable", "secret"],
  },
  {
    id: "dorian",
    title: "The Picture of Dorian Gray",
    author: "Oscar Wilde",
    gutenbergId: "174",
    cefr: "C1",
    words: 78500,
    interests: ["philosophy", "society"],
    summaryZh: "画像替少年承担衰老和罪恶。句子讲究，思想词密。",
    terms: ["portrait", "vanity", "soul", "corruption", "beauty", "conscience", "aesthetic", "youth", "temptation", "influence"],
  },
  {
    id: "wuthering",
    title: "Wuthering Heights",
    author: "Emily Brontë",
    gutenbergId: "768",
    cefr: "C1",
    words: 116000,
    interests: ["romance", "nature"],
    summaryZh: "荒原上的报复和痴恋，叙事套叙事。情绪词和风景词都很难。",
    terms: ["moor", "revenge", "passion", "haunting", "tempest", "cruelty", "exile", "ghost", "heath", "violence"],
  },
  {
    id: "two-cities",
    title: "A Tale of Two Cities",
    author: "Charles Dickens",
    gutenbergId: "98",
    cefr: "C1",
    words: 136000,
    interests: ["history", "politics"],
    summaryZh: "伦敦与巴黎，革命、监狱和一次替人赴死。历史词多，篇幅长。",
    terms: ["revolution", "sacrifice", "prison", "mob", "guillotine", "resurrection", "aristocrat", "spy", "knitting", "fate"],
  },
  {
    id: "walden",
    title: "Walden",
    author: "Henry David Thoreau",
    gutenbergId: "205",
    cefr: "C1",
    words: 115000,
    interests: ["nature", "philosophy"],
    summaryZh: "梭罗在湖边写简朴生活。议论多，适合已有一定阅读量的人慢慢读。",
    terms: ["solitude", "simplicity", "pond", "economy", "deliberate", "wilderness", "labor", "society", "cabin", "reflection"],
  },
  {
    id: "souls",
    title: "The Souls of Black Folk",
    author: "W. E. B. Du Bois",
    gutenbergId: "408",
    cefr: "C1",
    words: 76000,
    interests: ["history", "politics", "society"],
    summaryZh: "杜波依斯谈面纱、教育与平等。散文难，历史和政治词集中。",
    terms: ["veil", "sorrow", "equality", "education", "striving", "color", "freedom", "race", "progress", "soul"],
  },
  {
    id: "dracula",
    title: "Dracula",
    author: "Bram Stoker",
    gutenbergId: "345",
    cefr: "B2",
    words: 161000,
    interests: ["mystery", "adventure"],
    summaryZh: "用日记和书信拼出的吸血鬼故事。长，但情节清楚。",
    terms: ["vampire", "castle", "coffin", "blood", "superstition", "diary", "horror", "count", "stake", "fog"],
  },
  {
    id: "moby-dick",
    title: "Moby Dick",
    author: "Herman Melville",
    gutenbergId: "2701",
    cefr: "C2",
    words: 206000,
    interests: ["adventure", "nature", "philosophy"],
    summaryZh: "捕鲸船追逐白鲸。句子长、典故多，是目录里最难的小说之一。",
    terms: ["whale", "harpoon", "voyage", "obsession", "leviathan", "crew", "ocean", "captain", "blubber", "doom"],
  },
  {
    id: "origin",
    title: "On the Origin of Species",
    author: "Charles Darwin",
    gutenbergId: "1228",
    cefr: "C2",
    words: 156000,
    interests: ["science", "nature"],
    summaryZh: "进化论原典。论证密，生物词多，适合词汇量已经很大的时候读。",
    terms: ["species", "selection", "variation", "evolution", "descent", "adaptation", "instinct", "hybrid", "extinction", "offspring"],
  },
  {
    id: "republic",
    title: "The Republic",
    author: "Plato",
    gutenbergId: "1497",
    cefr: "C2",
    words: 118000,
    interests: ["philosophy", "politics"],
    summaryZh: "苏格拉底谈正义、灵魂和理想城邦。对话体，抽象词非常密。",
    terms: ["justice", "philosopher", "republic", "soul", "guardian", "ideal", "tyrant", "wisdom", "virtue", "state"],
  },
];
