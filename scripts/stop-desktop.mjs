#!/usr/bin/env node
/**
 * Stop any running `pnpm dev:desktop` (tauri dev) session before tests.
 * tauri dev watches src-tauri and relaunches the macOS window on rebuild,
 * which steals focus during cargo/vitest runs. Plain `pnpm dev` (browser
 * Vite) is left alone — it has no window.
 *
 * Two extra sweeps beyond the session patterns below:
 *  1. whoever holds TCP :1420 (the dev port tauri dev needs);
 *  2. stale dev processes: Shiyan-repo vite servers and `pnpm dev` shims
 *     reparented to init (ppid 1) after their `tauri dev` parent died.
 *     Killing the session alone just orphans its vite/shim — the old
 *     "Port 1420 is already in use" wedge. A stale `pnpm dev` shim is told
 *     apart from an intentional browser session by one fact: the shim's
 *     parent is dead (ppid 1) while a real `pnpm dev` keeps its shell.
 * A process with a live, unrelated parent is only reported, never killed.
 *
 * Never matches shells that merely *mention* the pattern in their cmdline,
 * and never kills this process or its parents.
 */
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

const SELF = new Set([String(process.pid), String(process.ppid)]);

/** Processes whose full cmdline contains every token and looks like a real session. */
function listCandidates(pattern) {
  let out = "";
  try {
    out = execSync(`pgrep -f ${JSON.stringify(pattern)}`, {
      stdio: ["ignore", "pipe", "ignore"],
    }).toString();
  } catch {
    return [];
  }
  return out
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
}

function cmdlineOf(pid) {
  try {
    return readFileSync(`/proc/${pid}/cmdline`, "utf8").replace(/\0/g, " ").trim();
  } catch {
    // macOS: no /proc; use ps
    try {
      return execSync(`ps -p ${pid} -o args=`, {
        stdio: ["ignore", "pipe", "ignore"],
      }).toString().trim();
    } catch {
      return "";
    }
  }
}

function ppidOf(pid) {
  try {
    return execSync(`ps -p ${pid} -o ppid=`, {
      stdio: ["ignore", "pipe", "ignore"],
    }).toString().trim();
  } catch {
    return "";
  }
}

/** PIDs currently listening on a TCP port (bare numbers, may be empty). */
function portHolders(port) {
  let out = "";
  try {
    out = execSync(`lsof -t -i :${port} -sTCP:LISTEN`, {
      stdio: ["ignore", "pipe", "ignore"],
    }).toString();
  } catch {
    return [];
  }
  return out
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
}

function isRealDesktopSession(pid, pattern) {
  if (SELF.has(pid)) return false;
  const cmd = cmdlineOf(pid);
  if (!cmd) return false;
  // Shells / editors / CI wrappers that only *quote* the pattern are not sessions.
  if (/^(\/bin\/|\/usr\/bin\/|\/usr\/local\/bin\/)?(zsh|bash|sh|dash)\b/.test(cmd)) {
    // Exception: an interactive shell running `pnpm dev:desktop` / `tauri dev` for real.
    if (/\b(tauri(\.js)?\s+dev|pnpm\s+dev:desktop)\b/.test(cmd) && !/stop-desktop|pretest|pnpm test/.test(cmd)) {
      return /tauri\.js dev|\bnode\b.*tauri|target\/debug\/shiyan/.test(cmd);
    }
    return false;
  }
  // Node hosting the tauri CLI dev loop
  if (/tauri\.js dev|@tauri-apps\/cli.*dev/.test(cmd) && /\bnode\b/.test(cmd)) return true;
  // The compiled app itself
  if (/target\/debug\/shiyan(\s|$)/.test(cmd)) return true;
  // Fallback: exact-ish pattern without shell wrappers
  return cmd.includes(pattern) && !/stop-desktop|pgrep|ps -p/.test(cmd);
}

const patterns = ["tauri.js dev", "target/debug/shiyan", "target/debug/shiyan.exe"];

/** Walk up the parent chain (capped): proves a vite belongs to a session. */
function ancestorsOf(pid, depth = 6) {
  const out = new Set();
  let cur = pid;
  for (let i = 0; i < depth; i++) {
    const ppid = ppidOf(cur);
    if (!ppid || ppid === "0" || ppid === "1") break;
    out.add(ppid);
    cur = ppid;
  }
  return out;
}

