# napkin M2：`scene` 檔案層與形狀規則 Implementation Plan

> Historical record, frozen 2026-09-14. Source code is authoritative; where this
> document and the code disagree, the code wins.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立 `scene` crate：無損讀寫 `.excalidraw`、依 Excalidraw 的預設值建立新元件、fractional index、深色模式色彩轉換，以及把每個元件轉成 rough ops 與 freedraw 外框的形狀規則；在 excalidraw.com 畫的語料 round-trip 不變，Excalidraw 原始碼產生的 1,219 個基準案例全部在 1e-9 誤差內通過。

**Architecture:** 元件以型別化的 serde 結構表示（共用欄位 `ElementBase` 加上各型別欄位，未知欄位進 `extra`），但只有「序列化回去與原始 JSON 語意相同」時才採用，否則整個元件保留為原始 JSON（`Element::Raw`）。頂層物件與 `appState` 保留成 JSON map，只提供 napkin 需要的存取函數。形狀規則逐函數 port 自 Excalidraw commit `afa3a65` 的 `shape.ts` 及其依賴，基準由 `tools/baseline/scene/generate.mjs` 直接打包該 commit 的原始碼執行產生。

**Tech Stack:** Rust 1.98.1（edition 2024）、serde 1、serde_json 1（`float_roundtrip`、`preserve_order`）、getrandom 0.3、regex 1（Task 7）、M1 的 `rough` 與 `testkit`；node 26.7.0、esbuild 0.28.2、git。

**Spec:** `docs/decisions/specs/2026-09-13-napkin-design.md`（§3、§4.2 `scene`、§5 資料模型、§6.6 深色模式、§9.2 測試）
**Roadmap:** `docs/decisions/plans/2026-09-13-napkin-roadmap.md`
**前置：** M1 已完成（`rough`、`testkit`、`tools/baseline/lib/harness.mjs` 都在 repo 裡）。

## Global Constraints

- 程式碼、註解、commit message 用英文；`docs/decisions/` 底下的文件用中文。
- commit message 不加任何 attribution trailer（不要 `Co-Authored-By`，也不要任何 generated-by 字樣）。
- Excalidraw 基準 commit：`afa3a653fc5d2b742adcbd5a6063187b056d2419`。
- 版本：`roughjs 4.6.4`、`perfect-freehand 1.2.0`（TypeScript 原始碼在 GitHub tag `v1.2.0`，commit `fa0b754d49bb60813c24cbd7fdcd5cda40f560f3`，`packages/perfect-freehand/src/`）、`tinycolor2 1.6.0`、`points-on-curve 1.0.1`（`simplify` 與 0.2.0 相同）、`@excalidraw/laser-pointer 1.3.1` 與 `@excalidraw/fractional-indexing 3.3.0`（兩者都在 Excalidraw repo 的 `packages/` 裡）、`nanoid 3.3.3` 的字母表。
- `scene` 不能依賴 egui 或任何繪圖相關 crate（spec §4.2）。允許的依賴：`rough`、`serde`、`serde_json`、`getrandom`、`regex`。
- 數字比對的絕對誤差 `1e-9`（`testkit::TOLERANCE`），不准放寬。
- 基準 JSON 只能由產生器寫出，不准手改。語料檔只能是 excalidraw.com 存出的原檔，不准手改。
- port 的來源：Task 2 跑過產生器後，Excalidraw 原始碼在 `tools/baseline/.cache/excalidraw-afa3a653fc5d2b742adcbd5a6063187b056d2419/packages/`（此目錄不進 git；新的工作目錄執行 `cd tools/baseline && npm ci && npm run scene` 會重新抓取）。
- 每個任務結束前都要通過：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、該任務新增的測試。

## 寫計畫時已經完成的驗證

- 產生器（Task 2 的完整程式碼）實際跑過：9 組共 1,219 個案例、3.5 MiB，連跑兩次輸出逐 byte 相同，M1 的 rough 基準在加入 M2 的 npm 依賴後重新產生也沒有變化。基準裡所有 rectangle、diamond、ellipse、line、arrow、freedraw、text 元件都能以型別化結構載入。
- Task 3 與 Task 5 的完整程式碼（`json.rs`、`element.rs`、`file.rs`、`env.rs`、`new_element.rs`）編譯過、clippy 沒有輸出，13 個單元測試通過；拿掉 `exact` 的寫回比較時 `null_in_absent_only_field_falls_back_to_raw` 會失敗。`new_element` 基準 8 個案例通過，把 `versionNonce` 預設改成 1 時 8 個全部失敗。
- Task 4 的語料測試對一個合成檔案跑過：round-trip 通過，涵蓋檢查正確列出缺少的項目。
- `tests/baseline.rs` 全部測試函數都在其餘模組為 `todo!()` 的 stub 上編譯過。
- Task 12 的 `truncate_path_number` 對 14 個數值與 node 的結果一致。
- 還沒寫過的：fractional-indexing、tinycolor、形狀規則、perfect-freehand、laser-pointer 的 port；在 excalidraw.com 上實際畫的語料。

## 與 spec 不同的地方

原始碼與 spec 不一致時以原始碼為準，以下幾點依原始碼修正：

1. **freedraw 有兩種外框演算法。** spec §4.2、§9.2 只提到 perfect-freehand。Excalidraw 在 `afa3a65` 依 `strokeOptions.variability` 分流：`"constant"` 用 `@excalidraw/laser-pointer`，其他值用 perfect-freehand；而 `appState.currentItemStrokeVariability` 的預設是 `"constant"`，所以在 excalidraw.com 用滑鼠或觸控板畫的 freedraw 全部走 laser-pointer。M2 兩者都 port（Task 11）。
2. **形狀規則的基準直接執行 Excalidraw 原始碼。** spec §9.2 寫的是把 `generateRoughOptions` 等函數複製進基準 script。實際上 `getArrowheadPoints` 一路依賴 `bounds.ts`、`math`、`common` 的許多模組，複製出來的版本本身就可能出錯。產生器改成 git fetch 該 commit、用 esbuild 按 repo 的 `tsconfig.json` 路徑別名打包 `@excalidraw/element`，只把四個形狀程式碼用不到的 import（`@braintree/sanitize-url`、`es6-promise-pool`、`lodash.throttle`、`nanoid`）換成呼叫即丟例外的替身。
3. **頂層物件與 `appState` 不做成 serde 結構。** spec §5.2 要求它們帶 `#[serde(flatten)] extra`。這兩者都保留成 JSON map，`SceneFile` 只提供 napkin 用到的存取函數（`view_background_color`、`napkin_view`、`set_napkin_view`）。保存未知欄位的效果相同，也不會因為某個 `appState` 欄位型別出乎預期而整個檔案讀不進來。

## 寫計畫時做的決定

1. **型別化只在能無損寫回時採用。** `Element::from_value` 先嘗試解析成對應型別的結構，再序列化回 JSON 與原始值做語意比較；不相同就整個元件存成 `Element::Raw(Value)`。結構設計的疏漏（例如某個欄位出現沒預期到的 `null`）只會讓那個元件在 M3 畫成虛線框，不會悄悄改寫檔案。語料測試另外要求 drawn 型別（rectangle、diamond、ellipse、line、arrow、text、freedraw）一個都不能落到 `Raw`，所以疏漏會在測試裡現形。
2. **缺席、`null`、有值三種狀態分開保存。** Excalidraw 陸續加過 `index`、`created` 等欄位，舊檔沒有這些 key，新檔寫成 `null`。可能缺席又可能為 `null` 的欄位用 `json::Slot<T>`；只可能缺席的欄位用 `Option<T>` 加 `skip_serializing_if`。Excalidraw 的解構預設值（`const { endArrowhead = "arrow" } = element`）只在缺席時生效、`null` 時不生效，`Slot` 剛好表達得出來。
3. **字串型的列舉欄位保持 `String`。** `fillStyle`、`strokeStyle`、箭頭頭部等在 Excalidraw 裡是字串比較，遇到沒見過的值走 `default` 分支。改成 Rust enum 會讓這種檔案解析失敗。
4. **寫檔時整數型的浮點數寫成整數。** serde_json 會把 `2.0` 寫成 `2.0`，`JSON.stringify` 寫成 `2`。`json::normalize_numbers` 在輸出前把絕對值不超過 2^53 的整數值改寫成整數，讓存出的檔案與 excalidraw.com 的寫法接近，值不變。
5. **不執行 `restore.ts`。** Excalidraw 載入檔案時會遷移舊格式（舊箭頭名稱 `dot`、`crowfoot_*`，`strokeSharpness`，舊綁定格式）。napkin 照原始資料讀寫與繪製：這類舊檔在 napkin 裡的外觀可能與 excalidraw.com 不同，資料不變。
6. **新元件的預設值照 `newElement.ts` 的建構函數。** 編輯器層級的目前設定（例如 excalidraw.com 用滑鼠畫 freedraw 時傳入的 `variability: "constant"`、矩形的 `currentItemRoundness`）屬於 M4 的工具，由呼叫端放進 `ElementProps` 或參數。`newFreeDrawElement` 本身在沒有傳 `strokeOptions` 時的預設是 `"variable"`。
7. **箭頭頭部的 `invariant` 不 panic。** `getArrowheadPoints` 在 op 不是 `bcurveTo` 時丟例外，Excalidraw 的繪製會因此中斷。napkin 在同樣情況下不畫那個箭頭頭部，其餘照畫。正常資料走不到這裡。

## JS → Rust 對照規則

M1 計畫的對照規則在 port Excalidraw 程式碼時同樣適用（`Math.round` 用 `rough::js::math_round`、呼叫順序照 JS、浮點累加迴圈不改寫、`throw` 對應回傳錯誤並印出相同訊息）。M2 另外常遇到：

| JS | Rust | 為什麼 |
|---|---|---|
| 字串 `a < b`、`a >= b` | `a.encode_utf16().cmp(b.encode_utf16())` | JS 比 UTF-16 code unit；`str` 比 UTF-8 位元組，BMP 以外的字元與 `U+E000` 以上的字元相比時順序相反 |
| 模板字串裡的數字 `` `${n}` `` | `format!("{n}")` | Rust 印出能還原成同一個 f64 的最短數字、不用指數；rough 的 path parser 讀回同一個值，這正是 path 字串需要的。元件幾何一定是有限數，serde_json 拒絕 `1e400` 這類輸入 |
| `const { x = d } = obj` | `Slot::Missing` 時用 `d`，`Slot::Null` 時是 `null` | 解構預設值只在 `undefined` 時生效 |
| `a ?? b`、`a?.b` | `slot.value().unwrap_or(b)`、`slot.value().and_then(...)` | 缺席與 `null` 同樣處理 |
| `!!element.roundness` | `matches!(base.roundness, Slot::Value(_))` | 物件一律為真 |
| `x \|\| y`（字串） | 空字串視為假 | |
| `{ ...options, k: v }`、`delete o.k` | `rough::Options` 的 clone 後改欄位、設成 `None` | |

---

### Task 1：在 excalidraw.com 上畫語料（人工）

**Files:**
- Create: `crates/scene/tests/corpus/shapes.excalidraw`
- Create: `crates/scene/tests/corpus/bindings.excalidraw`
- Create: `crates/scene/tests/corpus/image.excalidraw`

**Interfaces:**
- Consumes: 無
- Produces: Task 4 的 round-trip 與涵蓋測試讀取的語料。

這個任務要由使用者完成，控制者請使用者照步驟操作後把檔案交回。它不阻擋 Task 2、3、5 到 12，只有 Task 4 需要等它完成；語料還沒好時，把 Task 4 排到最後。

- [ ] **Step 1：畫 `shapes.excalidraw`**

在 https://excalidraw.com 開一個空白畫布：

1. 畫矩形、菱形、橢圓各一個，背景色選非透明的顏色，四種填充樣式 hachure、cross-hatch、solid、zigzag 都要出現至少一次（可以多畫一個）。
2. 畫一條線，線條樣式選 dashed；再畫一個矩形，線條樣式選 dotted。
3. 把其中一個矩形用旋轉控制點轉一個角度。
4. 選兩個元件按 `Ctrl+G` 群組起來。
5. 用滑鼠或觸控板畫一筆手繪（預設是 constant）。再畫一筆，選取它後在左側屬性面板點「筆壓」按鈕切成 variable。面板上找不到這個按鈕時停下來回報。
6. 用工具列的便利貼工具放一張 sticky note。找不到這個工具時停下來回報。
7. 刪除任一個元件，不要重新整理頁面，直接從選單「Save to…／儲存到…」存成 `shapes.excalidraw`。

- [ ] **Step 2：畫 `bindings.excalidraw`**

開新的空白畫布：

1. 畫兩個矩形，畫一支箭頭，把兩端分別拖到兩個矩形上，看到綁定提示後放開。
2. 在其中一個矩形上雙擊，用 fcitx5 輸入「你好世界」（容器文字）。
3. 在箭頭上雙擊，輸入任意文字（箭頭標籤）。
4. 把箭頭類型切成 elbow，再畫一支 elbow arrow。
5. 用 frame 工具（`F`）畫一個 frame，拖一個元件進去。
6. 存成 `bindings.excalidraw`。

- [ ] **Step 3：做 `image.excalidraw`**

開新的空白畫布，貼上或拖入一張小的 PNG 圖片，存成 `image.excalidraw`。

- [ ] **Step 4：放進 repo 並快速檢查**

把三個檔案原封不動複製到 `crates/scene/tests/corpus/`。

Run:

```bash
cd crates/scene/tests/corpus
grep -l '"isDeleted": true' *.excalidraw
grep -l '"variability": "variable"' *.excalidraw
grep -l '"type": "stickynote"' *.excalidraw
grep -l '"elbowed": true' *.excalidraw
grep -l '"dataURL"' *.excalidraw
```

Expected: 每個指令至少印出一個檔名。任何一個沒有輸出就重畫對應的檔案；刪除的元件沒有被存進去時停下來回報，不要手改檔案補上。

- [ ] **Step 5：Commit**

```bash
git add crates/scene/tests/corpus
git commit -m "Add excalidraw.com round-trip corpus"
```

---

### Task 2：scene 基準產生器

**Files:**
- Modify: `tools/baseline/package.json`
- Modify: `tools/baseline/package-lock.json`（由 `npm install` 更新）
- Modify: `tools/baseline/.gitignore`
- Create: `tools/baseline/lib/excalidraw.mjs`
- Create: `tools/baseline/scene/cases.mjs`
- Create: `tools/baseline/scene/generate.mjs`
- Create: `crates/scene/tests/baseline/*.json`（9 個檔案）

