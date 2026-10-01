import { invoke } from "@tauri-apps/api/core";
import type { Commands } from "./commands";

type ArgsFor<K extends keyof Commands> = Commands[K]["args"];

/**
 * Tauri's `#[tauri::command]` macro maps lowerCamelCase JS keys onto snake_case
 * Rust params. A snake_case key here does not error — it silently deserializes
 * to `None` for an `Option<T>` (the cursor-pagination regression). Assert the
 * top level only: fields nested inside an argument (e.g. `add_memory`'s
 * `input`) follow serde and are legitimately snake_case.
 */
export function assertCamelArgs(cmd: string, args: Record<string, unknown>): void {
  for (const key of Object.keys(args)) {
    if (key.includes("_")) {
      throw new Error(
        `typedInvoke("${cmd}"): argument key "${key}" must be lowerCamelCase; ` +
          `snake_case silently becomes None on the Rust side.`,
      );
    }
  }
}

/**
 * Type-safe `invoke`: the command name is checked against the `Commands`
 * contract, the argument shape must match that command's declared `args`, and
 * the return type is the declared `ret`. Commands declared `args: void` take no
 * second argument.
 */
export function typedInvoke<K extends keyof Commands>(
  cmd: K,
  ...rest: ArgsFor<K> extends void ? [] : [args: ArgsFor<K>]
): Promise<Commands[K]["ret"]> {
  const args = (rest[0] ?? {}) as Record<string, unknown>;
  if (import.meta.env.DEV) assertCamelArgs(String(cmd), args);
  return invoke(cmd as string, args) as Promise<Commands[K]["ret"]>;
}
