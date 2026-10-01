import { typedInvoke } from "./invoke";
import type { MemoryKind } from "./commands";

export type { MemoryKind };

export type AddMemoryInput = {
  kind: MemoryKind;
  term: string;
  contextSentence: string;
  articleId?: string | null;
  definitionZh?: string | null;
  wordType?: string | null;
  collocations?: string[] | null;
};

export const apiMemory = {
  addMemory: (input: AddMemoryInput) =>
    typedInvoke("add_memory", {
      input: {
        kind: input.kind,
        term: input.term,
        context_sentence: input.contextSentence,
        article_id: input.articleId ?? null,
        definition_zh: input.definitionZh ?? null,
        word_type: input.wordType ?? null,
        collocations: input.collocations ?? null,
      },
    }),
  listMemory: (kind: MemoryKind, status?: string) =>
    typedInvoke("list_memory", { kind, status: status ?? null }),
  dueMemory: (kind: MemoryKind) => typedInvoke("due_memory", { kind }),
  reviewMemory: (id: string, rating: string) =>
    typedInvoke("review_memory", { id, rating }),
  setMemoryStatus: (id: string, status: string) =>
    typedInvoke("set_memory_status", { id, status }),
  deleteMemory: (id: string) => typedInvoke("delete_memory", { id }),

  // Export the whole vocab library (words + phrases) as CSV; returns the
  // written path, or null when the save dialog was cancelled.
  exportVocabCsv: () => typedInvoke("export_memory_csv"),

  // Known words: marked as already known, so they stop being highlighted.
  listKnownWords: () => typedInvoke("list_known_words"),
  addKnownWord: (term: string) => typedInvoke("add_known_word", { term }),
  removeKnownWord: (term: string) => typedInvoke("remove_known_word", { term }),

  // Lookup history: every term resolved via the selection popover.
  recordLookup: (term: string, context?: string | null, articleId?: string | null) =>
    typedInvoke("record_lookup", {
      term,
      context: context ?? null,
      articleId: articleId ?? null,
    }),
  listLookups: (search?: string, limit?: number, offset?: number) =>
    typedInvoke("list_lookups", {
      search: search ?? null,
      limit: limit ?? null,
      offset: offset ?? null,
    }),
  deleteLookup: (id: number) => typedInvoke("delete_lookup", { id }),
  clearLookups: () => typedInvoke("clear_lookups"),
};
