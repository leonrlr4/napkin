// Shared machinery for the baseline generators: bundling, Math.random tracking,
// non-finite number encoding and the one-case-per-line JSON writer that the Rust
// `testkit` crate reads.

import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import * as esbuild from "esbuild";

export const BASELINE_DIR = dirname(dirname(fileURLToPath(import.meta.url)));
export const REPO_ROOT = resolve(BASELINE_DIR, "..", "..");

/** Directory of `pkg` as Node finds it from `fromDir`, walking up through node_modules. */
function packageDir(pkg, fromDir = BASELINE_DIR) {
  for (let dir = fromDir; ; dir = dirname(dir)) {
    const candidate = join(dir, "node_modules", pkg);
    if (existsSync(join(candidate, "package.json"))) return candidate;
    if (dirname(dir) === dir) throw new Error(`${pkg} is not installed; run npm ci`);
  }
}

/** Resolves `specifier` the way `fromPackage` itself would import it. */
export function resolveFrom(fromPackage, specifier) {
  return createRequire(join(packageDir(fromPackage), "package.json")).resolve(specifier);
}

export function installedVersion(pkg, fromPackage = null) {
  const dir = packageDir(pkg, fromPackage ? packageDir(fromPackage) : BASELINE_DIR);
  return JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).version;
}

/** Fails loudly when node_modules does not hold the versions the baseline claims. */
export function assertVersions(expected) {
  for (const [label, { pkg, from, version }] of Object.entries(expected)) {
    const actual = installedVersion(pkg, from);
    if (actual !== version) {
      throw new Error(`${label}: expected ${pkg}@${version}, found ${actual}; run npm ci`);
    }
  }
}

/** Bundles `entrySource` (ES module text) with esbuild and imports the result. */
export async function bundleAndImport(name, entrySource, buildOptions = {}) {
  const outDir = join(BASELINE_DIR, ".build");
  mkdirSync(outDir, { recursive: true });
  const outfile = join(outDir, `${name}.mjs`);
  await esbuild.build({
    stdin: { contents: entrySource, resolveDir: BASELINE_DIR, loader: "ts" },
    bundle: true,
    platform: "node",
    format: "esm",
    outfile,
    logLevel: "error",
    ...buildOptions,
  });
  return import(`${pathToFileURL(outfile).href}?t=${Date.now()}`);
}

// --- Math.random tracking ----------------------------------------------------

function lcg(seed) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 2 ** 32;
  };
}

let randomCalls = 0;
let randomStream = lcg(1);
Math.random = () => {
  randomCalls += 1;
  return randomStream();
};

function capture(fn) {
  try {
    return fn();
  } catch (error) {
    return { throws: error instanceof TypeError ? "TypeError" : error.message };
  }
}

/** Drops every number, keeping the JSON shape: what "structure" cases compare. */
function structureOf(value) {
  if (typeof value === "number") return 0;
  if (Array.isArray(value)) return value.map(structureOf);
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, structureOf(v)]));
  }
  return value;
}

/**
 * Runs `fn` and returns `{ compare, expected }`. A case that never calls Math.random is
 * compared number by number. A case that does is run a second time with a different
 * Math.random stream; if its structure is identical both times only the structure is
 * compared, otherwise the case is rejected because nothing about it is reproducible.
 */
export function runCase(name, fn) {
  randomCalls = 0;
  randomStream = lcg(1);
  const first = encode(capture(fn));
  if (randomCalls === 0) {
    return { compare: "exact", expected: first };
  }
  randomStream = lcg(2);
  const second = encode(capture(fn));
  if (JSON.stringify(structureOf(first)) !== JSON.stringify(structureOf(second))) {
    throw new Error(`${name}: output structure depends on Math.random; drop this case`);
  }
  // Numbers in a structure-only case are meaningless, so store zeros instead of them.
  return { compare: "structure", expected: structureOf(first) };
}

// --- number encoding -----------------------------------------------------------

/** JSON has no NaN or Infinity; testkit decodes these three strings back. */
export function encode(value) {
  if (typeof value === "number") {
    if (Number.isNaN(value)) return "NaN";
    if (value === Infinity) return "Infinity";
    if (value === -Infinity) return "-Infinity";
    return value;
  }
  if (Array.isArray(value)) return value.map(encode);
  if (value && typeof value === "object") {
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      if (v !== undefined) out[k] = encode(v);
    }
    return out;
  }
  return value;
}

// --- output ----------------------------------------------------------------------

/**
 * Writes `{ "source": ..., "cases": [...] }` with one case per line, so a regenerated
 * baseline diffs case by case. Case names must be unique within a group.
 */
export function writeGroup(dir, group, source, cases) {
  const names = new Set();
  for (const c of cases) {
    if (names.has(c.name)) throw new Error(`${group}: duplicate case name ${c.name}`);
    names.add(c.name);
  }
  mkdirSync(dir, { recursive: true });
  const lines = cases.map((c) => JSON.stringify(c));
  const text = `{"source":${JSON.stringify(source)},"cases":[\n${lines.join(",\n")}\n]}\n`;
  const path = join(dir, `${group}.json`);
  writeFileSync(path, text);
  const kb = (Buffer.byteLength(text) / 1024).toFixed(0);
  const structural = cases.filter((c) => c.compare === "structure").length;
  console.log(`${group}: ${cases.length} cases (${structural} structure-only), ${kb} KiB`);
}
