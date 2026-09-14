# napkin M1：`rough` Implementation Plan

> Historical record, frozen 2026-09-14. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 roughjs@4.6.4 逐行 port 成零依賴的 Rust crate `rough`，node 產生的 983 個基準案例全部在 1e-9 誤差內通過。

**Architecture:** `tools/baseline/` 是一個 node 專案，用 esbuild 打包 npm 上的 `roughjs/bin/*.js` 並跑一組固定輸入，輸出 JSON 基準檔到 `crates/rough/tests/baseline/`，commit 進 repo。Rust 端是 Cargo workspace：`crates/rough`（port 本體，無依賴）和 `crates/testkit`（讀基準、比對數字，M2 也會用）。一個基準檔對應一個 `#[test]`，port 的順序讓每個任務剛好把自己那幾組測試轉綠。

**Tech Stack:** Rust 1.98.1（edition 2024）、serde_json 1（只在測試）、node 26.7.0、npm 12.0.2、esbuild 0.28.2、roughjs 4.6.4。

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`（§3 相容性基準、§4.2 `rough`、§9.1 測試）
**Roadmap:** `docs/decisions/plans/2026-09-13-napkin-roadmap.md`

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- Excalidraw 基準 commit：`afa3a653fc5d2b742adcbd5a6063187b056d2419`。
- rough.js 與依賴的版本（Excalidraw 在該 commit 的 `yarn.lock`）：`roughjs 4.6.4`、`hachure-fill 0.5.2`、`path-data-parser 0.1.0`、`points-on-curve 0.2.0`、`points-on-path 0.2.1`。
- `crates/rough/Cargo.toml` 的 `[dependencies]` 必須是空的（spec §4.2）。測試用的依賴只能放 `[dev-dependencies]`。
- 數字比對的絕對誤差是 `1e-9`（`testkit::TOLERANCE`，spec §9.1），不准放寬。
- 基準 JSON 只能由產生器寫出，不准手改。
- port 的來源是 `tools/baseline/node_modules/` 裡實際執行的 JS：`roughjs/bin/*.js`（Excalidraw import 的就是這份 tsc 輸出，不是 `bundled/` 的 minify 版本）與四個依賴套件的 `lib/`、`bin/`。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、該任務新增的測試。

## 寫計畫時已經完成的驗證

- `tools/baseline/` 的產生器（Task 1 的完整程式碼）實際跑過，產出 15 組共 983 個案例、4.2 MiB；連跑兩次輸出逐 byte 相同。npm 安裝後四個依賴的版本與上面鎖定的一致。
- `testkit`、`js.rs`、`math.rs`、`core.rs`、`renderer.rs` 的 `Ctx`、`testkit::rough_json`、`tests/baseline.rs` 都在其餘模組為 `todo!()` 的 stub 上編譯過，`cargo clippy -D warnings` 沒有輸出。`random` 組 7 個案例全部通過；把 `48271` 改成 `48270` 時 7 個全部失敗。`testkit` 自己的 6 個測試通過。
- 還沒寫過任何 port 程式碼（path-data-parser 以後的部分）。

## 寫計畫時做的決定

以下每一項都會影響 port 的寫法，執行時不要改回來。

1. **每個基準案例用新的 `RoughGenerator`。** rough.js 的 `_o(options)` 在 `options` 為 `undefined` 時回傳 generator 自己的 `defaultOptions` 物件，第一次抽亂數就把 seed 0 的 randomizer 掛上去；之後所有呼叫都會複製到這個 randomizer，全部退回 `Math.random`。寫計畫時就是因此看到整批 ellipse 案例變成不可重現。Excalidraw 每次都有傳 options，所以 Rust 的 generator 方法一律要求 `&Options`，不 port「不傳 options」這條路。
2. **`Math.random` 的處理。** 產生器替換掉 `Math.random` 並計算呼叫次數。沒呼叫過的案例逐個數字比對；呼叫過的案例會換一串亂數再跑一次，結構相同才收錄，而且只比結構（數字存成 0）。目前只有 `outline_linear` 的 2 個 seed 0 案例和 `fill_dots` 的 12 個案例屬於這種。Rust 端凡是 JS 呼叫 `Math.random` 的地方都呼叫 `rough::math::math_random()`，它刻意不可重現。
3. **randomizer 共用語意用 `Rc` 表達。** rough.js 把 randomizer 掛在 options 物件上，`Object.assign({}, o)` 複製時，已存在的 randomizer 會被共用，還不存在的則各自在第一次抽亂數時建立。`renderer::Ctx` 的 `Clone` 共用 `Rc`，兩種情況都成立（Task 6 附測試）。
4. **`testkit::rough_json` 放 rough 型別的 JSON 表示。** `testkit` 依賴 `rough`，`rough` 的整合測試再依賴 `testkit`。cargo 接受這種 dev-dependency 循環，已驗證可以編譯；限制是 `rough` 的 `#[cfg(test)]` 單元測試不能用 `testkit`。M2 的形狀基準也用同一份轉換。
5. **不 port 的部分：** `canvas.js`、`svg.js`、`rough.js`（瀏覽器繪圖入口）；generator 的 `opsToPath`、`toPaths`、`fillSketch`（產生 SVG path 字串，napkin 的 renderer 直接吃 ops）；renderer 的 `randOffset`、`randOffsetWithRange`（放在 filler 的 `helper` 物件上，但 4.6.4 沒有任何 filler 呼叫）；path-data-parser 的 `serialize`；hachure-fill 接受單一多邊形（而非多邊形清單）的呼叫形式，rough.js 從不這樣呼叫。

## JS → Rust 對照規則

Task 3 到 Task 11 的 port 都照這張表寫。它們都是會讓輸出在第 9 位小數之後就分岔、而且編譯器不會提醒的差異。

| JS | Rust | 為什麼 |
|---|---|---|
| 函數、區域變數、敘述順序 | 保留原名（轉 snake_case）與順序；每個函數的 doc comment 寫出來源，例如 ``/// bin/renderer.js `_line` `` | 逐行對照除錯 |
| 呼叫亂數的運算式 | 維持 JS 的求值順序。不要把 `random()` 提到前面的 `let`，陣列字面值、函數參數、`a + b + random()` 都是由左到右 | 亂數序列錯一位，後面全部不同 |
| `Object.assign({}, o, { k: v })` | `let mut o2 = o.clone(); o2.o.k = v;` | 決定 3 |
| `if (o.fill)` | `o.o.fill.as_deref().is_some_and(\|f\| !f.is_empty())` | 空字串是 falsy |
| 數字的真假值，`x \|\| 0`、`x ? a : b` | `renderer::truthy(x)`（0 與 NaN 為假） | |
| `Math.round` | `js::math_round` | `f64::round` 對 -2.5 給 -3，JS 給 -2 |
| `Math.imul(a, b)`、位元運算 | `js::to_int32(a).wrapping_mul(...)` | ToInt32 的截斷與環繞 |
| `parseFloat(x.toFixed(n))` | `js::to_fixed(x, n).parse::<f64>()` | `format!("{:.9}")` 的進位方向不同 |
| `Math.pow(a, b)`、`a ** b` | `a.powf(b)` | |
| `Math.sin/cos/tan/atan/asin/atan2/sqrt/hypot/floor/ceil/abs/min/max` | 同名 `f64` 方法 | 輸入都是有限數，NaN 語意差異碰不到 |
| `for (let a = s; a <= e; a = a + inc)` | 同樣的浮點累加迴圈，不要改成 `s + i * inc` | 迭代次數取決於累加誤差 |
| `arr.sort(cmp)` | `sort_by`，比較函數依 JS 回傳值的正負給 `Ordering` | 兩者都是穩定排序 |
| `splice(0, n)`、`filter`、`concat`、`push(...x)` | `drain(..n)`、`retain`、`extend` | |
| 會改寫參數的 JS 函數（`hachureLines` 就地旋轉多邊形） | 參數用 `&mut`，其餘點一律以值複製 | 旋轉再轉回來的浮點漂移會被第二次填充看到 |
| `throw new Error(msg)` | 回傳 `Result`，錯誤型別的 `Display` 印出與 JS 相同的訊息；JS 因 `undefined.type` 丟的 TypeError 印成 `TypeError` | 基準記錄的是訊息 |

除錯方式：`BASELINE_CASE=<案例名稱片段> cargo test -p rough --test baseline <組名>` 只跑單一案例。數字差超過 1e-9 或結構不同時，對照 JS 找出第一個分歧的運算。只有在確認分歧來自 V8 與 glibc 的 `sin`/`cos` 最後一位、而 port 本身沒有錯時，才停下來回報控制者；不要為了通過而改運算順序或放寬容差。

---

### Task 1：rough.js 基準產生器

**Files:**
- Create: `tools/baseline/package.json`
- Create: `tools/baseline/package-lock.json`（由 `npm install` 產生）
- Create: `tools/baseline/.gitignore`
- Create: `tools/baseline/lib/harness.mjs`
- Create: `tools/baseline/rough/cases.mjs`
- Create: `tools/baseline/rough/generate.mjs`
- Create: `crates/rough/tests/baseline/*.json`（由產生器寫出，15 個檔案）

**Interfaces:**
- Consumes: 無
- Produces: 基準檔格式 `{"source": string, "cases": [{"name", "call", "args", "compare": "exact"|"structure", "expected"}]}`，一行一個案例；`expected` 裡的 NaN 與 ±Infinity 寫成字串 `"NaN"`、`"Infinity"`、`"-Infinity"`；JS 丟出例外時 `expected` 是 `{"throws": 訊息}`。`lib/harness.mjs` 的 `bundleAndImport`、`runCase`、`writeGroup`、`assertVersions`、`resolveFrom` 供 M2 的產生器沿用。

- [ ] **Step 1：建立 `tools/baseline/package.json`**

```json
{
  "name": "napkin-baseline",
  "private": true,
  "type": "module",
  "scripts": {
    "rough": "node rough/generate.mjs"
  },
  "devDependencies": {
    "esbuild": "0.28.2",
    "roughjs": "4.6.4"
  }
}
```

- [ ] **Step 2：建立 `tools/baseline/.gitignore`**

```gitignore
node_modules/
.build/
```

- [ ] **Step 3：安裝依賴，產生 lock 檔**

Run: `cd tools/baseline && npm install --no-audit --no-fund`
Expected: 產生 `package-lock.json`。npm 12 會警告 esbuild 的 install script 沒有執行，不影響使用：esbuild 的執行檔來自 optional dependency `@esbuild/linux-x64`。

- [ ] **Step 4：建立 `tools/baseline/lib/harness.mjs`**

```js
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
```

- [ ] **Step 5：建立 `tools/baseline/rough/cases.mjs`**

```js
// Inputs for every baseline group. Each case is { name, call, args }; `args` is the exact
// positional argument list, options object last. Names must be unique per group.

const SEEDS = [1, 12345, 2147483647];
const ROUGHNESS = [0, 1, 2];
const FILL_SEEDS = [1, 2147483647];

function cross(prefix, call, argsFor, variants) {
  const cases = [];
  for (const [label, extra] of variants) {
    cases.push({ name: `${prefix}/${label}`, call, args: argsFor(extra) });
  }
  return cases;
}

/** seed × roughness variants merged into `base` options. */
function seedRoughness(base = {}, seeds = SEEDS) {
  const variants = [];
  for (const seed of seeds) {
    for (const roughness of ROUGHNESS) {
      variants.push([`s${seed}/r${roughness}`, { ...base, seed, roughness }]);
    }
  }
  return variants;
}

// --- geometry --------------------------------------------------------------------

const LINES = {
  mid: [10, 20, 250, 180],
  short: [0, 0, 3, 4],
  long: [0, 0, 700, 0],
  vertical: [5, 5, 5, 305],
};
const POLYLINES = {
  empty: [],
  single: [[3, 4]],
  two: [[0, 0], [120, 40]],
  zigzag: [[0, 0], [60, 80], [120, 10], [180, 90], [240, 0]],
};
const POLYGONS = {
  triangle: [[10, 10], [110, 30], [40, 90]],
  concave: [[0, 0], [100, 0], [100, 80], [50, 30], [0, 80]],
};
const RECTS = {
  normal: [5, 10, 110, 70],
  tiny: [0, 0, 8, 6],
  negative: [100, 100, -80, -40],
};
const ELLIPSES = {
  normal: [60, 40, 110, 70],
  tiny: [10, 10, 6, 4],
};
const ARCS = {
  half: [60, 60, 100, 80, 0, Math.PI, false],
  closed: [60, 60, 100, 80, -Math.PI / 2, Math.PI, true],
  overfull: [60, 60, 100, 80, 0, 7, true],
};
const CURVES = {
  two: [[0, 0], [100, 50]],
  three: [[0, 0], [50, 80], [100, 0]],
  wave: [[0, 0], [30, 60], [70, -20], [110, 50], [140, 0], [160, 40]],
};
// Path strings mirror what Excalidraw builds (shape.ts) plus every SVG command form
// path-data-parser handles.
const PATHS = {
  roundedRect: "M 15 0 L 95 0 Q 110 0, 110 15 L 110 55 Q 110 70, 95 70 L 15 70 Q 0 70, 0 55 L 0 15 Q 0 0, 15 0",
  diamondRounded: `M 56 7 L 103 28
            C 110 35, 110 35, 103 42
            L 63 63
            C 56 70, 56 70, 49 63
            L 7 42
            C 0 35, 0 35, 7 28
            L 49 7
            C 56 0, 56 0, 56 7`,
  relative: "m 10 10 l 40 0 h 20 v 30 c 0 10 -10 20 -20 20 s -20 -10 -20 -20 q 0 -15 10 -20 t 10 -10 z",
  absoluteCurves: "M 0 0 H 50 V 40 C 60 50 70 50 80 40 S 100 30 110 40 Q 120 60 100 70 T 60 80 Z",
  arcs: "M 10 50 A 40 30 0 0 1 90 50 A 40 30 0 1 0 10 50 a 20 20 45 1 1 30 30 A 0 10 0 0 0 20 20",
  implicitRepeats: "M 0 0 10 10 20 0 30 10 L 40 0 50 10",
  numbers: "M1e1,2E1L.5-3.5,+7 8.25e-1",
  noMove: "L 30 30 L 60 0",
  subpaths: "M 0 0 L 40 0 L 40 40 Z M 60 0 L 100 0 L 80 40 Z",
  elbow: "M 0 0 L 34 0 Q 50 0, 50 16 L 50 84 Q 50 100, 66 100 L 150 100",
};
const BAD_PATHS = {
  invalidChar: "M 0 0 X 10 10",
  paramNotNumber: "M 0 L 10 10",
  endedShort: "M 0",
};

