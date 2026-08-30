import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = resolve(here, "..");
const allowlistPath = resolve(here, "../../src-tauri/src/ipc_allowlist.rs");

function walkTsFiles(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules") continue;
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) walkTsFiles(full, out);
    else if (/\.(ts|tsx)$/.test(name) && !name.endsWith(".test.ts")) out.push(full);
  }
  return out;
}

function parseAllowlist(src: string): string[] {
  const start = src.indexOf("&[");
  const end = src.indexOf("];", start);
  if (start < 0 || end < 0) throw new Error("APP_IPC_COMMANDS not found");
  return [...src.slice(start, end).matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]!);
}

describe("IPC allow-list vs UI invokes", () => {
  it("every invoke() target is in APP_IPC_COMMANDS", () => {
    const allow = new Set(parseAllowlist(readFileSync(allowlistPath, "utf8")));
    const invoked = new Set<string>();
    for (const file of walkTsFiles(srcRoot)) {
      const text = readFileSync(file, "utf8");
      for (const m of text.matchAll(/invoke(?:<[^>]+>)?\(\s*["']([a-z0-9_]+)["']/g)) {
        invoked.add(m[1]!);
      }
    }
    const missing = [...invoked].filter((name) => !allow.has(name)).sort();
    expect(missing).toEqual([]);
  });
});