/**
 * Orphaned Vite dev servers: whatever holds TCP :1420, plus Shiyan-repo vite
 * processes reparented to init (ppid 1) after their `tauri dev` parent died.
 * A port holder whose ancestor chain reaches a session being stopped dies
 * with it (killing the parent alone just orphans the vite — the old bug).
 * A process with a live, unrelated parent is only reported, never killed —
 * an intentional `pnpm dev` keeps its shell.
 */
/** True for a `pnpm dev` beforeDevCommand shim (never browser intent). */
function isPnpmDevShim(cmd) {
  if (!/\bpnpm\s+dev(\s|$)/.test(cmd)) return false;
  if (/dev:desktop|stop-desktop|pretest|pnpm test|pgrep|ps -p|lsof /.test(cmd)) {
    return false;
  }
  // A shell wrapper merely quoting it is not the shim itself.
  if (/^(\/bin\/|\/usr\/bin\/|\/usr\/local\/bin\/)?(zsh|bash|sh|dash)\b/.test(cmd)) {
    return false;
  }
  return true;
}

function listOrphanVite(sessionPids) {
  const holders = new Set(portHolders(1420));
  const found = new Set();
  // Stale `pnpm dev` shims first: ancestors of the port holder with a dead
  // parent (ppid 1). A real browser `pnpm dev` keeps its shell, so its chain
  // never matches this.
  for (const h of holders) {
    for (const a of ancestorsOf(h)) {
      if (SELF.has(a) || found.has(a)) continue;
      const cmd = cmdlineOf(a);
      if (!cmd || !isPnpmDevShim(cmd)) continue;
      if (ppidOf(a) !== "1") continue;
      found.add(a);
    }
  }
  // Anything already being stopped (session or stale shim above) takes its
  // vite children with it — no warn, no orphan window.
  const stopping = new Set([...sessionPids, ...found]);
  const cands = new Set(listCandidates("vite"));
  for (const h of holders) cands.add(h);
  for (const pid of cands) {
    if (SELF.has(pid) || found.has(pid)) continue;
    const cmd = cmdlineOf(pid);
    if (!cmd || !/vite(\.js)?(\s|$)/.test(cmd)) continue;
    if (/stop-desktop|pgrep|ps -p|lsof /.test(cmd)) continue;
    const holdsPort = holders.has(pid);
    const isRepoVite = cmd.includes("Shiyan");
    if (!holdsPort && !isRepoVite) continue;
    if (ppidOf(pid) !== "1") {
      const withStopping =
        holdsPort &&
        [...ancestorsOf(pid)].some((a) => stopping.has(a));
      if (!withStopping) {
        if (holdsPort) {
          console.log(
            `[pretest] port 1420 held by live process ${pid} (${cmd.slice(0, 120)}), not killing`,
          );
        }
        continue;
      }
    }
    found.add(pid);
  }
  return found;
}

const pids = new Set();
for (const p of patterns) {
  for (const pid of listCandidates(p)) {
    if (isRealDesktopSession(pid, p)) pids.add(pid);
  }
}
const orphans = listOrphanVite(pids);
for (const pid of orphans) pids.add(pid);

if (pids.size === 0) {
  console.log("[pretest] no desktop session");
  process.exit(0);
}

console.log(
  `[pretest] stopping desktop session: ${[...pids].join(", ")}` +
    (orphans.size > 0 ? ` (incl. stale dev: ${[...orphans].join(", ")})` : ""),
);
try {
  execSync(`kill -TERM ${[...pids].join(" ")}`, { stdio: "ignore" });
} catch {
  // fall through to SIGKILL
}

// Poll until everything is gone (deadline ~5s): TERMing a session can take a
// moment to orphan its vite/shim, and a single fixed sleep kept missing it.
const deadline = Date.now() + 5000;
(function sweep() {
  const rest = new Set();
  for (const p of patterns) {
    for (const pid of listCandidates(p)) {
      if (isRealDesktopSession(pid, p)) rest.add(pid);
    }
  }
  for (const pid of listOrphanVite(rest)) rest.add(pid);
  if (rest.size === 0) {
    console.log("[pretest] desktop session stopped");
    return;
  }
  try {
    execSync(`kill -KILL ${[...rest].join(" ")}`, { stdio: "ignore" });
  } catch {
    // already gone
  }
  if (Date.now() < deadline) {
    setTimeout(sweep, 500);
  } else {
    console.log(
      `[pretest] desktop session stopped ( survivors: ${[...rest].join(", ")})`,
    );
  }
})();