// --- groups -----------------------------------------------------------------------

const random = [1, 2, 12345, 2147483647, 2147483648, -7, 1.5].map((seed) => ({
  name: `seed${seed}`,
  call: "Random",
  args: [seed, 12],
}));

const pathData = [];
for (const call of ["parsePath", "absolutize", "normalize"]) {
  for (const [label, d] of Object.entries({ ...PATHS, ...BAD_PATHS })) {
    pathData.push({ name: `${call}/${label}`, call, args: [d] });
  }
}

const BEZIER = [[0, 0], [30, 80], [90, -40], [120, 40], [150, 90], [200, 10], [240, 60]];
const NOISY = Array.from({ length: 30 }, (_, i) => [i * 7, Math.round(40 * Math.sin(i / 3)) + (i % 4)]);
const pointsOnCurve = [
  ...[0.15, 1, 10].flatMap((tolerance) =>
    [undefined, 0.5, 3].map((distance) => ({
      name: `pointsOnBezierCurves/t${tolerance}/d${distance}`,
      call: "pointsOnBezierCurves",
      args: distance === undefined ? [BEZIER, tolerance] : [BEZIER, tolerance, distance],
    })),
  ),
  ...[0.75, 2, 10].map((distance) => ({ name: `simplify/d${distance}`, call: "simplify", args: [NOISY, distance] })),
  ...Object.entries({ ...CURVES, single: [[1, 1]] }).flatMap(([label, points]) =>
    [0, 0.5].map((tightness) => ({
      name: `curveToBezier/${label}/k${tightness}`,
      call: "curveToBezier",
      args: [points, tightness],
    })),
  ),
];

const pointsOnPath = Object.entries(PATHS).flatMap(([label, d]) =>
  [
    [1, undefined],
    [1, 1.5],
    [0.15, 0.5],
  ].map(([tolerance, distance]) => ({
    name: `pointsOnPath/${label}/t${tolerance}/d${distance}`,
    call: "pointsOnPath",
    args: distance === undefined ? [d, tolerance] : [d, tolerance, distance],
  })),
);

const hachureFill = [];
for (const [label, polygons] of Object.entries({
  triangle: [POLYGONS.triangle],
  concave: [POLYGONS.concave],
  twoPolygons: [POLYGONS.triangle, [[150, 0], [220, 0], [220, 60], [150, 60]]],
  closedAlready: [[[0, 0], [80, 0], [80, 50], [0, 0]]],
  degenerate: [[[0, 0], [50, 0], [100, 0]]],
})) {
  for (const [gap, angle, step] of [
    [4, 49, 1],
    [8, 0, 1],
    [8, 131, 8],
    [2.5, -41, 1],
    [0.01, 90, 1],
  ]) {
    hachureFill.push({
      name: `${label}/g${gap}/a${angle}/o${step}`,
      call: "hachureLines",
      args: [polygons, gap, angle, step],
    });
  }
}

const OPTION_SWEEP = [
  ["bowing0", { bowing: 0 }],
  ["bowing3", { bowing: 3 }],
  ["maxOffset0", { maxRandomnessOffset: 0 }],
  ["maxOffset5", { maxRandomnessOffset: 5 }],
  ["roughness0.5", { roughness: 0.5 }],
  ["singleStroke", { disableMultiStroke: true }],
  ["preserveVertices", { preserveVertices: true }],
  ["strokeWidth4", { strokeWidth: 4 }],
];
const ELLIPTIC_SWEEP = [
  ["curveFitting1", { curveFitting: 1 }],
  ["curveFitting0.5", { curveFitting: 0.5 }],
  ["curveStepCount4", { curveStepCount: 4 }],
  ["curveStepCount20", { curveStepCount: 20 }],
  ["singleStroke", { disableMultiStroke: true }],
  ["roughness0.5", { roughness: 0.5 }],
];

function withSweep(variants, sweep, seed = 12345) {
  return [...variants, ...sweep.map(([label, o]) => [`sweep/${label}`, { seed, roughness: 1, ...o }])];
}

const outlineLinear = [
  ...Object.entries(LINES).flatMap(([label, a]) =>
    cross(`line/${label}`, "line", (o) => [...a, o], withSweep(seedRoughness(), OPTION_SWEEP)),
  ),
  ...Object.entries(POLYLINES).flatMap(([label, p]) =>
    cross(`linearPath/${label}`, "linearPath", (o) => [p, o], seedRoughness()),
  ),
  ...Object.entries(POLYGONS).flatMap(([label, p]) =>
    cross(`polygon/${label}`, "polygon", (o) => [p, o], withSweep(seedRoughness(), OPTION_SWEEP)),
  ),
  ...Object.entries(RECTS).flatMap(([label, r]) =>
    cross(`rectangle/${label}`, "rectangle", (o) => [...r, o], seedRoughness()),
  ),
  ...Object.entries(CURVES).flatMap(([label, p]) =>
    cross(
      `curve/${label}`,
      "curve",
      (o) => [p, o],
      withSweep(seedRoughness(), [...OPTION_SWEEP, ["tightness0.5", { curveTightness: 0.5 }]]),
    ),
  ),
  // seed 0 makes rough.js fall back to Math.random: only the structure is reproducible.
  { name: "seed0/line", call: "line", args: [...LINES.mid, { seed: 0, roughness: 1 }] },
  { name: "seed0/rectangle", call: "rectangle", args: [...RECTS.normal, { seed: 0, roughness: 1 }] },
];

const outlineElliptic = [
  ...Object.entries(ELLIPSES).flatMap(([label, e]) =>
    cross(`ellipse/${label}`, "ellipse", (o) => [...e, o], withSweep(seedRoughness(), ELLIPTIC_SWEEP)),
  ),
  ...cross("circle", "circle", (o) => [50, 50, 90, o], seedRoughness()),
  ...Object.entries(ARCS).flatMap(([label, a]) =>
    cross(`arc/${label}`, "arc", (o) => [...a, o], withSweep(seedRoughness(), ELLIPTIC_SWEEP)),
  ),
];

const outlinePath = [
  ...Object.entries(PATHS).flatMap(([label, d]) =>
    cross(
      `path/${label}`,
      "path",
      (o) => [d, o],
      withSweep(seedRoughness(), [
        ["simplification0.5", { simplification: 0.5 }],
        ["singleStroke", { disableMultiStroke: true }],
        ["preserveVertices", { preserveVertices: true }],
      ]),
    ),
  ),
  ...Object.entries(BAD_PATHS).map(([label, d]) => ({ name: `path/${label}`, call: "path", args: [d, { seed: 1 }] })),
  { name: "path/empty", call: "path", args: ["", { seed: 1 }] },
  { name: "path/whitespace", call: "path", args: ["   ", { seed: 1 }] },
];

// Pattern fills emit a stroke per hachure line, so fill cases use small shapes: the
// generated JSON stays a few hundred KiB per style while every code path still runs.
const FILL_SHAPES = {
  rectangle: ["rectangle", (o) => [5, 10, 60, 40, o]],
  "polygon/concave": ["polygon", (o) => [[[0, 0], [60, 0], [60, 45], [30, 18], [0, 45]], o]],
  ellipse: ["ellipse", (o) => [35, 25, 60, 40, o]],
  "arc/closed": ["arc", (o) => [35, 25, 60, 40, -Math.PI / 2, Math.PI, true, o]],
  "curve/wave": ["curve", (o) => [[[0, 0], [15, 30], [35, -10], [55, 25], [70, 0]], o]],
  "path/roundedRect": ["path", (o) => ["M 10 0 L 50 0 Q 60 0, 60 10 L 60 30 Q 60 40, 50 40 L 10 40 Q 0 40, 0 30 L 0 10 Q 0 0, 10 0", o]],
  "path/subpaths": ["path", (o) => ["M 0 0 L 25 0 L 25 25 Z M 35 0 L 60 0 L 48 25 Z", o]],
};

/** Every fillable shape × seed × roughness, plus `sweep` variants, with `fillOptions` merged in. */
function fillable(fillOptions, sweep, shapes = FILL_SHAPES) {
  const cases = [];
  for (const [label, [call, argsFor]] of Object.entries(shapes)) {
    for (const [variant, o] of withSweep(seedRoughness({}, FILL_SEEDS), sweep)) {
      cases.push({ name: `${label}/${variant}`, call, args: argsFor({ fill: "#e03131", ...fillOptions, ...o }) });
    }
  }
  return cases;
}

