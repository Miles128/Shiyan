import { describe, expect, it } from "vitest";
import { assertCamelArgs } from "./invoke";

/**
 * `assertCamelArgs` is the runtime half of the IPC-contract guard: Tauri maps
 * lowerCamelCase top-level keys onto snake_case Rust params, and a snake_case
 * key silently deserializes to `None` (the cursor-pagination regression). The
 * compile-time half is the `Commands` map in commands.ts.
 */
describe("assertCamelArgs", () => {
  it("accepts lowerCamelCase top-level keys", () => {
    expect(() =>
      assertCamelArgs("list_articles_ranked", {
        cursorScore: 1,
        cursorId: "a",
        unreadOnly: null,
      }),
    ).not.toThrow();
  });

  it("accepts an empty argument object", () => {
    expect(() => assertCamelArgs("get_config", {})).not.toThrow();
  });

  it("throws on a snake_case top-level key, naming the key", () => {
    expect(() => assertCamelArgs("list_articles_ranked", { cursor_score: 1 })).toThrow(
      /cursor_score/,
    );
  });

  it("does not recurse: nested serde structs keep snake_case", () => {
    // add_memory's `input` is a serde struct whose fields are legitimately
    // snake_case; only the top-level Tauri argument keys are checked.
    expect(() =>
      assertCamelArgs("add_memory", {
        input: { context_sentence: "s", article_id: null },
      }),
    ).not.toThrow();
  });
});