**Interfaces:**
- Consumes: M1 的 `tools/baseline/lib/harness.mjs`（`REPO_ROOT`、`assertVersions`、`bundleAndImport`、`runCase`、`writeGroup`）。
- Produces: 9 組基準，格式同 M1。各組的 `call` 與 `args`：
  - `shapes_generic`、`shapes_linear`、`shapes_freedraw`、`shapes_other`：`generateElementShape`，`args = [元件 JSON, { theme, canvasBackgroundColor }]`；`expected` 是 rectangle／diamond／ellipse 的單一 drawable、line／arrow 的 drawable 陣列、freedraw 的 `[填充 drawable?, { svgPath: [ops] }]`（op 為 `move`、`quad`、`line`、`close`），其他型別為 `null`。
  - `rough_options`：`generateRoughOptions`，`args = [元件 JSON, continuousPath, isDarkMode]`。
  - `freedraw_outline`：`getFreedrawOutlinePoints`，`args = [元件 JSON]`。
  - `colors`：`applyDarkModeFilter` 或 `isTransparent`，`args = [顏色字串]`。
  - `fractional_index`：`generateKeyBetween [a, b]`、`generateNKeysBetween [a, b, n]`、`syncMovedIndices [indices, 被移動的位置]`、`syncInvalidIndices [indices]`；後兩者的 `expected` 是 `[{ id, index, version }]`，元件 id 為 `e0`、`e1`…。
  - `new_element`：`newElement`、`newLinearElement`、`newArrowElement`、`newFreeDrawElement`，`args = [opts]`，opts 固定了 `id` 與 `seed`，產生時 `Date.now()` 固定為 1。

- [ ] **Step 1：更新 `tools/baseline/package.json`**

```json
{
  "name": "napkin-baseline",
  "private": true,
  "type": "module",
  "scripts": {
    "rough": "node rough/generate.mjs",
    "scene": "node scene/generate.mjs"
  },
  "devDependencies": {
    "esbuild": "0.28.2",
    "perfect-freehand": "1.2.0",
    "points-on-curve": "1.0.1",
    "roughjs": "4.6.4",
    "tinycolor2": "1.6.0"
  }
}
```

Run: `cd tools/baseline && npm install --no-audit --no-fund`
Expected: lock 檔更新；`node_modules/points-on-curve` 是 1.0.1，roughjs 用的 0.2.0 移到 `node_modules/roughjs/node_modules/` 之下（M1 的 `resolveFrom("roughjs", ...)` 依然找得到它）。

- [ ] **Step 2：更新 `tools/baseline/.gitignore`**

```gitignore
node_modules/
.build/
.cache/
```

- [ ] **Step 3：建立 `tools/baseline/lib/excalidraw.mjs`**

```js
// Fetches Excalidraw at the commit spec §3 pins and bundles its own element/common sources,
// so the baseline runs exactly the code excalidraw.com runs instead of hand-copied excerpts.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";

import { BASELINE_DIR, bundleAndImport } from "./harness.mjs";

export const EXCALIDRAW_COMMIT = "afa3a653fc5d2b742adcbd5a6063187b056d2419";

function git(cwd, ...args) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] }).trim();
}

/** A shallow checkout of EXCALIDRAW_COMMIT under .cache/, reused when already present. */
export function excalidrawCheckout() {
  const dir = join(BASELINE_DIR, ".cache", `excalidraw-${EXCALIDRAW_COMMIT}`);
  if (!existsSync(join(dir, ".git"))) {
    mkdirSync(dir, { recursive: true });
    git(dir, "init", "--quiet");
    git(dir, "remote", "add", "origin", "https://github.com/excalidraw/excalidraw.git");
    git(dir, "fetch", "--quiet", "--depth", "1", "origin", EXCALIDRAW_COMMIT);
    git(dir, "checkout", "--quiet", "FETCH_HEAD");
  }
  const head = git(dir, "rev-parse", "HEAD");
  if (head !== EXCALIDRAW_COMMIT) {
    throw new Error(`${dir} is at ${head}, expected ${EXCALIDRAW_COMMIT}; delete it and rerun`);
  }
  return dir;
}

// Imported by Excalidraw modules that the shape code never calls; bundling them would
// need their npm packages installed for nothing. The default export stays callable because
// Scene.ts wraps a function with lodash.throttle at module load; whatever it returns throws
// if the baseline ever reaches it.
const STUBBED = /^(@braintree\/sanitize-url|es6-promise-pool|lodash\.throttle|nanoid)$/;

const stubPlugin = {
  name: "stub-unused",
  setup(build) {
    build.onResolve({ filter: STUBBED }, (args) => ({ path: args.path, namespace: "stub" }));
    build.onLoad({ filter: /.*/, namespace: "stub" }, () => ({
      contents: [
        "const unused = () => { throw new Error('stubbed module called'); };",
        "export default () => unused; export const sanitizeUrl = unused; export const nanoid = unused;",
      ].join("\n"),
      loader: "js",
    }));
  },
};

/** Bundles `entrySource` against the checkout's tsconfig path aliases and imports it. */
export function bundleExcalidraw(name, entrySource) {
  const checkout = excalidrawCheckout();
  return bundleAndImport(name, entrySource, {
    tsconfig: join(checkout, "tsconfig.json"),
    nodePaths: [join(BASELINE_DIR, "node_modules")],
    plugins: [stubPlugin],
    define: { "import.meta.env": "{}" },
    loader: { ".woff2": "empty", ".png": "empty", ".svg": "empty", ".scss": "empty", ".css": "empty" },
  });
}
```

- [ ] **Step 4：建立 `tools/baseline/scene/cases.mjs`**

```js
// Inputs for the scene baselines. Elements are complete objects shaped like Excalidraw's
// `newElement` output at the pinned commit, so the shape code sees what a saved file holds.

let nextId = 0;

/** `_newElementBase` defaults (packages/element/src/newElement.ts) plus overrides. */
function element(type, overrides = {}) {
  nextId += 1;
  return {
    id: `el${nextId}`,
    type,
    x: 0,
    y: 0,
    width: 120,
    height: 80,
    angle: 0,
    strokeColor: "#1e1e1e",
    backgroundColor: "transparent",
    fillStyle: "solid",
    strokeWidth: 2,
    strokeStyle: "solid",
    roughness: 1,
    opacity: 100,
    groupIds: [],
    frameId: null,
    index: "a0",
    roundness: null,
    seed: 1968410350,
    version: 1,
    versionNonce: 0,
    isDeleted: false,
    boundElements: null,
    updated: 1,
    created: 1,
    link: null,
    locked: false,
    ...overrides,
  };
}

function linear(type, points, overrides = {}) {
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => p[1]);
  return element(type, {
    width: points.length ? Math.max(...xs) - Math.min(...xs) : 0,
    height: points.length ? Math.max(...ys) - Math.min(...ys) : 0,
    points,
    startBinding: null,
    endBinding: null,
    startArrowhead: null,
    endArrowhead: null,
    ...(type === "line" ? { polygon: false } : { elbowed: false }),
    ...overrides,
  });
}

function freedraw(points, overrides = {}) {
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => p[1]);
  return element("freedraw", {
    width: points.length ? Math.max(...xs) - Math.min(...xs) : 0,
    height: points.length ? Math.max(...ys) - Math.min(...ys) : 0,
    points,
    pressures: [],
    simulatePressure: true,
    strokeOptions: { variability: "constant", streamline: 0.5 },
    ...overrides,
  });
}

const SEEDS = [1968410350, 7];
const SIZES = { normal: [120, 80], small: [30, 12], tiny: [8, 6], odd: [101, 57] };
const ROUNDNESS = {
  sharp: null,
  legacy: { type: 1 },
  proportional: { type: 2 },
  adaptive: { type: 3 },
  adaptiveValue: { type: 3, value: 12 },
};

/** One case per variant of each dimension, so every rule branch runs without a full cross product. */
function genericVariants(type) {
  const cases = [];
  const add = (label, overrides) => cases.push({ label: `${type}/${label}`, element: element(type, overrides) });
  for (const seed of SEEDS) {
    for (const roughness of [0, 1, 2]) add(`seed${seed}/r${roughness}`, { seed, roughness });
  }
  for (const [label, [width, height]] of Object.entries(SIZES)) add(`size/${label}`, { width, height });
  for (const [label, roundness] of Object.entries(ROUNDNESS)) {
    add(`roundness/${label}`, { roundness });
    add(`roundness/${label}/small`, { roundness, width: 18, height: 16 });
  }
  for (const fillStyle of ["hachure", "cross-hatch", "solid", "zigzag"]) {
    add(`fill/${fillStyle}`, { fillStyle, backgroundColor: "#ffc9c9" });
    add(`fill/${fillStyle}/r2/rounded`, { fillStyle, backgroundColor: "#a5d8ff", roughness: 2, roundness: ROUNDNESS.adaptive });
  }
  add("fill/transparentRgba", { fillStyle: "hachure", backgroundColor: "rgba(255, 0, 0, 0)" });
  for (const strokeStyle of ["dashed", "dotted"]) {
    add(`stroke/${strokeStyle}`, { strokeStyle });
    add(`stroke/${strokeStyle}/w4`, { strokeStyle, strokeWidth: 4, backgroundColor: "#b2f2bb", fillStyle: "hachure" });
  }
  add("strokeWidth1", { strokeWidth: 1, backgroundColor: "#ffec99", fillStyle: "hachure" });
  add("zeroSize", { width: 0, height: 0, roundness: ROUNDNESS.adaptive });
  return cases;
}

const ARROWHEADS = [
  "arrow", "bar", "circle", "circle_outline", "triangle", "triangle_outline", "diamond", "diamond_outline",
  "cardinality_one", "cardinality_many", "cardinality_one_or_many", "cardinality_exactly_one",
  "cardinality_zero_or_one", "cardinality_zero_or_many", "dot",
];

const TWO = [[0, 0], [160, 60]];
const ZIGZAG = [[0, 0], [60, 80], [140, 20], [200, 90]];
const SHORT_LAST = [[0, 0], [150, 0], [156, 4]];
const ELBOW = [[0, 0], [80, 0], [80, 120], [200, 120]];
const LOOP = [[0, 0], [100, 10], [60, 90], [3, 4]];

function linearVariants() {
  const cases = [];
  const add = (label, el) => cases.push({ label, element: el });
  for (const seed of SEEDS) {
    for (const roughness of [0, 1, 2]) {
      add(`line/sharp/seed${seed}/r${roughness}`, linear("line", ZIGZAG, { seed, roughness }));
      add(`line/round/seed${seed}/r${roughness}`, linear("line", ZIGZAG, { seed, roughness, roundness: ROUNDNESS.proportional }));
    }
  }
  add("line/two", linear("line", TWO));
  add("line/empty", linear("line", []));
  add("line/single", linear("line", [[0, 0]]));
  add("line/short", linear("line", [[0, 0], [20, 5]]));
  add("line/loop/transparent", linear("line", LOOP, { backgroundColor: "transparent" }));
  for (const fillStyle of ["hachure", "cross-hatch", "solid", "zigzag"]) {
    add(`line/loop/${fillStyle}`, linear("line", LOOP, { backgroundColor: "#ffc9c9", fillStyle }));
    add(`line/loop/${fillStyle}/round`, linear("line", LOOP, { backgroundColor: "#ffc9c9", fillStyle, roundness: ROUNDNESS.proportional }));
  }
  add("line/polygon", linear("line", [...LOOP.slice(0, 3), [0, 0]], { polygon: true, backgroundColor: "#ffc9c9", fillStyle: "solid" }));
  add("line/dotted", linear("line", ZIGZAG, { strokeStyle: "dotted" }));

  for (const head of ARROWHEADS) {
    add(`arrow/end/${head}`, linear("arrow", TWO, { endArrowhead: head }));
    add(`arrow/start/${head}/round`, linear("arrow", ZIGZAG, { startArrowhead: head, roundness: ROUNDNESS.proportional }));
    add(`arrow/both/${head}/dotted`, linear("arrow", SHORT_LAST, { startArrowhead: head, endArrowhead: head, strokeStyle: "dotted", strokeWidth: 1 }));
  }
  for (const roughness of [0, 2]) {
    add(`arrow/r${roughness}`, linear("arrow", ZIGZAG, { endArrowhead: "triangle", roughness }));
  }
  add("arrow/dashed", linear("arrow", ZIGZAG, { endArrowhead: "arrow", strokeStyle: "dashed" }));
  add("arrow/endArrowheadMissing", (() => {
    const el = linear("arrow", TWO);
    delete el.endArrowhead;
    return el;
  })());
  add("arrow/single", linear("arrow", [[0, 0]], { endArrowhead: "arrow" }));
  add("arrow/empty", linear("arrow", [], { endArrowhead: "arrow" }));
  add("arrow/elbow", linear("arrow", ELBOW, { elbowed: true, endArrowhead: "arrow", fixedSegments: null, startIsSpecial: null, endIsSpecial: null }));
  add("arrow/elbow/short", linear("arrow", [[0, 0], [10, 0], [10, 6], [40, 6]], { elbowed: true, startArrowhead: "circle_outline", endArrowhead: "triangle" }));
  add("arrow/elbow/extreme", linear("arrow", [[0, 0], [2e6, 0]], { elbowed: true, endArrowhead: "arrow" }));
  return cases;
}

function spiral(n, step = 6) {
  return Array.from({ length: n }, (_, i) => {
    const t = i / 4;
    return [Math.round((10 + 3 * t) * Math.cos(t) * 100) / 100 + step, Math.round((10 + 3 * t) * Math.sin(t) * 100) / 100];
  });
}

function freedrawVariants() {
  const cases = [];
  const add = (label, el) => cases.push({ label, element: el });
  const STROKES = { empty: [], one: [[0, 0]], two: [[0, 0], [12, 7]], three: [[0, 0], [12, 7], [30, 2]], spiral: spiral(40) };
  for (const variability of ["constant", "variable"]) {
    for (const [label, points] of Object.entries(STROKES)) {
      add(`freedraw/${variability}/${label}`, freedraw(points, { strokeOptions: { variability, streamline: 0.5 } }));
    }
    add(`freedraw/${variability}/duplicates`, freedraw([[0, 0], [0, 0], [5, 5], [5, 5], [5, 5], [20, 0]], { strokeOptions: { variability, streamline: 0.5 } }));
    add(`freedraw/${variability}/streamline0.2`, freedraw(spiral(25), { strokeOptions: { variability, streamline: 0.2 } }));
    add(`freedraw/${variability}/width4`, freedraw(spiral(25), { strokeWidth: 4, strokeOptions: { variability, streamline: 0.5 } }));
    add(`freedraw/${variability}/loopFill`, freedraw([...spiral(12), [6 + 10, 0]], { backgroundColor: "#ffc9c9", fillStyle: "hachure", strokeOptions: { variability, streamline: 0.5 } }));
    add(`freedraw/${variability}/sharpTurn`, freedraw([[0, 0], [40, 0], [80, 0], [40, 2], [0, 4]], { strokeOptions: { variability, streamline: 0.5 } }));
  }
  const pressurePoints = spiral(20);
  add("freedraw/variable/realPressure", freedraw(pressurePoints, {
    simulatePressure: false,
    pressures: pressurePoints.map((_, i) => 0.2 + (i % 7) / 10),
    strokeOptions: { variability: "variable", streamline: 0.5 },
  }));
  add("freedraw/variable/shortPressures", freedraw(pressurePoints, {
    simulatePressure: false,
    pressures: [0.3, 0.9],
    strokeOptions: { variability: "variable", streamline: 0.5 },
  }));
  add("freedraw/strokeOptionsMissing", (() => {
    const el = freedraw(spiral(10));
    delete el.strokeOptions;
    return el;
  })());
  add("freedraw/tinyNumbers", freedraw([[0, 0], [1e-7, 3e-7], [0.0000015, 2]], { strokeOptions: { variability: "constant", streamline: 0 } }));
  // A single point draws a circle of radius strokeWidth * 1.4 = 2.8 starting at x + 2.8, so
  // the first outline x is about 1.5e-7: JS prints it in exponent form, and the path's
  // TO_FIXED_PRECISION regex then drops the exponent, turning it into 1.49.
  add("freedraw/exponentQuirk", freedraw([[-2.79999985, 1.2e-7]], { strokeOptions: { variability: "constant", streamline: 0.5 } }));
  return cases;
}

export const shapeElements = [
  ...genericVariants("rectangle"),
  ...genericVariants("diamond"),
  ...genericVariants("ellipse"),
  ...linearVariants(),
  ...freedrawVariants(),
  { label: "text", element: element("text", { text: "hi", fontSize: 20, fontFamily: 5, textAlign: "left", verticalAlign: "top", containerId: null, originalText: "hi", autoResize: true, lineHeight: 1.25 }) },
  { label: "image", element: element("image", { fileId: null, status: "pending", scale: [1, 1], crop: null }) },
  { label: "frame", element: element("frame", { name: null }) },
];

/** Render contexts: dark mode flips colors; the canvas background feeds outline arrowheads. */
export const renderContexts = [
  { label: "light", theme: "light", canvasBackgroundColor: "#ffffff" },
  { label: "dark", theme: "dark", canvasBackgroundColor: "#ffffff" },
  { label: "lightTinted", theme: "light", canvasBackgroundColor: "#fffce8" },
];

export const colors = [
  "#1e1e1e", "#ffffff", "#000", "#fff", "#e03131", "#ffc9c9", "#a5d8ff80", "#a5d8ff00", "#abcd", "e03131", "#FFC9C9",
  "transparent", "TRANSPARENT", " #e03131 ", "red", "RebeccaPurple", "white", "black",
  "rgb(255, 0, 0)", "rgba(0, 128, 255, 0.5)", "rgba(0,0,0,0)", "rgb(100%, 50%, 0%)", "rgb 10 20 30", "rgb(300, -5, 1.5)",
  "hsl(120, 100%, 50%)", "hsla(240, 50%, 50%, 0.25)", "hsv(0, 100%, 100%)", "hsva(60 1 1 .5)",
  "rgba(10, 20, 30, 1.5)", "rgba(10, 20, 30, -1)", "rgba(10, 20, 30, 50%)", "rgb(.5, .5, .5)",
  "", "not a color", "#12345", "#1234567", "xxrgb(1, 2, 3)yy",
];

export const fractionalKeys = [
  [null, null], [null, "a0"], ["a0", null], ["a0", "a1"], ["a0", "a0V"], ["a0V", "a1"], ["a1", "a2"],
  ["Zz", "a0"], ["a0", "a01"], ["zzzzzzzzzzzzzzzzzzzzzzzzzzz", null], [null, "A00000000000000000000000001"],
  ["a1", "a0"], ["a0", "a0"], ["a00", null], ["b", null], ["", null], ["a0", "a0zz"], ["a9", "aA"], ["az", "b00"],
];

export const fractionalRanges = [
  [null, null, 0], [null, null, 5], ["a0", null, 4], [null, "a0", 4], ["a0", "a1", 7], ["a0", "a0V", 3], ["a1", "a0", 2],
];

/** [label, indices (null = missing), moved positions] for syncMovedIndices / syncInvalidIndices. */
export const indexScenarios = [
  ["allValid", ["a0", "a1", "a2", "a3"], [1]],
  ["newAtEnd", ["a0", "a1", null], [2]],
  ["newAtStart", [null, "a0", "a1"], [0]],
  ["movedDown", ["a0", "a2", "a1", "a3"], [2]],
  ["movedBlock", ["a0", "a3", "a4", "a1", "a2"], [1, 2]],
  ["allMissing", [null, null, null], [0, 1, 2]],
  ["duplicate", ["a0", "a1", "a1", "a2"], [2]],
  ["invalidFormat", ["a0", "zz", "a2"], [1]],
  ["trailingZero", ["a0", "a10", "a2"], [1]],
  ["descending", ["a3", "a2", "a1", "a0"], [3]],
  ["movedWrong", ["a0", "a1", "a1V", "a1"], [0]],
];

/** [case name, newElement.ts function, opts]; id and seed are fixed, everything else defaults. */
export const newElementCalls = [
  ["rectangle/defaults", "newElement", { type: "rectangle", id: "r1", seed: 5, x: 10, y: 20 }],
  ["diamond/props", "newElement", {
    type: "diamond", id: "d1", seed: 6, x: 1, y: 2, width: 30, height: 40, strokeColor: "#e03131",
    backgroundColor: "#ffc9c9", fillStyle: "hachure", strokeWidth: 4, strokeStyle: "dashed", roughness: 2,
    opacity: 50, roundness: { type: 2 }, groupIds: ["g1"], locked: true,
  }],
  ["ellipse/defaults", "newElement", { type: "ellipse", id: "e1", seed: 7, x: 0, y: 0 }],
  ["line/defaults", "newLinearElement", { type: "line", id: "l1", seed: 8, x: 0, y: 0, points: [[0, 0], [10, 10]] }],
  ["arrow/defaults", "newArrowElement", { type: "arrow", id: "a1", seed: 9, x: 0, y: 0, points: [[0, 0], [10, 10]], endArrowhead: "arrow" }],
  ["arrow/noHeads", "newArrowElement", { type: "arrow", id: "a2", seed: 10, x: 0, y: 0, points: [] }],
  ["freedraw/defaults", "newFreeDrawElement", { type: "freedraw", id: "f1", seed: 11, x: 0, y: 0, points: [[0, 0]], simulatePressure: true }],
  ["freedraw/pressures", "newFreeDrawElement", {
    type: "freedraw", id: "f2", seed: 12, x: 0, y: 0, points: [[0, 0], [3, 4]], pressures: [0.5, 0.7],
    simulatePressure: false, strokeOptions: { variability: "constant", streamline: 0.5 },
  }],
];
```