/** Cases where the fill branch is skipped or special-cased; one style is enough. */
const FILL_EDGES = [
  { name: "edge/circle", call: "circle", args: [30, 30, 50, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure", hachureGap: 8 }] },
  { name: "edge/arcOpen", call: "arc", args: [35, 25, 60, 40, 0, Math.PI, false, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure" }] },
  { name: "edge/curveTwoPoints", call: "curve", args: [[[0, 0], [60, 30]], { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure" }] },
  { name: "edge/pathTransparent", call: "path", args: ["M 0 0 L 60 0 L 60 40 Z", { seed: 1, roughness: 1, fill: "transparent", fillStyle: "hachure" }] },
  { name: "edge/emptyFillString", call: "rectangle", args: [0, 0, 60, 40, { seed: 1, roughness: 1, fill: "", fillStyle: "hachure" }] },
  { name: "edge/noStroke", call: "rectangle", args: [0, 0, 60, 40, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "hachure", hachureGap: 8, stroke: "none" }] },
  { name: "edge/unknownStyle", call: "rectangle", args: [0, 0, 60, 40, { seed: 1, roughness: 1, fill: "#e03131", fillStyle: "bogus", hachureGap: 8 }] },
];

const PATTERN_SWEEP = [
  ["hachureAngle0", { hachureAngle: 0 }],
  ["hachureGap12", { hachureGap: 12 }],
  ["fillWeight3", { fillWeight: 3 }],
  ["singleStrokeFill", { disableMultiStrokeFill: true }],
];

export const groups = {
  random,
  path_data: pathData,
  points_on_curve: pointsOnCurve,
  points_on_path: pointsOnPath,
  hachure_fill: hachureFill,
  outline_linear: outlineLinear,
  outline_elliptic: outlineElliptic,
  outline_path: outlinePath,
  fill_solid: [
    ...fillable({ fillStyle: "solid" }, [["gain0", { fillShapeRoughnessGain: 0 }]]),
    // No subpaths at all: the multi-set branch runs solidFillPolygon on an empty list.
    { name: "path/whitespace", call: "path", args: ["   ", { seed: 1, fill: "#f00", fillStyle: "solid" }] },
  ],
  fill_hachure: [...fillable({ fillStyle: "hachure", hachureGap: 8 }, PATTERN_SWEEP), ...FILL_EDGES],
  fill_cross_hatch: fillable({ fillStyle: "cross-hatch", hachureGap: 8 }, PATTERN_SWEEP),
  fill_zigzag: fillable({ fillStyle: "zigzag", hachureGap: 8 }, PATTERN_SWEEP),
  // Dot positions come from Math.random, so these are structure-only; two shapes suffice.
  fill_dots: fillable({ fillStyle: "dots", hachureGap: 12 }, [], {
    rectangle: FILL_SHAPES.rectangle,
    ellipse: FILL_SHAPES.ellipse,
  }),
  fill_dashed: fillable({ fillStyle: "dashed", hachureGap: 8 }, [...PATTERN_SWEEP, ["dash", { dashOffset: 3, dashGap: 5 }]]),
  fill_zigzag_line: fillable({ fillStyle: "zigzag-line", hachureGap: 8 }, [
    ...PATTERN_SWEEP,
    ["zigzagOffset3", { zigzagOffset: 3 }],
  ]),
};
```

`outline_*` 三組刻意不含任何 `fill`：它們在 Task 6 到 Task 8 就要通過，那時填充還沒有 port。

- [ ] **Step 6：建立 `tools/baseline/rough/generate.mjs`**

```js
// Generates crates/rough/tests/baseline/*.json from roughjs@4.6.4 and the dependency
// versions Excalidraw's yarn.lock resolves at commit afa3a653fc5d2b742adcbd5a6063187b056d2419.
//
//   cd tools/baseline && npm ci && npm run rough

import { join } from "node:path";

import { REPO_ROOT, assertVersions, bundleAndImport, resolveFrom, runCase, writeGroup } from "../lib/harness.mjs";
import { groups } from "./cases.mjs";

// Versions from Excalidraw's yarn.lock at the pinned commit. roughjs's own
// package.json only gives ranges (^0.5.2 ...), so a fresh install could drift.
assertVersions({
  roughjs: { pkg: "roughjs", from: null, version: "4.6.4" },
  "hachure-fill": { pkg: "hachure-fill", from: "roughjs", version: "0.5.2" },
  "path-data-parser": { pkg: "path-data-parser", from: "roughjs", version: "0.1.0" },
  "points-on-curve": { pkg: "points-on-curve", from: "roughjs", version: "0.2.0" },
  "points-on-path": { pkg: "points-on-path", from: "roughjs", version: "0.2.1" },
});

// Excalidraw imports roughjs/bin/* (the unminified tsc output), which Node cannot load
// directly because its internal imports omit file extensions; esbuild resolves them the
// same way Excalidraw's Vite build does. Dependencies are resolved from roughjs's own
// location so they are the copies roughjs runs with.
const lib = await bundleAndImport(
  "rough",
  `
  export { RoughGenerator } from ${JSON.stringify(resolveFrom("roughjs", "roughjs/bin/generator.js"))};
  export { Random } from ${JSON.stringify(resolveFrom("roughjs", "roughjs/bin/math.js"))};
  export { hachureLines } from ${JSON.stringify(resolveFrom("roughjs", "hachure-fill"))};
  export { parsePath, absolutize, normalize } from ${JSON.stringify(resolveFrom("roughjs", "path-data-parser"))};
  export { pointsOnBezierCurves, simplify } from ${JSON.stringify(resolveFrom("roughjs", "points-on-curve"))};
  export { curveToBezier } from ${JSON.stringify(resolveFrom("roughjs", "points-on-curve/lib/curve-to-bezier.js"))};
  export { pointsOnPath } from ${JSON.stringify(resolveFrom("roughjs", "points-on-path"))};
  `,
);

/**
 * Calls a RoughGenerator method on a fresh generator. rough.js's `_o(undefined)` returns the
 * generator's own defaultOptions object, and the first random draw then attaches a seed-0
 * randomizer to it that every later call copies, so one options-less call would turn the
 * rest of the run into Math.random. A fresh generator per case keeps cases independent.
 */
function generate(method, args) {
  return drawable(new lib.RoughGenerator()[method](...args));
}

/** The drawable as JSON, without the randomizer object rough.js leaves on its options. */
function drawable(d) {
  const { randomizer, ...options } = d.options;
  return { shape: d.shape, options, sets: d.sets.map(({ type, ops }) => ({ type, ops })) };
}

const calls = {
  Random: (seed, count) => {
    const random = new lib.Random(seed);
    return Array.from({ length: count }, () => random.next());
  },
  parsePath: (d) => lib.parsePath(d),
  absolutize: (d) => lib.absolutize(lib.parsePath(d)),
  normalize: (d) => lib.normalize(lib.absolutize(lib.parsePath(d))),
  pointsOnBezierCurves: (points, tolerance, distance) => lib.pointsOnBezierCurves(points, tolerance, distance),
  simplify: (points, distance) => lib.simplify(points, distance),
  curveToBezier: (points, curveTightness) => lib.curveToBezier(points, curveTightness),
  pointsOnPath: (d, tolerance, distance) => lib.pointsOnPath(d, tolerance, distance),
  // hachureLines rotates the caller's polygons in place and back again; the float drift
  // it leaves behind is observable (cross-hatch fills the same polygons twice).
  hachureLines: (polygons, gap, angle, stepOffset) => {
    const lines = lib.hachureLines(polygons, gap, angle, stepOffset);
    return { lines, polygons };
  },
  line: (...args) => generate("line", args),
  rectangle: (...args) => generate("rectangle", args),
  ellipse: (...args) => generate("ellipse", args),
  circle: (...args) => generate("circle", args),
  linearPath: (...args) => generate("linearPath", args),
  arc: (...args) => generate("arc", args),
  curve: (...args) => generate("curve", args),
  polygon: (...args) => generate("polygon", args),
  path: (...args) => generate("path", args),
};

const outDir = join(REPO_ROOT, "crates", "rough", "tests", "baseline");
const source = "roughjs@4.6.4 (hachure-fill@0.5.2, path-data-parser@0.1.0, points-on-curve@0.2.0, points-on-path@0.2.1)";

for (const [group, specs] of Object.entries(groups)) {
  const cases = specs.map(({ name, call, args }) => {
    // structuredClone: calls such as hachureLines mutate their arguments, and the
    // recorded args must be what the Rust side receives.
    const recordedArgs = structuredClone(args);
    const { compare, expected } = runCase(name, () => calls[call](...structuredClone(args)));
    return { name, call, args: recordedArgs, compare, expected };
  });
  writeGroup(outDir, group, source, cases);
}
```

- [ ] **Step 7：產生基準**

Run: `cd tools/baseline && npm run -s rough`
Expected: 輸出下面 15 行（KiB 數字可以有 ±1 的差異，案例數與 structure-only 數必須完全相同）：

```
random: 7 cases (0 structure-only), 2 KiB
path_data: 39 cases (0 structure-only), 13 KiB
points_on_curve: 20 cases (0 structure-only), 12 KiB
points_on_path: 30 cases (0 structure-only), 15 KiB
hachure_fill: 25 cases (0 structure-only), 31 KiB
outline_linear: 221 cases (2 structure-only), 289 KiB
outline_elliptic: 84 cases (0 structure-only), 388 KiB
outline_path: 125 cases (0 structure-only), 304 KiB
fill_solid: 50 cases (0 structure-only), 205 KiB
fill_hachure: 77 cases (0 structure-only), 444 KiB
fill_cross_hatch: 70 cases (0 structure-only), 624 KiB
fill_zigzag: 70 cases (0 structure-only), 577 KiB
fill_dots: 12 cases (12 structure-only), 152 KiB
fill_dashed: 77 cases (0 structure-only), 532 KiB
fill_zigzag_line: 77 cases (0 structure-only), 680 KiB
```

- [ ] **Step 8：確認可重現**

Run: `git add crates/rough/tests/baseline && cd tools/baseline && npm run -s rough > /dev/null && cd ../.. && git diff --exit-code -- crates/rough/tests/baseline && echo reproducible`
Expected: 印出 `reproducible`。

- [ ] **Step 9：Commit**

```bash
git add tools/baseline crates/rough/tests/baseline
git commit -m "Add rough.js baseline generator and recorded baselines"
```

---

### Task 2：Workspace、`testkit` 與 `Random`

**Files:**
- Create: `Cargo.toml`
- Create: `crates/testkit/Cargo.toml`
- Create: `crates/testkit/src/lib.rs`
- Create: `crates/rough/Cargo.toml`
- Create: `crates/rough/src/lib.rs`
- Create: `crates/rough/src/js.rs`
- Create: `crates/rough/src/math.rs`
- Create: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 1 的基準檔格式。
- Produces:
  - `testkit::{TOLERANCE, Compare, Case, load_group(&Path) -> Vec<Case>, num(&Value) -> f64, to_value(f64) -> Value, point_value([f64; 2]) -> Value, points_from(&Value) -> Vec<[f64; 2]>, diff(&Value, &Value, Compare) -> Option<String>, check_group(&Path, &str, impl Fn(&Case) -> Value)}`；`Case::num(&self, usize) -> f64`。
  - `rough::js::{to_int32(f64) -> i32, math_round(f64) -> f64}`
  - `rough::math::{Random, Random::new(f64) -> Random, Random::next(&mut self) -> f64, math_random() -> f64}`

- [ ] **Step 1：建立 `Cargo.toml`（workspace）**

```toml
[workspace]
members = ["crates/rough", "crates/testkit"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.98"
publish = false

[workspace.dependencies]
rough = { path = "crates/rough" }
serde_json = { version = "1", features = ["float_roundtrip"] }
testkit = { path = "crates/testkit" }
```

members 逐一列出而不用 `crates/*`：M2 的 Task 1 會在 `crates/scene/tests/` 寫基準檔，那時 `crates/scene/Cargo.toml` 還不存在，萬用字元會讓整個 workspace 無法載入。`float_roundtrip` 讓 serde_json 解析基準裡的數字時不會差一個 ulp。

- [ ] **Step 2：建立 `crates/testkit/Cargo.toml`**

```toml
[package]
name = "testkit"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
serde_json.workspace = true
```

- [ ] **Step 3：建立 `crates/testkit/src/lib.rs`**

```rust
//! Reads the JSON baselines written by `tools/baseline` and compares Rust output with them.
//!
//! A baseline group is `{ "source": ..., "cases": [ { name, call, args, compare, expected } ] }`.
//! JSON cannot hold NaN or ±Infinity, so the generator writes them as the strings `"NaN"`,
//! `"Infinity"` and `"-Infinity"`; [`num`] and [`to_value`] translate both ways.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use serde_json::{Map, Value};

/// Absolute tolerance for every compared number (spec §9.1). V8 and glibc may disagree in
/// the last bit of `sin`/`cos`; everything else in the ports is exact arithmetic.
pub const TOLERANCE: f64 = 1e-9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    /// Every number must be within [`TOLERANCE`].
    Exact,
    /// The case used `Math.random`: numbers are ignored, everything else must match.
    Structure,
}

#[derive(Debug)]
pub struct Case {
    pub name: String,
    pub call: String,
    pub args: Vec<Value>,
    pub compare: Compare,
    pub expected: Value,
}

impl Case {
    /// Positional argument `i` as a number.
    pub fn num(&self, i: usize) -> f64 {
        num(&self.args[i])
    }
}

pub fn load_group(path: &Path) -> Vec<Case> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read baseline {}: {e}", path.display()));
    let root: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("invalid baseline {}: {e}", path.display()));
    root["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: missing cases array", path.display()))
        .iter()
        .map(|case| Case {
            name: case["name"].as_str().expect("case name").to_owned(),
            call: case["call"].as_str().expect("case call").to_owned(),
            args: case["args"].as_array().expect("case args").clone(),
            compare: match case["compare"].as_str() {
                Some("exact") => Compare::Exact,
                Some("structure") => Compare::Structure,
                other => panic!("unknown compare mode {other:?}"),
            },
            expected: case["expected"].clone(),
        })
        .collect()
}

/// Decodes a baseline number, including the three non-finite spellings.
pub fn num(value: &Value) -> f64 {
    as_number(value).unwrap_or_else(|| panic!("expected a number, got {value}"))
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => match s.as_str() {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

/// Encodes a number the way the baseline generator does.
pub fn to_value(x: f64) -> Value {
    if x.is_nan() {
        Value::from("NaN")
    } else if x == f64::INFINITY {
        Value::from("Infinity")
    } else if x == f64::NEG_INFINITY {
        Value::from("-Infinity")
    } else {
        Value::from(x)
    }
}

pub fn point_value(p: [f64; 2]) -> Value {
    Value::Array(vec![to_value(p[0]), to_value(p[1])])
}

pub fn points_from(value: &Value) -> Vec<[f64; 2]> {
    value
        .as_array()
        .expect("array of points")
        .iter()
        .map(|p| [num(&p[0]), num(&p[1])])
        .collect()
}

fn numbers_match(expected: f64, actual: f64) -> bool {
    if expected.is_nan() || actual.is_nan() {
        return expected.is_nan() && actual.is_nan();
    }
    if expected.is_infinite() || actual.is_infinite() {
        return expected == actual;
    }
    (expected - actual).abs() <= TOLERANCE
}

/// The first difference between `expected` and `actual`, as `"path: description"`.
pub fn diff(expected: &Value, actual: &Value, compare: Compare) -> Option<String> {
    diff_at("$", expected, actual, compare)
}

fn diff_at(path: &str, expected: &Value, actual: &Value, compare: Compare) -> Option<String> {
    if let (Some(e), Some(a)) = (as_number(expected), as_number(actual)) {
        return match compare {
            Compare::Structure => None,
            Compare::Exact if numbers_match(e, a) => None,
            Compare::Exact => Some(format!(
                "{path}: expected {e:?}, got {a:?} (delta {:e})",
                a - e
            )),
        };
    }
    match (expected, actual) {
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                return Some(format!(
                    "{path}: expected {} items, got {}",
                    e.len(),
                    a.len()
                ));
            }
            e.iter()
                .zip(a)
                .enumerate()
                .find_map(|(i, (e, a))| diff_at(&format!("{path}[{i}]"), e, a, compare))
        }
        (Value::Object(e), Value::Object(a)) => diff_objects(path, e, a, compare),
        _ if expected == actual => None,
        _ => Some(format!("{path}: expected {expected}, got {actual}")),
    }
}

fn diff_objects(
    path: &str,
    expected: &Map<String, Value>,
    actual: &Map<String, Value>,
    compare: Compare,
) -> Option<String> {
    if let Some(key) = expected.keys().find(|k| !actual.contains_key(*k)) {
        return Some(format!("{path}: missing key {key:?}"));
    }
    if let Some(key) = actual.keys().find(|k| !expected.contains_key(*k)) {
        return Some(format!("{path}: unexpected key {key:?}"));
    }
    expected
        .iter()
        .find_map(|(k, e)| diff_at(&format!("{path}.{k}"), e, &actual[k], compare))
}

/// Runs every case of `dir/<group>.json` through `run` and panics with a report of each
/// failing case. A panic inside `run` fails that case only.
///
/// Set `BASELINE_CASE=<substring>` to run only cases whose name contains it.
pub fn check_group(dir: &Path, group: &str, run: impl Fn(&Case) -> Value) {
    let cases = load_group(&dir.join(format!("{group}.json")));
    assert!(!cases.is_empty(), "{group}: baseline has no cases");
    let filter = std::env::var("BASELINE_CASE").ok();
    let mut ran = 0;
    let mut failures = Vec::new();
    for case in &cases {
        if filter.as_deref().is_some_and(|f| !case.name.contains(f)) {
            continue;
        }
        ran += 1;
        match catch_unwind(AssertUnwindSafe(|| run(case))) {
            Ok(actual) => {
                if let Some(d) = diff(&case.expected, &actual, case.compare) {
                    failures.push(format!("{}: {d}", case.name));
                }
            }
            Err(panic) => {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("<non-string panic>");
                failures.push(format!("{}: panicked: {message}", case.name));
            }
        }
    }
    assert!(ran > 0, "{group}: BASELINE_CASE matched no case");
    if !failures.is_empty() {
        let shown: Vec<_> = failures.iter().take(20).map(|f| format!("  {f}")).collect();
        panic!(
            "{group}: {} of {ran} cases failed\n{}{}",
            failures.len(),
            shown.join("\n"),
            if failures.len() > 20 { "\n  ..." } else { "" }
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn exact_rejects_difference_above_tolerance() {
        let d = diff(&json!([1.0]), &json!([1.0 + 2e-9]), Compare::Exact);
        assert!(d.unwrap().starts_with("$[0]: expected 1.0"));
    }

    #[test]
    fn exact_accepts_difference_within_tolerance() {
        assert_eq!(
            diff(
                &json!({"a": [1.0]}),
                &json!({"a": [1.0 + 5e-10]}),
                Compare::Exact
            ),
            None
        );
    }

    #[test]
    fn nan_matches_only_nan() {
        assert_eq!(
            diff(&json!("NaN"), &to_value(f64::NAN), Compare::Exact),
            None
        );
        assert!(diff(&json!("NaN"), &json!(0.0), Compare::Exact).is_some());
    }

    #[test]
    fn structure_ignores_numbers_but_not_shape() {
        assert_eq!(
            diff(&json!([1, 2]), &json!([5, 6]), Compare::Structure),
            None
        );
        assert!(diff(&json!([1, 2]), &json!([5]), Compare::Structure).is_some());
        assert!(
            diff(
                &json!({"op": "move"}),
                &json!({"op": "lineTo"}),
                Compare::Structure
            )
            .is_some()
        );
    }

    #[test]
    fn key_sets_must_match() {
        assert!(
            diff(&json!({"a": 1}), &json!({}), Compare::Exact)
                .unwrap()
                .contains("missing key")
        );
        assert!(
            diff(&json!({}), &json!({"a": 1}), Compare::Exact)
                .unwrap()
                .contains("unexpected key")
        );
    }

    #[test]
    fn check_group_reports_failing_case() {
        let dir = std::env::temp_dir().join(format!("testkit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("g.json"),
            r#"{"source":"t","cases":[
{"name":"ok","call":"id","args":[1],"compare":"exact","expected":1},
{"name":"bad","call":"id","args":[2],"compare":"exact","expected":3}
]}"#,
        )
        .unwrap();
        let result = catch_unwind(|| check_group(&dir, "g", |case| case.args[0].clone()));
        let message = *result.unwrap_err().downcast::<String>().unwrap();
        assert!(message.contains("1 of 2 cases failed"), "{message}");
        assert!(message.contains("bad: $: expected 3"), "{message}");
    }
}
```

測試保護的是「比對寬鬆到什麼都會過」這種靜默失效：超過容差一定要失敗、結構模式仍然檢查形狀、多出或缺少 key 都算不同。

- [ ] **Step 4：跑 `testkit` 的測試**

Run: `cargo test -p testkit`
Expected: 6 passed。

- [ ] **Step 5：建立 `crates/rough/Cargo.toml`**

```toml
[package]
name = "rough"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

# No dependencies: rough is a line-by-line port of roughjs@4.6.4 and knows nothing
# about Excalidraw (spec §4.2).
[dependencies]

[dev-dependencies]
serde_json.workspace = true
testkit.workspace = true
```

- [ ] **Step 6：先寫 `random` 基準測試 `crates/rough/tests/baseline.rs`**

```rust
//! Compares the port with roughjs@4.6.4 output recorded in `tests/baseline/*.json`.
//! One test per baseline group, so each porting task turns exactly its groups green.

use std::path::PathBuf;

use rough::math::Random;
use serde_json::Value;
use testkit::{check_group, to_value};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/baseline")
}

#[test]
fn random() {
    check_group(&dir(), "random", |case| {
        let mut random = Random::new(case.num(0));
        Value::Array(
            (0..case.num(1) as usize)
                .map(|_| to_value(random.next()))
                .collect(),
        )
    });
}
```

同時建立 `crates/rough/src/lib.rs`：

```rust
//! Line-by-line port of roughjs@4.6.4 (`bin/*.js` in the npm package) and the dependency
//! versions Excalidraw's yarn.lock resolves for it. Baselines in `tests/baseline/` come from
//! `tools/baseline/rough/generate.mjs`.

pub mod js;
pub mod math;
```

- [ ] **Step 7：確認測試因為缺模組而失敗**

Run: `cargo test -p rough --test baseline random`
Expected: 編譯失敗，錯誤是找不到 `js`、`math` 模組。

- [ ] **Step 8：建立 `crates/rough/src/js.rs`**

```rust
//! JavaScript number semantics the port depends on. Rust's own operators differ from these
//! in ways that change output (rounding direction, integer wrap-around), so every call site
//! that mirrors one of these JS operations uses the helper, never the Rust look-alike.

/// ECMAScript `ToInt32`, as applied by `Math.imul` and bitwise operators.
pub fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc().rem_euclid(4_294_967_296.0);
    (if m >= 2_147_483_648.0 {
        m - 4_294_967_296.0
    } else {
        m
    }) as i32
}

/// `Math.round`: halves round towards +∞ (`Math.round(-2.5) == -2`), unlike `f64::round`.
pub fn math_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_int32_wraps_like_javascript() {
        assert_eq!(to_int32(2_147_483_648.0), i32::MIN);
        assert_eq!(to_int32(4_294_967_297.0), 1);
        assert_eq!(to_int32(-7.9), -7);
        assert_eq!(to_int32(f64::NAN), 0);
    }

    #[test]
    fn math_round_rounds_halves_up() {
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(0.49999999999999994), 0.0);
    }
}
```

- [ ] **Step 9：建立 `crates/rough/src/math.rs`**

```rust
//! `bin/math.js`.

use std::cell::Cell;
use std::hash::{BuildHasher, Hasher, RandomState};

use crate::js::to_int32;

/// rough.js `Random`. The state is a JS number, so a falsy seed (0 or NaN) makes every
/// draw fall back to `Math.random`.
#[derive(Clone, Debug)]
pub struct Random {
    seed: f64,
}

impl Random {
    pub fn new(seed: f64) -> Self {
        Random { seed }
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "keeps rough.js's name; not an iterator"
    )]
    pub fn next(&mut self) -> f64 {
        if self.seed != 0.0 && !self.seed.is_nan() {
            let state = to_int32(self.seed).wrapping_mul(48271);
            self.seed = f64::from(state);
            f64::from(state & 0x7FFF_FFFF) / 2_147_483_648.0
        } else {
            math_random()
        }
    }
}

/// Stand-in for `Math.random`, reached only where rough.js itself calls it (seed 0, the
/// dots filler, hachure skip offsets without a randomizer). Not reproducible, by design.
pub fn math_random() -> f64 {
    thread_local! {
        static SOURCE: (RandomState, Cell<u64>) = (RandomState::new(), const { Cell::new(0) });
    }
    SOURCE.with(|(state, counter)| {
        let n = counter.get();
        counter.set(n.wrapping_add(1));
        let mut hasher = state.build_hasher();
        hasher.write_u64(n);
        (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_zero_is_not_reproducible() {
        let a: Vec<f64> = (0..4).map(|_| Random::new(0.0).next()).collect();
        assert!(a.windows(2).any(|w| w[0] != w[1]), "{a:?}");
        assert!(a.iter().all(|x| (0.0..1.0).contains(x)));
    }
}
```

`seed_zero_is_not_reproducible` 對應 spec §4.2「seed 為 0 時的行為也要照原版」：原版在 seed 0 時每次抽到的都是 `Math.random()`。

- [ ] **Step 10：跑測試**

Run: `cargo test -p rough`
Expected: 單元測試 3 passed；`baseline` 的 `random` 1 passed。

- [ ] **Step 11：突變檢查**

把 `math.rs` 的 `48271` 暫時改成 `48270`，跑 `cargo test -p rough --test baseline random`。
Expected: `random: 7 of 7 cases failed`。改回 `48271`。

- [ ] **Step 12：格式與 lint**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 沒有輸出，exit 0。

- [ ] **Step 13：Commit**

```bash
git add Cargo.toml Cargo.lock crates/testkit crates/rough/Cargo.toml crates/rough/src crates/rough/tests/baseline.rs
git commit -m "Add cargo workspace, baseline testkit and rough.js Random"
```

---

### Task 3：path-data-parser

**Files:**
- Create: `crates/rough/src/path_data.rs`
- Modify: `crates/rough/src/js.rs`（加 `to_fixed`）
- Modify: `crates/rough/src/lib.rs`（加 `pub mod path_data;`）
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: `rough::js::to_int32`、`math_round`。
- Produces:
  - `rough::js::to_fixed(x: f64, digits: usize) -> String`
  - `rough::path_data::Segment { pub key: char, pub data: Vec<f64> }`（derive `Clone, Debug, PartialEq`）
  - `rough::path_data::PathError`：`InvalidCharacter`、`ParamNotNumber { mode: char, token: String }`、`EndedShort`；`Display` 分別印出 `TypeError`、`Param not a number: {mode},{token}`、`Path data ended short`；實作 `std::error::Error`。
  - `parse_path(d: &str) -> Result<Vec<Segment>, PathError>`、`absolutize(segments: &[Segment]) -> Vec<Segment>`、`normalize(segments: &[Segment]) -> Vec<Segment>`

- [ ] **Step 1：在 `baseline.rs` 加測試**

在檔案開頭的 `use` 區換成：

```rust
use std::fmt::Display;
use std::path::PathBuf;

use rough::math::Random;
use rough::path_data::{self, Segment};
use serde_json::{Value, json};
use testkit::{check_group, to_value};
```

在 `dir()` 後面加：

```rust
fn throws(error: impl Display) -> Value {
    json!({ "throws": error.to_string() })
}

fn numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().copied().map(to_value).collect())
}

fn segments_value(segments: &[Segment]) -> Value {
    Value::Array(
        segments
            .iter()
            .map(|s| json!({ "key": s.key.to_string(), "data": numbers(&s.data) }))
            .collect(),
    )
}
```

在檔案最後加：

```rust
#[test]
fn path_data() {
    check_group(&dir(), "path_data", |case| {
        let parsed = match path_data::parse_path(case.args[0].as_str().expect("path")) {
            Ok(segments) => segments,
            Err(e) => return throws(e),
        };
        match case.call.as_str() {
            "parsePath" => segments_value(&parsed),
            "absolutize" => segments_value(&path_data::absolutize(&parsed)),
            "normalize" => segments_value(&path_data::normalize(&path_data::absolutize(&parsed))),
            other => panic!("unknown call {other}"),
        }
    });
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p rough --test baseline path_data`
Expected: 編譯失敗，找不到 `rough::path_data`。

- [ ] **Step 3：在 `js.rs` 加 `to_fixed` 與測試**

在 `math_round` 後面加：

```rust
/// `Number.prototype.toFixed(digits)` for `|x| < 1e21`. JS rounds the exact binary value
/// half away from zero; Rust's `{:.N}` rounds ties to even, so `0.0009765625` (exactly
/// representable) gives `0.000976563` here and `0.000976562` with `format!`.
pub fn to_fixed(x: f64, digits: usize) -> String {
    // 1100 fractional digits hold the exact expansion of any f64.
    let exact = format!("{:.1100}", x.abs());
    let (int_part, frac) = exact.split_once('.').expect("fixed-point formatting");
    let mut kept: Vec<u8> = int_part.bytes().chain(frac.bytes().take(digits)).collect();
    if frac.as_bytes()[digits] >= b'5' {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let split = kept.len() - digits;
    let digits_str = String::from_utf8(kept).expect("ascii digits");
    let sign = if x < 0.0 { "-" } else { "" };
    if digits == 0 {
        format!("{sign}{digits_str}")
    } else {
        format!("{sign}{}.{}", &digits_str[..split], &digits_str[split..])
    }
}
```

在 `tests` 模組裡加：

```rust
    #[test]
    fn to_fixed_matches_javascript() {
        // Expected strings from node: `x.toFixed(9)`.
        for (x, expected) in [
            (0.0009765625, "0.000976563"),
            (-0.0009765625, "-0.000976563"),
            (0.1, "0.100000000"),
            (-0.5000000005, "-0.500000001"),
            (1.0 / 3.0, "0.333333333"),
            (0.9999999995, "0.999999999"),
            (1.0000000005, "1.000000001"),
            (-2.0 / 3.0, "-0.666666667"),
            (123.4560000005, "123.456000000"),
            (0.0, "0.000000000"),
            (3.0517578125e-5, "0.000030518"),
            (-0.0000000001, "-0.000000000"),
            (0.99999999999, "1.000000000"),
        ] {
            assert_eq!(to_fixed(x, 9), expected, "{x}");
        }
    }
```

- [ ] **Step 4：port `path_data.rs`**

來源：`tools/baseline/node_modules/path-data-parser/lib/parser.js`（`tokenize`、`parsePath`）、`absolutize.js`、`normalize.js`（含 `arcToCubicCurves`、`rotate`、`degToRad`）。不 port `serialize`。

port 時注意：

- `tokenize` 的三個正規表示式改寫成手寫掃描器，語意照原樣：分隔字元是 `[ \t\r\n,]`；指令字元是 `aAcChHlLmMqQsStTvVzZ`；數字是 `[-+]?[0-9]+(\.[0-9]*)?` 或 `[-+]?\.[0-9]+`，後面可接 `[eE][-+]?[0-9]+`（沒有數字的 `e` 不算指數，數字在 `e` 之前結束）。其他任何字元：JS 回傳空陣列，接著 `parsePath` 讀 `undefined.type` 丟 TypeError，Rust 回傳 `Err(PathError::InvalidCharacter)`。
- 數字 token 的值：JS 做 `+\`${parseFloat(text)}\``，結果等於直接解析比對到的文字；Rust 用 `text.parse::<f64>()`，它接受 `+7`、`.5`、`5.`、`8.25e-1` 這些形式（已驗證）。
- `parsePath` 在第一個 token 不是 `M`/`m` 時遞迴解析 `'M0,0' + d`，保留這個行為。
- `PARAMS` 表、`mode` 在 `M`→`L`、`m`→`l` 的切換照抄。`throw new Error('Bad segment: ' + mode)` 在實際輸入下走不到，不需要對應的錯誤型別。
- `normalize` 的 `A` 分支：`parseFloat(((y1 - cy) / r2).toFixed(9))` 用 `js::to_fixed(v, 9).parse::<f64>()`。`sweepFlag && f1 > f2`、`!sweepFlag` 是數字的真假值；`largeArcFlag === sweepFlag` 是數值相等。遞迴時傳入的 `recursive` 陣列用 `Option<[f64; 4]>`。
- `lastType = key` 在 `switch` 之後、每個 segment 都執行。

- [ ] **Step 5：跑測試**

Run: `cargo test -p rough`
Expected: 單元測試 4 passed（多了 `to_fixed_matches_javascript`）；`random`、`path_data` passed。

- [ ] **Step 6：格式、lint、commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/rough
git commit -m "Port path-data-parser 0.1.0"
```

---

### Task 4：points-on-curve 與 points-on-path

**Files:**
- Create: `crates/rough/src/points_on_curve.rs`
- Create: `crates/rough/src/points_on_path.rs`
- Modify: `crates/rough/src/lib.rs`（加 `pub mod points_on_curve;`、`pub mod points_on_path;`）
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: `rough::path_data::{parse_path, absolutize, normalize, PathError}`。
- Produces（`Point` 在 Task 6 之前先在 `points_on_curve.rs` 裡用 `[f64; 2]`，Task 6 改成 `crate::core::Point` 別名，型別相同）：
  - `rough::points_on_curve::points_on_bezier_curves(points: &[[f64; 2]], tolerance: f64, distance: Option<f64>) -> Vec<[f64; 2]>`
  - `rough::points_on_curve::simplify(points: &[[f64; 2]], distance: f64) -> Vec<[f64; 2]>`（M2 的 freedraw 填充會用）
  - `rough::points_on_curve::curve_to_bezier(points: &[[f64; 2]], curve_tightness: f64) -> Option<Vec<[f64; 2]>>`（少於 3 點回傳 `None`，對應 JS 的 throw）
  - `rough::points_on_path::points_on_path(d: &str, tolerance: f64, distance: Option<f64>) -> Result<Vec<Vec<[f64; 2]>>, PathError>`

- [ ] **Step 1：在 `baseline.rs` 加測試**

`use` 區加上 `use rough::{points_on_curve, points_on_path};`，並把 `testkit` 那行換成 `use testkit::{Case, check_group, num, point_value, points_from, to_value};`。在 `segments_value` 後面加：

```rust
fn points_value(points: &[[f64; 2]]) -> Value {
    Value::Array(points.iter().copied().map(point_value).collect())
}

fn optional_num(case: &Case, i: usize) -> Option<f64> {
    case.args.get(i).map(num)
}
```

檔案最後加：

```rust
#[test]
fn points_on_curve() {
    check_group(&dir(), "points_on_curve", |case| {
        let points = points_from(&case.args[0]);
        match case.call.as_str() {
            "pointsOnBezierCurves" => points_value(&points_on_curve::points_on_bezier_curves(
                &points,
                case.num(1),
                optional_num(case, 2),
            )),
            "simplify" => points_value(&points_on_curve::simplify(&points, case.num(1))),
            "curveToBezier" => match points_on_curve::curve_to_bezier(&points, case.num(1)) {
                Some(out) => points_value(&out),
                None => throws("A curve must have at least three points."),
            },
            other => panic!("unknown call {other}"),
        }
    });
}

#[test]
fn points_on_path() {
    check_group(&dir(), "points_on_path", |case| {
        let d = case.args[0].as_str().expect("path");
        match points_on_path::points_on_path(d, case.num(1), optional_num(case, 2)) {
            Ok(sets) => Value::Array(sets.iter().map(|set| points_value(set)).collect()),
            Err(e) => throws(e),
        }
    });
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p rough --test baseline points_on`
Expected: 編譯失敗，找不到兩個模組。

- [ ] **Step 3：port**

來源：`tools/baseline/node_modules/points-on-curve/lib/index.js`、`lib/curve-to-bezier.js`；`points-on-path/lib/index.js`。

port 時注意：

- `simplifyPoints` 的 `maxNdx` 初始值是 `1`，不是 `start + 1`；照抄。
- `pointsOnBezierCurves` 只在 `distance && distance > 0` 時簡化：`distance` 為 `None`、`0` 或 NaN 都不簡化。
- `getPointsOnBezierCurveWithSplitting` 只有在距離 `> 1` 時才加入分段起點，這是去重的關鍵。
- `points_on_path` 的 `if (!distance) return sets;`：`None`、`0`、NaN 都直接回傳；簡化後長度為 0 的 set 丟掉。
- `pointsOnPath` 內部呼叫 `pointsOnBezierCurves(pendingCurve, tolerance)` 時不帶 `distance`。

- [ ] **Step 4：跑測試**

Run: `cargo test -p rough --test baseline`
Expected: `random`、`path_data`、`points_on_curve`、`points_on_path` passed。

- [ ] **Step 5：格式、lint、commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/rough
git commit -m "Port points-on-curve 0.2.0 and points-on-path 0.2.1"
```

---

### Task 5：hachure-fill

**Files:**
- Create: `crates/rough/src/hachure_fill.rs`
- Modify: `crates/rough/src/lib.rs`（加 `pub mod hachure_fill;`）
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: `rough::js::math_round`。
- Produces: `rough::hachure_fill::hachure_lines(polygons: &mut [Vec<[f64; 2]>], hachure_gap: f64, hachure_angle: f64, hachure_step_offset: f64) -> Vec<[[f64; 2]; 2]>`

- [ ] **Step 1：在 `baseline.rs` 加測試**

`use` 區把 `use rough::{points_on_curve, points_on_path};` 換成 `use rough::{hachure_fill, points_on_curve, points_on_path};`，檔案最後加：

```rust
#[test]
fn hachure_fill() {
    check_group(&dir(), "hachure_fill", |case| {
        let mut polygons: Vec<Vec<[f64; 2]>> = case.args[0]
            .as_array()
            .expect("polygons")
            .iter()
            .map(points_from)
            .collect();
        let lines =
            hachure_fill::hachure_lines(&mut polygons, case.num(1), case.num(2), case.num(3));
        json!({
            "lines": lines.iter().map(|l| points_value(l)).collect::<Vec<_>>(),
            "polygons": polygons.iter().map(|p| points_value(p)).collect::<Vec<_>>(),
        })
    });
}
```

基準同時記錄呼叫後的 `polygons`：JS 會就地旋轉多邊形再轉回來，留下的浮點漂移會影響 cross-hatch 的第二次填充，所以 Rust 也必須就地改寫。

- [ ] **Step 2：確認失敗**

Run: `cargo test -p rough --test baseline hachure_fill`
Expected: 編譯失敗。

- [ ] **Step 3：port**

來源：`tools/baseline/node_modules/hachure-fill/bin/hachure.js`。

port 時注意：

- `if (angle)` 是數字真假值。旋轉用 `Math.cos(angle)`、`Math.sin(angle)` 各算一次再套到每個點；`rotatePoints` 就地改寫。
- `straightHachureLines` 的 `const vertices = [...polygon]` 只是淺複製，補上的收尾點是新陣列，不影響原多邊形。
- 邊表排序比較函數依序比 `ymin`、`x`、`ymax`，最後一項回傳 `(e1.ymax - e2.ymax) / Math.abs(...)`，也就是正負 1。用 `sort_by` 並依回傳值的正負給 `Ordering`。
- `(hachureStepOffset !== 1) || (iteration % gap === 0)`：`%` 是浮點取餘數，Rust 的 `%` 語意相同。
- 線段端點 x 用 `js::math_round`。
- `edges.splice(0, ix + 1)` 用 `drain(..=ix)`（`ix` 為 -1 時不取）；`activeEdges.filter` 用 `retain`。
- 不 port 開頭那段「傳入單一多邊形時自動包成清單」的判斷。

- [ ] **Step 4：跑測試**

Run: `cargo test -p rough --test baseline`
Expected: 前五組 passed。

- [ ] **Step 5：格式、lint、commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/rough
git commit -m "Port hachure-fill 0.5.2"
```

---

### Task 6：核心型別、generator、直線類外框

**Files:**
- Create: `crates/rough/src/core.rs`
- Create: `crates/rough/src/generator.rs`
- Create: `crates/rough/src/renderer.rs`
- Create: `crates/testkit/src/rough_json.rs`
- Modify: `crates/testkit/Cargo.toml`（加 `rough.workspace = true`）
- Modify: `crates/testkit/src/lib.rs`（加 `pub mod rough_json;`）
- Modify: `crates/rough/src/lib.rs`
- Modify: `crates/rough/src/points_on_curve.rs`、`points_on_path.rs`、`hachure_fill.rs`（`[f64; 2]` 改用 `crate::core::Point`）
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 2 到 Task 5 的全部 API。
- Produces（M2 依賴這些公開名稱，不准改名）：
  - `rough::core::{Point, Options, ResolvedOptions, Op, OpSet, OpSetType, Shape, Drawable}`，並在 crate 根目錄 re-export；`ResolvedOptions::merge(&self, &Options) -> ResolvedOptions`；`Op::name`、`Op::data`、`OpSetType::name`、`Shape::name`。
  - `rough::RoughGenerator`：`new() -> Self`、`with_options(&Options) -> Self`、`line(x1, y1, x2, y2, &Options) -> Drawable`、`rectangle(x, y, width, height, &Options)`、`ellipse(x, y, width, height, &Options)`、`circle(x, y, diameter, &Options)`、`linear_path(&[Point], &Options)`、`arc(x, y, width, height, start, stop, closed: bool, &Options)`、`curve(&[Point], &Options)`、`polygon(&[Point], &Options)`、`path(&str, &Options) -> Result<Drawable, PathError>`。所有數字參數都是 `f64`。
  - `testkit::rough_json::{options_from(&Value) -> Options, resolved_value(&ResolvedOptions) -> Value, op_value(&Op) -> Value, drawable_value(&Drawable) -> Value, options_value(&Options) -> Value}`
  - crate 內部（`pub(crate)`，簽名不准改）：
    - `renderer::truthy(f64) -> bool`、`renderer::Ctx`（`new`、`random`、`clone_alter_seed`）
    - 本任務實作：`renderer::{line, linear_path, polygon, rectangle, curve}` 與它們用到的 `_double_line`、`_line`、`_curve_with_offset`、`_curve`、`offset`、`offset_opt`
    - 本任務的 generator 會呼叫、但由後續任務實作的 renderer 函數，先建立簽名、內容寫 `todo!()`：Task 9 的 `solid_fill_polygon(polygon_list: &[Vec<Point>], o: &mut Ctx) -> OpSet`、Task 10 的 `pattern_fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet`。
    - 其餘 renderer 函數在第一個呼叫它的任務才建立，因為 `renderer` 是私有模組，沒被呼叫的 `pub(crate)` 項目會被 `dead_code` lint 擋下：`EllipseParams`、`EllipseResult`、`generate_ellipse_params`、`ellipse_with_params`、`arc`、`pattern_fill_arc`（Task 7）；`svg_path`（Task 8）；`Ctx::existing_random`、`double_line_fill_ops`（Task 10）；`ellipse`（Task 11）。
    - `todo!()` stub 的參數名加 `_` 前綴，避免 `unused_variables` 警告，實作時拿掉。

- [ ] **Step 1：`testkit` 加 rough 依賴與 `rough_json.rs`**

`crates/testkit/Cargo.toml` 的 `[dependencies]` 加 `rough.workspace = true`；`crates/testkit/src/lib.rs` 第一個 `use` 之前加 `pub mod rough_json;`。建立 `crates/testkit/src/rough_json.rs`：

```rust
//! JSON forms of rough types, matching what rough.js objects serialize to. Shared by the
//! rough and scene baseline tests.

use rough::{Drawable, Op, Options, ResolvedOptions};
use serde_json::{Map, Value, json};

use crate::{num, to_value};

fn numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().copied().map(to_value).collect())
}

/// JS options object -> `Options`. Unknown keys fail the case, so a typo in cases.mjs
/// cannot silently fall back to a default.
pub fn options_from(value: &Value) -> Options {
    let mut o = Options::default();
    let object = value.as_object().expect("options object");
    for (key, v) in object {
        let n = || Some(num(v));
        let s = || Some(v.as_str().expect("string option").to_owned());
        let b = || Some(v.as_bool().expect("bool option"));
        let list = || {
            Some(
                v.as_array()
                    .expect("array option")
                    .iter()
                    .map(num)
                    .collect(),
            )
        };
        match key.as_str() {
            "maxRandomnessOffset" => o.max_randomness_offset = n(),
            "roughness" => o.roughness = n(),
            "bowing" => o.bowing = n(),
            "stroke" => o.stroke = s(),
            "strokeWidth" => o.stroke_width = n(),
            "curveFitting" => o.curve_fitting = n(),
            "curveTightness" => o.curve_tightness = n(),
            "curveStepCount" => o.curve_step_count = n(),
            "fill" => o.fill = s(),
            "fillStyle" => o.fill_style = s(),
            "fillWeight" => o.fill_weight = n(),
            "hachureAngle" => o.hachure_angle = n(),
            "hachureGap" => o.hachure_gap = n(),
            "simplification" => o.simplification = n(),
            "dashOffset" => o.dash_offset = n(),
            "dashGap" => o.dash_gap = n(),
            "zigzagOffset" => o.zigzag_offset = n(),
            "seed" => o.seed = n(),
            "strokeLineDash" => o.stroke_line_dash = list(),
            "strokeLineDashOffset" => o.stroke_line_dash_offset = n(),
            "fillLineDash" => o.fill_line_dash = list(),
            "fillLineDashOffset" => o.fill_line_dash_offset = n(),
            "disableMultiStroke" => o.disable_multi_stroke = b(),
            "disableMultiStrokeFill" => o.disable_multi_stroke_fill = b(),
            "preserveVertices" => o.preserve_vertices = b(),
            "fixedDecimalPlaceDigits" => o.fixed_decimal_place_digits = n(),
            "fillShapeRoughnessGain" => o.fill_shape_roughness_gain = n(),
            other => panic!("unknown option {other}"),
        }
    }
    o
}

pub fn resolved_value(o: &ResolvedOptions) -> Value {
    let mut m = Map::new();
    m.insert(
        "maxRandomnessOffset".into(),
        to_value(o.max_randomness_offset),
    );
    m.insert("roughness".into(), to_value(o.roughness));
    m.insert("bowing".into(), to_value(o.bowing));
    m.insert("stroke".into(), json!(o.stroke));
    m.insert("strokeWidth".into(), to_value(o.stroke_width));
    m.insert("curveTightness".into(), to_value(o.curve_tightness));
    m.insert("curveFitting".into(), to_value(o.curve_fitting));
    m.insert("curveStepCount".into(), to_value(o.curve_step_count));
    m.insert("fillStyle".into(), json!(o.fill_style));
    m.insert("fillWeight".into(), to_value(o.fill_weight));
    m.insert("hachureAngle".into(), to_value(o.hachure_angle));
    m.insert("hachureGap".into(), to_value(o.hachure_gap));
    m.insert("dashOffset".into(), to_value(o.dash_offset));
    m.insert("dashGap".into(), to_value(o.dash_gap));
    m.insert("zigzagOffset".into(), to_value(o.zigzag_offset));
    m.insert("seed".into(), to_value(o.seed));
    m.insert("disableMultiStroke".into(), json!(o.disable_multi_stroke));
    m.insert(
        "disableMultiStrokeFill".into(),
        json!(o.disable_multi_stroke_fill),
    );
    m.insert("preserveVertices".into(), json!(o.preserve_vertices));
    m.insert(
        "fillShapeRoughnessGain".into(),
        to_value(o.fill_shape_roughness_gain),
    );
    if let Some(v) = &o.fill {
        m.insert("fill".into(), json!(v));
    }
    if let Some(v) = o.simplification {
        m.insert("simplification".into(), to_value(v));
    }
    if let Some(v) = &o.stroke_line_dash {
        m.insert("strokeLineDash".into(), numbers(v));
    }
    if let Some(v) = o.stroke_line_dash_offset {
        m.insert("strokeLineDashOffset".into(), to_value(v));
    }
    if let Some(v) = &o.fill_line_dash {
        m.insert("fillLineDash".into(), numbers(v));
    }
    if let Some(v) = o.fill_line_dash_offset {
        m.insert("fillLineDashOffset".into(), to_value(v));
    }
    if let Some(v) = o.fixed_decimal_place_digits {
        m.insert("fixedDecimalPlaceDigits".into(), to_value(v));
    }
    Value::Object(m)
}

pub fn op_value(op: &Op) -> Value {
    json!({ "op": op.name(), "data": numbers(op.data()) })
}

pub fn drawable_value(d: &Drawable) -> Value {
    json!({
        "shape": d.shape.name(),
        "options": resolved_value(&d.options),
        "sets": d.sets.iter().map(|set| json!({
            "type": set.kind.name(),
            "ops": set.ops.iter().map(op_value).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// `Options` as the JS object Excalidraw's `generateRoughOptions` returns: only set keys.
pub fn options_value(o: &Options) -> Value {
    let mut m = Map::new();
    let mut n = |key: &str, v: Option<f64>| {
        if let Some(v) = v {
            m.insert(key.into(), to_value(v));
        }
    };
    n("maxRandomnessOffset", o.max_randomness_offset);
    n("roughness", o.roughness);
    n("bowing", o.bowing);
    n("strokeWidth", o.stroke_width);
    n("curveFitting", o.curve_fitting);
    n("curveTightness", o.curve_tightness);
    n("curveStepCount", o.curve_step_count);
    n("fillWeight", o.fill_weight);
    n("hachureAngle", o.hachure_angle);
    n("hachureGap", o.hachure_gap);
    n("simplification", o.simplification);
    n("dashOffset", o.dash_offset);
    n("dashGap", o.dash_gap);
    n("zigzagOffset", o.zigzag_offset);
    n("seed", o.seed);
    n("strokeLineDashOffset", o.stroke_line_dash_offset);
    n("fillLineDashOffset", o.fill_line_dash_offset);
    n("fixedDecimalPlaceDigits", o.fixed_decimal_place_digits);
    n("fillShapeRoughnessGain", o.fill_shape_roughness_gain);
    for (key, v) in [
        ("stroke", &o.stroke),
        ("fill", &o.fill),
        ("fillStyle", &o.fill_style),
    ] {
        if let Some(v) = v {
            m.insert(key.into(), json!(v));
        }
    }
    for (key, v) in [
        ("strokeLineDash", &o.stroke_line_dash),
        ("fillLineDash", &o.fill_line_dash),
    ] {
        if let Some(v) = v {
            m.insert(key.into(), numbers(v));
        }
    }
    for (key, v) in [
        ("disableMultiStroke", o.disable_multi_stroke),
        ("disableMultiStrokeFill", o.disable_multi_stroke_fill),
        ("preserveVertices", o.preserve_vertices),
    ] {
        if let Some(v) = v {
            m.insert(key.into(), json!(v));
        }
    }
    Value::Object(m)
}
```

- [ ] **Step 2：在 `baseline.rs` 加 generator 的分派與 `outline_linear` 測試**

`use` 區換成：

```rust
use std::fmt::Display;
use std::path::PathBuf;

use rough::math::Random;
use rough::path_data::{self, Segment};
use rough::{RoughGenerator, hachure_fill, points_on_curve, points_on_path};
use serde_json::{Value, json};
use testkit::rough_json::{drawable_value, options_from};
use testkit::{Case, check_group, num, point_value, points_from, to_value};
```

在 `segments_value` 之後加：

```rust
/// Dispatches a RoughGenerator call. The options object is always the last argument.
fn generate(case: &Case) -> Value {
    let g = RoughGenerator::new();
    let o = options_from(case.args.last().expect("options"));
    let n = |i| case.num(i);
    let drawable = match case.call.as_str() {
        "line" => g.line(n(0), n(1), n(2), n(3), &o),
        "rectangle" => g.rectangle(n(0), n(1), n(2), n(3), &o),
        "ellipse" => g.ellipse(n(0), n(1), n(2), n(3), &o),
        "circle" => g.circle(n(0), n(1), n(2), &o),
        "linearPath" => g.linear_path(&points_from(&case.args[0]), &o),
        "arc" => {
            let closed = case.args[6].as_bool().expect("closed");
            g.arc(n(0), n(1), n(2), n(3), n(4), n(5), closed, &o)
        }
        "curve" => g.curve(&points_from(&case.args[0]), &o),
        "polygon" => g.polygon(&points_from(&case.args[0]), &o),
        "path" => match g.path(case.args[0].as_str().expect("path string"), &o) {
            Ok(d) => d,
            Err(e) => return throws(e),
        },
        other => panic!("unknown generator call {other}"),
    };
    drawable_value(&drawable)
}
```

檔案最後加：

```rust
#[test]
fn outline_linear() {
    check_group(&dir(), "outline_linear", generate);
}
```

- [ ] **Step 3：確認失敗**

Run: `cargo test -p rough --test baseline outline_linear`
Expected: 編譯失敗，找不到 `RoughGenerator`。

- [ ] **Step 4：建立 `crates/rough/src/core.rs`**

```rust
//! `bin/core.d.ts`: options, ops and drawables.

pub type Point = [f64; 2];

/// rough.js `Options`: every field optional. `None` means the key is absent from the JS
/// object, so the generator default applies (`Object.assign({}, defaults, options)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub max_randomness_offset: Option<f64>,
    pub roughness: Option<f64>,
    pub bowing: Option<f64>,
    pub stroke: Option<String>,
    pub stroke_width: Option<f64>,
    pub curve_fitting: Option<f64>,
    pub curve_tightness: Option<f64>,
    pub curve_step_count: Option<f64>,
    pub fill: Option<String>,
    pub fill_style: Option<String>,
    pub fill_weight: Option<f64>,
    pub hachure_angle: Option<f64>,
    pub hachure_gap: Option<f64>,
    pub simplification: Option<f64>,
    pub dash_offset: Option<f64>,
    pub dash_gap: Option<f64>,
    pub zigzag_offset: Option<f64>,
    pub seed: Option<f64>,
    pub stroke_line_dash: Option<Vec<f64>>,
    pub stroke_line_dash_offset: Option<f64>,
    pub fill_line_dash: Option<Vec<f64>>,
    pub fill_line_dash_offset: Option<f64>,
    pub disable_multi_stroke: Option<bool>,
    pub disable_multi_stroke_fill: Option<bool>,
    pub preserve_vertices: Option<bool>,
    pub fixed_decimal_place_digits: Option<f64>,
    pub fill_shape_roughness_gain: Option<f64>,
}

/// rough.js `ResolvedOptions`, minus the `randomizer` it carries at runtime (the port keeps
/// that in the renderer's private context, so a `Drawable` stays `Send`).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedOptions {
    pub max_randomness_offset: f64,
    pub roughness: f64,
    pub bowing: f64,
    pub stroke: String,
    pub stroke_width: f64,
    pub curve_fitting: f64,
    pub curve_tightness: f64,
    pub curve_step_count: f64,
    pub fill_style: String,
    pub fill_weight: f64,
    pub hachure_angle: f64,
    pub hachure_gap: f64,
    pub dash_offset: f64,
    pub dash_gap: f64,
    pub zigzag_offset: f64,
    pub seed: f64,
    pub disable_multi_stroke: bool,
    pub disable_multi_stroke_fill: bool,
    pub preserve_vertices: bool,
    pub fill_shape_roughness_gain: f64,
    pub fill: Option<String>,
    pub simplification: Option<f64>,
    pub stroke_line_dash: Option<Vec<f64>>,
    pub stroke_line_dash_offset: Option<f64>,
    pub fill_line_dash: Option<Vec<f64>>,
    pub fill_line_dash_offset: Option<f64>,
    pub fixed_decimal_place_digits: Option<f64>,
}

impl Default for ResolvedOptions {
    /// `RoughGenerator`'s `defaultOptions` (bin/generator.js).
    fn default() -> Self {
        ResolvedOptions {
            max_randomness_offset: 2.0,
            roughness: 1.0,
            bowing: 1.0,
            stroke: "#000".to_owned(),
            stroke_width: 1.0,
            curve_tightness: 0.0,
            curve_fitting: 0.95,
            curve_step_count: 9.0,
            fill_style: "hachure".to_owned(),
            fill_weight: -1.0,
            hachure_angle: -41.0,
            hachure_gap: -1.0,
            dash_offset: -1.0,
            dash_gap: -1.0,
            zigzag_offset: -1.0,
            seed: 0.0,
            disable_multi_stroke: false,
            disable_multi_stroke_fill: false,
            preserve_vertices: false,
            fill_shape_roughness_gain: 0.8,
            fill: None,
            simplification: None,
            stroke_line_dash: None,
            stroke_line_dash_offset: None,
            fill_line_dash: None,
            fill_line_dash_offset: None,
            fixed_decimal_place_digits: None,
        }
    }
}

impl ResolvedOptions {
    /// `Object.assign({}, self, options)`.
    pub fn merge(&self, options: &Options) -> ResolvedOptions {
        let o = options.clone();
        let d = self.clone();
        ResolvedOptions {
            max_randomness_offset: o.max_randomness_offset.unwrap_or(d.max_randomness_offset),
            roughness: o.roughness.unwrap_or(d.roughness),
            bowing: o.bowing.unwrap_or(d.bowing),
            stroke: o.stroke.unwrap_or(d.stroke),
            stroke_width: o.stroke_width.unwrap_or(d.stroke_width),
            curve_fitting: o.curve_fitting.unwrap_or(d.curve_fitting),
            curve_tightness: o.curve_tightness.unwrap_or(d.curve_tightness),
            curve_step_count: o.curve_step_count.unwrap_or(d.curve_step_count),
            fill_style: o.fill_style.unwrap_or(d.fill_style),
            fill_weight: o.fill_weight.unwrap_or(d.fill_weight),
            hachure_angle: o.hachure_angle.unwrap_or(d.hachure_angle),
            hachure_gap: o.hachure_gap.unwrap_or(d.hachure_gap),
            dash_offset: o.dash_offset.unwrap_or(d.dash_offset),
            dash_gap: o.dash_gap.unwrap_or(d.dash_gap),
            zigzag_offset: o.zigzag_offset.unwrap_or(d.zigzag_offset),
            seed: o.seed.unwrap_or(d.seed),
            disable_multi_stroke: o.disable_multi_stroke.unwrap_or(d.disable_multi_stroke),
            disable_multi_stroke_fill: o
                .disable_multi_stroke_fill
                .unwrap_or(d.disable_multi_stroke_fill),
            preserve_vertices: o.preserve_vertices.unwrap_or(d.preserve_vertices),
            fill_shape_roughness_gain: o
                .fill_shape_roughness_gain
                .unwrap_or(d.fill_shape_roughness_gain),
            fill: o.fill.or(d.fill),
            simplification: o.simplification.or(d.simplification),
            stroke_line_dash: o.stroke_line_dash.or(d.stroke_line_dash),
            stroke_line_dash_offset: o.stroke_line_dash_offset.or(d.stroke_line_dash_offset),
            fill_line_dash: o.fill_line_dash.or(d.fill_line_dash),
            fill_line_dash_offset: o.fill_line_dash_offset.or(d.fill_line_dash_offset),
            fixed_decimal_place_digits: o
                .fixed_decimal_place_digits
                .or(d.fixed_decimal_place_digits),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Move([f64; 2]),
    LineTo([f64; 2]),
    BCurveTo([f64; 6]),
}

impl Op {
    /// rough.js's `op` string.
    pub fn name(&self) -> &'static str {
        match self {
            Op::Move(_) => "move",
            Op::LineTo(_) => "lineTo",
            Op::BCurveTo(_) => "bcurveTo",
        }
    }

    /// rough.js's `data` array.
    pub fn data(&self) -> &[f64] {
        match self {
            Op::Move(d) | Op::LineTo(d) => d,
            Op::BCurveTo(d) => d,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpSetType {
    Path,
    FillPath,
    FillSketch,
}

impl OpSetType {
    pub fn name(&self) -> &'static str {
        match self {
            OpSetType::Path => "path",
            OpSetType::FillPath => "fillPath",
            OpSetType::FillSketch => "fillSketch",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpSet {
    pub kind: OpSetType,
    pub ops: Vec<Op>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Line,
    Rectangle,
    Ellipse,
    Circle,
    LinearPath,
    Arc,
    Curve,
    Polygon,
    Path,
}

impl Shape {
    pub fn name(&self) -> &'static str {
        match self {
            Shape::Line => "line",
            Shape::Rectangle => "rectangle",
            Shape::Ellipse => "ellipse",
            Shape::Circle => "circle",
            Shape::LinearPath => "linearPath",
            Shape::Arc => "arc",
            Shape::Curve => "curve",
            Shape::Polygon => "polygon",
            Shape::Path => "path",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drawable {
    pub shape: Shape,
    pub options: ResolvedOptions,
    pub sets: Vec<OpSet>,
}
```

- [ ] **Step 5：建立 `crates/rough/src/renderer.rs` 的 `Ctx` 部分**

```rust
//! `bin/renderer.js`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::core::ResolvedOptions;
use crate::math::Random;

/// JS truthiness of a number: `0` and `NaN` are falsy.
pub(crate) fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

/// The options object rough.js threads through the renderer. `random(ops)` lazily hangs a
/// `Random` on it; a copy made with `Object.assign({}, o)` shares that `Random` if it
/// already exists and gets its own on first use if not. Cloning the `Rc` reproduces both,
/// so port every `Object.assign({}, o, ...)` as `o.clone()` followed by field writes.
#[derive(Clone, Debug)]
pub(crate) struct Ctx {
    pub o: ResolvedOptions,
    randomizer: Option<Rc<RefCell<Random>>>,
}

impl Ctx {
    pub fn new(o: ResolvedOptions) -> Ctx {
        Ctx {
            o,
            randomizer: None,
        }
    }

    /// `random(ops)`.
    pub fn random(&mut self) -> f64 {
        let seed = if truthy(self.o.seed) {
            self.o.seed
        } else {
            0.0
        };
        self.randomizer
            .get_or_insert_with(|| Rc::new(RefCell::new(Random::new(seed))))
            .borrow_mut()
            .next()
    }

    /// `cloneOptionsAlterSeed`.
    pub fn clone_alter_seed(&self) -> Ctx {
        let mut o = self.o.clone();
        if truthy(o.seed) {
            o.seed += 1.0;
        }
        Ctx::new(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_an_existing_randomizer_only() {
        let mut a = Ctx::new(ResolvedOptions {
            seed: 1.0,
            ..ResolvedOptions::default()
        });
        let mut before = a.clone();
        let first = a.random();
        let mut after = a.clone();
        assert_eq!(
            before.random(),
            first,
            "copy made before the first draw starts its own sequence"
        );
        assert_ne!(
            after.random(),
            first,
            "copy made after the first draw continues the shared one"
        );
    }
}
```

- [ ] **Step 6：port generator 與直線類 renderer**

`crates/rough/src/lib.rs` 改成：

```rust
//! Line-by-line port of roughjs@4.6.4 (`bin/*.js` in the npm package) and the dependency
//! versions Excalidraw's yarn.lock resolves for it. Baselines in `tests/baseline/` come from
//! `tools/baseline/rough/generate.mjs`.

pub mod core;
pub mod generator;
pub mod hachure_fill;
pub mod js;
pub mod math;
pub mod path_data;
pub mod points_on_curve;
pub mod points_on_path;
mod renderer;

pub use crate::core::{Drawable, Op, OpSet, OpSetType, Options, Point, ResolvedOptions, Shape};
pub use crate::generator::RoughGenerator;
```

來源：`tools/baseline/node_modules/roughjs/bin/generator.js`（`RoughGenerator` 的建構子、`_o`、`_d`、`line`、`rectangle`、`linearPath`、`curve`、`polygon`、`_mergedShape`），與 `bin/renderer.js` 的 `line`、`linearPath`、`polygon`、`rectangle`、`curve`、`cloneOptionsAlterSeed`、`random`、`_offset`、`_offsetOpt`、`_doubleLine`、`_line`、`_curveWithOffset`、`_curve`。generator 的 `ellipse`、`circle`、`arc`（Task 7）與 `path`（Task 8）先建立公開簽名，本體寫 `todo!()`。

port 時注意：

- 每個 generator 方法：`let mut o = Ctx::new(self.default_options.merge(options));`，最後 `Drawable { shape, options: o.o, sets }`。`with_options` 對應建構子的 `config.options`。
- `if (o.fill)` 與 `o.fill && o.fill !== NOS` 照對照表處理；`o.stroke !== NOS` 是字串比較 `"none"`。
- `curve` 的 solid 分支：`o.clone()` 後改 `disable_multi_stroke = true`、`roughness = if truthy(o.o.roughness) { o.o.roughness + o.o.fill_shape_roughness_gain } else { 0.0 }`，結果交給 `_merged_shape`；pattern 分支用 `points_on_curve::curve_to_bezier` 與 `points_on_bezier_curves(&bcurve, 10.0, Some((1.0 + o.o.roughness) / 2.0))`。
- `_line` 的 `randomHalf`、`randomFull` 是閉包，每次呼叫都抽一次亂數；`preserveVertices` 為真時條件運算子不呼叫它們，亂數不會被抽。
- `_curve` 的 `closePoint` 參數在 4.6.4 的所有呼叫點都是 `null`，port 成 `Option<Point>` 並一律傳 `None`。
- generator 的 `arc` stub 有 9 個參數，加 `#[expect(clippy::too_many_arguments, reason = "mirrors RoughGenerator.arc")]`。

- [ ] **Step 7：跑測試**

Run: `cargo test -p rough`
Expected: `clones_share_an_existing_randomizer_only` passed；`outline_linear` passed（221 個案例，其中 2 個 seed 0 只比結構）；前面各組仍然 passed。

- [ ] **Step 8：格式、lint、commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: 沒有輸出。出現 `dead_code` 警告代表建立了還沒有呼叫者的函數，照 Interfaces 移到後面的任務。

```bash
git add crates/rough crates/testkit
git commit -m "Port rough.js core types, generator and linear outlines"
```

---

### Task 7：橢圓與弧線外框

**Files:**
- Modify: `crates/rough/src/generator.rs`
- Modify: `crates/rough/src/renderer.rs`
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 6 的 `Ctx`、`_curve`、`_double_line`、`offset_opt`、`solid_fill_polygon`／`pattern_fill_polygons` stub。
- Produces:
  - 實作 `RoughGenerator::{ellipse, circle, arc}`。
  - `pub(crate) struct EllipseParams { pub increment: f64, pub rx: f64, pub ry: f64 }`、`pub(crate) struct EllipseResult { pub estimated_points: Vec<Point>, pub opset: OpSet }`
  - `pub(crate) fn generate_ellipse_params(width: f64, height: f64, o: &mut Ctx) -> EllipseParams`、`ellipse_with_params(x: f64, y: f64, o: &mut Ctx, params: &EllipseParams) -> EllipseResult`、`arc(x: f64, y: f64, width: f64, height: f64, start: f64, stop: f64, closed: bool, rough_closure: bool, o: &mut Ctx) -> OpSet`，以及私有的 `_compute_ellipse_points`、`_arc`。
  - `pub(crate) fn pattern_fill_arc(x: f64, y: f64, width: f64, height: f64, start: f64, stop: f64, o: &mut Ctx) -> OpSet` 的 `todo!()` stub（Task 10 實作）。
  - renderer 的 `ellipse`（只有 dots filler 會呼叫）留到 Task 11。

- [ ] **Step 1：加測試**

`baseline.rs` 最後加：

```rust
#[test]
fn outline_elliptic() {
    check_group(&dir(), "outline_elliptic", generate);
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p rough --test baseline outline_elliptic`
Expected: `outline_elliptic: 84 of 84 cases failed`，訊息是 `panicked: not yet implemented`。

- [ ] **Step 3：port**

來源：`bin/generator.js` 的 `ellipse`、`circle`、`arc`；`bin/renderer.js` 的 `generateEllipseParams`、`ellipseWithParams`、`arc`、`_computeEllipsePoints`、`_arc`。

port 時注意：

- generator 的 `ellipse` 先算 `generateEllipseParams` 再算 `ellipseWithParams`（亂數依序被兩者抽）。solid 分支再呼叫一次 `ellipse_with_params`（同一個 `Ctx`，亂數接續），把 set 的 kind 改成 `FillPath`；pattern 分支傳 `vec![response.estimated_points]`。
- generator 的 `arc`：solid 分支是 `let mut fill_o = o.clone(); fill_o.o.disable_multi_stroke = true;`，共用 randomizer，再以 `closed = true, rough_closure = false` 呼叫 renderer 的 `arc`；pattern 分支呼叫 `pattern_fill_arc`。
- `circle` 呼叫 `ellipse(x, y, diameter, diameter, options)` 後把 `shape` 改成 `Shape::Circle`。
- renderer 的 `arc` 與 `pattern_fill_arc` 各有 9、7 個以上參數，需要時加 `#[expect(clippy::too_many_arguments, reason = "mirrors renderer.js")]`。

- `_computeEllipsePoints` 在 `roughness === 0` 時走 `coreOnly` 分支，不抽亂數，`increment` 先除以 4。
- 非 core 分支的迴圈條件是 `angle < endAngle`，`endAngle = Math.PI * 2 + radOffset - 0.01`；`_arc` 的迴圈條件是 `angle <= stp`。兩者都用浮點累加。
- `allPoints` 與 `corePoints` 共用同一個點（JS 把同一個陣列 push 進兩邊）。Rust 以值複製，因為之後只有 `corePoints` 會被填充改寫，`allPoints` 在那之前已經轉成 ops。
- `ellipseWithParams` 第二筆外框只在 `!o.disableMultiStroke && o.roughness !== 0` 時產生；`arc` 則只看 `disableMultiStroke`。
- `arc` 的 `roughClosure` 為假時直接推兩個 `lineTo`，不抽亂數。

- [ ] **Step 4：跑測試、格式、lint、commit**

Run: `cargo test -p rough && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `outline_elliptic` passed，其他測試不受影響。

```bash
git add crates/rough
git commit -m "Port rough.js ellipse and arc outlines"
```

---

### Task 8：SVG path 外框

**Files:**
- Modify: `crates/rough/src/renderer.rs`
- Modify: `crates/rough/src/generator.rs`
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: `path_data`、`points_on_path`、Task 6 的 renderer 函數。
- Produces: 新增 `pub(crate) fn renderer::svg_path(path: &str, o: &mut Ctx) -> Result<OpSet, PathError>` 與 `_bezier_to`，實作 `RoughGenerator::path`。

- [ ] **Step 1：加測試**

```rust
#[test]
fn outline_path() {
    check_group(&dir(), "outline_path", generate);
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p rough --test baseline outline_path`
Expected: 125 個案例全部失敗（`not yet implemented`）。

- [ ] **Step 3：port**

來源：`bin/generator.js` 的 `path`，`bin/renderer.js` 的 `svgPath`、`_bezierTo`。

port 時注意：

- `if (!d)`：空字串直接回傳 `sets` 為空的 drawable；全是空白的字串是 truthy，照常解析。
- 前處理有三步，照順序：
  1. `.replace(/\n/g, ' ')`：每個 `\n` 換成空白。
  2. `.replace(/(-\s)/g, '-')`：`-` 後面緊接一個 JS 空白字元時刪掉那個空白。JS 的 `\s` 是 `\t \n \u{b} \u{c} \r`、空白、`\u{a0} \u{1680}`、`\u{2000}` 到 `\u{200a}`、`\u{2028} \u{2029} \u{202f} \u{205f} \u{3000} \u{feff}`，與 `char::is_whitespace` 不同，寫成明確的字元清單。
  3. `.replace('/(\s\s)/g', ' ')`：第一個參數是字串不是正規表示式，只取代第一次出現的字面文字 `/(\s\s)/g`。實際輸入不會出現，仍照抄成 `replacen("/(\\s\\s)/g", " ", 1)`。
- `simplified = !!(o.simplification && o.simplification < 1)`；`distance = simplified ? 4 - 4 * (o.simplification || 1) : (1 + o.roughness) / 2`。
- `pointsOnPath(d, 1, distance)` 先於 `svgPath` 執行，錯誤從這裡丟出；`distance` 以 `Some(distance)` 傳入（`points_on_path` 內部自己處理 0 的情況）。
- `hasFill = o.fill && o.fill !== 'transparent' && o.fill !== NOS`。solid 分支：`sets.length === 1` 時用改過 options 的 `svgPath` 加 `_mergedShape`，否則 `solidFillPolygon(sets, o)`（Task 9 才實作，本任務的測試不會走到）。
- `hasStroke` 且 `simplified` 時對每個 set 呼叫 `linearPath(set, false, o)`，否則推入 `svgPath` 的結果。
- `_bezierTo` 的 `ros = [o.maxRandomnessOffset || 1, (o.maxRandomnessOffset || 1) + 0.3]`；第二輪的 `move` 用 `ros[0]`、`bcurveTo` 用 `ros[i]`。

- [ ] **Step 4：跑測試、格式、lint、commit**

Run: `cargo test -p rough && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `outline_path` passed。

```bash
git add crates/rough
git commit -m "Port rough.js SVG path outlines"
```

---

### Task 9：solid 填充

**Files:**
- Modify: `crates/rough/src/renderer.rs`
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 6 到 Task 8 的 generator 分支。
- Produces: 實作 `renderer::solid_fill_polygon`。

- [ ] **Step 1：加測試並確認失敗**

```rust
#[test]
fn fill_solid() {
    check_group(&dir(), "fill_solid", generate);
}
```

Run: `cargo test -p rough --test baseline fill_solid`
Expected: 失敗。rectangle、polygon、path/subpaths、path/whitespace 等案例 panic 在 `todo!()`；curve 的 solid 分支在 Task 6、ellipse 與 arc 的在 Task 7 已經寫好，若它們也失敗，代表那些分支有錯，一併修正。

- [ ] **Step 2：port**

來源：`bin/renderer.js` 的 `solidFillPolygon`。`const offset = o.maxRandomnessOffset || 0`；只處理長度大於 2 的多邊形；每個座標都先加上 `_offsetOpt(offset, o)`，x 先抽、y 後抽。

- [ ] **Step 3：跑測試、格式、lint、commit**

Run: `cargo test -p rough && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `fill_solid` passed（50 個案例）。

```bash
git add crates/rough
git commit -m "Port rough.js solid fills"
```

---

### Task 10：hachure、cross-hatch、zigzag 填充

**Files:**
- Create: `crates/rough/src/fillers/mod.rs`
- Create: `crates/rough/src/fillers/scan_line_hachure.rs`
- Create: `crates/rough/src/fillers/hachure.rs`
- Create: `crates/rough/src/fillers/hatch.rs`
- Create: `crates/rough/src/fillers/zigzag.rs`
- Create: `crates/rough/src/geometry.rs`
- Modify: `crates/rough/src/renderer.rs`
- Modify: `crates/rough/src/lib.rs`（加 `mod fillers;`、`mod geometry;`）
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: `hachure_fill::hachure_lines`、`renderer::Ctx`、`math::math_random`。
- Produces:
  - `pub(crate) fn fillers::fill_polygons(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> OpSet`：依 `o.o.fill_style` 分派，`"zigzag"`、`"cross-hatch"`、`"dots"`、`"dashed"`、`"zigzag-line"`，其他一律 hachure；dots、dashed、zigzag-line 在 Task 11 之前是 `todo!()`。
  - `pub(crate) fn fillers::scan_line_hachure::polygon_hachure_lines(polygon_list: &mut [Vec<Point>], o: &mut Ctx) -> Vec<[Point; 2]>`
  - `pub(crate) fn geometry::line_length(line: &[Point; 2]) -> f64`
  - 實作 `renderer::pattern_fill_polygons`、`pattern_fill_arc`；新增 `pub(crate) fn renderer::double_line_fill_ops(x1: f64, y1: f64, x2: f64, y2: f64, o: &mut Ctx) -> Vec<Op>`（JS `helper.doubleLineOps`）與 `Ctx::existing_random`。

- [ ] **Step 1：加測試並確認失敗**

```rust
#[test]
fn fill_hachure() {
    check_group(&dir(), "fill_hachure", generate);
}

#[test]
fn fill_cross_hatch() {
    check_group(&dir(), "fill_cross_hatch", generate);
}

#[test]
fn fill_zigzag() {
    check_group(&dir(), "fill_zigzag", generate);
}
```

Run: `cargo test -p rough --test baseline fill_`
Expected: 三組失敗；`fill_solid` 仍 passed。

- [ ] **Step 2：port**

來源：`bin/fillers/filler.js`、`scan-line-hachure.js`、`hachure-filler.js`、`hatch-filler.js`、`zigzag-filler.js`、`bin/geometry.js`，以及 `bin/renderer.js` 的 `patternFillPolygons`、`patternFillArc`、`doubleLineFillOps`。

先在 `renderer.rs` 的 `impl Ctx` 加上：

```rust
    /// `ops.randomizer?.next()`: draws only if a randomizer already exists
    /// (bin/fillers/scan-line-hachure.js).
    pub fn existing_random(&self) -> Option<f64> {
        self.randomizer.as_ref().map(|r| r.borrow_mut().next())
    }
```

port 時注意：

- `getFiller` 的快取物件不需要：filler 沒有狀態，直接依 `fill_style` 呼叫對應函數。
- `polygonHachureLines`：`if (o.roughness >= 1)` 時算 `(o.randomizer?.next() || Math.random()) > 0.7`。對應 `o.existing_random().filter(|v| truthy(*v)).unwrap_or_else(math_random) > 0.7`：已有 randomizer 才抽、不會建立新的，抽到 0 時退回 `Math.random`。
- `HatchFiller`：第一次 `_fillPolygons(polygonList, o)`，再用 `hachureAngle + 90` 的複製 options 對同一份 `polygonList` 做第二次；第二次看到的是第一次旋轉回來後帶浮點漂移的點。
- `ZigZagFiller`：`gap` 先處理負值與下限 0.1，用改過 `hachureGap` 的複製 options 算線，但 `renderLines` 傳原本的 `o`。`if (lineLength([p1, p2]))` 是數字真假值。
- `patternFillArc` 的迴圈 `for (angle = strt; angle <= stp; angle = angle + increment)` 用浮點累加，之後再推入終點與圓心。

- [ ] **Step 3：跑測試、格式、lint、commit**

Run: `cargo test -p rough && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `fill_hachure`（含 7 個 edge 案例）、`fill_cross_hatch`、`fill_zigzag` passed。

```bash
git add crates/rough
git commit -m "Port rough.js hachure, cross-hatch and zigzag fills"
```

---

### Task 11：dots、dashed、zigzag-line 填充與收尾

**Files:**
- Create: `crates/rough/src/fillers/dot.rs`
- Create: `crates/rough/src/fillers/dashed.rs`
- Create: `crates/rough/src/fillers/zigzag_line.rs`
- Modify: `crates/rough/src/fillers/mod.rs`
- Modify: `crates/rough/src/renderer.rs`
- Modify: `crates/rough/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 10 的 filler 架構與 renderer helper。
- Produces: 新增 `pub(crate) fn renderer::ellipse(x: f64, y: f64, width: f64, height: f64, o: &mut Ctx) -> OpSet`（JS `helper.ellipse`）；`fillers::fill_polygons` 支援全部七種樣式，crate 內沒有 `todo!()`。

- [ ] **Step 1：加測試並確認失敗**

```rust
#[test]
fn fill_dots() {
    check_group(&dir(), "fill_dots", generate);
}

#[test]
fn fill_dashed() {
    check_group(&dir(), "fill_dashed", generate);
}

#[test]
fn fill_zigzag_line() {
    check_group(&dir(), "fill_zigzag_line", generate);
}
```

Run: `cargo test -p rough --test baseline fill_`
Expected: 新的三組失敗，其餘 passed。

- [ ] **Step 2：port**

來源：`bin/fillers/dot-filler.js`、`dashed-filler.js`、`zigzag-line-filler.js`，以及 `bin/renderer.js` 的 `ellipse`（`generateEllipseParams` 後接 `ellipseWithParams(...).opset`）。

port 時注意：

- `DotFiller`：先複製 options 並把 `hachureAngle` 設成 0；點的位置 `(x - ro) + Math.random() * 2 * ro` 用 `math_random()`。這組基準只比結構，數字不可重現是預期行為。每個點呼叫 renderer 的 `ellipse(cx, cy, fweight, fweight, o)` 並接上它的 ops。
- `DashedFiller` 與 `ZigZagLineFiller` 都在 `p1[0] > p2[0]` 時交換端點，角度用 `Math.atan(dy / dx)`（不是 `atan2`）。
- `DashedFiller` 的 `offset`、`gap` 在 `dashOffset`/`dashGap` 為負時退回 `hachureGap`，再退回 `strokeWidth * 4`。
- `ZigZagLineFiller`：`count = Math.round(length / (2 * zo))` 用 `js::math_round`；複製的 options 把 `hachureGap` 設成 `gap + zo`。

- [ ] **Step 3：整體驗證**

Run:

```bash
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
grep -rn 'todo!\|unimplemented!' crates/rough/src && echo "stubs left" || echo "no stubs"
cargo tree -p rough -e normal --depth 1
```

Expected:
- `cargo test`：`baseline` 15 passed；`rough` 單元測試 5 passed；`testkit` 6 passed。
- `no stubs`。
- `cargo tree` 只印出 `rough v0.0.0 (...)` 一行，沒有子節點。

- [ ] **Step 4：Commit**

```bash
git add crates/rough
git commit -m "Port rough.js dots, dashed and zigzag-line fills"
```

M1 完成條件（roadmap）：node 產生的基準全部在 1e-9 誤差內通過。
