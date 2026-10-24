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
  setMemoryStatus: (id: string, status: string) =>
    typedInvoke("set_memory_status", { id, status }),
  deleteMemory: (id: string) => typedInvoke("delete_memory", { id }),

  // Export the whole vocab library (words + phrases) as CSV; returns the
  // written path, or null when the save dialog was cancelled.
  exportVocabCsv: () => typedInvoke("export_memory_csv"),
};