- [ ] **Step 5：建立 `tools/baseline/scene/generate.mjs`**

```js
// Generates crates/scene/tests/baseline/*.json by running Excalidraw's own source at the
// pinned commit (fetched into .cache/) on the inputs in cases.mjs.
//
//   cd tools/baseline && npm ci && npm run scene

import { join } from "node:path";

import { EXCALIDRAW_COMMIT, bundleExcalidraw } from "../lib/excalidraw.mjs";
import { REPO_ROOT, assertVersions, runCase, writeGroup } from "../lib/harness.mjs";
import { colors, fractionalKeys, fractionalRanges, indexScenarios, newElementCalls, renderContexts, shapeElements } from "./cases.mjs";

// Versions from Excalidraw's yarn.lock at the pinned commit.
assertVersions({
  roughjs: { pkg: "roughjs", from: null, version: "4.6.4" },
  "perfect-freehand": { pkg: "perfect-freehand", from: null, version: "1.2.0" },
  "points-on-curve": { pkg: "points-on-curve", from: null, version: "1.0.1" },
  tinycolor2: { pkg: "tinycolor2", from: null, version: "1.6.0" },
});

const lib = await bundleExcalidraw(
  "scene",
  `
  export { ShapeCache, generateRoughOptions, getFreedrawOutlinePoints } from "@excalidraw/element/shape";
  export { newElement, newLinearElement, newArrowElement, newFreeDrawElement } from "@excalidraw/element/newElement";
  export { syncMovedIndices, syncInvalidIndices } from "@excalidraw/element/fractionalIndex";
  export { applyDarkModeFilter, isTransparent } from "@excalidraw/common";
  export { generateKeyBetween, generateNKeysBetween } from "@excalidraw/fractional-indexing";
  `,
);

const outDir = join(REPO_ROOT, "crates", "scene", "tests", "baseline");
const source = `excalidraw@${EXCALIDRAW_COMMIT} (roughjs@4.6.4, perfect-freehand@1.2.0, tinycolor2@1.6.0)`;

function drawable(d) {
  const { randomizer, ...options } = d.options;
  return { shape: d.shape, options, sets: d.sets.map(({ type, ops }) => ({ type, ops })) };
}

/**
 * The freedraw stroke is an SVG path string built by getSvgPathFromStroke:
 * "M x,y Q x,y x,y ... L x,y Z", numbers already cut by its TO_FIXED_PRECISION regex.
 * Recorded as ops so the Rust side compares numbers, not JS number formatting.
 */
function svgPathOps(path) {
  const ops = [];
  let command = null;
  let pending = [];
  for (const token of path.split(" ").filter(Boolean)) {
    if (/^[A-Z]$/.test(token)) {
      command = token;
      if (command === "Z") ops.push({ op: "close", data: [] });
      continue;
    }
    pending.push(...token.split(",").map(Number));
    if (command === "M" && pending.length === 2) {
      ops.push({ op: "move", data: pending });
      pending = [];
    } else if (command === "L" && pending.length === 2) {
      ops.push({ op: "line", data: pending });
      pending = [];
    } else if (command === "Q" && pending.length === 4) {
      ops.push({ op: "quad", data: pending });
      pending = [];
    }
  }
  if (pending.length) throw new Error(`dangling numbers in ${path}`);
  return ops;
}

function shapeJson(shape) {
  if (shape === null) return null;
  if (Array.isArray(shape)) {
    return shape.map((s) => (typeof s === "string" ? { svgPath: svgPathOps(s) } : drawable(s)));
  }
  return drawable(shape);
}

const shapes = [];
const roughOptions = [];
for (const { label, element } of shapeElements) {
  for (const context of renderContexts) {
    const renderConfig = {
      isExporting: true,
      canvasBackgroundColor: context.canvasBackgroundColor,
      embedsValidationStatus: null,
      theme: context.theme,
    };
    const args = [structuredClone(element), { theme: context.theme, canvasBackgroundColor: context.canvasBackgroundColor }];
    const name = `${label}/${context.label}`;
    shapes.push({
      name,
      call: "generateElementShape",
      args,
      ...runCase(name, () => shapeJson(lib.ShapeCache.generateElementShape(structuredClone(element), renderConfig))),
    });
  }
  if (["rectangle", "diamond", "ellipse", "line", "arrow", "freedraw"].includes(element.type)) {
    // continuousPath only sets preserveVertices and dark mode only maps colors, so two
    // combinations cover both flags.
    for (const [continuousPath, isDarkMode] of [[false, false], [true, true]]) {
      const name = `${label}/continuous${continuousPath}/dark${isDarkMode}`;
      roughOptions.push({
        name,
        call: "generateRoughOptions",
        args: [structuredClone(element), continuousPath, isDarkMode],
        ...runCase(name, () => lib.generateRoughOptions(structuredClone(element), continuousPath, isDarkMode)),
      });
    }
  }
}
const FAMILY = { rectangle: "generic", diamond: "generic", ellipse: "generic", line: "linear", arrow: "linear", freedraw: "freedraw" };
for (const family of ["generic", "linear", "freedraw", "other"]) {
  writeGroup(outDir, `shapes_${family}`, source, shapes.filter((c) => (FAMILY[c.args[0].type] ?? "other") === family));
}
writeGroup(outDir, "rough_options", source, roughOptions);

writeGroup(
  outDir,
  "freedraw_outline",
  source,
  shapeElements
    .filter(({ element }) => element.type === "freedraw")
    .map(({ label, element }) => ({
      name: label,
      call: "getFreedrawOutlinePoints",
      args: [element],
      ...runCase(label, () => lib.getFreedrawOutlinePoints(structuredClone(element))),
    })),
);

writeGroup(
  outDir,
  "colors",
  source,
  colors.flatMap((color) => [
    { name: `dark/${JSON.stringify(color)}`, call: "applyDarkModeFilter", args: [color], ...runCase(color, () => lib.applyDarkModeFilter(color)) },
    { name: `transparent/${JSON.stringify(color)}`, call: "isTransparent", args: [color], ...runCase(color, () => lib.isTransparent(color)) },
  ]),
);

const indexCases = [
  ...fractionalKeys.map(([a, b]) => {
    const name = `between/${a}/${b}`;
    return { name, call: "generateKeyBetween", args: [a, b], ...runCase(name, () => lib.generateKeyBetween(a, b)) };
  }),
  ...fractionalRanges.map(([a, b, n]) => {
    const name = `nBetween/${a}/${b}/${n}`;
    return { name, call: "generateNKeysBetween", args: [a, b, n], ...runCase(name, () => lib.generateNKeysBetween(a, b, n)) };
  }),
];
for (const [label, indices, moved] of indexScenarios) {
  const elements = () =>
    indices.map((index, i) => ({ id: `e${i}`, type: "rectangle", index, version: 1, versionNonce: 0, updated: 1 }));
  const summary = (els) => els.map(({ id, index, version }) => ({ id, index, version }));
  const movedName = `syncMoved/${label}`;
  indexCases.push({
    name: movedName,
    call: "syncMovedIndices",
    args: [indices, moved],
    ...runCase(movedName, () => {
      const els = elements();
      const movedMap = new Map(moved.map((i) => [els[i].id, els[i]]));
      return summary(lib.syncMovedIndices(els, movedMap));
    }),
  });
  const invalidName = `syncInvalid/${label}`;
  indexCases.push({
    name: invalidName,
    call: "syncInvalidIndices",
    args: [indices],
    ...runCase(invalidName, () => summary(lib.syncInvalidIndices(elements()))),
  });
}
writeGroup(outDir, "fractional_index", source, indexCases);

// newElement stamps `updated`/`created` with Date.now(); pin it so the defaults are comparable.
// Callers pass id and seed, the two values that are random by design.
Date.now = () => 1;
writeGroup(
  outDir,
  "new_element",
  source,
  newElementCalls.map(([name, fn, opts]) => ({
    name,
    call: fn,
    args: [opts],
    ...runCase(name, () => lib[fn](structuredClone(opts))),
  })),
);
```

- [ ] **Step 6：產生基準**

Run: `cd tools/baseline && npm run -s scene`
Expected: 第一次執行會 git fetch Excalidraw（需要網路）。stderr 會印三次 `Elbow arrow with extreme point positions detected. Arrow not rendered.`，那是 Excalidraw 對 `arrow/elbow/extreme` 案例的正常反應。stdout 是（KiB 可以有 ±1 的差異，案例數必須相同）：

```
shapes_generic: 315 cases (0 structure-only), 1625 KiB
shapes_linear: 243 cases (0 structure-only), 1092 KiB
shapes_freedraw: 75 cases (0 structure-only), 308 KiB
shapes_other: 9 cases (0 structure-only), 6 KiB
rough_options: 422 cases (0 structure-only), 339 KiB
freedraw_outline: 25 cases (0 structure-only), 84 KiB
colors: 74 cases (0 structure-only), 9 KiB
fractional_index: 48 cases (0 structure-only), 9 KiB
new_element: 8 cases (0 structure-only), 5 KiB
```

- [ ] **Step 7：確認可重現，而且 M1 的基準沒有被新依賴影響**

Run:

```bash
git add crates/scene/tests/baseline
cd tools/baseline && npm run -s scene > /dev/null 2>&1 && npm run -s rough > /dev/null && cd ../..
git diff --exit-code -- crates && echo reproducible
```

Expected: 印出 `reproducible`。

- [ ] **Step 8：Commit**

```bash
git add tools/baseline crates/scene/tests/baseline
git commit -m "Add scene baseline generator running Excalidraw source at the pinned commit"
```

---

### Task 3：`scene` crate 的檔案層

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/scene/Cargo.toml`
- Create: `crates/scene/src/lib.rs`
- Create: `crates/scene/src/json.rs`
- Create: `crates/scene/src/element.rs`
- Create: `crates/scene/src/file.rs`

**Interfaces:**
- Consumes: 無（`rough` 在 Task 8 才會用到）。
- Produces:
  - `scene::json::{Slot<T> (Missing | Null | Value(T)), Slot::is_missing, Slot::value, semantic_eq(&Value, &Value) -> bool, normalize_numbers(&mut Value)}`
  - `scene::element::{ElementBase, Roundness, GenericElement, LinearElement, FreedrawElement, StrokeOptions, TextElement, Element}`；欄位名稱見 Step 4 的程式碼。`Element` 的變體：`Rectangle`、`Diamond`、`Ellipse`（`GenericElement`）、`Line`、`Arrow`（`LinearElement`）、`Text`、`Freedraw`、`Raw(Value)`。方法：`from_value(Value) -> Element`、`to_value(&self) -> Value`、`base(&self) -> Option<&ElementBase>`、`base_mut`、`id(&self) -> Option<&str>`、`index(&self) -> Option<&str>`、`set_index(&mut self, String)`、`is_deleted(&self) -> bool`。
  - `scene::file::{SceneFile, LoadError, NapkinView, DEFAULT_VIEW_BACKGROUND_COLOR}`；`SceneFile { pub elements: Vec<Element>, pub app_state: Map<String, Value> }`、`new()`、`from_json_str(&str) -> Result<SceneFile, LoadError>`、`to_json_string(&self) -> String`、`view_background_color(&self) -> &str`、`napkin_view(&self) -> Option<NapkinView>`、`set_napkin_view(&mut self, NapkinView)`。
  - crate 根目錄 re-export `scene::{Element, SceneFile}`。

- [ ] **Step 1：更新 workspace `Cargo.toml`**

```toml
[workspace]
members = ["crates/rough", "crates/scene", "crates/testkit"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.98"
publish = false

[workspace.dependencies]
getrandom = "0.3"
regex = "1"
rough = { path = "crates/rough" }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["float_roundtrip", "preserve_order"] }
testkit = { path = "crates/testkit" }
```

`preserve_order` 讓 `extra` 裡的未知欄位照原檔順序寫回，存檔後的 diff 比較小。M1 的 `testkit` 比對 JSON 物件時不看 key 順序，不受影響。

- [ ] **Step 2：建立 `crates/scene/Cargo.toml`**

```toml
[package]
name = "scene"
version = "0.0.0"
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[dependencies]
getrandom.workspace = true
regex.workspace = true
rough.workspace = true
serde.workspace = true
serde_json.workspace = true

[dev-dependencies]
testkit.workspace = true
```

`getrandom`、`regex`、`rough` 分別在 Task 5、7、8 才用到；未使用的依賴不會產生警告。

- [ ] **Step 3：建立 `crates/scene/src/json.rs`（含測試）**

```rust
//! JSON plumbing for the file layer: a field that remembers whether it was absent, and
//! the comparison and number formatting that make "read then write" lossless.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, Value};

/// A field that may be absent, `null`, or hold a value. Excalidraw added fields over the
/// years (`index`, `created`, ...), so an older file lacks keys a newer one writes as
/// `null`; writing either back as the other would change the file.
///
/// Use with `#[serde(default, skip_serializing_if = "Slot::is_missing")]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Slot<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

impl<T> Slot<T> {
    pub fn is_missing(&self) -> bool {
        matches!(self, Slot::Missing)
    }

    /// The value, treating absent and `null` alike (JS `?.` / `??`).
    pub fn value(&self) -> Option<&T> {
        match self {
            Slot::Value(v) => Some(v),
            _ => None,
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Slot<T> {
    /// Only called when the key is present: `null` becomes `Null`.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            None => Slot::Null,
            Some(v) => Slot::Value(v),
        })
    }
}

impl<T: Serialize> Serialize for Slot<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Slot::Value(v) => v.serialize(serializer),
            Slot::Missing | Slot::Null => serializer.serialize_none(),
        }
    }
}

/// Equality as a JS reader sees two JSON documents: key order is irrelevant and numbers
/// compare as f64 (`1` and `1.0` are equal). Spec §9.2's round-trip criterion.
pub fn semantic_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| semantic_eq(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| semantic_eq(v, w)))
        }
        _ => a == b,
    }
}

/// Largest integer JS numbers hold exactly.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Rewrites integral floats as JSON integers, so a saved file reads like
/// `JSON.stringify` output (`"version": 2`, not `2.0`). Values are unchanged.
pub fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|_| n.is_f64())
                && f.fract() == 0.0
                && f.abs() <= MAX_SAFE_INTEGER
            {
                *n = Number::from(f as i64);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_numbers),
        Value::Object(map) => map.values_mut().for_each(normalize_numbers),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn semantic_eq_ignores_key_order_and_number_spelling() {
        assert!(semantic_eq(
            &json!({"a": 1, "b": [2.0]}),
            &json!({"b": [2], "a": 1.0})
        ));
        assert!(!semantic_eq(&json!({"a": 1}), &json!({"a": 1, "b": null})));
        assert!(!semantic_eq(&json!([1, 2]), &json!([2, 1])));
    }

    #[test]
    fn normalize_numbers_writes_integers_like_javascript() {
        let mut v = json!({"version": 2.0, "x": -0.0, "y": 0.5, "big": 1e300});
        normalize_numbers(&mut v);
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"version":2,"x":0,"y":0.5,"big":1e+300}"#
        );
    }
}
```

- [ ] **Step 4：建立 `crates/scene/src/element.rs`（含測試）**

```rust
//! Excalidraw elements (`packages/element/src/types.ts` at the pinned commit).
//!
//! Every struct keeps unknown keys in `extra` (spec §5.2). String-valued enums such as
//! `fillStyle` stay `String`: Excalidraw's code compares strings and falls through to a
//! default for unexpected values, and a Rust enum would reject such a file instead.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::json::{Slot, semantic_eq};

