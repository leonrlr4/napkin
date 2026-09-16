# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 這是什麼

napkin 是常駐的原生 Rust 手繪白板，存檔格式就是 `.excalidraw`，對同一個 seed 要畫出和 excalidraw.com 一樣的線條。Cargo workspace，依賴方向只有 `app → scene → rough`（`app` 還沒建立）：

- `crates/rough`：roughjs@4.6.4 逐行 port。零依賴，不知道 Excalidraw 的存在。
- `crates/scene`：`.excalidraw` 讀寫、新元件預設值、fractional index、深色模式色彩、Excalidraw 的形狀規則（元件 → rough ops 或 freedraw 外框，純資料）。不依賴 egui，不渲染。
- `crates/testkit`：只給測試用，讀 JSON 基準並比對。

程式碼、註解、commit message、PR 描述用英文；`docs/decisions/` 底下的文件用中文。這條 repo 慣例壓過全域「個人專案用中文」的規則。

程式碼註解裡的「spec §N」指 `docs/decisions/specs/2026-09-13-napkin-design.md`。那是凍結的決策紀錄，只拿來解讀引用，不當現況看。

## 指令

```
cargo test --workspace
cargo test -p rough --test baseline fill_hachure   # 單一基準群組：測試名 = 群組名 = tests/baseline/<名>.json
cargo test -p scene --test corpus                  # excalidraw.com 語料 round-trip
cargo clippy --workspace --all-targets && cargo fmt --check
```

重新產生基準，只在改了 `tools/baseline/rough/cases.mjs` 或 `tools/baseline/scene/cases.mjs` 之後做，產出的 JSON 要 commit：

```
cd tools/baseline && npm ci && npm run rough && npm run scene
```

`npm run scene` 第一次會把 Excalidraw 釘選的 commit 淺層 clone 到 `tools/baseline/.cache/`，需要網路。沒改 cases 就重新產生，`git diff` 必須是空的；有差異代表 node_modules 或 node 版本漂了，不是 Rust 這邊有 bug。

字型：`tools/fonts/build_fonts.py`（uv script，需要網路）重建 `assets/fonts/`。它的測試不在 cargo 裡：

```
uv run --with 'fonttools[woff]==4.65.0' --with pytest pytest tools/fonts
```

## 正確性怎麼定義

`rough` 和 `scene` 沒有自己發明的期望值。基準 JSON 由 node 跑真正的 JavaScript 產生：rough 直接跑 npm 的 roughjs 4.6.4，scene 用 esbuild 打包釘選 commit 的 Excalidraw 原始碼（`tools/baseline/lib/excalidraw.mjs`）。Rust 這邊一個測試對應一個群組，透過 `testkit::check_group` 逐個數字比對，絕對誤差 1e-9。例外：

- 呼叫過 `Math.random` 的案例只比結構，不比數字（`compare: "structure"`）。產生器會把結構隨亂數變動的案例直接拒絕，這種案例不能進基準。
- `js_math` 群組（`rough::js::atan2`／`hypot`）要 bit 完全相同，因為 scene 裡有迴圈次數取決於這兩個函數最後一個 bit 的結果。
- JSON 放不下 NaN 和 ±Infinity，基準裡寫成字串 `"NaN"`、`"Infinity"`、`"-Infinity"`，testkit 兩邊轉換。

port 的時候看的是 JS 原始碼（rough：npm 套件的 `bin/*.js`；scene：`.cache/` 裡的 checkout），不是 baseline JSON。

## 改壞了也不會有錯誤訊息的地方

- **釘選版本要三處一致**：Excalidraw commit `afa3a653fc5d2b742adcbd5a6063187b056d2419` 寫在 `tools/baseline/lib/excalidraw.mjs`、`tools/fonts/build_fonts.py` 和 `crates/scene/src/lib.rs` 的 doc comment。roughjs 4.6.4 和它的依賴版本由 generate.mjs 的 `assertVersions` 鎖住，來源是 Excalidraw 在該 commit 的 yarn.lock，不是各套件 package.json 的範圍。
- **JS 數值語意一律走 `rough::js`**：`math_round`（`Math.round` 的 .5 往正無限大）、`to_int32`（`Math.imul` 與位元運算）、`truthy`、`to_fixed`、`atan2`、`hypot`。Rust 的 `f64::round`、`as i32`、`f64::min/max` 在邊界值上跟 JS 不同，基準抓得到，但只在基準恰好涵蓋那個邊界時。
- **元件只在能無損寫回時才用型別化結構**：`Element::from_value` 把 JSON 解析成 struct 後再序列化一次，語意不相等就整個退回 `Element::Raw`。所以 schema 寫錯的結果是元件變成佔位框，不是檔案被改寫。新增欄位時，`Slot<T>` 區分「沒有」「null」「有值」，`Option<T>` 不區分前兩者；Excalidraw 舊檔沒有的 key 新檔會寫 `null`，選錯會讓元件在 corpus 測試裡退回 Raw。`crates/scene/tests/corpus.rs` 檢查七種型別化元件在語料裡都不會退回 Raw。
- **亂數與時間走 `scene::env::Env`**，測試用固定實作釘住；不要在 scene 裡直接呼叫 `getrandom` 或 `SystemTime`。