/// Fields every element type has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementBase {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub angle: f64,
    pub stroke_color: String,
    pub background_color: String,
    pub fill_style: String,
    pub stroke_width: f64,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub group_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub index: Slot<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub roundness: Slot<Roundness>,
    pub seed: f64,
    pub version: f64,
    pub version_nonce: f64,
    pub is_deleted: bool,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub updated: Slot<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Roundness {
    #[serde(rename = "type")]
    pub kind: f64,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub value: Slot<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `rectangle`, `diamond`, `ellipse`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenericElement {
    #[serde(flatten)]
    pub base: ElementBase,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `line`, `arrow`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearElement {
    #[serde(flatten)]
    pub base: ElementBase,
    pub points: Vec<[f64; 2]>,
    /// Absent differs from `null`: shape.ts defaults a missing `endArrowhead` to "arrow".
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub start_arrowhead: Slot<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub end_arrowhead: Slot<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elbowed: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreedrawElement {
    #[serde(flatten)]
    pub base: ElementBase,
    pub points: Vec<[f64; 2]>,
    pub pressures: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulate_pressure: Option<bool>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub stroke_options: Slot<StrokeOptions>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeOptions {
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub variability: Slot<String>,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub streamline: Slot<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextElement {
    #[serde(flatten)]
    pub base: ElementBase,
    pub text: String,
    pub font_size: f64,
    pub font_family: f64,
    pub text_align: String,
    pub vertical_align: String,
    #[serde(default, skip_serializing_if = "Slot::is_missing")]
    pub container_id: Slot<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_resize: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    Rectangle(GenericElement),
    Diamond(GenericElement),
    Ellipse(GenericElement),
    Line(LinearElement),
    Arrow(LinearElement),
    Text(TextElement),
    Freedraw(FreedrawElement),
    /// Any other type (image, frame, embeddable, stickynote, ...), and any element of a
    /// known type whose JSON the typed struct cannot reproduce exactly. Kept verbatim.
    Raw(Value),
}

/// Parses `value` as `T` only if serializing `T` gives back the same JSON. This makes
/// "read then write" lossless by construction: a schema mistake here demotes an element
/// to `Raw` (drawn as a placeholder box) instead of silently rewriting the file.
fn exact<T: DeserializeOwned + Serialize>(value: &Value) -> Option<T> {
    let parsed: T = serde_json::from_value(value.clone()).ok()?;
    let written = serde_json::to_value(&parsed).ok()?;
    semantic_eq(&written, value).then_some(parsed)
}

impl Element {
    pub fn from_value(value: Value) -> Element {
        let typed = match value.get("type").and_then(Value::as_str) {
            Some("rectangle") => exact(&value).map(Element::Rectangle),
            Some("diamond") => exact(&value).map(Element::Diamond),
            Some("ellipse") => exact(&value).map(Element::Ellipse),
            Some("line") => exact(&value).map(Element::Line),
            Some("arrow") => exact(&value).map(Element::Arrow),
            Some("text") => exact(&value).map(Element::Text),
            Some("freedraw") => exact(&value).map(Element::Freedraw),
            _ => None,
        };
        typed.unwrap_or(Element::Raw(value))
    }

    pub fn to_value(&self) -> Value {
        let written = match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => {
                serde_json::to_value(e)
            }
            Element::Line(e) | Element::Arrow(e) => serde_json::to_value(e),
            Element::Text(e) => serde_json::to_value(e),
            Element::Freedraw(e) => serde_json::to_value(e),
            Element::Raw(v) => return v.clone(),
        };
        written.expect("element structs serialize to JSON")
    }

    pub fn base(&self) -> Option<&ElementBase> {
        match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => Some(&e.base),
            Element::Line(e) | Element::Arrow(e) => Some(&e.base),
            Element::Text(e) => Some(&e.base),
            Element::Freedraw(e) => Some(&e.base),
            Element::Raw(_) => None,
        }
    }

    pub fn base_mut(&mut self) -> Option<&mut ElementBase> {
        match self {
            Element::Rectangle(e) | Element::Diamond(e) | Element::Ellipse(e) => Some(&mut e.base),
            Element::Line(e) | Element::Arrow(e) => Some(&mut e.base),
            Element::Text(e) => Some(&mut e.base),
            Element::Freedraw(e) => Some(&mut e.base),
            Element::Raw(_) => None,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Element::Raw(v) => v.get("id").and_then(Value::as_str),
            _ => self.base().map(|b| b.id.as_str()),
        }
    }

    /// `element.index`; `None` for absent, `null` or non-string.
    pub fn index(&self) -> Option<&str> {
        match self {
            Element::Raw(v) => v.get("index").and_then(Value::as_str),
            _ => self
                .base()
                .and_then(|b| b.index.value())
                .map(String::as_str),
        }
    }

    pub fn set_index(&mut self, index: String) {
        match self {
            Element::Raw(Value::Object(map)) => {
                map.insert("index".into(), Value::String(index));
            }
            Element::Raw(_) => {}
            _ => self.base_mut().expect("typed element").index = Slot::Value(index),
        }
    }

    pub fn is_deleted(&self) -> bool {
        match self {
            Element::Raw(v) => v.get("isDeleted").and_then(Value::as_bool).unwrap_or(false),
            _ => self.base().is_some_and(|b| b.is_deleted),
        }
    }
}

impl Serialize for Element {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Element {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Value::deserialize(deserializer).map(Element::from_value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn rectangle() -> Value {
        json!({
            "id": "r", "type": "rectangle", "x": 1, "y": 2, "width": 3, "height": 4, "angle": 0,
            "strokeColor": "#1e1e1e", "backgroundColor": "transparent", "fillStyle": "solid",
            "strokeWidth": 2, "strokeStyle": "solid", "roughness": 1, "opacity": 100,
            "groupIds": [], "frameId": null, "index": "a0", "roundness": null, "seed": 1,
            "version": 3, "versionNonce": 4, "isDeleted": false, "boundElements": null,
            "updated": 5, "link": null, "locked": false, "customData": {"k": [1, 2]}
        })
    }

    #[test]
    fn typed_element_writes_back_identical_json() {
        let element = Element::from_value(rectangle());
        assert!(matches!(element, Element::Rectangle(_)));
        assert!(semantic_eq(&element.to_value(), &rectangle()));
    }

    #[test]
    fn absent_and_null_fields_stay_distinct() {
        let mut old = rectangle();
        old.as_object_mut().unwrap().remove("index");
        let element = Element::from_value(old.clone());
        assert!(matches!(element, Element::Rectangle(_)));
        assert_eq!(element.to_value().get("index"), None);
        assert_eq!(Element::from_value(rectangle()).index(), Some("a0"));
    }

    #[test]
    fn unrepresentable_known_type_falls_back_to_raw() {
        let mut bad = rectangle();
        bad["roughness"] = json!("rough");
        let element = Element::from_value(bad.clone());
        assert_eq!(element, Element::Raw(bad));
    }

    #[test]
    fn null_in_absent_only_field_falls_back_to_raw() {
        // `elbowed` is `Option<bool>`: serde reads `null` as `None` and would then omit the
        // key on write. Only the exact round-trip check in `from_value` catches that.
        let mut arrow = rectangle();
        arrow["type"] = json!("arrow");
        arrow["points"] = json!([[0, 0], [10, 10]]);
        arrow["elbowed"] = Value::Null;
        assert!(matches!(Element::from_value(arrow), Element::Raw(_)));
    }

    #[test]
    fn unknown_type_is_raw_but_indexable() {
        let mut element =
            Element::from_value(json!({"id": "i", "type": "image", "index": null, "fileId": "f"}));
        assert_eq!(element.id(), Some("i"));
        assert_eq!(element.index(), None);
        element.set_index("a1".into());
        assert_eq!(
            element.to_value(),
            json!({"id": "i", "type": "image", "index": "a1", "fileId": "f"})
        );
    }
}
```

- [ ] **Step 5：建立 `crates/scene/src/file.rs`（含測試）**

```rust
//! The `.excalidraw` document: `{ type, version, source, elements, appState, files }`.

use std::fmt;

use serde_json::{Map, Value, json};

use crate::element::Element;
use crate::json::normalize_numbers;

/// `COLOR_PALETTE.white`, Excalidraw's default `viewBackgroundColor`.
pub const DEFAULT_VIEW_BACKGROUND_COLOR: &str = "#ffffff";

#[derive(Debug)]
pub enum LoadError {
    Json(serde_json::Error),
    /// The top level is not an object with `"type": "excalidraw"`.
    NotExcalidraw,
    ElementsNotArray,
    AppStateNotObject,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Json(e) => write!(f, "invalid JSON: {e}"),
            LoadError::NotExcalidraw => {
                write!(f, "not an Excalidraw file (type is not \"excalidraw\")")
            }
            LoadError::ElementsNotArray => write!(f, "\"elements\" is not an array"),
            LoadError::AppStateNotObject => write!(f, "\"appState\" is not an object"),
        }
    }
}

impl std::error::Error for LoadError {}

/// The view napkin stores in `appState.napkin` (spec §5.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NapkinView {
    pub scroll_x: f64,
    pub scroll_y: f64,
    pub zoom: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneFile {
    /// Every top-level key in file order. `elements` and `appState` hold `null` while the
    /// parsed copies below are authoritative; `to_json_string` puts them back in place.
    root: Map<String, Value>,
    pub elements: Vec<Element>,
    pub app_state: Map<String, Value>,
}

impl SceneFile {
    /// An empty document as napkin creates it.
    pub fn new() -> SceneFile {
        let mut root = Map::new();
        root.insert("type".into(), json!("excalidraw"));
        root.insert("version".into(), json!(2));
        root.insert("source".into(), json!("napkin"));
        root.insert("elements".into(), Value::Null);
        root.insert("appState".into(), Value::Null);
        root.insert("files".into(), json!({}));
        SceneFile {
            root,
            elements: Vec::new(),
            app_state: Map::new(),
        }
    }

    pub fn from_json_str(text: &str) -> Result<SceneFile, LoadError> {
        let value: Value = serde_json::from_str(text).map_err(LoadError::Json)?;
        let Value::Object(mut root) = value else {
            return Err(LoadError::NotExcalidraw);
        };
        if root.get("type").and_then(Value::as_str) != Some("excalidraw") {
            return Err(LoadError::NotExcalidraw);
        }
        let elements = match root.get_mut("elements").map(Value::take) {
            None => Vec::new(),
            Some(Value::Array(items)) => items.into_iter().map(Element::from_value).collect(),
            Some(_) => return Err(LoadError::ElementsNotArray),
        };
        let app_state = match root.get_mut("appState").map(Value::take) {
            None => Map::new(),
            Some(Value::Object(map)) => map,
            Some(_) => return Err(LoadError::AppStateNotObject),
        };
        Ok(SceneFile {
            root,
            elements,
            app_state,
        })
    }

    /// Pretty-printed like Excalidraw's `JSON.stringify(data, null, 2)`.
    pub fn to_json_string(&self) -> String {
        let mut root = self.root.clone();
        let elements = Value::Array(self.elements.iter().map(Element::to_value).collect());
        if root.contains_key("elements") || !self.elements.is_empty() {
            root.insert("elements".into(), elements);
        }
        if root.contains_key("appState") || !self.app_state.is_empty() {
            root.insert("appState".into(), Value::Object(self.app_state.clone()));
        }
        let mut value = Value::Object(root);
        normalize_numbers(&mut value);
        serde_json::to_string_pretty(&value).expect("JSON values serialize")
    }

    pub fn view_background_color(&self) -> &str {
        self.app_state
            .get("viewBackgroundColor")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_VIEW_BACKGROUND_COLOR)
    }

    /// `None` when absent or malformed, e.g. after excalidraw.com re-saved the file.
    pub fn napkin_view(&self) -> Option<NapkinView> {
        let view = self.app_state.get("napkin")?;
        Some(NapkinView {
            scroll_x: view.get("scrollX")?.as_f64()?,
            scroll_y: view.get("scrollY")?.as_f64()?,
            zoom: view.get("zoom")?.as_f64()?,
        })
    }

    pub fn set_napkin_view(&mut self, view: NapkinView) {
        self.app_state.insert(
            "napkin".into(),
            json!({ "scrollX": view.scroll_x, "scrollY": view.scroll_y, "zoom": view.zoom }),
        );
    }
}

impl Default for SceneFile {
    fn default() -> Self {
        SceneFile::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::semantic_eq;

    #[test]
    fn unknown_top_level_and_app_state_keys_survive() {
        let text = r##"{"type":"excalidraw","version":2,"source":"https://excalidraw.com",
            "elements":[{"type":"image","id":"i","fileId":"f1"}],
            "appState":{"gridSize":20,"viewBackgroundColor":"#fffce8"},
            "files":{"f1":{"dataURL":"data:image/png;base64,AAAA"}},"future":{"x":1}}"##;
        let file = SceneFile::from_json_str(text).unwrap();
        let written: Value = serde_json::from_str(&file.to_json_string()).unwrap();
        assert!(semantic_eq(&written, &serde_json::from_str(text).unwrap()));
        assert_eq!(file.view_background_color(), "#fffce8");
    }

    #[test]
    fn missing_elements_and_app_state_stay_missing() {
        let file = SceneFile::from_json_str(r#"{"type":"excalidraw"}"#).unwrap();
        assert_eq!(file.to_json_string(), "{\n  \"type\": \"excalidraw\"\n}");
    }

    #[test]
    fn napkin_view_round_trips() {
        let mut file = SceneFile::new();
        assert_eq!(file.napkin_view(), None);
        let view = NapkinView {
            scroll_x: -10.5,
            scroll_y: 3.0,
            zoom: 1.25,
        };
        file.set_napkin_view(view);
        let reread = SceneFile::from_json_str(&file.to_json_string()).unwrap();
        assert_eq!(reread.napkin_view(), Some(view));
    }

    #[test]
    fn rejects_other_documents() {
        assert!(matches!(
            SceneFile::from_json_str("[]"),
            Err(LoadError::NotExcalidraw)
        ));
        assert!(matches!(
            SceneFile::from_json_str(r#"{"type":"excalidrawlib"}"#),
            Err(LoadError::NotExcalidraw)
        ));
        assert!(matches!(
            SceneFile::from_json_str(r#"{"type":"excalidraw","elements":{}}"#),
            Err(LoadError::ElementsNotArray)
        ));
        assert!(matches!(
            SceneFile::from_json_str("{"),
            Err(LoadError::Json(_))
        ));
    }
}
```

- [ ] **Step 6：建立 `crates/scene/src/lib.rs`**

```rust
//! Excalidraw's file format and element rules at commit afa3a653fc5d2b742adcbd5a6063187b056d2419.
//! No egui, no rendering: shape output is data (spec §4.2).

pub mod element;
pub mod file;
pub mod json;

pub use crate::element::Element;
pub use crate::file::SceneFile;
```

- [ ] **Step 7：跑測試**

Run: `cargo test -p scene`
Expected: 11 passed（json 2、element 5、file 4）。

- [ ] **Step 8：突變檢查**

把 `exact` 的最後一行暫時改成 `Some(parsed)`（不做寫回比較），跑 `cargo test -p scene --lib element::`。
Expected: `null_in_absent_only_field_falls_back_to_raw` 失敗：serde 能解析 `"elbowed": null`，但寫回時會少掉這個 key，只有寫回比較擋得住。改回原樣。

- [ ] **Step 9：格式、lint、commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add Cargo.toml Cargo.lock crates/scene
git commit -m "Add scene file layer with lossless element round trip"
```

---

### Task 4：語料 round-trip 與涵蓋檢查

**Files:**
- Create: `crates/scene/tests/corpus.rs`

**Interfaces:**
- Consumes: Task 1 的語料；Task 3 的 `SceneFile`、`Element`、`json::semantic_eq`；`testkit::{diff, Compare}`。
- Produces: 無（驗收測試）。

- [ ] **Step 1：建立 `crates/scene/tests/corpus.rs`**

```rust
//! Spec §9.2 round trip: every file in `tests/corpus/`, drawn on excalidraw.com, must come
//! back semantically unchanged after `SceneFile` reads and writes it. A second test checks
//! the corpus still contains everything §9.2 asks for, so a thin corpus cannot pass.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use scene::json::semantic_eq;
use scene::{Element, SceneFile};
use serde_json::Value;
use testkit::{Compare, diff};

/// Element types `Element::from_value` parses into structs.
const TYPED: [&str; 7] = [
    "rectangle",
    "diamond",
    "ellipse",
    "line",
    "arrow",
    "text",
    "freedraw",
];

fn corpus() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "excalidraw"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no .excalidraw files in {}", dir.display());
    paths
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("readable corpus file");
            (p.file_name().unwrap().to_string_lossy().into_owned(), text)
        })
        .collect()
}

#[test]
fn corpus_round_trips() {
    for (name, text) in corpus() {
        let original: Value = serde_json::from_str(&text).expect("corpus file is JSON");
        let file = SceneFile::from_json_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        for element in &file.elements {
            if let Element::Raw(v) = element {
                let kind = v["type"].as_str().unwrap_or_default();
                assert!(
                    !TYPED.contains(&kind),
                    "{name}: {kind} element {} fell back to Raw; its JSON does not fit the struct",
                    v["id"]
                );
            }
        }
        let written: Value = serde_json::from_str(&file.to_json_string()).expect("written JSON");
        assert!(
            semantic_eq(&written, &original),
            "{name} changed on round trip: {}",
            diff(&original, &written, Compare::Exact)
                .unwrap_or_else(|| "difference below 1e-9".into())
        );
    }
}

#[test]
fn corpus_covers_spec_checklist() {
    let mut seen = BTreeSet::new();
    for (_, text) in corpus() {
        let root: Value = serde_json::from_str(&text).expect("corpus file is JSON");
        if root["files"].as_object().is_some_and(|f| !f.is_empty()) {
            seen.insert("binary files");
        }
        let elements = root["elements"].as_array().cloned().unwrap_or_default();
        let types: HashMap<&str, &str> = elements
            .iter()
            .filter_map(|e| Some((e["id"].as_str()?, e["type"].as_str()?)))
            .collect();
        for e in &elements {
            let kind = e["type"].as_str().unwrap_or_default();
            seen.insert(match kind {
                "image" => "image",
                "frame" => "frame",
                "stickynote" => "sticky note",
                _ => "",
            });
            if kind == "arrow" && (e["startBinding"].is_object() || e["endBinding"].is_object()) {
                seen.insert("arrow binding");
            }
            if kind == "arrow" && e["elbowed"] == true {
                seen.insert("elbow arrow");
            }
            if let Some(container) = e["containerId"].as_str().and_then(|id| types.get(id)) {
                seen.insert(if *container == "arrow" {
                    "arrow label"
                } else {
                    "container text"
                });
            }
            if e["text"]
                .as_str()
                .is_some_and(|t| t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)))
            {
                seen.insert("CJK text");
            }
            if e["backgroundColor"]
                .as_str()
                .is_some_and(|c| c != "transparent")
            {
                seen.insert(match e["fillStyle"].as_str() {
                    Some("hachure") => "fill hachure",
                    Some("cross-hatch") => "fill cross-hatch",
                    Some("solid") => "fill solid",
                    Some("zigzag") => "fill zigzag",
                    _ => "",
                });
            }
            seen.insert(match e["strokeStyle"].as_str() {
                Some("dashed") => "dashed stroke",
                Some("dotted") => "dotted stroke",
                _ => "",
            });
            if e["angle"].as_f64().is_some_and(|a| a != 0.0) {
                seen.insert("rotated element");
            }
            if e["groupIds"].as_array().is_some_and(|g| !g.is_empty()) {
                seen.insert("group");
            }
            if e["isDeleted"] == true {
                seen.insert("deleted element");
            }
            if kind == "freedraw" {
                seen.insert(match e["strokeOptions"]["variability"].as_str() {
                    Some("constant") => "freedraw constant",
                    Some("variable") => "freedraw variable",
                    _ => "",
                });
            }
        }
    }
    let required = [
        "arrow binding",
        "arrow label",
        "binary files",
        "CJK text",
        "container text",
        "dashed stroke",
        "deleted element",
        "dotted stroke",
        "elbow arrow",
        "fill cross-hatch",
        "fill hachure",
        "fill solid",
        "fill zigzag",
        "frame",
        "freedraw constant",
        "freedraw variable",
        "group",
        "image",
        "rotated element",
        "sticky note",
    ];
    let missing: Vec<_> = required.iter().filter(|r| !seen.contains(*r)).collect();
    assert!(missing.is_empty(), "corpus lacks: {missing:?}");
}
```

- [ ] **Step 2：跑測試**

Run: `cargo test -p scene --test corpus`
Expected: 2 passed。

`corpus_round_trips` 失敗時的處理：訊息是「fell back to Raw」代表 excalidraw.com 存出的某個欄位型態不在 `element.rs` 的預期內，例如某欄位缺席或為 `null`。把那個欄位改成 `Slot` 或 `Option`，或者若 napkin 用不到它，從結構移出、讓它留在 `extra`；在 `element.rs` 的測試裡加一個重現該情況的案例。不准修改語料檔。訊息是「changed on round trip」時，`diff` 會指出第一個不同的路徑。

- [ ] **Step 3：Commit**

```bash
git add crates/scene/tests/corpus.rs
git commit -m "Check the excalidraw.com corpus round-trips and covers spec 9.2"
```

---

### Task 5：新元件與版本號

**Files:**
- Create: `crates/scene/src/env.rs`
- Create: `crates/scene/src/new_element.rs`
- Create: `crates/scene/tests/baseline.rs`
- Modify: `crates/scene/src/lib.rs`

**Interfaces:**
- Consumes: Task 3 的元件結構。
- Produces:
  - `scene::env::{Env (trait: fill_random(&mut self, &mut [u8]), now_ms(&mut self) -> f64), SystemEnv, random_id(&mut impl Env) -> String, random_integer(&mut impl Env) -> f64}`
  - `scene::new_element::{DEFAULT_STROKE_STREAMLINE, ElementProps, GenericKind (Rectangle | Diamond | Ellipse), new_generic_element(GenericKind, ElementProps, &mut impl Env) -> Element, new_line_element(ElementProps, Vec<[f64; 2]>, &mut impl Env) -> Element, new_arrow_element(ElementProps, Vec<[f64; 2]>, Option<String>, Option<String>, &mut impl Env) -> Element, new_freedraw_element(ElementProps, Vec<[f64; 2]>, Vec<f64>, bool, Option<StrokeOptions>, &mut impl Env) -> Element, bump_version(&mut Element, &mut impl Env)}`

- [ ] **Step 1：先寫基準測試 `crates/scene/tests/baseline.rs`**

```rust
//! Compares scene with Excalidraw's own code at the pinned commit, recorded in
//! `tests/baseline/*.json` by tools/baseline/scene/generate.mjs.

use std::path::PathBuf;

use scene::element::{Roundness, StrokeOptions};
use scene::env::Env;
use scene::new_element::{
    ElementProps, GenericKind, new_arrow_element, new_freedraw_element, new_generic_element,
    new_line_element,
};
use serde_json::Value;
use testkit::{check_group, num, points_from};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/baseline")
}

/// Random bytes all zero, clock pinned at 1 ms, matching the generator's `Date.now = () => 1`.
struct FixedEnv;

impl Env for FixedEnv {
    fn fill_random(&mut self, bytes: &mut [u8]) {
        bytes.fill(0);
    }

    fn now_ms(&mut self) -> f64 {
        1.0
    }
}

/// `newElement` options -> `ElementProps`. Keys the constructors read separately are skipped;
/// anything else unknown fails the case.
fn props_from(opts: &Value) -> ElementProps {
    let mut props = ElementProps::default();
    for (key, v) in opts.as_object().expect("opts object") {
        let s = || v.as_str().expect("string").to_owned();
        match key.as_str() {
            "x" => props.x = num(v),
            "y" => props.y = num(v),
            "width" => props.width = num(v),
            "height" => props.height = num(v),
            "angle" => props.angle = num(v),
            "strokeColor" => props.stroke_color = s(),
            "backgroundColor" => props.background_color = s(),
            "fillStyle" => props.fill_style = s(),
            "strokeWidth" => props.stroke_width = num(v),
            "strokeStyle" => props.stroke_style = s(),
            "roughness" => props.roughness = num(v),
            "opacity" => props.opacity = num(v),
            "groupIds" => props.group_ids = serde_json::from_value(v.clone()).expect("groupIds"),
            "roundness" => {
                props.roundness =
                    serde_json::from_value::<Option<Roundness>>(v.clone()).expect("roundness")
            }
            "locked" => props.locked = v.as_bool().expect("locked"),
            "type" | "id" | "seed" | "points" | "pressures" | "simulatePressure"
            | "strokeOptions" | "startArrowhead" | "endArrowhead" => {}
            other => panic!("unknown newElement option {other}"),
        }
    }
    props
}

#[test]
fn new_element() {
    check_group(&dir(), "new_element", |case| {
        let opts = &case.args[0];
        let props = props_from(opts);
        let points = || opts.get("points").map(points_from).unwrap_or_default();
        let head = |key| opts.get(key).and_then(Value::as_str).map(str::to_owned);
        let env = &mut FixedEnv;
        let element = match (case.call.as_str(), opts["type"].as_str()) {
            ("newElement", Some("rectangle")) => {
                new_generic_element(GenericKind::Rectangle, props, env)
            }
            ("newElement", Some("diamond")) => {
                new_generic_element(GenericKind::Diamond, props, env)
            }
            ("newElement", Some("ellipse")) => {
                new_generic_element(GenericKind::Ellipse, props, env)
            }
            ("newLinearElement", _) => new_line_element(props, points(), env),
            ("newArrowElement", _) => new_arrow_element(
                props,
                points(),
                head("startArrowhead"),
                head("endArrowhead"),
                env,
            ),
            ("newFreeDrawElement", _) => new_freedraw_element(
                props,
                points(),
                opts.get("pressures")
                    .map(|p| p.as_array().expect("pressures").iter().map(num).collect())
                    .unwrap_or_default(),
                opts["simulatePressure"]
                    .as_bool()
                    .expect("simulatePressure"),
                opts.get("strokeOptions").map(|o| {
                    serde_json::from_value::<StrokeOptions>(o.clone()).expect("strokeOptions")
                }),
                env,
            ),
            other => panic!("unknown constructor {other:?}"),
        };
        let mut value = element.to_value();
        // id and seed are random by design; the generator fixed them in opts.
        value["id"] = opts["id"].clone();
        value["seed"] = opts["seed"].clone();
        value
    });
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --test baseline new_element`
Expected: 編譯失敗，找不到 `scene::env`、`scene::new_element`。

- [ ] **Step 3：建立 `crates/scene/src/env.rs`**

```rust
//! Randomness and time, behind a trait so tests can pin them.

use std::time::{SystemTime, UNIX_EPOCH};

pub trait Env {
    fn fill_random(&mut self, bytes: &mut [u8]);
    /// Milliseconds since the Unix epoch (JS `Date.now()`).
    fn now_ms(&mut self) -> f64;
}

pub struct SystemEnv;

impl Env for SystemEnv {
    fn fill_random(&mut self, bytes: &mut [u8]) {
        getrandom::fill(bytes).expect("OS random source");
    }

    fn now_ms(&mut self) -> f64 {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970");
        since_epoch.as_millis() as f64
    }
}

/// nanoid@3.3.3's `urlAlphabet`.
const URL_ALPHABET: &[u8; 64] = b"useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";

/// `nanoid()`: 21 characters, each a random byte masked to 6 bits.
pub fn random_id(env: &mut impl Env) -> String {
    let mut bytes = [0u8; 21];
    env.fill_random(&mut bytes);
    bytes
        .iter()
        .map(|b| URL_ALPHABET[usize::from(b & 63)] as char)
        .collect()
}

/// Excalidraw's `randomInteger()`: uniform in `[0, 2^31)`.
pub fn random_integer(env: &mut impl Env) -> f64 {
    let mut bytes = [0u8; 4];
    env.fill_random(&mut bytes);
    f64::from(u32::from_le_bytes(bytes) >> 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_have_nanoid_format() {
        let mut env = SystemEnv;
        let id = random_id(&mut env);
        assert_eq!(id.len(), 21);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "{id}"
        );
        assert_ne!(id, random_id(&mut env));
    }

    #[test]
    fn integers_stay_below_two_to_the_31() {
        let mut env = SystemEnv;
        assert!(
            (0..1000)
                .map(|_| random_integer(&mut env))
                .all(|n| (0.0..2_147_483_648.0).contains(&n))
        );
    }
}
```

- [ ] **Step 4：建立 `crates/scene/src/new_element.rs`**

```rust
//! `packages/element/src/newElement.ts` (`_newElementBase` and the per-type constructors)
//! and `bumpVersion` from `mutateElement.ts`. Defaults are the constructors' own, not the
//! editor's current-item settings; the tools in M4 pass those in `ElementProps`.

use serde_json::{Map, Value, json};

use crate::element::{
    Element, ElementBase, FreedrawElement, GenericElement, LinearElement, Roundness, StrokeOptions,
};
use crate::env::{Env, random_id, random_integer};
use crate::json::Slot;

/// `DEFAULT_STROKE_STREAMLINE` (packages/common/src/constants.ts).
pub const DEFAULT_STROKE_STREAMLINE: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct ElementProps {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub angle: f64,
    pub stroke_color: String,
    pub background_color: String,
    pub fill_style: String,
    pub stroke_width: f64,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub group_ids: Vec<String>,
    pub roundness: Option<Roundness>,
    pub locked: bool,
}

impl Default for ElementProps {
    /// `DEFAULT_ELEMENT_PROPS` plus `_newElementBase`'s parameter defaults.
    fn default() -> Self {
        ElementProps {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            angle: 0.0,
            stroke_color: "#1e1e1e".into(),
            background_color: "transparent".into(),
            fill_style: "solid".into(),
            stroke_width: 2.0,
            stroke_style: "solid".into(),
            roughness: 1.0,
            opacity: 100.0,
            group_ids: Vec::new(),
            roundness: None,
            locked: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenericKind {
    Rectangle,
    Diamond,
    Ellipse,
}

/// `_newElementBase`: the shared fields, plus the untyped ones for `extra`.
fn new_base(
    kind: &str,
    props: ElementProps,
    env: &mut impl Env,
) -> (ElementBase, Map<String, Value>) {
    let timestamp = env.now_ms();
    let base = ElementBase {
        id: random_id(env),
        kind: kind.into(),
        x: props.x,
        y: props.y,
        width: props.width,
        height: props.height,
        angle: props.angle,
        stroke_color: props.stroke_color,
        background_color: props.background_color,
        fill_style: props.fill_style,
        stroke_width: props.stroke_width,
        stroke_style: props.stroke_style,
        roughness: props.roughness,
        opacity: props.opacity,
        group_ids: props.group_ids,
        index: Slot::Null,
        roundness: props.roundness.map_or(Slot::Null, Slot::Value),
        seed: random_integer(env),
        version: 1.0,
        version_nonce: 0.0,
        is_deleted: false,
        updated: Slot::Value(timestamp),
    };
    let mut extra = Map::new();
    extra.insert("frameId".into(), Value::Null);
    extra.insert("boundElements".into(), Value::Null);
    extra.insert("created".into(), json!(timestamp));
    extra.insert("link".into(), Value::Null);
    extra.insert("locked".into(), json!(props.locked));
    (base, extra)
}

pub fn new_generic_element(kind: GenericKind, props: ElementProps, env: &mut impl Env) -> Element {
    let name = match kind {
        GenericKind::Rectangle => "rectangle",
        GenericKind::Diamond => "diamond",
        GenericKind::Ellipse => "ellipse",
    };
    let (base, extra) = new_base(name, props, env);
    let element = GenericElement { base, extra };
    match kind {
        GenericKind::Rectangle => Element::Rectangle(element),
        GenericKind::Diamond => Element::Diamond(element),
        GenericKind::Ellipse => Element::Ellipse(element),
    }
}

/// `newLinearElement` for `type: "line"`. Width and height come from `props`, as in
/// Excalidraw (the constructor does not derive them from `points`).
pub fn new_line_element(props: ElementProps, points: Vec<[f64; 2]>, env: &mut impl Env) -> Element {
    let (base, mut extra) = new_base("line", props, env);
    extra.insert("startBinding".into(), Value::Null);
    extra.insert("endBinding".into(), Value::Null);
    extra.insert("polygon".into(), json!(false));
    Element::Line(LinearElement {
        base,
        points,
        start_arrowhead: Slot::Null,
        end_arrowhead: Slot::Null,
        elbowed: None,
        extra,
    })
}

/// `newArrowElement` without `elbowed` (napkin cannot create elbow arrows, spec §1.2).
pub fn new_arrow_element(
    props: ElementProps,
    points: Vec<[f64; 2]>,
    start_arrowhead: Option<String>,
    end_arrowhead: Option<String>,
    env: &mut impl Env,
) -> Element {
    let (base, mut extra) = new_base("arrow", props, env);
    extra.insert("startBinding".into(), Value::Null);
    extra.insert("endBinding".into(), Value::Null);
    Element::Arrow(LinearElement {
        base,
        points,
        start_arrowhead: start_arrowhead.map_or(Slot::Null, Slot::Value),
        end_arrowhead: end_arrowhead.map_or(Slot::Null, Slot::Value),
        elbowed: Some(false),
        extra,
    })
}

pub fn new_freedraw_element(
    props: ElementProps,
    points: Vec<[f64; 2]>,
    pressures: Vec<f64>,
    simulate_pressure: bool,
    stroke_options: Option<StrokeOptions>,
    env: &mut impl Env,
) -> Element {
    let (base, extra) = new_base("freedraw", props, env);
    let stroke_options = stroke_options.unwrap_or_else(|| StrokeOptions {
        variability: Slot::Value("variable".into()),
        streamline: Slot::Value(DEFAULT_STROKE_STREAMLINE),
        extra: Map::new(),
    });
    Element::Freedraw(FreedrawElement {
        base,
        points,
        pressures,
        simulate_pressure: Some(simulate_pressure),
        stroke_options: Slot::Value(stroke_options),
        extra,
    })
}

/// `bumpVersion`: every modification increments `version`, redraws `versionNonce` and
/// stamps `updated` (spec §5.3).
pub fn bump_version(element: &mut Element, env: &mut impl Env) {
    let nonce = random_integer(env);
    let now = env.now_ms();
    match element {
        Element::Raw(Value::Object(map)) => {
            let version = map.get("version").and_then(Value::as_f64).unwrap_or(0.0);
            map.insert("version".into(), json!(version + 1.0));
            map.insert("versionNonce".into(), json!(nonce));
            map.insert("updated".into(), json!(now));
        }
        Element::Raw(_) => {}
        _ => {
            let base = element.base_mut().expect("typed element");
            base.version += 1.0;
            base.version_nonce = nonce;
            base.updated = Slot::Value(now);
        }
    }
}
```

`lib.rs` 加上 `pub mod env;`、`pub mod new_element;`（照字母順序排在 `element` 與 `file` 附近）。

- [ ] **Step 5：跑測試**

Run: `cargo test -p scene`
Expected: 單元測試 13 passed；`new_element` 1 passed（8 個案例）。

- [ ] **Step 6：突變檢查**

把 `new_base` 的 `version_nonce: 0.0` 暫時改成 `1.0`，跑 `cargo test -p scene --test baseline new_element`。
Expected: `new_element: 8 of 8 cases failed`，訊息指出 `$.versionNonce`。改回 `0.0`。

- [ ] **Step 7：格式、lint、commit**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`

```bash
git add crates/scene
git commit -m "Add Excalidraw element constructors and version bumping"
```

---

### Task 6：fractional index

**Files:**
- Create: `crates/scene/src/fractional_index.rs`
- Modify: `crates/scene/src/lib.rs`（加 `pub mod fractional_index;`）
- Modify: `crates/scene/tests/baseline.rs`

**Interfaces:**
- Consumes: `Element::{id, index, set_index}`、`new_element::bump_version`、`env::Env`、`rough::js::math_round`。
- Produces:
  - `scene::fractional_index::IndexError(pub String)`，`Display` 印出內含的 JS 錯誤訊息；實作 `std::error::Error`。
  - `validate_order_key(key: &str) -> Result<(), IndexError>`
  - `generate_key_between(a: Option<&str>, b: Option<&str>) -> Result<String, IndexError>`
  - `generate_n_keys_between(a: Option<&str>, b: Option<&str>, n: usize) -> Result<Vec<String>, IndexError>`
  - `sync_moved_indices(elements: &mut [Element], moved: &HashSet<String>, env: &mut impl Env)`（`moved` 是元件 id）
  - `sync_invalid_indices(elements: &mut [Element], env: &mut impl Env)`

- [ ] **Step 1：在 `baseline.rs` 加測試**

開頭的 `use` 區換成：

```rust
use std::collections::HashSet;
use std::path::PathBuf;

use scene::element::{Element, Roundness, StrokeOptions};
use scene::env::Env;
use scene::fractional_index::{
    generate_key_between, generate_n_keys_between, sync_invalid_indices, sync_moved_indices,
};
use scene::new_element::{
    ElementProps, GenericKind, new_arrow_element, new_freedraw_element, new_generic_element,
    new_line_element,
};
use serde_json::{Value, json};
use testkit::{Case, check_group, num, points_from};
```

檔案最後加：

```rust
fn throws(error: impl std::fmt::Display) -> Value {
    json!({ "throws": error.to_string() })
}

fn key_arg(case: &Case, i: usize) -> Option<&str> {
    case.args[i].as_str()
}

/// Elements shaped like the generator's `{ id, type, index, version, versionNonce, updated }`.
/// They lack most fields, so they load as `Raw`, which is also the path images and frames take.
fn index_elements(indices: &Value) -> Vec<Element> {
    indices
        .as_array()
        .expect("indices")
        .iter()
        .enumerate()
        .map(|(i, index)| {
            Element::from_value(json!({
                "id": format!("e{i}"), "type": "rectangle", "index": index,
                "version": 1, "versionNonce": 0, "updated": 1,
            }))
        })
        .collect()
}

fn index_summary(elements: &[Element]) -> Value {
    Value::Array(
        elements
            .iter()
            .map(|e| {
                let v = e.to_value();
                json!({ "id": v["id"], "index": v["index"], "version": v["version"] })
            })
            .collect(),
    )
}

#[test]
fn fractional_index() {
    check_group(&dir(), "fractional_index", |case| {
        match case.call.as_str() {
            "generateKeyBetween" => {
                match generate_key_between(key_arg(case, 0), key_arg(case, 1)) {
                    Ok(key) => json!(key),
                    Err(e) => throws(e),
                }
            }
            "generateNKeysBetween" => {
                match generate_n_keys_between(
                    key_arg(case, 0),
                    key_arg(case, 1),
                    case.num(2) as usize,
                ) {
                    Ok(keys) => json!(keys),
                    Err(e) => throws(e),
                }
            }
            "syncMovedIndices" => {
                let mut elements = index_elements(&case.args[0]);
                let moved: HashSet<String> = case.args[1]
                    .as_array()
                    .expect("moved positions")
                    .iter()
                    .map(|i| format!("e{}", num(i)))
                    .collect();
                sync_moved_indices(&mut elements, &moved, &mut FixedEnv);
                index_summary(&elements)
            }
            "syncInvalidIndices" => {
                let mut elements = index_elements(&case.args[0]);
                sync_invalid_indices(&mut elements, &mut FixedEnv);
                index_summary(&elements)
            }
            other => panic!("unknown call {other}"),
        }
    });
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --test baseline fractional_index`
Expected: 編譯失敗，找不到 `scene::fractional_index`。

- [ ] **Step 3：port**

來源：`packages/fractional-indexing/src/index.ts`（全檔，`digits` 參數固定為 `BASE_62_DIGITS`，不必保留成參數）；`packages/element/src/fractionalIndex.ts` 的 `syncMovedIndices`、`syncInvalidIndices`、`getMovedIndicesGroups`、`getInvalidIndicesGroups`、`isValidFractionalIndex`、`generateIndices`，以及 `validateFractionalIndices` 在 `shouldThrow: true, includeBoundTextValidation: false, ignoreLogs: true` 時的行為。不 port `orderByFractionalIndex`、`syncInvalidIndicesImmutable`。

port 時注意：

- 錯誤訊息逐字照抄：`${a} >= ${b}`、`trailing zero`、`invalid integer part of order key: ${int}`、`invalid order key head: ${head}`、`invalid order key: ${key}`、`cannot decrement any more`、`cannot increment any more`。空字串的 `key[0]` 是 `undefined`，訊息印出 `invalid order key head: undefined`。
- 字串比較照對照規則用 UTF-16 順序。`a[n] || zero` 在索引超出長度時用 `zero`。
- `Math.round(0.5 * (digitA + digitB))` 用 `rough::js::math_round`。
- `syncMovedIndices` 的 `try/catch`：`generateIndices` 或驗證任何一步失敗，就改做 `syncInvalidIndices`。先算出所有新 index、驗證通過後才寫入元件。
- `mutateElement(element, elementsMap, { index })` 只在新值與舊值不同時才改：`set_index` 後呼叫 `bump_version`。寫入順序照 `elementsUpdates` 這個 `Map` 的插入順序。
- `isValidFractionalIndex` 與 bounds 的判斷用 JS 真假值：缺席、`null`、空字串都是假。元件的 index 用 `Element::index()` 讀。

- [ ] **Step 4：跑測試、格式、lint、commit**

Run: `cargo test -p scene && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `fractional_index` passed（48 個案例）。

```bash
git add crates/scene
git commit -m "Port Excalidraw fractional indexing and index sync"
```

---

### Task 7：色彩解析與深色模式

**Files:**
- Create: `crates/scene/src/color.rs`
- Modify: `crates/scene/src/lib.rs`（加 `pub mod color;`）
- Modify: `crates/scene/tests/baseline.rs`

**Interfaces:**
- Consumes: `rough::js::math_round`、`regex`。
- Produces:
  - `scene::color::TinyColor { pub r: f64, pub g: f64, pub b: f64, pub a: f64, pub ok: bool }`、`tinycolor(input: &str) -> TinyColor`（對應 tinycolor2 建構後的 `_r`、`_g`、`_b`、`_a`、`_ok`）
  - `apply_dark_mode_filter(color: &str) -> String`
  - `is_transparent(color: &str) -> bool`

- [ ] **Step 1：在 `baseline.rs` 加測試**

`use` 區加一行 `use scene::color::{apply_dark_mode_filter, is_transparent};`（`cargo fmt` 會排好順序），檔案最後加：

```rust
#[test]
fn colors() {
    check_group(&dir(), "colors", |case| {
        let color = case.args[0].as_str().expect("color");
        match case.call.as_str() {
            "applyDarkModeFilter" => json!(apply_dark_mode_filter(color)),
            "isTransparent" => json!(is_transparent(color)),
            other => panic!("unknown call {other}"),
        }
    });
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --test baseline colors`
Expected: 編譯失敗。

- [ ] **Step 3：port**

來源：`tools/baseline/node_modules/tinycolor2/esm/tinycolor.js` 的字串輸入路徑：建構函數 `tinycolor`、`inputToRGB`、`rgbToRgb`、`hslToRgb`（含 `hue2rgb`）、`hsvToRgb`、`boundAlpha`、`bound01`、`clamp01`、`isOnePointZero`、`isPercentage`、`convertToPercentage`、`parseIntFromHex`、`convertHexToDecimal`、`matchers`、`isValidCSSUnit`、`stringInputToObject`、`names`。Excalidraw 端：`packages/common/src/colors.ts` 的 `cssHueRotate`、`cssInvert`、`applyDarkModeFilter`、`rgbToHex`、`isTransparent`；`packages/math/src/angle.ts` 的 `degreesToRadians`；`@excalidraw/math` 的 `clamp`。不 port 快取 `DARK_MODE_COLORS_CACHE`。

port 時注意：

- `matchers` 的正規表示式用 `regex` crate 照字面翻譯，但字元類別改成與 JS 相同的集合：`\d` 寫成 `[0-9]`；`\s` 寫成 `[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]`（`regex` 的 `\s` 是 Unicode White_Space，含 `\u{85}`、不含 `\u{FEFF}`，與 JS 不同）。`[\s|\(]` 與 `[,|\s]` 裡的 `|` 是字面的直線字元，照留。`rgb`、`rgba`、`hsl` 等 matcher 沒有錨點，用 `captures`（找字串中任意位置）；hex 的四個 matcher 有 `^…$`。`regex` 的交替是最左優先，與 JS 對這些不含回溯參照的樣式行為相同。
- `stringInputToObject` 先用 JS `\s` 去頭尾空白再 `toLowerCase()`；比對順序是 names、`transparent`、rgb、rgba、hsl、hsla、hsv、hsva、hex8、hex6、hex4、hex3。`names` 是一般 JS 物件，`"constructor"` 這類繼承屬性雖然為真，最後也不會比對成功，結果同樣是無效色彩，Rust 用一般的查表即可。
- rgb 等 matcher 取到的是字串，`bound01` 對字串做 `parseFloat`、`isPercentage` 看 `%`、`isOnePointZero` 看是否含 `.` 且值為 1。`parseFloat` 解析字串開頭最長的合法數字（`"50%"` 得 50），自己寫一個對應函數，不要用 `str::parse`。`bound01` 裡的 `parseInt(n * max, 10)` 對 `[0, 65025]` 範圍內的數等於取整數部分。
- `inputToRGB` 最後把 r、g、b 夾在 `[0, 255]`；建構函數對小於 1 的分量做 `Math.round`。
- `toRgb()` 的 r、g、b 用 `Math.round`，a 保持原值。
- `rgbToHex(r, g, b, a)`：6 位小寫十六進位，`a < 1` 時再接 `Math.round(a * 255)` 的 2 位十六進位。

- [ ] **Step 4：跑測試、格式、lint、commit**

Run: `cargo test -p scene && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `colors` passed（74 個案例）。

```bash
git add crates/scene
git commit -m "Port tinycolor parsing and Excalidraw dark mode color filter"
```

---

### Task 8：rough options

**Files:**
- Create: `crates/scene/src/shape/mod.rs`
- Create: `crates/scene/src/shape/options.rs`
- Modify: `crates/scene/src/lib.rs`（加 `pub mod shape;`）
- Modify: `crates/scene/tests/baseline.rs`

**Interfaces:**
- Consumes: `color::{apply_dark_mode_filter, is_transparent}`、元件結構、`rough::Options`。
- Produces:
  - `scene::shape::ShapeContext<'a> { pub dark_mode: bool, pub canvas_background_color: &'a str }`
  - `scene::shape::PathOp`：`Move([f64; 2])`、`Quad([f64; 4])`、`Line([f64; 2])`、`Close`（derive `Clone, Debug, PartialEq`）
  - `scene::shape::ElementShape`：`None`、`Drawables(Vec<rough::Drawable>)`、`Freedraw { fill: Option<Box<rough::Drawable>>, stroke: Vec<PathOp> }`（derive `Clone, Debug, PartialEq`）
  - `scene::shape::generate_rough_options(element: &Element, continuous_path: bool, dark_mode: bool) -> Option<rough::Options>`：rectangle、diamond、ellipse、line、arrow、freedraw 以外回傳 `None`。
  - `pub(crate) fn shape::options::is_path_a_loop(points: &[[f64; 2]]) -> bool`、`pub(crate) fn shape::options::dark(color: &str, dark_mode: bool) -> String`（`applyDarkModeFilter(color, enable)`），供 Task 9 到 12 使用。

- [ ] **Step 1：在 `baseline.rs` 加測試**

`use` 區加兩行 `use scene::shape::generate_rough_options;`、`use testkit::rough_json::options_value;`，檔案最後加：

```rust
/// Element JSON from a case, which must load as a typed element when its type is one scene
/// draws: a silent fallback to `Raw` would make every shape comparison vacuous.
fn element_from(value: &Value) -> Element {
    let element = Element::from_value(value.clone());
    let drawn = [
        "rectangle",
        "diamond",
        "ellipse",
        "line",
        "arrow",
        "freedraw",
        "text",
    ];
    if value["type"].as_str().is_some_and(|t| drawn.contains(&t)) {
        assert!(
            !matches!(element, Element::Raw(_)),
            "baseline element fell back to Raw"
        );
    }
    element
}

#[test]
fn rough_options() {
    check_group(&dir(), "rough_options", |case| {
        let element = element_from(&case.args[0]);
        let continuous = case.args[1].as_bool().expect("continuousPath");
        let dark = case.args[2].as_bool().expect("isDarkMode");
        options_value(&generate_rough_options(&element, continuous, dark).expect("drawable type"))
    });
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --test baseline rough_options`
Expected: 編譯失敗。

- [ ] **Step 3：建立型別並 port**

`shape/mod.rs` 放 Interfaces 列出的三個公開型別與 `pub use options::generate_rough_options;`，`mod options;`。

來源：`packages/element/src/shape.ts` 的 `getDashArrayDashed`、`getDashArrayDotted`、`adjustRoughness`、`generateRoughOptions`；`packages/element/src/utils.ts` 的 `isPathALoop`（`zoomValue` 固定為 1，`LINE_CONFIRM_THRESHOLD` 是 8）；`packages/element/src/comparisons.ts` 的 `canChangeRoundness`；`packages/element/src/typeChecks.ts` 的 `isLinearElement`；`ROUGHNESS.cartoonist` 是 2。

port 時注意：

- 回傳的 `rough::Options` 只設定 JS 物件上有值的 key：`strokeLineDash` 在 solid 時是 `undefined`（`None`）；rectangle／diamond／ellipse 的 `fill` 在 `isTransparent(backgroundColor)` 時是 `undefined`；line／freedraw 只在 `isPathALoop(points)` 時才設 `fillStyle` 與 `fill`，而且判斷透明用的是 `backgroundColor === "transparent"` 字串比較，不是 `isTransparent`；ellipse 另設 `curveFitting = 1`；arrow 兩者都不設。
- `adjustRoughness` 的 `!!element.roundness` 見對照規則；`maxSize < 10 ? 3 : 2`。
- `stroke` 與 `fill` 經過 `dark(color, dark_mode)`：`dark_mode` 為假時原樣回傳。
- `preserveVertices: continuousPath || element.roughness < 2`。

- [ ] **Step 4：跑測試、格式、lint、commit**

Run: `cargo test -p scene && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `rough_options` passed（422 個案例）。

```bash
git add crates/scene
git commit -m "Port Excalidraw generateRoughOptions"
```

---

### Task 9：rectangle、diamond、ellipse 的形狀

**Files:**
- Create: `crates/scene/src/shape/generic.rs`
- Modify: `crates/scene/src/shape/mod.rs`
- Modify: `crates/scene/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 8 的型別與 `generate_rough_options`、`rough::RoughGenerator`。
- Produces: `pub fn scene::shape::generate_element_shape(element: &Element, ctx: &ShapeContext) -> ElementShape`。本任務處理 rectangle、diamond、ellipse（回傳單一 drawable 的 `Drawables`），text 與 `Raw` 回傳 `ElementShape::None`；line、arrow 分支寫 `todo!()`（Task 10），freedraw 分支寫 `todo!()`（Task 12）。

- [ ] **Step 1：在 `baseline.rs` 加測試**

`use` 區裡 `scene::shape`、`testkit::rough_json`、`testkit` 這三行換成：

```rust
use scene::shape::{
    ElementShape, PathOp, ShapeContext, generate_element_shape, generate_rough_options,
};
use testkit::rough_json::{drawable_value, options_value};
use testkit::{Case, check_group, num, points_from, to_value};
```

檔案最後加：

```rust
fn path_op_value(op: &PathOp) -> Value {
    let (name, data): (&str, &[f64]) = match op {
        PathOp::Move(d) => ("move", d),
        PathOp::Line(d) => ("line", d),
        PathOp::Quad(d) => ("quad", d),
        PathOp::Close => ("close", &[]),
    };
    json!({ "op": name, "data": data.iter().copied().map(to_value).collect::<Vec<_>>() })
}

/// The JSON Excalidraw's ShapeCache returns for each element type.
fn shape_value(element: &Element, shape: &ElementShape) -> Value {
    match shape {
        ElementShape::None => Value::Null,
        ElementShape::Drawables(drawables) => match element {
            Element::Rectangle(_) | Element::Diamond(_) | Element::Ellipse(_) => {
                assert_eq!(drawables.len(), 1, "generic elements have one drawable");
                drawable_value(&drawables[0])
            }
            _ => Value::Array(drawables.iter().map(drawable_value).collect()),
        },
        ElementShape::Freedraw { fill, stroke } => {
            let mut items: Vec<Value> = fill.iter().map(|d| drawable_value(d)).collect();
            items.push(json!({ "svgPath": stroke.iter().map(path_op_value).collect::<Vec<_>>() }));
            Value::Array(items)
        }
    }
}

fn check_shapes(group: &str) {
    check_group(&dir(), group, |case| {
        let element = element_from(&case.args[0]);
        let context = &case.args[1];
        let ctx = ShapeContext {
            dark_mode: context["theme"] == "dark",
            canvas_background_color: context["canvasBackgroundColor"].as_str().expect("color"),
        };
        shape_value(&element, &generate_element_shape(&element, &ctx))
    });
}

#[test]
fn shapes_generic() {
    check_shapes("shapes_generic");
}

#[test]
fn shapes_other() {
    check_shapes("shapes_other");
}
```

- [ ] **Step 2：確認失敗**

Run: `cargo test -p scene --test baseline shapes_`
Expected: 編譯失敗，找不到 `generate_element_shape`。

- [ ] **Step 3：port**

來源：`packages/element/src/shape.ts` 的 `_generateElementShape` 中 `rectangle`、`diamond`、`ellipse` 三個分支，以及 `stickynote`／`frame`／`magicframe`／`text`／`image` 回傳 `null` 的分支；`packages/element/src/utils.ts` 的 `getCornerRadius`（`DEFAULT_PROPORTIONAL_RADIUS` 0.25、`DEFAULT_ADAPTIVE_RADIUS` 32、`ROUNDNESS` 的 1、2、3）；`packages/element/src/bounds.ts` 的 `getDiamondPoints`。`modifyIframeLikeForRoughOptions` 只影響 iframe 與 embeddable，napkin 把它們當 `Raw`，不 port。

port 時注意：

- rectangle 有 roundness 時產生 path 字串再交給 `RoughGenerator::path`，options 用 `continuousPath = true`；沒有 roundness 時是 `rectangle(0, 0, width, height, options)`，`continuousPath = false`。diamond 同理：有 roundness 用含 `C` 指令的 path，沒有則 `polygon` 四個頂點。ellipse 是 `ellipse(width / 2, height / 2, width, height, options)`。
- path 字串照模板的 token 順序組出來，數字用 `format!("{n}")`（見對照規則）。模板裡的換行與縮排不影響解析結果。有限的元件幾何不會讓 path parser 失敗，`generator.path(...)` 的錯誤用 `expect` 並在訊息寫明這個前提。
- `getCornerRadius` 的 `roundness?.value ?? DEFAULT_ADAPTIVE_RADIUS` 用 `Slot::value()`。

- [ ] **Step 4：跑測試、格式、lint、commit**

Run: `cargo test -p scene && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `shapes_generic` passed（315 個案例）、`shapes_other` passed（9 個案例）。

```bash
git add crates/scene
git commit -m "Port Excalidraw shapes for rectangles, diamonds and ellipses"
```

---

### Task 10：line 與 arrow 的形狀

**Files:**
- Create: `crates/scene/src/shape/linear.rs`
- Create: `crates/scene/src/shape/arrowhead.rs`
- Modify: `crates/scene/src/shape/mod.rs`
- Modify: `crates/scene/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 8、9 的函數；`rough::{RoughGenerator, Drawable, Op, Options}`。
- Produces: `generate_element_shape` 的 line 與 arrow 分支，回傳 `ElementShape::Drawables`：第一個是線本身，接著是起點箭頭頭部、終點箭頭頭部的 drawable。

- [ ] **Step 1：加測試並確認失敗**

```rust
#[test]
fn shapes_linear() {
    check_shapes("shapes_linear");
}
```

Run: `cargo test -p scene --test baseline shapes_linear`
Expected: 243 個案例全部失敗（`not yet implemented`）。

- [ ] **Step 2：port**

來源：`packages/element/src/shape.ts` 的 `line`／`arrow` 分支、`generateElbowArrowShape`、`getArrowheadShapes`、`getArrowheadLineOptions`、`generateArrowheadCardinalityOne`、`generateArrowheadLinesToTip`、`generateArrowheadOutlineCircle`；`packages/element/src/bounds.ts` 的 `getArrowheadSize`、`getArrowheadAngle`、`getArrowheadPoints`；`packages/utils/src/shape.ts` 的 `getCurvePathOps`；`packages/element/src/heading.ts` 的 `vectorToHeading`、`headingForPoint`、`headingForPointIsHorizontal`；`packages/math/src/point.ts` 的 `pointRotateRads`（`if (!angle) return point`）、`pointDistance`（`Math.hypot`）；`packages/math/src/vector.ts` 的 `vectorFromPoint`。

port 時注意：

- `points` 為空時用 `[[0, 0]]` 產生線，但 `getArrowheadPoints` 計算長度時讀的是元件原本的 `points`（線沒有 op 時會先回傳 `null`，不會讀到空陣列）。
- elbow arrow：任何點的 x 或 y 絕對值大於 1e6 時，線本身是空的 drawable 清單（之後箭頭頭部也因沒有 op 而不畫）。否則 `path(generateElbowArrowShape(points, 16), options(continuousPath = true))`。
- 非 elbow：沒有 roundness 時，`options.fill` 為真用 `polygon`，否則 `linearPath`；有 roundness 用 `curve`。
- 箭頭頭部預設值：`startArrowhead` 缺席或 `null` 都是 `null`；`endArrowhead` 缺席是 `"arrow"`、`null` 是 `null`。
- `getArrowheadShapes` 的每個分支都從 `options` 複製一份再改：`delete strokeLineDash` 設成 `None`；`roughness: Math.min(1, options.roughness || 0)`（circle 是 0.5）。`strokeColor` 與 `backgroundFillColor` 分別是 `dark(element.strokeColor)`、`dark(ctx.canvas_background_color)`。未知的箭頭名稱（包括舊名稱 `dot`）走 `default` 分支，跟 `arrow`、`bar` 相同。
- `getArrowheadPoints` 的 `invariant(data.length === 6)`：op 不是 `BCurveTo` 時回傳 `None`，不畫這個頭部（決定 7）。

- [ ] **Step 3：跑測試、格式、lint、commit**

Run: `cargo test -p scene && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `shapes_linear` passed。

```bash
git add crates/scene
git commit -m "Port Excalidraw shapes for lines, arrows and arrowheads"
```

---

### Task 11：freedraw 外框（perfect-freehand 與 laser-pointer）

**Files:**
- Create: `crates/scene/src/perfect_freehand.rs`
- Create: `crates/scene/src/laser_pointer.rs`
- Create: `crates/scene/src/shape/freedraw.rs`
- Modify: `crates/scene/src/lib.rs`（加 `mod laser_pointer;`、`mod perfect_freehand;`）
- Modify: `crates/scene/src/shape/mod.rs`
- Modify: `crates/scene/tests/baseline.rs`

**Interfaces:**
- Consumes: `FreedrawElement`。
- Produces: `pub fn scene::shape::freedraw_outline_points(element: &FreedrawElement) -> Vec<[f64; 2]>`（`getFreedrawOutlinePoints`）。

- [ ] **Step 1：在 `baseline.rs` 加測試並確認失敗**

`use` 區裡 `scene::shape` 與 `testkit` 這兩行換成：

```rust
use scene::shape::{
    ElementShape, PathOp, ShapeContext, freedraw_outline_points, generate_element_shape,
    generate_rough_options,
};
use testkit::{Case, check_group, num, point_value, points_from, to_value};
```

檔案最後加：

```rust
#[test]
fn freedraw_outline() {
    check_group(&dir(), "freedraw_outline", |case| {
        let Element::Freedraw(element) = element_from(&case.args[0]) else {
            panic!("not a freedraw element");
        };
        Value::Array(
            freedraw_outline_points(&element)
                .into_iter()
                .map(point_value)
                .collect(),
        )
    });
}
```

Run: `cargo test -p scene --test baseline freedraw_outline`
Expected: 編譯失敗。

- [ ] **Step 2：port**

來源：
- `packages/element/src/shape.ts` 的 `getFreedrawOutlinePoints`、`getVariableWidthFreedrawOutline`、`getConstantWidthFreedrawOutline`、`createLaserPointer`、`getFreedrawStreamline`、`VARIABLE_WIDTH_FREEDRAW`、`CONSTANT_WIDTH_FREEDRAW`。
- perfect-freehand 1.2.0 的 TypeScript 原始碼（Global Constraints 的 tag）：`getStroke.ts`、`getStrokePoints.ts`、`getStrokeOutlinePoints.ts`、`getStrokeRadius.ts`、`vec.ts`。基準實際執行的是 `node_modules/perfect-freehand/dist/esm/index.js`（minify 過），兩者不一致時以基準為準。
- `packages/laser-pointer/src/state.ts`、`math.ts`（`simplify.ts` 在 Excalidraw 的 `simplify: 0` 下走不到，不 port）。

port 時注意：

- 分流：`strokeOptions?.variability === "constant"` 用 laser-pointer，其他情況（包括 `strokeOptions` 缺席）用 perfect-freehand。`streamline` 是 `strokeOptions?.streamline ?? 0.5`。
- variable 的輸入點：`simulatePressure` 為真時直接用 `points`（沒有壓力）；否則點數不為 0 時是 `[x, y, pressures[i]]`，為 0 時是 `[[0, 0, 0.5]]`。沒有壓力或壓力陣列比點數短時，用 NaN 表示 JS 的 `undefined`：`pts[i][2] >= 0` 對兩者都是假。`simulatePressure` 缺席時選輸入點的判斷是假，但傳給 `getStroke` 的 `simulatePressure` 選項因為解構預設值而是 `true`。
- perfect-freehand 只需要 Excalidraw 傳入的選項組合：`size = strokeWidth * 4.25`、`thinning 0.6`、`smoothing 0.5`、`easing = sin(t * π / 2)`、`last: true`、`start`／`end` 沒傳。在這組選項下 `taperStart`、`taperEnd` 都是 `undefined`，與 taper 有關的分支、flat cap 分支都走不到，不 port；程式碼註明這個前提。
- laser-pointer：`size = strokeWidth * 1.4`、`simplify 0`、`simplifyPhase` 預設 `"output"`、`keepHead` 預設 false、`sizeMapping = max(0.1, pressure)`。每個點以 `[x, y, 1]` 加入；Excalidraw 不呼叫 `close()`，外框由 `stablePoints` 接 `tailPoints` 組成。`angle` 內的 `atan2`、各個 `for (theta = ...; theta <= ...; theta += Math.PI / 16)` 迴圈照浮點累加。
- 兩個模組都是 crate 私有，只從 `shape::freedraw` 呼叫。

- [ ] **Step 3：跑測試、格式、lint、commit**

Run: `cargo test -p scene && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: `freedraw_outline` passed（25 個案例）。

```bash
git add crates/scene
git commit -m "Port perfect-freehand and laser-pointer freedraw outlines"
```

---

### Task 12：freedraw 的形狀與收尾

**Files:**
- Modify: `crates/scene/src/shape/freedraw.rs`
- Modify: `crates/scene/src/shape/mod.rs`
- Modify: `crates/scene/tests/baseline.rs`

**Interfaces:**
- Consumes: Task 11 的外框、Task 8 的 `generate_rough_options` 與 `is_path_a_loop`、`rough::points_on_curve::simplify`。
- Produces: `generate_element_shape` 的 freedraw 分支回傳 `ElementShape::Freedraw { fill, stroke }`；`pub(crate) fn shape::freedraw::truncate_path_number(x: f64) -> f64`。crate 內沒有 `todo!()`。

- [ ] **Step 1：加測試並確認失敗**

```rust
#[test]
fn shapes_freedraw() {
    check_shapes("shapes_freedraw");
}
```

Run: `cargo test -p scene --test baseline shapes_freedraw`
Expected: 75 個案例全部失敗。

- [ ] **Step 2：加入 `truncate_path_number` 與它的測試**

在 `shape/freedraw.rs` 加：

```rust
/// One coordinate as it comes out of `getSvgPathFromStroke`: JS prints the number, then the
/// TO_FIXED_PRECISION regex keeps at most two decimals and deletes the digits, `e` and `-`
/// that follow. Truncation, not rounding; and a mantissa with a decimal point loses its
/// exponent, so 1.4999e-7 becomes 1.49. Excalidraw draws that path, so napkin must too.
/// Assumes |x| < 1e21, where JS switches to exponent form with a "+" the regex keeps.
pub(crate) fn truncate_path_number(x: f64) -> f64 {
    // JS prints |x| < 1e-6 in exponent form; `{:e}` and `{}` give the same shortest digits.
    let text = if x != 0.0 && x.abs() < 1e-6 {
        format!("{x:e}")
    } else {
        format!("{x}")
    };
    match text.split_once('.') {
        None => x,
        Some((int_part, frac)) => {
            let kept: String = frac
                .chars()
                .take_while(char::is_ascii_digit)
                .take(2)
                .collect();
            format!("{int_part}.{kept}")
                .parse()
                .expect("decimal literal")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_like_the_svg_path_regex() {
        // Expected values from node: the TO_FIXED_PRECISION regex applied to `${x}`.
        for (x, expected) in [
            (1.4999999997655777e-7, 1.49),
            (1.2e-7, 1.2),
            (1e-7, 1e-7),
            (-1.2e-7, -1.2),
            (-2.6789, -2.67),
            (0.29, 0.29),
            (2.675, 2.67),
            (123.0, 123.0),
            (0.000001, 0.0),
            (-0.000001, 0.0),
            (12.5, 12.5),
            (0.30000000000000004, 0.3),
            (5e-324, 5e-324),
            (-0.0000015, 0.0),
        ] {
            assert_eq!(truncate_path_number(x), expected, "{x:e}");
        }
    }
}
```

- [ ] **Step 3：port freedraw 分支**

來源：`packages/element/src/shape.ts` 的 `freedraw` 分支、`getFreeDrawSvgPath`、`getSvgPathFromStroke`、`med`。

port 時注意：

- 填充：`isPathALoop(points)` 時，`simplify(points, 0.75)` 之後呼叫 `curve(simplified, { ...generateRoughOptions(element, false, dark), stroke: "none" })`，放進 `fill`；否則 `fill` 是 `None`。
- 外框 path：外框點為空時 `stroke` 是空 `Vec`。否則依 `getSvgPathFromStroke` 的 `reduce` 展開成 `Move(p0)`、每個 `i < max` 一個 `Quad(p_i, med(p_i, p_{i+1}))`、最後一個 `Quad(p_max, med(p_max, p_0))`、`Line(p0)`、`Close`。`med` 用未截斷的座標計算，所有輸出座標再各自經過 `truncate_path_number`。
- Excalidraw 在繪製時才對 freedraw 的線條色套深色模式（`renderElement.ts`），這不屬於形狀資料，留給 M3。

- [ ] **Step 4：整體驗證**

Run:

```bash
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
grep -rn 'todo!\|unimplemented!' crates/scene/src && echo "stubs left" || echo "no stubs"
cargo tree -p scene -e normal --depth 1
```

Expected:
- `cargo test`：`scene` 的 `baseline` 9 passed、`corpus` 2 passed、單元測試 14 passed；M1 的測試全部 passed。
- `no stubs`。
- `cargo tree` 的直接依賴只有 `getrandom`、`regex`、`rough`、`serde`、`serde_json`。

- [ ] **Step 5：Commit**

```bash
git add crates/scene
git commit -m "Port Excalidraw freedraw shapes and SVG path truncation"
```

M2 完成條件（roadmap）：round-trip 語料與形狀基準全部通過。

## 留給後續里程碑

- 繪製時才套用的規則（freedraw 線條色的深色模式、`opacity` 與所屬 frame 相乘、淺色模式下 CSS 色彩字串的解析）、`Raw` 元件畫虛線框所需的 `x`、`y`、`width`、`height`、`angle` 存取函數：M3。
- `Raw` 元件搬移時只改 `x`／`y`、`groupIds` 的讀取：M4。
- 文字量測與新文字元件、編輯器的目前設定（包括 freedraw 預設 `"constant"`）、原子寫入與 debounce：M4。
- 綁定與 `boundElements` 的型別化：M5。
